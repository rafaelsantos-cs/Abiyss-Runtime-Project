"""O validador do replay: aceita o que a sessão grava e pega adulterações."""

import json

import validar_replay
from sessao import Sessao
from test_sessao import SOLUCAO_EXEMPLO, config, pastas  # noqa: F401 (fixture)


def jogar(pastas):  # noqa: F811
    s = Sessao(config(pastas))
    s.executar(["V L", "V N", "V L"])
    s.reiniciar_fase()
    s.simular(["R L"])
    s.executar(SOLUCAO_EXEMPLO)
    s.proxima_fase()
    s.executar(["V L", "V S", "V L"])
    s.desistir_fase()
    s.proxima_fase()
    s.executar(["V L", "V L", "V L", "V L"])
    s.proxima_fase()
    return (pastas / "replay.jsonl").read_text().splitlines()


def test_replay_da_sessao_e_valido(pastas, capsys):  # noqa: F811
    linhas = jogar(pastas)
    v = validar_replay.Validador()
    v.validar(linhas)
    assert v.erros == []
    resumo = v.resumo()
    assert "fase  1 Segura a porta: concluida, 16 jogadas, 1 inválidas, 1 reinícios, 1 simulações" in resumo
    assert "fase  2 Névoa de teste: desistencia" in resumo
    assert "2 de 3 fase(s) concluída(s)" in resumo and "fim da execução: campanha_completa" in resumo
    assert validar_replay.main([str(pastas / "replay.jsonl")]) == 0
    assert "válido" in capsys.readouterr().out


def test_adulteracoes_sao_pegas(pastas):  # noqa: F811
    linhas = jogar(pastas)
    # Um passo inválido marcado como ok.
    adulteradas = []
    for l in linhas:
        d = json.loads(l)
        if d["tipo"] == "passo" and d["resultado"] == "invalida" and d["acao"] == "V N":
            d["resultado"] = "ok"
        adulteradas.append(json.dumps(d))
    v = validar_replay.Validador()
    v.validar(adulteradas)
    assert any("marcado ok, mas o motor diz" in e for e in v.erros)
    # Um segredo vazado.
    d = json.loads(linhas[1])
    d["otimo"] = 15
    v = validar_replay.Validador()
    v.validar([linhas[0], json.dumps(d)] + linhas[2:])
    assert any("campo proibido" in e for e in v.erros)
    # Sem cabeçalho.
    v = validar_replay.Validador()
    v.validar(linhas[1:])
    assert any("cabeçalho" in e for e in v.erros)


def test_linha_do_tempo():
    boa = [json.dumps({"tipo": "delegacao", "t": 10, "id": 1}), json.dumps({"tipo": "chamada_modelo", "t": 12})]
    assert validar_replay.validar_linha_do_tempo(boa) == []
    ruim = [json.dumps({"tipo": "xpto", "t": 1}), json.dumps({"tipo": "fase", "t": 2, "otimo": 3})]
    assert len(validar_replay.validar_linha_do_tempo(ruim)) == 2
