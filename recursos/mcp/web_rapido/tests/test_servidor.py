"""As ferramentas pelo cliente MCP, com o site e o SearXNG de mentira."""

import os
import subprocess
import sys

import pytest
from mcp import Client, StdioServerParameters

import servidor
from conftest import PASTA_SERVIDOR

pytestmark = pytest.mark.anyio


async def test_buscar(fazer_config, site):
    mcp, _ = servidor.criar_servidor(fazer_config())
    async with Client(mcp) as cliente:
        r = await cliente.call_tool("buscar", {"consulta": "  sono   e memória ", "max_resultados": 2})
        assert not r.is_error, r.content[0].text
        texto = r.content[0].text
        assert texto.startswith('Busca: "sono e memória": 2 resultado(s) via SearXNG; buscado em 20')
        assert "1. Como o sono consolida a memória" in texto
        assert f"   {site.base}/artigo.html" in texto
        assert "motores: duckduckgo, brave · publicado: 2026-09-30T08:00:00" in texto
        assert "aviso: motores sem resposta: google" in texto
        assert "Notas em texto puro" not in texto, "max_resultados = 2"
        e = r.structured_content
        assert e["do_cache"] is False and len(e["resultados"]) == 2
        assert e["resultados"][0]["trecho"].startswith("Durante o sono profundo")

        r = await cliente.call_tool("buscar", {"consulta": "SONO E MEMÓRIA"})
        assert "(do cache)" in r.content[0].text and r.structured_content["do_cache"] is True
        assert "várias linhas de texto" in r.content[0].text, "espaços do trecho normalizados"
        assert site.pedidos["/search"] == 1

        r = await cliente.call_tool("buscar", {"consulta": "nada"})
        assert "Nenhum resultado." in r.content[0].text and "google, brave" in r.content[0].text
        await cliente.call_tool("buscar", {"consulta": "nada"})
        assert site.pedidos["/search"] == 3, "busca sem resultado não fica no cache"

        r = await cliente.call_tool("buscar", {"consulta": "   "})
        assert r.is_error and "consulta vazia" in r.content[0].text


async def test_searxng_fora_do_ar_ou_sem_json(fazer_config, site):
    mcp, _ = servidor.criar_servidor(fazer_config(WEB_SEARXNG_URL="http://127.0.0.1:9"))
    async with Client(mcp) as cliente:
        r = await cliente.call_tool("buscar", {"consulta": "x"})
        assert r.is_error and "SearXNG fora do ar" in r.content[0].text


async def test_ler_pagina_extrai_o_texto_principal(fazer_config, site):
    mcp, _ = servidor.criar_servidor(fazer_config())
    async with Client(mcp) as cliente:
        r = await cliente.call_tool("ler_pagina", {"url": f"{site.base}/redireciona"})
        assert not r.is_error, r.content[0].text
        texto = r.content[0].text
        assert texto.startswith(f"URL: {site.base}/artigo.html (pedida: {site.base}/redireciona)")
        assert "Título: Como o sono consolida a memória" in texto
        assert "Publicado em: 2026-09-30" in texto
        assert "Buscado em: 20" in texto and "(do cache)" not in texto
        assert "o hipocampo reativa as experiências do dia" in texto
        assert "sono REM" in texto
        for lixo in ("COMPRE AGORA", "Assine já", "Todos os direitos reservados", "rastreador"):
            assert lixo not in texto, lixo
        assert r.structured_content["titulo"] == "Como o sono consolida a memória"

        r = await cliente.call_tool("ler_pagina", {"url": f"{site.base}/redireciona"})
        assert "(do cache)" in r.content[0].text
        assert site.pedidos["/artigo.html"] == 1


