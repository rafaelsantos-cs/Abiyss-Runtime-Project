"""Os moldes das 12 fases.

Cada molde recebe um `random.Random` (já semeado pelo gerador) e devolve um
`Rascunho`: a planta desenhada, a simetria sorteada e o que os testes de
aceite da fase precisam (a armadilha a procurar, as alas). Quem decide se o
rascunho entra no catálogo é o gerador (solucionador + testes de aceite).

Convenção: os moldes desenham numa orientação fixa; a simetria sorteada
gira/espelha tudo no fim, para a mesma ideia aparecer de jeitos diferentes.
"""

from __future__ import annotations

import random
from dataclasses import dataclass, field

import motor
from motor import Pos
from planta import Planta, sortear_simetria

CHAO, PAREDE, AGUA, FRESTA = motor.CHAO, motor.PAREDE, motor.AGUA, motor.FRESTA


@dataclass
class Armadilha:
    """Um estado alcançável que deixa a fase sem solução: algum bloco numa
    destas células, ou alguma destas células de água virou ponte."""

    blocos_em: tuple[Pos, ...] = ()
    pontes_em: tuple[Pos, ...] = ()


@dataclass
class Rascunho:
    planta: Planta
    simetria: int
    armadilha: Armadilha | None = None
    # Fase 10: (avatar, placa) de cada ala.
    alas: tuple[tuple[str, str], ...] = ()
    notas: dict = field(default_factory=dict)


def vizinhos4(p: Pos) -> list[Pos]:
    x, y = p
    return [(x, y - 1), (x, y + 1), (x + 1, y), (x - 1, y)]


# ---------------------------------------------------------------------------
# 1. Lago — o Vau vai até a ilha buscar a Chave Dourada e volta ao portão.
# ---------------------------------------------------------------------------


def lago(rng: random.Random) -> Rascunho:
    p = Planta(7, 7)
    p.ret(1, 1, 5, 5, AGUA)
    p.ret(1, 1, 5, 1, CHAO)  # a margem
    ilha = (rng.randint(1, 5), rng.randint(3, 5))
    p.cel(ilha, CHAO)
    if rng.random() < 0.5:
        extra = rng.choice([q for q in vizinhos4(ilha) if 1 <= q[0] <= 5 and 3 <= q[1] <= 5])
        p.cel(extra, CHAO)
    p.item(ilha, motor.DOURADA)
    for _ in range(rng.randint(0, 2)):
        pedra = (rng.randint(1, 5), rng.randint(2, 5))
        if p[pedra] == AGUA and pedra not in vizinhos4(ilha):
            p.cel(pedra, PAREDE)
    xs = rng.sample(range(1, 6), 2)
    p.avatar("V", (xs[0], 1))
    p.portao((xs[1], 1))
    return Rascunho(p, sortear_simetria(rng))


# ---------------------------------------------------------------------------
# 2. Empurrão — a Rocha leva um bloco até a placa, a porta abre e ela busca
#    a chave.
# ---------------------------------------------------------------------------


def _bloco_e_placa(rng: random.Random, p: Planta, x0: int, y0: int, x1: int, y1: int) -> tuple[Pos, Pos]:
    """Uma placa e um bloco alinhados na área, com espaço para empurrar."""
    for _ in range(200):
        placa = (rng.randint(x0, x1), rng.randint(y0, y1))
        dx, dy = rng.choice([(1, 0), (-1, 0), (0, 1), (0, -1)])
        dist = rng.randint(1, 2)
        bloco = (placa[0] - dx * dist, placa[1] - dy * dist)
        atras = (bloco[0] - dx, bloco[1] - dy)
        if all(x0 <= q[0] <= x1 and y0 <= q[1] <= y1 for q in (bloco, atras)):
            return placa, bloco
    raise RuntimeError("não achei bloco e placa")


def empurrao(rng: random.Random) -> Rascunho:
    p = Planta(8, 8)
    p.ret(1, 1, 6, 6, CHAO)
    c = rng.choice([3, 4])
    p.ret(c, 1, c, 6, PAREDE)
    porta = (c, rng.randint(2, 5))
    placa, bloco = _bloco_e_placa(rng, p, 1, 1, c - 1, 6)
    p.placa("a", placa)
    p.porta("A", porta, "a")
    p.bloco(bloco)
    livres = [q for q in p.livres() if q[0] < c and q != placa]
    p.avatar("R", rng.choice(livres))
    direita = [q for q in p.livres() if q[0] > c]
    p.item(rng.choice(direita), motor.DOURADA)
    # Portão numa parede da borda, do lado da chave ou do lado da Rocha.
    if rng.random() < 0.6:
        gate = rng.choice([(7, y) for y in range(1, 7)] + [(x, rng.choice([0, 7])) for x in range(c + 1, 7)])
    else:
        gate = rng.choice([(0, y) for y in range(1, 7)])
    p.portao(gate)
    for _ in range(rng.randint(0, 2)):
        q = rng.choice(p.livres())
        if q not in vizinhos4(porta) and q not in vizinhos4(gate) and q != placa:
            p.cel(q, PAREDE)
    return Rascunho(p, sortear_simetria(rng))


