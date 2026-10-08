"""O instantâneo em texto: o que aparece, as referências e os cortes."""

import asyncio

import pytest
from playwright.async_api import async_playwright

import instantaneo
from conftest import CHROMIUM, precisa_chromium

pytestmark = [pytest.mark.anyio, precisa_chromium]

ATRIBUTO = "data-abiyss-teste"


@pytest.fixture
async def pagina():
    async with async_playwright() as p:
        navegador = await p.chromium.launch(executable_path=CHROMIUM, args=["--no-sandbox"])
        contexto = await navegador.new_context()
        yield await contexto.new_page()
        await navegador.close()


async def ler(pagina, estado=None):
    estado = {"proximo": 1} if estado is None else estado
    prazo = asyncio.get_running_loop().time() + 10
    return await instantaneo.tirar(pagina, ATRIBUTO, estado, prazo), estado


async def test_texto_titulos_e_elementos(pagina):
    await pagina.set_content(
        """<title>T</title><h1>Título  principal</h1><p>Um   parágrafo
        com <b>negrito</b> e <a href="https://site.com/x">um link</a>.</p>
        <ul><li>um</li><li>dois</li></ul>
        <table><tr><td>a</td><td>b</td></tr></table>
        <img alt="Foto do gato"><img src="x.png">
        <p style="display:none">ESCONDIDO</p><div hidden>TAMBÉM ESCONDIDO</div><span aria-hidden="true">DECORATIVO</span>
        <script>var x = "CODIGO";</script><style>.a{}</style>
        <label>Nome <input name="nome" value="Ana"></label>
        <input type="password" value="segredo123" placeholder="Senha">
        <input type="search" placeholder="Buscar no site">
        <textarea aria-label="Comentário"></textarea>
        <label><input type="checkbox" checked> Aceito</label>
        <select aria-label="País"><option>Brasil</option><option selected>Portugal</option></select>
        <form><button>Enviar</button><input type="submit" value="Mandar"></form>
        <button disabled>Parado</button><div role="button" aria-expanded="false">Menu</div>
        <span onclick="1">clicável</span>"""
    )
    inst, estado = await ler(pagina)
    linhas = inst.texto.splitlines()
    assert linhas[0] == "# Título principal"
    assert "Um parágrafo com negrito e" in linhas[1]
    assert "[link e1] um link → https://site.com/x" in linhas
    assert "• um" in linhas and "• dois" in linhas
    assert "a | b" in linhas
    assert "[imagem] Foto do gato" in linhas
    for fora in ("ESCONDIDO", "DECORATIVO", "CODIGO", ".a{}", "segredo123"):
        assert fora not in inst.texto, fora
    assert '[campo e2] Nome = "Ana"' in linhas
    assert "[campo e3] Senha (senha)" in linhas
    assert "[campo e4] Buscar no site (vazio)" in linhas
    assert "[campo e5] Comentário (vazio)" in linhas
    assert "[caixa e6] ☑ Aceito" in linhas
    assert any(l.startswith('[seleção e7] País = "Portugal" (opções: Brasil | Portugal)') for l in linhas)
    assert "[botão e8] Enviar (envia formulário)" in linhas
    assert "[botão e9] Mandar (envia formulário)" in linhas
    assert "[botão e10] Parado (desativado)" in linhas
    assert "[botão e11] Menu (fechado)" in linhas
    assert "[clicável e12] clicável" in linhas
    assert estado["proximo"] == 13 and not inst.cortado_na_pagina
    assert inst.titulo == "T"


async def test_referencias_sao_estaveis_e_nunca_reaproveitadas(pagina):
    await pagina.set_content('<a href="/a">A</a><button>B</button>')
    inst, estado = await ler(pagina)
    assert "[link e1] A" in inst.texto and "[botão e2] B" in inst.texto
    await pagina.evaluate("document.body.insertAdjacentHTML('afterbegin', '<button>Novo</button>')")
    inst, estado = await ler(pagina, estado)
    linhas = inst.texto.splitlines()
    assert linhas[0] == "[botão e3] Novo", "elemento novo ganha número novo"
    assert "[link e1] A" in inst.texto and "[botão e2] B" in inst.texto, "os antigos mantêm o número"
    assert await pagina.locator(f'[{ATRIBUTO}="e2"]').inner_text() == "B"
    # Outro documento: a numeração continua (e1 nunca aponta para outra coisa).
    await pagina.set_content('<a href="/c">C</a>')
    inst, _ = await ler(pagina, estado)
    assert inst.texto.startswith("[link e4] C")


async def test_shadow_dom_e_quadros(pagina):
    await pagina.set_content(
        """<div id="h"></div><iframe srcdoc="<p>dentro do quadro</p><a href='https://x.com'>link do quadro</a>"></iframe>
        <script>
        const r = document.getElementById('h').attachShadow({mode: 'open'});
        r.innerHTML = '<p>texto na sombra</p><button>Botão na sombra</button>';
        </script>"""
    )
    await pagina.wait_for_timeout(200)
    inst, _ = await ler(pagina)
    assert "texto na sombra" in inst.texto and "[botão e1] Botão na sombra" in inst.texto
    assert "--- quadro 1:" in inst.texto
    assert "dentro do quadro" in inst.texto and "[link e2] link do quadro" in inst.texto
    assert inst.quadros == 2


async def test_pagina_enorme_e_cortada_na_coleta(pagina):
    corpo = "".join(f"<p>linha {i}</p>" for i in range(instantaneo.MAX_NOS + 5000))
    await pagina.set_content(f"<body>{corpo}</body>")
    inst, _ = await ler(pagina)
    assert inst.cortado_na_pagina
    assert len(inst.texto.splitlines()) <= instantaneo.MAX_LINHAS


def test_fatia():
    texto = "".join(f"linha {i}\n" for i in range(1000))
    pedaco, fim = instantaneo.fatia(texto, 0, 500)
    assert len(pedaco.encode()) <= 500 and pedaco.endswith("\n") and fim == len(pedaco)
    resto, fim2 = instantaneo.fatia(texto, fim, 10_000_000)
    assert pedaco + resto == texto and fim2 == len(texto)
    assert instantaneo.cortar_bytes("ação", 2) == "a"
