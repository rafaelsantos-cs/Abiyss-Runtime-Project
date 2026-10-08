"""A árvore de processos do Chromium: memória e encerramento (só Linux).

O kernel sobe este servidor num grupo de processos próprio e, para matá-lo,
manda SIGKILL ao grupo inteiro (o uv, o Python e o driver do Playwright,
que é um processo Node). O Playwright, porém, sobe o Chromium DESTACADO:
num grupo de processos novo. Então:

- a memória do Chromium é medida aqui, somando o RSS dos descendentes deste
  servidor que estão em outro grupo (a mesma conta que o kernel faz para a
  árvore inteira: RSS somado, lido de /proc);
- para matar o Chromium, mata-se o grupo dele (SIGKILL), com os filhos;
- se o próprio servidor morrer de repente, quem mata o Chromium é o vigia
  (vigia.py), que roda numa sessão própria e não morre junto.
"""

from __future__ import annotations

import os
import signal
from collections.abc import Iterable


def _stat(pid: int) -> list[str] | None:
    """Campos de /proc/<pid>/stat depois do nome (que pode ter espaços)."""
    try:
        texto = open(f"/proc/{pid}/stat", encoding="latin-1").read()
    except OSError:
        return None
    return texto[texto.rfind(")") + 2 :].split()


def pai_de(pid: int) -> int | None:
    campos = _stat(pid)
    return int(campos[1]) if campos else None


def grupo_de(pid: int) -> int | None:
    campos = _stat(pid)
    return int(campos[2]) if campos else None


def inicio_de(pid: int) -> int | None:
    """Instante de início (em tiques desde o boot): distingue um PID reusado."""
    campos = _stat(pid)
    return int(campos[19]) if campos else None


def exe_de(pid: int) -> str | None:
    try:
        return os.readlink(f"/proc/{pid}/exe")
    except OSError:
        return None


def rss_bytes(pid: int) -> int:
    try:
        with open(f"/proc/{pid}/status", encoding="latin-1") as arquivo:
            for linha in arquivo:
                if linha.startswith("VmRSS:"):
                    return int(linha.split()[1]) * 1024
    except (OSError, ValueError, IndexError):
        pass
    return 0


def todos() -> list[int]:
    try:
        return [int(n) for n in os.listdir("/proc") if n.isdigit()]
    except OSError:
        return []


def descendentes(raiz: int) -> list[int]:
    filhos: dict[int, list[int]] = {}
    for pid in todos():
        pai = pai_de(pid)
        if pai is not None:
            filhos.setdefault(pai, []).append(pid)
    achados, pendentes = [], list(filhos.get(raiz, []))
    while pendentes:
        pid = pendentes.pop()
        achados.append(pid)
        pendentes.extend(filhos.get(pid, []))
    return achados


def grupos_destacados(raiz: int, excluir: Iterable[int] = ()) -> dict[int, list[int]]:
    """Grupos de processos (pgid → pids) dos descendentes de `raiz` que NÃO
    estão no grupo dela: o Chromium (e os filhos dele)."""
    meu_grupo = grupo_de(raiz)
    fora = set(excluir)
    grupos: dict[int, list[int]] = {}
    for pid in descendentes(raiz):
        if pid in fora:
            continue
        grupo = grupo_de(pid)
        if grupo is not None and grupo != meu_grupo and grupo not in fora:
            grupos.setdefault(grupo, []).append(pid)
    return grupos


def membros_do_grupo(grupo: int) -> list[int]:
    return [pid for pid in todos() if grupo_de(pid) == grupo]


def rss_dos_grupos(grupos: Iterable[int]) -> int:
    """RSS somado de todos os processos desses grupos (inclusive os que já
    ficaram órfãos e não são mais descendentes de ninguém daqui)."""
    alvo = set(grupos)
    if not alvo:
        return 0
    return sum(rss_bytes(pid) for pid in todos() if grupo_de(pid) in alvo)


def matar_grupos(grupos: Iterable[int]) -> None:
    for grupo in set(grupos):
        if grupo <= 1 or grupo == os.getpgid(0):
            continue
        try:
            os.killpg(grupo, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass
