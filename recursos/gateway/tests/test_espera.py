from espera import Espera


def test_cresce_ate_o_teto_e_zera():
    e = Espera(base=1, teto=10, fator=2, sorteio=0, aleatorio=lambda: 0.5)
    assert [e.proxima() for _ in range(6)] == [1, 2, 4, 8, 10, 10]
    e.zerar()
    assert e.proxima() == 1


def test_sorteio_fica_na_faixa_e_nunca_passa_do_teto():
    for sorte in (0.0, 0.25, 0.75, 0.999):
        e = Espera(base=4, teto=5, fator=2, sorteio=0.2, aleatorio=lambda s=sorte: s)
        primeira = e.proxima()
        assert 4 * 0.8 <= primeira <= 4 * 1.2
        assert all(e.proxima() <= 5 for _ in range(5))
