"""Sem caixa segura, o servidor não sobe e diz por quê. Estes testes não
precisam de um bwrap funcionando: usam um bwrap ausente ou falso."""

import os
import stat
import subprocess
import sys
from pathlib import Path

import pytest

import isolamento
from conftest import PASTA_SERVIDOR


def bwrap_falso(pasta: Path, erro: str, codigo: int = 1) -> Path:
    """Um 'bwrap' que responde a --version/--help e falha ao criar a caixa."""
    caminho = pasta / "bwrap"
    caminho.write_text(
        "#!/bin/sh\n"
        'case "$1" in\n'
        "  --version) echo 'bubblewrap 0.9.0'; exit 0;;\n"
        "  --help) echo '--disable-userns --size'; exit 0;;\n"
        "esac\n"
        "cat >&2 <<'FIM'\n"
        f"{erro}\n"
        "FIM\n"
        f"exit {codigo}\n"
    )
    caminho.chmod(0o755)
    return caminho


@pytest.fixture
def nao_root(monkeypatch):
    monkeypatch.setattr(isolamento.os, "geteuid", lambda: 1000)


def test_sem_bwrap_recusa(fazer_config, tmp_path):
    cfg = fazer_config(TERMINAL_BWRAP=str(tmp_path / "nao-existe"))
    with pytest.raises(isolamento.Recusa, match="não encontrado.*sudo apt install bubblewrap"):
        isolamento.preparar(cfg)


def test_bwrap_setuid_recusa(fazer_config, tmp_path):
    falso = bwrap_falso(tmp_path, "nunca chega aqui")
    falso.chmod(0o755 | stat.S_ISUID)
    if not os.stat(falso).st_mode & stat.S_ISUID:
        pytest.skip("o sistema de arquivos do teste não guarda o bit setuid")
    with pytest.raises(isolamento.Recusa, match="setuid"):
        isolamento.preparar(fazer_config(TERMINAL_BWRAP=str(falso)))


def test_sem_namespaces_de_usuario_recusa_explicando(fazer_config, tmp_path, nao_root):
    falso = bwrap_falso(tmp_path, "bwrap: setting up uid map: Permission denied")
    with pytest.raises(isolamento.Recusa) as erro:
        isolamento.preparar(fazer_config(TERMINAL_BWRAP=str(falso)))
    texto = str(erro.value)
    assert "setting up uid map: Permission denied" in texto
    assert "namespaces de usuário estão bloqueados" in texto
    assert "apparmor_restrict_unprivileged_userns" in texto
    assert "/etc/apparmor.d/bwrap" in texto


def test_caixa_que_falha_por_outro_motivo_recusa_com_o_erro(fazer_config, tmp_path, nao_root):
    falso = bwrap_falso(tmp_path, "bwrap: Can't mount proc on /newroot/proc: Device busy")
    with pytest.raises(isolamento.Recusa, match="Can't mount proc"):
        isolamento.preparar(fazer_config(TERMINAL_BWRAP=str(falso)))


def test_como_root_recusa(fazer_config, tmp_path, monkeypatch):
    monkeypatch.setattr(isolamento.os, "geteuid", lambda: 0)
    falso = bwrap_falso(tmp_path, "nunca chega aqui")
    with pytest.raises(isolamento.Recusa, match="não roda como root"):
        isolamento.preparar(fazer_config(TERMINAL_BWRAP=str(falso)))


def test_processo_do_servidor_sai_com_codigo_2_e_o_motivo(env_base, tmp_path):
    """O caminho de verdade: servidor.py sobe, recusa e não abre o stdio."""
    env = {**os.environ, **env_base, "TERMINAL_BWRAP": str(tmp_path / "nao-existe")}
    r = subprocess.run(
        [sys.executable, "servidor.py"],
        cwd=PASTA_SERVIDOR,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert r.returncode == 2
    assert r.stdout == "", "nada no stdout: é o canal do MCP"
    assert "terminal: NÃO vou subir: bwrap (bubblewrap) não encontrado" in r.stderr


def test_config_invalida_tambem_recusa(env_base):
    env = {**os.environ, **env_base, "TERMINAL_TIMEOUT_MAXIMO": "500"}
    r = subprocess.run(
        [sys.executable, "servidor.py"],
        cwd=PASTA_SERVIDOR,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert r.returncode == 2
    assert "o kernel mataria o servidor no meio do comando" in r.stderr
