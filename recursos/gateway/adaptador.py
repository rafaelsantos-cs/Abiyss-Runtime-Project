"""Adaptador do Discord do Abiyss (processo separado do daemon).

    uv run --frozen --quiet --no-dev adaptador.py

Liga duas pontas, cada uma com a sua reconexão:
- o daemon, pelo socket Unix local de ``[gateway] socket`` (``ponte``);
- o Discord, com o token de ``ABIYSS_DISCORD_TOKEN`` (``discordio``).

Uma ponta fora do ar não derruba a outra: mensagens do dono esperam o
daemon voltar, e as saídas esperam no banco do daemon até o adaptador
confirmar a entrega. Passo a passo em docs/GATEWAY.md.
"""

from __future__ import annotations

import asyncio
import logging
import os
import sys

import discord

import config
from discordio import ClienteDiscord, manter
from espera import Espera
from ponte import Ponte
from saida import Entregador

log = logging.getLogger("abiyss.gateway")


async def rodar(cfg: config.Config) -> None:
    cliente = ClienteDiscord()
    entregador = Entregador(cliente, lambda: ponte.ola)
    ponte = Ponte(cfg.socket, entregador.processar, ao_ola=cliente.ao_ola)
    cliente.ponte = ponte
    cliente.ao_receber_dm = entregador.notar_dm

    async def iniciar() -> None:
        if cliente.is_closed():
            cliente.clear()
        try:
            await cliente.start(cfg.token)
        finally:
            if not cliente.is_closed():
                await cliente.close()

    fatais = (discord.LoginFailure, discord.PrivilegedIntentsRequired)
    await asyncio.gather(
        ponte.rodar(),
        manter(iniciar, Espera(base=5, teto=300), asyncio.sleep, fatais),
    )


def main() -> int:
    logging.basicConfig(
        level=os.environ.get("ABIYSS_GATEWAY_LOG", "INFO").upper(),
        format="%(levelname)s %(name)s: %(message)s",
        stream=sys.stderr,
    )
    # O discord.py é falante em DEBUG; o token nunca aparece nos logs dele.
    logging.getLogger("discord").setLevel(logging.WARNING)
    try:
        cfg = config.carregar(os.environ)
    except config.ErroConfig as e:
        log.error("%s", e)
        return 2
    try:
        asyncio.run(rodar(cfg))
    except discord.LoginFailure:
        log.error("o Discord recusou o token (ABIYSS_DISCORD_TOKEN): confira e reinicie")
        return 1
    except discord.PrivilegedIntentsRequired:
        log.error(
            "falta a intent MESSAGE CONTENT: Discord Developer Portal > Bot > "
            "Privileged Gateway Intents > Message Content Intent"
        )
        return 1
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
