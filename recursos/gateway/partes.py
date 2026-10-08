"""Divide um texto nas mensagens do Discord (no máximo 2000 caracteres cada).

O Discord conta o tamanho em unidades UTF-16 (um emoji conta 2); aqui
também, para nunca passar do limite.
"""

from __future__ import annotations

LIMITE_DISCORD = 2000


def tamanho(texto: str) -> int:
    """Tamanho como o Discord conta (unidades UTF-16)."""
    return len(texto.encode("utf-16-le")) // 2


def cortar(texto: str, limite: int) -> tuple[str, str]:
    """O maior começo de ``texto`` que cabe em ``limite`` e o resto."""
    usado = 0
    for i, c in enumerate(texto):
        usado += 2 if ord(c) > 0xFFFF else 1
        if usado > limite:
            return texto[:i], texto[i:]
    return texto, ""


def dividir(texto: str, limite: int = LIMITE_DISCORD) -> list[str]:
    """Pedaços de até ``limite``, quebrando em fim de linha quando dá."""
    partes: list[str] = []
    resto = texto
    while tamanho(resto) > limite:
        cabe, _ = cortar(resto, limite)
        quebra = cabe.rfind("\n")
        if quebra <= 0:
            quebra = len(cabe)
        partes.append(resto[:quebra])
        resto = resto[quebra:].lstrip("\n")
    if resto or not partes:
        partes.append(resto)
    return partes