# ---------------------------------------------------------------------------
# 3. Ponte — duas células de água entre a Rocha e a chave, e exatamente dois
#    blocos. Um deles começa perto de um canto: empurrado pelo lado errado,
#    trava e a fase fica sem solução.
# ---------------------------------------------------------------------------


def ponte(rng: random.Random) -> Rascunho:
    p = Planta(9, 9)
    p.ret(1, 1, 7, 3, CHAO)  # área de cima: a Rocha e os blocos
    p.ret(1, 6, 7, 7, CHAO)  # área de baixo: a chave
    cx = rng.randint(2, 6)
    p.cel((cx, 4), AGUA)
    p.cel((cx, 5), AGUA)
    # Bloco perto de um canto (a armadilha) e outro solto.
    canto_x = rng.choice([2, 6])
    p.bloco((canto_x, 2))
    outros = [(x, y) for x in range(2, 7) for y in (2, 3) if abs(x - canto_x) >= 2 and (x, y) != (cx, 3)]
    p.bloco(rng.choice(outros))
    livres = [q for q in p.livres() if q[1] <= 3]
    p.avatar("R", rng.choice(livres))
    p.item(rng.choice([q for q in p.livres() if q[1] >= 6]), motor.DOURADA)
    if rng.random() < 0.5:
        gate = rng.choice([(x, 8) for x in range(1, 8)])
    else:
        gate = rng.choice([(x, 0) for x in range(1, 8)])
    p.portao(gate)
    # Um bloco encostado na parede de cima ou numa lateral da área de cima
    # nunca mais desce: a fase trava.
    presos = tuple([(x, 1) for x in range(1, 8)] + [(1, y) for y in (2, 3)] + [(7, y) for y in (2, 3)])
    return Rascunho(p, sortear_simetria(rng), armadilha=Armadilha(blocos_em=presos))


# ---------------------------------------------------------------------------
# 4. Segura a porta — um avatar fica na placa enquanto o outro passa.
# ---------------------------------------------------------------------------


def segura_a_porta(rng: random.Random) -> Rascunho:
    p = Planta(9, 9)
    p.ret(1, 1, 7, 7, CHAO)
    c = rng.choice([4, 5])
    p.ret(c, 1, c, 7, PAREDE)
    porta = (c, rng.randint(2, 6))
    p.porta("A", porta, "a")
    esquerda = [q for q in p.livres() if q[0] < c - 1 or (q[0] == c - 1 and abs(q[1] - porta[1]) > 1)]
    placa = rng.choice(esquerda)
    p.placa("a", placa)
    v, r = rng.sample([q for q in p.livres() if q[0] < c], 2)
    p.avatar("V", v)
    p.avatar("R", r)
    p.item(rng.choice([q for q in p.livres() if q[0] > c + 1]), motor.DOURADA)
    gate = rng.choice([(x, rng.choice([0, 8])) for x in range(c + 1, 8)] + [(8, y) for y in range(1, 8)])
    p.portao(gate)
    for _ in range(rng.randint(1, 4)):
        q = rng.choice(p.livres())
        if q not in vizinhos4(porta) and q not in vizinhos4(gate) and q not in vizinhos4(placa):
            p.cel(q, PAREDE)
    return Rascunho(p, sortear_simetria(rng))


# ---------------------------------------------------------------------------
# Ajudantes de desenho
# ---------------------------------------------------------------------------


def serpentina(p: Planta, x0: int, y0: int, x1: int, y1: int, rng: random.Random, passo: int = 2) -> None:
    """Paredes horizontais alternadas dentro do retângulo, com a passagem
    ora numa ponta, ora na outra: o caminho fica longo."""
    esquerda = rng.random() < 0.5
    for y in range(y0 + 1, y1, passo):
        p.ret(x0, y, x1, y, PAREDE)
        p.cel((x0, y) if esquerda else (x1, y), CHAO)
        esquerda = not esquerda


