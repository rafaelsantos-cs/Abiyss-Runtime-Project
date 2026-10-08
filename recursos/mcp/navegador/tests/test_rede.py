"""Regras de rede sem navegador: endereços, URL, métodos e o filtro (proxy)."""

import ast
import asyncio
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

import rede
from conftest import PASTA_SERVIDOR
from rede import Bloqueio, Filtro, Rede, endereco_publico, politica, validar_url

pytestmark = pytest.mark.anyio


def _funcao(arquivo, nome):
    arvore = ast.parse(arquivo.read_text())
    no = next(n for n in arvore.body if isinstance(n, ast.FunctionDef) and n.name == nome)
    return ast.unparse(no)


def test_regra_de_endereco_e_a_mesma_do_web_rapido():
    """A regra de "endereço público" é copiada do web_rapido (projetos uv
    separados não importam um do outro): as duas não podem divergir."""
    web = PASTA_SERVIDOR.parent / "web_rapido" / "coleta.py"
    if not web.is_file():
        pytest.skip("web_rapido não está ao lado")
    assert _funcao(web, "endereco_publico") == _funcao(PASTA_SERVIDOR / "rede.py", "endereco_publico")


def test_enderecos_publicos():
    for interno in ("127.0.0.1", "10.0.0.5", "192.168.1.1", "172.16.0.1", "169.254.169.254", "100.64.0.1",
                    "0.0.0.0", "::1", "fe80::1", "fd00::1", "::ffff:127.0.0.1", "224.0.0.1", "lixo"):
        assert not endereco_publico(interno), interno
    for publico in ("1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"):
        assert endereco_publico(publico), publico


def test_validar_url():
    assert validar_url(" https://exemplo.com/a#b ") == "https://exemplo.com/a#b"
    for ruim, motivo in (
        ("file:///etc/passwd", "só http e https"),
        ("view-source:https://x.com", "só http e https"),
        ("javascript:alert(1)", "só http e https"),
        ("chrome://settings", "só http e https"),
        ("ftp://x.com/a", "só http e https"),
        ("exemplo.com/a", "só http e https"),
        ("http:///semdominio", "sem domínio"),
        ("http://user:senha@x.com/", "usuário/senha"),
        ("http://x.com:99999/", "inválida"),
    ):
        with pytest.raises(Bloqueio, match=motivo):
            validar_url(ruim)


def test_origem():
    assert rede.origem("https://A.com/x") == "https://a.com:443"
    assert rede.origem("http://a.com:8080/") == "http://a.com:8080"
    assert rede.origem("about:blank") is None and rede.origem("data:text/html,x") is None


def test_politica_de_metodos():
    pagina = "https://site.com/form"
    # Ler é sempre permitido.
    for metodo in ("GET", "HEAD", "OPTIONS"):
        assert politica(metodo, "https://outro.com/x", pagina, True, False) is None
    # Só leitura: nenhum envio de formulário, nem para a mesma origem.
    assert "só leitura" in politica("POST", "https://site.com/enviar", pagina, True, False)
    # Interagir: formulário para a mesma origem passa; para outra, nunca.
    assert politica("POST", "https://site.com/enviar", pagina, True, True) is None
    assert "outra origem" in politica("POST", "https://outro.com/coleta", pagina, True, True)
    assert "outra origem" in politica("POST", "http://site.com/enviar", pagina, True, True), "esquema conta"
    # fetch/XHR da página: mesma origem passa (sites em JavaScript); outra, nunca.
    for interagir in (False, True):
        assert politica("POST", "https://site.com/api", pagina, False, interagir) is None
        assert "outra origem" in politica("PUT", "https://outro.com/api", pagina, False, interagir)
    # Página em branco (aba nova): não tem origem, então nada sai.
    assert "outra origem" in politica("POST", "https://site.com/x", "about:blank", False, True)


async def test_resolver_bloqueia_antes_e_depois_do_dns():
    r = Rede()
    for host in ("127.0.0.1", "169.254.169.254", "10.1.2.3", "[::1]", "100.64.0.1", "localhost", "a.localhost"):
        with pytest.raises(Bloqueio, match="interno|própria máquina"):
            await r.resolver(host, 80)
    assert await r.resolver("1.1.1.1", 443) == ["1.1.1.1"]


async def test_resolver_confere_todos_os_enderecos_do_nome(monkeypatch):
    r = Rede()

    async def getaddrinfo(host, porta, **_):
        return [(0, 0, 0, "", ("93.184.216.34", porta)), (0, 0, 0, "", ("10.0.0.7", porta))]

    monkeypatch.setattr(asyncio.get_running_loop(), "getaddrinfo", getaddrinfo)
    with pytest.raises(Bloqueio, match=r"aponta para endereço interno \(10.0.0.7\)"):
        await r.resolver("meio-interno.exemplo", 80)


async def test_excecao_vale_so_para_o_ip_e_a_porta_exatos():
    r = Rede(frozenset({("127.0.0.1", 8080)}))
    assert await r.resolver("127.0.0.1", 8080) == ["127.0.0.1"]
    with pytest.raises(Bloqueio):
        await r.resolver("127.0.0.1", 8081)
    with pytest.raises(Bloqueio):
        await r.resolver("localhost", 8080)


