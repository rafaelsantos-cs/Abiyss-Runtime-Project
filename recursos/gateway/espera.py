"""Espera entre tentativas de reconexão: exponencial, com teto e sorteio.

Usada nas duas pontas que caem: o socket do daemon (daemon reiniciando,
fora do ar) e o login no Discord (rede fora, Discord instável). O sorteio
evita que vários processos tentem sempre no mesmo instante.
"""

from __future__ import annotations

import random
from collections.abc import Callable


class Espera:
    def __init__(
        self,
        base: float = 1.0,
        teto: float = 60.0,
        fator: float = 2.0,
        sorteio: float = 0.2,
        aleatorio: Callable[[], float] = random.random,
    ) -> None:
        if base <= 0 or teto < base or fator < 1 or not 0 <= sorteio < 1:
            raise ValueError("espera inválida")
        self.base, self.teto, self.fator, self.sorteio = base, teto, fator, sorteio
        self._aleatorio = aleatorio
        self.tentativas = 0

    def proxima(self) -> float:
        """Quanto esperar antes da próxima tentativa (e conta a tentativa)."""
        bruto = min(self.teto, self.base * self.fator**self.tentativas)
        self.tentativas += 1
        # ±sorteio, sem passar do teto.
        variacao = (self._aleatorio() * 2 - 1) * self.sorteio
        return min(self.teto, bruto * (1 + variacao))

    def zerar(self) -> None:
        """Conectou: a próxima queda começa de novo da base."""
        self.tentativas = 0