def borda_de(p: Planta, celulas: list[Pos]) -> list[Pos]:
    """Paredes da borda do mapa vizinhas de alguma destas células (lugar de
    portão)."""
    saida = []
    for c in celulas:
        for q in vizinhos4(c):
            x, y = q
            if (x in (0, p.largura - 1) or y in (0, p.altura - 1)) and p[q] == PAREDE:
                if not (x in (0, p.largura - 1) and y in (0, p.altura - 1)):
                    saida.append(q)
    return sorted(set(saida))


# ---------------------------------------------------------------------------
# 5. Mão em mão — só o Vau alcança a chave vermelha, numa ilha; depois da
#    porta vermelha há um bloco que só a Rocha tira. Entregar poupa jogadas.
# ---------------------------------------------------------------------------


def mao_em_mao(rng: random.Random) -> Rascunho:
    p = Planta(10, 10)
    c = rng.choice([4, 5])
    ys = rng.choice([3, 4])
    p.ret(1, 1, c - 1, ys, CHAO)  # margem
    p.ret(1, ys + 1, c - 1, 8, AGUA)  # lago
    ilha = (rng.randint(1, c - 1), rng.randint(ys + 2, 8))
    p.cel(ilha, CHAO)
    p.item(ilha, "vermelha")
    yd = rng.randint(1, ys)
    p.trancada((c, yd), "vermelha")
    # Depois da porta: corredor com o bloco, um nicho no fim e a descida.
    vx = rng.randint(c + 2, 7)
    p.ret(c + 1, yd, 8, yd, CHAO)
    p.ret(vx, yd, vx, 8, CHAO)
    p.ret(c + 1, 7, 8, 8, CHAO)
    p.bloco((c + rng.randint(1, max(1, vx - c - 1)), yd))
    sala = [q for q in p.livres() if q[1] >= 7 and q[0] > c]
    p.item(rng.choice(sala), motor.DOURADA)
    p.portao(rng.choice(borda_de(p, [q for q in p.celulas(CHAO) if q[1] >= 7 and q[0] > c])))
    margem = [q for q in p.livres() if q[0] < c and q[1] <= ys]
    v, r = rng.sample(margem, 2)
    p.avatar("V", v)
    p.avatar("R", r)
    return Rascunho(p, sortear_simetria(rng))


# ---------------------------------------------------------------------------
# 6. Duas placas — a porta precisa de duas placas: uma só se alcança pela
#    água (o Vau fica nela), a outra recebe um bloco empurrado pela Rocha.
# ---------------------------------------------------------------------------


def duas_placas(rng: random.Random) -> Rascunho:
    p = Planta(10, 10)
    c = rng.choice([5, 6])
    p.ret(1, 1, c - 1, 8, CHAO)
    # Lago num canto de baixo, com a placa a numa ilha no fundo.
    lw = rng.randint(3, c - 1)
    lh = rng.randint(3, 4)
    p.ret(1, 9 - lh, lw, 8, AGUA)
    ilha = (rng.randint(1, lw - 1), 8)
    p.placa("a", ilha)
    # Placa b e bloco na terra.
    for _ in range(100):
        placa, bloco = _bloco_e_placa(rng, p, 1, 1, c - 1, 8 - lh)
        atras = (2 * bloco[0] - placa[0], 2 * bloco[1] - placa[1])
        if p[placa] == CHAO and p[bloco] == CHAO and p[atras] == CHAO and placa != ilha:
            break
    else:
        raise RuntimeError("sem lugar para a placa b")
    p.placa("b", placa)
    p.bloco(bloco)
    p.porta("A", (c, rng.randint(1, 8 - lh)), "ab")
    # Do outro lado: serpentina até a chave.
    p.ret(c + 1, 1, 8, 8, CHAO)
    serpentina(p, c + 1, 1, 8, 8, rng)
    lado = [q for q in p.livres() if q[0] > c]
    p.item(max(lado, key=lambda q: (abs(q[1] - p.portas[0][1][1]), rng.random())), motor.DOURADA)
    p.portao(rng.choice(borda_de(p, [q for q in p.celulas(CHAO) if q[0] < c] + lado)))
    terra = [q for q in p.livres() if q[0] < c]
    v, r = rng.sample(terra, 2)
    p.avatar("V", v)
    p.avatar("R", r)
    return Rascunho(p, sortear_simetria(rng))


# ---------------------------------------------------------------------------
# 7. Corrente — a chave vermelha abre a área da azul, e a azul abre a área da
#    Chave Dourada. Cada etapa exige o terreno do outro avatar.
# ---------------------------------------------------------------------------


