"""Entrega no Discord o que o kernel manda, sem abusar da API.

O lado Discord de verdade fica em ``discordio`` (discord.py). Aqui só há a
lógica, que os testes exercitam com um Discord de mentira:

- ``Ritmo``: no máximo uma operação no Discord (enviar, editar) a cada
  ``intervalo`` segundos (1,2 s), para todas as entregas juntas;
- ``Transmissao``: a resposta da conversa chegando aos poucos. Uma
  mensagem "pensando" (``.`` → ``..`` → ``...``) que vira a resposta,
  editada no ritmo; passou de 2000 caracteres, continua em mensagens
  novas (``partes.dividir``, sem quebrar blocos de código). Se uma edição
  falhar, para de editar e, no fim, manda a resposta inteira como
  mensagem(ns) nova(s) (e tenta apagar as parciais);
- ``Entregador``: recebe os pedidos do kernel (pela ``Ponte``) e devolve
  os IDs das mensagens criadas.

Destino (defesa em dobro: o kernel já só manda para esses lugares):
- ``canal_id``: só um dos canais permitidos que o kernel informou no ``ola``;
- ``dm_para``: DM só ao dono, a uma pessoa conhecida do ``ola`` ou a quem
  mandou DM ao bot há pouco (``JANELA_DM_RECENTE``; a resposta fixa a
  desconhecidos). O adaptador nunca puxa conversa com qualquer um;
- os dois nulos: a DM do dono.

Arquivo: só de dentro do workspace que o kernel informou no ``ola``,
conferido de novo aqui (``anexo_seguro``): caminho real, sem link
simbólico, arquivo comum, até ``MAX_BYTES_ANEXO``.
"""

from __future__ import annotations

import asyncio
import logging
import os
import stat
import time
from collections.abc import Awaitable, Callable
from pathlib import Path
from typing import Any, Protocol

from partes import dividir

log = logging.getLogger("abiyss.gateway.saida")

INTERVALO_PADRAO = 1.2
# Quem mandou DM ao bot há menos que isto pode receber DM de volta.
JANELA_DM_RECENTE = 3600.0
# Teto do adaptador para anexos (o do kernel, [gateway] max_bytes_anexo,
# costuma ser menor; o Discord recusa acima de ~10 MB sem impulso).
MAX_BYTES_ANEXO = 10 * 1024 * 1024
PENSANDO = (".", "..", "...")


class MensagemSaida(Protocol):
    id: int

    async def editar(self, texto: str) -> None: ...

    async def apagar(self) -> None: ...


class CanalSaida(Protocol):
    async def enviar(self, texto: str, responder_a: str | None, arquivo: Path | None = None) -> MensagemSaida: ...


class Mensageiro(Protocol):
    async def canal(self, canal_id: str | None, dm_para: str | None = None) -> CanalSaida:
        """Um canal, a DM de ``dm_para``, ou (os dois ``None``) a DM do dono."""
        ...


class ErroDestino(Exception):
    pass


class ErroAnexo(Exception):
    pass


def anexo_seguro(caminho: str, workspace: str | None, max_bytes: int = MAX_BYTES_ANEXO) -> Path:
    """O arquivo, se ele estiver mesmo dentro do workspace. O kernel manda
    o caminho REAL: qualquer diferença depois de resolver (link simbólico,
    ``..``) é recusa."""
    if not workspace:
        raise ErroAnexo("o kernel não informou o workspace")
    raiz = Path(workspace).resolve(strict=True)
    pedido = Path(caminho)
    if not pedido.is_absolute():
        raise ErroAnexo("caminho de anexo precisa ser absoluto")
    try:
        real = pedido.resolve(strict=True)
    except OSError:
        raise ErroAnexo(f"anexo não existe: {caminho}") from None
    if real != pedido or not real.is_relative_to(raiz):
        raise ErroAnexo(f"anexo fora do workspace: {caminho}")
    info = os.stat(real, follow_symlinks=False)
    if not stat.S_ISREG(info.st_mode):
        raise ErroAnexo(f"anexo não é um arquivo comum: {caminho}")
    if info.st_size > max_bytes:
        raise ErroAnexo(f"anexo grande demais ({info.st_size} bytes)")
    return real


