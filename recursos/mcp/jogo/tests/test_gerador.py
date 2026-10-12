"""O gerador: determinismo, catálogo × gabarito, testes de aceite e o ótimo."""

import os

import pytest

import catalogo
import gerador
import motor
import solucionador

LENTOS = os.environ.get("JOGO_TESTES_LENTOS") == "1"
SEED = "teste-hash"
RAPIDAS = list(range(1, 11))

# sha256 do arquivo de cada fase gerada com a seed SEED. Mudou o gerador (ou
# os moldes)? Suba VERSAO_GERADOR e atualize esta tabela: o catálogo antigo
# não pode ser confundido com o novo.
HASHES = {
    1: "8dd537367abd77527fc93a672f4c0efbbff4614dbf2701b752d9407cd9a93ca3",
    2: "dfb9d77bd5b646ed311e0d238a06793d995a58626e1c5ee6dfbce54cac4f9219",
    3: "c9604d2661e1c8bfb94cab5b29cd9a1f35cc586e1e7f247ac346e7c70e82c366",
    4: "09ab2a3afb93bcdf8bce1205bf90bf30e0efd11b3dcdd02d55db10cb4ae58282",
    5: "397d4d1ff90abcd2c8137844f636ec81e93c078ac3e5a8697755598966385951",
    6: "9ac53fb9c59ce03c5bd49b2a2fc5bcb55867f5cbba9dc4ef8abda1678963c922",
    7: "447f0f299fdadc6e83bb77c0aa64dc13beff1cf55a60bb16d86b49d431dcb0ec",
    8: "23b3ea54f3167e210c4b55cb8ed5c7b53208738e5a9d6f7623d040033ad26d1e",
    9: "607c5cd0abdeb4cae11a11ad6d3f3757b19776cdcdf04e09fd500680d2233cbc",
    10: "84bebf7320374acd7ff143d81a2ed0cbc484c51bc93c5900d62e8c2c83ccbdae",
}


def test_seed_da_fase_e_deterministica():
    a = gerador.seed_da_fase("x", 3, 0)
    assert a == gerador.seed_da_fase("x", 3, 0)
    assert len({a, gerador.seed_da_fase("x", 3, 1), gerador.seed_da_fase("x", 4, 0), gerador.seed_da_fase("y", 3, 0)}) == 4


def test_mesma_seed_mesmos_bytes(tmp_path):
    h1 = gerador.gerar_campanha(SEED, tmp_path / "c1", tmp_path / "g1", [2, 3, 5])
    h2 = gerador.gerar_campanha(SEED, tmp_path / "c2", tmp_path / "g2", [2, 3, 5])
    assert h1 == h2
    for n in (2, 3, 5):
        nome = catalogo.nome_arquivo(n)
        assert (tmp_path / "g1" / nome).read_bytes() == (tmp_path / "g2" / nome).read_bytes()


def test_hashes_registrados(tmp_path):
    hashes = gerador.gerar_campanha(SEED, tmp_path / "c", tmp_path / "g", RAPIDAS)
    assert hashes == HASHES, f"o gerador mudou: suba VERSAO_GERADOR e registre {hashes}"


def test_catalogo_e_gabarito_separados(tmp_path):
    gerador.gerar_campanha(SEED, tmp_path / "c", tmp_path / "g", [3, 4, 5])
    for n in (3, 4, 5):
        fase_bruta = (tmp_path / "c" / catalogo.nome_arquivo(n)).read_text()
        for segredo in ('"otimo"', '"solucao"', '"armadilha"'):
            assert segredo not in fase_bruta
        dados = catalogo.ler(tmp_path / "c" / catalogo.nome_arquivo(n))
        assert dados["molde"] == gerador.ESPECS[n].molde and dados["gerador"] == gerador.VERSAO_GERADOR
        gab = catalogo.ler(tmp_path / "g" / catalogo.nome_arquivo(n))
        lo, hi = gerador.ESPECS[n].faixa
        assert lo <= gab["otimo"] <= hi and gab["seed"] == dados["seed"]
        fase = catalogo.fase_de_dict(dados)
        e, rs = motor.executar_plano(fase, motor.estado_inicial(fase), [motor.interpretar(p) for p in gab["solucao"]])
        assert e.concluida and len(rs) == gab["otimo"]
    gab3 = catalogo.ler(tmp_path / "g" / catalogo.nome_arquivo(3))
    fase3 = catalogo.carregar_fase(tmp_path / "c" / catalogo.nome_arquivo(3))
    e, rs = motor.executar_plano(fase3, motor.estado_inicial(fase3), [motor.interpretar(p) for p in gab3["armadilha"]])
    assert all(r.ok for r in rs)
    assert not solucionador.tem_solucao_a_partir(fase3, e)
    gab5 = catalogo.ler(tmp_path / "g" / catalogo.nome_arquivo(5))
    assert gab5["testes"]["otimo_sem_entregar"] is None or gab5["testes"]["otimo_sem_entregar"] > gab5["otimo"]


@pytest.mark.parametrize("numero", [1, 2, 3, 4, 5])
def test_a_estrela_da_o_mesmo_otimo_que_a_largura(numero):
    """A heurística é admissível: nas fases pequenas, A* e busca em largura
    pura (sem heurística) dão o mesmo mínimo."""
    for i in range(3):
        r = gerador.gerar_fase(f"admissivel-{i}", numero)
        fase = catalogo.fase_de_dict(r.fase)
        largura = solucionador.Problema(fase)
        largura.h = lambda s: 0
        assert solucionador.buscar(largura).otimo == r.gabarito["otimo"]


def test_cli_recusa_catalogo_e_gabarito_na_mesma_pasta(tmp_path, capsys):
    assert gerador.main(["--seed", "x", "--catalogo", str(tmp_path), "--gabarito", str(tmp_path), "--fases", "1"]) == 2
    assert gerador.main(["--seed", "x", "--catalogo", str(tmp_path / "c"), "--gabarito", str(tmp_path / "g"), "--fases", "1,2"]) == 0
    saida = capsys.readouterr().out.splitlines()
    assert [l.split()[0] for l in saida] == ["fase-01.json", "fase-02.json"]


@pytest.mark.skipif(not LENTOS, reason="lento (minutos): JOGO_TESTES_LENTOS=1")
def test_campanha_inteira(tmp_path):
    hashes = gerador.gerar_campanha(SEED, tmp_path / "c", tmp_path / "g")
    assert sorted(hashes) == list(range(1, 13))
    for n in range(1, 13):
        gab = catalogo.ler(tmp_path / "g" / catalogo.nome_arquivo(n))
        lo, hi = gerador.ESPECS[n].faixa
        assert lo <= gab["otimo"] <= hi
        if gerador.ESPECS[n].armadilha:
            assert gab["armadilha"]
