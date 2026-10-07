"""Configuração: limites do kernel lidos do abiyss.toml e as recusas."""

import shutil
import tomllib
from pathlib import Path

import pytest
from mcp import Client

import config
import servidor
from conftest import PASTA_SERVIDOR, escrever_projeto
from test_argumentos import ambiente_falso

RAIZ_DO_REPOSITORIO = PASTA_SERVIDOR.parent.parent.parent


def test_le_os_limites_do_kernel_do_proprio_item(fazer_config, projeto):
    cfg = fazer_config()
    assert cfg.kernel.timeout_segundos == 120
    assert cfg.kernel.max_memoria_mb == 512
    assert cfg.kernel.max_bytes_resultado == 1_000_000
    assert cfg.raiz == projeto.resolve()
    assert cfg.workspace == (projeto / "workspace").resolve()
    assert cfg.workspace.is_dir(), "o workspace é criado se não existir"
    # Padrões: cabem nos limites do kernel.
    assert cfg.timeout_maximo == 120 - config.FOLGA_TIMEOUT_SEGUNDOS
    assert cfg.timeout_padrao == 30
    assert cfg.rede is False
    assert not cfg.avisos


def test_areas_protegidas_como_no_kernel(fazer_config, projeto):
    cfg = fazer_config()
    raiz = projeto.resolve()
    for relativo in ("kernel", "recursos", ".git", ".env", "abiyss.toml", "Cargo.toml", "data",
                     "identity/nucleo.md", "identity", "cofre", "identity/memoria-central.md", "skills"):
        assert raiz / relativo in cfg.protegidos, relativo


def test_timeout_maior_que_o_do_kernel_recusa(fazer_config):
    with pytest.raises(config.ErroConfig, match="o kernel mataria o servidor no meio do comando"):
        fazer_config(TERMINAL_TIMEOUT_MAXIMO="116")
    # 115 + 5 de folga = 120: cabe exatamente.
    assert fazer_config(TERMINAL_TIMEOUT_MAXIMO="115").timeout_maximo == 115


def test_timeout_padrao_maior_que_o_maximo_recusa(fazer_config):
    with pytest.raises(config.ErroConfig, match="TERMINAL_TIMEOUT_PADRAO"):
        fazer_config(TERMINAL_TIMEOUT_PADRAO="60", TERMINAL_TIMEOUT_MAXIMO="50")


def test_memoria_dos_comandos_tem_de_caber_no_limite_do_servidor(fazer_config):
    # 3 × 160 + 128 = 608 > 512.
    with pytest.raises(config.ErroConfig, match="max_memoria_mb"):
        fazer_config(TERMINAL_MAX_CONCORRENTES="3")
    # 2 × 192 + 128 = 512: cabe exatamente.
    assert fazer_config(TERMINAL_MEMORIA_MB="192").memoria_mb == 192
    with pytest.raises(config.ErroConfig, match="max_memoria_mb"):
        fazer_config(TERMINAL_MEMORIA_MB="193")


def test_saida_tem_de_caber_no_corte_do_kernel(tmp_path, env_base):
    escrever_projeto(tmp_path / "pequeno", max_bytes_resultado=50_000)
    env = {**env_base, "ABIYSS_CONFIG": str(tmp_path / "pequeno" / "abiyss.toml")}
    with pytest.raises(config.ErroConfig, match="max_bytes_resultado"):
        config.carregar({**env, "TERMINAL_MAX_SAIDA_BYTES": "32000"})
    cfg = config.carregar({**env, "TERMINAL_MAX_SAIDA_BYTES": "20000"})
    assert 2 * cfg.max_saida_bytes + config.FOLGA_RESULTADO_BYTES <= 50_000


def test_valores_invalidos_no_env(fazer_config):
    with pytest.raises(config.ErroConfig, match="não é um número inteiro"):
        fazer_config(TERMINAL_MEMORIA_MB="muito")
    with pytest.raises(config.ErroConfig, match="o mínimo é"):
        fazer_config(TERMINAL_MAX_PROCESSOS="1")
    with pytest.raises(config.ErroConfig, match="use sim ou nao"):
        fazer_config(TERMINAL_REDE="talvez")
    assert fazer_config(TERMINAL_REDE="sim").rede is True


@pytest.mark.parametrize("workspace", [".", "data/ws", "identity", "skills/x", "cofre/ws", ".git/ws"])
def test_workspace_que_contem_ou_fica_em_area_protegida_recusa(tmp_path, env_base, workspace):
    escrever_projeto(tmp_path / "p", workspace=workspace)
    with pytest.raises(config.ErroConfig, match="workspace inválido"):
        config.carregar({**env_base, "ABIYSS_CONFIG": str(tmp_path / "p" / "abiyss.toml")})


def test_workspace_que_e_link_para_area_protegida_recusa(tmp_path, env_base):
    raiz = tmp_path / "p"
    escrever_projeto(raiz)
    (raiz / "workspace").symlink_to(raiz / "data")
    with pytest.raises(config.ErroConfig, match="workspace inválido"):
        config.carregar({**env_base, "ABIYSS_CONFIG": str(raiz / "abiyss.toml")})


