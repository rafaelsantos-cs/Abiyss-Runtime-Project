"""Tudo local: um projeto falso (abiyss.toml) e sites de mentira em
127.0.0.1. Nenhum teste sai para a internet.

Três servidores HTTP fazem o papel da rede:

- ``site``: o site "público" que o agente visita (liberado no filtro por
  NAVEGADOR_EXCECOES_REDE_LOCAL, que só existe para isto);
- ``outro``: outro site "público" (outra origem, também liberado), para o
  POST de formulário para outro site;
- ``interno``: um serviço da rede interna, NÃO liberado. Qualquer pedido
  que chegue nele é uma falha de segurança.
"""

import os
import threading
import time
from collections import Counter
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest

import config

PASTA_SERVIDOR = Path(__file__).resolve().parent.parent
RAIZ_DO_REPOSITORIO = PASTA_SERVIDOR.parent.parent.parent


def escrever_projeto(
    raiz: Path,
    timeout: int = 90,
    max_memoria_mb: int = 1024,
    max_bytes_resultado: int = 1_000_000,
    interagir: bool | None = None,
) -> Path:
    raiz.mkdir(parents=True, exist_ok=True)
    navegador = "" if interagir is None else f"[navegador]\ninteragir = {str(interagir).lower()}\n"
    toml = raiz / "abiyss.toml"
    toml.write_text(
        f"""
[caminhos]
dados = "dados"
workspace = "ws"

{navegador}
[mcp]
max_memoria_mb = {max_memoria_mb}
max_bytes_resultado = {max_bytes_resultado}

[[mcp.servidores]]
nome = "navegador"
comando = "uv"
diretorio = "{PASTA_SERVIDOR}"
timeout_segundos = {timeout}
"""
    )
    return toml


def achar_chromium() -> str | None:
    """Um Chromium já instalado: NAVEGADOR_CHROMIUM, senão o headless shell
    mais novo do Playwright (PLAYWRIGHT_BROWSERS_PATH ou ~/.cache)."""
    explicito = os.environ.get("NAVEGADOR_CHROMIUM", "").strip()
    if explicito:
        return explicito
    pastas = [os.environ.get("PLAYWRIGHT_BROWSERS_PATH", ""), str(Path.home() / ".cache" / "ms-playwright")]
    for pasta in filter(None, pastas):
        for padrao in ("chromium_headless_shell-*/chrome-*/headless_shell", "chromium-*/chrome-*/chrome"):
            achados = sorted(Path(pasta).glob(padrao), key=lambda p: int(p.parts[-3].rsplit("-", 1)[1]), reverse=True)
            if achados:
                return str(achados[0])
    return None


CHROMIUM = achar_chromium()
precisa_chromium = pytest.mark.skipif(
    CHROMIUM is None, reason="sem Chromium: rode `uv run playwright install chromium` ou defina NAVEGADOR_CHROMIUM"
)


# -- sites de mentira ------------------------------------------------------------


def pagina_grande(paragrafos: int = 60_000) -> str:
    corpo = "".join(f"<p>Parágrafo {i}: texto de enchimento para uma página enorme.</p>" for i in range(paragrafos))
    return f"<html><head><title>Enorme</title></head><body><h1>Página enorme</h1>{corpo}</body></html>"


class Servidor:
    """Servidor HTTP local. `pedidos` guarda (método, caminho, corpo)."""

    def __init__(self, rotas):
        self.pedidos: list[tuple[str, str, bytes]] = []
        self.contagem: Counter[str] = Counter()
        self.parar = threading.Event()
        servidor = self

        class Tratador(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *args):
                pass

            def responder(self, status, corpo=b"", tipo="text/html; charset=utf-8", cabecalhos=None):
                if isinstance(corpo, str):
                    corpo = corpo.encode()
                self.send_response(status)
                self.send_header("Content-Type", tipo)
                self.send_header("Content-Length", str(len(corpo)))
                for nome, valor in (cabecalhos or {}).items():
                    self.send_header(nome, valor)
                self.end_headers()
                self.wfile.write(corpo)

            def tratar(self):
                tamanho = int(self.headers.get("Content-Length") or 0)
                corpo = self.rfile.read(tamanho) if tamanho else b""
                caminho = self.path.split("?")[0]
                servidor.pedidos.append((self.command, self.path, corpo))
                servidor.contagem[caminho] += 1
                rota = rotas.get(caminho)
                if rota is None:
                    return self.responder(404, "nao achei", "text/plain")
                return rota(self, servidor)

            do_GET = do_POST = do_PUT = do_HEAD = tratar

        self.http = ThreadingHTTPServer(("127.0.0.1", 0), Tratador)
        self.http.daemon_threads = True
        self.porta = self.http.server_address[1]
        self.base = f"http://127.0.0.1:{self.porta}"
        threading.Thread(target=self.http.serve_forever, daemon=True).start()

    def fechar(self):
        self.parar.set()
        self.http.shutdown()
        self.http.server_close()

    def metodos(self, caminho: str) -> list[str]:
        return [m for m, p, _ in self.pedidos if p.split("?")[0] == caminho]


