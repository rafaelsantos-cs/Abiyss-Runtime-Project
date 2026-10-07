"""Cache SQLite: prazo, tamanho máximo e arquivo corrompido."""

from cache import Cache


class Relogio:
    def __init__(self):
        self.agora = 1_000_000.0

    def __call__(self):
        return self.agora


def test_guarda_e_vence(tmp_path):
    relogio = Relogio()
    c = Cache(tmp_path / "c.sqlite3", 10_000, relogio)
    c.gravar("pagina", "https://a", {"texto": "olá"}, ttl_segundos=60, buscado_em=999_000.0)
    assert c.ler("pagina", "https://a") == ({"texto": "olá"}, 999_000.0)
    assert c.ler("busca", "https://a") is None, "tipo faz parte da chave"
    relogio.agora += 61
    assert c.ler("pagina", "https://a") is None
    assert c.tamanho_total() == 0, "a vencida sai ao ser lida"


def test_ttl_zero_nao_guarda(tmp_path):
    c = Cache(tmp_path / "c.sqlite3", 10_000)
    c.gravar("busca", "x", [1, 2], ttl_segundos=0)
    assert c.ler("busca", "x") is None


def test_tamanho_maximo_tira_as_menos_usadas(tmp_path):
    relogio = Relogio()
    c = Cache(tmp_path / "c.sqlite3", 1000, relogio)
    for i in range(5):
        relogio.agora += 1
        c.gravar("pagina", f"p{i}", "x" * 150, ttl_segundos=3600)
    relogio.agora += 1
    assert c.ler("pagina", "p0") is not None  # p0 passa a ser a usada mais recentemente
    for i in range(5, 8):
        relogio.agora += 1
        c.gravar("pagina", f"p{i}", "x" * 150, ttl_segundos=3600)
    assert c.tamanho_total() <= 1000
    assert c.ler("pagina", "p0") is not None
    assert c.ler("pagina", "p1") is None and c.ler("pagina", "p2") is None
    assert c.ler("pagina", "p7") is not None


def test_valor_maior_que_o_cache_nao_entra(tmp_path):
    c = Cache(tmp_path / "c.sqlite3", 100)
    c.gravar("pagina", "grande", "x" * 500, ttl_segundos=60)
    assert c.ler("pagina", "grande") is None and c.tamanho_total() == 0


def test_persiste_entre_aberturas(tmp_path):
    Cache(tmp_path / "c.sqlite3", 10_000).gravar("robots", "https://a", {"status": 200}, 60)
    assert Cache(tmp_path / "c.sqlite3", 10_000).ler("robots", "https://a")[0] == {"status": 200}


def test_arquivo_corrompido_e_recriado(tmp_path):
    arquivo = tmp_path / "c.sqlite3"
    arquivo.write_bytes(b"isto nao e um banco sqlite" * 100)
    c = Cache(arquivo, 10_000)
    c.gravar("busca", "x", [1], 60)
    assert c.ler("busca", "x")[0] == [1]
    assert (tmp_path / "c.sqlite3.corrompido").exists()
