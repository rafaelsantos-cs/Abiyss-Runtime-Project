"""Gerador de fases do Travessia: seed → catálogo conferido pelo solucionador.

    uv run gerador.py --seed CAMPANHA --catalogo DIR --gabarito DIR [--fases 1-12]

1. A seed da campanha gera a seed de cada fase por hash (seed + número da
   fase + tentativa). Tentativa rejeitada: a seguinte usa a próxima
   sub-seed, também determinística.
2. O molde da fase sorteia posições, tamanhos e detalhes.
3. O solucionador acha o mínimo de jogadas (A*, heurística admissível),
   ignorando a névoa.
4. A fase só entra no catálogo se passar nos testes de aceite (`ESPECS`).

Saída: `<catalogo>/fase-NN.json` (mapa, objetos, molde, seed, versão do
gerador) e `<gabarito>/fase-NN.json` (ótimo, solução, armadilha e o
resultado dos testes). O gabarito é só do harness: nunca o ponha onde o
Abiyss alcança (workspace, caixa do terminal).

Determinismo: a mesma seed com a mesma versão do gerador produz os mesmos
bytes. Por isso a REJEIÇÃO usa só o orçamento de estados (que não depende da
máquina); estourar o orçamento de TEMPO é erro de geração, não rejeição.
"""

from __future__ import annotations

import argparse
import hashlib
import random
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

import catalogo
import moldes
import motor
import solucionador
from planta import transformacao
from solucionador import Orcamento, OrcamentoEsgotado, Problema, SemSolucao

VERSAO_GERADOR = "travessia-1"
MAX_TENTATIVAS = 300
# Estados que a busca da armadilha examina, e quantos candidatos testa.
MAX_CANDIDATOS_ARMADILHA = 12


@dataclass(frozen=True)
class Espec:
    numero: int
    nome: str
    molde: str
    avatares: str
    nevoa: bool
    tamanho: int
    faixa: tuple[int, int]
    # Testes de aceite além de "tem solução" e "ótimo na faixa".
    sem_cada_avatar: bool = False  # 4 a 12
    entregar_poupa: bool = False  # 5
    troca_obrigatoria: bool = False  # 11
    armadilha: bool = False  # 3, 11 e 12
    alas: bool = False  # 10


ESPECS: dict[int, Espec] = {
    e.numero: e
    for e in [
        Espec(1, "Lago", "lago", "V", False, 7, (8, 14)),
        Espec(2, "Empurrão", "empurrao", "R", False, 8, (12, 22)),
        Espec(3, "Ponte", "ponte", "R", False, 9, (20, 35), armadilha=True),
        Espec(4, "Segura a porta", "segura_a_porta", "VR", False, 9, (15, 25), sem_cada_avatar=True),
        Espec(5, "Mão em mão", "mao_em_mao", "VR", False, 10, (25, 40), sem_cada_avatar=True, entregar_poupa=True),
        Espec(6, "Duas placas", "duas_placas", "VR", False, 10, (30, 45), sem_cada_avatar=True),
        Espec(7, "Corrente", "corrente", "VR", False, 11, (40, 60), sem_cada_avatar=True),
        Espec(8, "Primeira névoa", "primeira_nevoa", "VRC", True, 12, (35, 55), sem_cada_avatar=True),
        Espec(9, "Fresta", "fresta", "VRC", True, 12, (45, 65), sem_cada_avatar=True),
        Espec(10, "Três frentes", "tres_frentes", "VRC", True, 14, (60, 90), sem_cada_avatar=True, alas=True),
        Espec(
            11, "Relé", "rele", "VRC", True, 14, (70, 100), sem_cada_avatar=True, troca_obrigatoria=True, armadilha=True
        ),
        Espec(12, "Travessia", "travessia", "VRC", True, 16, (90, 140), sem_cada_avatar=True, armadilha=True),
    ]
}


class ErroGeracao(Exception):
    """A geração não pôde terminar (nenhuma tentativa aceita, ou tempo)."""


@dataclass
class Resultado:
    fase: dict
    gabarito: dict
    tentativa: int
    rejeicoes: dict[str, int] = field(default_factory=dict)


def seed_da_fase(seed_campanha: str, numero: int, tentativa: int) -> str:
    texto = f"{VERSAO_GERADOR}:{seed_campanha}:{numero}:{tentativa}"
    return hashlib.sha256(texto.encode("utf-8")).hexdigest()[:16]


def _decidir(problema: Problema, orcamento: Orcamento, inicio=None) -> str:
    """"sim", "nao" ou "indecidivel" (orçamento de estados). Tempo: erro."""
    try:
        solucionador.buscar(problema, orcamento, inicio)
        return "sim"
    except SemSolucao:
        return "nao"
    except OrcamentoEsgotado as e:
        if e.por_tempo:
            raise ErroGeracao(f"orçamento de tempo esgotado ({e}); aumente --max-segundos") from None
        return "indecidivel"


# Orçamento da busca curta por uma armadilha a partir de cada estado da
# solução ótima (determinístico: só estados).
ESTADOS_POR_PONTO_DA_SOLUCAO = 20_000


def _armadilha(
    fase: motor.Fase, rascunho: moldes.Rascunho, solucao: list[str], orcamento: Orcamento
) -> list[str] | None:
    """Um plano válido que leva a um estado sem solução, ou None.

    Percorre a solução ótima; de cada estado dela, procura (busca em largura
    curta) um estado da armadilha do molde e confere que ele não tem solução.
    O plano devolvido = prefixo da solução + o caminho até a armadilha."""
    t = transformacao(rascunho.planta.largura, rascunho.simetria)
    problema = Problema(fase)
    blocos = 0
    for p in rascunho.armadilha.blocos_em:
        blocos |= 1 << problema.i(t(p))
    pontes = 0
    for p in rascunho.armadilha.pontes_em:
        pontes |= 1 << problema.i(t(p))

    def predicado(s):
        return bool(s[-3] & blocos) or bool(s[-2] & pontes)

    curto = Orcamento(max_estados=ESTADOS_POR_PONTO_DA_SOLUCAO, max_segundos=orcamento.max_segundos)
    e = motor.estado_inicial(fase)
    candidatos = 0
    try:
        for i in range(len(solucao) + 1):
            if i > 0:
                e = motor.aplicar(fase, e, motor.interpretar(solucao[i - 1])).estado
                if e.concluida:
                    break
            for plano, estado in solucionador.estados_que(problema, predicado, curto, problema.de_estado(e)):
                candidatos += 1
                if _decidir(problema, orcamento, estado) == "nao":
                    return solucao[:i] + plano
                if candidatos >= MAX_CANDIDATOS_ARMADILHA:
                    return None
                break  # um candidato por ponto da solução
    except OrcamentoEsgotado as e:
        raise ErroGeracao(f"orçamento de tempo esgotado na busca da armadilha ({e})") from None
    return None


def aceitar(
    espec: Espec, fase: motor.Fase, rascunho: moldes.Rascunho, orcamento: Orcamento
) -> tuple[dict | None, str]:
    """Roda os testes de aceite. Devolve (gabarito, "") ou (None, motivo)."""
    if "".join(fase.ativos()) != "".join(a for a in motor.ORDEM_AVATARES if a in espec.avatares):
        return None, "avatares"
    if fase.largura != espec.tamanho or fase.altura != espec.tamanho:
        return None, "tamanho"
    problema = Problema(fase)
    try:
        sol = solucionador.buscar(problema, orcamento)
    except SemSolucao:
        return None, "sem_solucao"
    except OrcamentoEsgotado as e:
        if e.por_tempo:
            raise ErroGeracao(f"orçamento de tempo esgotado ({e}); aumente --max-segundos") from None
        return None, "orcamento"
    lo, hi = espec.faixa
    if not lo <= sol.otimo <= hi:
        return None, "fora_da_faixa"
    # Confere a solução no motor (as duas implementações das regras).
    e, rs = motor.executar_plano(fase, motor.estado_inicial(fase), [motor.interpretar(p) for p in sol.passos])
    if not (e.concluida and all(r.ok for r in rs) and len(rs) == sol.otimo):
        raise ErroGeracao(f"fase {espec.numero}: a solução do solucionador não executa no motor")
    testes: dict = {"otimo_na_faixa": [lo, hi]}
    if espec.sem_cada_avatar:
        for a in fase.ativos():
            r = _decidir(Problema(fase, sem_avatares=(a,)), orcamento)
            if r != "nao":
                return None, f"resolve_sem_{a}" if r == "sim" else "orcamento"
        testes["sem_cada_avatar"] = "sem solução"
    if espec.entregar_poupa:
        try:
            sem = solucionador.buscar(Problema(fase, sem_entregar=True), orcamento).otimo
        except SemSolucao:
            sem = None
        except OrcamentoEsgotado:
            return None, "orcamento"
        if sem is not None and sem <= sol.otimo:
            return None, "entregar_nao_poupa"
        testes["otimo_sem_entregar"] = sem
    if espec.troca_obrigatoria:
        r = _decidir(Problema(fase, sem_troca=True), orcamento)
        if r != "nao":
            return None, "resolve_sem_troca" if r == "sim" else "orcamento"
        testes["sem_troca"] = "sem solução"
    armadilha = None
    if espec.armadilha:
        armadilha = _armadilha(fase, rascunho, sol.passos, orcamento)
        if armadilha is None:
            return None, "sem_armadilha"
        testes["armadilha"] = len(armadilha)
    if espec.alas:
        for avatar, placa in rascunho.alas:
            outros = tuple(a for a in fase.ativos() if a != avatar)
            r = _decidir(Problema(fase, sem_avatares=outros, objetivo_placas=(placa,)), orcamento)
            if r != "sim":
                return None, f"ala_{avatar}"
        testes["alas"] = [list(a) for a in rascunho.alas]
    gabarito = {
        "formato": catalogo.FORMATO,
        "gerador": VERSAO_GERADOR,
        "numero": espec.numero,
        "otimo": sol.otimo,
        "solucao": sol.passos,
        "explorados": sol.explorados,
        "armadilha": armadilha,
        "testes": testes,
    }
    return gabarito, ""


def gerar_fase(
    seed_campanha: str, numero: int, orcamento: Orcamento | None = None, max_tentativas: int = MAX_TENTATIVAS
) -> Resultado:
    espec = ESPECS[numero]
    orcamento = orcamento or Orcamento()
    rejeicoes: dict[str, int] = {}
    for tentativa in range(max_tentativas):
        sub = seed_da_fase(seed_campanha, numero, tentativa)
        rng = random.Random(int(sub, 16))
        try:
            rascunho = moldes.MOLDES[espec.molde](rng)
            fase = rascunho.planta.fase(numero, espec.nome, nevoa=espec.nevoa, simetria=rascunho.simetria)
        except (RuntimeError, motor.ErroFase, ValueError, IndexError) as e:
            motivo = f"rascunho_invalido:{type(e).__name__}"
            rejeicoes[motivo] = rejeicoes.get(motivo, 0) + 1
            continue
        gabarito, motivo = aceitar(espec, fase, rascunho, orcamento)
        if gabarito is None:
            rejeicoes[motivo] = rejeicoes.get(motivo, 0) + 1
            continue
        dados = catalogo.fase_para_dict(fase, molde=espec.molde, seed=sub, gerador=VERSAO_GERADOR)
        gabarito["seed"] = sub
        gabarito["tentativa"] = tentativa
        return Resultado(dados, gabarito, tentativa, rejeicoes)
    raise ErroGeracao(f"fase {numero}: nenhuma de {max_tentativas} tentativas foi aceita ({rejeicoes})")


def gerar_campanha(
    seed_campanha: str,
    pasta_catalogo: Path,
    pasta_gabarito: Path,
    fases: list[int] | None = None,
    orcamento: Orcamento | None = None,
    log=None,
) -> dict[int, str]:
    """Gera e grava as fases; devolve o sha256 do arquivo de cada uma."""
    hashes = {}
    for numero in fases or sorted(ESPECS):
        comeco = time.monotonic()
        r = gerar_fase(seed_campanha, numero, orcamento)
        hashes[numero] = catalogo.gravar(pasta_catalogo / catalogo.nome_arquivo(numero), r.fase)
        catalogo.gravar(pasta_gabarito / catalogo.nome_arquivo(numero), r.gabarito)
        if log:
            log(
                f"fase {numero:2d} ({ESPECS[numero].molde}): ótimo {r.gabarito['otimo']}, "
                f"tentativa {r.tentativa}, {r.gabarito['explorados']} estados, "
                f"{time.monotonic() - comeco:.1f} s"
            )
    return hashes


def _faixa_de_fases(texto: str) -> list[int]:
    numeros: set[int] = set()
    for parte in texto.split(","):
        if "-" in parte:
            a, b = parte.split("-", 1)
            numeros.update(range(int(a), int(b) + 1))
        elif parte.strip():
            numeros.add(int(parte))
    desconhecidas = numeros - set(ESPECS)
    if desconhecidas:
        raise SystemExit(f"fases sem molde: {sorted(desconhecidas)}")
    return sorted(numeros)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Gera o catálogo do Travessia a partir de uma seed.")
    ap.add_argument("--seed", required=True, help="seed da campanha (texto qualquer)")
    ap.add_argument("--catalogo", required=True, type=Path, help="pasta das fases (o servidor lê)")
    ap.add_argument("--gabarito", required=True, type=Path, help="pasta do gabarito (só o harness lê)")
    ap.add_argument("--fases", default=f"1-{max(ESPECS)}", help="ex.: 1-12 ou 1,3,5")
    ap.add_argument("--max-estados", type=int, default=5_000_000)
    ap.add_argument("--max-segundos", type=float, default=60.0)
    args = ap.parse_args(argv)
    if args.catalogo.resolve() == args.gabarito.resolve():
        print("gerador: catálogo e gabarito precisam ficar em pastas diferentes", file=sys.stderr)
        return 2
    orcamento = Orcamento(max_estados=args.max_estados, max_segundos=args.max_segundos)
    try:
        hashes = gerar_campanha(
            args.seed,
            args.catalogo,
            args.gabarito,
            _faixa_de_fases(args.fases),
            orcamento,
            log=lambda texto: print(f"gerador: {texto}", file=sys.stderr, flush=True),
        )
    except ErroGeracao as e:
        print(f"gerador: {e}", file=sys.stderr)
        return 1
    for numero, h in hashes.items():
        print(f"{catalogo.nome_arquivo(numero)} {h}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
