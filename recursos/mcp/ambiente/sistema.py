"""Leituras da máquina, todas SÓ LEITURA.

O que vem do /proc e do statvfs é lido direto (sem comando nenhum). Do
systemd, só dois comandos, com a linha montada aqui e nomes de unidade da
lista permitida: ``systemctl show`` (estado) e ``journalctl`` (diário).
Nada de start/stop/restart, nada de sudo.
"""

from __future__ import annotations

import ipaddress
import os
import pwd
import re
import struct
from dataclasses import dataclass
from pathlib import Path

import anyio

PROC = Path("/proc")

# Sistemas de arquivos que não são disco de verdade.
FS_VIRTUAIS = {
    "proc", "sysfs", "devtmpfs", "devpts", "tmpfs", "cgroup", "cgroup2", "securityfs", "pstore", "bpf",
    "tracefs", "debugfs", "configfs", "fusectl", "mqueue", "hugetlbfs", "autofs", "binfmt_misc",
    "rpc_pipefs", "nsfs", "efivarfs", "ramfs", "squashfs", "overlay", "fuse.lxcfs", "fuse.portal",
    "fuse.gvfsd-fuse", "selinuxfs",
}

PROPRIEDADES = (
    "Id", "Description", "LoadState", "ActiveState", "SubState", "UnitFileState", "MainPID",
    "ActiveEnterTimestamp", "NRestarts", "MemoryCurrent", "Result",
)
PRIORIDADES = ("emerg", "alert", "crit", "err", "warning", "notice", "info", "debug")

# Segredos em linhas de comando de outros processos (--password=x, token=y,
# usuario:senha@host...). O que casar vira ***.
_SEGREDO_OPCAO = re.compile(
    r"(?i)((?:^|\s)--?[\w.-]*(?:pass(?:word|wd)?|senha|token|secret|segredo|api[-_]?key|apikey|auth|credential)[\w.-]*)(=|\s+)(\S+)"
)
_SEGREDO_ATRIBUICAO = re.compile(
    r"(?i)\b([\w.-]*(?:pass(?:word|wd)?|senha|token|secret|segredo|api[-_]?key|apikey)[\w.-]*=)(\S+)"
)
_SEGREDO_URL = re.compile(r"(?i)\b([a-z][a-z0-9+.-]*://)[^/\s:@]+:[^/\s@]+@")


class ErroSistema(Exception):
    """Falha esperada (comando ausente, sem permissão...): vai para o modelo."""


def ocultar_segredos(texto: str) -> str:
    texto = _SEGREDO_URL.sub(r"\1***@", texto)
    texto = _SEGREDO_OPCAO.sub(lambda m: f"{m.group(1)}{m.group(2)}***", texto)
    return _SEGREDO_ATRIBUICAO.sub(lambda m: f"{m.group(1)}***", texto)


def _usuario(uid: int, cache: dict[int, str] = {}) -> str:
    if uid not in cache:
        try:
            cache[uid] = pwd.getpwuid(uid).pw_name
        except KeyError:
            cache[uid] = str(uid)
    return cache[uid]


# ---------------------------------------------------------------------------
# Memória, carga, disco.
# ---------------------------------------------------------------------------


def memoria() -> dict[str, int]:
    valores = {}
    for linha in (PROC / "meminfo").read_text().splitlines():
        nome, _, resto = linha.partition(":")
        partes = resto.split()
        if partes:
            valores[nome] = int(partes[0]) * 1024
    return {
        "total": valores.get("MemTotal", 0),
        "disponivel": valores.get("MemAvailable", 0),
        "swap_total": valores.get("SwapTotal", 0),
        "swap_livre": valores.get("SwapFree", 0),
    }


def carga() -> dict[str, float]:
    um, cinco, quinze = (float(x) for x in (PROC / "loadavg").read_text().split()[:3])
    ligado = float((PROC / "uptime").read_text().split()[0])
    return {"1min": um, "5min": cinco, "15min": quinze, "cpus": os.cpu_count() or 1, "ligado_segundos": ligado}


def _desescapar_montagem(caminho: str) -> str:
    # /proc/self/mounts escreve espaço como \040, tab como \011 etc.
    return re.sub(r"\\([0-7]{3})", lambda m: chr(int(m.group(1), 8)), caminho)


