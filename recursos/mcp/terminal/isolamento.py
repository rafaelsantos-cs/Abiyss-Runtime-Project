"""Caixa de areia do terminal: bubblewrap (bwrap) + limites por comando.

Cada comando roda num bwrap novo:

- namespaces novos de usuário, PID, IPC, UTS, cgroup e (por padrão) rede;
- só o workspace (gravável, em /workspace) e pastas do sistema (só
  leitura). kernel/, identity/, data/, skills/, .env, .git e o abiyss.toml
  nunca entram, nem o resto do projeto, nem a pasta pessoal;
- ``--die-with-parent`` (se o servidor morrer, a caixa morre junto),
  ``--new-session``, nenhuma capability e no_new_privs (setuid não eleva);
  o bwrap não pode ser setuid e o servidor não roda como root;
- limites do próprio comando (``prlimit`` dentro da caixa): memória por
  processo (RLIMIT_AS), número de processos (RLIMIT_NPROC, contado só
  dentro da caixa), tamanho de arquivo e nada de core dump;
- um vigia mede a memória da árvore toda a cada 0,2 s e mata a caixa que
  passar do limite: o kernel mede a árvore inteira do servidor e, se ela
  passar de ``[mcp] max_memoria_mb``, mata o servidor com tudo dentro.

Sem bwrap, ou sem namespaces de usuário, o servidor NÃO sobe: nunca há
plano B sem caixa.
"""

from __future__ import annotations

import codecs
import logging
import os
import posixpath
import re
import shutil
import stat
import subprocess
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath

import anyio
from anyio.abc import ByteReceiveStream, Process

from config import MIB, Config, conflito_de_montagem

log = logging.getLogger("terminal")

PONTO_WORKSPACE = "/workspace"
PATH_DA_CAIXA = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
# Pastas do sistema montadas só para leitura.
SISTEMA = ("/usr", "/etc")
# Em sistemas com /usr unificado, estas são links (/bin -> usr/bin).
LIGACOES_DA_RAIZ = ("/bin", "/sbin", "/lib", "/lib32", "/lib64", "/libx32")
# DNS do systemd-resolved (o /etc/resolv.conf aponta para cá): só com rede.
RESOLVED = "/run/systemd/resolve"
PREFIXO_TMP = "abiyss-terminal-"

INTERVALO_VIGIA = 0.2
# Se a espera por uma vaga deixou menos que isto do prazo, nem começa.
SOBRA_MINIMA = 0.25
# Depois que o processo acaba, quanto esperar os pipes fecharem.
PRAZO_ESVAZIAR = 2.0
# Acima disto (stdout + stderr) o comando é interrompido: o resto seria
# descartado de qualquer jeito, e ler sem parar só gastaria CPU.
LIMITE_LEITURA_BYTES = 64 * MIB
# O bwrap de fora e o init da caixa.
PROCESSOS_DO_BWRAP = 2
# RLIMIT_NPROC é contado por namespace de usuário desde o Linux 5.14; antes
# disso contaria todos os processos do usuário na máquina.
KERNEL_NPROC_POR_NAMESPACE = (5, 14)

MOTIVOS = {
    "tempo": "passou do tempo limite",
    "memoria": "passou do limite de memória",
    "processos": "passou do limite de processos",
    "saida": f"escreveu mais de {LIMITE_LEITURA_BYTES // MIB} MiB de saída",
}


class Recusa(Exception):
    """O ambiente não permite uma caixa de areia segura: o servidor não sobe."""


class Ocupado(Exception):
    """Todas as vagas de execução ocupadas até o prazo do comando."""


# ---------------------------------------------------------------------------
# O que existe na máquina (verificado uma vez, ao subir).
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Bwrap:
    caminho: str
    versao: str
    # --disable-userns (bwrap >= 0.8): a caixa não cria namespaces aninhados.
    desligar_userns: bool
    # --size antes de --tmpfs (bwrap >= 0.9): /tmp com tamanho máximo.
    tamanho_tmpfs: bool


@dataclass(frozen=True)
class Montagem:
    tipo: str  # "ro-bind" ou "symlink"
    origem: str
    destino: str


@dataclass(frozen=True)
class Ambiente:
    bwrap: Bwrap
    prlimit: str
    shell: str
    limitar_processos: bool
    sistema: tuple[Montagem, ...]