def corrente(rng: random.Random) -> Rascunho:
    p = Planta(11, 11)
    # Faixa de cima: margem (x 1..3) e a área do meio (x 5..9).
    p.ret(1, 1, 3, 3, CHAO)
    p.ret(1, 4, 3, 9, AGUA)  # lago 1, com a chave vermelha
    ilha = (rng.randint(1, 3), rng.randint(6, 9))
    p.cel(ilha, CHAO)
    p.item(ilha, "vermelha")
    yr = rng.randint(1, 3)
    p.trancada((4, yr), "vermelha")
    # Área do meio: corredor na linha yr com o bloco e um nicho no fim.
    p.ret(5, yr, 9, yr, CHAO)
    p.bloco((5, yr))
    # Descida até a chave azul (beco), e a porta azul para o lago 2.
    xs = rng.sample(range(6, 9), 2)
    xa, xb = xs
    ya = 3 if yr < 3 else 1
    p.ret(xa, min(yr, ya), xa, max(yr, ya), CHAO)
    p.item((xa, ya), "azul")
    p.ret(xb, yr, xb, 3, CHAO)
    p.trancada((xb, 4), "azul")
    p.ret(5, 5, 9, 9, AGUA)  # lago 2, com a Chave Dourada
    ilha2 = (rng.randint(5, 9), rng.randint(7, 9))
    p.cel(ilha2, CHAO)
    p.item(ilha2, motor.DOURADA)
    if rng.random() < 0.5:
        p.portao(rng.choice([(0, y) for y in range(1, 4)] + [(x, 0) for x in range(1, 4)]))
    else:
        p.portao(rng.choice([(10, y) for y in range(5, 10)] + [(x, 10) for x in range(5, 10)]))
    margem = p.livres()
    margem = [q for q in margem if q[0] <= 3 and q[1] <= 3]
    v, r = rng.sample(margem, 2)
    p.avatar("V", v)
    p.avatar("R", r)
    return Rascunho(p, sortear_simetria(rng))


# ---------------------------------------------------------------------------
# 8. Primeira névoa — porta de duas placas, seguradas por dois avatares
#    enquanto o terceiro passa. A Coruja enxerga longe e poupa exploração.
# ---------------------------------------------------------------------------


def primeira_nevoa(rng: random.Random) -> Rascunho:
    p = Planta(12, 12)
    c = rng.choice([4, 5])
    p.ret(1, 1, c - 1, 10, CHAO)
    for _ in range(rng.randint(2, 4)):
        q = (rng.randint(1, c - 1), rng.randint(2, 9))
        p.cel(q, PAREDE)
    yd = rng.randint(3, 8)
    p.porta("A", (c, yd), "ab")
    cima = [q for q in p.livres() if q[1] <= 3 and q[0] < c - 1]
    baixo = [q for q in p.livres() if q[1] >= 8 and q[0] < c - 1]
    p.placa("a", rng.choice(cima))
    p.placa("b", rng.choice(baixo))
    p.ret(c + 1, 1, 10, 10, CHAO)
    serpentina(p, c + 1, 1, 10, 10, rng, passo=rng.choice([2, 3]))
    lado = [q for q in p.livres() if q[0] > c]
    p.item(max(lado, key=lambda q: (abs(q[1] - yd), rng.random())), motor.DOURADA)
    p.portao(rng.choice(borda_de(p, [q for q in p.celulas(CHAO) if q[0] < c or q[0] > c])))
    meio = [q for q in p.livres() if q[0] < c and 4 <= q[1] <= 7]
    for a, q in zip("VRC", rng.sample(meio, 3)):
        p.avatar(a, q)
    return Rascunho(p, sortear_simetria(rng))


# ---------------------------------------------------------------------------
# 9. Fresta — uma placa fica numa sala que só se alcança por frestas: só a
#    Coruja chega e fica nela. Depois da porta, um bloco que só a Rocha tira.
# ---------------------------------------------------------------------------


