"""Uma execução do Travessia: as fases em sequência, o estado, os contadores,
a névoa, os limites, o replay e as métricas.

Tudo síncrono e sem MCP (o servidor só embrulha). O relógio é injetável para
os testes. Nada aqui lê o gabarito: o limite de jogadas de cada fase vem
pronto no arquivo da execução, escrito pelo harness.
"""

from __future__ import annotations

import json
import time
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

import catalogo
import motor
import regras
import texto

FORMATO_EXECUCAO = 1
FORMATO_REPLAY = 1

JOGANDO, CONCLUIDA, FALHA, DESISTENCIA = "jogando", "concluida", "falha", "desistencia"
MODOS = ("avaliacao", "campanha")


class ErroJogo(Exception):
    """Pedido que o jogo recusa (vira erro da ferramenta; não conta jogada)."""


class ErroExecucao(Exception):
    """Arquivo da execução inválido: o servidor não sobe."""


@dataclass(frozen=True)
class ConfigExecucao:
    id: str
    braco: str
    modo: str
    catalogo: Path
    fases: tuple[int, ...]
    max_jogadas: dict[int, int]
    max_reinicios: int = 5
    relogio_segundos: float | None = None
    replay: Path | None = None
    metricas: Path | None = None
    max_passos_executar: int = 40
    max_passos_simular: int = 200


def carregar_execucao(caminho: Path) -> ConfigExecucao:
    try:
        d = json.loads(Path(caminho).read_text(encoding="utf-8"))
    except (OSError, ValueError) as e:
        raise ErroExecucao(f"não consegui ler {caminho}: {e}") from None
    try:
        if d.get("formato") != FORMATO_EXECUCAO:
            raise ErroExecucao(f"formato {d.get('formato')!r} desconhecido")
        modo = d.get("modo", "avaliacao")
        if modo not in MODOS:
            raise ErroExecucao(f"modo {modo!r} inválido (use {', '.join(MODOS)})")
        fases = tuple(int(n) for n in d["fases"])
        if not fases:
            raise ErroExecucao("nenhuma fase")
        limites = {int(n): int(v) for n, v in d["max_jogadas"].items()}
        faltam = [n for n in fases if n not in limites]
        if faltam:
            raise ErroExecucao(f"sem limite de jogadas para as fases {faltam}")
        relogio = d.get("relogio_segundos")
        cfg = ConfigExecucao(
            id=str(d["id"]),
            braco=str(d.get("braco", "")),
            modo=modo,
            catalogo=Path(d["catalogo"]),
            fases=fases,
            max_jogadas=limites,
            max_reinicios=int(d.get("max_reinicios", 5)),
            relogio_segundos=float(relogio) if relogio else None,
            replay=Path(d["replay"]) if d.get("replay") else None,
            metricas=Path(d["metricas"]) if d.get("metricas") else None,
            max_passos_executar=int(d.get("max_passos_executar", 40)),
            max_passos_simular=int(d.get("max_passos_simular", 200)),
        )
    except (KeyError, TypeError, ValueError) as e:
        if isinstance(e, ErroExecucao):
            raise
        raise ErroExecucao(f"arquivo da execução incompleto: {e}") from None
    for n in cfg.fases:
        arquivo = cfg.catalogo / catalogo.nome_arquivo(n)
        if not arquivo.is_file():
            raise ErroExecucao(f"fase {n} não está no catálogo ({arquivo})")
    return cfg


class Registro:
    """Um arquivo JSONL (replay ou métricas). Cada linha vai inteira e já
    gravada no disco: o arquivo serve mesmo se o processo morrer."""

    def __init__(self, caminho: Path | None):
        self.caminho = caminho
        if caminho is not None:
            caminho.parent.mkdir(parents=True, exist_ok=True)

    def escrever(self, linha: dict[str, Any]) -> None:
        if self.caminho is None:
            return
        with self.caminho.open("a", encoding="utf-8") as f:
            f.write(json.dumps(linha, ensure_ascii=False, separators=(",", ":")) + "\n")


