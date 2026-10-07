"""As ferramentas pelo cliente MCP, com systemctl e journalctl de mentira."""

import os
import subprocess
import sys

import pytest
from mcp import Client, StdioServerParameters

import config
import servidor
from conftest import PASTA_SERVIDOR, comandos

pytestmark = pytest.mark.anyio

# Verbos/opções que mudariam o sistema: nunca podem aparecer.
PROIBIDOS = {"start", "stop", "restart", "reload", "enable", "disable", "kill", "mask", "unmask", "isolate",
             "daemon-reload", "reset-failed", "set-property", "edit", "--rotate", "--flush", "--sync",
             "--vacuum-size", "--vacuum-time", "--vacuum-files", "--relinquish-var", "--setup-keys"}


def so_leitura(registro):
    for argv in comandos(registro):
        assert not (set(argv) & PROIBIDOS), argv
        assert not any(a.split("=")[0] in PROIBIDOS for a in argv), argv


async def test_servicos(fazer_config, registro):
    async with Client(servidor.criar_servidor(fazer_config())) as cliente:
        r = await cliente.call_tool("servicos", {})
        texto = r.content[0].text
        assert "- abiyss.service: active (running), enabled, desde Tue 2026-10-06 03:00:00 -03, pid 1234, memória 300,0 MiB, 2 reinício(s)" in texto
        assert "- docker.service ⚠: failed (failed), enabled, último resultado: exit-code" in texto
        assert "- nao-existe.service: NÃO EXISTE nesta máquina" in texto
        assert "ssh.service" in texto and "memória" not in texto.split("ssh.service")[1].split("\n")[0]
        assert r.structured_content["servicos"][0]["memoria"] == 314572800
    so_leitura(registro)
    assert comandos(registro)[0][:2] == ["show", "--no-pager"]


async def test_diario(fazer_config, registro):
    async with Client(servidor.criar_servidor(fazer_config(AMBIENTE_MAX_BYTES="3000", AMBIENTE_MAX_LINHAS_DIARIO="250"))) as cliente:
        r = await cliente.call_tool("diario", {"servico": "abiyss", "linhas": 200, "prioridade": "warning"})
        assert not r.is_error, r.content[0].text
        texto = r.content[0].text
        assert texto.startswith("Diário de abiyss.service: 200 linha(s) mais recente(s), prioridade warning+")
        assert "linha(s) mais antiga(s) cortada(s) pelo ambiente" in texto, "corta as velhas, guarda as novas"
        assert "token=***" in texto and "abc123segredo" not in texto
        assert "-- Boot" not in texto
        assert len(texto.encode()) < 3000 + 500
        argv = comandos(registro)[-1]
        assert "--unit=abiyss.service" in argv and "--lines=200" in argv and "--priority=warning" in argv

        r = await cliente.call_tool("diario", {"servico": "abiyss", "linhas": 9999})
        assert "--lines=250" in comandos(registro)[-1], "teto de AMBIENTE_MAX_LINHAS_DIARIO"

        r = await cliente.call_tool("diario", {"servico": "sshd-de-outro"})
        assert r.is_error and "não está na lista" in r.content[0].text
        r = await cliente.call_tool("diario", {"servico": "abiyss", "prioridade": "tudo"})
        assert r.is_error and "prioridade" in r.content[0].text
        r = await cliente.call_tool("diario", {"servico": "sem-permissao"})
        assert r.is_error and "grupo adm ou systemd-journal" in r.content[0].text
        r = await cliente.call_tool("diario", {"servico": "quebrado"})
        assert r.is_error and "Bad message" in r.content[0].text
    so_leitura(registro)


async def test_disco_processos_portas_e_resumo(fazer_config, registro):
    async with Client(servidor.criar_servidor(fazer_config())) as cliente:
        r = await cliente.call_tool("disco", {})
        assert "% usado" in r.content[0].text and "livres de" in r.content[0].text
        r = await cliente.call_tool("processos", {"quantos": 3})
        texto = r.content[0].text
        assert texto.startswith("Memória: ") and "Carga: " in texto
        assert len(r.structured_content["processos"]) == 3
        r = await cliente.call_tool("portas", {})
        assert not r.is_error
        r = await cliente.call_tool("resumo", {})
        texto = r.content[0].text
        for secao in ("Memória:", "Serviços:", "Disco:", "Processos que mais usam memória:", "Portas abertas"):
            assert secao in texto, secao
        assert len(r.structured_content["processos"]) == 5
    so_leitura(registro)


async def test_sem_systemd_avisa_e_o_resto_funciona(fazer_config, tmp_path):
    cfg = fazer_config(AMBIENTE_SYSTEMCTL=str(tmp_path / "nao-existe"), AMBIENTE_JOURNALCTL=str(tmp_path / "nao-existe"))
    async with Client(servidor.criar_servidor(cfg)) as cliente:
        r = await cliente.call_tool("servicos", {})
        assert r.is_error and "não consegui executar" in r.content[0].text
        r = await cliente.call_tool("resumo", {})
        assert not r.is_error and "(não consegui ler:" in r.content[0].text


def test_nao_roda_como_root(monkeypatch):
    monkeypatch.setattr(servidor.os, "geteuid", lambda: 0)
    with pytest.raises(config.ErroConfig, match="não roda como root"):
        servidor.verificar_usuario()


async def test_servidor_de_verdade_pelo_stdio(env_base):
    if os.geteuid() == 0:
        pytest.skip("como root o servidor recusa subir (rode os testes como usuário comum)")
    parametros = StdioServerParameters(
        command=sys.executable, args=["servidor.py"], cwd=str(PASTA_SERVIDOR), env={"PATH": os.environ["PATH"], **env_base}
    )
    async with Client(parametros) as cliente:
        nomes = sorted(f.name for f in (await cliente.list_tools()).tools)
        assert nomes == ["diario", "disco", "portas", "processos", "resumo", "servicos"]
        r = await cliente.call_tool("servicos", {})
        assert "abiyss.service" in r.content[0].text


def test_processo_recusa_root_ou_config_ruim(env_base):
    env = {**os.environ, **env_base, "AMBIENTE_SERVICOS": "x;y"}
    r = subprocess.run([sys.executable, "servidor.py"], cwd=PASTA_SERVIDOR, env=env, stdin=subprocess.DEVNULL,
                       capture_output=True, text=True, timeout=60)
    assert r.returncode == 2 and r.stdout == "" and "ambiente: NÃO vou subir" in r.stderr
