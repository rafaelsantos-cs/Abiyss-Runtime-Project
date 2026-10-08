"""A resposta chegando aos poucos: "pensando", ritmo das edições, pedaços
de 2000 e o plano B quando a edição falha. Tempo virtual: o teste faz o
relógio andar e as esperas acordam no instante certo."""

import asyncio

import pytest

from conftest import DiscordFalso, RelogioVirtual
from saida import PENSANDO, Entregador, Ritmo


@pytest.fixture
def relogio(virtual: RelogioVirtual) -> RelogioVirtual:
    return virtual


@pytest.fixture
def discord_falso(virtual: RelogioVirtual) -> DiscordFalso:
    return DiscordFalso(virtual)


def novo(discord_falso, relogio):
    return Entregador(discord_falso, lambda: {"dono_id": "111", "canal_id": None}, Ritmo(1.2, relogio, relogio.dormir))


async def deixar_passar(relogio, segundos: float) -> None:
    await relogio.avancar(segundos)


async def processar(relogio, e, pedido):
    return await relogio.ate_terminar(e.processar(pedido))


def espacadas(operacoes) -> bool:
    instantes = [o[0] for o in operacoes]
    return all(b - a >= 1.2 - 1e-9 for a, b in zip(instantes, instantes[1:]))


def test_pensando_vira_a_resposta_editando_no_ritmo(discord_falso, relogio):
    async def cenario():
        e = novo(discord_falso, relogio)
        await processar(relogio, e, {"tipo": "resposta_inicio", "ref": 1, "canal_id": None, "responder_a": "55"})
        await deixar_passar(relogio, 5)
        # Muitas parciais seguidas (o kernel manda até 4 por segundo).
        texto = ""
        for i in range(40):
            texto += f"palavra{i} "
            await e.processar({"tipo": "resposta_parcial", "ref": 1, "texto": texto})
            await deixar_passar(relogio, 0.25)
        ids = await processar(relogio, e, {"tipo": "resposta_fim", "ref": 1, "texto": texto + "fim."})
        return ids

    ids = asyncio.run(cenario())
    dm = discord_falso.dm
    assert len(dm.mensagens) == 1, "uma mensagem só, editada"
    m = dm.mensagens[0]
    assert ids == [str(m.id)]
    assert m.responder_a == "55"
    assert m.texto.endswith("palavra39 fim.")
    ops = discord_falso.operacoes
    assert ops[0][1:] == ("enviar", m.id, PENSANDO[0])
    # "Pensando" anda: . → .. → ...
    textos = [o[3] for o in ops]
    assert textos[1:3] == [PENSANDO[1], PENSANDO[2]]
    # No máximo uma operação a cada 1,2 s (10 s de parciais → poucas edições).
    assert espacadas(ops)
    edicoes_de_texto = [t for t in textos if t.startswith("palavra")]
    assert 3 <= len(edicoes_de_texto) <= 10, len(edicoes_de_texto)


def test_resposta_longa_continua_em_mensagens_novas_sem_quebrar_codigo(discord_falso, relogio):
    codigo = "\n".join(f"x = {i}  # " + "z" * 40 for i in range(100))
    final = f"Aqui está:\n```python\n{codigo}\n```\nPronto."

    async def cenario():
        e = novo(discord_falso, relogio)
        await processar(relogio, e, {"tipo": "resposta_inicio", "ref": 2, "canal_id": None, "responder_a": "56"})
        await e.processar({"tipo": "resposta_parcial", "ref": 2, "texto": final[:2500]})
        await deixar_passar(relogio, 6)
        return await processar(relogio, e, {"tipo": "resposta_fim", "ref": 2, "texto": final})

    ids = asyncio.run(cenario())
    visiveis = discord_falso.dm.visiveis()
    assert len(ids) == len(visiveis) >= 3
    assert all(len(v) <= 2000 for v in visiveis)
    assert all(sum(l.startswith("```") for l in v.split("\n")) % 2 == 0 for v in visiveis)
    assert visiveis[-1].endswith("Pronto.")
    assert [m.responder_a for m in discord_falso.dm.mensagens] == ["56"] + [None] * (len(visiveis) - 1)
    assert espacadas(discord_falso.operacoes)


def test_edicao_recusada_vira_uma_mensagem_final(discord_falso, relogio):
    async def cenario():
        e = novo(discord_falso, relogio)
        await processar(relogio, e, {"tipo": "resposta_inicio", "ref": 3, "canal_id": None, "responder_a": "57"})
        discord_falso.falhar_edicoes = True
        await e.processar({"tipo": "resposta_parcial", "ref": 3, "texto": "começando"})
        await deixar_passar(relogio, 5)
        return await processar(relogio, e, {"tipo": "resposta_fim", "ref": 3, "texto": "resposta completa"})

    ids = asyncio.run(cenario())
    dm = discord_falso.dm
    # O "pensando" saiu e a resposta veio inteira numa mensagem nova.
    assert dm.visiveis() == ["resposta completa"]
    final = [m for m in dm.mensagens if not m.apagada][0]
    assert ids == [str(final.id)] and final.responder_a == "57"
    assert dm.mensagens[0].apagada
    # Depois da primeira falha, ninguém insistiu em editar.
    assert not any(o[1] == "editar" for o in discord_falso.operacoes)


def test_fim_sem_inicio_vale_como_enviar(discord_falso, relogio):
    async def cenario():
        e = novo(discord_falso, relogio)
        return await processar(
            relogio, e, {"tipo": "resposta_fim", "ref": 9, "canal_id": None, "responder_a": "58", "texto": "resposta"}
        )

    ids = asyncio.run(cenario())
    assert discord_falso.dm.visiveis() == ["resposta"]
    assert ids == [str(discord_falso.dm.mensagens[0].id)]
