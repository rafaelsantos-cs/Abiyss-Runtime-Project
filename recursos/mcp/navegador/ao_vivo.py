"""Visão ao vivo (opcional): o dono vê as sessões do navegador.

Com ``NAVEGADOR_AO_VIVO_PORTA`` (0 = desligado, o padrão), um servidor HTTP
mínimo ouve SÓ em 127.0.0.1 e mostra, para cada sessão aberta, a captura
da aba atual (JPEG), renovada a cada 1,5 s. Só ver: nenhum clique ou tecla
chega ao Chromium por aqui. Cada subida sorteia um token (no journal) que
vai na URL. Da sua máquina, por um túnel SSH:

    ssh -L 8790:127.0.0.1:8790 ubuntu@<vm>
    # abra http://127.0.0.1:8790/?t=<token do journal>

Uma captura por sessão a cada segundo no máximo, por mais abas abertas.
"""

from __future__ import annotations

import asyncio
import contextlib
import hmac
import json
import logging
import secrets
import time
from urllib.parse import parse_qs, urlsplit

log = logging.getLogger("navegador")

INTERVALO_CAPTURA = 1.0
PAGINA = """<!doctype html>
<html lang="pt-BR"><head><meta charset="utf-8"><title>Abiyss: navegador ao vivo</title>
<style>
body { font: 14px system-ui, sans-serif; margin: 16px; background: #111; color: #ddd; }
figure { margin: 0 0 24px; } img { max-width: 100%; border: 1px solid #444; background: #fff; }
figcaption { margin: 4px 0; word-break: break-all; } .vazio { color: #888; }
</style></head><body>
<h1>Navegador do Abiyss, ao vivo (só ver)</h1>
<div id="sessoes" class="vazio">nenhuma sessão aberta</div>
<script>
const token = new URLSearchParams(location.search).get("t") || "";
async function atualizar() {
  try {
    const r = await fetch("/sessoes.json?t=" + encodeURIComponent(token), { cache: "no-store" });
    const lista = await r.json();
    const raiz = document.getElementById("sessoes");
    raiz.textContent = lista.length ? "" : "nenhuma sessão aberta";
    raiz.className = lista.length ? "" : "vazio";
    for (const s of lista) {
      const fig = document.createElement("figure");
      const leg = document.createElement("figcaption");
      leg.textContent = `${s.sessao} · aba ${s.aba} de ${s.abas} · ${s.url}`;
      const img = document.createElement("img");
      img.alt = "captura da sessão " + s.sessao;
      img.src = `/quadro/${encodeURIComponent(s.sessao)}.jpg?t=${encodeURIComponent(token)}&_=${Date.now()}`;
      fig.append(leg, img);
      raiz.append(fig);
    }
  } catch (e) {}
  setTimeout(atualizar, 1500);
}
atualizar();
</script></body></html>
"""
CABECALHOS = (
    "Cache-Control: no-store\r\n"
    "X-Frame-Options: DENY\r\n"
    "Referrer-Policy: no-referrer\r\n"
    "X-Content-Type-Options: nosniff\r\n"
    "Content-Security-Policy: default-src 'none'; img-src 'self'; connect-src 'self'; "
    "style-src 'unsafe-inline'; script-src 'unsafe-inline'\r\n"
)


class AoVivo:
    def __init__(self, nav, porta: int):
        self.nav = nav
        self.porta = porta
        self.token = secrets.token_urlsafe(18)
        self._servidor: asyncio.base_events.Server | None = None
        self._cache: dict[str, tuple[float, bytes]] = {}

    @property
    def url(self) -> str:
        return f"http://127.0.0.1:{self.porta}/?t={self.token}"

    async def iniciar(self) -> None:
        try:
            self._servidor = await asyncio.start_server(self._atender, "127.0.0.1", self.porta, limit=16 * 1024)
        except OSError as e:
            log.warning("visão ao vivo desligada: não consegui ouvir em 127.0.0.1:%d (%s)", self.porta, e)
            return
        log.info("visão ao vivo (só ver, só localhost): %s", self.url)

    async def fechar(self) -> None:
        if self._servidor is not None:
            self._servidor.close()
            with contextlib.suppress(Exception):
                await asyncio.wait_for(self._servidor.wait_closed(), 2)
            self._servidor = None

    async def _quadro(self, sessao_id: str) -> bytes | None:
        sessao = self.nav.sessoes.get(sessao_id)
        if sessao is None or not sessao.abas:
            return None
        guardado = self._cache.get(sessao_id)
        if guardado and time.monotonic() - guardado[0] < INTERVALO_CAPTURA:
            return guardado[1]
        try:
            imagem = await sessao.pagina.screenshot(type="jpeg", quality=55, timeout=3000)
        except Exception:
            return guardado[1] if guardado else None
        self._cache = {k: v for k, v in self._cache.items() if k in self.nav.sessoes}
        self._cache[sessao_id] = (time.monotonic(), imagem)
        return imagem

    async def _atender(self, leitor: asyncio.StreamReader, escritor: asyncio.StreamWriter) -> None:
        try:
            try:
                cabecalho = await asyncio.wait_for(leitor.readuntil(b"\r\n\r\n"), 10)
            except (asyncio.IncompleteReadError, asyncio.LimitOverrunError, asyncio.TimeoutError, ConnectionError):
                return
            partes = cabecalho.split(b"\r\n", 1)[0].decode("latin-1").split(" ")
            if len(partes) != 3 or partes[0] != "GET":
                return await self._responder(escritor, 405, "text/plain", b"so GET")
            url = urlsplit(partes[1])
            token = parse_qs(url.query).get("t", [""])[0]
            if not hmac.compare_digest(token.encode(), self.token.encode()):
                return await self._responder(escritor, 403, "text/plain", b"token invalido")
            if url.path == "/":
                return await self._responder(escritor, 200, "text/html; charset=utf-8", PAGINA.encode())
            if url.path == "/sessoes.json":
                lista = []
                for s in list(self.nav.sessoes.values()):
                    if s.abas:
                        lista.append({"sessao": s.id, "url": s.pagina.url, "aba": s.atual + 1, "abas": len(s.abas)})
                return await self._responder(escritor, 200, "application/json", json.dumps(lista).encode())
            if url.path.startswith("/quadro/") and url.path.endswith(".jpg"):
                imagem = await self._quadro(url.path[len("/quadro/") : -len(".jpg")])
                if imagem is None:
                    return await self._responder(escritor, 404, "text/plain", b"sem sessao")
                return await self._responder(escritor, 200, "image/jpeg", imagem)
            return await self._responder(escritor, 404, "text/plain", b"nao achei")
        finally:
            escritor.close()

    @staticmethod
    async def _responder(escritor: asyncio.StreamWriter, status: int, tipo: str, corpo: bytes) -> None:
        frase = {200: "OK", 403: "Forbidden", 404: "Not Found", 405: "Method Not Allowed"}.get(status, "Error")
        with contextlib.suppress(Exception):
            escritor.write(
                f"HTTP/1.1 {status} {frase}\r\nContent-Type: {tipo}\r\nContent-Length: {len(corpo)}\r\n"
                f"{CABECALHOS}Connection: close\r\n\r\n".encode() + corpo
            )
            await escritor.drain()
