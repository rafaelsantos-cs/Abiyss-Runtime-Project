"""A linha de comando do bwrap, montada sem executar nada."""

from pathlib import Path

import pytest

import config
import isolamento


def ambiente_falso(desligar_userns=True, tamanho_tmpfs=True, limitar_processos=True, rede=False):
    return isolamento.Ambiente(
        bwrap=isolamento.Bwrap("/usr/bin/bwrap", "bubblewrap 0.9.0", desligar_userns, tamanho_tmpfs),
        prlimit="/usr/bin/prlimit",
        shell="/usr/bin/bash",
        limitar_processos=limitar_processos,
        sistema=isolamento.montagens_do_sistema(rede),
    )


def opcoes(argv, nome):
    """Todos os argumentos que vêm logo depois de `nome` (até o '--')."""
    fim = argv.index("--")
    return [argv[i + 1 : i + 3] for i, a in enumerate(argv[:fim]) if a == nome]


def origens_montadas(argv):
    fim = argv.index("--")
    return [Path(argv[i + 1]) for i, a in enumerate(argv[:fim]) if a in ("--bind", "--ro-bind")]


def test_isolamento_completo_por_padrao(fazer_config):
    cfg = fazer_config()
    argv = isolamento.montar_argumentos(cfg, ambiente_falso(), "echo oi", "", None)
    for opcao in ("--unshare-all", "--unshare-user", "--die-with-parent", "--new-session", "--disable-userns"):
        assert opcao in argv, opcao
    assert argv[argv.index("--cap-drop") + 1] == "ALL"
    assert "--share-net" not in argv, "sem rede por padrão"
    # Só o workspace é gravável, montado em /workspace, e o comando começa lá.
    assert opcoes(argv, "--bind") == [[str(cfg.workspace), "/workspace"]]
    assert argv[argv.index("--chdir") + 1] == "/workspace"
    # /tmp é um tmpfs com tamanho máximo.
    i = argv.index("--size")
    assert argv[i + 1] == str(cfg.tmp_mb * config.MIB) and argv[i + 2 : i + 4] == ["--tmpfs", "/tmp"]
    # O sistema entra só para leitura.
    assert ["/usr", "/usr"] in opcoes(argv, "--ro-bind")
    assert ["/etc", "/etc"] in opcoes(argv, "--ro-bind")


def test_nada_do_projeto_alem_do_workspace_e_montado(fazer_config, projeto):
    cfg = fazer_config()
    argv = isolamento.montar_argumentos(cfg, ambiente_falso(rede=True), "true", "", None)
    raiz = projeto.resolve()
    for origem in origens_montadas(argv):
        origem = origem.resolve()
        if origem == cfg.workspace:
            continue
        assert raiz not in origem.parents and origem != raiz, origem
        assert not any(origem == p or origem in p.parents for p in [raiz, *cfg.protegidos]), origem
        assert origem in (Path(p).resolve() for p in ("/usr", "/etc", "/bin", "/sbin", "/lib", "/lib32",
                                                      "/lib64", "/libx32", isolamento.RESOLVED)), origem
    # Nenhum caminho de área protegida aparece em lugar nenhum da linha de comando.
    texto = " ".join(argv)
    for area in cfg.protegidos:
        assert str(area) not in texto, area


def test_limites_do_comando_dentro_da_caixa(fazer_config):
    cfg = fazer_config(TERMINAL_MEMORIA_MB="100", TERMINAL_MAX_PROCESSOS="20", TERMINAL_MAX_ARQUIVO_MB="7")
    argv = isolamento.montar_argumentos(cfg, ambiente_falso(), "ls -la", "", None)
    depois = argv[argv.index("--") + 1 :]
    assert depois[0] == "/usr/bin/prlimit"
    assert f"--as={100 * config.MIB}" in depois
    assert "--nproc=20" in depois
    assert f"--fsize={7 * config.MIB}" in depois
    assert "--core=0" in depois
    assert depois[-3:] == ["/usr/bin/bash", "-c", "ls -la"]


def test_sem_nproc_por_namespace_o_limite_fica_com_o_vigia(fazer_config):
    argv = isolamento.montar_argumentos(fazer_config(), ambiente_falso(limitar_processos=False), "x", "", None)
    assert not any(a.startswith("--nproc") for a in argv)


def test_rede_ligada(fazer_config):
    cfg = fazer_config(TERMINAL_REDE="sim")
    assert "--share-net" in isolamento.montar_argumentos(cfg, ambiente_falso(rede=True), "x", "", None)


def test_bwrap_antigo_usa_pasta_temporaria_em_disco(fazer_config, tmp_path):
    cfg = fazer_config()
    amb = ambiente_falso(desligar_userns=False, tamanho_tmpfs=False)
    argv = isolamento.montar_argumentos(cfg, amb, "x", "sub", tmp_path / "t")
    assert "--disable-userns" not in argv and "--size" not in argv
    assert [str(tmp_path / "t"), "/tmp"] in opcoes(argv, "--bind")
    assert argv[argv.index("--chdir") + 1] == "/workspace/sub"


def test_validar_pasta(fazer_config):
    cfg = fazer_config()
    (cfg.workspace / "a" / "b").mkdir(parents=True)
    assert isolamento.validar_pasta(cfg, "") == ""
    assert isolamento.validar_pasta(cfg, ".") == ""
    assert isolamento.validar_pasta(cfg, "a/./b/") == "a/b"
    for ruim, motivo in (
        ("/etc", "relativo"),
        ("../projeto", "'..'"),
        ("a/../../x", "'..'"),
        ("nao-existe", "não existe"),
    ):
        with pytest.raises(ValueError, match=motivo):
            isolamento.validar_pasta(cfg, ruim)


def test_validar_pasta_recusa_link_para_fora(fazer_config, projeto):
    cfg = fazer_config()
    (cfg.workspace / "atalho").symlink_to(projeto / "data")
    with pytest.raises(ValueError, match="sai do workspace"):
        isolamento.validar_pasta(cfg, "atalho")


def test_ambiente_do_comando_e_minimo(fazer_config, monkeypatch):
    monkeypatch.setenv("NIM_API_KEY", "nao-pode-vazar")
    monkeypatch.setenv("HTTPS_PROXY", "http://proxy:3128")
    cfg = fazer_config()
    env = isolamento.ambiente_da_caixa(cfg, ambiente_falso())
    assert env["HOME"] == "/tmp" and env["PATH"] == isolamento.PATH_DA_CAIXA
    assert "NIM_API_KEY" not in env
    assert "HTTPS_PROXY" not in env, "sem rede, nem o proxy"
    env = isolamento.ambiente_da_caixa(fazer_config(TERMINAL_REDE="sim"), ambiente_falso(rede=True))
    assert env["HTTPS_PROXY"] == "http://proxy:3128" and "NIM_API_KEY" not in env
