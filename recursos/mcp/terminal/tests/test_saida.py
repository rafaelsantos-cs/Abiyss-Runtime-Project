"""Corte da saída: começo e fim, nunca acima do limite em bytes UTF-8."""

import isolamento


def capturar(limite, *pedacos):
    c = isolamento.Captura(limite)
    for p in pedacos:
        c.adicionar(p)
    return c


def test_saida_pequena_vem_inteira():
    c = capturar(100, b"ola\n", b"mundo\n")
    assert c.texto() == ("ola\nmundo\n", False)
    assert c.total == 10


def test_saida_no_limite_exato_vem_inteira():
    c = capturar(10, b"0123456789")
    assert c.texto() == ("0123456789", False)


def test_saida_grande_guarda_comeco_e_fim_com_marca():
    dados = b"".join(f"linha {i:05d}\n".encode() for i in range(20000))
    c = isolamento.Captura(1000)
    for i in range(0, len(dados), 4096):  # em pedaços, como vem do pipe
        c.adicionar(dados[i : i + 4096])
    texto, cortado = c.texto()
    assert cortado
    assert c.total == len(dados)
    assert texto.startswith("linha 00000\n")
    assert texto.endswith("linha 19999\n")
    assert "saída cortada pelo terminal" in texto and f"{len(dados)} bytes no total" in texto
    corpo = texto.replace(texto[texto.index("\n[…") : texto.index("…]\n") + 3], "")
    assert len(corpo.encode()) <= 1000


def test_memoria_da_captura_e_limitada():
    c = isolamento.Captura(1000)
    for _ in range(1000):
        c.adicionar(b"x" * 65536)
    assert len(c._inicio) + len(c._fim) <= 1000
    assert c.total == 1000 * 65536


def test_utf8_invalido_e_caractere_cortado_no_meio():
    # "ã" tem 2 bytes; o corte cai no meio de vários deles.
    c = capturar(11, "ã".encode() * 50)
    texto, cortado = c.texto()
    assert cortado
    cabeca, cauda = texto.split("\n[…")[0], texto.split("…]\n")[1]
    assert set(cabeca) == {"ã"} and set(cauda) == {"ã"}
    assert len(cabeca.encode()) <= 5 and len(cauda.encode()) <= 6


def test_binario_nao_estoura_o_limite_por_causa_da_substituicao():
    # Cada byte inválido vira U+FFFD (3 bytes): o limite vale para o texto final.
    c = capturar(100, b"\xff" * 90)
    texto, cortado = c.texto()
    assert cortado
    corpo = texto.split("\n[…")[0] + texto.split("…]\n")[1]
    assert len(corpo.encode()) <= 100


def test_nulo_vira_simbolo_visivel():
    assert capturar(100, b"a\x00b").texto() == ("a␀b", False)


def test_cortes_respeitam_caracteres():
    assert isolamento.cortar_inicio("aãb", 2) == "a"
    assert isolamento.cortar_fim("aãb", 2) == "b"
    assert isolamento.cortar_fim("aãb", 3) == "ãb"
