"""Rede do navegador: as mesmas regras do web_rapido, em duas camadas.

1. **Interceptação** (``context.route`` no Playwright, ver sessoes.py): todo
   pedido que uma página faz (navegação, script, imagem, fetch, XHR, form)
   passa por ``Rede.conferir_pedido`` ANTES de sair. Só http e https; o
   nome é resolvido e TODOS os endereços têm de ser públicos (endereço
   literal é conferido antes do DNS). Aqui também ficam as regras de método
   (``politica``): envio de formulário e POST para outra origem.
2. **Filtro** (um proxy HTTP local, ``Filtro``): o Chromium sobe com ele
   como proxy obrigatório (o Playwright tira a exceção implícita do
   loopback com ``<-loopback>``). Cada conexão é conferida de novo: o nome
   é resolvido, a conexão vai para um endereço já conferido (nada de DNS que
   muda no meio) e o endereço de fato conectado é conferido DEPOIS. Pega o
   que a interceptação não vê: cada salto de redirecionamento, WebSocket,
   pedidos de service worker (bloqueados também no contexto) e o próprio
   navegador.

Endereços que não são públicos: loopback, redes privadas, link-local (o
169.254.169.254 do metadata da nuvem), CGNAT (100.64/10), multicast, IPv6
local... A única exceção é NAVEGADOR_EXCECOES_REDE_LOCAL (ip:porta exatos,
só para testes).
"""

from __future__ import annotations

import asyncio
import contextlib
import ipaddress
import logging
import socket
import time
from collections import deque
from dataclasses import dataclass
from urllib.parse import urlsplit

log = logging.getLogger("navegador")

ESQUEMAS_REDE = ("http", "https")
# Esquemas que não saem para a rede (a página monta o conteúdo sozinha).
ESQUEMAS_LOCAIS = ("data", "blob", "about")
METODOS_SEGUROS = ("GET", "HEAD", "OPTIONS")
TTL_DNS = 30.0
MAX_CABECALHO = 64 * 1024
TIMEOUT_CABECALHO = 30.0
TIMEOUT_CONEXAO = 15.0
CABECALHOS_DO_PROXY = ("proxy-connection", "proxy-authorization", "connection", "keep-alive")


class Bloqueio(Exception):
    """Pedido recusado pelas regras de rede: a mensagem vai para o modelo."""


def endereco_publico(ip: str) -> bool:
    try:
        endereco = ipaddress.ip_address(ip.split("%")[0])
    except ValueError:
        return False
    if isinstance(endereco, ipaddress.IPv6Address) and endereco.ipv4_mapped:
        endereco = endereco.ipv4_mapped
    return endereco.is_global and not endereco.is_multicast


def validar_url(url: str) -> str:
    """URL que o modelo pediu: absoluta, http(s), com domínio, sem usuário e
    senha. Devolve a URL sem espaços em volta."""
    url = (url or "").strip()
    try:
        partes = urlsplit(url)
        _ = partes.port
    except ValueError:
        raise Bloqueio(f"URL inválida: {url!r}") from None
    if partes.scheme.lower() not in ESQUEMAS_REDE:
        raise Bloqueio(f"só http e https (veio {partes.scheme or 'nenhum esquema'!r}); file:, ftp: e afins são recusados")
    if not partes.hostname:
        raise Bloqueio(f"URL sem domínio: {url!r}")
    if partes.username or partes.password:
        raise Bloqueio("URL com usuário/senha não é aceita")
    return url


def origem(url: str) -> str | None:
    """esquema://host:porta de uma URL http(s); None para o resto
    (about:blank, data:...)."""
    try:
        partes = urlsplit(url)
        porta = partes.port
    except ValueError:
        return None
    esquema = partes.scheme.lower()
    if esquema not in ESQUEMAS_REDE or not partes.hostname:
        return None
    porta = porta or (443 if esquema == "https" else 80)
    return f"{esquema}://{partes.hostname.lower()}:{porta}"


def politica(metodo: str, url: str, url_da_pagina: str, navegacao: bool, interagir: bool) -> str | None:
    """Motivo para recusar o pedido pelo método, ou None.

    - GET/HEAD/OPTIONS: sempre (ler é o propósito);
    - outro método numa navegação (envio de formulário): só com interagir e
      só para a MESMA origem da página;
    - outro método fora de navegação (fetch/XHR da própria página): só para
      a mesma origem da página, nos dois modos (sites montados por
      JavaScript carregam conteúdo assim). Para outra origem, nunca.
    """
    metodo = metodo.upper()
    if metodo in METODOS_SEGUROS:
        return None
    destino, pagina = origem(url), origem(url_da_pagina)
    if navegacao and not interagir:
        return (
            f"envio de formulário ({metodo}) recusado: o navegador está no modo só leitura "
            "([navegador] interagir = false no abiyss.toml)"
        )
    if pagina is None or destino != pagina:
        return (
            f"{metodo} para outra origem ({destino or url[:80]}; a página é {pagina or url_da_pagina[:80]}) "
            "recusado: o navegador nunca manda dados de uma página para outro site"
        )
    return None


