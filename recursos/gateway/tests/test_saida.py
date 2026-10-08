"""Entrega no Discord de mentira: pedaços, destino e ritmo."""

import asyncio

import pytest

from saida import Entregador, ErroDestino, Ritmo


def entregador(discord_falso, relogio, canal_id=None):
    ola = {"dono_id": "111", "canal_id": canal_id}
    return Entregador(discord_falso, lambda: ola, Ritmo(1.2, relogio, relogio.dormir))


def test_texto_longo_vira_varias_mensagens_e_so_a_primeira_responde(discord_falso, relogio):
    e = entregador(discord_falso, relogio)
    texto = "\n".join(f"linha {i:04d} " + "x" * 40 for i in range(120))  # ~6 KB
    ids = asyncio.run(e.processar({"tipo": "enviar", "ref": 1, "canal_id": None, "responder_a": "55", "texto": texto}))
    dm = discord_falso.dm.mensagens
    assert len(ids) == len(dm) >= 3
    assert [m.responder_a for m in dm] == ["55"] + [None] * (len(dm) - 1)
    assert "\n".join(m.texto for m in dm) == texto, "nada se perde na divisão"


def test_so_a_dm_do_dono_ou_o_canal_permitido(discord_falso, relogio):
    e = entregador(discord_falso, relogio, canal_id="222")
    asyncio.run(e.processar({"tipo": "enviar", "ref": 1, "canal_id": "222", "texto": "no canal"}))
    assert discord_falso.canais["222"].visiveis() == ["no canal"]
    with pytest.raises(ErroDestino):
        asyncio.run(e.processar({"tipo": "enviar", "ref": 2, "canal_id": "999", "texto": "vazou?"}))
    assert "999" not in discord_falso.canais


def test_ritmo_espaca_as_operacoes(relogio):
    ritmo = Ritmo(1.2, relogio, relogio.dormir)

    async def tres():
        instantes = []
        for _ in range(3):
            await ritmo.vez()
            instantes.append(relogio())
        return instantes

    a, b, c = asyncio.run(tres())
    assert b - a >= 1.2 and c - b >= 1.2
