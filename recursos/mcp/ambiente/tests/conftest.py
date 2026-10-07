"""Projeto falso e systemctl/journalctl de mentira, que guardam cada linha
de comando recebida (para conferir que nada além de leitura é pedido)."""

import json
import sys
from pathlib import Path

import pytest

import config

PASTA_SERVIDOR = Path(__file__).resolve().parent.parent

UNIDADES = {
    "abiyss.service": {
        "Description": "Abiyss - agente de IA autonomo 24/7", "LoadState": "loaded", "ActiveState": "active",
        "SubState": "running", "UnitFileState": "enabled", "MainPID": "1234",
        "ActiveEnterTimestamp": "Tue 2026-10-06 03:00:00 -03", "NRestarts": "2", "MemoryCurrent": "314572800",
        "Result": "success",
    },
    "docker.service": {
        "Description": "Docker Application Container Engine", "LoadState": "loaded", "ActiveState": "failed",
        "SubState": "failed", "UnitFileState": "enabled", "MainPID": "0", "ActiveEnterTimestamp": "",
        "NRestarts": "0", "MemoryCurrent": "[not set]", "Result": "exit-code",
    },
    "ssh.service": {
        "Description": "OpenBSD Secure Shell server", "LoadState": "loaded", "ActiveState": "active",
        "SubState": "running", "UnitFileState": "enabled", "MainPID": "800", "ActiveEnterTimestamp": "",
        "NRestarts": "0", "MemoryCurrent": "18446744073709551615", "Result": "success",
    },
}

SYSTEMCTL = """#!{python}
import json, sys
json.dump(sys.argv[1:], open({registro!r}, "a")); open({registro!r}, "a").write("\\n")
unidades = {unidades!r}
args = sys.argv[1:]
assert args[0] == "show", args
nomes = args[args.index("--") + 1:]
blocos = []
for nome in nomes:
    campos = unidades.get(nome, {{"Description": nome, "LoadState": "not-found", "ActiveState": "inactive",
                                  "SubState": "dead", "UnitFileState": "", "MainPID": "0", "NRestarts": "0",
                                  "MemoryCurrent": "[not set]", "ActiveEnterTimestamp": "", "Result": "success"}})
    blocos.append("\\n".join(["Id=" + nome] + [k + "=" + v for k, v in campos.items()]))
print("\\n\\n".join(blocos))
"""

JOURNALCTL = """#!{python}
import json, sys
json.dump(sys.argv[1:], open({registro!r}, "a")); open({registro!r}, "a").write("\\n")
args = dict(a[2:].split("=", 1) for a in sys.argv[1:] if "=" in a)
unidade = args["unit"]
if unidade == "sem-permissao.service":
    print("Hint: You are currently not seeing messages from other users and the system.", file=sys.stderr)
    print("      Users in groups 'adm', 'systemd-journal' can see all messages.", file=sys.stderr)
    print("-- No entries --")
    sys.exit(0)
if unidade == "quebrado.service":
    print("Failed to open journal: Bad message", file=sys.stderr)
    sys.exit(1)
todas = ["2026-10-07T03:%02d:00-0300 vm abiyss[1234]: linha %03d do serviço" % (i % 60, i) for i in range(300)]
todas[-1] = "2026-10-07T03:59:59-0300 vm abiyss[1234]: conectando com token=abc123segredo"
print("-- Boot 1a2b3c --")
print("\\n".join(todas[-int(args["lines"]):]))
"""


def escrever_projeto(raiz: Path, timeout: int = 30, max_bytes_resultado: int = 1_000_000) -> Path:
    raiz.mkdir(parents=True, exist_ok=True)
    toml = raiz / "abiyss.toml"
    toml.write_text(
        f"""
[mcp]
max_bytes_resultado = {max_bytes_resultado}

[[mcp.servidores]]
nome = "ambiente"
comando = "uv"
diretorio = "{PASTA_SERVIDOR}"
timeout_segundos = {timeout}
"""
    )
    return toml


@pytest.fixture
def registro(tmp_path) -> Path:
    return tmp_path / "comandos.jsonl"


@pytest.fixture
def env_base(tmp_path, registro):
    toml = escrever_projeto(tmp_path / "projeto")
    falsos = tmp_path / "bin"
    falsos.mkdir()
    for nome, modelo in (("systemctl", SYSTEMCTL), ("journalctl", JOURNALCTL)):
        script = falsos / nome
        script.write_text(modelo.format(python=sys.executable, registro=str(registro), unidades=UNIDADES))
        script.chmod(0o755)
    return {
        "ABIYSS_CONFIG": str(toml),
        "AMBIENTE_SYSTEMCTL": str(falsos / "systemctl"),
        "AMBIENTE_JOURNALCTL": str(falsos / "journalctl"),
        "AMBIENTE_SERVICOS": "abiyss, docker.service ssh nao-existe sem-permissao quebrado",
    }


@pytest.fixture
def fazer_config(env_base):
    def fazer(**extra: str) -> config.Config:
        return config.carregar({**env_base, **extra})

    return fazer


def comandos(registro: Path) -> list[list[str]]:
    if not registro.exists():
        return []
    return [json.loads(l) for l in registro.read_text().splitlines() if l.strip()]


@pytest.fixture
def anyio_backend():
    return "asyncio"
