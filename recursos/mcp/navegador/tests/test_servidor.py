"""As ferramentas pelo cliente MCP, com um Chromium de verdade contra os
sites de mentira (conftest.py). Nada sai para a internet."""

import json
import os
import re
import signal
import subprocess
import sys
import time

import pytest
from mcp import Client

import processos
import servidor
from conftest import PASTA_SERVIDOR, esperar, precisa_chromium

pytestmark = [pytest.mark.anyio, precisa_chromium]


async def abrir(cliente, url, sessao=""):
    r = await cliente.call_tool("navegar", {"url": url, "sessao": sessao})
    assert not r.is_error, r.content[0].text
    return r.content[0].text, r.structured_content["sessao"]


def ref_de(texto: str, rotulo: str, nome: str) -> str:
    achado = re.search(rf"\[{rotulo} (e\d+)\] (?:☐ |☑ )?{re.escape(nome)}", texto)
    assert achado, f"não achei [{rotulo} …] {nome} em:\n{texto}"
    return achado.group(1)


async def test_pagina_estatica_e_navegacao(config_navegador, rede_de_mentira):
    site = rede_de_mentira["site"]
    srv, nav = servidor.criar_servidor(config_navegador())
    async with Client(srv) as c:
        texto, sid = await abrir(c, f"{site.base}/estatica.html")
        assert texto.startswith(f"Sessão {sid} · aba 1 de 1 · modo só leitura\nURL: {site.base}/estatica.html\n")
        assert "Título: Página estática" in texto
        assert "# Sono e memória" in texto and "O hipocampo consolida memórias durante o sono profundo." in texto
        assert "TEXTO-ESCONDIDO" not in texto
        sobre = ref_de(texto, "link", "Sobre")

        r = await c.call_tool("ler", {"sessao": sid})
        assert ref_de(r.content[0].text, "link", "Sobre") == sobre, "a referência é estável"

        r = await c.call_tool("clicar", {"sessao": sid, "ref": sobre})
        assert not r.is_error and "# Sobre nós" in r.content[0].text
        assert r.structured_content["url"] == f"{site.base}/sobre.html"

        r = await c.call_tool("voltar", {"sessao": sid})
        assert "# Sono e memória" in r.content[0].text
        r = await c.call_tool("voltar", {"sessao": sid})
        assert not r.is_error
        r = await c.call_tool("voltar", {"sessao": sid})
        assert r.is_error and "não há página anterior" in r.content[0].text

        r = await c.call_tool("clicar", {"sessao": sid, "ref": "e999"})
        assert r.is_error and "não existe nesta página" in r.content[0].text
        r = await c.call_tool("clicar", {"sessao": sid, "ref": "#x"})
        assert r.is_error and "referência inválida" in r.content[0].text

        r = await c.call_tool("rolar", {"sessao": sid, "direcao": "fim"})
        assert not r.is_error and "Rolei para fim" in r.content[0].text

        r = await c.call_tool("fechar", {"sessao": sid})
        assert not r.is_error
        r = await c.call_tool("ler", {"sessao": sid})
        assert r.is_error and "foi encerrada: fechada a pedido" in r.content[0].text


async def test_pagina_montada_por_javascript(config_navegador, rede_de_mentira):
    site = rede_de_mentira["site"]
    srv, _ = servidor.criar_servidor(config_navegador())
    async with Client(srv) as c:
        texto, _ = await abrir(c, f"{site.base}/js.html")
        assert "# Conteúdo dinâmico" in texto and "Texto que só existe depois do JavaScript." in texto
        assert "carregando" not in texto
        assert site.contagem["/dados.json"] == 1


async def test_redirecionamento_para_endereco_interno(config_navegador, rede_de_mentira):
    site, interno = rede_de_mentira["site"], rede_de_mentira["interno"]
    srv, _ = servidor.criar_servidor(config_navegador())
    async with Client(srv) as c:
        for caminho in ("/redireciona-interno", "/redireciona-metadata"):
            r = await c.call_tool("navegar", {"url": site.base + caminho})
            assert r.is_error, r.content[0].text
            assert "bloqueado" in r.content[0].text and "endereço interno" in r.content[0].text
        # Direto, nem chega a sair: recusado antes de conectar.
        for url in (f"{interno.base}/segredo", "http://169.254.169.254/latest/meta-data/", "http://localhost:9/"):
            r = await c.call_tool("navegar", {"url": url})
            assert r.is_error and "bloqueado" in r.content[0].text, url
        # Um link da página para a rede interna.
        texto, sid = await abrir(c, f"{site.base}/vai-para-interno.html")
        r = await c.call_tool("clicar", {"sessao": sid, "ref": ref_de(texto, "link", "Clique aqui")})
        assert "SEGREDO-INTERNO" not in r.content[0].text
        assert "endereço interno" in r.content[0].text
    assert interno.pedidos == [], "nada chegou ao serviço interno"


