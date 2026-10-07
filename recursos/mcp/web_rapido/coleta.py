"""Coleta de páginas: robots.txt, limite por domínio, tamanho máximo e
bloqueio de endereços internos.

- Só http e https. Cada salto de redirecionamento passa pelas mesmas
  checagens (no máximo 5 saltos).
- Endereços que não são públicos (loopback, rede privada, link-local como
  o 169.254.169.254 do metadata da nuvem, CGNAT...) são recusados, a não
  ser com WEB_PERMITIR_REDE_LOCAL: o nome é resolvido ANTES e o endereço
  conectado é conferido DEPOIS (pega DNS que muda no meio).
- As variáveis HTTP(S)_PROXY são ignoradas (a conexão é sempre direta,
  para a checagem do endereço valer).
- robots.txt (RFC 9309, ver robots.py) fica no cache: 24 h; erro do
  servidor ou de rede conta como "proibido" por 1 h.
- Um pedido por domínio a cada WEB_INTERVALO_DOMINIO_SEGUNDOS (ou o
  Crawl-delay do site, se maior, até 60 s).
- No máximo WEB_MAX_PAGINA_MB de conteúdo (já descomprimido): acima disso,
  a leitura para e o que veio é usado, com aviso.
"""

from __future__ import annotations

import ipaddress
import socket
from dataclasses import dataclass
from urllib.parse import urljoin, urlsplit, urlunsplit

import anyio
import httpx

import robots
from cache import Cache
from config import MIB, Config

MAX_SALTOS = 5
MAX_ROBOTS_BYTES = 512 * 1024
MAX_CRAWL_DELAY = 60.0
TTL_ROBOTS = 24 * 3600
TTL_ROBOTS_ERRO = 3600
TIPOS_TEXTO = ("text/html", "application/xhtml+xml", "text/plain", "text/markdown")
REDIRECIONAMENTOS = (301, 302, 303, 307, 308)


class ErroColeta(Exception):
    """Falha esperada (site fora do ar, robots.txt, endereço interno...):
    a mensagem vai para o modelo."""


@dataclass
class Download:
    url: str
    status: int
    tipo: str
    charset: str | None
    conteudo: bytes
    cortado: bool


def normalizar_url(url: str) -> str:
    """URL absoluta http(s), sem o fragmento (#...)."""
    url = (url or "").strip()
    try:
        partes = urlsplit(url)
    except ValueError:
        raise ErroColeta(f"URL inválida: {url!r}") from None
    if partes.scheme.lower() not in ("http", "https"):
        raise ErroColeta(f"só http e https (veio {partes.scheme or 'nenhum esquema'!r})")
    if not partes.hostname:
        raise ErroColeta(f"URL sem domínio: {url!r}")
    if partes.username or partes.password:
        raise ErroColeta("URL com usuário/senha não é aceita")
    try:
        _ = partes.port
    except ValueError:
        raise ErroColeta(f"porta inválida em {url!r}") from None
    return urlunsplit((partes.scheme.lower(), partes.netloc.lower(), partes.path or "/", partes.query, ""))


def endereco_publico(ip: str) -> bool:
    try:
        endereco = ipaddress.ip_address(ip.split("%")[0])
    except ValueError:
        return False
    if isinstance(endereco, ipaddress.IPv6Address) and endereco.ipv4_mapped:
        endereco = endereco.ipv4_mapped
    return endereco.is_global and not endereco.is_multicast


class LimitePorDominio:
    """Reserva horários: o próximo pedido a um domínio só sai depois do intervalo."""

    def __init__(self):
        self._proximo: dict[str, float] = {}

    async def esperar(self, dominio: str, intervalo: float, prazo: float) -> None:
        agora = anyio.current_time()
        livre = max(agora, self._proximo.get(dominio, 0.0))
        if livre > prazo:
            raise ErroColeta(
                f"limite por domínio: {dominio} só aceita outro pedido daqui a {livre - agora:.0f} s "
                "(intervalo entre pedidos ou Crawl-delay do site); tente de novo depois"
            )
        self._proximo[dominio] = livre + intervalo
        await anyio.sleep(livre - agora)


