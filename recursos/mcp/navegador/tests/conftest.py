"""Tudo local: um projeto falso (abiyss.toml) e, para as ferramentas, sites
de mentira num servidor HTTP em 127.0.0.1. Nenhum teste sai para a internet."""

from pathlib import Path

import pytest

import config

PASTA_SERVIDOR = Path(__file__).resolve().parent.parent
RAIZ_DO_REPOSITORIO = PASTA_SERVIDOR.parent.parent.parent


def escrever_projeto(
    raiz: Path,
    timeout: int = 90,
    max_memoria_mb: int = 1024,
    max_bytes_resultado: int = 1_000_000,
    interagir: bool | None = None,
) -> Path:
    raiz.mkdir(parents=True, exist_ok=True)
    navegador = "" if interagir is None else f"[navegador]\ninteragir = {str(interagir).lower()}\n"
    toml = raiz / "abiyss.toml"
    toml.write_text(
        f"""
[caminhos]
dados = "dados"
workspace = "ws"

{navegador}
[mcp]
max_memoria_mb = {max_memoria_mb}
max_bytes_resultado = {max_bytes_resultado}

[[mcp.servidores]]
nome = "navegador"
comando = "uv"
diretorio = "{PASTA_SERVIDOR}"
timeout_segundos = {timeout}
"""
    )
    return toml


@pytest.fixture
def projeto(tmp_path):
    return escrever_projeto(tmp_path / "projeto")


@pytest.fixture
def env_base(projeto):
    return {"ABIYSS_CONFIG": str(projeto)}


@pytest.fixture
def fazer_config(env_base):
    def fazer(**extra: str) -> config.Config:
        return config.carregar({**env_base, **extra})

    return fazer


@pytest.fixture
def anyio_backend():
    return "asyncio"
