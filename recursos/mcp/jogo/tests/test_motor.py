"""As regras do jogo, uma por uma."""

import pytest

import desenho
import motor
import texto
from conftest import EXEMPLO_FASE_4
from motor import interpretar


def jogar(fase, passos, estado=None, conhecidas=None):
    e = estado or motor.estado_inicial(fase)
    return motor.executar_plano(fase, e, [interpretar(p) for p in passos], conhecidas)


def ok(fase, passos, estado=None):
    e, rs = jogar(fase, passos, estado)
    assert all(r.ok for r in rs), [r.mensagem for r in rs]
    return e


def invalido(fase, passos, trecho, estado=None):
    e, rs = jogar(fase, passos, estado)
    assert rs[-1].ok is False, [r.mensagem for r in rs]
    assert trecho in rs[-1].mensagem, rs[-1].mensagem
    # Nada muda num passo inválido.
    anterior = rs[-2].estado if len(rs) > 1 else (estado or motor.estado_inicial(fase))
    assert rs[-1].estado == anterior
    return rs[-1]


def test_exemplo_da_especificacao_desenhado_igual(exemplo):
    e = motor.estado_inicial(exemplo)
    s = texto.Status(fase=4, jogadas=0, invalidas=0, reinicios=0, versao=0)
    esperado = "\n".join(
        [
            "FASE 4 · jogadas 0 · inválidas 0 · reinícios 0 · versão 0",
            "    0 1 2 3 4 5 6 7 8",
            *[f" {y}  {linha}" for y, linha in enumerate(EXEMPLO_FASE_4)],
            "",
            "Objetos:",
            "- Vau (V) em (1,1), mãos vazias",
            "- Rocha (R) em (1,3), mãos vazias",
            "- Placa a em (4,1): inativa",
            "- Porta A em (5,2): fechada; abre com as placas: a",
            "- Chave Dourada (K) em (7,1)",
            "- Portão (G) em (6,4)",
        ]
    )
    assert texto.visao(s, exemplo, e) == esperado
    assert "Segura" not in texto.visao(s, exemplo, e)


def test_exemplo_resolvido_em_15(exemplo):
    passos = ["V L"] * 3 + ["R L", "R L", "R L", "R N", "R L", "R L", "R N", "R L", "R S", "R S", "R O", "R S"]
    assert len(passos) == 15
    e, rs = jogar(exemplo, passos)
    assert all(r.ok for r in rs) and e.concluida
    assert "porta" in [m["tipo"] for m in rs[2].mudancas]  # o Vau pisou na placa: a porta abriu
    assert "pegou a Chave Dourada" in rs[10].mensagem
    assert "fase concluída" in rs[-1].mensagem


def test_porta_de_placa_nao_fecha_com_alguem_dentro(exemplo):
    # Vau na placa, Rocha entra na porta; o Vau sai da placa: a porta segura.
    e = ok(exemplo, ["V L"] * 3 + ["R L", "R L", "R L", "R N", "R L"])
    assert e.pos("R") == (5, 2) and "A" in e.portas_abertas
    e = ok(exemplo, ["V O"], e)
    assert "A" in e.portas_abertas  # alguém dentro: não fecha
    e2 = ok(exemplo, ["R L"], e)
    assert "A" not in e2.portas_abertas  # ficou vazia e a condição é falsa: fecha
    invalido(exemplo, ["R O"], "está fechada", e2)
    # Saindo para trás também fecha.
    e3 = ok(exemplo, ["R O"], e)
    assert "A" not in e3.portas_abertas


def test_agua_so_o_vau_e_bloco_vira_ponte():
    f = desenho.fase(
        [
            "#######",
            "#V~~.K#",
            "#RB~.G#",
            "#######",
        ]
    )
    e = ok(f, ["V L", "V L"])
    assert e.pos("V") == (3, 1)
    invalido(f, ["R N", "R L"], "não entra na água", e)
    # Empurrar o bloco para a água: afunda e vira ponte permanente.
    e2, rs = jogar(f, ["R L"])
    assert rs[0].ok and "afundou" in rs[0].mensagem
    assert (3, 2) in e2.pontes and not e2.blocos
    assert motor.tipo_atual(f, e2, (3, 2)) == motor.PONTE
    e3 = ok(f, ["R L", "R L"], e2)
    assert e3.pos("R") == (4, 2)