class Coletor:
    def __init__(self, cfg: Config, cache: Cache):
        self.cfg = cfg
        self.cache = cache
        self.limite = LimitePorDominio()
        self.cliente = httpx.AsyncClient(
            follow_redirects=False,
            trust_env=False,
            # O padrão do httpx (5 s) cortaria antes do WEB_TIMEOUT_REQUISICAO.
            timeout=cfg.timeout_requisicao,
            headers={
                "User-Agent": cfg.agente,
                "Accept": "text/html,application/xhtml+xml,text/plain;q=0.9,*/*;q=0.1",
                "Accept-Language": "pt-BR,pt;q=0.9,en;q=0.8",
            },
        )

    async def fechar(self) -> None:
        await self.cliente.aclose()

    # -- checagens -----------------------------------------------------------

    async def _conferir_destino(self, url: str) -> None:
        """Antes de conectar: todo endereço do nome tem de ser público."""
        if self.cfg.permitir_rede_local:
            return
        partes = urlsplit(url)
        porta = partes.port or (443 if partes.scheme == "https" else 80)
        try:
            infos = await anyio.getaddrinfo(partes.hostname, porta, type=socket.SOCK_STREAM)
        except OSError as e:
            raise ErroColeta(f"não consegui resolver {partes.hostname}: {e}") from None
        internos = sorted({str(info[4][0]) for info in infos if not endereco_publico(str(info[4][0]))})
        if internos:
            raise ErroColeta(
                f"{partes.hostname} aponta para endereço interno ({', '.join(internos)}): bloqueado "
                "(proteção contra acesso à rede local e ao metadata da nuvem)"
            )

    def _conferir_conexao(self, resposta: httpx.Response) -> None:
        """Depois de conectar: o endereço de verdade também tem de ser público."""
        if self.cfg.permitir_rede_local:
            return
        fluxo = resposta.extensions.get("network_stream")
        par = fluxo.get_extra_info("server_addr") if fluxo is not None else None
        if not par or not endereco_publico(str(par[0])):
            raise ErroColeta(f"a conexão foi para um endereço interno ({par and par[0]}): bloqueado")

    # -- robots.txt ----------------------------------------------------------

    async def _robots(self, url: str, prazo: float) -> robots.Robots:
        partes = urlsplit(url)
        origem = f"{partes.scheme}://{partes.netloc}"
        guardado = self.cache.ler("robots", origem)
        if guardado is not None:
            valor, _ = guardado
            return self._interpretar_robots(valor["status"], valor["texto"])
        status, texto = await self._baixar_robots(origem + "/robots.txt", prazo)
        if status == 0 and anyio.current_time() >= prazo - 0.5:
            # Foi o NOSSO prazo que acabou, não o site que falhou: não guarda.
            raise ErroColeta("o prazo da chamada acabou enquanto lia o robots.txt")
        ttl = TTL_ROBOTS_ERRO if status == 0 or status >= 500 else TTL_ROBOTS
        self.cache.gravar("robots", origem, {"status": status, "texto": texto}, ttl)
        return self._interpretar_robots(status, texto)

    @staticmethod
    def _interpretar_robots(status: int, texto: str) -> robots.Robots:
        if 200 <= status < 300:
            return robots.interpretar(texto)
        if status in (401, 403) or status == 0 or status >= 500:
            # Acesso negado, servidor com erro ou fora do ar: na dúvida, não lê.
            return robots.Robots.proibe_tudo()
        return robots.Robots.permite_tudo()  # 404 e afins: o site não tem regras

    async def _baixar_robots(self, url: str, prazo: float) -> tuple[int, str]:
        """(status, texto); status 0 = falha de rede."""
        try:
            for _ in range(MAX_SALTOS + 1):
                await self._conferir_destino(url)
                with anyio.fail_after(self._tempo(prazo)):
                    async with self.cliente.stream("GET", url) as resposta:
                        self._conferir_conexao(resposta)
                        if resposta.status_code in REDIRECIONAMENTOS and "location" in resposta.headers:
                            url = normalizar_url(urljoin(url, resposta.headers["location"]))
                            continue
                        corpo = await self._ler_corpo(resposta, MAX_ROBOTS_BYTES)
                        return resposta.status_code, corpo[0].decode("utf-8", errors="replace")
            return 0, ""
        except (httpx.HTTPError, TimeoutError, ErroColeta):
            return 0, ""

    # -- página --------------------------------------------------------------

    def _tempo(self, prazo: float) -> float:
        restante = prazo - anyio.current_time()
        if restante <= 0:
            raise ErroColeta("o prazo da chamada acabou")
        return min(self.cfg.timeout_requisicao, restante)

    @staticmethod
    async def _ler_corpo(resposta: httpx.Response, maximo: int) -> tuple[bytes, bool]:
        partes, total = [], 0
        async for pedaco in resposta.aiter_bytes():
            partes.append(pedaco)
            total += len(pedaco)
            if total > maximo:
                return b"".join(partes)[:maximo], True
        return b"".join(partes), False

    async def baixar(self, url: str, prazo: float) -> Download:
        maximo = self.cfg.max_pagina_mb * MIB
        for _ in range(MAX_SALTOS + 1):
            await self._conferir_destino(url)
            regras = await self._robots(url, prazo)
            if not regras.permite(self.cfg.produto, url):
                raise ErroColeta(
                    f"bloqueado pelo robots.txt de {urlsplit(url).netloc}: o site pede que robôs não leiam "
                    "esta página. Não insista por outro caminho; se precisar mesmo, peça ao dono."
                )
            atraso = regras.atraso(self.cfg.produto) or 0.0
            intervalo = max(self.cfg.intervalo_dominio, min(atraso, MAX_CRAWL_DELAY))
            await self.limite.esperar(urlsplit(url).hostname or "", intervalo, prazo)
            try:
                with anyio.fail_after(self._tempo(prazo)):
                    async with self.cliente.stream("GET", url) as resposta:
                        self._conferir_conexao(resposta)
                        if resposta.status_code in REDIRECIONAMENTOS and "location" in resposta.headers:
                            url = normalizar_url(urljoin(url, resposta.headers["location"]))
                            continue
                        if resposta.status_code >= 400:
                            raise ErroColeta(f"o site respondeu {resposta.status_code} para {url}")
                        tipo = resposta.headers.get("content-type", "text/html").split(";")[0].strip().lower()
                        if tipo not in TIPOS_TEXTO:
                            raise ErroColeta(
                                f"tipo de conteúdo não suportado: {tipo} (o web_rapido lê HTML e texto; "
                                "para PDF e afins, delegue ou use outra ferramenta)"
                            )
                        conteudo, cortado = await self._ler_corpo(resposta, maximo)
                        return Download(
                            url=str(resposta.url),
                            status=resposta.status_code,
                            tipo=tipo,
                            charset=resposta.charset_encoding,
                            conteudo=conteudo,
                            cortado=cortado,
                        )
            except TimeoutError:
                raise ErroColeta(f"{urlsplit(url).netloc} não respondeu a tempo") from None
            except httpx.HTTPError as e:
                raise ErroColeta(f"falha ao buscar {url}: {type(e).__name__}: {e}") from None
        raise ErroColeta(f"redirecionamentos demais (mais de {MAX_SALTOS})")