def diagnostico_userns() -> list[str]:
    """Os ajustes do kernel que costumam bloquear namespaces de usuário."""
    linhas = []
    for arquivo, leitura in (
        ("/proc/sys/user/max_user_namespaces", "0 = desligados"),
        ("/proc/sys/kernel/unprivileged_userns_clone", "0 = só root pode criar"),
        ("/proc/sys/kernel/apparmor_restrict_unprivileged_userns", "1 = o AppArmor bloqueia (padrão do Ubuntu 24.04)"),
    ):
        try:
            valor = Path(arquivo).read_text().strip()
        except OSError:
            continue
        linhas.append(f"  {arquivo} = {valor} ({leitura})")
    return linhas


DICA_USERNS = """\
Como liberar (Ubuntu): o bwrap precisa criar namespaces de usuário sem ser root.
  - Ubuntu 24.04+ (AppArmor): crie /etc/apparmor.d/bwrap com
        abi <abi/4.0>,
        include <tunables/global>
        profile bwrap /usr/bin/bwrap flags=(unconfined) {
          userns,
          include if exists <local/bwrap>
        }
    e rode: sudo apparmor_parser -r /etc/apparmor.d/bwrap
    (alternativa para a máquina toda: sudo sysctl -w kernel.apparmor_restrict_unprivileged_userns=0)
  - user.max_user_namespaces = 0: sudo sysctl -w user.max_user_namespaces=15000
  - kernel.unprivileged_userns_clone = 0: sudo sysctl -w kernel.unprivileged_userns_clone=1
Para não perder no reboot, ponha a linha do sysctl em /etc/sysctl.d/60-abiyss-userns.conf."""


def versao_do_kernel() -> tuple[int, int]:
    numeros = re.match(r"(\d+)\.(\d+)", os.uname().release)
    return (int(numeros.group(1)), int(numeros.group(2))) if numeros else (0, 0)


def inspecionar_bwrap(cfg: Config) -> Bwrap:
    caminho = cfg.bwrap or shutil.which("bwrap")
    if not caminho or not os.path.isfile(caminho):
        raise Recusa(
            "bwrap (bubblewrap) não encontrado"
            + (f" em {cfg.bwrap}" if cfg.bwrap else " no PATH")
            + ". Instale com: sudo apt install bubblewrap"
        )
    caminho = os.path.realpath(caminho)
    if os.stat(caminho).st_mode & stat.S_ISUID:
        raise Recusa(
            f"{caminho} é setuid root. Este servidor só usa o bwrap sem setuid (namespaces de "
            "usuário): um bwrap setuid roda com privilégios e não é o modelo de segurança "
            "esperado. Use o pacote da distribuição (sudo apt install bubblewrap)"
        )
    try:
        versao = subprocess.run([caminho, "--version"], capture_output=True, text=True, timeout=10)
        ajuda = subprocess.run([caminho, "--help"], capture_output=True, text=True, timeout=10)
    except (OSError, subprocess.SubprocessError) as e:
        raise Recusa(f"não consegui executar {caminho}: {e}") from None
    texto_ajuda = ajuda.stdout + ajuda.stderr
    return Bwrap(
        caminho=caminho,
        versao=(versao.stdout or versao.stderr).strip(),
        desligar_userns="--disable-userns" in texto_ajuda,
        tamanho_tmpfs="--size" in texto_ajuda,
    )


def montagens_do_sistema(rede: bool) -> tuple[Montagem, ...]:
    lista = [Montagem("ro-bind", pasta, pasta) for pasta in SISTEMA]
    for caminho in LIGACOES_DA_RAIZ:
        if os.path.islink(caminho):
            lista.append(Montagem("symlink", os.readlink(caminho), caminho))
        elif os.path.isdir(caminho):
            lista.append(Montagem("ro-bind", caminho, caminho))
    if rede and os.path.isdir(RESOLVED):
        lista.append(Montagem("ro-bind", RESOLVED, RESOLVED))
    return tuple(lista)


def _programa_do_sistema(nome: str, sistema: tuple[Montagem, ...]) -> str | None:
    """Caminho de um programa que existe DENTRO da caixa (numa pasta montada)."""
    achado = shutil.which(nome, path=PATH_DA_CAIXA)
    if not achado:
        return None
    raizes = [m.destino for m in sistema]
    if any(achado == r or achado.startswith(r + "/") for r in raizes):
        return achado
    return None