async def test_pagina_que_busca_o_metadata(config_navegador, rede_de_mentira):
    site, interno = rede_de_mentira["site"], rede_de_mentira["interno"]
    srv, _ = servidor.criar_servidor(config_navegador())
    async with Client(srv) as c:
        texto, _ = await abrir(c, f"{site.base}/metadata.html")
        assert "resultados: rejected: / rejected: / rejected:" in texto
        assert "169.254.169.254" in texto and "pedido(s) da página bloqueado(s)" in texto
        assert "SEGREDO-INTERNO" not in texto
    assert interno.pedidos == []


async def test_esquemas_que_nao_sao_http_sao_recusados(config_navegador, rede_de_mentira):
    site = rede_de_mentira["site"]
    srv, _ = servidor.criar_servidor(config_navegador())
    async with Client(srv) as c:
        for url in ("file:///etc/passwd", "chrome://version", "javascript:alert(1)", "view-source:http://x.com"):
            r = await c.call_tool("navegar", {"url": url})
            assert r.is_error and "só http e https" in r.content[0].text, url
        texto, sid = await abrir(c, f"{site.base}/arquivo-local.html")
        hostname = open("/etc/hostname").read().strip() if os.path.exists("/etc/hostname") else "nenhum"
        assert hostname not in texto.split("---", 1)[1]
        r = await c.call_tool("clicar", {"sessao": sid, "ref": ref_de(texto, "link", "senhas")})
        assert r.structured_content is None or not r.structured_content["url"].startswith("file:")
        assert "root:" not in r.content[0].text


async def test_formularios_no_modo_so_leitura(config_navegador, rede_de_mentira):
    site, outro = rede_de_mentira["site"], rede_de_mentira["outro"]
    srv, _ = servidor.criar_servidor(config_navegador(interagir=False))
    async with Client(srv) as c:
        texto, sid = await abrir(c, f"{site.base}/form.html")
        for nome in ("Enviar aqui", "Enviar para fora"):
            r = await c.call_tool("clicar", {"sessao": sid, "ref": ref_de(texto, "botão", nome)})
            assert r.is_error and "não é um link" in r.content[0].text and "só leitura" in r.content[0].text
        r = await c.call_tool("digitar", {"sessao": sid, "ref": ref_de(texto, "campo", "Nome"), "texto": "x"})
        assert r.is_error and "digitar está desligado" in r.content[0].text
        # Página que se envia sozinha (POST para outro site) ao carregar.
        texto, _ = await abrir(c, f"{site.base}/auto-envio.html", sid)
        assert "Esta página envia sozinha" in texto
    assert [m for m, _, _ in outro.pedidos if m != "GET"] == [], "nenhum POST chegou ao outro site"
    assert site.metodos("/enviar") == []


