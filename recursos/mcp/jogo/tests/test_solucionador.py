"""O solucionador: ótimo certo, e as mesmas regras do motor, passo a passo."""

import random

import pytest

import desenho
import motor
import solucionador
from motor import interpretar

# Fases pequenas que juntas usam todas as mecânicas.
MECANICAS = {
    "agua_e_pontes": desenho.fase(
        [
            "#########",
            "#V.B.~~K#",
            "#R.B.~~.#",
            "#.......#",
            "#.....G.#",
            "#########",
        ]
    ),
    "frestas_e_placas": desenho.fase(
        [
            "##########",
            "#C:_#..K.#",
            "#...D....#",
            "#VR.#._.G#",
            "#..B#....#",
            "##########",
        ],
        ligacoes={"A": "a"},
    ),
    "chaves_e_entregas": desenho.fase(
        [
            "##########",
            "#Vk~~+.K.#",
            "#.~~~#...#",
            "#R..k+.G.#",
            "#.C..#...#",
            "##########",
        ],
        cores_chaves=["vermelha", "azul"],
        cores_trancadas=["azul", "vermelha"],
    ),
    "duas_placas": desenho.fase(
        [
            "##########",
            "#V~~_#.K.#",
            "#.~~.D...#",
            "#RB._#.G.#",
            "#....#...#",
            "##########",
        ],
        ligacoes={"A": "ab"},
    ),
}


def estados_do_motor(fase, e):
    """Todos os sucessores de `e` pelo motor (todas as ações possíveis)."""
    saida = []
    for a in fase.ativos():
        passos = [motor.Passo(a, "mover", direcao=d) for d in "NSLO"] + [motor.Passo(a, "soltar")]
        passos += [motor.Passo(a, "entregar", alvo=b) for b in fase.ativos() if b != a]
        for passo in passos:
            r = motor.aplicar(fase, e, passo)
            if r.ok:
                saida.append((r.estado, passo.notacao()))
    return saida


@pytest.mark.parametrize("nome", sorted(MECANICAS))
def test_motor_e_solucionador_concordam(nome):
    fase = MECANICAS[nome]
    problema = solucionador.Problema(fase)
    sorteio = random.Random(nome)
    for _ in range(60):
        e = motor.estado_inicial(fase)
        for _ in range(sorteio.randrange(0, 40)):
            if e.concluida:
                break
            pelo_motor = estados_do_motor(fase, e)
            pelo_solucionador = list(problema.sucessores(problema.de_estado(e)))
            esperado = {
                (None if m.concluida else problema.de_estado(m), passo) for m, passo in pelo_motor
            }
            obtido = {(s, problema.notacao(acao)) for s, acao in pelo_solucionador}
            if any(m.concluida for m, _ in pelo_motor):
                # O solucionador para no primeiro objetivo: basta conter o fim.
                assert any(s is None for s, _ in obtido)
                break
            assert obtido == esperado
            e = sorteio.choice(pelo_motor)[0]


def test_exemplo_tem_minimo_15(exemplo):
    sol = solucionador.resolver(exemplo)
    assert sol.otimo == 15
    e, rs = motor.executar_plano(exemplo, motor.estado_inicial(exemplo), [interpretar(p) for p in sol.passos])
    assert all(r.ok for r in rs) and e.concluida and len(rs) == 15


def test_solucoes_das_mecanicas_executam_no_motor():
    for nome, fase in MECANICAS.items():
        sol = solucionador.resolver(fase)
        e, rs = motor.executar_plano(fase, motor.estado_inicial(fase), [interpretar(p) for p in sol.passos])
        assert e.concluida and all(r.ok for r in rs), nome
        # A* e busca em largura pura dão o mesmo ótimo.
        problema = solucionador.Problema(fase)
        problema.h = lambda s: 0
        assert solucionador.buscar(problema).otimo == sol.otimo, nome


def test_variantes(exemplo):
    assert not solucionador.tem_solucao(exemplo, sem_avatares=("V",))
    assert not solucionador.tem_solucao(exemplo, sem_avatares=("R",))
    entrega = desenho.fase(
        [
            "#########",
            "#R.~~~.K#",
            "#V.~~~..#",
            "#...G...#",
            "#########",
        ],
    )
    com = solucionador.resolver(entrega)
    sem = solucionador.resolver(entrega, sem_entregar=True)
    assert sem.otimo >= com.otimo
    # Sem troca de chave: a chave fica com quem a pegou primeiro.
    troca = desenho.fase(
        [
            "########",
            "#V~K~#R#",
            "#~~~~#.#",
            "#~~~~:.#",
            "#.....G#",
            "########",
        ],
        extras={"avatares": {"C": (4, 4)}},
    )
    assert solucionador.tem_solucao(troca)
    sol = solucionador.resolver(troca, sem_troca=True)
    assert all("entregar" not in p for p in sol.passos)


def test_objetivo_de_placas():
    fase = MECANICAS["duas_placas"]
    sol = solucionador.resolver(fase, sem_avatares=("R",), objetivo_placas=("a",))
    e, rs = motor.executar_plano(fase, motor.estado_inicial(fase), [interpretar(p) for p in sol.passos])
    assert motor.placa_ativa(fase, e, "a")


def test_orcamento_esgotado():
    linhas = ["#" * 16] + ["#" + "." * 14 + "#" for _ in range(14)] + ["#" * 16]
    linhas[1] = "#V" + "." * 13 + "#"
    linhas[2] = "#R" + "." * 13 + "#"
    linhas[3] = "#C" + "." * 13 + "#"
    linhas[14] = "#" + "." * 12 + "KG#"
    fase = desenho.fase(linhas)
    with pytest.raises(solucionador.OrcamentoEsgotado):
        problema = solucionador.Problema(fase)
        problema.h = lambda s: 0
        solucionador.buscar(problema, solucionador.Orcamento(max_estados=5000))


def test_sem_solucao_a_partir_de_um_estado():
    # Bloco empurrado para o canto: a fase fica sem solução.
    fase = desenho.fase(
        [
            "#######",
            "#.....#",
            "#.RB..#",
            "#.....#",
            "#~~~~~#",
            "#..K..#",
            "##G####",
        ]
    )
    assert solucionador.tem_solucao(fase)
    armadilha = ["R L", "R L", "R S", "R L", "R N"]
    e, rs = motor.executar_plano(fase, motor.estado_inicial(fase), [interpretar(p) for p in armadilha])
    assert all(r.ok for r in rs) and e.blocos == {(5, 1)}
    assert not solucionador.tem_solucao_a_partir(fase, e)
