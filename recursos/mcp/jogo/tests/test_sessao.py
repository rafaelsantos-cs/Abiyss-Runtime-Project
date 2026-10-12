"""A execução: contadores, versão, limites, relógio, modos, replay e métricas."""

import json

import pytest

import catalogo
import desenho
from conftest import EXEMPLO_FASE_4
from sessao import ConfigExecucao, ErroExecucao, ErroJogo, Sessao, carregar_execucao

SOLUCAO_EXEMPLO = ["V L"] * 3 + ["R L", "R L", "R L", "R N", "R L", "R L", "R N", "R L", "R S", "R S", "R O", "R S"]

NEVOA = [
    "############",
    "#V.........#",
    "#..........#",
    "#..........#",
    "#..........#",
    "#..........#",
    "#..........#",
    "#..........#",
    "#..........#",
    "#.......K..#",
    "#........G.#",
    "############",
]


class Relogio:
    def __init__(self):
        self.agora = 1_000_000.0

    def __call__(self):
        return self.agora


@pytest.fixture
def pastas(tmp_path):
    cat = tmp_path / "catalogo"
    f1 = desenho.fase(EXEMPLO_FASE_4, numero=1, nome="Segura a porta")
    f2 = desenho.fase(NEVOA, numero=2, nome="Névoa de teste", nevoa=True)
    f3 = desenho.fase(["#######", "#V.K.G#", "#######"], numero=3, nome="Corredor")
    for f in (f1, f2, f3):
        catalogo.gravar(cat / catalogo.nome_arquivo(f.numero), catalogo.fase_para_dict(f, molde="m", seed="SEGREDO", gerador="g"))
    return tmp_path


def config(pastas, **mudar):
    base = dict(
        id="teste",
        braco="A",
        modo="avaliacao",
        catalogo=pastas / "catalogo",
        fases=(1, 2, 3),
        max_jogadas={1: 15 * 8 + 20, 2: 300, 3: 50},
        replay=pastas / "replay.jsonl",
        metricas=pastas / "metricas.jsonl",
    )
    base.update(mudar)
    return ConfigExecucao(**base)


def linhas(caminho):
    return [json.loads(l) for l in caminho.read_text().splitlines()]


def test_jogo_completo_da_fase_e_proxima(pastas):
    s = Sessao(config(pastas))
    v = s.ver()
    assert v.startswith("FASE 1 · jogadas 0 · inválidas 0 · reinícios 0 · versão 0")
    assert "Segura" not in v and "SEGREDO" not in v
    with pytest.raises(ErroJogo, match="ainda não acabou"):
        s.proxima_fase()
    r = s.executar(SOLUCAO_EXEMPLO[:5])
    assert "jogadas 5" in r and "versão 5" in r
    r = s.executar(SOLUCAO_EXEMPLO[5:], versao=5)
    assert "FASE 1 CONCLUÍDA em 15 jogadas" in r
    with pytest.raises(ErroJogo, match="já terminou"):
        s.executar(["V L"])
    r = s.proxima_fase()
    assert r.startswith("FASE 2 · jogadas 0")
    m = linhas(pastas / "metricas.jsonl")
    assert m[0]["tipo"] == "fase" and m[0]["resultado"] == "concluida" and m[0]["jogadas"] == 15
    assert m[0]["chamadas"]["executar"] == 2


def test_passo_invalido_para_o_plano_e_nao_conta_jogada(pastas):
    s = Sessao(config(pastas))
    r = s.executar(["V L", "V N", "V L"])
    assert "1. V L: ok" in r and "2. V N: INVÁLIDA" in r and "3." not in r
    assert s.jogadas == 1 and s.invalidas == 1 and s.versao == 1
    r = s.executar(["X Y"])
    assert "INVÁLIDA" in r and s.invalidas == 2 and s.jogadas == 1
    with pytest.raises(ErroJogo, match="mudou desde a versão 0"):
        s.executar(["V L"], versao=0)
    assert s.jogadas == 1
    with pytest.raises(ErroJogo, match="no máximo 40"):
        s.executar(["V L"] * 41)
    with pytest.raises(ErroJogo, match="lista de passos"):
        s.executar([1, 2])


def test_simular_nao_muda_nada(pastas):
    s = Sessao(config(pastas))
    r = s.simular(SOLUCAO_EXEMPLO)
    assert "A fase seria concluída" in r and "nada foi executado" in r
    assert s.jogadas == 0 and s.versao == 0 and s.simulacoes == 1
    r = s.simular(["R O"])
    assert "INVÁLIDO" in r and s.invalidas == 0


def test_limite_de_jogadas_vira_falha(pastas):
    s = Sessao(config(pastas, max_jogadas={1: 3, 2: 300, 3: 50}))
    r = s.executar(["V L", "V O", "V L", "V O", "V L"])
    assert s.situacao == "falha" and s.motivo == "jogadas" and s.jogadas == 4
    # O aviso não diz qual era o limite.
    assert r.endswith("FASE 1 FALHOU: passou do limite de jogadas da fase. Chame proxima_fase.")