async def test_conferir_pedido_por_esquema():
    r = Rede()
    for url in ("data:text/html,oi", "blob:https://x.com/1", "about:blank"):
        await r.conferir_pedido(url)
    for url in ("file:///etc/passwd", "ftp://x.com/", "chrome-extension://abc/x.js"):
        with pytest.raises(Bloqueio, match="só http e https"):
            await r.conferir_pedido(url)
    with pytest.raises(Bloqueio, match="interno"):
        await r.conferir_pedido("http://169.254.169.254/latest/meta-data/")


class Alvo:
    """Um servidor HTTP local que responde "ok <caminho>"."""

    def __init__(self):
        self.pedidos = []
        alvo = self

        class Tratador(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_GET(self):
                alvo.pedidos.append(self.path)
                corpo = f"ok {self.path}".encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(corpo)))
                self.end_headers()
                self.wfile.write(corpo)

        self.servidor = ThreadingHTTPServer(("127.0.0.1", 0), Tratador)
        self.porta = self.servidor.server_address[1]
        threading.Thread(target=self.servidor.serve_forever, daemon=True).start()

    def fechar(self):
        self.servidor.shutdown()
        self.servidor.server_close()


@pytest.fixture
def alvos():
    publico, interno = Alvo(), Alvo()
    yield publico, interno
    publico.fechar()
    interno.fechar()


async def pedir(filtro, texto: str) -> bytes:
    leitor, escritor = await asyncio.open_connection("127.0.0.1", filtro.porta)
    escritor.write(texto.encode())
    await escritor.drain()
    resposta = await asyncio.wait_for(leitor.read(), 10)
    escritor.close()
    return resposta


async def test_filtro_deixa_passar_o_permitido_e_bloqueia_o_interno(alvos):
    publico, interno = alvos
    filtro = Filtro(Rede(frozenset({("127.0.0.1", publico.porta)})))
    await filtro.iniciar()
    try:
        ok = await pedir(filtro, f"GET http://127.0.0.1:{publico.porta}/a?b=1 HTTP/1.1\r\nHost: x\r\nProxy-Connection: keep-alive\r\n\r\n")
        assert ok.startswith(b"HTTP/1.") and b"ok /a?b=1" in ok
        assert publico.pedidos == ["/a?b=1"], "vai na forma de origem, sem o esquema e o host"

        for pedido in (
            f"GET http://127.0.0.1:{interno.porta}/segredo HTTP/1.1\r\nHost: x\r\n\r\n",
            f"CONNECT 127.0.0.1:{interno.porta} HTTP/1.1\r\nHost: x\r\n\r\n",
            "GET http://169.254.169.254/latest/meta-data/ HTTP/1.1\r\nHost: x\r\n\r\n",
            "CONNECT 169.254.169.254:443 HTTP/1.1\r\n\r\n",
            "CONNECT [::1]:443 HTTP/1.1\r\n\r\n",
            "GET http://localhost:1/ HTTP/1.1\r\n\r\n",
        ):
            resposta = await pedir(filtro, pedido)
            assert resposta.startswith(b"HTTP/1.1 403"), (pedido, resposta)
            assert "bloqueado pelo navegador do Abiyss".encode() in resposta
        assert interno.pedidos == [], "nada chegou ao endereço interno"
        assert filtro.total_bloqueados == 6
        assert any("169.254.169.254" in b.onde for b in filtro.bloqueios)

        tunel = await pedir(filtro, f"CONNECT 127.0.0.1:{publico.porta} HTTP/1.1\r\n\r\nGET /t HTTP/1.0\r\n\r\n")
        assert tunel.startswith(b"HTTP/1.1 200 Connection Established") and b"ok /t" in tunel

        for lixo in ("oi\r\n\r\n", "GET /relativo HTTP/1.1\r\n\r\n", "GET file:///etc/passwd HTTP/1.1\r\n\r\n"):
            assert (await pedir(filtro, lixo)).startswith(b"HTTP/1.1 400"), lixo
    finally:
        await filtro.fechar()


async def test_filtro_confere_o_endereco_conectado(alvos, monkeypatch):
    """DNS que muda no meio: o nome passou na checagem, mas a conexão caiu
    num endereço interno. O par conectado é conferido depois."""
    publico, _ = alvos
    r = Rede()

    async def resolver(host, porta):
        return ["127.0.0.1"]  # "público" segundo um DNS mentiroso

    monkeypatch.setattr(r, "resolver", resolver)
    filtro = Filtro(r)
    await filtro.iniciar()
    try:
        resposta = await pedir(filtro, f"GET http://enganador.exemplo:{publico.porta}/ HTTP/1.1\r\n\r\n")
        assert resposta.startswith(b"HTTP/1.1 403") and "endereço interno".encode() in resposta
        assert publico.pedidos == []
    finally:
        await filtro.fechar()
