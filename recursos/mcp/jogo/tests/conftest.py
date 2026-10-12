"""Fases pequenas, desenhadas à mão, usadas em vários testes."""

import pytest

import desenho

# O exemplo da especificação (formato da fase 4): mínimo de 15 jogadas.
EXEMPLO_FASE_4 = [
    "# # # # # # # # #",
    "# V . . _ # . K #",
    "# . . . . D . . #",
    "# R . . . # . . #",
    "# # # # # # G # #",
]


@pytest.fixture
def exemplo():
    return desenho.fase(EXEMPLO_FASE_4, numero=4, nome="Segura a porta")


@pytest.fixture
def anyio_backend():
    return "asyncio"
