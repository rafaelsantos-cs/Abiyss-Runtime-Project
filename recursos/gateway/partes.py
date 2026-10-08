"""Divide um texto nas mensagens do Discord (no máximo 2000 caracteres cada)
sem quebrar blocos de código.

- Quebra em fim de linha; linha maior que o limite é cortada no meio.
- Se o corte cai dentro de um bloco ```, o pedaço fecha o bloco e o
  próximo reabre com a mesma linguagem: cada mensagem tem os blocos
  fechados (o Discord formata cada mensagem sozinha).
- O Discord conta o tamanho em unidades UTF-16 (um emoji conta 2); aqui
  também, para nunca passar do limite.
"""

from __future__ import annotations

LIMITE_DISCORD = 2000
CERCA = "```"
# Linha que reabre um bloco: no máximo isto (```python, ```json...).
MAX_ABERTURA = 40


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


def _eh_cerca(linha: str) -> bool:
    return linha.lstrip().startswith(CERCA)


def dividir(texto: str, limite: int = LIMITE_DISCORD) -> list[str]:
    """Pedaços de até ``limite`` unidades, com os blocos de código fechados
    em cada um."""
    if tamanho(texto) <= limite:
        return [texto] if texto.strip() else ["(vazio)"]
    # Espaço guardado em cada pedaço para reabrir e fechar um bloco.
    folga = MAX_ABERTURA + len(CERCA) + 2
    if limite <= folga * 2:
        raise ValueError("limite pequeno demais")
    partes: list[str] = []
    atual: list[str] = []
    usado = 0  # tamanho de "\n".join(atual)
    aberta: str | None = None  # linha que abriu o bloco ainda aberto

    def fechar_pedaco() -> None:
        nonlocal atual, usado
        linhas = atual + ([CERCA] if aberta else [])
        pedaco = "\n".join(linhas)
        if pedaco.strip():
            partes.append(pedaco)
        atual = [aberta] if aberta else []
        usado = tamanho(aberta) if aberta else 0

    for linha in texto.split("\n"):
        # Linha longa demais: em pedaços que cabem com a folga.
        pedacos = []
        resto = linha
        while tamanho(resto) > limite - folga:
            cabe, resto = cortar(resto, limite - folga)
            pedacos.append(cabe)
        pedacos.append(resto)
        for pedaco in pedacos:
            custo = tamanho(pedaco) + (1 if atual else 0)
            # O bloco continua aberto depois desta linha? Então guarda o
            # espaço do fechamento.
            abre_ou_segue = (aberta is None) == _eh_cerca(pedaco)
            fechamento = len(CERCA) + 1 if abre_ou_segue else 0
            if atual and usado + custo + fechamento > limite:
                fechar_pedaco()
                custo = tamanho(pedaco) + (1 if atual else 0)
            atual.append(pedaco)
            usado += custo
            if _eh_cerca(pedaco):
                aberta = None if aberta else pedaco.strip()[: MAX_ABERTURA]
    if atual:
        aberta_final = aberta
        aberta = None  # um bloco sem fechamento no texto fica como veio
        if atual != ([aberta_final] if aberta_final else []):
            fechar_pedaco()
    return partes or ["(vazio)"]