HABILIDADES = {
    "V": ["entra na água"],
    "R": ["empurra blocos"],
    "C": ["passa por frestas", "enxerga longe"],
}


def _objetos_do_estado(e: motor.Estado) -> dict[str, Any]:
    return {
        "avatares": {a: {"pos": list(p), "carrega": e.carga(a)} for a, p in e.avatares},
        "blocos": [list(p) for p in sorted(e.blocos)],
        "itens": [{"item": i, "pos": list(p)} for p, i in e.itens],
        "pontes": [list(p) for p in sorted(e.pontes)],
        "trancadas_abertas": [list(p) for p in sorted(e.abertas)],
        "portas_abertas": sorted(e.portas_abertas),
    }


@dataclass
class ResultadoPasso:
    passo: str
    ok: bool
    mensagem: str
    desconhecido: bool = False


@dataclass
class Fim:
    resultado: str
    motivo: str


class Sessao:
    def __init__(self, cfg: ConfigExecucao, relogio: Callable[[], float] = time.time):
        self.cfg = cfg
        self.relogio = relogio
        self.replay = Registro(cfg.replay)
        self.metricas = Registro(cfg.metricas)
        self.versao = 0
        self.indice = -1
        self.encerrada = False
        self.motivo_encerramento = ""
        self.replay.escrever(
            {
                "tipo": "cabecalho",
                "formato": FORMATO_REPLAY,
                "execucao": cfg.id,
                "braco": cfg.braco,
                "modo": cfg.modo,
                "relogio_segundos": cfg.relogio_segundos,
                "data": time.strftime("%Y-%m-%dT%H:%M:%S%z", time.localtime(self.relogio())),
                "t": self._ms(),
            }
        )
        self._carregar(0)

    # -- utilidades -------------------------------------------------------

    def _ms(self) -> int:
        return int(self.relogio() * 1000)

    @property
    def numero(self) -> int:
        return self.cfg.fases[self.indice]

    @property
    def jogando(self) -> bool:
        return self.situacao == JOGANDO and not self.encerrada

    def _carregar(self, indice: int) -> None:
        self.indice = indice
        self.fase = catalogo.carregar_fase(self.cfg.catalogo / catalogo.nome_arquivo(self.numero))
        self.inicial = motor.estado_inicial(self.fase)
        self.estado = self.inicial
        self.jogadas = self.invalidas = self.reinicios = self.simulacoes = 0
        self.chamadas: Counter = Counter()
        self.situacao, self.motivo = JOGANDO, ""
        self.inicio: float | None = None
        self.conhecidas: set[motor.Pos] | None = set() if self.fase.nevoa else None
        f = self.fase
        self.replay.escrever(
            {
                "tipo": "fase",
                "t": self._ms(),
                "numero": f.numero,
                "nome": f.nome,
                "largura": f.largura,
                "altura": f.altura,
                "celulas": list(f.celulas),
                "objetos": catalogo.fase_para_dict(f, molde="", seed="", gerador="")["objetos"],
                "avatares": [
                    {
                        "letra": a,
                        "nome": motor.NOMES_AVATARES[a],
                        "pos": list(p),
                        "habilidades": HABILIDADES[a],
                        "raio": f.raios[a],
                    }
                    for a, p in sorted(f.avatares.items(), key=lambda t: motor.ORDEM_AVATARES.index(t[0]))
                ],
                "nevoa": f.nevoa,
                "versao": self.versao,
            }
        )
        self._revelar()

    def _revelar(self) -> list[motor.Pos]:
        if self.conhecidas is None:
            return []
        novas = sorted(motor.visiveis(self.fase, self.estado) - self.conhecidas, key=lambda p: (p[1], p[0]))
        if novas:
            self.conhecidas.update(novas)
            self.replay.escrever({"tipo": "revelacao", "t": self._ms(), "celulas": [list(p) for p in novas]})
        return novas

    def _status(self, situacao: str = "") -> texto.Status:
        restante = None
        if self.cfg.relogio_segundos:
            gasto = (self.relogio() - self.inicio) if self.inicio is not None else 0.0
            restante = max(0.0, self.cfg.relogio_segundos - gasto)
        if not situacao:
            situacao = {
                CONCLUIDA: "CONCLUÍDA",
                FALHA: f"FALHOU ({self.motivo})",
                DESISTENCIA: "DESISTÊNCIA",
            }.get(self.situacao, "")
        return texto.Status(
            fase=self.numero,
            jogadas=self.jogadas,
            invalidas=self.invalidas,
            reinicios=self.reinicios,
            versao=self.versao,
            tempo_restante=restante,
            situacao=situacao,
        )

    def _visao(self) -> str:
        return texto.visao(self._status(), self.fase, self.estado, self.conhecidas)

    def _terminar(self, resultado: str, motivo: str) -> None:
        self.situacao, self.motivo = resultado, motivo
        agora = self._ms()
        inicio_ms = int(self.inicio * 1000) if self.inicio is not None else agora
        linha = {
            "numero": self.numero,
            "resultado": resultado,
            "motivo": motivo,
            "jogadas": self.jogadas,
            "invalidas": self.invalidas,
            "reinicios": self.reinicios,
            "simulacoes": self.simulacoes,
            "tempo_s": round((agora - inicio_ms) / 1000, 3),
        }
        self.replay.escrever({"tipo": "fim_fase", "t": agora, **linha})
        self.metricas.escrever(
            {"tipo": "fase", **linha, "chamadas": dict(self.chamadas), "inicio_ms": inicio_ms, "fim_ms": agora}
        )

    def _encerrar(self, motivo: str) -> None:
        if self.encerrada:
            return
        self.encerrada = True
        self.motivo_encerramento = motivo
        self.metricas.escrever(
            {"tipo": "fim_execucao", "t": self._ms(), "motivo": motivo, "fases_jogadas": self.indice + 1}
        )
        self.replay.escrever({"tipo": "fim_execucao", "t": self._ms(), "motivo": motivo})

    def _entrada(self, ferramenta: str) -> None:
        """Comum a toda chamada: conta, liga o relógio da fase na primeira
        chamada e confere o tempo (braço com relógio)."""
        if self.encerrada:
            raise ErroJogo(f"a execução terminou ({self.motivo_encerramento}); não há mais fases")
        self.chamadas[ferramenta] += 1
        if self.inicio is None:
            self.inicio = self.relogio()
        self._conferir_tempo()

    def _conferir_tempo(self) -> None:
        if (
            self.situacao == JOGANDO
            and self.cfg.relogio_segundos
            and self.inicio is not None
            and self.relogio() - self.inicio >= self.cfg.relogio_segundos
        ):
            self._terminar(FALHA, "tempo")

    def _exigir_jogando(self) -> None:
        if self.situacao != JOGANDO:
            raise ErroJogo(
                f"a fase {self.numero} já terminou ({self._status().situacao.lower()}); chame proxima_fase"
            )

    def _aviso_fim(self) -> str:
        if self.situacao == CONCLUIDA:
            return f"\nFASE {self.numero} CONCLUÍDA em {self.jogadas} jogadas. Chame proxima_fase."
        if self.situacao == FALHA:
            motivo = {
                "jogadas": "passou do limite de jogadas da fase",
                "reinicios": f"mais de {self.cfg.max_reinicios} reinícios",
                "tempo": "o tempo da fase acabou",
            }.get(self.motivo, self.motivo)
            return f"\nFASE {self.numero} FALHOU: {motivo}. Chame proxima_fase."
        if self.situacao == DESISTENCIA:
            return f"\nVocê desistiu da fase {self.numero}. Chame proxima_fase."
        return ""

    # -- ferramentas ----------------------------------------------------------

    def regras(self) -> str:
        self._entrada("regras")
        texto_regras = regras.texto(
            self.cfg.max_reinicios, self.cfg.max_passos_executar, self.cfg.relogio_segundos, self.cfg.modo
        )
        return f"{texto_regras}\nAgora: fase {self.numero}, versão {self.versao}."

    def ver(self, avatar: str | None = None) -> str:
        self._entrada("ver")
        if avatar is None or not str(avatar).strip():
            return self._visao() + self._aviso_fim()
        letra = str(avatar).strip().upper()[:1]
        if letra not in dict(self.estado.avatares):
            ativos = ", ".join(self.fase.ativos())
            raise ErroJogo(f"avatar {avatar!r} não joga esta fase (use {ativos})")
        return texto.visao_de_avatar(self._status(), self.fase, self.estado, letra, self.conhecidas) + self._aviso_fim()

    @staticmethod
    def _plano(plano: Any, maximo: int, nome: str) -> list[str]:
        if isinstance(plano, str):
            plano = [plano]
        if not isinstance(plano, list) or not all(isinstance(p, str) for p in plano):
            raise ErroJogo('o plano é uma lista de passos, como ["R L", "V soltar", "V entregar R"]')
        if not plano:
            raise ErroJogo("o plano está vazio")
        if len(plano) > maximo:
            raise ErroJogo(f"{nome} aceita no máximo {maximo} passos por chamada (vieram {len(plano)})")
        return plano

    def simular(self, plano: Any) -> str:
        self._entrada("simular")
        self._exigir_jogando()
        passos = self._plano(plano, self.cfg.max_passos_simular, "simular")
        self.simulacoes += 1
        versao = self.versao
        conhecidas = frozenset(self.conhecidas) if self.conhecidas is not None else None
        e = self.estado
        linhas = [f"SIMULAÇÃO a partir da versão {versao} (nada foi executado; não conta jogada):"]
        aplicados = 0
        for i, bruto in enumerate(passos, 1):
            try:
                passo = motor.interpretar(bruto)
            except motor.ErroNotacao as erro:
                linhas.append(f"{i}. {bruto}: INVÁLIDO — {erro} (a simulação parou aqui)")
                break
            r = motor.aplicar(self.fase, e, passo, conhecidas)
            if r.desconhecido:
                linhas.append(f"{i}. {passo.notacao()}: DESCONHECIDO — {r.mensagem} (a simulação parou aqui)")
                break
            if not r.ok:
                linhas.append(f"{i}. {passo.notacao()}: INVÁLIDO — {r.mensagem} (a simulação parou aqui)")
                break
            linhas.append(f"{i}. {passo.notacao()}: ok — {r.mensagem}")
            e = r.estado
            aplicados += 1
            if e.concluida:
                linhas.append("A fase seria concluída neste passo.")
                break
        linhas += [
            "",
            f"Estado final simulado (FASE {self.numero} · {aplicados} de {len(passos)} passo(s) aplicados · "
            f"a partir da versão {versao}):",
            texto.desenhar(self.fase, e, conhecidas),
            "",
            "Objetos:",
            *texto.objetos(self.fase, e, conhecidas),
        ]
        return "\n".join(linhas)

    def executar(self, plano: Any, versao: int | None = None) -> str:
        self._entrada("executar")
        self._exigir_jogando()
        passos = self._plano(plano, self.cfg.max_passos_executar, "executar")
        if versao is not None and int(versao) != self.versao:
            raise ErroJogo(
                f"o estado mudou desde a versão {versao} (agora é a versão {self.versao}); "
                "nada foi executado. Veja o estado atual e simule de novo."
            )
        linhas = ["Passos:"]
        reveladas_total: list[motor.Pos] = []
        feitos = 0
        for i, bruto in enumerate(passos, 1):
            try:
                passo = motor.interpretar(bruto)
            except motor.ErroNotacao as erro:
                self.invalidas += 1
                self.replay.escrever(
                    {
                        "tipo": "passo",
                        "t": self._ms(),
                        "versao": self.versao,
                        "avatar": None,
                        "acao": str(bruto),
                        "resultado": "invalida",
                        "motivo": str(erro),
                        "mudancas": [],
                    }
                )
                linhas.append(f"{i}. {bruto}: INVÁLIDA — {erro} (nada mudou; o plano parou aqui)")
                break
            r = motor.aplicar(self.fase, self.estado, passo)
            if not r.ok:
                self.invalidas += 1
                self.replay.escrever(
                    {
                        "tipo": "passo",
                        "t": self._ms(),
                        "versao": self.versao,
                        "avatar": passo.avatar,
                        "acao": passo.notacao(),
                        "resultado": "invalida",
                        "motivo": r.mensagem,
                        "mudancas": [],
                    }
                )
                linhas.append(f"{i}. {passo.notacao()}: INVÁLIDA — {r.mensagem} (nada mudou; o plano parou aqui)")
                break
            self.estado = r.estado
            self.jogadas += 1
            self.versao += 1
            self.replay.escrever(
                {
                    "tipo": "passo",
                    "t": self._ms(),
                    "versao": self.versao,
                    "avatar": passo.avatar,
                    "acao": passo.notacao(),
                    "resultado": "ok",
                    "mudancas": list(r.mudancas),
                }
            )
            linhas.append(f"{i}. {passo.notacao()}: ok — {r.mensagem}")
            feitos = i
            reveladas_total += self._revelar()
            if self.estado.concluida:
                self._terminar(CONCLUIDA, "")
                break
            if self.jogadas > self.cfg.max_jogadas[self.numero]:
                self._terminar(FALHA, "jogadas")
                break
            self._conferir_tempo()
            if self.situacao != JOGANDO:
                break
        if feitos == i and feitos < len(passos) and self.situacao != JOGANDO:
            linhas.append(f"(a fase acabou: {len(passos) - feitos} passo(s) do plano não foram executados)")
        if reveladas_total:
            linhas.append(f"Células reveladas: {len(reveladas_total)}.")
        return "\n".join(linhas + ["", self._visao()]) + self._aviso_fim()

    def reiniciar_fase(self) -> str:
        self._entrada("reiniciar_fase")
        self._exigir_jogando()
        self.reinicios += 1
        if self.reinicios > self.cfg.max_reinicios:
            self._terminar(FALHA, "reinicios")
            return self._visao() + self._aviso_fim()
        self.estado = self.inicial
        self.versao += 1
        self.replay.escrever(
            {
                "tipo": "reinicio",
                "t": self._ms(),
                "versao": self.versao,
                "reinicios": self.reinicios,
                "estado": _objetos_do_estado(self.estado),
            }
        )
        self._revelar()
        return "Fase reiniciada (o mapa descoberto continua descoberto; as jogadas continuam somando).\n\n" + self._visao()

    def desistir_fase(self) -> str:
        self._entrada("desistir_fase")
        self._exigir_jogando()
        self.versao += 1
        self._terminar(DESISTENCIA, "desistencia")
        return f"Você desistiu da fase {self.numero} (conta como falha; versão {self.versao}). Chame proxima_fase."

    def proxima_fase(self) -> str:
        self._entrada("proxima_fase")
        if self.situacao == JOGANDO:
            raise ErroJogo(
                f"a fase {self.numero} ainda não acabou: conclua (abra o portão com a Chave Dourada) "
                "ou desista (desistir_fase)"
            )
        if self.cfg.modo == "campanha" and self.situacao != CONCLUIDA:
            self._encerrar("falha_na_campanha")
            return (
                f"Execução encerrada: no modo campanha, a primeira falha encerra o jogo "
                f"(fase {self.numero}; versão {self.versao})."
            )
        if self.indice + 1 >= len(self.cfg.fases):
            self._encerrar("campanha_completa")
            return f"Não há mais fases: a execução terminou (versão {self.versao})."
        self.versao += 1
        self._carregar(self.indice + 1)
        self.inicio = self.relogio()
        return self._visao()

    # -- para o harness e os testes ---------------------------------------------

    def resumo(self) -> dict[str, Any]:
        return {
            "fase": self.numero,
            "situacao": self.situacao,
            "versao": self.versao,
            "jogadas": self.jogadas,
            "encerrada": self.encerrada,
        }
