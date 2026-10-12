"""Valida um replay do Travessia e imprime um resumo em texto.

    uv run validar_replay.py ARQUIVO.jsonl [--linha-do-tempo ARQUIVO.jsonl]

Confere o formato (docs/REPLAY.md), a ordem das linhas e que nada do
gabarito vazou (ótimo, solução, limite). Também refaz cada fase no motor a
partir da linha `fase`: os passos válidos precisam dar ok, os inválidos
precisam ser inválidos, e as mudanças e o resultado do fim precisam bater.
Código de saída: 0 = válido, 1 = inválido, 2 = não consegui ler.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import catalogo
import motor

FORMATO = 1
TIPOS = {"cabecalho", "fase", "passo", "revelacao", "reinicio", "fim_fase", "fim_execucao"}
PROIBIDOS = {"otimo", "solucao", "max_jogadas", "limite", "gabarito", "seed", "molde"}
CAMPOS = {
    "cabecalho": {"formato", "execucao", "braco", "modo", "data", "t"},
    "fase": {"t", "numero", "nome", "largura", "altura", "celulas", "objetos", "avatares", "nevoa", "versao"},
    "passo": {"t", "versao", "avatar", "acao", "resultado", "mudancas"},
    "revelacao": {"t", "celulas"},
    "reinicio": {"t", "versao", "reinicios", "estado"},
    "fim_fase": {"t", "numero", "resultado", "motivo", "jogadas", "invalidas", "reinicios", "simulacoes", "tempo_s"},
    "fim_execucao": {"t", "motivo"},
}
TIPOS_LINHA_DO_TEMPO = {"chamada_modelo", "delegacao", "subagente_fim", "ferramenta", "fase"}


def _chaves(valor, caminho=""):
    """Todas as chaves de dicionário, em qualquer profundidade."""
    if isinstance(valor, dict):
        for k, v in valor.items():
            yield k
            yield from _chaves(v)
    elif isinstance(valor, list):
        for v in valor:
            yield from _chaves(v)


class Validador:
    def __init__(self):
        self.erros: list[str] = []
        self.fases: list[dict] = []
        self.cabecalho: dict | None = None
        self.fim_execucao: dict | None = None

    def erro(self, n: int, texto: str) -> None:
        self.erros.append(f"linha {n}: {texto}")

    def validar(self, linhas: list[str]) -> None:
        fase = None
        estado = None
        ultimo_t = None
        ultima_versao = -1
        for n, bruta in enumerate(linhas, 1):
            if not bruta.strip():
                continue
            try:
                d = json.loads(bruta)
            except ValueError as e:
                self.erro(n, f"JSON inválido: {e}")
                continue
            tipo = d.get("tipo")
            if tipo not in TIPOS:
                self.erro(n, f"tipo desconhecido {tipo!r}")
                continue
            faltam = CAMPOS[tipo] - set(d)
            if faltam:
                self.erro(n, f"{tipo}: faltam os campos {sorted(faltam)}")
                continue
            vazados = PROIBIDOS & set(_chaves(d))
            if vazados:
                self.erro(n, f"{tipo}: campo proibido no replay {sorted(vazados)}")
            if ultimo_t is not None and d["t"] < ultimo_t:
                self.erro(n, "o instante voltou no tempo")
            ultimo_t = d["t"]
            if "versao" in d:
                if d["versao"] < ultima_versao:
                    self.erro(n, f"a versão voltou ({ultima_versao} → {d['versao']})")
                ultima_versao = d["versao"]
            if n == 1 or self.cabecalho is None:
                if tipo != "cabecalho":
                    self.erro(n, "a primeira linha precisa ser o cabeçalho")
                    continue
                if d["formato"] != FORMATO:
                    self.erro(n, f"formato {d['formato']} desconhecido (este validador conhece o {FORMATO})")
                self.cabecalho = d
                continue
            if tipo == "cabecalho":
                self.erro(n, "cabeçalho repetido")
            elif tipo == "fase":
                if fase is not None and fase.get("fim") is None:
                    self.erro(n, f"a fase {fase['numero']} não terminou antes da próxima")
                try:
                    f = catalogo.fase_de_dict(
                        {
                            "formato": catalogo.FORMATO,
                            "numero": d["numero"],
                            "nome": d["nome"],
                            "largura": d["largura"],
                            "altura": d["altura"],
                            "nevoa": d["nevoa"],
                            "raios": {a["letra"]: a["raio"] for a in d["avatares"]},
                            "mapa": d["celulas"],
                            "objetos": d["objetos"],
                        }
                    )
                except motor.ErroFase as e:
                    self.erro(n, f"fase {d['numero']} inválida: {e}")
                    fase, estado = None, None
                    continue
                fase = {"numero": d["numero"], "nome": d["nome"], "def": f, "fim": None, "passos": 0}
                estado = motor.estado_inicial(f)
                self.fases.append(fase)
            elif fase is None:
                if tipo != "fim_execucao":
                    self.erro(n, f"{tipo} antes de qualquer fase")
            elif tipo == "passo":
                fase["passos"] += 1
                if fase["fim"] is not None:
                    self.erro(n, "passo depois do fim da fase")
                try:
                    passo = motor.interpretar(d["acao"])
                except motor.ErroNotacao:
                    if d["resultado"] != "invalida":
                        self.erro(n, f"passo {d['acao']!r} fora da notação marcado como {d['resultado']}")
                    continue
                r = motor.aplicar(fase["def"], estado, passo)
                if d["resultado"] == "ok":
                    if not r.ok:
                        self.erro(n, f"passo {d['acao']} marcado ok, mas o motor diz: {r.mensagem}")
                        continue
                    if [dict(m) for m in r.mudancas] != d["mudancas"]:
                        self.erro(n, f"passo {d['acao']}: as mudanças não batem com o motor")
                    estado = r.estado
                elif r.ok:
                    self.erro(n, f"passo {d['acao']} marcado inválido, mas o motor aceita")
            elif tipo == "reinicio":
                estado = motor.estado_inicial(fase["def"])
            elif tipo == "fim_fase":
                if d["numero"] != fase["numero"]:
                    self.erro(n, f"fim da fase {d['numero']} durante a fase {fase['numero']}")
                if d["resultado"] not in ("concluida", "falha", "desistencia"):
                    self.erro(n, f"resultado {d['resultado']!r} desconhecido")
                if (d["resultado"] == "concluida") != estado.concluida:
                    self.erro(n, "o resultado do fim não bate com o estado refeito pelo motor")
                fase["fim"] = d
            elif tipo == "fim_execucao":
                self.fim_execucao = d
        if self.cabecalho is None:
            self.erros.append("arquivo sem cabeçalho")

    def resumo(self) -> str:
        c = self.cabecalho or {}
        linhas = [f"Execução {c.get('execucao', '?')} · braço {c.get('braco', '?')} · modo {c.get('modo', '?')}"]
        concluidas = 0
        for f in self.fases:
            fim = f["fim"]
            if fim is None:
                linhas.append(f"  fase {f['numero']:2d} {f['nome']}: sem fim registrado ({f['passos']} passos)")
                continue
            concluidas += fim["resultado"] == "concluida"
            motivo = f" ({fim['motivo']})" if fim["resultado"] == "falha" and fim["motivo"] else ""
            linhas.append(
                f"  fase {f['numero']:2d} {f['nome']}: {fim['resultado']}{motivo}, {fim['jogadas']} jogadas, "
                f"{fim['invalidas']} inválidas, {fim['reinicios']} reinícios, {fim['simulacoes']} simulações, "
                f"{fim['tempo_s']:.1f} s"
            )
        linhas.append(f"{concluidas} de {len(self.fases)} fase(s) concluída(s)")
        if self.fim_execucao:
            linhas.append(f"fim da execução: {self.fim_execucao['motivo']}")
        return "\n".join(linhas)


def validar_linha_do_tempo(linhas: list[str]) -> list[str]:
    erros = []
    ultimo_t = None
    for n, bruta in enumerate(linhas, 1):
        if not bruta.strip():
            continue
        try:
            d = json.loads(bruta)
        except ValueError as e:
            erros.append(f"linha do tempo, linha {n}: JSON inválido: {e}")
            continue
        if d.get("tipo") not in TIPOS_LINHA_DO_TEMPO or "t" not in d:
            erros.append(f"linha do tempo, linha {n}: tipo {d.get('tipo')!r} desconhecido ou sem instante")
            continue
        if ultimo_t is not None and d["t"] < ultimo_t - 60_000:
            erros.append(f"linha do tempo, linha {n}: instante muito fora de ordem")
        ultimo_t = max(ultimo_t or d["t"], d["t"])
        vazados = PROIBIDOS & set(_chaves(d))
        if vazados:
            erros.append(f"linha do tempo, linha {n}: campo proibido {sorted(vazados)}")
    return erros


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Valida um replay do Travessia e imprime um resumo.")
    ap.add_argument("replay", type=Path)
    ap.add_argument("--linha-do-tempo", type=Path, help="a linha do tempo do harness, para conferir junto")
    args = ap.parse_args(argv)
    try:
        linhas = args.replay.read_text(encoding="utf-8").splitlines()
        tempo = args.linha_do_tempo.read_text(encoding="utf-8").splitlines() if args.linha_do_tempo else []
    except OSError as e:
        print(f"validar_replay: {e}", file=sys.stderr)
        return 2
    v = Validador()
    v.validar(linhas)
    erros = v.erros + validar_linha_do_tempo(tempo)
    print(v.resumo())
    if erros:
        print(f"\nINVÁLIDO: {len(erros)} problema(s)")
        for e in erros[:50]:
            print(f"- {e}")
        return 1
    print("\nválido")
    return 0


if __name__ == "__main__":
    sys.exit(main())
