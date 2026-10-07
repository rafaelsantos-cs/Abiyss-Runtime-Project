"""A caixa de verdade (bwrap). Pulados, com o motivo, onde o bwrap não roda
(sem bwrap, sem namespaces de usuário ou rodando como root)."""

import os
import signal
import subprocess
import sys
import textwrap
import time
from pathlib import Path

import anyio
import pytest

import isolamento
from conftest import PASTA_SERVIDOR, SEGREDOS, exigir_caixa

pytestmark = pytest.mark.anyio


async def rodar(cfg, comando, timeout=20, pasta=""):
    amb = exigir_caixa(cfg)
    return await isolamento.Executor(cfg, amb).executar(comando, timeout, pasta)


def processos_com(marca: str, exato: bool = False) -> list[int]:
    """PIDs cuja linha de comando contém `marca` (ou é exatamente `marca`).
    Sem `exato`, pega também o bwrap, que leva o comando nos argumentos."""
    achados = []
    for nome in os.listdir("/proc"):
        if nome.isdigit():
            try:
                linha = Path(f"/proc/{nome}/cmdline").read_bytes().rstrip(b"\0").replace(b"\0", b" ").decode()
            except OSError:
                continue
            if (linha == marca) if exato else (marca in linha):
                achados.append(int(nome))
    return achados


async def test_codigo_de_saida_stdout_e_stderr(fazer_config):
    r = await rodar(fazer_config(), "echo saida; echo erro >&2; exit 3")
    assert (r.codigo_saida, r.stdout, r.stderr) == (3, "saida\n", "erro\n")
    assert r.interrupcao is None and not r.stdout_cortado


async def test_workspace_gravavel_persiste_e_e_a_pasta_atual(fazer_config):
    cfg = fazer_config()
    (cfg.workspace / "sub").mkdir()
    r = await rodar(cfg, "pwd; echo dado > arquivo.txt", pasta="sub")
    assert r.stdout == "/workspace/sub\n", r.stderr
    assert (cfg.workspace / "sub" / "arquivo.txt").read_text() == "dado\n"


async def test_sistema_so_leitura(fazer_config):
    r = await rodar(fazer_config(), "touch /usr/x; echo $?; touch /etc/x; echo $?; ls /usr/bin/bash")
    linhas = r.stdout.split()
    assert linhas[0] != "0" and linhas[1] != "0", r.stderr
    assert "Read-only file system" in r.stderr
    assert linhas[2] == "/usr/bin/bash"


async def test_raiz_da_caixa_so_tem_o_sistema_e_o_workspace(fazer_config):
    r = await rodar(fazer_config(), "ls -A /")
    esperadas = {"usr", "etc", "bin", "sbin", "lib", "lib32", "lib64", "libx32", "proc", "dev", "tmp", "workspace"}
    assert set(r.stdout.split()) <= esperadas, r.stdout
    assert {"usr", "etc", "workspace", "tmp", "proc", "dev"} <= set(r.stdout.split())


async def test_areas_protegidas_e_o_projeto_nao_existem_na_caixa(fazer_config, projeto):
    cfg = fazer_config()
    caminhos = [projeto / relativo for relativo in SEGREDOS]
    caminhos += [projeto, projeto / "abiyss.toml", PASTA_SERVIDOR.parent.parent.parent, Path.home()]
    script = "; ".join(f"test -e '{c}' && echo VISIVEL:{c}" for c in caminhos) + "; echo fim"
    r = await rodar(cfg, script)
    assert r.stdout == "fim\n", r.stdout
    # E nenhum segredo aparece em lugar nenhum que a caixa enxerga fora do sistema.
    r = await rodar(cfg, "grep -rs segredo- /workspace /tmp /proc/self/environ; echo fim")
    assert r.stdout == "fim\n"


async def test_sem_rede_por_padrao(fazer_config):
    r = await rodar(fazer_config(), "tail -n +3 /proc/net/dev | cut -d: -f1; exec 3<>/dev/tcp/1.1.1.1/53")
    assert r.stdout.split() == ["lo"], "só a interface de loopback"
    assert r.codigo_saida != 0 and "unreachable" in r.stderr.lower()


