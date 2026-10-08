"""Configuração do navegador: prazos, memória e tamanhos que cabem no kernel."""

import pytest

import config
from conftest import escrever_projeto


def test_padroes_cabem_no_kernel(fazer_config, tmp_path):
    cfg = fazer_config()
    assert cfg.kernel.timeout_segundos == 90
    assert cfg.prazo_segundos == 90 - config.FOLGA_PRAZO_SEGUNDOS
    assert cfg.timeout_carregamento + config.FOLGA_CARREGAMENTO_SEGUNDOS <= cfg.prazo_segundos
    assert cfg.interagir is False, "só leitura é o padrão"
    assert cfg.workspace == tmp_path / "projeto" / "ws"
    assert cfg.pasta_capturas == tmp_path / "projeto" / "ws" / "navegador"
    assert cfg.pasta_dados == tmp_path / "projeto" / "dados" / "navegador"
    assert cfg.sandbox == "auto" and cfg.chromium is None and cfg.ao_vivo_porta == 0
    assert cfg.excecoes_rede == frozenset() and cfg.avisos == ()
    assert cfg.memoria_chromium_mb + cfg.reserva_servidor_mb <= cfg.kernel.max_memoria_mb


def test_interagir_vem_do_abiyss_toml(tmp_path):
    for valor in (True, False):
        toml = escrever_projeto(tmp_path / str(valor), interagir=valor)
        assert config.carregar({"ABIYSS_CONFIG": str(toml)}).interagir is valor
    toml = escrever_projeto(tmp_path / "ruim")
    toml.write_text(toml.read_text() + '\n[navegador]\ninteragir = "sim"\n')
    with pytest.raises(config.ErroConfig, match="use true ou false"):
        config.carregar({"ABIYSS_CONFIG": str(toml)})


def test_interagir_nao_vem_do_ambiente(fazer_config):
    """O modo é do dono, no abiyss.toml; uma variável não o liga."""
    assert fazer_config(NAVEGADOR_INTERAGIR="sim").interagir is False


def test_prazo_maior_que_o_timeout_do_kernel_recusa(fazer_config):
    with pytest.raises(config.ErroConfig, match="o kernel mataria o servidor"):
        fazer_config(NAVEGADOR_PRAZO_SEGUNDOS="85")
    assert fazer_config(NAVEGADOR_PRAZO_SEGUNDOS="80").prazo_segundos == 80


def test_carregamento_menor_que_o_prazo(fazer_config):
    with pytest.raises(config.ErroConfig, match="NAVEGADOR_TIMEOUT_CARREGAMENTO"):
        fazer_config(NAVEGADOR_PRAZO_SEGUNDOS="30", NAVEGADOR_TIMEOUT_CARREGAMENTO="25")
    with pytest.raises(config.ErroConfig, match="NAVEGADOR_TIMEOUT_ACAO"):
        fazer_config(NAVEGADOR_TIMEOUT_CARREGAMENTO="10", NAVEGADOR_TIMEOUT_ACAO="20")


def test_texto_tem_de_caber_no_corte_do_kernel(tmp_path):
    toml = escrever_projeto(tmp_path / "p", max_bytes_resultado=30_000)
    with pytest.raises(config.ErroConfig, match="max_bytes_resultado"):
        config.carregar({"ABIYSS_CONFIG": str(toml)})
    cfg = config.carregar({"ABIYSS_CONFIG": str(toml), "NAVEGADOR_MAX_TEXTO_BYTES": "20000"})
    assert cfg.max_texto_bytes == 20_000


def test_memoria_do_chromium_tem_de_caber(tmp_path):
    """Com o limite padrão do kernel (512 MiB) o navegador não sobe: só o
    uv, o Python e o driver do Playwright já passam de 200 MiB."""
    toml = escrever_projeto(tmp_path / "p", max_memoria_mb=512)
    with pytest.raises(config.ErroConfig, match="max_memoria_mb.*pelo menos 1024"):
        config.carregar({"ABIYSS_CONFIG": str(toml)})
    cfg = config.carregar({"ABIYSS_CONFIG": str(toml), "NAVEGADOR_MEMORIA_MB": "256"})
    assert cfg.memoria_chromium_mb == 256
    assert cfg.js_heap_mb == 85


def test_valores_invalidos(fazer_config, tmp_path):
    with pytest.raises(config.ErroConfig, match="não é um número"):
        fazer_config(NAVEGADOR_MAX_SESSOES="duas")
    with pytest.raises(config.ErroConfig, match="o máximo é 16"):
        fazer_config(NAVEGADOR_MAX_SESSOES="100")
    with pytest.raises(config.ErroConfig, match="auto, sim ou nao"):
        fazer_config(NAVEGADOR_SANDBOX="talvez")
    assert fazer_config(NAVEGADOR_SANDBOX="não").sandbox == "nao"
    with pytest.raises(config.ErroConfig, match="não existe"):
        fazer_config(NAVEGADOR_CHROMIUM=str(tmp_path / "chrome"))
    for ruim in ("localhost:80", "127.0.0.1", "10.0.0.0/8:80", "127.0.0.1:0"):
        with pytest.raises(config.ErroConfig, match="ip:porta"):
            fazer_config(NAVEGADOR_EXCECOES_REDE_LOCAL=ruim)


def test_excecoes_de_rede_sao_avisadas(fazer_config):
    cfg = fazer_config(NAVEGADOR_EXCECOES_REDE_LOCAL="127.0.0.1:8080, [::1]:9000")
    assert cfg.excecoes_rede == {("127.0.0.1", 8080), ("::1", 9000)}
    assert any("só para testes" in a for a in cfg.avisos)
