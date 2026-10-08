"""Divisão no limite de 2000 do Discord, sem quebrar blocos de código."""

from partes import dividir, tamanho


def cercas_fechadas(parte: str) -> bool:
    return sum(1 for l in parte.split("\n") if l.lstrip().startswith("```")) % 2 == 0


def test_curto_fica_inteiro():
    assert dividir("oi") == ["oi"]
    assert dividir("") == ["(vazio)"]


def test_texto_longo_quebra_em_fim_de_linha_sem_perder_nada():
    texto = "\n".join(f"linha {i} " + "x" * 50 for i in range(200))
    partes = dividir(texto)
    assert len(partes) > 1
    assert all(tamanho(p) <= 2000 for p in partes)
    assert "\n".join(partes) == texto


def test_bloco_de_codigo_cortado_e_fechado_e_reaberto():
    codigo = "\n".join(f"    print({i})  # " + "y" * 30 for i in range(120))
    texto = f"Veja:\n```python\n{codigo}\n```\nFim."
    partes = dividir(texto)
    assert len(partes) >= 3
    for p in partes:
        assert tamanho(p) <= 2000
        assert cercas_fechadas(p), p[:80]
    # O pedaço que continua o bloco reabre com a mesma linguagem.
    assert all(p.startswith("```python") for p in partes[1:-1])
    assert partes[-1].rstrip().endswith("Fim.")
    # Nenhuma linha de código se perdeu.
    juntas = "\n".join(partes)
    assert all(f"print({i})" in juntas for i in range(120))


def test_linha_gigante_e_emoji_contam_como_o_discord():
    partes = dividir("a" * 4500)
    assert all(tamanho(p) <= 2000 for p in partes)
    assert "".join(partes) == "a" * 4500
    emojis = "😀" * 1500  # 3000 unidades UTF-16
    partes = dividir(emojis)
    assert len(partes) == 2 and all(tamanho(p) <= 2000 for p in partes)
    assert "".join(partes) == emojis