async def test_formularios_no_modo_interagir(config_navegador, rede_de_mentira):
    site, outro = rede_de_mentira["site"], rede_de_mentira["outro"]
    srv, _ = servidor.criar_servidor(config_navegador(interagir=True))
    async with Client(srv) as c:
        texto, sid = await abrir(c, f"{site.base}/form.html")
        assert "modo interagir" in texto.splitlines()[0]
        # Para outra origem: recusado no digitar e no clicar.
        r = await c.call_tool("digitar", {"sessao": sid, "ref": ref_de(texto, "campo", "Mensagem"), "texto": "segredo"})
        assert r.is_error and "outra origem" in r.content[0].text
        r = await c.call_tool("clicar", {"sessao": sid, "ref": ref_de(texto, "botão", "Enviar para fora")})
        assert r.is_error and "outra origem" in r.content[0].text
        # Um botão comum cujo JavaScript faz POST para outro site: o clique
        # acontece, o POST não sai.
        r = await c.call_tool("clicar", {"sessao": sid, "ref": ref_de(texto, "botão", "Mandar por fetch")})
        assert not r.is_error and "outra origem" in r.content[0].text
        # Para a mesma origem: funciona.
        r = await c.call_tool("digitar", {"sessao": sid, "ref": ref_de(texto, "campo", "Nome"), "texto": "Ana"})
        assert not r.is_error and "Digitei 3 caractere(s)" in r.content[0].text
        r = await c.call_tool("clicar", {"sessao": sid, "ref": ref_de(texto, "botão", "Enviar aqui")})
        assert not r.is_error and "Formulário recebido." in r.content[0].text
        texto, _ = await abrir(c, f"{site.base}/form.html", sid)
        r = await c.call_tool("digitar", {"sessao": sid, "ref": ref_de(texto, "campo", "Busca"), "texto": "sono", "enter": True})
        assert not r.is_error and "Resultados da busca." in r.content[0].text
        assert r.structured_content["url"] == f"{site.base}/busca?q=sono"
        # Página que se envia sozinha para outro site: nem com interagir.
        await abrir(c, f"{site.base}/auto-envio.html", sid)
    assert site.metodos("/enviar") == ["POST"]
    assert [m for m, _, _ in outro.pedidos if m != "GET"] == [], "nenhum POST chegou ao outro site"


async def test_pagina_enorme_vem_cortada_e_continua(config_navegador, rede_de_mentira):
    site = rede_de_mentira["site"]
    cfg = config_navegador(NAVEGADOR_MAX_TEXTO_BYTES="20000")
    srv, _ = servidor.criar_servidor(cfg)
    async with Client(srv) as c:
        r = await c.call_tool("navegar", {"url": f"{site.base}/grande.html"})
        texto = r.content[0].text
        assert not r.is_error
        assert len(texto.encode()) <= cfg.max_texto_bytes + 8192
        assert "a página é grande demais" in texto
        marca = re.search(r"\[… instantâneo cortado pelo navegador: faltam (\d+) caracteres\. Para continuar, "
                          r"chame ler com sessao=(\w+) e inicio=(\d+) …\]", texto)
        assert marca, texto[-500:]
        sid, inicio = marca.group(2), int(marca.group(3))
        assert r.structured_content["fim"] == inicio
        r = await c.call_tool("ler", {"sessao": sid, "inicio": inicio})
        assert not r.is_error and f"continuando de {inicio}" in r.content[0].text
        assert r.content[0].text.split("---\n", 1)[1].startswith("Parágrafo ")
        r = await c.call_tool("ler", {"sessao": sid, "inicio": 10**9})
        assert r.is_error and "passa do fim" in r.content[0].text


async def test_pagina_que_nunca_responde(config_navegador, rede_de_mentira):
    site = rede_de_mentira["site"]
    cfg = config_navegador()
    srv, nav = servidor.criar_servidor(cfg)
    async with Client(srv) as c:
        r = await c.call_tool("navegar", {"url": f"{site.base}/trava"})
        assert r.is_error and f"não respondeu em {cfg.timeout_carregamento} s" in r.content[0].text
        assert nav.sessoes == {}, "a sessão nova que não abriu nada foi fechada"
        texto, sid = await abrir(c, f"{site.base}/sobre.html")
        r = await c.call_tool("navegar", {"url": f"{site.base}/trava", "sessao": sid})
        assert r.is_error and "a página continua a anterior" in r.content[0].text
        r = await c.call_tool("ler", {"sessao": sid})
        assert not r.is_error and "# Sobre nós" in r.content[0].text


async def test_javascript_em_laco_nao_trava_o_servidor(config_navegador, rede_de_mentira):
    site = rede_de_mentira["site"]
    cfg = config_navegador()
    srv, nav = servidor.criar_servidor(cfg)
    async with Client(srv) as c:
        texto, sid = await abrir(c, f"{site.base}/sobre.html")
        r = await c.call_tool("navegar", {"url": f"{site.base}/laco.html", "sessao": sid})
        assert r.is_error and ("travado" in r.content[0].text or "prazo" in r.content[0].text), r.content[0].text
        # O servidor continua respondendo e abre outra página.
        r = await c.call_tool("navegar", {"url": f"{site.base}/estatica.html"})
        assert not r.is_error, r.content[0].text


