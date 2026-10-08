"""Arquivo só sai de dentro do workspace (conferido de novo no adaptador)."""

import asyncio
import os

import pytest

from saida import Entregador, ErroAnexo, Ritmo, anexo_seguro


@pytest.fixture
def area(tmp_path):
    ws = tmp_path / "workspace"
    (ws / "relatorios").mkdir(parents=True)
    (ws / "relatorios/hoje.md").write_text("# hoje")
    (tmp_path / ".env").write_text("ABIYSS_DISCORD_TOKEN=segredo")
    os.symlink(tmp_path / ".env", ws / "atalho")
    return tmp_path, ws.resolve()


def test_dentro_do_workspace_passa(area):
    _, ws = area
    assert anexo_seguro(str(ws / "relatorios/hoje.md"), str(ws)) == ws / "relatorios/hoje.md"


@pytest.mark.parametrize(
    "pedido",
    [
        lambda raiz, ws: raiz / ".env",  # fora
        lambda raiz, ws: ws / "atalho",  # link para fora
        lambda raiz, ws: ws / "relatorios/../../.env",  # .. para fora
        lambda raiz, ws: ws / "relatorios",  # pasta
        lambda raiz, ws: ws / "nao-existe",
        lambda raiz, ws: "relatorios/hoje.md",  # relativo
    ],
)
def test_fora_do_workspace_nao_passa(area, pedido):
    raiz, ws = area
    with pytest.raises(ErroAnexo):
        anexo_seguro(str(pedido(raiz, ws)), str(ws))


def test_grande_demais_ou_sem_workspace(area):
    _, ws = area
    with pytest.raises(ErroAnexo):
        anexo_seguro(str(ws / "relatorios/hoje.md"), str(ws), max_bytes=3)
    with pytest.raises(ErroAnexo):
        anexo_seguro(str(ws / "relatorios/hoje.md"), None)


def test_entregador_anexa_na_primeira_mensagem_e_recusa_o_resto(area, discord_falso, relogio):
    raiz, ws = area
    e = Entregador(discord_falso, lambda: {"dono_id": "1", "canal_id": None, "workspace": str(ws)},
                   Ritmo(1.2, relogio, relogio.dormir))
    pedido = {"tipo": "enviar", "ref": 1, "canal_id": None, "texto": "📎", "anexo": str(ws / "relatorios/hoje.md")}
    asyncio.run(e.processar(pedido))
    assert discord_falso.dm.mensagens[0].arquivo == ws / "relatorios/hoje.md"
    fora = dict(pedido, ref=2, anexo=str(raiz / ".env"))
    with pytest.raises(ErroAnexo):
        asyncio.run(e.processar(fora))
    assert len(discord_falso.dm.mensagens) == 1, "nada saiu"
