"""A camada MCP: a ferramenta que o modelo vê e o texto que ela devolve."""

import os
import sys

import pytest
from mcp import Client, StdioServerParameters

import isolamento
import servidor
from conftest import PASTA_SERVIDOR, exigir_caixa

pytestmark = pytest.mark.anyio


def resultado(**campos):
    padrao = dict(
        codigo_saida=0, stdout="", stderr="", stdout_bytes=0, stderr_bytes=0, stdout_cortado=False,
        stderr_cortado=False, duracao_segundos=0.5, espera_fila_segundos=0.0, timeout_segundos=30,
        interrupcao=None, pico_memoria_mb=3.0,
    )
    return isolamento.Resultado(**{**padrao, **campos})


def test_texto_da_resposta(fazer_config):
    cfg = fazer_config()
    texto = servidor.formatar(cfg, "ls -la\necho fim", resultado(stdout="a\nb\n", stdout_bytes=4), [])
    assert texto.splitlines() == [
        "$ ls -la …",
        "código de saída: 0",
        "duração: 0,50 s",
        "--- stdout (4 bytes) ---",
        "a",
        "b",
        "--- stderr (vazio) ---",
    ]


def test_texto_de_comando_interrompido_e_com_dica(fazer_config):
    cfg = fazer_config()
    r = resultado(codigo_saida=None, interrupcao="tempo", timeout_segundos=7, espera_fila_segundos=2.0)
    texto = servidor.formatar(cfg, "sleep 99", r, ["timeout pedido (999 s) maior que o máximo; usei 115 s"])
    assert "código de saída: nenhum (o comando foi interrompido)" in texto
    assert "INTERROMPIDO: passou do tempo limite (7 s); a saída abaixo é parcial." in texto
    assert "(mais 2,00 s esperando vaga)" in texto
    assert "aviso: timeout pedido (999 s)" in texto
    r = resultado(codigo_saida=1, stderr="MemoryError\n", stderr_bytes=12)
    assert f"dica: o limite de memória é de {cfg.memoria_mb} MiB por comando" in servidor.formatar(cfg, "x", r, [])
    r = resultado(codigo_saida=7, stderr="curl: (6) Could not resolve host: exemplo.com\n", stderr_bytes=40)
    assert "dica: a caixa não tem rede" in servidor.formatar(cfg, "x", r, [])


def test_texto_cabe_no_limite_do_kernel(fazer_config):
    cfg = fazer_config()
    grande = "x" * cfg.max_saida_bytes
    r = resultado(stdout=grande, stderr=grande, stdout_bytes=10**9, stderr_bytes=10**9,
                  stdout_cortado=True, stderr_cortado=True)
    texto = servidor.formatar(cfg, "y" * 5000, r, ["a" * 200])
    assert len(texto.encode()) <= cfg.kernel.max_bytes_resultado


def test_descricao_diz_os_limites(fazer_config):
    cfg = fazer_config()
    d = servidor.descricao(cfg)
    for trecho in ("bubblewrap", "/workspace", "SEM rede", f"{cfg.memoria_mb} MiB", f"até {cfg.timeout_maximo} s"):
        assert trecho in d


async def test_ferramenta_pelo_cliente_mcp(fazer_config):
    cfg = fazer_config()
    amb = exigir_caixa(cfg)
    async with Client(servidor.criar_servidor(cfg, amb)) as cliente:
        ferramentas = (await cliente.list_tools()).tools
        assert [f.name for f in ferramentas] == ["executar"]
        assert "SEM rede" in ferramentas[0].description
        propriedades = ferramentas[0].input_schema["properties"]
        assert set(propriedades) == {"comando", "timeout_segundos", "pasta"}

        r = await cliente.call_tool("executar", {"comando": "echo oi; echo erro >&2; exit 4"})
        assert not r.is_error
        texto = r.content[0].text
        assert "código de saída: 4" in texto and "oi" in texto and "erro" in texto
        assert r.structured_content["codigo_saida"] == 4
        assert r.structured_content["stdout"] == "oi\n"

        r = await cliente.call_tool("executar", {"comando": "pwd", "timeout_segundos": 99999})
        assert "aviso: timeout pedido (99999 s) maior que o máximo" in r.content[0].text

        r = await cliente.call_tool("executar", {"comando": "pwd", "pasta": "../"})
        assert r.is_error and "pasta inválida" in r.content[0].text

        r = await cliente.call_tool("executar", {"comando": "   "})
        assert r.is_error and "comando vazio" in r.content[0].text


async def test_servidor_de_verdade_pelo_stdio(env_base, fazer_config):
    """servidor.py como o kernel sobe: processo filho, conversa por stdio."""
    exigir_caixa(fazer_config())
    parametros = StdioServerParameters(
        command=sys.executable,
        args=["servidor.py"],
        cwd=str(PASTA_SERVIDOR),
        env={"PATH": os.environ["PATH"], **env_base},
    )
    async with Client(parametros) as cliente:
        r = await cliente.call_tool("executar", {"comando": "echo pelo-stdio"})
        assert "pelo-stdio" in r.content[0].text
