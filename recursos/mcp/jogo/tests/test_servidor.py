"""A camada MCP: as sete ferramentas, a trava e o servidor de verdade por stdio."""

import asyncio
import json
import os
import sys
from pathlib import Path

import pytest
from mcp import Client, StdioServerParameters

import servidor
from sessao import Sessao
from test_sessao import SOLUCAO_EXEMPLO, config, pastas  # noqa: F401 (fixture)

pytestmark = pytest.mark.anyio

PASTA_SERVIDOR = Path(__file__).resolve().parent.parent
FERRAMENTAS = ["regras", "ver", "simular", "executar", "reiniciar_fase", "desistir_fase", "proxima_fase"]


async def test_ferramentas_pelo_cliente_mcp(pastas):  # noqa: F811
    s = Sessao(config(pastas))
    async with Client(servidor.criar_servidor(s)) as cliente:
        nomes = [f.name for f in (await cliente.list_tools()).tools]
        assert nomes == FERRAMENTAS
        r = await cliente.call_tool("regras", {})
        assert "TRAVESSIA" in r.content[0].text and "8 ×" not in r.content[0].text
        r = await cliente.call_tool("ver", {"avatar": "R"})
        assert "Janela em volta de Rocha" in r.content[0].text
        r = await cliente.call_tool("ver", {"avatar": "C"})
        assert r.is_error and "não joga esta fase" in r.content[0].text
        r = await cliente.call_tool("simular", {"plano": SOLUCAO_EXEMPLO})
        assert "A fase seria concluída" in r.content[0].text
        r = await cliente.call_tool("executar", {"plano": SOLUCAO_EXEMPLO[:3], "versao": 0})
        assert not r.is_error and "versão 3" in r.content[0].text
        r = await cliente.call_tool("executar", {"plano": ["V L"], "versao": 0})
        assert r.is_error and "o estado mudou" in r.content[0].text
        r = await cliente.call_tool("proxima_fase", {})
        assert r.is_error and "ainda não acabou" in r.content[0].text
        r = await cliente.call_tool("desistir_fase", {})
        assert "desistiu" in r.content[0].text
        r = await cliente.call_tool("proxima_fase", {})
        assert r.content[0].text.startswith("FASE 2")


async def test_leituras_em_paralelo_e_mudancas_uma_de_cada_vez(pastas):  # noqa: F811
    s = Sessao(config(pastas))
    async with Client(servidor.criar_servidor(s)) as cliente:
        leituras = [cliente.call_tool("simular", {"plano": SOLUCAO_EXEMPLO}) for _ in range(8)]
        leituras += [cliente.call_tool("ver", {}) for _ in range(8)]
        resultados = await asyncio.gather(*leituras)
        assert all(not r.is_error for r in resultados)
        assert s.simulacoes == 8 and s.jogadas == 0
        mudancas = [cliente.call_tool("executar", {"plano": ["V L", "V O"]}) for _ in range(5)]
        resultados = await asyncio.gather(*mudancas)
        assert all(not r.is_error for r in resultados)
        assert s.jogadas == 10 and s.versao == 10


async def test_servidor_de_verdade_pelo_stdio(pastas):  # noqa: F811
    """servidor.py como o kernel sobe: processo filho, conversa por stdio."""
    execucao = pastas / "execucao.json"
    execucao.write_text(
        json.dumps(
            {
                "formato": 1,
                "id": "stdio",
                "braco": "A",
                "modo": "avaliacao",
                "catalogo": str(pastas / "catalogo"),
                "fases": [1, 3],
                "max_jogadas": {"1": 140, "3": 52},
                "replay": str(pastas / "r.jsonl"),
                "metricas": str(pastas / "m.jsonl"),
            }
        )
    )
    parametros = StdioServerParameters(
        command=sys.executable,
        args=["servidor.py"],
        cwd=str(PASTA_SERVIDOR),
        env={"PATH": os.environ["PATH"], "JOGO_EXECUCAO": str(execucao)},
    )
    async with Client(parametros) as cliente:
        r = await cliente.call_tool("executar", {"plano": SOLUCAO_EXEMPLO})
        assert "FASE 1 CONCLUÍDA em 15 jogadas" in r.content[0].text
        r = await cliente.call_tool("proxima_fase", {})
        assert r.content[0].text.startswith("FASE 3")
    m = [json.loads(l) for l in (pastas / "m.jsonl").read_text().splitlines()]
    assert m[0]["resultado"] == "concluida" and m[0]["jogadas"] == 15


async def test_sem_execucao_nao_sobe(tmp_path):
    proc = await asyncio.create_subprocess_exec(
        sys.executable,
        "servidor.py",
        cwd=str(PASTA_SERVIDOR),
        env={"PATH": os.environ["PATH"]},
        stdin=asyncio.subprocess.DEVNULL,
        stderr=asyncio.subprocess.PIPE,
    )
    _, erro = await proc.communicate()
    assert proc.returncode == 2 and b"JOGO_EXECUCAO" in erro