async def test_rede_ligada_quando_configurada(fazer_config):
    r = await rodar(fazer_config(TERMINAL_REDE="sim"), "tail -n +3 /proc/net/dev | cut -d: -f1")
    interfaces = set(r.stdout.split())
    if interfaces <= {"lo"}:
        pytest.skip("a máquina de teste não tem interface de rede além do loopback")
    assert len(interfaces) > 1


async def test_sem_privilegios_e_sem_setuid(fazer_config):
    r = await rodar(fazer_config(), "grep -E '^(NoNewPrivs|CapEff|CapPrm|CapBnd):' /proc/self/status")
    campos = dict(linha.split(":\t") for linha in r.stdout.strip().splitlines())
    assert campos["NoNewPrivs"] == "1"
    assert campos["CapEff"] == campos["CapPrm"] == "0000000000000000"


async def test_nova_sessao(fazer_config):
    cfg = fazer_config()

    async def procurar(resultado):
        for _ in range(50):
            # Só o comando dentro da caixa (o bwrap de fora fica na sessão do servidor).
            pids = processos_com("sleep 2.71", exato=True)
            if pids:
                resultado.extend(os.getsid(p) for p in pids)
                return
            await anyio.sleep(0.05)

    sessoes: list[int] = []
    async with anyio.create_task_group() as grupo:
        grupo.start_soon(procurar, sessoes)
        await rodar(cfg, "sleep 2.71")
    assert sessoes and os.getsid(0) not in sessoes


async def test_tempo_esgotado_mata_a_caixa_inteira(fazer_config):
    inicio = time.monotonic()
    r = await rodar(fazer_config(), "sleep 3021 & sleep 3022", timeout=1)
    assert time.monotonic() - inicio < 4
    assert r.interrupcao == "tempo" and r.codigo_saida is None
    await anyio.sleep(0.3)
    assert not processos_com("sleep 302"), "nada sobrevive ao comando"


async def test_processo_em_segundo_plano_nao_sobrevive(fazer_config):
    r = await rodar(fazer_config(), "sleep 3031 & echo pronto")
    assert r.stdout == "pronto\n" and r.codigo_saida == 0
    await anyio.sleep(0.3)
    assert not processos_com("sleep 3031")


async def test_limite_de_memoria_por_processo(fazer_config):
    r = await rodar(fazer_config(TERMINAL_MEMORIA_MB="64"), "python3 -c \"x = b'a' * (200 * 1024 * 1024)\"")
    assert r.codigo_saida != 0 and "MemoryError" in r.stderr


async def test_vigia_mata_quando_a_arvore_passa_do_limite(fazer_config):
    # Cada processo cabe no limite por processo, mas juntos passam dele.
    cfg = fazer_config(TERMINAL_MEMORIA_MB="128")
    filho = "import time; x = b'a' * (60 * 1024 * 1024); time.sleep(10)"
    r = await rodar(cfg, f'for i in 1 2 3 4; do python3 -c "{filho}" & done; wait', timeout=15)
    assert r.interrupcao == "memoria", r
    assert r.pico_memoria_mb > 128
    assert r.duracao_segundos < 8
    await anyio.sleep(0.3)
    assert not processos_com("time.sleep(10)")


async def test_limite_de_processos(fazer_config):
    cfg = fazer_config(TERMINAL_MAX_PROCESSOS="16")
    r = await rodar(cfg, "sh -c 'for i in $(seq 1 40); do sleep 2.13 & done; wait'", timeout=15)
    assert r.interrupcao == "processos" or "fork" in r.stderr.lower(), r
    await anyio.sleep(0.3)
    assert not processos_com("sleep 2.13")


