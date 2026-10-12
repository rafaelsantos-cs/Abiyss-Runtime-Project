"""Planta: o rascunho de uma fase enquanto o molde desenha.

Uma grade de caracteres (células-base) mais os objetos com posição. No fim,
`fase()` aplica uma das 8 simetrias do quadrado (sorteada pelo molde) e
devolve a `motor.Fase` validada.
"""

from __future__ import annotations

import random

import motor
from motor import Pos


class Planta:
    def __init__(self, largura: int, altura: int, preencher: str = motor.PAREDE):
        self.largura, self.altura = largura, altura
        self.g = [[preencher] * largura for _ in range(altura)]
        self.avatares: dict[str, Pos] = {}
        self.blocos: list[Pos] = []
        self.itens: dict[Pos, str] = {}
        self.placas: dict[str, Pos] = {}
        self.portas: list[tuple[str, Pos, tuple[str, ...]]] = []
        self.trancadas: dict[Pos, str] = {}

    # -- células ------------------------------------------------------------

    def __getitem__(self, p: Pos) -> str:
        return self.g[p[1]][p[0]]

    def cel(self, p: Pos, c: str) -> None:
        self.g[p[1]][p[0]] = c

    def ret(self, x0: int, y0: int, x1: int, y1: int, c: str) -> None:
        """Retângulo inclusivo."""
        for y in range(min(y0, y1), max(y0, y1) + 1):
            for x in range(min(x0, x1), max(x0, x1) + 1):
                self.g[y][x] = c

    def borda(self) -> None:
        for x in range(self.largura):
            self.g[0][x] = self.g[self.altura - 1][x] = motor.PAREDE
        for y in range(self.altura):
            self.g[y][0] = self.g[y][self.largura - 1] = motor.PAREDE

    def celulas(self, *tipos: str) -> list[Pos]:
        return [(x, y) for y in range(self.altura) for x in range(self.largura) if self.g[y][x] in tipos]

    def livres(self, *tipos: str) -> list[Pos]:
        """Células dos tipos pedidos (padrão: chão) sem nenhum objeto."""
        tipos = tipos or (motor.CHAO,)
        ocupadas = set(self.avatares.values()) | set(self.blocos) | set(self.itens)
        return [p for p in self.celulas(*tipos) if p not in ocupadas]

    # -- objetos ------------------------------------------------------------

    def placa(self, id_: str, p: Pos) -> None:
        self.cel(p, motor.PLACA)
        self.placas[id_] = p

    def porta(self, id_: str, p: Pos, placas: str | tuple[str, ...]) -> None:
        self.cel(p, motor.PORTA)
        self.portas.append((id_, p, tuple(placas)))

    def trancada(self, p: Pos, cor: str) -> None:
        self.cel(p, motor.TRANCADA)
        self.trancadas[p] = cor

    def portao(self, p: Pos) -> None:
        self.cel(p, motor.PORTAO)

    def avatar(self, a: str, p: Pos) -> None:
        self.avatares[a] = p

    def bloco(self, p: Pos) -> None:
        self.blocos.append(p)

    def item(self, p: Pos, item: str) -> None:
        self.itens[p] = item

    # -- saída --------------------------------------------------------------

    def fase(
        self,
        numero: int,
        nome: str,
        *,
        nevoa: bool = False,
        simetria: int = 0,
    ) -> motor.Fase:
        """`simetria` de 0 a 7: rotação (0, 90, 180, 270) × espelho.
        Só vale para grades quadradas (todas as fases são)."""
        t = transformacao(self.largura, simetria)
        grade = [[motor.PAREDE] * self.largura for _ in range(self.altura)]
        for y in range(self.altura):
            for x in range(self.largura):
                nx, ny = t((x, y))
                grade[ny][nx] = self.g[y][x]
        f = motor.Fase(
            numero=numero,
            nome=nome,
            largura=self.largura,
            altura=self.altura,
            celulas=tuple("".join(linha) for linha in grade),
            placas={i: t(p) for i, p in self.placas.items()},
            portas=tuple(motor.PortaDePlaca(i, t(p), pl) for i, p, pl in self.portas),
            trancadas={t(p): c for p, c in self.trancadas.items()},
            avatares={a: t(p) for a, p in self.avatares.items()},
            blocos=tuple(sorted(t(p) for p in self.blocos)),
            itens={t(p): i for p, i in self.itens.items()},
            nevoa=nevoa,
            raios=dict(motor.RAIOS_PADRAO),
        )
        f.validar()
        return f

    def desenho(self) -> str:
        """Para depurar: a grade com os objetos por cima."""
        linhas = []
        for y in range(self.altura):
            linha = []
            for x in range(self.largura):
                p = (x, y)
                c = self.g[y][x]
                for a, q in self.avatares.items():
                    if q == p:
                        c = a
                if p in self.blocos:
                    c = "B"
                if p in self.itens:
                    c = "K" if self.itens[p] == motor.DOURADA else "k"
                linha.append(c)
            linhas.append(" ".join(linha))
        return "\n".join(linhas)


def transformacao(n: int, simetria: int):
    """Uma das 8 simetrias de um quadrado n × n."""
    giros, espelho = simetria % 4, simetria // 4 % 2

    def t(p: Pos) -> Pos:
        x, y = p
        if espelho:
            x = n - 1 - x
        for _ in range(giros):
            x, y = n - 1 - y, x
        return (x, y)

    return t


def sortear_simetria(rng: random.Random) -> int:
    return rng.randrange(8)
