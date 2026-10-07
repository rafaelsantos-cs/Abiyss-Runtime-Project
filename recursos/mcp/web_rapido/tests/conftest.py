"""Tudo local: um site de mentira, um SearXNG de mentira e um projeto falso.
Nenhum teste sai para a internet."""

import json
import threading
import time
from collections import Counter
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

import pytest

import config

PASTA_SERVIDOR = Path(__file__).resolve().parent.parent
FIXTURES = Path(__file__).resolve().parent / "fixtures"

PARAGRAFO_LONGO = (
    "Este parágrafo fala sobre sono, memória e aprendizado, e se repete para formar um texto longo "
    "que precisa ser lido em partes. "
)


def pagina_longa() -> str:
    corpo = "\n".join(f"<p>{i:04d}. {PARAGRAFO_LONGO * 3}</p>" for i in range(400))
    return f"<html><head><title>Tudo sobre o sono</title></head><body><article><h1>Tudo sobre o sono</h1>{corpo}</article></body></html>"


class Site:
    """Servidor HTTP local. `pedidos` conta os acessos por caminho."""

    def __init__(self, robots_status: int = 200):
        self.pedidos: Counter[str] = Counter()
        self.robots_status = robots_status
        self.robots_atraso = 0.0
        site = self

        class Tratador(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def responder(self, status, corpo=b"", tipo="text/html; charset=utf-8", cabecalhos=None):
                self.send_response(status)
                self.send_header("Content-Type", tipo)
                self.send_header("Content-Length", str(len(corpo)))
                for nome, valor in (cabecalhos or {}).items():
                    self.send_header(nome, valor)
                self.end_headers()
                self.wfile.write(corpo)

            def do_GET(self):
                partes = urlsplit(self.path)
                caminho = partes.path
                site.pedidos[caminho] += 1
                base = f"http://127.0.0.1:{site.porta}"
                if caminho == "/robots.txt":
                    time.sleep(site.robots_atraso)
                    if site.robots_status != 200:
                        return self.responder(site.robots_status, b"erro", "text/plain")
                    return self.responder(200, (FIXTURES / "robots.txt").read_bytes(), "text/plain")
                if caminho == "/search":
                    q = parse_qs(partes.query)
                    if q.get("format") != ["json"]:
                        return self.responder(403, b"Forbidden")
                    if q.get("q") == ["nada"]:
                        dados = {"results": [], "unresponsive_engines": [["google", "timeout"], ["brave", "erro"]]}
                        return self.responder(200, json.dumps(dados).encode(), "application/json")
                    texto = (FIXTURES / "searxng.json").read_text().replace("{BASE}", base)
                    return self.responder(200, texto.encode(), "application/json")
                if caminho in ("/artigo.html", "/privado/publico.html", "/privado/segredo.html", "/so-humanos.html"):
                    return self.responder(200, (FIXTURES / "artigo.html").read_bytes())
                if caminho == "/vazio.html":
                    return self.responder(200, (FIXTURES / "vazio.html").read_bytes())
                if caminho == "/longo.html":
                    return self.responder(200, pagina_longa().encode())
                if caminho == "/grande.html":
                    corpo = "<html><body><article>" + "<p>bloco de texto grande. </p>\n" * 60000 + "</article></body></html>"
                    return self.responder(200, corpo.encode())
                if caminho == "/texto.txt":
                    return self.responder(200, "linha um\nlinha dois com acentuação\n".encode("latin-1"), "text/plain; charset=latin-1")
                if caminho == "/arquivo.pdf":
                    return self.responder(200, b"%PDF-1.4", "application/pdf")
                if caminho == "/redireciona":
                    return self.responder(302, b"", cabecalhos={"Location": "/artigo.html"})
                if caminho == "/ciclo":
                    return self.responder(302, b"", cabecalhos={"Location": "/ciclo"})
                if caminho == "/lento":
                    time.sleep(3)
                    return self.responder(200, (FIXTURES / "artigo.html").read_bytes())
                if caminho == "/muito-lento":
                    time.sleep(5.5)  # mais que o timeout padrão do httpx (5 s)
                    return self.responder(200, (FIXTURES / "artigo.html").read_bytes())
                if caminho == "/erro":
                    return self.responder(500, b"falhou")
                if caminho == "/healthz":
                    return self.responder(200, b"OK", "text/plain")
                return self.responder(404, b"nao achei")

        self.servidor = ThreadingHTTPServer(("127.0.0.1", 0), Tratador)
        self.servidor.daemon_threads = True
        self.porta = self.servidor.server_address[1]
        self.base = f"http://127.0.0.1:{self.porta}"
        self._thread = threading.Thread(target=self.servidor.serve_forever, daemon=True)
        self._thread.start()

    def fechar(self):
        self.servidor.shutdown()
        self.servidor.server_close()


@pytest.fixture
def site():
    s = Site()
    yield s
    s.fechar()


@pytest.fixture
def site_quebrado():
    """Um site cujo robots.txt dá erro 500: na dúvida, nada é lido."""
    s = Site(robots_status=500)
    yield s
    s.fechar()


def escrever_projeto(raiz: Path, timeout: int = 60, max_bytes_resultado: int = 1_000_000) -> Path:
    raiz.mkdir(parents=True, exist_ok=True)
    toml = raiz / "abiyss.toml"
    toml.write_text(
        f"""
[mcp]
max_bytes_resultado = {max_bytes_resultado}

[[mcp.servidores]]
nome = "web_rapido"
comando = "uv"
diretorio = "{PASTA_SERVIDOR}"
timeout_segundos = {timeout}
"""
    )
    return toml


@pytest.fixture
def env_base(tmp_path, site):
    toml = escrever_projeto(tmp_path / "projeto")
    return {
        "ABIYSS_CONFIG": str(toml),
        "WEB_SEARXNG_URL": site.base,
        "WEB_CACHE_ARQUIVO": str(tmp_path / "cache" / "web.sqlite3"),
        # O site de teste é local: sem isto, o bloqueio de rede interna recusaria tudo.
        "WEB_PERMITIR_REDE_LOCAL": "sim",
        "WEB_INTERVALO_DOMINIO_SEGUNDOS": "0",
    }


@pytest.fixture
def fazer_config(env_base):
    def fazer(**extra: str) -> config.Config:
        return config.carregar({**env_base, **extra})

    return fazer


@pytest.fixture
def anyio_backend():
    return "asyncio"