def test_mais_de_5_reinicios_vira_falha(pastas):
    s = Sessao(config(pastas))
    for i in range(5):
        s.executar(["V L"])
        r = s.reiniciar_fase()
        assert "reiniciada" in r
    assert s.jogadas == 5 and s.estado == s.inicial
    r = s.reiniciar_fase()
    assert s.situacao == "falha" and s.motivo == "reinicios"
    assert "mais de 5 reinícios" in r


def test_desistir_e_modo_campanha(pastas):
    s = Sessao(config(pastas, modo="campanha"))
    s.desistir_fase()
    r = s.proxima_fase()
    assert "Execução encerrada" in r and s.encerrada
    with pytest.raises(ErroJogo, match="a execução terminou"):
        s.ver()
    m = linhas(pastas / "metricas.jsonl")
    assert m[0]["resultado"] == "desistencia" and m[-1] == {**m[-1], "tipo": "fim_execucao", "motivo": "falha_na_campanha"}


def test_avaliacao_segue_depois_de_falha_e_termina(pastas):
    s = Sessao(config(pastas))
    s.desistir_fase()
    s.proxima_fase()
    s.desistir_fase()
    s.proxima_fase()
    r = s.executar(["V L", "V L", "V L", "V L"])
    assert "CONCLUÍDA" in r
    r = s.proxima_fase()
    assert "Não há mais fases" in r and s.encerrada


def test_relogio(pastas):
    relogio = Relogio()
    s = Sessao(config(pastas, relogio_segundos=600), relogio=relogio)
    relogio.agora += 5000  # antes da primeira chamada não conta
    assert "tempo restante 10:00" in s.ver()
    relogio.agora += 61
    assert "tempo restante 8:59" in s.ver()
    assert "10 min por fase" in s.regras()
    relogio.agora += 600
    r = s.ver()
    assert s.situacao == "falha" and s.motivo == "tempo" and "o tempo da fase acabou" in r
    s.proxima_fase()
    assert "tempo restante 10:00" in s.ver()


def test_nevoa_revela_e_simular_nao(pastas):
    s = Sessao(config(pastas, fases=(2,), max_jogadas={2: 300}))
    v = s.ver()
    assert "?" in v and "Chave Dourada" not in v
    antes = set(s.conhecidas)
    r = s.simular(["V L"] * 5)
    assert "DESCONHECIDO" in r and s.conhecidas == antes
    r = s.executar(["V L", "V L"])
    assert "Células reveladas" in r and len(s.conhecidas) > len(antes)
    s.reiniciar_fase()
    assert len(s.conhecidas) > len(antes)  # o mapa descoberto continua


def test_replay_tem_tudo_e_nenhum_segredo(pastas):
    s = Sessao(config(pastas))
    s.executar(["V L", "V N"])
    s.reiniciar_fase()
    s.executar(SOLUCAO_EXEMPLO)
    s.proxima_fase()
    s.executar(["V L"])
    tipos = [l["tipo"] for l in linhas(pastas / "replay.jsonl")]
    assert tipos[0] == "cabecalho"
    for t in ("fase", "passo", "reinicio", "fim_fase", "revelacao"):
        assert t in tipos
    bruto = (pastas / "replay.jsonl").read_text()
    for proibido in ("otimo", "solucao", "max_jogadas", "SEGREDO", "gabarito"):
        assert proibido not in bruto
    fase = next(l for l in linhas(pastas / "replay.jsonl") if l["tipo"] == "fase")
    assert fase["nome"] == "Segura a porta" and fase["avatares"][0]["habilidades"] == ["entra na água"]
    invalido = next(l for l in linhas(pastas / "replay.jsonl") if l["tipo"] == "passo" and l["resultado"] == "invalida")
    assert invalido["acao"] == "V N" and invalido["motivo"]


def test_arquivo_da_execucao(pastas):
    caminho = pastas / "execucao.json"
    caminho.write_text(
        json.dumps(
            {
                "formato": 1,
                "id": "x",
                "braco": "B",
                "modo": "campanha",
                "catalogo": str(pastas / "catalogo"),
                "fases": [1, 3],
                "max_jogadas": {"1": 140, "3": 52},
                "relogio_segundos": 600,
            }
        )
    )
    cfg = carregar_execucao(caminho)
    assert cfg.fases == (1, 3) and cfg.max_jogadas == {1: 140, 3: 52} and cfg.relogio_segundos == 600
    caminho.write_text(json.dumps({"formato": 1, "id": "x", "catalogo": str(pastas), "fases": [1], "max_jogadas": {"1": 9}}))
    with pytest.raises(ErroExecucao, match="não está no catálogo"):
        carregar_execucao(caminho)
    caminho.write_text(json.dumps({"formato": 1, "id": "x", "catalogo": str(pastas / "catalogo"), "fases": [1], "max_jogadas": {}}))
    with pytest.raises(ErroExecucao, match="sem limite"):
        carregar_execucao(caminho)