async def test_texto_longo_vem_em_partes(fazer_config, site):
    mcp, _ = servidor.criar_servidor(fazer_config(WEB_MAX_TEXTO_BYTES="20000"))
    async with Client(mcp) as cliente:
        partes, inicio = [], 0
        while True:
            r = await cliente.call_tool("ler_pagina", {"url": f"{site.base}/longo.html", "inicio": inicio})
            assert not r.is_error, r.content[0].text
            e = r.structured_content
            assert len(r.content[0].text.encode()) <= 20000 + 8192
            partes.append(e["texto"])
            if e["fim"] >= e["total_caracteres"]:
                assert "texto cortado pelo web_rapido" not in r.content[0].text
                break
            assert f"chame ler_pagina com inicio={e['fim']}" in r.content[0].text
            inicio = e["fim"]
        assert len(partes) > 3
        completo = "".join(partes)
        assert len(completo) == e["total_caracteres"]
        assert "0000." in completo and "0399." in completo
        assert site.pedidos["/longo.html"] == 1, "as partes seguintes vêm do cache"
        r = await cliente.call_tool("ler_pagina", {"url": f"{site.base}/longo.html", "inicio": 10**9})
        assert r.is_error and "passa do fim do texto" in r.content[0].text


async def test_ler_pagina_recusas(fazer_config, site):
    mcp, _ = servidor.criar_servidor(fazer_config())
    async with Client(mcp) as cliente:
        for url, motivo in (
            (f"{site.base}/privado/segredo.html", "bloqueado pelo robots.txt"),
            (f"{site.base}/arquivo.pdf", "tipo de conteúdo não suportado"),
            (f"{site.base}/vazio.html", "não achei texto principal"),
            ("file:///etc/passwd", "só http e https"),
        ):
            r = await cliente.call_tool("ler_pagina", {"url": url})
            assert r.is_error and motivo in r.content[0].text, (url, r.content[0].text)


async def test_texto_puro_e_pagina_grande(fazer_config, site):
    mcp, _ = servidor.criar_servidor(fazer_config(WEB_MAX_PAGINA_MB="1"))
    async with Client(mcp) as cliente:
        r = await cliente.call_tool("ler_pagina", {"url": f"{site.base}/texto.txt"})
        assert "linha dois com acentuação" in r.content[0].text, "charset do cabeçalho (latin-1)"
        r = await cliente.call_tool("ler_pagina", {"url": f"{site.base}/grande.html"})
        assert not r.is_error, r.content[0].text
        assert "aviso: a página passou de 1 MiB" in r.content[0].text


async def test_rede_interna_bloqueada_por_padrao(fazer_config, site):
    mcp, _ = servidor.criar_servidor(fazer_config(WEB_PERMITIR_REDE_LOCAL="nao"))
    async with Client(mcp) as cliente:
        r = await cliente.call_tool("ler_pagina", {"url": f"{site.base}/artigo.html"})
        assert r.is_error and "endereço interno" in r.content[0].text
        # O SearXNG (configurado pelo dono) continua acessível na rede local.
        r = await cliente.call_tool("buscar", {"consulta": "sono"})
        assert not r.is_error


async def test_servidor_de_verdade_pelo_stdio(env_base):
    parametros = StdioServerParameters(
        command=sys.executable,
        args=["servidor.py"],
        cwd=str(PASTA_SERVIDOR),
        env={"PATH": os.environ["PATH"], **env_base},
    )
    async with Client(parametros) as cliente:
        nomes = sorted(f.name for f in (await cliente.list_tools()).tools)
        assert nomes == ["buscar", "ler_pagina"]
        r = await cliente.call_tool("buscar", {"consulta": "sono"})
        assert "Como o sono consolida a memória" in r.content[0].text


def test_configuracao_invalida_nao_sobe(env_base):
    r = subprocess.run(
        [sys.executable, "servidor.py"],
        cwd=PASTA_SERVIDOR,
        env={**os.environ, **env_base, "WEB_PRAZO_SEGUNDOS": "300"},
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        timeout=60,
    )
    assert r.returncode == 2 and r.stdout == ""
    assert "web_rapido: NÃO vou subir" in r.stderr and "o kernel mataria o servidor" in r.stderr