def preparar(cfg: Config) -> Ambiente:
    """Confere a máquina e monta o plano da caixa. `Recusa` = não sobe."""
    bwrap = inspecionar_bwrap(cfg)
    if os.geteuid() == 0:
        raise Recusa(
            "o terminal não roda como root: mesmo sem capabilities, a caixa seria dona dos "
            "arquivos do root e leria os que só ele lê (ex.: /etc/ssh/ssh_host_*_key). Rode o "
            "daemon como usuário comum (o deploy/abiyss.service usa User=ubuntu)"
        )
    sistema = montagens_do_sistema(cfg.rede)
    for montagem in sistema:
        if montagem.tipo != "ro-bind":
            continue
        problema = conflito_de_montagem(Path(montagem.origem), cfg.protegidos, cfg.raiz, gravavel=False)
        if problema:
            raise Recusa(f"a pasta do sistema {montagem.origem} não pode entrar na caixa: {problema}")
    prlimit = _programa_do_sistema("prlimit", sistema)
    if not prlimit:
        raise Recusa("prlimit (util-linux) não encontrado em /usr: sudo apt install util-linux")
    shell = _programa_do_sistema("bash", sistema) or _programa_do_sistema("sh", sistema)
    if not shell:
        raise Recusa("nenhum shell (bash ou sh) em /usr")
    limitar = versao_do_kernel() >= KERNEL_NPROC_POR_NAMESPACE
    if not limitar:
        log.warning(
            "Linux %s: RLIMIT_NPROC não é contado por caixa antes do 5.14; o limite de "
            "processos fica só com o vigia",
            os.uname().release,
        )
    ambiente = Ambiente(bwrap=bwrap, prlimit=prlimit, shell=shell, limitar_processos=limitar, sistema=sistema)
    testar_caixa(cfg, ambiente)
    return ambiente


def testar_caixa(cfg: Config, amb: Ambiente) -> None:
    """Roda um comando de verdade na caixa, com os limites de produção."""
    pasta_tmp = None if amb.bwrap.tamanho_tmpfs else Path(tempfile.mkdtemp(prefix=PREFIXO_TMP, dir=cfg.pasta_temporaria))
    try:
        argv = montar_argumentos(cfg, amb, "echo caixa-ok | cat", "", pasta_tmp)
        try:
            r = subprocess.run(
                argv,
                stdin=subprocess.DEVNULL,
                capture_output=True,
                text=True,
                timeout=20,
                env=ambiente_da_caixa(cfg, amb),
            )
        except (OSError, subprocess.SubprocessError) as e:
            raise Recusa(f"o bwrap não rodou: {e}") from None
    finally:
        if pasta_tmp:
            remover_pasta(pasta_tmp)
    if r.returncode == 0 and r.stdout.strip() == "caixa-ok":
        return
    erro = (r.stderr or r.stdout).strip() or f"código de saída {r.returncode}"
    partes = [f"o teste da caixa de areia falhou ({amb.bwrap.versao}): {erro}"]
    if re.search(r"namespace|uid map|Operation not permitted|Permission denied", erro, re.I):
        partes.append("Parece que namespaces de usuário estão bloqueados nesta máquina:")
        partes += diagnostico_userns() or ["  (não consegui ler os ajustes em /proc/sys)"]
        partes.append(DICA_USERNS)
    raise Recusa("\n".join(partes))


# ---------------------------------------------------------------------------
# Montagem do comando.
# ---------------------------------------------------------------------------


def validar_pasta(cfg: Config, pasta: str) -> str:
    """Subpasta do workspace (relativa, sem '..') onde o comando começa.
    Devolve o caminho normalizado ('' = a raiz do workspace)."""
    pasta = (pasta or "").strip()
    if "\0" in pasta:
        raise ValueError("pasta com caractere nulo")
    if not pasta or pasta == ".":
        return ""
    relativo = PurePosixPath(pasta)
    if relativo.is_absolute():
        raise ValueError("use um caminho relativo ao workspace (sem / no começo)")
    if ".." in relativo.parts:
        raise ValueError("'..' não é permitido: a pasta tem de ficar dentro do workspace")
    alvo = Path(os.path.realpath(cfg.workspace / relativo))
    if not (alvo == cfg.workspace or cfg.workspace in alvo.parents):
        raise ValueError("a pasta sai do workspace (link simbólico?)")
    if not alvo.is_dir():
        raise ValueError(f"a pasta {pasta} não existe no workspace")
    return relativo.as_posix()


