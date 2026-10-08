"""Entrega no Discord de mentira: pedaços, destino e ritmo."""

import asyncio

import pytest

from saida import Entregador, ErroDestino, Ritmo


def entregador(discord_falso, relogio, canal_id=None):
    ola = {"dono_id": "111", "canais": [canal_id] if canal_id else [], "pessoas": ["333"]}
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


def test_dm_so_ao_dono_a_conhecidos_ou_a_quem_mandou_dm(discord_falso, relogio):
    e = entregador(discord_falso, relogio)
    enviar = lambda ref, para: asyncio.run(  # noqa: E731
        e.processar({"tipo": "enviar", "ref": ref, "canal_id": None, "dm_para": para, "texto": "oi"})
    )
    enviar(1, "333")  # pessoa conhecida
    assert discord_falso.canais["dm:333"].visiveis() == ["oi"]
    with pytest.raises(ErroDestino):
        enviar(2, "444")  # estranho que nunca escreveu
    assert "dm:444" not in discord_falso.canais
    e.notar_dm("444")  # mandou DM agora: a resposta fixa pode ir
    enviar(3, "444")
    assert discord_falso.canais["dm:444"].visiveis() == ["oi"]
    relogio.agora += 3601  # uma hora depois, não mais
    with pytest.raises(ErroDestino):
        enviar(4, "444")
