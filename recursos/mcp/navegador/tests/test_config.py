"""Configuração do navegador: prazos, memória e tamanhos que cabem no kernel."""

import shutil
import tomllib

import pytest
from mcp import Client

import config
import servidor
from conftest import PASTA_SERVIDOR, RAIZ_DO_REPOSITORIO, escrever_projeto


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


# -- o abiyss.toml versionado ------------------------------------------------------


def _servidores() -> list[dict]:
    return tomllib.loads((RAIZ_DO_REPOSITORIO / "abiyss.toml").read_text())["mcp"]["servidores"]


def test_abiyss_toml_do_repositorio_tem_o_navegador(tmp_path):
    """Registrado, desligado (512 MiB não cabem) e só leitura. Com o limite
    de memória do kernel aumentado como o README manda, sobe."""
    (tmp_path / "projeto").mkdir()
    copia = tmp_path / "projeto" / "abiyss.toml"
    shutil.copy(RAIZ_DO_REPOSITORIO / "abiyss.toml", copia)
    dados = tomllib.loads(copia.read_text())
    item = next(s for s in dados["mcp"]["servidores"] if s["nome"] == "navegador")
    assert item["diretorio"] == "recursos/mcp/navegador" and "--no-dev" in item["args"]
    assert item["ativo"] is False
    assert dados["navegador"]["interagir"] is False
    assert "NAVEGADOR_EXCECOES_REDE_LOCAL" not in item["env"]
    env = {"ABIYSS_CONFIG": str(copia), "ABIYSS_MCP_NOME": "navegador"}
    with pytest.raises(config.ErroConfig, match=r"max_memoria_mb \(512\)"):
        config.carregar(env)
    copia.write_text(copia.read_text().replace("max_memoria_mb = 512", "max_memoria_mb = 1024"))
    cfg = config.carregar(env)
    assert cfg.interagir is False and cfg.excecoes_rede == frozenset() and cfg.ao_vivo_porta == 0
    assert cfg.prazo_segundos + config.FOLGA_PRAZO_SEGUNDOS <= cfg.kernel.timeout_segundos
    assert cfg.workspace == tmp_path / "projeto" / "workspace"


def permitida(lista: list[str], nome: str) -> bool:
    """A regra do kernel (CaixaDeFerramentas::permitida): nome exato, ou
    prefixo quando o item termina em *."""
    return any(nome.startswith(p[:-1]) if p.endswith("*") else nome == p for p in lista)


def alcanca(lista: list[str], servidor: str) -> bool:
    """Algum item da lista deixa passar ALGUMA ferramenta do servidor? Um
    nome com o prefixo dele, ou um curinga que cobre o prefixo ("*", "nav*")."""
    prefixo = servidor + "__"
    return any(
        (p[:-1].startswith(prefixo) or prefixo.startswith(p[:-1])) if p.endswith("*") else p.startswith(prefixo)
        for p in lista
    )


def test_alcanca_ve_nome_exato_e_curinga_largo():
    assert alcanca(["terminal__*"], "terminal") and alcanca(["terminal__executar"], "terminal")
    assert alcanca(["*"], "terminal") and alcanca(["term*"], "terminal")
    assert not alcanca(["navegador__*", "ler_arquivo"], "terminal")


@pytest.mark.anyio
async def test_navegador_so_no_ultra_e_nunca_com_o_terminal(fazer_config, anyio_backend):
    """Só o ultra dirige o Chromium. Nenhum nível tem o navegador e o
    terminal juntos: uma página mandaria rodar um programa sobre o workspace
    e levaria o resultado para fora (docs/LIMITES.md)."""
    dados = tomllib.loads((RAIZ_DO_REPOSITORIO / "abiyss.toml").read_text())
    meu = next(s["nome"] for s in _servidores() if s.get("diretorio") == "recursos/mcp/" + PASTA_SERVIDOR.name)
    terminal = next(s["nome"] for s in _servidores() if s.get("diretorio") == "recursos/mcp/terminal")
    async with Client(servidor.criar_servidor(fazer_config())[0]) as cliente:
        nomes = [meu + "__" + f.name for f in (await cliente.list_tools()).tools]
    assert len(nomes) == 9
    for nivel, lista in ((n, dados["subagentes"][n]["ferramentas"]) for n in ("ultra", "medium", "low")):
        for nome in nomes:
            assert permitida(lista, nome) is (nivel == "ultra"), (nivel, nome)
        if alcanca(lista, meu):
            assert not alcanca(lista, terminal), f"{nivel}: {meu} e {terminal} no mesmo nível"
