"""A ponte com o kernel: socket Unix local, JSON, uma mensagem por linha.

- Conecta (e reconecta com espera exponencial) ao socket do daemon. O
  daemon fora do ar não derruba o adaptador: ele só espera e tenta de novo.
- Mensagens do Discord ficam guardadas até o kernel confirmar
  (``recebido``); numa reconexão, as não confirmadas vão de novo (o kernel
  descarta repetidas pelo ID do Discord).
- O que o kernel manda entregar passa por UMA fila, em ordem (a resposta
  não ultrapassa a anterior). Cada entrega é confirmada com os IDs no
  Discord (``enviado``) ou recusada (``falhou``); se o kernel mandar de
  novo uma ``ref`` já entregue (a confirmação se perdeu), só confirma de
  novo, sem duplicar no Discord.
"""

from __future__ import annotations

import asyncio
import json
import logging
from collections import OrderedDict
from collections.abc import Awaitable, Callable
from pathlib import Path
from typing import Any

from espera import Espera

log = logging.getLogger("abiyss.gateway.ponte")

VERSAO = 1
MAX_LINHA = 256 * 1024
PRAZO_OLA = 10.0
# Mensagens do Discord esperando o kernel (daemon fora do ar por muito
# tempo): passou disso, as mais antigas saem (com aviso no log).
MAX_ESPERANDO = 500
# Refs já entregues lembradas (para não duplicar numa reentrega).
MAX_LEMBRADAS = 1000

Processar = Callable[[dict[str, Any]], Awaitable[list[str] | None]]


class ErroProtocolo(Exception):
    pass


class Ponte:
    def __init__(
        self,
        caminho: Path,
        processar: Processar,
        ao_ola: Callable[[dict[str, Any]], Awaitable[None]] | None = None,
        espera: Espera | None = None,
        dormir: Callable[[float], Awaitable[None]] = asyncio.sleep,
    ) -> None:
        self.caminho = Path(caminho)
        self._processar = processar
        self._ao_ola = ao_ola
        self.espera = espera or Espera()
        self._dormir = dormir
        self._esperando: OrderedDict[str, dict[str, Any]] = OrderedDict()
        self._entregues: OrderedDict[int, list[str]] = OrderedDict()
        self._fila: asyncio.Queue[dict[str, Any]] = asyncio.Queue()
        self._escritor: asyncio.StreamWriter | None = None
        self._trava_escrita = asyncio.Lock()
        self.ola: dict[str, Any] | None = None
        self.conectado = asyncio.Event()

    # ------------------------------------------------------------------
    # Para o lado do Discord
    # ------------------------------------------------------------------

    async def mensagem(self, fatos: dict[str, Any]) -> None:
        """Uma mensagem do Discord para o kernel (guardada até ele confirmar)."""
        self._esperando[fatos["id"]] = fatos
        while len(self._esperando) > MAX_ESPERANDO:
            antiga, _ = self._esperando.popitem(last=False)
            log.warning("kernel fora do ar há muito tempo: mensagem %s descartada", antiga)
        await self._mandar({"tipo": "mensagem", "mensagem": fatos})

    def esperando(self) -> list[str]:
        """IDs ainda não confirmados pelo kernel."""
        return list(self._esperando)

    # ------------------------------------------------------------------
    # Conexão
    # ------------------------------------------------------------------

    async def rodar(self) -> None:
        """Para sempre: conecta, atende, cai, espera, conecta de novo."""
        entregador = asyncio.create_task(self._entregar())
        try:
            while True:
                try:
                    await self._uma_conexao()
                    log.warning("o daemon fechou a conexão")
                except (OSError, asyncio.IncompleteReadError, ErroProtocolo, ValueError) as e:
                    log.warning("sem conexão com o daemon (%s): %s", self.caminho, e)
                espera = self.espera.proxima()
                log.info("tentando o daemon de novo em %.1f s", espera)
                await self._dormir(espera)
        finally:
            entregador.cancel()

    async def _uma_conexao(self) -> None:
        leitor, escritor = await asyncio.open_unix_connection(str(self.caminho), limit=MAX_LINHA)
        try:
            escritor.write(_linha({"tipo": "ola", "versao": VERSAO}))
            await escritor.drain()
            ola = json.loads(await asyncio.wait_for(leitor.readline(), PRAZO_OLA) or b"null")
            if not isinstance(ola, dict) or ola.get("tipo") != "ola":
                raise ErroProtocolo("o daemon não respondeu ao ola")
            if ola.get("versao") != VERSAO:
                raise ErroProtocolo(f"protocolo v{ola.get('versao')}, o adaptador fala v{VERSAO}")
            self.ola = ola
            self.espera.zerar()
            self._escritor = escritor
            self.conectado.set()
            log.info("conectado ao daemon")
            # O que ficou sem confirmação vai de novo, na ordem.
            for fatos in list(self._esperando.values()):
                await self._mandar({"tipo": "mensagem", "mensagem": fatos})
            if self._ao_ola is not None:
                asyncio.create_task(self._ao_ola(ola))
            while linha := await leitor.readline():
                try:
                    mensagem = json.loads(linha)
                except json.JSONDecodeError:
                    log.warning("linha inválida do daemon")
                    continue
                self._tratar(mensagem)
        finally:
            self.conectado.clear()
            self._escritor = None
            escritor.close()

    def _tratar(self, mensagem: dict[str, Any]) -> None:
        tipo = mensagem.get("tipo")
        if tipo == "recebido":
            self._esperando.pop(mensagem.get("id"), None)
        elif tipo == "ola":
            self.ola = mensagem
        else:
            self._fila.put_nowait(mensagem)

    async def _mandar(self, mensagem: dict[str, Any]) -> bool:
        escritor = self._escritor
        if escritor is None:
            return False
        try:
            async with self._trava_escrita:
                escritor.write(_linha(mensagem))
                await escritor.drain()
            return True
        except OSError:
            return False

    # ------------------------------------------------------------------
    # Entregas (o que o kernel manda)
    # ------------------------------------------------------------------

    async def _entregar(self) -> None:
        while True:
            pedido = await self._fila.get()
            ref = pedido.get("ref")
            if ref is not None and ref in self._entregues and pedido.get("tipo") in ("enviar", "resposta_fim"):
                # A confirmação anterior se perdeu: confirma de novo, sem duplicar.
                await self._mandar({"tipo": "enviado", "ref": ref, "ids": self._entregues[ref]})
                continue
            try:
                ids = await self._processar(pedido)
            except Exception as e:  # noqa: BLE001 — qualquer falha vira `falhou`, o kernel tenta de novo
                log.warning("não consegui entregar %s #%s: %s", pedido.get("tipo"), ref, e)
                if ref is not None:
                    await self._mandar({"tipo": "falhou", "ref": ref, "erro": str(e)[:300]})
                continue
            if ref is not None and ids is not None:
                self._entregues[ref] = ids
                while len(self._entregues) > MAX_LEMBRADAS:
                    self._entregues.popitem(last=False)
                await self._mandar({"tipo": "enviado", "ref": ref, "ids": ids})


def _linha(mensagem: dict[str, Any]) -> bytes:
    return (json.dumps(mensagem, ensure_ascii=False) + "\n").encode("utf-8")
