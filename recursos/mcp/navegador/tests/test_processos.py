"""A árvore do Chromium sem o Chromium: grupos destacados, memória e o vigia.

O "Chromium" aqui é um grupo de `sleep` destacado (start_new_session), como
o Playwright faz com o navegador de verdade."""

import os
import signal
import subprocess
import sys
import time

import processos
import vigia
from conftest import PASTA_SERVIDOR

SERVIDOR_FALSO = r"""
import os, subprocess, sys, time
vigia = subprocess.Popen([sys.executable, "vigia.py", str(os.getpid())], stdin=subprocess.PIPE,
                         start_new_session=True, cwd=sys.argv[1])
falso = subprocess.Popen(["sh", "-c", "sleep 300 & sleep 300 & wait"], start_new_session=True)
print(falso.pid, vigia.pid, flush=True)
time.sleep(300)
"""


def vivos(grupo: int) -> list[int]:
    return processos.membros_do_grupo(grupo)


def esperar(condicao, segundos=8.0):
    fim = time.monotonic() + segundos
    while time.monotonic() < fim:
        if condicao():
            return True
        time.sleep(0.1)
    return condicao()


def test_grupos_destacados_e_memoria():
    falso = subprocess.Popen(["sh", "-c", "sleep 300 & wait"], start_new_session=True)
    try:
        assert esperar(lambda: len(vivos(falso.pid)) == 2)
        grupos = processos.grupos_destacados(os.getpid())
        assert set(grupos[falso.pid]) == set(vivos(falso.pid))
        assert processos.grupos_destacados(os.getpid(), excluir=[falso.pid]).get(falso.pid) is None
        anotados: dict[int, set[str]] = {}
        processos.anotar(anotados, {falso.pid: grupos[falso.pid]})
        assert anotados == {falso.pid: {processos.exe_de(p) for p in vivos(falso.pid)}}
        membros = processos.membros_verificados(anotados)
        assert set(membros) == set(vivos(falso.pid)) and processos.rss_bytes_de(membros) > 0
        # Mesmo número de grupo, outro executável: não é o "Chromium" anotado.
        assert processos.membros_verificados({falso.pid: {"/outro/programa"}}) == []
    finally:
        processos.matar_grupos([falso.pid])
        falso.wait()
    assert esperar(lambda: not vivos(falso.pid))


def test_vigia_nao_mata_executavel_que_nao_anotou():
    """PID (e grupo) reusado por outro programa: o vigia não toca."""
    falso = subprocess.Popen(["sh", "-c", "sleep 300 & wait"], start_new_session=True)
    try:
        assert esperar(lambda: len(vivos(falso.pid)) == 2)
        assert vigia.limpar({falso.pid: {"/nao/e/o/chromium"}}) == 0
        assert len(vivos(falso.pid)) == 2
        exes = {processos.exe_de(p) for p in vivos(falso.pid)}
        assert vigia.limpar({falso.pid: exes}) == 2
        assert esperar(lambda: not [p for p in vivos(falso.pid) if processos.pai_de(p) != os.getpid()])
    finally:
        processos.matar_grupos([falso.pid])
        falso.wait()


def test_servidor_morto_com_sigkill_nao_deixa_orfao():
    """Como o kernel faz: SIGKILL no grupo do servidor. O Chromium (em outro
    grupo) sobreviveria; o vigia, numa sessão própria, mata o grupo dele."""
    servidor = subprocess.Popen(
        [sys.executable, "-c", SERVIDOR_FALSO, str(PASTA_SERVIDOR)],
        stdout=subprocess.PIPE,
        text=True,
        start_new_session=True,
    )
    grupo_falso, pid_vigia = map(int, servidor.stdout.readline().split())
    try:
        assert esperar(lambda: len(vivos(grupo_falso)) == 3)
        time.sleep(2 * vigia.INTERVALO + 0.3)  # o vigia anota o grupo
        os.killpg(servidor.pid, signal.SIGKILL)
        servidor.wait()
        assert esperar(lambda: not vivos(grupo_falso)), "o grupo destacado ficou órfão"
        assert esperar(lambda: processos.inicio_de(pid_vigia) is None or processos._stat(pid_vigia)[0] == "Z"), (
            "o vigia sai depois de limpar"
        )
    finally:
        processos.matar_grupos([grupo_falso])