def fresta(rng: random.Random) -> Rascunho:
    p = Planta(12, 12)
    c = rng.choice([5, 6])
    p.ret(1, 1, c - 1, 10, CHAO)
    # Sala da Coruja: um canto da área, fechado, com uma ou duas frestas.
    sx, sy = rng.randint(2, 3), rng.randint(2, 3)
    p.ret(1, 1, sx, sy, CHAO)
    p.ret(sx + 1, 1, sx + 1, sy + 1, PAREDE)
    p.ret(1, sy + 1, sx + 1, sy + 1, PAREDE)
    for _ in range(rng.randint(1, 2)):
        if rng.random() < 0.5:
            p.cel((sx + 1, rng.randint(1, sy)), FRESTA)
        else:
            p.cel((rng.randint(1, sx), sy + 1), FRESTA)
    p.placa("a", rng.choice([(x, y) for x in range(1, sx + 1) for y in range(1, sy + 1)]))
    fora = [q for q in p.livres() if q[0] > sx + 1 or q[1] > sy + 1]
    p.placa("b", rng.choice([q for q in fora if q[1] >= 7]))
    yd = rng.randint(max(sy + 2, 4), 9)
    p.porta("A", (c, yd), "ab")
    # Depois da porta: corredor com o bloco até a parede; a subida (xv) para
    # a área da chave fica antes do fim, e o bloco termina no nicho depois
    # dela. O portão nunca fica no corredor (o bloco o trancaria).
    p.ret(c + 1, yd, 10, yd, CHAO)
    p.bloco((c + 1, yd))
    xv = rng.randint(c + 2, 9)
    p.ret(c + 1, 1, 10, yd - 2, CHAO)
    serpentina(p, c + 1, 1, 10, yd - 2, rng)
    p.cel((xv, yd - 1), CHAO)
    if p[(xv, yd - 2)] == PAREDE:
        p.cel((xv, yd - 2), CHAO)
    area = [q for q in p.livres() if q[0] > c and q[1] < yd - 1]
    p.item(max(area, key=lambda q: (abs(q[0] - xv) + abs(q[1] - yd), rng.random())), motor.DOURADA)
    p.portao(rng.choice(borda_de(p, area + [q for q in p.celulas(CHAO) if q[0] < c and q[1] > sy + 1])))
    inicio = [q for q in p.livres() if q[0] < c and q[1] > sy + 1]
    for a, q in zip("VRC", rng.sample(inicio, 3)):
        p.avatar(a, q)
    return Rascunho(p, sortear_simetria(rng))


# ---------------------------------------------------------------------------
# 10. Três frentes — três alas, cada uma resolvida pela habilidade de um
#     avatar: o Vau termina numa placa (labirinto de água), a Coruja noutra
#     (labirinto de frestas) e a Rocha empurra um bloco para a terceira. Com
#     as três ativas, a porta central abre e a Rocha busca a chave.
# ---------------------------------------------------------------------------


def serpente(colunas: list[int], y0: int, y1: int, comeca_em_cima: bool) -> list[Pos]:
    """Caminho que percorre as colunas dadas, de ponta a ponta, ligando
    cada coluna à seguinte pela linha da ponta em que chegou."""
    caminho: list[Pos] = []
    em_cima = comeca_em_cima
    for i, x in enumerate(colunas):
        ys = range(y0, y1 + 1) if em_cima else range(y1, y0 - 1, -1)
        caminho.extend((x, y) for y in ys)
        if i + 1 < len(colunas):
            fim_y = y1 if em_cima else y0
            proximo = colunas[i + 1]
            passo = 1 if proximo > x else -1
            caminho.extend((xx, fim_y) for xx in range(x + passo, proximo, passo))
        em_cima = not em_cima
    return caminho


def transpor(caminho: list[Pos]) -> list[Pos]:
    return [(y, x) for x, y in caminho]


def tres_frentes(rng: random.Random) -> Rascunho:
    p = Planta(14, 14)
    p.ret(5, 5, 8, 8, CHAO)  # centro: todos começam aqui
    # Ala oeste (Vau): serpente de água nas colunas 3 e 1 (x 1..3, y 1..12).
    ye = rng.randint(5, 8)
    p.cel((4, ye), AGUA)
    cima = rng.random() < 0.5
    # Da entrada até a ponta da coluna 3, depois a coluna 1 inteira.
    trecho = [(3, y) for y in (range(ye, 0, -1) if cima else range(ye, 13))]
    resto = serpente([1], 1, 12, comeca_em_cima=cima)
    caminho_v = trecho + [(2, trecho[-1][1])] + resto
    if rng.random() < 0.5:
        caminho_v = caminho_v[: rng.randint(len(caminho_v) - 4, len(caminho_v))]
    for q in caminho_v:
        p.cel(q, AGUA)
    p.placa("a", caminho_v[-1])
    # Ala norte (Coruja): serpente de frestas nas linhas 3 e 1 (x 5..12).
    xe = rng.randint(5, 8)
    p.cel((xe, 4), FRESTA)
    direita = rng.random() < 0.5
    trecho = [(x, 3) for x in (range(xe, 13) if direita else range(xe, 4, -1))]
    resto = transpor(serpente([1], 5, 12, comeca_em_cima=not direita))
    caminho_c = trecho + [(trecho[-1][0], 2)] + resto
    if rng.random() < 0.5:
        caminho_c = caminho_c[: rng.randint(len(caminho_c) - 4, len(caminho_c))]
    for q in caminho_c:
        p.cel(q, FRESTA)
    p.placa("b", caminho_c[-1])
    # Ala leste (Rocha): sala x 10..12, y 5..12, com um bloco para a placa c.
    p.ret(10, 5, 12, 12, CHAO)
    p.cel((9, rng.randint(5, 8)), CHAO)
    for _ in range(100):
        placa, bloco = _bloco_e_placa(rng, p, 10, 6, 12, 12)
        if abs(placa[0] - bloco[0]) + abs(placa[1] - bloco[1]) >= 2 or rng.random() < 0.3:
            break
    p.placa("c", placa)
    p.bloco(bloco)
    # Sul: porta central de três placas e a sala da chave (x 5..8, y 10..12).
    xd = rng.randint(5, 8)
    p.porta("A", (xd, 9), "abc")
    p.ret(5, 10, 8, 12, CHAO)
    if rng.random() < 0.6:
        p.ret(5, 11, 8, 11, PAREDE)
        p.cel((rng.choice([5, 8]), 11), CHAO)
    sala = [q for q in p.livres() if q[1] >= 10 and 5 <= q[0] <= 8]
    p.item(max(sala, key=lambda q: (abs(q[0] - xd) + abs(q[1] - 9), rng.random())), motor.DOURADA)
    p.portao(rng.choice(borda_de(p, sala)))
    centro = [q for q in p.livres() if 5 <= q[0] <= 8 and 5 <= q[1] <= 8]
    for a, q in zip("VRC", rng.sample(centro, 3)):
        p.avatar(a, q)
    return Rascunho(p, sortear_simetria(rng), alas=(("V", "a"), ("C", "b"), ("R", "c")))


