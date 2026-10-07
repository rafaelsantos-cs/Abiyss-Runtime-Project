"""Configuração do web_rapido: prazos e tamanhos que cabem no kernel."""

import shutil
import tomllib
from pathlib import Path

import pytest
from mcp import Client

import config
import servidor
from conftest import PASTA_SERVIDOR, escrever_projeto

RAIZ_DO_REPOSITORIO = PASTA_SERVIDOR.parent.parent.parent


def test_padroes_cabem_no_kernel(fazer_config, tmp_path):
    cfg = fazer_config()
    assert cfg.kernel.timeout_segundos == 60
    assert cfg.prazo_segundos == 60 - config.FOLGA_PRAZO_SEGUNDOS
    assert cfg.produto == "AbiyssBot"
    assert cfg.cache_arquivo == tmp_path / "cache" / "web.sqlite3"


def test_cache_fica_na_pasta_de_dados_por_padrao(env_base, tmp_path):
    env = {k: v for k, v in env_base.items() if k != "WEB_CACHE_ARQUIVO"}
    cfg = config.carregar(env)
    assert cfg.cache_arquivo == tmp_path / "projeto" / "data" / "web_rapido" / "cache.sqlite3"


def test_prazo_maior_que_o_timeout_do_kernel_recusa(fazer_config):
    with pytest.raises(config.ErroConfig, match="o kernel mataria o servidor"):
        fazer_config(WEB_PRAZO_SEGUNDOS="55")
    assert fazer_config(WEB_PRAZO_SEGUNDOS="52").prazo_segundos == 52


def test_timeout_de_requisicao_maior_que_o_prazo_recusa(fazer_config):
    with pytest.raises(config.ErroConfig, match="WEB_TIMEOUT_REQUISICAO"):
        fazer_config(WEB_PRAZO_SEGUNDOS="10", WEB_TIMEOUT_REQUISICAO="20")


def test_texto_tem_de_caber_no_corte_do_kernel(tmp_path, env_base):
    toml = escrever_projeto(tmp_path / "p", max_bytes_resultado=30_000)
    with pytest.raises(config.ErroConfig, match="max_bytes_resultado"):
        config.carregar({**env_base, "ABIYSS_CONFIG": str(toml)})
    cfg = config.carregar({**env_base, "ABIYSS_CONFIG": str(toml), "WEB_MAX_TEXTO_BYTES": "20000"})
    assert cfg.max_texto_bytes == 20_000


def test_memoria_das_extracoes_tem_de_caber(fazer_config):
    with pytest.raises(config.ErroConfig, match="max_memoria_mb"):
        fazer_config(WEB_MAX_CONCORRENTES="4", WEB_MAX_PAGINA_MB="5")


def test_valores_invalidos(fazer_config):
    with pytest.raises(config.ErroConfig, match="WEB_SEARXNG_URL"):
        fazer_config(WEB_SEARXNG_URL="localhost:8888")
    with pytest.raises(config.ErroConfig, match="não é um número"):
        fazer_config(WEB_CACHE_TTL_PAGINAS_HORAS="um dia")
    with pytest.raises(config.ErroConfig, match="use sim ou nao"):
        fazer_config(WEB_PERMITIR_REDE_LOCAL="talvez")
    assert fazer_config(WEB_INTERVALO_DOMINIO_SEGUNDOS="1,5").intervalo_dominio == 1.5


def test_abiyss_toml_do_repositorio_tem_o_web_rapido(tmp_path):
    (tmp_path / "projeto").mkdir()
    copia = tmp_path / "projeto" / "abiyss.toml"
    shutil.copy(RAIZ_DO_REPOSITORIO / "abiyss.toml", copia)
    item = next(s for s in tomllib.loads(copia.read_text())["mcp"]["servidores"] if s["nome"] == "web_rapido")
    assert item["diretorio"] == "recursos/mcp/web_rapido"
    assert "--no-dev" in item["args"]
    cfg = config.carregar({"ABIYSS_CONFIG": str(copia), "ABIYSS_MCP_NOME": "web_rapido"})
    assert cfg.permitir_rede_local is False
    assert cfg.searxng_url == item["env"]["WEB_SEARXNG_URL"]
    assert cfg.prazo_segundos + config.FOLGA_PRAZO_SEGUNDOS <= cfg.kernel.timeout_segundos
    assert cfg.cache_arquivo == tmp_path / "projeto" / "data" / "web_rapido" / "cache.sqlite3"


def permitida(lista: list[str], nome: str) -> bool:
    """A regra do kernel (CaixaDeFerramentas::permitida): nome exato, ou
    prefixo quando o item termina em *."""
    return any(nome.startswith(p[:-1]) if p.endswith("*") else nome == p for p in lista)


def alcanca(lista: list[str], servidor: str) -> bool:
    """Algum item da lista deixa passar ALGUMA ferramenta do servidor? Um
    nome com o prefixo dele, ou um curinga que cobre o prefixo ("*", "term*")."""
    prefixo = servidor + "__"
    return any(
        (p[:-1].startswith(prefixo) or prefixo.startswith(p[:-1])) if p.endswith("*") else p.startswith(prefixo)
        for p in lista
    )


def test_alcanca_ve_nome_exato_e_curinga_largo():
    assert alcanca(["terminal__*"], "terminal")
    assert alcanca(["terminal__executar"], "terminal")
    assert alcanca(["*"], "terminal") and alcanca(["term*"], "terminal")
    assert not alcanca(["web_rapido__*", "ler_arquivo"], "terminal")


@pytest.mark.anyio
async def test_subagentes_recebem_as_ferramentas(fazer_config, anyio_backend):
    """O abiyss.toml versionado dá as ferramentas deste servidor aos níveis
    de sub-agente certos (é por eles que o heartbeat usa as mãos)."""
    dados = tomllib.loads((RAIZ_DO_REPOSITORIO / "abiyss.toml").read_text())
    pasta = "recursos/mcp/" + PASTA_SERVIDOR.name
    meu = next(s["nome"] for s in dados["mcp"]["servidores"] if s.get("diretorio") == pasta)
    terminal = next(s["nome"] for s in dados["mcp"]["servidores"] if s.get("diretorio") == "recursos/mcp/terminal")
    async with Client(servidor.criar_servidor(fazer_config())[0]) as cliente:
        nomes = [meu + "__" + f.name for f in (await cliente.list_tools()).tools]
    assert nomes
    # A web fica com o ultra e o low; o medium (que roda comandos) não lê a web.
    for nivel in ("ultra", "medium", "low"):
        lista = dados["subagentes"][nivel]["ferramentas"]
        for nome in nomes:
            assert permitida(lista, nome) is (nivel in ("ultra", "low")), (nivel, nome)
        # Leitura da web e comandos nunca no mesmo nível: uma página poderia
        # mandar ler um arquivo (ou rodar um programa) e buscar uma URL do
        # atacante com o conteúdo (docs/LIMITES.md).
        if any(permitida(lista, nome) for nome in nomes):
            assert not alcanca(lista, terminal), f"{nivel}: {meu} e {terminal} no mesmo nível"