def test_rocha_empurra_mas_nao_puxa_e_destinos_do_bloco():
    f = desenho.fase(
        [
            "#########",
            "#RB.B.:.#",
            "#.......#",
            "#.k.D._G#",
            "#########",
        ],
        cores_chaves=["azul"],
        extras={"itens": {(4, 2): motor.DOURADA}},
    )
    invalido(f, ["R L", "R L"], "há outro bloco")
    e = ok(f, ["R L"])
    assert e.pos("R") == (2, 1) and (3, 1) in e.blocos
    # Não existe ação de puxar: voltar só anda.
    e = ok(f, ["R O"], e)
    assert (3, 1) in e.blocos and e.pos("R") == (1, 1)
    # Bloco contra fresta.
    f2 = desenho.fase(["#######", "#RB:..#", "#K...G#", "#######"])
    invalido(f2, ["R L"], "blocos não passam por frestas")
    # Bloco contra porta (mesmo aberta) e contra item.
    f3 = desenho.fase(["########", "#RBD._.#", "#.Bk..G#", "#K.....#", "########"], cores_chaves=["azul"])
    invalido(f3, ["R L"], "blocos não entram em portas")
    invalido(f3, ["R S", "R L"], "há um item")
    # Só a Rocha empurra.
    f4 = desenho.fase(["#######", "#VB...#", "#K...G#", "#######"])
    invalido(f4, ["V L"], "só a Rocha")


def test_fresta_so_a_coruja():
    f = desenho.fase(["#######", "#C:.K.#", "#R:..G#", "#######"])
    e = ok(f, ["C L", "C L"])
    assert e.pos("C") == (3, 1)
    invalido(f, ["R L"], "não passa por frestas")


def test_pegar_soltar_e_entregar():
    f = desenho.fase(
        [
            "#########",
            "#VK~~kR.#",
            "#..~~...#",
            "#.....G.#",
            "#########",
        ],
        cores_chaves=["azul"],
    )
    e, rs = jogar(f, ["V L"])
    assert e.carga("V") == motor.DOURADA and "pegou a Chave Dourada" in rs[0].mensagem
    # Mãos ocupadas: o item fica onde está.
    e, rs = jogar(f, ["V L", "V L", "V L"], e)
    assert rs[-1].ok and "ficou no chão (mãos ocupadas)" in rs[-1].mensagem
    assert e.pos("V") == (5, 1) and e.item_em((5, 1)) == "azul"
    # Soltar na água não pode.
    e = ok(f, ["V O"], e)
    invalido(f, ["V soltar"], "só solta itens em chão", e)
    # Entregar só para vizinho; através da água vale.
    invalido(f, ["V entregar R"], "não está ao lado", e)
    e = ok(f, ["R S", "R O", "V S"], e)
    assert e.pos("V") == (4, 2) and e.pos("R") == (5, 2)
    e, rs = jogar(f, ["V entregar R"], e)
    assert rs[0].ok and e.carga("R") == motor.DOURADA and e.carga("V") is None
    invalido(f, ["V entregar R"], "mãos vazias", e)
    invalido(f, ["R entregar R"], "para si mesmo", e)
    # Soltar em chão livre; em cima de outro item, não.
    e = ok(f, ["R soltar"], e)
    assert e.item_em((5, 2)) == motor.DOURADA and e.carga("R") is None
    e = ok(f, ["R N"], e)
    assert e.carga("R") == "azul"
    e = ok(f, ["R S"], e)
    invalido(f, ["R soltar"], "já há um item", e)
    # Quem recebe precisa estar de mãos vazias.
    e = ok(f, ["R L", "V L"], e)
    assert e.pos("V") == (5, 2) and e.carga("V") == motor.DOURADA and e.carga("R") == "azul"
    invalido(f, ["V entregar R"], "mãos ocupadas", e)


def test_porta_trancada_e_portao():
    f = desenho.fase(
        [
            "########",
            "#Vk+.KG#",
            "########",
        ],
        cores_chaves=["vermelha"],
        cores_trancadas=["vermelha"],
    )
    e, rs = jogar(f, ["V L", "V L"])
    assert all(r.ok for r in rs)
    assert "porta trancada abriu" in rs[1].mensagem and e.carga("V") is None
    assert motor.tipo_atual(f, e, (3, 1)) == motor.CHAO
    e, rs = jogar(f, ["V L", "V L", "V L"], e)
    assert e.concluida and "fase concluída" in rs[-1].mensagem
    # Sem a chave, porta trancada é parede; e o portão também.
    f2 = desenho.fase(["#######", "#Vk+KG#", "#######"], cores_chaves=["azul"], cores_trancadas=["vermelha"])
    invalido(f2, ["V L", "V L"], "só abre com a chave vermelha")
    f3 = desenho.fase(["######", "#V.G.#", "#K...#", "######"])
    invalido(f3, ["V L", "V L"], "só abre para quem traz a Chave Dourada")


