"""O que o modelo vê: linha de status, mapa com coordenadas e lista de objetos.

Regras do desenho:
- uma célula com mais de uma coisa mostra o avatar, depois o bloco, depois
  o item e por fim a célula;
- nas fases com névoa, célula desconhecida é `?` e o que está nela (bloco,
  chave, placa...) não aparece no desenho nem na lista;
- o nome da fase nunca aparece (alguns nomes dão a dica da estratégia).
"""

from __future__ import annotations

from dataclasses import dataclass

import motor
from motor import Estado, Fase, Pos

DESCONHECIDA = "?"


@dataclass(frozen=True)
class Status:
    fase: int
    jogadas: int
    invalidas: int
    reinicios: int
    versao: int
    tempo_restante: float | None = None  # segundos; só no braço com relógio
    situacao: str = ""  # "", "CONCLUÍDA", "FALHOU: ..." ou "DESISTÊNCIA"


def tempo(segundos: float) -> str:
    s = max(0, int(segundos))
    return f"{s // 60}:{s % 60:02d}"


def linha_status(s: Status) -> str:
    partes = [
        f"FASE {s.fase}",
        f"jogadas {s.jogadas}",
        f"inválidas {s.invalidas}",
        f"reinícios {s.reinicios}",
        f"versão {s.versao}",
    ]
    if s.tempo_restante is not None:
        partes.append(f"tempo restante {tempo(s.tempo_restante)}")
    if s.situacao:
        partes.append(s.situacao)
    return " · ".join(partes)


def simbolo(fase: Fase, e: Estado, p: Pos, conhecidas: frozenset[Pos] | set[Pos] | None) -> str:
    if conhecidas is not None and p not in conhecidas:
        return DESCONHECIDA
    avatar = e.avatar_em(p)
    if avatar is not None:
        return avatar
    if p in e.blocos:
        return "B"
    item = e.item_em(p)
    if item is not None:
        return "K" if item == motor.DOURADA else "k"
    return motor.tipo_atual(fase, e, p)


def desenhar(
    fase: Fase,
    e: Estado,
    conhecidas: frozenset[Pos] | set[Pos] | None = None,
    janela: tuple[int, int, int, int] | None = None,
) -> str:
    """O mapa com coordenadas. `janela` = (x0, y0, x1, y1), inclusive."""
    x0, y0, x1, y1 = janela or (0, 0, fase.largura - 1, fase.altura - 1)
    largura_coluna = len(str(x1))
    largura_rotulo = max(2, len(str(y1)))
    cabecalho = " " * (largura_rotulo + 2) + " ".join(f"{x:>{largura_coluna}}" for x in range(x0, x1 + 1))
    linhas = [cabecalho.rstrip()]
    for y in range(y0, y1 + 1):
        celulas = " ".join(f"{simbolo(fase, e, (x, y), conhecidas):>{largura_coluna}}" for x in range(x0, x1 + 1))
        linhas.append(f"{y:>{largura_rotulo}}  {celulas}")
    return "\n".join(linhas)


def _em(p: Pos) -> str:
    return f"({p[0]},{p[1]})"


def descricao_carga(item: str | None) -> str:
    if item is None:
        return "mãos vazias"
    return f"carrega {motor.nome_item(item)}"


def objetos(
    fase: Fase,
    e: Estado,
    conhecidas: frozenset[Pos] | set[Pos] | None = None,
    janela: tuple[int, int, int, int] | None = None,
) -> list[str]:
    """A lista de objetos (só o que está em células conhecidas e na janela)."""

    def visivel(p: Pos) -> bool:
        if conhecidas is not None and p not in conhecidas:
            return False
        if janela is not None:
            x0, y0, x1, y1 = janela
            return x0 <= p[0] <= x1 and y0 <= p[1] <= y1
        return True

    linhas = []
    for a, p in e.avatares:
        if visivel(p):
            linhas.append(f"- {motor.NOMES_AVATARES[a]} ({a}) em {_em(p)}, {descricao_carga(e.carga(a))}")
    for p in sorted(e.blocos, key=lambda q: (q[1], q[0])):
        if visivel(p):
            linhas.append(f"- Bloco (B) em {_em(p)}")
    for id_, p in sorted(fase.placas.items()):
        if visivel(p):
            ativa = "ativa" if motor.placa_ativa(fase, e, id_) else "inativa"
            linhas.append(f"- Placa {id_} em {_em(p)}: {ativa}")
    for porta in sorted(fase.portas, key=lambda q: q.id):
        if visivel(porta.pos):
            estado = "aberta" if porta.id in e.portas_abertas else "fechada"
            ligadas = ", ".join(porta.placas)
            nao_vistas = [pl for pl in porta.placas if conhecidas is not None and fase.placas[pl] not in conhecidas]
            extra = f" ({', '.join(nao_vistas)} ainda não {'vista' if len(nao_vistas) == 1 else 'vistas'})" if nao_vistas else ""
            linhas.append(f"- Porta {porta.id} em {_em(porta.pos)}: {estado}; abre com as placas: {ligadas}{extra}")
    for p, cor in sorted(fase.trancadas.items(), key=lambda t: (t[0][1], t[0][0])):
        if p not in e.abertas and visivel(p):
            linhas.append(f"- Porta trancada {cor} (+) em {_em(p)}: abre com a chave {cor}")
    for p, item in sorted(e.itens, key=lambda t: (t[0][1], t[0][0])):
        if item != motor.DOURADA and visivel(p):
            linhas.append(f"- Chave {item} (k) em {_em(p)}")
    for p, item in e.itens:
        if item == motor.DOURADA and visivel(p):
            linhas.append(f"- Chave Dourada (K) em {_em(p)}")
    if visivel(fase.portao):
        linhas.append(f"- Portão (G) em {_em(fase.portao)}")
    return linhas


def visao(
    status: Status,
    fase: Fase,
    e: Estado,
    conhecidas: frozenset[Pos] | set[Pos] | None = None,
) -> str:
    """As três partes: status, mapa e objetos."""
    return "\n".join(
        [linha_status(status), desenhar(fase, e, conhecidas), "", "Objetos:", *objetos(fase, e, conhecidas)]
    )


RAIO_JANELA = 4


def janela_de(fase: Fase, centro: Pos, raio: int = RAIO_JANELA) -> tuple[int, int, int, int]:
    x, y = centro
    return (max(0, x - raio), max(0, y - raio), min(fase.largura - 1, x + raio), min(fase.altura - 1, y + raio))


def visao_de_avatar(
    status: Status,
    fase: Fase,
    e: Estado,
    avatar: str,
    conhecidas: frozenset[Pos] | set[Pos] | None = None,
) -> str:
    """A mesma coisa, recortada numa janela em volta do avatar, mais o que ele carrega."""
    p = e.pos(avatar)
    janela = janela_de(fase, p)
    nome = motor.NOMES_AVATARES[avatar]
    return "\n".join(
        [
            linha_status(status),
            f"Janela em volta de {nome} ({avatar}), de ({janela[0]},{janela[1]}) a ({janela[2]},{janela[3]}):",
            desenhar(fase, e, conhecidas, janela),
            "",
            f"{nome} está em {_em(p)} e {descricao_carga(e.carga(avatar))}.",
            "",
            "Objetos na janela:",
            *(objetos(fase, e, conhecidas, janela) or ["- (nenhum)"]),
        ]
    )