def montar_argumentos(cfg: Config, amb: Ambiente, comando: str, pasta: str, pasta_tmp: Path | None) -> list[str]:
    """A linha de comando inteira: bwrap + prlimit + shell."""
    argv = [
        amb.bwrap.caminho,
        "--unshare-all",
        # Explícito: sem namespace de usuário o bwrap falha (nunca roda "sem caixa").
        "--unshare-user",
        "--die-with-parent",
        "--new-session",
        "--cap-drop",
        "ALL",
        "--hostname",
        "abiyss-terminal",
    ]
    if amb.bwrap.desligar_userns:
        argv.append("--disable-userns")
    if cfg.rede:
        argv.append("--share-net")
    for m in amb.sistema:
        argv += [f"--{m.tipo}", m.origem, m.destino]
    argv += ["--proc", "/proc", "--dev", "/dev"]
    if pasta_tmp is None:
        argv += ["--size", str(cfg.tmp_mb * MIB), "--tmpfs", "/tmp"]
    else:
        argv += ["--bind", str(pasta_tmp), "/tmp"]
    argv += ["--bind", str(cfg.workspace), PONTO_WORKSPACE]
    argv += ["--chdir", posixpath.join(PONTO_WORKSPACE, pasta) if pasta else PONTO_WORKSPACE]
    limites = [
        amb.prlimit,
        f"--as={cfg.memoria_mb * MIB}",
        f"--fsize={cfg.max_arquivo_mb * MIB}",
        "--core=0",
    ]
    if amb.limitar_processos:
        limites.append(f"--nproc={cfg.max_processos}")
    argv += ["--", *limites, "--", amb.shell, "-c", comando]
    return argv


