"""Entrega no Discord o que o kernel manda, sem abusar da API.

O lado Discord de verdade fica em ``discordio`` (discord.py). Aqui só há a
lógica, que os testes exercitam com um Discord de mentira:

- ``Ritmo``: no máximo uma operação no Discord (enviar, editar) a cada
  ``intervalo`` segundos;
- ``Entregador``: recebe os pedidos do kernel (pela ``Ponte``) e devolve
  os IDs das mensagens criadas.

Destino: ``canal_id`` nulo é a DM do dono; qualquer outro canal só se for
o canal permitido que o kernel informou no ``ola`` (defesa em dobro: o
kernel já só manda para lá).
"""

from __future__ import annotations

import asyncio
import logging
import time
from collections.abc import Awaitable, Callable
from typing import Any, Protocol

from partes import dividir

log = logging.getLogger("abiyss.gateway.saida")

INTERVALO_PADRAO = 1.2


class MensagemSaida(Protocol):
    id: int

    async def editar(self, texto: str) -> None: ...

    async def apagar(self) -> None: ...


class CanalSaida(Protocol):
    async def enviar(self, texto: str, responder_a: str | None) -> MensagemSaida: ...


class Mensageiro(Protocol):
    async def canal(self, canal_id: str | None) -> CanalSaida:
        """``None`` = a DM do dono."""
        ...


class ErroDestino(Exception):
    pass


class Ritmo:
    """No máximo uma operação a cada ``intervalo`` segundos."""

    def __init__(
        self,
        intervalo: float = INTERVALO_PADRAO,
        relogio: Callable[[], float] = time.monotonic,
        dormir: Callable[[float], Awaitable[None]] = asyncio.sleep,
    ) -> None:
        self.intervalo = intervalo
        self._relogio = relogio
        self._dormir = dormir
        self._ultima: float | None = None

    async def vez(self) -> None:
        """Espera a vez da próxima operação (e marca o instante dela)."""
        if self._ultima is not None:
            falta = self._ultima + self.intervalo - self._relogio()
            if falta > 0:
                await self._dormir(falta)
        self._ultima = self._relogio()


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

    async def _canal(self, canal_id: str | None) -> CanalSaida:
        if canal_id is not None:
            ola = self._ola() or {}
            if canal_id != ola.get("canal_id"):
                raise ErroDestino(f"canal {canal_id} não é o canal permitido")
        return await self.mensageiro.canal(canal_id)

    async def processar(self, pedido: dict[str, Any]) -> list[str] | None:
        tipo = pedido.get("tipo")
        if tipo == "enviar":
            return await self.enviar(pedido.get("canal_id"), pedido.get("responder_a"), pedido.get("texto", ""))
        log.warning("pedido desconhecido do kernel: %s", tipo)
        return None

    async def enviar(self, canal_id: str | None, responder_a: str | None, texto: str) -> list[str]:
        """Manda ``texto`` (em quantas mensagens precisar). A primeira
        responde a ``responder_a``."""
        canal = await self._canal(canal_id)
        ids: list[str] = []
        for i, parte in enumerate(dividir(texto or "(vazio)")):
            await self.ritmo.vez()
            m = await canal.enviar(parte, responder_a if i == 0 else None)
            ids.append(str(m.id))
        return ids