def test_placa_ativa_com_bloco_e_nao_com_item():
    f = desenho.fase(
        [
            "########",
            "#RB_D.K#",
            "#...#..#",
            "#k..#.G#",
            "########",
        ],
        cores_chaves=["azul"],
    )
    e = ok(f, ["R L"])
    assert "A" in e.portas_abertas and motor.placa_ativa(f, e, "a")
    # Item em cima da placa não ativa.
    f2 = desenho.fase(["#######", "#Vk_D.#", "#K...G#", "#######"], cores_chaves=["azul"])
    e = ok(f2, ["V L", "V L", "V soltar", "V O"])
    assert e.item_em((3, 1)) == "azul" and "A" not in e.portas_abertas


def test_dois_avatares_nunca_na_mesma_celula(exemplo):
    e = ok(exemplo, ["V S"])
    invalido(exemplo, ["R N"], "Vau está lá", e)


def test_notacao():
    assert interpretar("R L") == motor.Passo("R", "mover", direcao="L")
    assert interpretar(" r  n ") == motor.Passo("R", "mover", direcao="N")
    assert interpretar("V soltar") == motor.Passo("V", "soltar")
    assert interpretar("V entregar r") == motor.Passo("V", "entregar", alvo="R")
    assert interpretar("C entregar V").notacao() == "C entregar V"
    for ruim in ["", "RL", "X L", "R norte", "V entregar", "V entregar X", "V soltar agora", 7]:
        with pytest.raises(motor.ErroNotacao):
            interpretar(ruim)


def test_avatar_fora_da_fase_e_passo_depois_do_fim(exemplo):
    invalido(exemplo, ["C L"], "não joga esta fase")


def test_nevoa_raio_do_rei_e_simulacao_para_no_desconhecido():
    linhas = ["." * 12 for _ in range(12)]
    linhas[0] = "#" * 12
    linhas[11] = "#" * 12
    linhas = ["#" + l[1:-1] + "#" for l in linhas]
    linhas[1] = "#V" + linhas[1][2:]
    linhas[10] = linhas[10][:9] + "KG#"
    linhas[5] = "#C" + linhas[5][2:]
    f = desenho.fase(linhas, nevoa=True)
    e = motor.estado_inicial(f)
    vistas = motor.visiveis(f, e)
    # Vau em (1,1) com raio 2; Coruja em (1,5) com raio 5.
    assert (3, 3) in vistas and (6, 10) in vistas and (7, 1) not in vistas
    assert (6, 0) in vistas  # pelo raio da Coruja: |6-1| = 5
    # Simular não entra no desconhecido.
    e2, rs = motor.executar_plano(f, e, [interpretar("V L")] * 6, frozenset(vistas))
    assert rs[-1].desconhecido and "desconhecido" in rs[-1].mensagem
    assert e2.pos("V") == (6, 1)
    # O desenho e a lista escondem o que está no desconhecido.
    s = texto.Status(fase=8, jogadas=0, invalidas=0, reinicios=0, versao=0)
    v = texto.visao(s, f, e, vistas)
    assert "?" in v and "Chave Dourada" not in v and "Portão" not in v


def test_fase_invalida_e_recusada():
    with pytest.raises(motor.ErroFase):
        desenho.fase(["#####", "#V.K#", "#####"])  # sem portão
    with pytest.raises(motor.ErroFase):
        desenho.fase(["#####", "#VGK#", "#.K.#", "#####"])  # duas Chaves Douradas
    with pytest.raises(motor.ErroFase):
        desenho.fase(["#####", "#RKG#", "#####"], extras={"avatares": {"R": (0, 0)}})  # Rocha na parede


def test_janela_de_avatar(exemplo):
    e = motor.estado_inicial(exemplo)
    s = texto.Status(fase=4, jogadas=0, invalidas=0, reinicios=0, versao=0)
    v = texto.visao_de_avatar(s, exemplo, e, "R")
    assert "Janela em volta de Rocha (R), de (0,0) a (5,4)" in v
    assert "Rocha está em (1,3) e mãos vazias." in v
    assert "Chave Dourada" not in v  # (7,1) fica fora da janela