@dataclass
class RegistroBloqueio:
    quando: float
    onde: str
    motivo: str


class Rede:
    """As regras de endereço e o cache de DNS (usado pela interceptação)."""

    def __init__(self, excecoes: frozenset[tuple[str, int]] = frozenset()):
        self.excecoes = excecoes
        self._dns: dict[tuple[str, int], tuple[float, list[str]]] = {}

    def permitido(self, ip: str, porta: int) -> bool:
        if endereco_publico(ip):
            return True
        try:
            normal = str(ipaddress.ip_address(ip.split("%")[0]))
        except ValueError:
            return False
        return (normal, porta) in self.excecoes

    async def resolver(self, host: str, porta: int) -> list[str]:
        """Endereços do nome, TODOS conferidos; recusa se algum for interno."""
        host = host.strip("[]").lower()
        try:
            ipaddress.ip_address(host.split("%")[0])
            literal = True
        except ValueError:
            literal = False
        if literal:
            # Antes do DNS: um endereço literal interno nem é resolvido.
            if not self.permitido(host, porta):
                raise Bloqueio(
                    f"{host} é endereço interno: bloqueado (proteção contra acesso à rede local e ao "
                    "metadata da nuvem)"
                )
            return [host]
        if host == "localhost" or host.endswith(".localhost"):
            raise Bloqueio(f"{host} é a própria máquina: bloqueado")
        chave = (host, porta)
        guardado = self._dns.get(chave)
        if guardado and guardado[0] > time.monotonic():
            ips = guardado[1]
        else:
            try:
                infos = await asyncio.get_running_loop().getaddrinfo(host, porta, type=socket.SOCK_STREAM)
            except OSError as e:
                raise Bloqueio(f"não consegui resolver {host}: {e}") from None
            ips = list(dict.fromkeys(str(info[4][0]) for info in infos))
            if len(self._dns) > 1000:
                self._dns.clear()
            self._dns[chave] = (time.monotonic() + TTL_DNS, ips)
        internos = sorted(ip for ip in ips if not self.permitido(ip, porta))
        if internos or not ips:
            raise Bloqueio(
                f"{host} aponta para endereço interno ({', '.join(internos) or 'nenhum'}): bloqueado "
                "(proteção contra acesso à rede local e ao metadata da nuvem)"
            )
        return ips

    async def conferir_pedido(self, url: str) -> None:
        """Interceptação: esquema e endereço de um pedido da página."""
        partes = urlsplit(url)
        esquema = partes.scheme.lower()
        if esquema in ESQUEMAS_LOCAIS:
            return
        if esquema not in ESQUEMAS_REDE:
            raise Bloqueio(f"esquema {esquema or '(nenhum)'}: só http e https")
        if partes.username or partes.password:
            raise Bloqueio("URL com usuário/senha não é aceita")
        try:
            porta = partes.port or (443 if esquema == "https" else 80)
        except ValueError:
            raise Bloqueio(f"porta inválida em {url[:200]}") from None
        if not partes.hostname:
            raise Bloqueio("URL sem domínio")
        await self.resolver(partes.hostname, porta)


