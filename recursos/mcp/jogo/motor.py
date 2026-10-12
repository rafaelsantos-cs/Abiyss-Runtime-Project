"""Regras do Travessia: a única fonte da verdade do jogo.

Tudo aqui é puro e determinístico: uma `Fase` (o que vem do catálogo, não
muda) e um `Estado` (imutável; cada passo devolve um novo). Nada lê relógio,
sorteia ou toca em disco. O servidor, o solucionador (pelos testes de
equivalência) e o gerador dependem destas regras.

Coordenadas (x, y) a partir do canto superior esquerdo: x cresce para a
direita, y para baixo. Direções: N (y − 1), S (y + 1), L (x + 1), O (x − 1).
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field, replace
from typing import Iterable

Pos = tuple[int, int]

DIRECOES: dict[str, Pos] = {"N": (0, -1), "S": (0, 1), "L": (1, 0), "O": (-1, 0)}
NOMES_DIRECOES = {"N": "norte", "S": "sul", "L": "leste", "O": "oeste"}

# Ordem fixa dos avatares (desenho, lista de objetos, solucionador).
ORDEM_AVATARES = ("V", "R", "C")
NOMES_AVATARES = {"V": "Vau", "R": "Rocha", "C": "Coruja"}
RAIOS_PADRAO = {"V": 2, "R": 2, "C": 5}

# Células-base do catálogo. `d` (porta aberta) e `?` (desconhecida) só
# aparecem no desenho: são estado, não célula.
CHAO, PAREDE, AGUA, PONTE, FRESTA, PLACA = ".", "#", "~", "=", ":", "_"
PORTA, TRANCADA, PORTAO = "D", "+", "G"
CELULAS = {CHAO, PAREDE, AGUA, PONTE, FRESTA, PLACA, PORTA, TRANCADA, PORTAO}

# Item: a cor de uma chave colorida, ou a Chave Dourada.
DOURADA = "dourada"
CORES = ("vermelha", "azul", "verde", "amarela")


def nome_item(item: str) -> str:
    return "a Chave Dourada" if item == DOURADA else f"a chave {item}"


def somar(p: Pos, d: Pos) -> Pos:
    return (p[0] + d[0], p[1] + d[1])


def distancia_rei(a: Pos, b: Pos) -> int:
    return max(abs(a[0] - b[0]), abs(a[1] - b[1]))


class ErroFase(ValueError):
    """Fase do catálogo inválida."""


@dataclass(frozen=True)
class PortaDePlaca:
    id: str
    pos: Pos
    placas: tuple[str, ...]


@dataclass(frozen=True)
class Fase:
    """O que não muda durante a fase (vem do catálogo)."""

    numero: int
    nome: str
    largura: int
    altura: int
    celulas: tuple[str, ...]
    placas: dict[str, Pos]
    portas: tuple[PortaDePlaca, ...]
    trancadas: dict[Pos, str]
    avatares: dict[str, Pos]
    blocos: tuple[Pos, ...]
    itens: dict[Pos, str]
    nevoa: bool = False
    raios: dict[str, int] = field(default_factory=lambda: dict(RAIOS_PADRAO))

    def celula(self, p: Pos) -> str:
        return self.celulas[p[1]][p[0]]

    def dentro(self, p: Pos) -> bool:
        return 0 <= p[0] < self.largura and 0 <= p[1] < self.altura

    @property
    def portao(self) -> Pos:
        for y, linha in enumerate(self.celulas):
            x = linha.find(PORTAO)
            if x >= 0:
                return (x, y)
        raise ErroFase("fase sem portão")

    def ativos(self) -> tuple[str, ...]:
        return tuple(a for a in ORDEM_AVATARES if a in self.avatares)

    def placa_em(self, p: Pos) -> str | None:
        for id_, pos in self.placas.items():
            if pos == p:
                return id_
        return None

    def porta_em(self, p: Pos) -> PortaDePlaca | None:
        for porta in self.portas:
            if porta.pos == p:
                return porta
        return None

    def validar(self) -> None:
        """Confere a coerência da fase; levanta ErroFase."""
        if len(self.celulas) != self.altura or any(len(l) != self.largura for l in self.celulas):
            raise ErroFase("mapa não bate com largura × altura")
        for y, linha in enumerate(self.celulas):
            for x, c in enumerate(linha):
                if c not in CELULAS:
                    raise ErroFase(f"célula desconhecida {c!r} em ({x},{y})")
        if sum(l.count(PORTAO) for l in self.celulas) != 1:
            raise ErroFase("a fase precisa de exatamente um portão")
        if not self.avatares or any(a not in ORDEM_AVATARES for a in self.avatares):
            raise ErroFase("avatares inválidos")
        ocupadas: set[Pos] = set()
        for a, p in self.avatares.items():
            if not self.dentro(p) or not pode_ficar(a, self.celula(p)) or p in ocupadas:
                raise ErroFase(f"avatar {a} em posição inválida {p}")
            ocupadas.add(p)
        for p in self.blocos:
            if not self.dentro(p) or self.celula(p) not in (CHAO, PONTE, PLACA) or p in ocupadas:
                raise ErroFase(f"bloco em posição inválida {p}")
            ocupadas.add(p)
        if list(self.itens.values()).count(DOURADA) != 1:
            raise ErroFase("a fase precisa de exatamente uma Chave Dourada")
        for p, item in self.itens.items():
            if not self.dentro(p) or self.celula(p) not in (CHAO, PONTE, PLACA) or p in self.blocos:
                raise ErroFase(f"item em posição inválida {p}")
            if item != DOURADA and item not in CORES:
                raise ErroFase(f"item desconhecido {item!r}")
        celulas_placa = {(x, y) for y, l in enumerate(self.celulas) for x, c in enumerate(l) if c == PLACA}
        if set(self.placas.values()) != celulas_placa or len(self.placas) != len(celulas_placa):
            raise ErroFase("placas da lista não batem com as células _")
        celulas_porta = {(x, y) for y, l in enumerate(self.celulas) for x, c in enumerate(l) if c == PORTA}
        if {p.pos for p in self.portas} != celulas_porta or len(self.portas) != len(celulas_porta):
            raise ErroFase("portas da lista não batem com as células D")
        for porta in self.portas:
            if not porta.placas or any(pl not in self.placas for pl in porta.placas):
                raise ErroFase(f"porta {porta.id} ligada a placa inexistente")
        celulas_trancada = {(x, y) for y, l in enumerate(self.celulas) for x, c in enumerate(l) if c == TRANCADA}
        if set(self.trancadas) != celulas_trancada:
            raise ErroFase("portas trancadas da lista não batem com as células +")
        for cor in self.trancadas.values():
            if cor not in CORES:
                raise ErroFase(f"cor de porta desconhecida {cor!r}")


def pode_ficar(avatar: str, celula: str) -> bool:
    """O avatar pode estar parado numa célula deste tipo (sem olhar portas)."""
    if celula == PAREDE:
        return False
    if celula == AGUA:
        return avatar == "V"
    if celula == FRESTA:
        return avatar == "C"
    return True


@dataclass(frozen=True)
class Estado:
    """O que muda. Imutável: cada passo devolve um novo Estado."""

    avatares: tuple[tuple[str, Pos], ...]
    carregando: tuple[tuple[str, str | None], ...]
    blocos: frozenset[Pos]
    itens: tuple[tuple[Pos, str], ...]
    pontes: frozenset[Pos]
    abertas: frozenset[Pos]  # portas trancadas já abertas (viraram chão)
    portas_abertas: frozenset[str]
    concluida: bool = False

    def pos(self, avatar: str) -> Pos:
        return dict(self.avatares)[avatar]

    def carga(self, avatar: str) -> str | None:
        return dict(self.carregando)[avatar]

    def avatar_em(self, p: Pos) -> str | None:
        for a, q in self.avatares:
            if q == p:
                return a
        return None

    def item_em(self, p: Pos) -> str | None:
        for q, item in self.itens:
            if q == p:
                return item
        return None


def estado_inicial(fase: Fase) -> Estado:
    ativos = fase.ativos()
    e = Estado(
        avatares=tuple((a, fase.avatares[a]) for a in ativos),
        carregando=tuple((a, None) for a in ativos),
        blocos=frozenset(fase.blocos),
        itens=tuple(sorted(fase.itens.items())),
        pontes=frozenset(),
        abertas=frozenset(),
        portas_abertas=frozenset(),
    )
    return replace(e, portas_abertas=_recalcular_portas(fase, e))


def tipo_atual(fase: Fase, e: Estado, p: Pos) -> str:
    """A célula como ela está agora: água com bloco vira ponte, porta
    trancada aberta vira chão, porta de placa aberta vira `d`."""
    c = fase.celula(p)
    if c == AGUA and p in e.pontes:
        return PONTE
    if c == TRANCADA and p in e.abertas:
        return CHAO
    if c == PORTA:
        porta = fase.porta_em(p)
        if porta is not None and porta.id in e.portas_abertas:
            return "d"
    return c


def placa_ativa(fase: Fase, e: Estado, id_placa: str) -> bool:
    p = fase.placas[id_placa]
    return p in e.blocos or e.avatar_em(p) is not None


def _recalcular_portas(fase: Fase, e: Estado) -> frozenset[str]:
    """Porta de placa: aberta enquanto todas as placas dela estão ativas.
    Nunca fecha com alguém dentro: fecha assim que fica vazia."""
    abertas = set()
    for porta in fase.portas:
        condicao = all(placa_ativa(fase, e, pl) for pl in porta.placas)
        ocupada = e.avatar_em(porta.pos) is not None
        if condicao or (ocupada and porta.id in e.portas_abertas):
            abertas.add(porta.id)
    return frozenset(abertas)


# --------------------------------------------------------------------------
# Passos (a notação das ações)
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class Passo:
    avatar: str
    acao: str  # "mover", "soltar" ou "entregar"
    direcao: str | None = None
    alvo: str | None = None

    def notacao(self) -> str:
        if self.acao == "mover":
            return f"{self.avatar} {self.direcao}"
        if self.acao == "soltar":
            return f"{self.avatar} soltar"
        return f"{self.avatar} entregar {self.alvo}"


class ErroNotacao(ValueError):
    """O texto não é um passo na notação do jogo."""


_PASSO = re.compile(r"^\s*([A-Za-z])\s+(\S+)(?:\s+(\S+))?\s*$")


def interpretar(texto: str) -> Passo:
    """"R L" → mover; "V soltar"; "V entregar R"."""
    if not isinstance(texto, str):
        raise ErroNotacao("cada passo é um texto, como \"R L\", \"V soltar\" ou \"V entregar R\"")
    m = _PASSO.match(texto)
    if not m:
        raise ErroNotacao(f"{texto!r} não está na notação (ex.: \"R L\", \"V soltar\", \"V entregar R\")")
    avatar, verbo, extra = m.group(1).upper(), m.group(2), m.group(3)
    if avatar not in ORDEM_AVATARES:
        raise ErroNotacao(f"{texto!r}: avatar {avatar!r} não existe (use V, R ou C)")
    if verbo.upper() in DIRECOES and extra is None:
        return Passo(avatar, "mover", direcao=verbo.upper())
    if verbo.lower() == "soltar" and extra is None:
        return Passo(avatar, "soltar")
    if verbo.lower() == "entregar" and extra is not None:
        alvo = extra.upper()
        if alvo not in ORDEM_AVATARES:
            raise ErroNotacao(f"{texto!r}: avatar {extra!r} não existe (use V, R ou C)")
        return Passo(avatar, "entregar", alvo=alvo)
    raise ErroNotacao(
        f"{texto!r} não está na notação: mover = avatar e direção (N, S, L ou O), "
        "ou \"<avatar> soltar\", ou \"<avatar> entregar <avatar>\""
    )


# --------------------------------------------------------------------------
# Aplicar um passo
# --------------------------------------------------------------------------


@dataclass(frozen=True)
class Resultado:
    ok: bool
    mensagem: str
    estado: Estado
    # O que mudou, para o replay (lista de dicionários simples).
    mudancas: tuple[dict, ...] = ()
    # True quando a simulação parou por esbarrar no desconhecido.
    desconhecido: bool = False


def _invalido(e: Estado, mensagem: str, desconhecido: bool = False) -> Resultado:
    return Resultado(False, mensagem, e, desconhecido=desconhecido)


def _com(e: Estado, **mudar) -> Estado:
    return replace(e, **mudar)


def _mover_avatar(e: Estado, avatar: str, destino: Pos) -> Estado:
    return _com(e, avatares=tuple((a, destino if a == avatar else p) for a, p in e.avatares))


def _definir_carga(e: Estado, avatar: str, item: str | None) -> Estado:
    return _com(e, carregando=tuple((a, item if a == avatar else c) for a, c in e.carregando))


def _sem_item(e: Estado, p: Pos) -> Estado:
    return _com(e, itens=tuple((q, i) for q, i in e.itens if q != p))


def aplicar(fase: Fase, e: Estado, passo: Passo, conhecidas: set[Pos] | frozenset[Pos] | None = None) -> Resultado:
    """Aplica um passo. Inválido: o estado volta igual, com o motivo.

    `conhecidas` (só na simulação de fases com névoa): entrar numa célula
    desconhecida, ou empurrar um bloco para uma, interrompe com "desconhecido".
    """
    if e.concluida:
        return _invalido(e, "a fase já terminou")
    ativos = dict(e.avatares)
    if passo.avatar not in ativos:
        return _invalido(e, f"{NOMES_AVATARES[passo.avatar]} não joga esta fase")
    if passo.acao == "mover":
        r = _mover(fase, e, passo.avatar, passo.direcao, conhecidas)
    elif passo.acao == "soltar":
        r = _soltar(fase, e, passo.avatar)
    else:
        r = _entregar(fase, e, passo.avatar, passo.alvo)
    if not r.ok:
        return r
    novas = _recalcular_portas(fase, r.estado)
    mudancas = list(r.mudancas)
    for porta in fase.portas:
        antes, depois = porta.id in e.portas_abertas, porta.id in novas
        if antes != depois:
            mudancas.append({"tipo": "porta", "id": porta.id, "pos": list(porta.pos), "aberta": depois})
    return replace(r, estado=_com(r.estado, portas_abertas=novas), mudancas=tuple(mudancas))


def _mover(fase: Fase, e: Estado, avatar: str, direcao: str, conhecidas) -> Resultado:
    nome = NOMES_AVATARES[avatar]
    origem = e.pos(avatar)
    alvo = somar(origem, DIRECOES[direcao])
    para = f"para o {NOMES_DIRECOES[direcao]}"
    if not fase.dentro(alvo):
        return _invalido(e, f"{nome} não pode sair do mapa")
    if conhecidas is not None and alvo not in conhecidas:
        return _invalido(e, f"desconhecido: {nome} entraria em ({alvo[0]},{alvo[1]}), que ainda não foi vista", True)
    outro = e.avatar_em(alvo)
    if outro is not None:
        return _invalido(e, f"{nome} não pode entrar em ({alvo[0]},{alvo[1]}): {NOMES_AVATARES[outro]} está lá")
    mudancas: list[dict] = []
    estado = e
    if alvo in e.blocos:
        if avatar != "R":
            return _invalido(e, f"{nome} não empurra blocos (só a Rocha)")
        destino = somar(alvo, DIRECOES[direcao])
        if conhecidas is not None and fase.dentro(destino) and destino not in conhecidas:
            return _invalido(
                e, f"desconhecido: o bloco iria para ({destino[0]},{destino[1]}), que ainda não foi vista", True
            )
        motivo = _destino_do_bloco_invalido(fase, e, destino)
        if motivo:
            return _invalido(e, f"a Rocha não consegue empurrar o bloco {para}: {motivo}")
        blocos = set(e.blocos)
        blocos.discard(alvo)
        if tipo_atual(fase, e, destino) == AGUA:
            estado = _com(estado, blocos=frozenset(blocos), pontes=e.pontes | {destino})
            mudancas.append({"tipo": "bloco", "de": list(alvo), "para": list(destino), "afundou": True})
        else:
            blocos.add(destino)
            estado = _com(estado, blocos=frozenset(blocos))
            mudancas.append({"tipo": "bloco", "de": list(alvo), "para": list(destino), "afundou": False})
    else:
        tipo = tipo_atual(fase, e, alvo)
        carga = e.carga(avatar)
        if tipo == PAREDE:
            return _invalido(e, f"{nome} não atravessa parede")
        if tipo == AGUA and avatar != "V":
            return _invalido(e, f"{nome} não entra na água (só o Vau)")
        if tipo == FRESTA and avatar != "C":
            return _invalido(e, f"{nome} não passa por frestas (só a Coruja)")
        if tipo == PORTA:
            return _invalido(e, f"a porta em ({alvo[0]},{alvo[1]}) está fechada")
        if tipo == TRANCADA:
            cor = fase.trancadas[alvo]
            if carga != cor:
                return _invalido(e, f"a porta trancada {cor} em ({alvo[0]},{alvo[1]}) só abre com a chave {cor}")
            estado = _definir_carga(_com(estado, abertas=e.abertas | {alvo}), avatar, None)
            mudancas.append({"tipo": "trancada", "pos": list(alvo), "cor": cor})
        if tipo == PORTAO:
            if carga != DOURADA:
                return _invalido(e, "o portão só abre para quem traz a Chave Dourada")
            estado = _com(_definir_carga(estado, avatar, None), concluida=True)
            mudancas.append({"tipo": "portao", "pos": list(alvo)})
    estado = _mover_avatar(estado, avatar, alvo)
    mudancas.insert(0, {"tipo": "avatar", "avatar": avatar, "de": list(origem), "para": list(alvo)})
    mensagem = f"{nome} foi {para}, para ({alvo[0]},{alvo[1]})"
    if mudancas and any(m["tipo"] == "bloco" for m in mudancas):
        bloco = next(m for m in mudancas if m["tipo"] == "bloco")
        destino = tuple(bloco["para"])
        mensagem += (
            f"; o bloco afundou em ({destino[0]},{destino[1]}) e virou ponte"
            if bloco["afundou"]
            else f", empurrando o bloco para ({destino[0]},{destino[1]})"
        )
    if any(m["tipo"] == "trancada" for m in mudancas):
        mensagem += f"; a porta trancada abriu e a chave {fase.trancadas[alvo]} foi gasta"
    if estado.concluida:
        return Resultado(True, mensagem + "; o portão abriu: fase concluída!", estado, tuple(mudancas))
    item = estado.item_em(alvo)
    if item is not None and estado.carga(avatar) is None:
        estado = _definir_carga(_sem_item(estado, alvo), avatar, item)
        mudancas.append({"tipo": "pegar", "avatar": avatar, "item": item, "pos": list(alvo)})
        mensagem += f"; pegou {nome_item(item)}"
    elif item is not None:
        mensagem += f"; {nome_item(item)} ficou no chão (mãos ocupadas)"
    return Resultado(True, mensagem, estado, tuple(mudancas))


def _destino_do_bloco_invalido(fase: Fase, e: Estado, destino: Pos) -> str | None:
    if not fase.dentro(destino):
        return "o bloco sairia do mapa"
    tipo = tipo_atual(fase, e, destino)
    if tipo not in (CHAO, PONTE, PLACA, AGUA):
        descricao = {
            PAREDE: "há uma parede",
            FRESTA: "blocos não passam por frestas",
            PORTA: "blocos não entram em portas",
            "d": "blocos não entram em portas",
            TRANCADA: "há uma porta trancada",
            PORTAO: "há o portão",
        }.get(tipo, "o lugar não aceita bloco")
        return f"{descricao} em ({destino[0]},{destino[1]})"
    if e.avatar_em(destino) is not None:
        return f"{NOMES_AVATARES[e.avatar_em(destino)]} está em ({destino[0]},{destino[1]})"
    if destino in e.blocos:
        return f"há outro bloco em ({destino[0]},{destino[1]})"
    if e.item_em(destino) is not None:
        return f"há um item em ({destino[0]},{destino[1]})"
    return None


def _soltar(fase: Fase, e: Estado, avatar: str) -> Resultado:
    nome = NOMES_AVATARES[avatar]
    item = e.carga(avatar)
    p = e.pos(avatar)
    if item is None:
        return _invalido(e, f"{nome} está de mãos vazias")
    if tipo_atual(fase, e, p) not in (CHAO, PONTE, PLACA):
        return _invalido(e, f"{nome} só solta itens em chão, ponte ou placa")
    if e.item_em(p) is not None:
        return _invalido(e, f"já há um item em ({p[0]},{p[1]})")
    estado = _definir_carga(_com(e, itens=tuple(sorted(e.itens + ((p, item),)))), avatar, None)
    return Resultado(
        True,
        f"{nome} soltou {nome_item(item)} em ({p[0]},{p[1]})",
        estado,
        ({"tipo": "soltar", "avatar": avatar, "item": item, "pos": list(p)},),
    )


def _entregar(fase: Fase, e: Estado, avatar: str, alvo: str) -> Resultado:
    nome = NOMES_AVATARES[avatar]
    if alvo == avatar:
        return _invalido(e, f"{nome} não pode entregar para si mesmo")
    if alvo not in dict(e.avatares):
        return _invalido(e, f"{NOMES_AVATARES[alvo]} não joga esta fase")
    item = e.carga(avatar)
    if item is None:
        return _invalido(e, f"{nome} está de mãos vazias")
    a, b = e.pos(avatar), e.pos(alvo)
    if abs(a[0] - b[0]) + abs(a[1] - b[1]) != 1:
        return _invalido(e, f"{NOMES_AVATARES[alvo]} não está ao lado de {nome}")
    if e.carga(alvo) is not None:
        return _invalido(e, f"{NOMES_AVATARES[alvo]} está com as mãos ocupadas")
    estado = _definir_carga(_definir_carga(e, avatar, None), alvo, item)
    return Resultado(
        True,
        f"{nome} entregou {nome_item(item)} para {NOMES_AVATARES[alvo]}",
        estado,
        ({"tipo": "entregar", "de": avatar, "para": alvo, "item": item},),
    )


# --------------------------------------------------------------------------
# Névoa
# --------------------------------------------------------------------------


def visiveis(fase: Fase, e: Estado) -> set[Pos]:
    """Células dentro do raio de visão de algum avatar (distância do rei;
    paredes não bloqueiam a visão)."""
    vistas: set[Pos] = set()
    for a, (x, y) in e.avatares:
        r = fase.raios.get(a, RAIOS_PADRAO[a])
        for yy in range(max(0, y - r), min(fase.altura, y + r + 1)):
            for xx in range(max(0, x - r), min(fase.largura, x + r + 1)):
                vistas.add((xx, yy))
    return vistas


def todas(fase: Fase) -> frozenset[Pos]:
    return frozenset((x, y) for y in range(fase.altura) for x in range(fase.largura))


def executar_plano(
    fase: Fase, e: Estado, passos: Iterable[Passo], conhecidas: set[Pos] | frozenset[Pos] | None = None
) -> tuple[Estado, list[Resultado]]:
    """Aplica os passos em ordem e para no primeiro inválido."""
    resultados = []
    for passo in passos:
        r = aplicar(fase, e, passo, conhecidas)
        resultados.append(r)
        if not r.ok:
            break
        e = r.estado
        if e.concluida:
            break
    return e, resultados
