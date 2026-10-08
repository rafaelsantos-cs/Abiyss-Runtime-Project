"""Fatos de uma mensagem do Discord e a reconexão com o Discord."""

import asyncio
from types import SimpleNamespace as NS

import discord
import pytest

from discordio import fatos, manter
from espera import Espera

EU = 999
DONO = 111
CANAL = "222"


def mensagem(autor=DONO, guild=None, canal=5, **extra):
    base = dict(
        id=1,
        author=NS(id=autor, bot=False, display_name="Dono"),
        type=discord.MessageType.default,
        guild=guild,
        channel=NS(id=canal),
        content="oi",
        reference=None,
        attachments=[],
        mentions=[],
        webhook_id=None,
    )
    base.update(extra)
    return NS(**base)


def test_dm_do_dono_vira_fatos_sem_decidir_nada():
    f = fatos(mensagem(), EU, [])
    assert f == {
        "id": "1", "canal_id": "5", "dm": True, "autor_id": "111", "autor_nome": "Dono",
        "autor_bot": False, "menciona_bot": False, "responde_a": None, "responde_ao_bot": False,
        "texto": "oi",
    }


def test_proprio_bot_sistema_e_outros_canais_nem_vao_ao_kernel():
    assert fatos(mensagem(autor=EU), EU, []) is None
    assert fatos(mensagem(type=discord.MessageType.pins_add), EU, []) is None
    assert fatos(mensagem(guild=NS(id=1), canal=333), EU, [CANAL]) is None
    # Canal permitido: vai (o kernel decide se é o dono ou externo).
    assert fatos(mensagem(guild=NS(id=1), canal=int(CANAL)), EU, [CANAL])["dm"] is False


def test_reply_mencao_webhook_e_anexos():
    citada = NS(author=NS(id=EU))
    m = mensagem(
        reference=NS(message_id=77, resolved=citada),
        mentions=[NS(id=EU)],
        webhook_id=5,
        attachments=[NS(filename="foto.png")],
    )
    f = fatos(m, EU, [])
    assert f["responde_a"] == "77" and f["responde_ao_bot"] and f["menciona_bot"]
    assert f["autor_bot"], "webhook conta como bot"
    assert "foto.png" in f["texto"]


def test_manter_reconecta_com_espera_e_desiste_do_que_e_fatal():
    tentativas = []
    dormidas = []

    async def iniciar():
        tentativas.append(1)
        if len(tentativas) <= 2:
            raise OSError("rede fora")
        if len(tentativas) == 3:
            return  # conectou e caiu limpo
        raise discord.LoginFailure("token errado")

    async def dormir(s):
        dormidas.append(s)

    with pytest.raises(discord.LoginFailure):
        asyncio.run(manter(iniciar, Espera(base=5, teto=300, sorteio=0), dormir, (discord.LoginFailure,)))
    # 5, 10 (falhas seguidas); depois de uma sessão boa, volta à base.
    assert dormidas == [5, 10, 5]
