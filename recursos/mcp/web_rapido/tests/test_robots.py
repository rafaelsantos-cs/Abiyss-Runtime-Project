"""robots.txt pelo RFC 9309 (o que o urllib.robotparser não faz)."""

import robots

TEXTO = """
User-agent: *
Disallow: /

User-agent: AbiyssBot
User-agent: OutroBot
Disallow: /privado/
Disallow: /*?sessao=
Disallow: /*.pdf$
Allow: /privado/publico.html
Crawl-delay: 4

User-agent: abiyssbot
Disallow: /mais-um-grupo/
"""


def test_grupo_do_robo_soma_grupos_com_o_mesmo_nome_e_ignora_o_asterisco():
    r = robots.interpretar(TEXTO)
    assert r.permite("AbiyssBot", "https://x.com/artigo.html")
    assert not r.permite("AbiyssBot", "https://x.com/mais-um-grupo/a")
    assert not r.permite("QualquerBot", "https://x.com/artigo.html"), "sem grupo próprio, vale o *"


def test_regra_mais_longa_vence_e_allow_desempata():
    r = robots.interpretar(TEXTO)
    assert not r.permite("AbiyssBot", "https://x.com/privado/segredo.html")
    assert r.permite("AbiyssBot", "https://x.com/privado/publico.html")
    empate = robots.interpretar("User-agent: *\nDisallow: /a\nAllow: /a\n")
    assert empate.permite("bot", "https://x.com/a")


def test_curingas():
    r = robots.interpretar(TEXTO)
    assert not r.permite("AbiyssBot", "https://x.com/busca?sessao=abc")
    # "/*?sessao=" exige o "?" logo antes: "&sessao=" não casa (como no RFC e no Google).
    assert r.permite("AbiyssBot", "https://x.com/busca?q=1&sessao=abc")
    assert r.permite("AbiyssBot", "https://x.com/busca?q=1")
    assert not r.permite("AbiyssBot", "https://x.com/docs/manual.pdf")
    assert r.permite("AbiyssBot", "https://x.com/docs/manual.pdf?v=2"), "$ ancora no fim"


def test_casos_de_borda():
    assert robots.interpretar("User-agent: *\nDisallow:\n").permite("bot", "https://x.com/qualquer")
    assert robots.interpretar("").permite("bot", "https://x.com/a")
    assert robots.interpretar("User-agent: *\nDisallow: /\n").permite("bot", "https://x.com/robots.txt")
    # %XX comparado sem codificação dos dois lados.
    assert not robots.interpretar("User-agent: *\nDisallow: /caf%C3%A9\n").permite("bot", "https://x.com/café")
    # Regra antes de qualquer User-agent: ignorada.
    assert robots.interpretar("Disallow: /\nUser-agent: *\nAllow: /\n").permite("bot", "https://x.com/a")


def test_crawl_delay_e_tudo_ou_nada():
    r = robots.interpretar(TEXTO)
    assert r.atraso("AbiyssBot") == 4
    assert r.atraso("QualquerBot") is None
    assert robots.Robots.permite_tudo().permite("bot", "https://x.com/a")
    assert not robots.Robots.proibe_tudo().permite("bot", "https://x.com/a")
