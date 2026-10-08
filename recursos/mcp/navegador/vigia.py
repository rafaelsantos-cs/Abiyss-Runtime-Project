"""Vigia do Chromium: mata a árvore do navegador quando o servidor morre.

O servidor sobe este script numa SESSÃO própria (fora do grupo de
processos que o kernel mata) com um cano no stdin:

    python vigia.py <pid do servidor>

A cada meio segundo, o vigia anota os grupos de processos destacados que
descendem do servidor (o Chromium) e o executável de cada um. O servidor
também avisa pelo cano ("grupo <pgid>", "fim <pgid>"). Quando o cano fecha
(o servidor morreu, de qualquer jeito, inclusive SIGKILL) ou o servidor some,
o vigia manda SIGKILL a todo processo de um grupo anotado cujo executável
seja um dos anotados (um PID reusado por outro programa não é tocado) e sai.

Só biblioteca padrão; nunca escreve no stdout.
"""

from __future__ import annotations

import os
import signal
import sys
import threading
import time

import processos

INTERVALO = 0.5


def limpar(grupos: dict[int, set[str]]) -> int:
    mortos = 0
    for _ in range(3):  # um processo pode ter criado outro no meio
        for pid in processos.todos():
            grupo = processos.grupo_de(pid)
            if grupo in grupos and processos.exe_de(pid) in grupos[grupo] and pid != os.getpid():
                try:
                    os.kill(pid, signal.SIGKILL)
                    mortos += 1
                except (ProcessLookupError, PermissionError):
                    pass
        time.sleep(0.05)
    return mortos


def main() -> int:
    servidor = int(sys.argv[1])
    inicio = processos.inicio_de(servidor)
    if inicio is None:
        return 0
    grupos: dict[int, set[str]] = {}
    trava = threading.Lock()
    cano_fechou = threading.Event()

    def anotar(grupo: int) -> None:
        exes = {e for pid in processos.membros_do_grupo(grupo) if (e := processos.exe_de(pid))}
        with trava:
            grupos.setdefault(grupo, set()).update(exes)

    def ler_cano() -> None:
        for linha in sys.stdin:
            partes = linha.split()
            if len(partes) == 2 and partes[1].isdigit():
                if partes[0] == "grupo":
                    anotar(int(partes[1]))
                elif partes[0] == "fim":
                    with trava:
                        grupos.pop(int(partes[1]), None)
        cano_fechou.set()

    threading.Thread(target=ler_cano, daemon=True).start()
    while not cano_fechou.is_set():
        if processos.inicio_de(servidor) != inicio:
            break  # o servidor sumiu (ou o PID já é de outro processo)
        for grupo in processos.grupos_destacados(servidor, excluir=(os.getpid(),)):
            anotar(grupo)
        cano_fechou.wait(INTERVALO)
    with trava:
        anotados = dict(grupos)
    if anotados:
        mortos = limpar(anotados)
        if mortos:
            print(f"navegador (vigia): o servidor morreu; matei {mortos} processo(s) do Chromium", file=sys.stderr, flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
