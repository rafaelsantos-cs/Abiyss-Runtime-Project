"""Visão ao vivo: só localhost, só com o token, só ver."""

import asyncio
import json
import socket

import pytest
from mcp import Client

import servidor
from conftest import precisa_chromium

pytestmark = [pytest.mark.anyio, precisa_chromium]


def porta_livre() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


async def pedir(porta: int, caminho: str, metodo: str = "GET") -> tuple[int, dict, bytes]:
    leitor, escritor = await asyncio.open_connection("127.0.0.1", porta)
    escritor.write(f"{metodo} {caminho} HTTP/1.1\r\nHost: x\r\n\r\n".encode())
    await escritor.drain()
    resposta = await asyncio.wait_for(leitor.read(), 10)
    escritor.close()
    cabeca, corpo = resposta.split(b"\r\n\r\n", 1)
    linhas = cabeca.decode().split("\r\n")
    cabecalhos = dict(l.split(": ", 1) for l in linhas[1:])
    return int(linhas[0].split(" ")[1]), cabecalhos, corpo


async def test_desligada_por_padrao(config_navegador):
    _, nav = servidor.criar_servidor(config_navegador())
    assert nav.ao_vivo is None


async def test_ver_as_sessoes(config_navegador, rede_de_mentira):
    porta = porta_livre()
    srv, nav = servidor.criar_servidor(config_navegador(NAVEGADOR_AO_VIVO_PORTA=str(porta)))
    token = nav.ao_vivo.token
    async with Client(srv) as c:
        r = await c.call_tool("navegar", {"url": rede_de_mentira["site"].base + "/estatica.html"})
        sid = r.structured_content["sessao"]

        for caminho in ("/", "/?t=errado", f"/quadro/{sid}.jpg", "/sessoes.json?t="):
            status, _, _ = await pedir(porta, caminho)
            assert status == 403, caminho
        status, _, _ = await pedir(porta, f"/?t={token}", "POST")
        assert status == 405

        status, cab, corpo = await pedir(porta, f"/?t={token}")
        assert status == 200 and b"ao vivo" in corpo
        assert cab["X-Frame-Options"] == "DENY" and cab["Referrer-Policy"] == "no-referrer"
        assert "default-src 'none'" in cab["Content-Security-Policy"]

        status, _, corpo = await pedir(porta, f"/sessoes.json?t={token}")
        lista = json.loads(corpo)
        assert status == 200 and lista[0]["sessao"] == sid and lista[0]["url"].endswith("/estatica.html")

        status, cab, corpo = await pedir(porta, f"/quadro/{sid}.jpg?t={token}")
        assert status == 200 and cab["Content-Type"] == "image/jpeg" and corpo.startswith(b"\xff\xd8")
        status, _, _ = await pedir(porta, f"/quadro/s000000.jpg?t={token}")
        assert status == 404

        # Só no loopback.
        with socket.socket() as s:
            s.bind(("0.0.0.0", 0))
            ip = socket.gethostbyname(socket.gethostname())
        if not ip.startswith("127."):
            with pytest.raises(OSError):
                await asyncio.wait_for(asyncio.open_connection(ip, porta), 3)
    with pytest.raises(OSError):
        await asyncio.open_connection("127.0.0.1", porta)