def test_pasta_temporaria_dentro_de_area_protegida_recusa(fazer_config, projeto):
    with pytest.raises(config.ErroConfig, match="pasta temporária inválida"):
        fazer_config(TERMINAL_PASTA_TEMPORARIA=str(projeto / "data"))


def test_sem_item_no_abiyss_toml_usa_o_padrao_do_kernel_com_aviso(tmp_path, env_base):
    raiz = tmp_path / "p"
    escrever_projeto(raiz)
    toml = raiz / "abiyss.toml"
    toml.write_text(toml.read_text().split("[[mcp.servidores]]\nnome = \"terminal\"")[0])
    cfg = config.carregar({**env_base, "ABIYSS_CONFIG": str(toml)})
    assert cfg.kernel.timeout_segundos == config.PADRAO_KERNEL_TIMEOUT
    assert cfg.avisos and "não está em [[mcp.servidores]]" in cfg.avisos[0]


def test_nome_explicito(fazer_config):
    with pytest.raises(config.ErroConfig, match="não há servidor com esse nome"):
        fazer_config(ABIYSS_MCP_NOME="nao-existe")
    assert fazer_config(ABIYSS_MCP_NOME="terminal").kernel.timeout_segundos == 120


def test_conflito_de_montagem():
    raiz = Path("/p")
    protegidos = (Path("/p/kernel"), Path("/p/data"))
    assert config.conflito_de_montagem(Path("/p/workspace"), protegidos, raiz, gravavel=True) is None
    assert "contém /p" in config.conflito_de_montagem(Path("/"), protegidos, raiz, gravavel=False)
    assert "contém /p" in config.conflito_de_montagem(Path("/p"), protegidos, raiz, gravavel=True)
    assert "dentro da área" in config.conflito_de_montagem(Path("/p/data/x"), protegidos, raiz, gravavel=True)
    assert config.conflito_de_montagem(Path("/usr"), protegidos, raiz, gravavel=False) is None


def test_abiyss_toml_do_repositorio_tem_o_terminal_e_cabe_nos_limites(tmp_path):
    """O item do terminal no abiyss.toml versionado sobe com estes limites."""
    import tomllib

    (tmp_path / "projeto").mkdir()
    (tmp_path / "tmp").mkdir()
    copia = tmp_path / "projeto" / "abiyss.toml"
    shutil.copy(RAIZ_DO_REPOSITORIO / "abiyss.toml", copia)
    dados = tomllib.loads(copia.read_text())
    item = next(s for s in dados["mcp"]["servidores"] if s["nome"] == "terminal")
    assert item["diretorio"] == "recursos/mcp/terminal"
    assert "--no-dev" in item["args"]
    env = {
        "ABIYSS_CONFIG": str(copia),
        "ABIYSS_MCP_NOME": "terminal",
        "TERMINAL_PASTA_TEMPORARIA": str(tmp_path / "tmp"),
    }
    # Os valores da tabela env do item valem também sem o kernel no meio.
    cfg = config.carregar(env)
    assert cfg.rede is False
    assert cfg.timeout_maximo == int(item["env"]["TERMINAL_TIMEOUT_MAXIMO"])
    assert cfg.memoria_mb == int(item["env"]["TERMINAL_MEMORIA_MB"])
    assert cfg.timeout_maximo + config.FOLGA_TIMEOUT_SEGUNDOS <= cfg.kernel.timeout_segundos


def test_tabela_env_do_item_e_o_ambiente_tem_precedencia(tmp_path, env_base):
    raiz = tmp_path / "p"
    toml = escrever_projeto(raiz)
    toml.write_text(toml.read_text() + '\n[mcp.servidores.env]\nTERMINAL_MEMORIA_MB = "100"\nTERMINAL_REDE = "sim"\n')
    env = {**env_base, "ABIYSS_CONFIG": str(toml)}
    cfg = config.carregar(env)
    assert (cfg.memoria_mb, cfg.rede) == (100, True)
    assert config.carregar({**env, "TERMINAL_REDE": "nao"}).rede is False


def permitida(lista: list[str], nome: str) -> bool:
    """A regra do kernel (CaixaDeFerramentas::permitida): nome exato, ou
    prefixo quando o item termina em *."""
    return any(nome.startswith(p[:-1]) if p.endswith("*") else nome == p for p in lista)


@pytest.mark.anyio
async def test_subagentes_recebem_as_ferramentas(fazer_config, anyio_backend):
    """O abiyss.toml versionado dá as ferramentas deste servidor aos níveis
    de sub-agente certos (é por eles que o heartbeat usa as mãos)."""
    dados = tomllib.loads((RAIZ_DO_REPOSITORIO / "abiyss.toml").read_text())
    pasta = "recursos/mcp/" + PASTA_SERVIDOR.name
    meu = next(s["nome"] for s in dados["mcp"]["servidores"] if s.get("diretorio") == pasta)
    async with Client(servidor.criar_servidor(fazer_config(), ambiente_falso())) as cliente:
        nomes = [meu + "__" + f.name for f in (await cliente.list_tools()).tools]
    assert nomes
    # o low é só leitura: sem o terminal
    for nivel in ("ultra", "medium", "low"):
        lista = dados["subagentes"][nivel]["ferramentas"]
        for nome in nomes:
            assert permitida(lista, nome) is (nivel in ("ultra", "medium")), (nivel, nome)