# ---------------------------------------------------------------------------
# 11. Relé — há exatamente os blocos necessários para as pontes: gastar um
#     na água errada deixa a fase sem solução. Uma porta de placa precisa ser
#     segurada na ida e na volta, e as chaves passam de mão em mão: a
#     vermelha do Vau para a Coruja, a Dourada da Coruja para quem atravessa.
# ---------------------------------------------------------------------------


def rele(rng: random.Random) -> Rascunho:
    """Tudo em corredores estreitos (o espaço de estados fica pequeno):

    - início (3 células) na linha yh; a porta P liga o início ao corredor H;
    - lago embaixo do início: um canal de água até a ilha da chave vermelha e
      um ramo até a ilha da placa de P (só o Vau segura P, e por isso nunca
      passa por ela);
    - no corredor H: P2 para o norte (a placa de P2 num nicho ao sul; quem
      segura é a Rocha, na ida e na volta da Coruja), frestas, o patamar, a
      porta vermelha e a sala da Chave Dourada;
    - no fim de H: o canal de duas células de água e o portão; um bloco no
      corredor e outro num nicho que dá no corredor; um poço (a água
      errada) ao lado do corredor.
    """
    p = Planta(14, 14)
    yh = rng.randint(5, 7)
    p.ret(1, yh, 3, yh, CHAO)  # início
    p.porta("A", (4, yh), "a")
    # Lago: desce da margem (x0, yh) até a ilha vermelha; ramo até a ilha da placa.
    x0 = rng.randint(1, 3)
    fundo = rng.randint(yh + 5, 12)
    p.ret(x0, yh + 1, x0, fundo - 1, AGUA)
    p.cel((x0, fundo), CHAO)
    p.item((x0, fundo), "vermelha")
    yr = rng.randint(yh + 2, fundo - 1)
    lado = 1 if x0 < 3 else -1
    comprimento = rng.randint(2, 4)
    xs = [x0 + lado * i for i in range(1, comprimento + 1)]
    xs = [x for x in xs if 1 <= x <= 5]
    for x in xs[:-1]:
        p.cel((x, yr), AGUA)
    p.placa("a", (xs[-1], yr))
    # Corredor H: de x 5 até 10; canal em 11 e 12; portão na borda (13).
    xc = 11
    p.ret(5, yh, xc - 1, yh, CHAO)
    p.cel((xc, yh), AGUA)
    p.cel((xc + 1, yh), AGUA)
    p.portao((xc + 2, yh))
    # P2 ao norte de H, com a placa b num nicho ao sul (não vizinho de P2).
    x2 = rng.choice([5, 6])
    xb = 11 - x2
    p.porta("B", (x2, yh - 1), "b")
    p.placa("b", (xb, yh + 1))
    # Frestas: sobem pela coluna de P2 até a linha 1 e andam para leste; o
    # patamar, a porta vermelha (ao sul) e a sala da chave descendo.
    p.ret(x2, 1, x2, yh - 2, FRESTA)
    xf = x2 + rng.randint(4, 6)
    p.ret(x2 + 1, 1, xf - 1, 1, FRESTA)
    p.cel((xf, 1), CHAO)
    p.trancada((xf, 2), "vermelha")
    fundo_sala = rng.randint(3, max(3, yh - 3))
    p.ret(xf, 3, xf, fundo_sala, CHAO)
    if rng.random() < 0.5 and xf + 1 <= 12:
        p.cel((xf + 1, fundo_sala), CHAO)
    sala = [q for q in p.livres() if q[0] >= xf and 3 <= q[1] <= fundo_sala]
    p.item(max(sala, key=lambda q: (abs(q[0] - xf) + abs(q[1] - 2), rng.random())), motor.DOURADA)
    # Blocos: A no corredor; B num nicho ao sul. A volta do nicho sai do
    # lado OESTE de B: depois de empurrá-lo para o corredor, a Rocha precisa
    # chegar a oeste dele para empurrá-lo até o canal.
    xn = rng.randint(xb + 2, 9)
    p.cel((xn, yh + 1), CHAO)
    p.bloco((xn, yh + 1))
    p.ret(xn - 1, yh + 2, xn, yh + 2, CHAO)
    p.cel((xn - 1, yh + 1), CHAO)
    xa = rng.choice([x for x in range(xb + 1, xc - 1) if x not in (xn - 1, xn)])
    p.bloco((xa, yh))
    # Poço (a água errada) ao sul do corredor e, do outro lado, um nicho
    # para o empurrão errado ser possível. O nicho do norte nunca encosta na
    # ala da chave (seria um atalho para ela).
    def nicho_livre(x: int) -> bool:
        q = (x, yh - 1)
        return p[q] == PAREDE and all(p[v] == PAREDE for v in vizinhos4(q) if v[1] != yh)

    candidatos = [
        x for x in range(xb + 1, xc) if nicho_livre(x) and p[(x, yh + 1)] == PAREDE and x not in (xn - 1, xn)
    ]
    if not candidatos:
        raise RuntimeError("sem lugar para o poço")
    xp = rng.choice(candidatos)
    poco = (xp, yh + 1)
    p.cel(poco, AGUA)
    p.cel((xp, yh - 1), CHAO)
    # O Vau começa em cima da entrada do lago (senão o corredor de início
    # pode encurralá-lo); a Rocha e a Coruja nas outras células.
    p.avatar("V", (x0, yh))
    outros = [(x, yh) for x in range(1, 4) if x != x0]
    for a, q in zip("RC", rng.sample(outros, 2)):
        p.avatar(a, q)
    return Rascunho(p, sortear_simetria(rng), armadilha=Armadilha(pontes_em=(poco,)))


