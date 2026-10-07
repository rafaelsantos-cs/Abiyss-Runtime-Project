"""Configuração do ambiente: lista de serviços e limites do kernel."""

import shutil
import tomllib

import pytest

import config
from conftest import PASTA_SERVIDOR, escrever_projeto

RAIZ_DO_REPOSITORIO = PASTA_SERVIDOR.parent.parent.parent


def test_lista_de_servicos(fazer_config):
    cfg = fazer_config(AMBIENTE_SERVICOS="abiyss, docker.service,,qmd.socket  backup.timer abiyss")
    assert cfg.servicos == ("abiyss.service", "docker.service", "qmd.socket", "backup.timer")


@pytest.mark.parametrize("ruim", ["abiyss;reboot", "../etc", "--all", "a b/c", "$(id)"])
def test_nome_de_servico_invalido_recusa(fazer_config, ruim):
    with pytest.raises(config.ErroConfig, match="não é um nome de unidade|está vazio"):
        fazer_config(AMBIENTE_SERVICOS=ruim)


def test_lista_vazia_recusa(fazer_config):
    with pytest.raises(config.ErroConfig, match="está vazio"):
        fazer_config(AMBIENTE_SERVICOS=" , ")


def test_limites_do_kernel(fazer_config, tmp_path, env_base):
    with pytest.raises(config.ErroConfig, match="timeout_segundos"):
        fazer_config(AMBIENTE_TIMEOUT_COMANDO="26")
    assert fazer_config(AMBIENTE_TIMEOUT_COMANDO="25").timeout_comando == 25
    toml = escrever_projeto(tmp_path / "pequeno", max_bytes_resultado=20_000)
    with pytest.raises(config.ErroConfig, match="max_bytes_resultado"):
        config.carregar({**env_base, "ABIYSS_CONFIG": str(toml)})
    with pytest.raises(config.ErroConfig, match="use de 1 a 500"):
        fazer_config(AMBIENTE_MAX_LINHAS_DIARIO="501")


def test_abiyss_toml_do_repositorio_tem_o_ambiente(tmp_path):
    (tmp_path / "projeto").mkdir()
    copia = tmp_path / "projeto" / "abiyss.toml"
    shutil.copy(RAIZ_DO_REPOSITORIO / "abiyss.toml", copia)
    item = next(s for s in tomllib.loads(copia.read_text())["mcp"]["servidores"] if s["nome"] == "ambiente")
    assert item["diretorio"] == "recursos/mcp/ambiente" and "--no-dev" in item["args"]
    cfg = config.carregar({"ABIYSS_CONFIG": str(copia), "ABIYSS_MCP_NOME": "ambiente"})
    assert "abiyss.service" in cfg.servicos
    assert cfg.timeout_comando + config.FOLGA_TIMEOUT_SEGUNDOS <= cfg.timeout_kernel
