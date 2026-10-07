"""Coleta: rede interna, robots.txt, redirecionamentos, tamanho e prazos."""

import time

import anyio
import pytest

from cache import Cache
from coleta import Coletor, ErroColeta, LimitePorDominio, endereco_publico, normalizar_url

pytestmark = pytest.mark.anyio


def coletor(cfg):
    return Coletor(cfg, Cache(cfg.cache_arquivo, cfg.cache_max_mb * 1024 * 1024))


def prazo(segundos=20):
    return anyio.current_time() + segundos


def test_normalizar_url():
    assert normalizar_url(" HTTPS://Exemplo.COM/a?b=1#c ") == "https://exemplo.com/a?b=1"
    assert normalizar_url("http://exemplo.com") == "http://exemplo.com/"
    for ruim, motivo in (
        ("file:///etc/passwd", "só http e https"),
        ("ftp://x.com/a", "só http e https"),
        ("exemplo.com/a", "só http e https"),
        ("http:///semdominio", "sem domínio"),
        ("http://user:senha@x.com/", "usuário/senha"),
        ("http://x.com:99999/", "porta inválida"),
    ):
        with pytest.raises(ErroColeta, match=motivo):
            normalizar_url(ruim)


def test_enderecos_publicos():
    for interno in ("127.0.0.1", "10.0.0.5", "192.168.1.1", "172.16.0.1", "169.254.169.254", "100.64.0.1",
                    "0.0.0.0", "::1", "fe80::1", "fd00::1", "::ffff:127.0.0.1", "224.0.0.1"):
        assert not endereco_publico(interno), interno
    for publico in ("1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"):
        assert endereco_publico(publico), publico


async def test_rede_interna_bloqueada_antes_de_conectar(fazer_config, site):
    c = coletor(fazer_config(WEB_PERMITIR_REDE_LOCAL="nao"))
    for url in (f"{site.base}/artigo.html", f"http://localhost:{site.porta}/artigo.html",
                "http://169.254.169.254/latest/meta-data/", f"http://[::1]:{site.porta}/"):
        with pytest.raises(ErroColeta, match="endereço interno"):
            await c.baixar(normalizar_url(url), prazo())
    assert site.pedidos["/artigo.html"] == 0, "nem chegou a conectar"


async def test_endereco_conectado_tambem_e_conferido(fazer_config):
    c = coletor(fazer_config(WEB_PERMITIR_REDE_LOCAL="nao"))

    class Fluxo:
        def get_extra_info(self, nome):
            return ("127.0.0.1", 80) if nome == "server_addr" else None

    class Resposta:
        extensions = {"network_stream": Fluxo()}

    with pytest.raises(ErroColeta, match="endereço interno"):
        c._conferir_conexao(Resposta())


async def test_baixa_html_e_respeita_robots(fazer_config, site):
    c = coletor(fazer_config())
    d = await c.baixar(f"{site.base}/artigo.html", prazo())
    assert d.status == 200 and d.tipo == "text/html" and b"hipocampo" in d.conteudo and not d.cortado
    assert (await c.baixar(f"{site.base}/privado/publico.html", prazo())).status == 200
    for proibida in ("/privado/segredo.html", "/artigo.html?sessao=123"):
        with pytest.raises(ErroColeta, match="bloqueado pelo robots.txt"):
            await c.baixar(site.base + proibida, prazo())
    assert site.pedidos["/privado/segredo.html"] == 0
    assert site.pedidos["/robots.txt"] == 1, "o robots.txt vem do cache depois da primeira vez"


async def test_robots_com_erro_proibe_tudo(fazer_config, site_quebrado):
    c = coletor(fazer_config())
    with pytest.raises(ErroColeta, match="bloqueado pelo robots.txt"):
        await c.baixar(f"{site_quebrado.base}/artigo.html", prazo())
    assert site_quebrado.pedidos["/artigo.html"] == 0


async def test_redirecionamentos(fazer_config, site):
    c = coletor(fazer_config())
    d = await c.baixar(f"{site.base}/redireciona", prazo())
    assert d.url == f"{site.base}/artigo.html"
    with pytest.raises(ErroColeta, match="redirecionamentos demais"):
        await c.baixar(f"{site.base}/ciclo", prazo())


async def test_tamanho_maximo_corta_a_leitura(fazer_config, site):
    c = coletor(fazer_config(WEB_MAX_PAGINA_MB="1"))
    d = await c.baixar(f"{site.base}/grande.html", prazo())
    assert d.cortado and len(d.conteudo) == 1024 * 1024


async def test_erros_do_site(fazer_config, site):
    c = coletor(fazer_config(WEB_TIMEOUT_REQUISICAO="1"))
    with pytest.raises(ErroColeta, match="respondeu 500"):
        await c.baixar(f"{site.base}/erro", prazo())
    with pytest.raises(ErroColeta, match="tipo de conteúdo não suportado: application/pdf"):
        await c.baixar(f"{site.base}/arquivo.pdf", prazo())
    inicio = time.monotonic()
    with pytest.raises(ErroColeta, match="não respondeu a tempo"):
        await c.baixar(f"{site.base}/lento", prazo())
    assert time.monotonic() - inicio < 2.5


async def test_site_lento_dentro_do_timeout_configurado(fazer_config, site):
    c = coletor(fazer_config(WEB_TIMEOUT_REQUISICAO="8"))
    d = await c.baixar(f"{site.base}/muito-lento", prazo())
    assert d.status == 200


async def test_prazo_nosso_esgotado_nao_envenena_o_robots(fazer_config, site):
    c = coletor(fazer_config())
    site.robots_atraso = 2
    with pytest.raises(ErroColeta, match="prazo da chamada acabou"):
        await c.baixar(f"{site.base}/artigo.html", prazo(1))
    site.robots_atraso = 0
    d = await c.baixar(f"{site.base}/artigo.html", prazo())
    assert d.status == 200, "o robots.txt não ficou guardado como 'proibido'"
    assert site.pedidos["/robots.txt"] == 2


async def test_limite_por_dominio(fazer_config, site):
    c = coletor(fazer_config(WEB_INTERVALO_DOMINIO_SEGUNDOS="0.6"))
    inicio = time.monotonic()
    await c.baixar(f"{site.base}/artigo.html", prazo())
    await c.baixar(f"{site.base}/longo.html", prazo())
    await c.baixar(f"{site.base}/texto.txt", prazo())
    assert time.monotonic() - inicio >= 1.2, "três pedidos ao mesmo domínio: dois intervalos"


async def test_limite_por_dominio_desiste_se_passar_do_prazo():
    limite = LimitePorDominio()
    await limite.esperar("x.com", 30, prazo(60))
    with pytest.raises(ErroColeta, match="limite por domínio"):
        await limite.esperar("x.com", 30, prazo(5))
    await limite.esperar("y.com", 30, prazo(5))  # outro domínio: livre