def discos(raiz_projeto: Path | None = None) -> list[dict]:
    """Uso de cada sistema de arquivos de verdade (um por dispositivo)."""
    dispositivo_projeto = None
    if raiz_projeto is not None:
        try:
            dispositivo_projeto = os.stat(raiz_projeto).st_dev
        except OSError:
            pass
    vistos, lista = set(), []
    for linha in (PROC / "self" / "mounts").read_text().splitlines():
        partes = linha.split()
        if len(partes) < 3:
            continue
        origem, ponto, tipo = partes[0], _desescapar_montagem(partes[1]), partes[2]
        # overlay é ruído (uma montagem por contêiner do Docker), menos quando é a raiz.
        if tipo in FS_VIRTUAIS and not (tipo == "overlay" and ponto == "/"):
            continue
        try:
            st = os.statvfs(ponto)
            dispositivo = os.stat(ponto).st_dev
        except OSError:
            continue
        if dispositivo in vistos or st.f_blocks == 0:
            continue
        vistos.add(dispositivo)
        total = st.f_blocks * st.f_frsize
        livre = st.f_bavail * st.f_frsize
        usado = (st.f_blocks - st.f_bfree) * st.f_frsize
        lista.append(
            {
                "ponto": ponto,
                "dispositivo": origem,
                "tipo": tipo,
                "total": total,
                "usado": usado,
                "livre": livre,
                "uso_pct": round(100 * usado / (usado + livre), 1) if usado + livre else 0.0,
                "inodes_uso_pct": round(100 * (st.f_files - st.f_ffree) / st.f_files, 1) if st.f_files else None,
                "projeto": dispositivo == dispositivo_projeto,
            }
        )
    return sorted(lista, key=lambda d: d["ponto"])


# ---------------------------------------------------------------------------
# Processos.
# ---------------------------------------------------------------------------


@dataclass
class Processo:
    pid: int
    usuario: str
    nome: str
    estado: str
    rss: int
    threads: int
    comando: str


def processos(maximo: int = 10, max_comando: int = 160) -> list[Processo]:
    """Os que mais usam memória (RSS). Linha de comando com segredos ocultos."""
    lista = []
    for entrada in PROC.iterdir():
        if not entrada.name.isdigit():
            continue
        try:
            status = (entrada / "status").read_text()
            bruto = (entrada / "cmdline").read_bytes()
        except OSError:
            continue  # acabou no meio da leitura
        campos = {}
        for linha in status.splitlines():
            nome, _, valor = linha.partition(":")
            campos[nome] = valor.strip()
        if "VmRSS" not in campos:
            continue  # thread do kernel
        rss = int(campos["VmRSS"].split()[0]) * 1024
        comando = bruto.rstrip(b"\0").replace(b"\0", b" ").decode("utf-8", errors="replace") or f"[{campos.get('Name', '?')}]"
        comando = ocultar_segredos(comando)
        if len(comando) > max_comando:
            comando = comando[:max_comando] + " …"
        lista.append(
            Processo(
                pid=int(entrada.name),
                usuario=_usuario(int(campos.get("Uid", "0").split()[0])),
                nome=campos.get("Name", "?"),
                estado=campos.get("State", "?").split()[0],
                rss=rss,
                threads=int(campos.get("Threads", "1")),
                comando=comando,
            )
        )
    lista.sort(key=lambda p: p.rss, reverse=True)
    return lista[:maximo]


# ---------------------------------------------------------------------------
# Portas.
# ---------------------------------------------------------------------------


def _endereco(hexa: str) -> str:
    """Endereço de /proc/net/tcp(6). O kernel escreve cada palavra de 32 bits
    como um inteiro na ordem da máquina ("0100007F" = 127.0.0.1 em x86 e
    arm64): voltar o inteiro para bytes na ordem da máquina dá a ordem de rede."""
    bruto = b"".join(struct.pack("=I", int(hexa[i : i + 8], 16)) for i in range(0, len(hexa), 8))
    return str(ipaddress.ip_address(bruto))


def _donos_dos_sockets() -> dict[int, tuple[int, str]]:
    """inode do socket → (pid, nome), só dos processos que dá para ler
    (os do próprio usuário; os dos outros precisam de root)."""
    donos = {}
    for entrada in PROC.iterdir():
        if not entrada.name.isdigit():
            continue
        try:
            fds = list((entrada / "fd").iterdir())
            nome = (entrada / "comm").read_text().strip()
        except OSError:
            continue
        for fd in fds:
            try:
                alvo = os.readlink(fd)
            except OSError:
                continue
            if alvo.startswith("socket:["):
                donos.setdefault(int(alvo[8:-1]), (int(entrada.name), nome))
    return donos


def portas(base: Path | None = None) -> list[dict]:
    """Portas TCP em LISTEN e UDP abertas, com o processo dono quando visível."""
    base = base or PROC / "net"
    donos = _donos_dos_sockets()
    lista, vistos = [], set()
    for arquivo, protocolo, estado_aberto in (
        ("tcp", "tcp", "0A"),
        ("tcp6", "tcp", "0A"),
        ("udp", "udp", "07"),
        ("udp6", "udp", "07"),
    ):
        try:
            linhas = (base / arquivo).read_text().splitlines()[1:]
        except OSError:
            continue
        for linha in linhas:
            campos = linha.split()
            if len(campos) < 10 or campos[3] != estado_aberto:
                continue
            local_hex, porta_hex = campos[1].split(":")
            endereco, porta = _endereco(local_hex), int(porta_hex, 16)
            chave = (protocolo, endereco, porta)
            if chave in vistos:
                continue
            vistos.add(chave)
            ip = ipaddress.ip_address(endereco)
            dono = donos.get(int(campos[9]))
            lista.append(
                {
                    "protocolo": protocolo,
                    "endereco": endereco,
                    "porta": porta,
                    "alcance": "todas as interfaces" if ip.is_unspecified else ("só local" if ip.is_loopback else "rede"),
                    "usuario": _usuario(int(campos[7])),
                    "pid": dono[0] if dono else None,
                    "processo": dono[1] if dono else None,
                }
            )
    return sorted(lista, key=lambda p: (p["protocolo"], p["porta"], p["endereco"]))