async def test_dialogos_abas_e_downloads(config_navegador, rede_de_mentira):
    site = rede_de_mentira["site"]
    srv, _ = servidor.criar_servidor(config_navegador(interagir=True, NAVEGADOR_MAX_PAGINAS="3"))
    async with Client(srv) as c:
        texto, sid = await abrir(c, f"{site.base}/popup.html")
        assert "diálogo alert dispensado: 'Clique em OK para ganhar um prêmio'" in texto
        assert "diálogo confirm dispensado" in texto
        r = await c.call_tool("clicar", {"sessao": sid, "ref": ref_de(texto, "link", "Abrir em outra aba")})
        assert "aba 2 de 2" in r.content[0].text and "# Sobre nós" in r.content[0].text
        r = await c.call_tool("abas", {"sessao": sid})
        assert "1. Insistente" in r.content[0].text and "2. Sobre — " in r.content[0].text and "← atual" in r.content[0].text

        texto, _ = await abrir(c, f"{site.base}/janelas.html", sid)
        r = await c.call_tool("clicar", {"sessao": sid, "ref": ref_de(texto, "botão", "Abrir")})
        assert "limite de 3 aba(s) por sessão" in r.content[0].text
        r = await c.call_tool("abas", {"sessao": sid})
        assert len(r.structured_content["abas"]) == 3

        r = await c.call_tool("abas", {"sessao": sid, "fechar": 3, "trocar_para": 1})
        assert len(r.structured_content["abas"]) == 2 and r.structured_content["abas"][0]["atual"]

        r = await c.call_tool("navegar", {"url": f"{site.base}/baixar", "sessao": sid})
        assert r.is_error and "download" in r.content[0].text
        texto, _ = await abrir(c, f"{site.base}/baixar.html", sid)
        r = await c.call_tool("clicar", {"sessao": sid, "ref": ref_de(texto, "link", "Baixar arquivo")})
        assert "downloads estão desligados" in r.content[0].text


async def test_limite_de_sessoes(config_navegador, rede_de_mentira):
    site = rede_de_mentira["site"]
    srv, _ = servidor.criar_servidor(config_navegador(NAVEGADOR_MAX_SESSOES="2"))
    async with Client(srv) as c:
        _, a = await abrir(c, f"{site.base}/sobre.html")
        _, b = await abrir(c, f"{site.base}/sobre.html")
        assert a != b
        r = await c.call_tool("navegar", {"url": f"{site.base}/sobre.html"})
        assert r.is_error and "limite de 2 sessão(ões)" in r.content[0].text
        await c.call_tool("fechar", {"sessao": a})
        await abrir(c, f"{site.base}/sobre.html")


async def test_sessoes_sao_anonimas_e_separadas(config_navegador, rede_de_mentira):
    site = rede_de_mentira["site"]
    srv, nav = servidor.criar_servidor(config_navegador())
    async with Client(srv) as c:
        _, a = await abrir(c, f"{site.base}/sobre.html")
        await nav.sessoes[a].contexto.add_cookies([{"name": "x", "value": "1", "url": site.base}])
        _, b = await abrir(c, f"{site.base}/sobre.html")
        assert await nav.sessoes[b].contexto.cookies() == []


async def test_capturar_tela_grava_no_workspace(config_navegador, rede_de_mentira):
    site = rede_de_mentira["site"]
    cfg = config_navegador(NAVEGADOR_MAX_CAPTURAS="2")
    srv, _ = servidor.criar_servidor(cfg)
    async with Client(srv) as c:
        _, sid = await abrir(c, f"{site.base}/estatica.html")
        for _ in range(3):
            r = await c.call_tool("capturar_tela", {"sessao": sid})
            assert not r.is_error, r.content[0].text
        arquivo = r.structured_content["arquivo"]
        assert arquivo == f"navegador/{sid}-003.png" and f"Captura salva no workspace: {arquivo}" in r.content[0].text
        png = (cfg.workspace / arquivo).read_bytes()
        assert png.startswith(b"\x89PNG") and r.structured_content["largura"] == 1280
        assert sorted(p.name for p in cfg.pasta_capturas.iterdir()) == [f"{sid}-002.png", f"{sid}-003.png"]
        r = await c.call_tool("capturar_tela", {"sessao": sid, "pagina_inteira": True})
        assert not r.is_error