async def test_saida_grande_e_cortada_com_marca(fazer_config):
    cfg = fazer_config(TERMINAL_MAX_SAIDA_BYTES="4096")
    r = await rodar(cfg, "seq 1 100000")
    assert r.stdout_cortado and r.codigo_saida == 0
    assert r.stdout_bytes == len("".join(f"{i}\n" for i in range(1, 100001)))
    assert r.stdout.startswith("1\n2\n3\n") and r.stdout.endswith("99999\n100000\n")
    assert "saída cortada pelo terminal" in r.stdout


async def test_saida_sem_fim_interrompe(fazer_config, monkeypatch):
    monkeypatch.setattr(isolamento, "LIMITE_LEITURA_BYTES", 2 * 1024 * 1024)
    r = await rodar(fazer_config(TERMINAL_MAX_SAIDA_BYTES="2048"), "yes", timeout=20)
    assert r.interrupcao == "saida"
    assert r.duracao_segundos < 10
    assert r.stdout_cortado and r.stdout.startswith("y\ny\n")


async def test_fila_e_ocupado(fazer_config):
    cfg = fazer_config(TERMINAL_MAX_CONCORRENTES="1")
    amb = exigir_caixa(cfg)
    executor = isolamento.Executor(cfg, amb)
    resultados = []

    async def um(comando, timeout):
        try:
            resultados.append(await executor.executar(comando, timeout, ""))
        except isolamento.Ocupado as e:
            resultados.append(e)

    async with anyio.create_task_group() as grupo:
        grupo.start_soon(um, "sleep 1.5; echo primeiro", 10)
        await anyio.sleep(0.2)
        grupo.start_soon(um, "echo segundo", 10)  # espera a vaga
        grupo.start_soon(um, "echo terceiro", 1)  # desiste antes
    ocupados = [r for r in resultados if isinstance(r, isolamento.Ocupado)]
    feitos = sorted(r.stdout for r in resultados if not isinstance(r, isolamento.Ocupado))
    assert feitos == ["primeiro\n", "segundo\n"]
    assert len(ocupados) == 1
    segundo = next(r for r in resultados if not isinstance(r, isolamento.Ocupado) and r.stdout == "segundo\n")
    assert segundo.espera_fila_segundos > 1


async def test_bwrap_antigo_com_tmp_em_disco(fazer_config, monkeypatch):
    cfg = fazer_config()
    amb = exigir_caixa(cfg)
    antigo = isolamento.Ambiente(
        bwrap=isolamento.Bwrap(amb.bwrap.caminho, amb.bwrap.versao, False, False),
        prlimit=amb.prlimit,
        shell=amb.shell,
        limitar_processos=amb.limitar_processos,
        sistema=amb.sistema,
    )
    r = await isolamento.Executor(cfg, antigo).executar("echo x > /tmp/a; cat /tmp/a; echo $HOME", 10, "")
    assert r.stdout == "x\n/tmp\n"
    assert not list(cfg.pasta_temporaria.iterdir()), "a pasta temporária do comando é apagada"


def test_morte_do_servidor_derruba_a_caixa(env_base, fazer_config):
    """Se o processo do servidor morrer (o kernel mata o grupo), nada da caixa fica."""
    exigir_caixa(fazer_config())
    script = textwrap.dedent(
        """
        import anyio, config, isolamento
        cfg = config.carregar()
        amb = isolamento.preparar(cfg)
        anyio.run(isolamento.Executor(cfg, amb).executar, "sleep 3041", 60, "")
        """
    )
    servidor = subprocess.Popen(
        [sys.executable, "-c", script], cwd=PASTA_SERVIDOR, env={**os.environ, **env_base}
    )
    try:
        for _ in range(100):
            if processos_com("sleep 3041"):
                break
            time.sleep(0.05)
        assert processos_com("sleep 3041"), "o comando não começou"
        servidor.send_signal(signal.SIGKILL)
        servidor.wait(timeout=5)
        for _ in range(40):
            if not processos_com("sleep 3041"):
                break
            time.sleep(0.05)
        assert not processos_com("sleep 3041"), "a caixa sobreviveu ao servidor"
    finally:
        servidor.kill()
