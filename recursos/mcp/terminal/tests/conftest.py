"""Projeto falso para os testes: um abiyss.toml e as áreas protegidas, cada
uma com um "segredo" que nunca pode aparecer dentro da caixa."""

from pathlib import Path

import pytest

import config
import isolamento

PASTA_SERVIDOR = Path(__file__).resolve().parent.parent

# Áreas protegidas do projeto falso (caminho relativo → conteúdo).
SEGREDOS = {
    "kernel/segredo.rs": "segredo-kernel",
    "identity/nucleo.md": "segredo-identidade",
    "data/abiyss.db": "segredo-dados",
    "skills/alguma/SKILL.md": "segredo-skill",
    ".env": "NIM_API_KEY=segredo-env",
    ".git/config": "segredo-git",
    "cofre/01_internal/nota.md": "segredo-cofre",
}


def escrever_projeto(
    raiz: Path,
    *,
    timeout: int = 120,
    max_memoria_mb: int = 512,
    max_bytes_resultado: int = 1_000_000,
    workspace: str = "workspace",
) -> Path:
    raiz.mkdir(parents=True, exist_ok=True)
    for relativo, conteudo in SEGREDOS.items():
        arquivo = raiz / relativo
        arquivo.parent.mkdir(parents=True, exist_ok=True)
        arquivo.write_text(conteudo)
    toml = raiz / "abiyss.toml"
    toml.write_text(
        f"""
[caminhos]
workspace = "{workspace}"

[mcp]
max_memoria_mb = {max_memoria_mb}
max_bytes_resultado = {max_bytes_resultado}

[[mcp.servidores]]
nome = "exemplo"
comando = "uv"
diretorio = "recursos/mcp/exemplo"

[[mcp.servidores]]
nome = "terminal"
comando = "uv"
diretorio = "{PASTA_SERVIDOR}"
timeout_segundos = {timeout}
"""
    )
    return toml


@pytest.fixture
def projeto(tmp_path: Path) -> Path:
    escrever_projeto(tmp_path / "projeto")
    return tmp_path / "projeto"


@pytest.fixture
def env_base(projeto: Path, tmp_path: Path) -> dict[str, str]:
    temporaria = tmp_path / "temporaria"
    temporaria.mkdir()
    return {
        "ABIYSS_CONFIG": str(projeto / "abiyss.toml"),
        "TERMINAL_PASTA_TEMPORARIA": str(temporaria),
    }


@pytest.fixture
def fazer_config(env_base):
    def fazer(**extra: str) -> config.Config:
        return config.carregar({**env_base, **extra})

    return fazer


def exigir_caixa(cfg: config.Config) -> isolamento.Ambiente:
    """O ambiente da caixa de verdade, ou pula o teste dizendo por quê."""
    try:
        return isolamento.preparar(cfg)
    except isolamento.Recusa as e:
        pytest.skip(f"caixa de areia indisponível neste ambiente: {e}")


@pytest.fixture
def anyio_backend():
    return "asyncio"