async def test_capturas_nao_seguem_link_simbolico(config_navegador, rede_de_mentira, tmp_path):
    site = rede_de_mentira["site"]
    cfg = config_navegador()
    fora = tmp_path / "fora-do-workspace"
    fora.mkdir()
    cfg.workspace.mkdir(parents=True, exist_ok=True)
    cfg.pasta_capturas.symlink_to(fora)
    srv, _ = servidor.criar_servidor(cfg)
    async with Client(srv) as c:
        _, sid = await abrir(c, f"{site.base}/sobre.html")
        r = await c.call_tool("capturar_tela", {"sessao": sid})
        assert r.is_error and "link simbólico" in r.content[0].text
    assert list(fora.iterdir()) == []


async def test_limite_de_memoria_mata_o_chromium(config_navegador, rede_de_mentira):
    """Abaixo do que o Chromium usa parado: a vigilância mata a árvore e as
    sessões avisam o motivo."""
    site = rede_de_mentira["site"]
    srv, nav = servidor.criar_servidor(config_navegador(NAVEGADOR_MEMORIA_MB="192"))
    async with Client(srv) as c:
        r = await c.call_tool("navegar", {"url": f"{site.base}/grande.html"})
        sid = (r.structured_content or {}).get("sessao") or next(iter(nav.encerradas), "")
        assert esperar(lambda: sid in nav.encerradas, 10)
        r = await c.call_tool("ler", {"sessao": sid})
        assert r.is_error and "limite de memória" in r.content[0].text, r.content[0].text
        assert esperar(lambda: not nav._grupos_do_chromium(), 5), "a árvore do Chromium morreu"


async def test_servidor_fecha_o_chromium_ao_sair(config_navegador, rede_de_mentira):
    site = rede_de_mentira["site"]
    srv, nav = servidor.criar_servidor(config_navegador())
    async with Client(srv) as c:
        await abrir(c, f"{site.base}/sobre.html")
        grupos = set(nav._grupos_do_chromium())
        assert grupos
        # O perfil temporário do Chromium fica na pasta do servidor, não no /tmp.
        assert any(p.name.startswith("playwright") for p in nav._tmp.iterdir())
        assert "TMPDIR" not in os.environ or os.environ["TMPDIR"] != str(nav._tmp)
    assert esperar(lambda: all(not processos.membros_do_grupo(g) for g in grupos), 5)
    assert not nav._tmp.exists()


SERVIDOR_QUE_VAI_MORRER = r"""
import asyncio, json, sys
import config, sessoes

async def main():
    nav = sessoes.Navegador(config.carregar(json.loads(sys.argv[1])))
    await nav.iniciar()
    sessao = await nav.nova_sessao()
    await sessao.pagina.goto(sys.argv[2])
    print(json.dumps(sorted(nav._grupos_do_chromium())), flush=True)
    await asyncio.sleep(300)

asyncio.run(main())
"""


def test_servidor_morto_com_sigkill_nao_deixa_chromium_orfao(env_navegador, rede_de_mentira):
    """Como o kernel faz ao estourar um limite: SIGKILL no grupo do servidor.
    O Chromium está noutro grupo e, congelado (SIGSTOP), nem percebe que o
    cano fechou: só o vigia pode matá-lo."""
    url = rede_de_mentira["site"].base + "/sobre.html"
    processo = subprocess.Popen(
        [sys.executable, "-c", SERVIDOR_QUE_VAI_MORRER, json.dumps(env_navegador()), url],
        cwd=PASTA_SERVIDOR,
        stdout=subprocess.PIPE,
        text=True,
        start_new_session=True,
    )
    grupos: list[int] = []
    try:
        grupos = json.loads(processo.stdout.readline() or "[]")
        assert grupos, "o Chromium não subiu"
        for grupo in grupos:
            os.killpg(grupo, signal.SIGSTOP)
        time.sleep(1.5)  # o vigia anota os grupos a cada 0,5 s
        os.killpg(processo.pid, signal.SIGKILL)
        processo.wait()
        assert esperar(lambda: all(not processos.membros_do_grupo(g) for g in grupos), 10), "Chromium órfão"
    finally:
        processos.matar_grupos([processo.pid, *grupos])