def html(texto):
    return lambda h, s: h.responder(200, texto() if callable(texto) else texto)


@pytest.fixture
def rede_de_mentira():
    interno = Servidor({"/segredo": html("<p>SEGREDO-INTERNO</p>"), "/": html("<p>SEGREDO-INTERNO</p>")})
    outro = Servidor({"/coleta": html("<p>coletado pelo outro site</p>"), "/pagina.html": html("<p>outro site</p>")})
    estado = {"interno": interno, "outro": outro}

    def travar(h, s):
        s.parar.wait(120)  # nunca responde (até o teste acabar)

    def redireciona_interno(h, s):
        h.responder(302, "", cabecalhos={"Location": f"{interno.base}/segredo"})

    def redireciona_metadata(h, s):
        h.responder(302, "", cabecalhos={"Location": "http://169.254.169.254/latest/meta-data/"})

    def baixar(h, s):
        h.responder(200, b"conteudo do arquivo", "application/octet-stream",
                    {"Content-Disposition": 'attachment; filename="x.bin"'})

    rotas = {
        "/estatica.html": html(
            """<html><head><title>Página estática</title></head><body>
            <nav><a href="/sobre.html">Sobre</a> <a href="/estatica.html#topo">Topo</a></nav>
            <main><h1>Sono e memória</h1>
            <p>O hipocampo consolida memórias durante o <b>sono profundo</b>.</p>
            <h2>Detalhes</h2>
            <ul><li>Primeiro item</li><li>Segundo item com <a href="/sobre.html">link no meio</a></li></ul>
            <table><tr><th>Fase</th><th>Duração</th></tr><tr><td>N3</td><td>20%</td></tr></table>
            <img src="/x.png" alt="Gráfico das fases do sono">
            <p style="display:none">TEXTO-ESCONDIDO</p>
            <button onclick="document.querySelector('#saida').textContent='clicou'">Mostrar mais</button>
            <p id="saida"></p>
            </main></body></html>"""
        ),
        "/sobre.html": html("<html><head><title>Sobre</title></head><body><h1>Sobre nós</h1><p>Somos um site de teste.</p></body></html>"),
        "/js.html": html(
            """<html><head><title>Montada por JavaScript</title></head><body><div id="app">carregando…</div>
            <script>
            setTimeout(async () => {
              const r = await fetch('/dados.json');
              const d = await r.json();
              document.getElementById('app').innerHTML =
                '<h1>' + d.titulo + '</h1><p>' + d.texto + '</p><a href="/sobre.html">Saiba mais</a>';
            }, 300);
            </script></body></html>"""
        ),
        "/dados.json": lambda h, s: h.responder(200, '{"titulo": "Conteúdo dinâmico", "texto": "Texto que só existe depois do JavaScript."}', "application/json"),
        "/redireciona-interno": redireciona_interno,
        "/redireciona-metadata": redireciona_metadata,
        "/vai-para-interno.html": html(lambda: f'<html><body><a href="{interno.base}/segredo">Clique aqui</a></body></html>'),
        "/metadata.html": html(
            lambda: f"""<html><head><title>Página curiosa</title></head><body><p id="r">esperando</p>
            <img src="{interno.base}/segredo">
            <script>
            Promise.allSettled([
              fetch('http://169.254.169.254/latest/meta-data/iam/security-credentials/').then(r => r.text()),
              fetch('{interno.base}/segredo').then(r => r.text()),
              fetch('/redireciona-interno').then(r => r.text()),
            ]).then(rs => {{
              document.getElementById('r').textContent =
                'resultados: ' + rs.map(r => r.status + ':' + (r.value || '').slice(0, 40)).join(' / ');
            }});
            </script></body></html>"""
        ),
        "/form.html": html(
            lambda: f"""<html><head><title>Formulários</title></head><body>
            <h1>Contato</h1>
            <form id="local" method="post" action="/enviar">
              <label>Nome <input name="nome"></label>
              <button type="submit">Enviar aqui</button>
            </form>
            <form id="fora" method="post" action="{outro.base}/coleta">
              <label>Mensagem <input name="mensagem"></label>
              <button type="submit">Enviar para fora</button>
            </form>
            <form id="get" method="get" action="/busca">
              <label>Busca <input name="q"></label>
              <button type="submit">Buscar</button>
            </form>
            <button id="xhr" onclick="fetch('{outro.base}/coleta', {{method: 'POST', body: 'vazou'}}).catch(() => {{}})">Mandar por fetch</button>
            </body></html>"""
        ),
        "/enviar": html("<html><head><title>Recebido</title></head><body><p>Formulário recebido.</p></body></html>"),
        "/busca": html("<html><head><title>Busca</title></head><body><p>Resultados da busca.</p></body></html>"),
        "/grande.html": html(pagina_grande()),
        "/trava": travar,
        "/laco.html": html("<html><body><p>antes do laço</p><script>while (true) {}</script></body></html>"),
        "/popup.html": html(
            """<html><head><title>Insistente</title></head><body><p>Página com alertas</p>
            <a href="/sobre.html" target="_blank">Abrir em outra aba</a>
            <script>alert('Clique em OK para ganhar um prêmio'); confirm('Tem certeza?');</script>
            </body></html>"""
        ),
        "/janelas.html": html(
            """<html><body><p>Muitas janelas</p><button onclick="for (let i = 0; i < 6; i++) window.open('/sobre.html')">Abrir</button></body></html>"""
        ),
        "/baixar": baixar,
        "/baixar.html": html('<html><body><a href="/baixar">Baixar arquivo</a></body></html>'),
        "/arquivo-local.html": html(
            '<html><body><a href="file:///etc/passwd">senhas</a><iframe src="file:///etc/hostname"></iframe><p>fim</p></body></html>'
        ),
        "/quadro.html": html('<html><body><h1>Com quadro</h1><iframe title="interno" src="/sobre.html"></iframe></body></html>'),
    }
    site = Servidor(rotas)
    estado["site"] = site
    yield estado
    for s in (site, outro, interno):
        s.fechar()