# ---------------------------------------------------------------------------
# systemd (só leitura).
# ---------------------------------------------------------------------------


def _ambiente_dos_comandos() -> dict[str, str]:
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "LANG": "C.UTF-8",
        "SYSTEMD_PAGER": "cat",
        "SYSTEMD_COLORS": "0",
        "SYSTEMD_URLIFY": "0",
    }
    if os.environ.get("TZ"):
        env["TZ"] = os.environ["TZ"]
    return env


async def _rodar(argv: list[str], timeout: int) -> tuple[int, str, str]:
    try:
        with anyio.fail_after(timeout):
            r = await anyio.run_process(argv, check=False, env=_ambiente_dos_comandos(), stdin=None)
    except TimeoutError:
        raise ErroSistema(f"{Path(argv[0]).name} não respondeu em {timeout} s") from None
    except OSError as e:
        raise ErroSistema(f"não consegui executar {argv[0]}: {e}") from None
    return r.returncode, r.stdout.decode("utf-8", errors="replace"), r.stderr.decode("utf-8", errors="replace")


async def estado_dos_servicos(systemctl: str | None, unidades: tuple[str, ...], timeout: int) -> list[dict]:
    if not systemctl:
        raise ErroSistema("systemctl não encontrado: esta máquina não usa systemd?")
    codigo, saida, erro = await _rodar(
        [systemctl, "show", "--no-pager", "--property=" + ",".join(PROPRIEDADES), "--", *unidades], timeout
    )
    if codigo != 0 and not saida.strip():
        raise ErroSistema(f"systemctl show falhou: {erro.strip() or f'código {codigo}'}")
    blocos = [b for b in saida.strip().split("\n\n") if b.strip()]
    estados = []
    for bloco in blocos:
        campos = {}
        for linha in bloco.splitlines():
            nome, _, valor = linha.partition("=")
            campos[nome] = valor
        memoria_atual = campos.get("MemoryCurrent", "")
        estados.append(
            {
                "unidade": campos.get("Id", "?"),
                "descricao": campos.get("Description", ""),
                "carregada": campos.get("LoadState", ""),
                "ativa": campos.get("ActiveState", ""),
                "sub": campos.get("SubState", ""),
                "habilitada": campos.get("UnitFileState", ""),
                "pid": int(campos["MainPID"]) if campos.get("MainPID", "0").isdigit() and campos["MainPID"] != "0" else None,
                "desde": campos.get("ActiveEnterTimestamp") or None,
                "reinicios": int(campos["NRestarts"]) if campos.get("NRestarts", "").isdigit() else None,
                # 2^64-1 = "sem contabilidade de memória" no systemd.
                "memoria": int(memoria_atual) if memoria_atual.isdigit() and int(memoria_atual) < 2**63 else None,
                "resultado": campos.get("Result", ""),
            }
        )
    return estados


DICA_DIARIO = (
    "o usuário do daemon precisa estar no grupo adm ou systemd-journal para ler o diário do sistema "
    "(o dono configura: sudo usermod -aG systemd-journal <usuário>, e reinicia o daemon)"
)


async def diario(
    journalctl: str | None, unidade: str, linhas: int, prioridade: str, timeout: int
) -> tuple[list[str], str | None]:
    """(linhas, aviso). Sem --quiet de propósito: é ele que esconderia o aviso
    de que o usuário não enxerga o diário do sistema."""
    if not journalctl:
        raise ErroSistema("journalctl não encontrado: esta máquina não usa systemd?")
    argv = [journalctl, "--no-pager", "--output=short-iso", f"--lines={linhas}", f"--unit={unidade}"]
    if prioridade:
        argv.append(f"--priority={prioridade}")
    codigo, saida, erro = await _rodar(argv, timeout)
    sem_permissao = any(t in erro for t in ("insufficient permissions", "not seeing messages", "ermission denied"))
    if codigo != 0:
        raise ErroSistema(f"journalctl falhou: {erro.strip() or f'código {codigo}'}" + (f"; {DICA_DIARIO}" if sem_permissao else ""))
    # "-- No entries --", "-- Boot ... --": avisos do journalctl, não do serviço.
    registros = [ocultar_segredos(l) for l in saida.splitlines() if l.strip() and not l.startswith("-- ")]
    if sem_permissao and not registros:
        raise ErroSistema(f"sem permissão para ler o diário de {unidade}: {DICA_DIARIO}")
    return registros, (f"pode faltar parte do diário: {DICA_DIARIO}" if sem_permissao else None)