class Ritmo:
    """No máximo uma operação a cada ``intervalo`` segundos."""

    def __init__(
        self,
        intervalo: float = INTERVALO_PADRAO,
        relogio: Callable[[], float] = time.monotonic,
        dormir: Callable[[float], Awaitable[None]] = asyncio.sleep,
    ) -> None:
        self.intervalo = intervalo
        self.relogio = relogio
        self._relogio = relogio
        self._dormir = dormir
        self._ultima: float | None = None
        # Transmissões e entregas dividem o mesmo ritmo.
        self._trava = asyncio.Lock()

    async def esperar_um_pouco(self) -> None:
        """Nada a fazer agora: espera um pedaço do intervalo."""
        await self._dormir(self.intervalo / 4)

    async def vez(self) -> None:
        """Espera a vez da próxima operação (e marca o instante dela)."""
        async with self._trava:
            if self._ultima is not None:
                falta = self._ultima + self.intervalo - self._relogio()
                if falta > 0:
                    await self._dormir(falta)
            self._ultima = self._relogio()


class Transmissao:
    """Uma resposta sendo escrita: uma mensagem editada no ritmo."""

    def __init__(
        self,
        canal: CanalSaida,
        responder_a: str | None,
        ritmo: Ritmo,
        relogio: Callable[[], float] = time.monotonic,
    ) -> None:
        self.canal = canal
        self.responder_a = responder_a
        self.ritmo = ritmo
        self._relogio = relogio
        self.texto = ""
        self.mensagens: list[MensagemSaida] = []
        self.mostrado: list[str] = []
        self.edicao_falhou = False
        self._inicio = relogio()
        self._tarefa: asyncio.Task[None] | None = None

    async def comecar(self) -> None:
        """Manda o "pensando" e começa a acompanhar o texto."""
        try:
            await self.ritmo.vez()
            m = await self.canal.enviar(PENSANDO[0], self.responder_a)
            self.mensagens, self.mostrado = [m], [PENSANDO[0]]
        except Exception as e:  # noqa: BLE001 — sem a mensagem, o fim manda tudo de uma vez
            log.warning("não consegui mandar o \"pensando\": %s", e)
            self.edicao_falhou = True
            return
        self._tarefa = asyncio.create_task(self._acompanhar())

    def atualizar(self, texto: str) -> None:
        """O texto inteiro até agora (vazio = pensando). Só guarda: quem
        edita é o acompanhamento, no ritmo."""
        self.texto = texto

    def _alvo(self) -> list[str]:
        if not self.texto.strip():
            quadro = int((self._relogio() - self._inicio) / self.ritmo.intervalo) % len(PENSANDO)
            return [PENSANDO[quadro]]
        return dividir(self.texto)

    async def _acompanhar(self) -> None:
        while not self.edicao_falhou:
            alvo = self._alvo()
            if alvo != self.mostrado[: len(alvo)]:
                try:
                    await self._mostrar(alvo, apagar_sobras=False)
                except Exception as e:  # noqa: BLE001 — para de editar; o fim manda tudo de novo
                    log.warning("edição recusada pelo Discord (%s): a resposta vai inteira no fim", e)
                    self.edicao_falhou = True
            else:
                await self.ritmo.esperar_um_pouco()

    async def _mostrar(self, alvo: list[str], apagar_sobras: bool) -> None:
        """Deixa as mensagens com os pedaços de ``alvo`` (uma operação por vez, no ritmo)."""
        for i, parte in enumerate(alvo):
            if i < len(self.mensagens):
                if self.mostrado[i] != parte:
                    await self.ritmo.vez()
                    await self.mensagens[i].editar(parte)
                    self.mostrado[i] = parte
            else:
                await self.ritmo.vez()
                self.mensagens.append(await self.canal.enviar(parte, None))
                self.mostrado.append(parte)
        if apagar_sobras:
            while len(self.mensagens) > len(alvo):
                sobra = self.mensagens.pop()
                self.mostrado.pop()
                await self.ritmo.vez()
                await sobra.apagar()

    async def terminar(self, texto: str) -> list[str]:
        """O texto final. Devolve os IDs das mensagens que ficam."""
        self.texto = texto
        if self._tarefa is not None:
            self._tarefa.cancel()
            try:
                await self._tarefa
            except asyncio.CancelledError:
                pass
        alvo = dividir(texto or "(vazio)")
        if not self.edicao_falhou:
            try:
                await self._mostrar(alvo, apagar_sobras=True)
                return [str(m.id) for m in self.mensagens]
            except Exception as e:  # noqa: BLE001 — cai para mensagens novas
                log.warning("edição final recusada (%s): mandando a resposta inteira", e)
        # Plano B: a resposta inteira em mensagem(ns) nova(s); as parciais
        # (se houver) saem, se der.
        for m in self.mensagens:
            try:
                await m.apagar()
            except Exception:  # noqa: BLE001 — sobrou um "..." no canal: só estética
                pass
        self.mensagens, self.mostrado = [], []
        ids = []
        for i, parte in enumerate(alvo):
            await self.ritmo.vez()
            m = await self.canal.enviar(parte, self.responder_a if i == 0 else None)
            ids.append(str(m.id))
        return ids


class Entregador:
    def __init__(
        self,
        mensageiro: Mensageiro,
        ola: Callable[[], dict[str, Any] | None],
        ritmo: Ritmo | None = None,
    ) -> None:
        self.mensageiro = mensageiro
        self._ola = ola
        self.ritmo = ritmo or Ritmo()
        self.transmissoes: dict[int, Transmissao] = {}
        self._dm_recentes: dict[str, float] = {}

    def notar_dm(self, autor_id: str) -> None:
        """Alguém mandou DM ao bot (pode receber resposta por um tempo)."""
        self._dm_recentes[autor_id] = self.ritmo.relogio()

    def _dm_permitida(self, pessoa: str, ola: dict[str, Any]) -> bool:
        if pessoa == ola.get("dono_id") or pessoa in (ola.get("pessoas") or []):
            return True
        quando = self._dm_recentes.get(pessoa)
        return quando is not None and self.ritmo.relogio() - quando <= JANELA_DM_RECENTE

    async def _canal(self, canal_id: str | None, dm_para: str | None = None) -> CanalSaida:
        ola = self._ola() or {}
        if canal_id is not None and canal_id not in (ola.get("canais") or []):
            raise ErroDestino(f"canal {canal_id} não é um canal permitido")
        if dm_para is not None and not self._dm_permitida(dm_para, ola):
            raise ErroDestino(f"DM para {dm_para}: não é o dono, nem pessoa conhecida, nem mandou DM")
        return await self.mensageiro.canal(canal_id, dm_para)

    async def processar(self, pedido: dict[str, Any]) -> list[str] | None:
        tipo = pedido.get("tipo")
        ref = pedido.get("ref")
        if tipo == "enviar":
            return await self.enviar(
                pedido.get("canal_id"),
                pedido.get("responder_a"),
                pedido.get("texto", ""),
                pedido.get("anexo"),
                pedido.get("dm_para"),
            )
        if tipo == "resposta_inicio":
            canal = await self._canal(pedido.get("canal_id"), pedido.get("dm_para"))
            t = Transmissao(canal, pedido.get("responder_a"), self.ritmo, self.ritmo.relogio)
            self.transmissoes[ref] = t
            await t.comecar()
            return None
        if tipo == "resposta_parcial":
            if (t := self.transmissoes.get(ref)) is not None:
                t.atualizar(pedido.get("texto", ""))
            return None
        if tipo == "resposta_fim":
            t = self.transmissoes.pop(ref, None)
            if t is None:
                # O adaptador reconectou no meio: vale como `enviar`.
                return await self.enviar(
                    pedido.get("canal_id"),
                    pedido.get("responder_a"),
                    pedido.get("texto", ""),
                    dm_para=pedido.get("dm_para"),
                )
            return await t.terminar(pedido.get("texto", ""))
        log.warning("pedido desconhecido do kernel: %s", tipo)
        return None

    async def enviar(
        self,
        canal_id: str | None,
        responder_a: str | None,
        texto: str,
        anexo: str | None = None,
        dm_para: str | None = None,
    ) -> list[str]:
        """Manda ``texto`` (em quantas mensagens precisar). A primeira
        responde a ``responder_a`` e leva o anexo, se houver."""
        canal = await self._canal(canal_id, dm_para)
        arquivo = anexo_seguro(anexo, (self._ola() or {}).get("workspace")) if anexo else None
        ids: list[str] = []
        for i, parte in enumerate(dividir(texto or "(vazio)")):
            await self.ritmo.vez()
            if i == 0 and arquivo is not None:
                m = await canal.enviar(parte, responder_a, arquivo)
            else:
                m = await canal.enviar(parte, responder_a if i == 0 else None)
            ids.append(str(m.id))
        return ids