class Filtro:
    """Proxy HTTP local (só 127.0.0.1) que o Chromium é obrigado a usar.

    CONNECT host:porta (https, wss) e pedidos em forma absoluta (http, ws).
    Cada destino é resolvido e conferido; a conexão vai para um endereço já
    conferido e o par conectado é conferido de novo. Recusa = 403.
    """

    def __init__(self, rede: Rede, nome: str = "navegador"):
        self.rede = rede
        self.nome = nome
        self.porta = 0
        self.bloqueios: deque[RegistroBloqueio] = deque(maxlen=20)
        self.total_bloqueados = 0
        self._servidor: asyncio.base_events.Server | None = None
        self._abertas: set[asyncio.StreamWriter] = set()

    @property
    def endereco(self) -> str:
        return f"http://127.0.0.1:{self.porta}"

    async def iniciar(self) -> None:
        self._servidor = await asyncio.start_server(self._atender, "127.0.0.1", 0, limit=MAX_CABECALHO)
        self.porta = self._servidor.sockets[0].getsockname()[1]

    async def fechar(self) -> None:
        if self._servidor is None:
            return
        self._servidor.close()
        for escritor in list(self._abertas):
            escritor.close()
        with contextlib.suppress(Exception):
            await asyncio.wait_for(self._servidor.wait_closed(), 2)
        self._servidor = None

    def registrar(self, onde: str, motivo: str) -> None:
        self.total_bloqueados += 1
        self.bloqueios.append(RegistroBloqueio(time.time(), onde[:200], motivo))
        log.info("%s: bloqueado %s: %s", self.nome, onde[:200], motivo)

    async def _conectar(self, host: str, porta: int) -> tuple[asyncio.StreamReader, asyncio.StreamWriter]:
        ips = await self.rede.resolver(host, porta)
        ultimo: Exception | None = None
        for ip in ips:
            try:
                leitor, escritor = await asyncio.wait_for(asyncio.open_connection(ip, porta), TIMEOUT_CONEXAO)
            except (OSError, asyncio.TimeoutError) as e:
                ultimo = e
                continue
            par = escritor.get_extra_info("peername")
            if not par or not self.rede.permitido(str(par[0]), int(par[1])):
                escritor.close()
                raise Bloqueio(f"a conexão foi para um endereço interno ({par and par[0]}): bloqueado")
            return leitor, escritor
        raise ConnectionError(f"não consegui conectar em {host}:{porta}: {ultimo}")

    async def _atender(self, cliente_r: asyncio.StreamReader, cliente_w: asyncio.StreamWriter) -> None:
        self._abertas.add(cliente_w)
        remoto_w: asyncio.StreamWriter | None = None
        onde = "?"
        try:
            try:
                cabecalho = await asyncio.wait_for(cliente_r.readuntil(b"\r\n\r\n"), TIMEOUT_CABECALHO)
            except (asyncio.IncompleteReadError, asyncio.LimitOverrunError, asyncio.TimeoutError, ConnectionError):
                return
            linhas = cabecalho.decode("latin-1").split("\r\n")
            partes = linhas[0].split(" ")
            if len(partes) != 3:
                return await self._responder(cliente_w, 400, "pedido inválido")
            metodo, alvo, versao = partes
            if metodo.upper() == "CONNECT":
                host, _, porta_texto = alvo.rpartition(":")
                onde = alvo
                try:
                    porta = int(porta_texto)
                except ValueError:
                    return await self._responder(cliente_w, 400, "porta inválida")
                remoto_r, remoto_w = await self._conectar(host, porta)
                cliente_w.write(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                await cliente_w.drain()
            else:
                url = urlsplit(alvo)
                if url.scheme.lower() not in ("http", "ws") or not url.hostname:
                    return await self._responder(cliente_w, 400, "só pedidos http em forma absoluta")
                porta = url.port or 80
                onde = f"{url.hostname}:{porta}"
                remoto_r, remoto_w = await self._conectar(url.hostname, porta)
                caminho = (url.path or "/") + (f"?{url.query}" if url.query else "")
                cabecalhos = [l for l in linhas[1:] if l and l.split(":", 1)[0].strip().lower() not in CABECALHOS_DO_PROXY]
                atualizacao = any(l.split(":", 1)[0].strip().lower() == "upgrade" for l in cabecalhos)
                # Uma conexão por pedido: nada de reaproveitar o túnel para outro destino.
                fim = ["Connection: Upgrade"] if atualizacao else ["Connection: close"]
                remoto_w.write("\r\n".join([f"{metodo} {caminho} {versao}", *cabecalhos, *fim, "", ""]).encode("latin-1"))
                await remoto_w.drain()
            await self._encanar(cliente_r, cliente_w, remoto_r, remoto_w)
        except Bloqueio as e:
            self.registrar(onde, str(e))
            await self._responder(cliente_w, 403, f"bloqueado pelo navegador do Abiyss: {e}")
        except (ConnectionError, OSError) as e:
            await self._responder(cliente_w, 502, f"falha ao conectar: {e}")
        finally:
            self._abertas.discard(cliente_w)
            for escritor in (cliente_w, remoto_w):
                if escritor is not None:
                    escritor.close()

    @staticmethod
    async def _responder(escritor: asyncio.StreamWriter, status: int, texto: str) -> None:
        corpo = texto.encode("utf-8")
        frase = {400: "Bad Request", 403: "Forbidden", 502: "Bad Gateway"}.get(status, "Error")
        with contextlib.suppress(Exception):
            escritor.write(
                f"HTTP/1.1 {status} {frase}\r\nContent-Type: text/plain; charset=utf-8\r\n"
                f"Content-Length: {len(corpo)}\r\nConnection: close\r\n\r\n".encode() + corpo
            )
            await escritor.drain()

    @staticmethod
    async def _encanar(a_r, a_w, b_r, b_w) -> None:
        async def copiar(leitor: asyncio.StreamReader, escritor: asyncio.StreamWriter) -> None:
            try:
                while dados := await leitor.read(65536):
                    escritor.write(dados)
                    await escritor.drain()
                if escritor.can_write_eof():
                    escritor.write_eof()
            except (ConnectionError, OSError):
                pass

        ida = asyncio.ensure_future(copiar(a_r, b_w))
        volta = asyncio.ensure_future(copiar(b_r, a_w))
        try:
            feitas, _ = await asyncio.wait({ida, volta}, return_when=asyncio.FIRST_COMPLETED)
            # O destino terminou: nada mais do navegador interessa. O navegador
            # terminou de mandar: ainda falta a resposta.
            if volta not in feitas:
                await volta
        finally:
            ida.cancel()
            volta.cancel()