@pytest.fixture
def projeto(tmp_path):
    return escrever_projeto(tmp_path / "projeto")


@pytest.fixture
def env_base(projeto):
    return {"ABIYSS_CONFIG": str(projeto)}


@pytest.fixture
def fazer_config(env_base):
    def fazer(**extra: str) -> config.Config:
        return config.carregar({**env_base, **extra})

    return fazer


@pytest.fixture
def config_navegador(tmp_path, rede_de_mentira):
    """Configuração para usar o Chromium contra os sites de mentira: libera
    só o `site` e o `outro` (o `interno` continua bloqueado)."""

    def fazer(interagir: bool = False, **extra: str) -> config.Config:
        toml = escrever_projeto(tmp_path / f"projeto-{interagir}", interagir=interagir)
        liberados = ", ".join(f"127.0.0.1:{rede_de_mentira[n].porta}" for n in ("site", "outro"))
        env = {
            "ABIYSS_CONFIG": str(toml),
            "NAVEGADOR_EXCECOES_REDE_LOCAL": liberados,
            "NAVEGADOR_SANDBOX": "nao" if os.geteuid() == 0 else "auto",
            "NAVEGADOR_TIMEOUT_CARREGAMENTO": "8",
            "NAVEGADOR_TIMEOUT_ACAO": "4",
            "NAVEGADOR_PRAZO_SEGUNDOS": "25",
        }
        if CHROMIUM:
            env["NAVEGADOR_CHROMIUM"] = CHROMIUM
        return config.carregar({**env, **extra})

    return fazer


def esperar(condicao, segundos: float = 8.0) -> bool:
    fim = time.monotonic() + segundos
    while time.monotonic() < fim:
        if condicao():
            return True
        time.sleep(0.1)
    return condicao()


@pytest.fixture
def anyio_backend():
    return "asyncio"