# ---------------------------------------------------------------------------
# 12. Travessia — mapa grande com todas as mecânicas, cadeia de três chaves
#     (vermelha → azul → verde → Dourada) e dois ramos falsos (becos que só
#     gastam jogadas).
# ---------------------------------------------------------------------------


def travessia(rng: random.Random) -> Rascunho:
    """Em corredores (o espaço de estados precisa caber no solucionador):

    - início: sala 3×2 (x 1..3, linhas yS e yS+1); lago 1 subindo da sala
      até a ilha da chave vermelha; lago 2 descendo até a ilha da placa a;
    - porta vermelha (4, yS) → corredor B com o bloco A (empurrado até o
      nicho depois da junção xj), um recuo ao norte da entrada para dois
      avatares se cruzarem;
    - da junção, subida até a linha 3, curva a leste até a chave azul; porta
      azul ao norte da curva; frestas pela linha 1 até a sala da chave verde;
    - porta verde ao sul do corredor B → corredor D descendo e virando a
      leste; no caminho, um nicho de fresta com a placa b; a porta de placa P
      (placas a e b); depois dela, o bloco C, uma célula de água e a sala da
      Chave Dourada com o portão;
    - dois becos: um ao sul do corredor B, outro ao sul do corredor D.
    """
    p = Planta(16, 16)
    yS = rng.randint(6, 8)
    p.ret(1, yS, 3, yS + 1, CHAO)
    # Lago 1: sobe da sala até a ilha da chave vermelha.
    x0 = rng.randint(1, 3)
    p.ret(x0, 2, x0, yS - 1, AGUA)
    p.cel((x0, 1), CHAO)
    p.item((x0, 1), "vermelha")
    if rng.random() < 0.5:
        # Uma curva no lago: a ilha fica de lado.
        lado = 1 if x0 < 3 else -1
        p.cel((x0, 1), AGUA)
        p.cel((x0 + lado, 1), CHAO)
        p.itens.pop((x0, 1))
        p.item((x0 + lado, 1), "vermelha")
    # Lago 2: desce da sala até a ilha da placa a.
    x1 = rng.randint(1, 3)
    ya = rng.randint(yS + 5, 14)
    p.ret(x1, yS + 2, x1, ya - 1, AGUA)
    p.placa("a", (x1, ya))
    # Porta vermelha e corredor B.
    p.trancada((4, yS), "vermelha")
    xj = rng.randint(8, 10)
    p.ret(5, yS, xj + 1, yS, CHAO)
    p.cel((5, yS - 1), CHAO)  # recuo para cruzar
    xa = rng.randint(6, xj - 1)
    p.bloco((xa, yS))
    # Subida da junção, curva a leste até a chave azul, porta azul ao norte.
    p.ret(xj, 3, xj, yS - 1, CHAO)
    p.ret(xj + 1, 3, xj + 2, 3, CHAO)
    p.item((xj + 2, 3), "azul")
    p.trancada((xj, 2), "azul")
    # Frestas pela linha 1 até a sala da chave verde.
    xg = xj + rng.randint(3, 4)
    p.cel((xj, 1), FRESTA)
    p.ret(xj + 1, 1, xg - 1, 1, FRESTA)
    p.ret(xg, 1, min(14, xg + 1), 1, CHAO)
    p.item((min(14, xg + 1), 1), "verde")
    # Porta verde ao sul do corredor B e o corredor D.
    xd = rng.choice([x for x in (6, 7) if x != xa and x < xj])
    p.trancada((xd, yS + 1), "verde")
    yD = rng.randint(yS + 5, min(13, yS + 7))
    p.ret(xd, yS + 2, xd, yD, CHAO)
    xp = rng.randint(xd + 3, 10)
    p.ret(xd, yD, xp - 1, yD, CHAO)
    # Nicho de fresta com a placa b, ao norte do trecho horizontal de D.
    xb = rng.randint(xd + 1, xp - 1)
    p.cel((xb, yD - 1), FRESTA)
    p.placa("b", (xb, yD - 2))
    if p[(xb, yD - 3)] != PAREDE:
        raise RuntimeError("nicho da placa b encosta em outra coisa")
    # Porta P, bloco C, água e a sala da Chave Dourada.
    p.porta("A", (xp, yD), "ab")
    p.ret(xp + 1, yD, xp + 2, yD, CHAO)
    p.bloco((xp + 2, yD))
    # A armadilha: um laço em cima do bloco e um bolso embaixo. Empurrado
    # para o bolso, o bloco nunca mais sai, e a água fica sem ponte.
    p.ret(xp + 1, yD - 1, xp + 2, yD - 1, CHAO)
    bolso = (xp + 2, yD + 1)
    p.cel(bolso, CHAO)
    p.cel((xp + 3, yD), AGUA)
    p.ret(xp + 4, yD - 1, min(14, xp + 5), yD + 1, CHAO)
    sala = [q for q in p.livres() if q[0] >= xp + 4 and abs(q[1] - yD) <= 1]
    p.item(max(sala, key=lambda q: (q[0], rng.random())), motor.DOURADA)
    p.portao(rng.choice(borda_de(p, sala) or [(15, yD)]))
    # Becos: um ao sul do corredor B, outro ao sul do corredor D.
    becos = [x for x in range(6, xj + 2) if abs(x - xd) > 1 and p[(x, yS + 1)] == PAREDE]
    if becos:
        xbeco = rng.choice(becos)
        p.ret(xbeco, yS + 1, xbeco, yS + rng.randint(2, 3), CHAO)
    becos = [x for x in range(xd + 1, xp) if x != xb and p[(x, yD + 1)] == PAREDE]
    if becos:
        xbeco = rng.choice(becos)
        p.ret(xbeco, yD + 1, xbeco, min(14, yD + rng.randint(1, 2)), CHAO)
    # Quem começa onde: o Vau perto dos dois lagos.
    sala_inicio = [(x, y) for x in range(1, 4) for y in (yS, yS + 1)]
    p.avatar("V", (x0, yS))
    outros = [q for q in sala_inicio if q != p.avatares["V"]]
    for a, q in zip("RC", rng.sample(outros, 2)):
        p.avatar(a, q)
    return Rascunho(p, sortear_simetria(rng), armadilha=Armadilha(blocos_em=(bolso,)))


MOLDES = {
    "lago": lago,
    "empurrao": empurrao,
    "ponte": ponte,
    "segura_a_porta": segura_a_porta,
    "mao_em_mao": mao_em_mao,
    "duas_placas": duas_placas,
    "corrente": corrente,
    "primeira_nevoa": primeira_nevoa,
    "fresta": fresta,
    "tres_frentes": tres_frentes,
    "rele": rele,
    "travessia": travessia,
}