def ambiente_da_caixa(cfg: Config, amb: Ambiente) -> dict[str, str]:
    """Variáveis de ambiente do comando: um conjunto mínimo, montado do zero."""
    env = {
        "PATH": PATH_DA_CAIXA,
        "HOME": "/tmp",
        "TMPDIR": "/tmp",
        "LANG": "C.UTF-8",
        "TERM": "dumb",
        "SHELL": amb.shell,
        "PAGER": "cat",
        "GIT_PAGER": "cat",
        "GIT_TERMINAL_PROMPT": "0",
    }
    if os.environ.get("TZ"):
        env["TZ"] = os.environ["TZ"]
    if cfg.rede:
        for nome in ("HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "no_proxy"):
            if os.environ.get(nome):
                env[nome] = os.environ[nome]
    return env


def remover_pasta(pasta: Path) -> None:
    """Apaga a pasta temporária de um comando, mesmo com permissões trocadas."""

    def destravar(funcao, caminho, _erro):
        try:
            os.chmod(os.path.dirname(caminho), 0o700)
            os.chmod(caminho, 0o700)
        except OSError:
            pass
        try:
            funcao(caminho)
        except OSError:
            pass

    shutil.rmtree(pasta, onerror=destravar)


def limpar_sobras(cfg: Config, idade_minima: float = 3600) -> None:
    """Pastas temporárias de execuções que morreram no meio (servidor morto)."""
    agora = time.time()
    for pasta in cfg.pasta_temporaria.glob(PREFIXO_TMP + "*"):
        try:
            if pasta.is_dir() and not pasta.is_symlink() and agora - pasta.stat().st_mtime > idade_minima:
                remover_pasta(pasta)
        except OSError:
            continue


# ---------------------------------------------------------------------------
# Saída: começo e fim de cada fluxo, com memória limitada.
# ---------------------------------------------------------------------------


def _decodificar(dados: bytes, final: bool = True) -> str:
    decodificador = codecs.getincrementaldecoder("utf-8")(errors="replace")
    return decodificador.decode(dados, final=final).replace("\x00", "␀")


def cortar_inicio(texto: str, max_bytes: int) -> str:
    """Os primeiros `max_bytes` bytes (UTF-8) do texto, sem quebrar caractere."""
    return texto.encode("utf-8")[:max_bytes].decode("utf-8", errors="ignore")


def cortar_fim(texto: str, max_bytes: int) -> str:
    """Os últimos `max_bytes` bytes (UTF-8) do texto, sem quebrar caractere."""
    dados = texto.encode("utf-8")
    return dados[max(0, len(dados) - max_bytes) :].decode("utf-8", errors="ignore")


class Captura:
    """Guarda o começo e o fim de um fluxo; o meio é contado e descartado."""

    def __init__(self, limite_bytes: int):
        self.limite = limite_bytes
        self._max_inicio = limite_bytes // 2
        self._max_fim = limite_bytes - self._max_inicio
        self._inicio = bytearray()
        self._fim = bytearray()
        self.total = 0

    def adicionar(self, dados: bytes) -> None:
        self.total += len(dados)
        falta = self._max_inicio - len(self._inicio)
        if falta > 0:
            self._inicio += dados[:falta]
            dados = dados[falta:]
        if dados:
            self._fim += dados
            excesso = len(self._fim) - self._max_fim
            if excesso > 0:
                del self._fim[:excesso]

    def texto(self) -> tuple[str, bool]:
        """(texto, cortado). O texto mostrado nunca passa de `limite` bytes."""
        if self.total <= self.limite:
            completo = _decodificar(bytes(self._inicio + self._fim))
            if len(completo.encode("utf-8")) <= self.limite:
                return completo, False
        fim = bytes(self._fim)
        # O fim pode começar no meio de um caractere: pula os bytes de continuação.
        pulo = 0
        while pulo < min(3, len(fim)) and fim[pulo] & 0xC0 == 0x80:
            pulo += 1
        cabeca = cortar_inicio(_decodificar(bytes(self._inicio), final=False), self._max_inicio)
        cauda = cortar_fim(_decodificar(fim[pulo:]), self._max_fim)
        marca = (
            f"\n[… saída cortada pelo terminal: {self.total} bytes no total; mostrando o começo "
            f"e o fim (até {self.limite} bytes) …]\n"
        )
        return cabeca + marca + cauda, True


# ---------------------------------------------------------------------------
# Árvore de processos (como o kernel mede: /proc, pai → filhos, VmRSS).
# ---------------------------------------------------------------------------


def arvore(pid: int) -> list[int]:
    filhos: dict[int, list[int]] = {}
    for nome in os.listdir("/proc"):
        if not nome.isdigit():
            continue
        try:
            with open(f"/proc/{nome}/stat", "rb") as f:
                stat_ = f.read()
        except OSError:
            continue
        # O nome do programa vem entre parênteses e pode ter espaços.
        depois = stat_[stat_.rfind(b")") + 2 :].split()
        if len(depois) > 1:
            filhos.setdefault(int(depois[1]), []).append(int(nome))
    todos, pendentes = [], [pid]
    while pendentes:
        atual = pendentes.pop()
        todos.append(atual)
        pendentes.extend(filhos.get(atual, ()))
    return todos


def rss_bytes(pid: int) -> int:
    try:
        with open(f"/proc/{pid}/status", "rb") as f:
            for linha in f:
                if linha.startswith(b"VmRSS:"):
                    return int(linha.split()[1]) * 1024
    except (OSError, ValueError, IndexError):
        pass
    return 0


# ---------------------------------------------------------------------------
# Execução.
# ---------------------------------------------------------------------------


@dataclass
class Resultado:
    codigo_saida: int | None
    stdout: str
    stderr: str
    stdout_bytes: int
    stderr_bytes: int
    stdout_cortado: bool
    stderr_cortado: bool
    duracao_segundos: float
    espera_fila_segundos: float
    timeout_segundos: int
    interrupcao: str | None
    pico_memoria_mb: float


@dataclass
class _Estado:
    processo: Process
    interrupcao: str | None = None
    lidos: int = 0
    pico_rss: int = 0
    leitores: int = 2
    leitores_terminaram: anyio.Event = field(default_factory=anyio.Event)

    def interromper(self, motivo: str) -> None:
        if self.interrupcao is None:
            self.interrupcao = motivo
        matar(self.processo)

    def leitor_terminou(self) -> None:
        self.leitores -= 1
        if self.leitores <= 0:
            self.leitores_terminaram.set()


def matar(processo: Process) -> None:
    """SIGKILL no bwrap de fora: --die-with-parent derruba o init da caixa e,
    com ele, o namespace de PID inteiro (inclusive processos em segundo plano)."""
    if processo.returncode is None:
        try:
            processo.kill()
        except ProcessLookupError:
            pass


class Executor:
    def __init__(self, cfg: Config, amb: Ambiente):
        self.cfg = cfg
        self.amb = amb
        self._vagas: anyio.CapacityLimiter | None = None

    def _limitador(self) -> anyio.CapacityLimiter:
        # Criado dentro do laço de eventos (na primeira chamada).
        if self._vagas is None:
            self._vagas = anyio.CapacityLimiter(self.cfg.max_concorrentes)
        return self._vagas

    async def executar(self, comando: str, timeout: int, pasta: str) -> Resultado:
        """Espera uma vaga e roda. O `timeout` cobre a espera E a execução,
        para a chamada inteira nunca passar do que o kernel espera."""
        inicio = anyio.current_time()
        vagas = self._limitador()
        with anyio.move_on_after(timeout) as espera:
            await vagas.acquire()
        if espera.cancelled_caught:
            raise Ocupado(
                f"{self.cfg.max_concorrentes} comando(s) já rodando e nenhuma vaga abriu em "
                f"{timeout} s; tente de novo daqui a pouco"
            )
        try:
            espera_fila = anyio.current_time() - inicio
            restante = timeout - espera_fila
            if restante < SOBRA_MINIMA:
                raise Ocupado("a espera por uma vaga consumiu o tempo do comando; tente de novo")
            return await self._rodar(comando, restante, pasta, espera_fila, timeout)
        finally:
            vagas.release()

    async def _rodar(self, comando: str, restante: float, pasta: str, espera_fila: float, timeout: int) -> Resultado:
        pasta_tmp = None
        if not self.amb.bwrap.tamanho_tmpfs:
            pasta_tmp = Path(tempfile.mkdtemp(prefix=PREFIXO_TMP, dir=self.cfg.pasta_temporaria))
        saida = Captura(self.cfg.max_saida_bytes)
        erro = Captura(self.cfg.max_saida_bytes)
        try:
            argv = montar_argumentos(self.cfg, self.amb, comando, pasta, pasta_tmp)
            inicio = anyio.current_time()
            processo = await anyio.open_process(
                argv,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=ambiente_da_caixa(self.cfg, self.amb),
            )
            estado = _Estado(processo)
            try:
                async with anyio.create_task_group() as grupo:
                    grupo.start_soon(self._ler, processo.stdout, saida, estado)
                    grupo.start_soon(self._ler, processo.stderr, erro, estado)
                    grupo.start_soon(self._vigiar, estado)
                    with anyio.move_on_after(restante):
                        await processo.wait()
                    if processo.returncode is None:
                        estado.interromper("tempo")
                        await processo.wait()
                    # Acabou: os leitores pegam o que sobrou nos pipes.
                    with anyio.move_on_after(PRAZO_ESVAZIAR):
                        await estado.leitores_terminaram.wait()
                    grupo.cancel_scope.cancel()
            finally:
                matar(processo)
                with anyio.CancelScope(shield=True):
                    await processo.aclose()
            duracao = anyio.current_time() - inicio
        finally:
            if pasta_tmp:
                remover_pasta(pasta_tmp)
        texto_saida, saida_cortada = saida.texto()
        texto_erro, erro_cortado = erro.texto()
        codigo = processo.returncode
        return Resultado(
            # Morto por nós (SIGKILL no bwrap): não há código do comando.
            codigo_saida=None if estado.interrupcao or codigo is None or codigo < 0 else codigo,
            stdout=texto_saida,
            stderr=texto_erro,
            stdout_bytes=saida.total,
            stderr_bytes=erro.total,
            stdout_cortado=saida_cortada,
            stderr_cortado=erro_cortado,
            duracao_segundos=round(duracao, 3),
            espera_fila_segundos=round(espera_fila, 3),
            timeout_segundos=timeout,
            interrupcao=estado.interrupcao,
            pico_memoria_mb=round(estado.pico_rss / MIB, 1),
        )

    async def _ler(self, fluxo: ByteReceiveStream | None, captura: Captura, estado: _Estado) -> None:
        try:
            if fluxo is None:
                return
            while True:
                dados = await fluxo.receive(65536)
                captura.adicionar(dados)
                estado.lidos += len(dados)
                if estado.lidos > LIMITE_LEITURA_BYTES:
                    estado.interromper("saida")
        except (anyio.EndOfStream, anyio.ClosedResourceError, anyio.BrokenResourceError):
            pass
        finally:
            estado.leitor_terminou()

    async def _vigiar(self, estado: _Estado) -> None:
        limite = self.cfg.memoria_mb * MIB
        max_processos = self.cfg.max_processos + PROCESSOS_DO_BWRAP
        while estado.processo.returncode is None:
            pids = arvore(estado.processo.pid)
            rss = sum(rss_bytes(p) for p in pids)
            estado.pico_rss = max(estado.pico_rss, rss)
            if rss > limite:
                estado.interromper("memoria")
                return
            if len(pids) > max_processos:
                estado.interromper("processos")
                return
            await anyio.sleep(INTERVALO_VIGIA)
