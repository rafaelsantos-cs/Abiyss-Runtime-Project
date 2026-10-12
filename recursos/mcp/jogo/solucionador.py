"""Solucionador do Travessia: o mínimo de jogadas, por A* sobre o estado conjunto.

Usado só FORA do jogo (gerador e harness): o servidor nunca resolve nada.
Ignora a névoa (o mínimo das fases com névoa é com o mapa inteiro à vista).

O estado é uma tupla de inteiros, para caber muitos na memória e ser rápido
de comparar:

    (pos de cada avatar..., local de cada item..., blocos, pontes, portas)

- posição = índice da célula (y * largura + x);
- local de item = índice da célula no chão, ou N + i (na mão do avatar i),
  ou GASTA (chave já usada);
- blocos e pontes = máscaras de bits por célula; portas = bits por porta.

Porta trancada abre de vez quando a chave da cor dela é gasta, então "aberta"
é deduzido do local da chave (o gerador usa no máximo uma porta e uma chave
por cor; `Problema` recusa outra coisa).

As regras aqui são uma segunda implementação, rápida, das de `motor.py`; os
testes conferem que as duas concordam passo a passo.

A heurística é consistente (cada jogada muda o valor em no máximo 1): a
Chave Dourada precisa chegar ao portão, e alguém precisa ir buscá-la. Por
isso o primeiro objetivo gerado já é ótimo.
"""

from __future__ import annotations

import time
from collections import deque
from dataclasses import dataclass, field

import motor

PAREDE, CHAO, AGUA, PONTE, FRESTA, PLACA, PORTA, TRANCADA, PORTAO = range(9)
_CODIGO = {
    motor.PAREDE: PAREDE,
    motor.CHAO: CHAO,
    motor.AGUA: AGUA,
    motor.PONTE: PONTE,
    motor.FRESTA: FRESTA,
    motor.PLACA: PLACA,
    motor.PORTA: PORTA,
    motor.TRANCADA: TRANCADA,
    motor.PORTAO: PORTAO,
}
DIRS = ("N", "S", "L", "O")
INF = 10**9

# Códigos de ação: avatar * 8 + (0..3 = mover N S L O, 4 = soltar, 5 + b = entregar ao avatar b).
SOLTAR = 4
ENTREGAR = 5


class SemSolucao(Exception):
    """Busca esgotada: não há solução."""

    def __init__(self, explorados: int):
        super().__init__(f"sem solução ({explorados} estados explorados)")
        self.explorados = explorados


class OrcamentoEsgotado(Exception):
    """Passou do orçamento de estados ou de tempo sem decidir."""

    def __init__(self, explorados: int, segundos: float):
        super().__init__(f"orçamento esgotado ({explorados} estados, {segundos:.1f} s)")
        self.explorados = explorados
        self.segundos = segundos


@dataclass
class Solucao:
    otimo: int
    passos: list[str]
    explorados: int
    segundos: float


@dataclass
class Orcamento:
    max_estados: int = 5_000_000
    max_segundos: float = 60.0


class Problema:
    """Uma fase compilada para a busca, com variantes para os testes de aceite.

    - `sem_avatares`: tira esses avatares da fase;
    - `sem_entregar`: a ação entregar não existe;
    - `sem_troca`: uma chave nunca passa de um avatar para outro (nem
      entregando, nem soltando para o outro pegar);
    - `objetivo_placas`: em vez do portão, o objetivo é ter todas estas
      placas ativas ao mesmo tempo (teste das alas da fase 10).
    """

    def __init__(
        self,
        fase: motor.Fase,
        *,
        sem_avatares: tuple[str, ...] = (),
        sem_entregar: bool = False,
        sem_troca: bool = False,
        objetivo_placas: tuple[str, ...] = (),
        blocos: tuple[motor.Pos, ...] | None = None,
    ):
        self.fase = fase
        self.W, self.H = fase.largura, fase.altura
        self.N = self.W * self.H
        self.sem_entregar = sem_entregar
        self.sem_troca = sem_troca
        self.letras = tuple(a for a in fase.ativos() if a not in sem_avatares)
        if not self.letras:
            raise ValueError("nenhum avatar")
        self.k = len(self.letras)
        self.tipo = [_CODIGO[fase.celula(self.p(i))] for i in range(self.N)]
        self.viz: list[tuple[int, int, int, int]] = []
        for i in range(self.N):
            x, y = i % self.W, i // self.W
            vs = []
            for dx, dy in (motor.DIRECOES[d] for d in DIRS):
                xx, yy = x + dx, y + dy
                vs.append(yy * self.W + xx if 0 <= xx < self.W and 0 <= yy < self.H else -1)
            self.viz.append(tuple(vs))
        # Itens: a Chave Dourada é sempre o índice 0.
        itens = sorted(fase.itens.items(), key=lambda t: (t[1] != motor.DOURADA, t[1], t[0]))
        self.itens = [item for _, item in itens]
        self.m = len(self.itens)
        self.GASTA = self.N + 3
        cores = [i for i in self.itens if i != motor.DOURADA]
        if len(set(cores)) != len(cores) or len(set(fase.trancadas.values())) != len(fase.trancadas):
            raise ValueError("no máximo uma chave e uma porta trancada por cor")
        # Porta trancada → índice da chave da cor (-1: não existe a chave).
        self.trancada_chave = {
            self.i(p): (self.itens.index(c) if c in self.itens else -1) for p, c in fase.trancadas.items()
        }
        self.chave_abre = {j: c for c, j in self.trancada_chave.items() if j >= 0}
        self.placa_celula = {id_: self.i(p) for id_, p in fase.placas.items()}
        self.portas = [(self.i(p.pos), tuple(self.placa_celula[pl] for pl in p.placas)) for p in fase.portas]
        self.porta_indice = {c: n for n, (c, _) in enumerate(self.portas)}
        self.portao = self.i(fase.portao)
        self.objetivo = tuple(self.placa_celula[pl] for pl in objetivo_placas)
        posicoes = tuple(self.i(fase.avatares[a]) for a in self.letras)
        locais = tuple(self.i(p) for p, _ in itens)
        lista_blocos = fase.blocos if blocos is None else blocos
        mascara = 0
        for p in lista_blocos:
            mascara |= 1 << self.i(p)
        donos = (-1,) * self.m if sem_troca else ()
        base = posicoes + locais + donos + (mascara, 0, 0)
        self.inicial = base[:-1] + (self._portas(posicoes, mascara, 0),)
        self._preparar_heuristica(bool(lista_blocos))

    # -- coordenadas ------------------------------------------------------

    def i(self, p: motor.Pos) -> int:
        return p[1] * self.W + p[0]

    def p(self, i: int) -> motor.Pos:
        return (i % self.W, i // self.W)

    # -- estado -----------------------------------------------------------

    def de_estado(self, e: motor.Estado) -> tuple:
        """Converte um Estado do motor (para continuar a busca dali)."""
        posicoes = tuple(self.i(e.pos(a)) for a in self.letras)
        locais = []
        for item in self.itens:
            local = self.GASTA
            for pp, it in e.itens:
                if it == item:
                    local = self.i(pp)
            for n, a in enumerate(self.letras):
                if e.carga(a) == item:
                    local = self.N + n
            locais.append(local)
        blocos = 0
        for b in e.blocos:
            blocos |= 1 << self.i(b)
        pontes = 0
        for b in e.pontes:
            pontes |= 1 << self.i(b)
        portas = 0
        for n, porta in enumerate(self.fase.portas):
            if porta.id in e.portas_abertas:
                portas |= 1 << n
        donos = (-1,) * self.m if self.sem_troca else ()
        return posicoes + tuple(locais) + donos + (blocos, pontes, portas)

    def _portas(self, pos: tuple, blocos: int, antes: int) -> int:
        abertas = 0
        for n, (celula, placas) in enumerate(self.portas):
            cond = True
            for pl in placas:
                if pl not in pos and not (blocos >> pl) & 1:
                    cond = False
                    break
            if cond or ((antes >> n) & 1 and celula in pos):
                abertas |= 1 << n
        return abertas

    def trancada_aberta(self, celula: int, locs: tuple) -> bool:
        j = self.trancada_chave[celula]
        return j >= 0 and locs[j] == self.GASTA

    def objetivo_cumprido(self, pos: tuple, blocos: int) -> bool:
        return bool(self.objetivo) and all(pl in pos or (blocos >> pl) & 1 for pl in self.objetivo)

    # -- sucessores -------------------------------------------------------

    def sucessores(self, s: tuple):
        """Gera (novo_estado, ação). Novo estado None = objetivo alcançado."""
        k, m, N = self.k, self.m, self.N
        GASTA = self.GASTA
        tipo, viz = self.tipo, self.viz
        pos = s[:k]
        locs = s[k : k + m]
        if self.sem_troca:
            donos = s[k + m : k + 2 * m]
            resto = k + 2 * m
        else:
            donos = None
            resto = k + m
        blocos, pontes, portas = s[resto], s[resto + 1], s[resto + 2]
        carrega = [-1, -1, -1]
        for j in range(m):
            loc = locs[j]
            if N <= loc < GASTA:
                carrega[loc - N] = j
        letras = self.letras
        for a in range(k):
            p = pos[a]
            letra = letras[a]
            c = carrega[a]
            vz = viz[p]
            for d in range(4):
                t = vz[d]
                if t < 0 or t in pos:
                    continue
                nblocos, npontes, nlocs = blocos, pontes, locs
                nc = c
                if (blocos >> t) & 1:
                    if letra != "R":
                        continue
                    b = viz[t][d]
                    if b < 0 or b in pos or (blocos >> b) & 1 or b in locs:
                        continue
                    tb = tipo[b]
                    if tb == AGUA and not (pontes >> b) & 1:
                        nblocos = blocos & ~(1 << t)
                        npontes = pontes | (1 << b)
                    elif tb == CHAO or tb == PLACA or tb == PONTE or tb == AGUA:
                        nblocos = (blocos & ~(1 << t)) | (1 << b)
                    elif tb == TRANCADA and self.trancada_aberta(b, locs):
                        nblocos = (blocos & ~(1 << t)) | (1 << b)
                    else:
                        continue
                else:
                    tt = tipo[t]
                    if tt == PAREDE:
                        continue
                    if tt == AGUA:
                        if letra != "V" and not (pontes >> t) & 1:
                            continue
                    elif tt == FRESTA:
                        if letra != "C":
                            continue
                    elif tt == PORTA:
                        if not (portas >> self.porta_indice[t]) & 1:
                            continue
                    elif tt == TRANCADA:
                        j = self.trancada_chave[t]
                        if j < 0:
                            continue
                        if locs[j] != GASTA:
                            if c != j:
                                continue
                            nlocs = locs[:j] + (GASTA,) + locs[j + 1 :]
                            nc = -1
                    elif tt == PORTAO:
                        if c != 0:
                            continue
                        yield None, a * 8 + d
                        return
                npos = pos[:a] + (t,) + pos[a + 1 :]
                ndonos = donos
                if nc < 0 and t in nlocs:
                    j = nlocs.index(t)
                    if donos is None:
                        nlocs = nlocs[:j] + (N + a,) + nlocs[j + 1 :]
                    elif donos[j] < 0 or donos[j] == a:
                        nlocs = nlocs[:j] + (N + a,) + nlocs[j + 1 :]
                        ndonos = donos[:j] + (a,) + donos[j + 1 :]
                nportas = self._portas(npos, nblocos, portas) if self.portas else 0
                if self.objetivo and self.objetivo_cumprido(npos, nblocos):
                    yield None, a * 8 + d
                    return
                if ndonos is None:
                    yield npos + nlocs + (nblocos, npontes, nportas), a * 8 + d
                else:
                    yield npos + nlocs + ndonos + (nblocos, npontes, nportas), a * 8 + d
            if c < 0:
                continue
            # soltar
            tp = tipo[p]
            pode = tp == CHAO or tp == PLACA or tp == PONTE or (tp == AGUA and (pontes >> p) & 1)
            if tp == TRANCADA:
                pode = self.trancada_aberta(p, locs)
            if pode and p not in locs:
                nlocs = locs[:c] + (p,) + locs[c + 1 :]
                if donos is None:
                    yield pos + nlocs + (blocos, pontes, portas), a * 8 + SOLTAR
                else:
                    yield pos + nlocs + donos + (blocos, pontes, portas), a * 8 + SOLTAR
            # entregar
            if self.sem_entregar or self.sem_troca:
                continue
            for b in range(k):
                if b != a and carrega[b] < 0:
                    q = pos[b]
                    if q == vz[0] or q == vz[1] or q == vz[2] or q == vz[3]:
                        nlocs = locs[:c] + (N + b,) + locs[c + 1 :]
                        yield pos + nlocs + (blocos, pontes, portas), a * 8 + ENTREGAR + b

    def notacao(self, acao: int) -> str:
        a, r = divmod(acao, 8)
        letra = self.letras[a]
        if r < 4:
            return f"{letra} {DIRS[r]}"
        if r == SOLTAR:
            return f"{letra} soltar"
        return f"{letra} entregar {self.letras[r - ENTREGAR]}"

    # -- heurística ---------------------------------------------------------

    def _bfs(self, origem: int, passa) -> list[int]:
        dist = [INF] * self.N
        dist[origem] = 0
        fila = deque([origem])
        while fila:
            u = fila.popleft()
            for v in self.viz[u]:
                if v >= 0 and dist[v] == INF and passa(v):
                    dist[v] = dist[u] + 1
                    fila.append(v)
        return dist

    def _preparar_heuristica(self, ha_blocos: bool) -> None:
        tipo = self.tipo
        self.ha_blocos = ha_blocos
        self.trancada_sem_chave = {c for c, j in self.trancada_chave.items() if j < 0}
        self._tabelas: dict[tuple[int, int], list[int]] = {}
        self._tabelas_item: dict[int, list[int]] = {}
        # Itens que precisam viajar e para onde: a Chave Dourada até o
        # portão; cada chave colorida cuja porta é obrigatória, até a porta.
        self.necessarios: list[tuple[int, list[int]]] = [(0, self._dist_item(self.portao))]
        for celula in sorted(self._portas_obrigatorias()):
            j = self.trancada_chave[celula]
            self.necessarios.append((j, self._dist_item(celula)))

    def _dist_item(self, alvo: int) -> list[int]:
        """Distância que um item percorre até `alvo`: anda por qualquer célula
        que não seja parede (de mão em mão, nadando com o Vau, pelas frestas
        com a Coruja). Cada jogada move no máximo UM item UMA célula."""
        tabela = self._tabelas_item.get(alvo)
        if tabela is None:
            tipo = self.tipo
            tabela = self._bfs(alvo, lambda v: tipo[v] != PAREDE and v not in self.trancada_sem_chave)
            self._tabelas_item[alvo] = tabela
        return tabela

    def _passa_relaxado(self, letra: str, bloqueadas: set[int]):
        tipo = self.tipo

        def passa(v):
            t = tipo[v]
            if t == PAREDE or v in self.trancada_sem_chave or v in bloqueadas:
                return False
            if t == FRESTA:
                return letra == "C"
            if t == AGUA:
                return letra == "V" or self.ha_blocos
            return True

        return passa

    def _alcancaveis(self, bloqueadas: set[int]) -> set[int]:
        """Células que algum avatar alcança no relaxado (portas de placa
        abertas, portas trancadas abertas, menos as `bloqueadas`)."""
        vistas: set[int] = set()
        for a, letra in enumerate(self.letras):
            dist = self._bfs(self.inicial[a], self._passa_relaxado(letra, bloqueadas))
            vistas.update(i for i, d in enumerate(dist) if d < INF)
        return vistas

    def _portas_obrigatorias(self) -> set[int]:
        """Portas trancadas que toda solução precisa abrir (ponto fixo): com a
        porta virando parede, o problema relaxado fica impossível."""
        obrigatorias: set[int] = set()
        locais_iniciais = self.inicial[self.k : self.k + self.m]
        mudou = True
        while mudou:
            mudou = False
            for celula, j in self.trancada_chave.items():
                if celula in obrigatorias or j < 0:
                    continue
                bloqueadas = {celula}
                alcance = self._alcancaveis(bloqueadas)

                def item_chega(item: int, alvo: int) -> bool:
                    origem = locais_iniciais[item]
                    if origem not in alcance:
                        return False
                    tipo = self.tipo
                    dist = self._bfs(
                        alvo, lambda v: tipo[v] != PAREDE and v not in self.trancada_sem_chave and v not in bloqueadas
                    )
                    return dist[origem] < INF

                possivel = item_chega(0, self.portao)
                for outra in obrigatorias:
                    possivel = possivel and item_chega(self.trancada_chave[outra], outra)
                if not possivel:
                    obrigatorias.add(celula)
                    mudou = True
        return obrigatorias

    def _dist_ate(self, a: int, alvo: int) -> list[int]:
        """Distância relaxada (portas abertas; água só para o Vau, a não ser
        que existam blocos para fazer pontes) de qualquer célula até `alvo`."""
        chave = (a, alvo)
        tabela = self._tabelas.get(chave)
        if tabela is None:
            # O grafo é simétrico: a BFS a partir do alvo serve.
            tabela = self._bfs(alvo, self._passa_relaxado(self.letras[a], set()))
            self._tabelas[chave] = tabela
        return tabela

    def h(self, s: tuple) -> int:
        """Soma do caminho que cada item necessário ainda precisa percorrer
        (cada jogada move no máximo um item uma célula) + o caminho até o
        item mais perto, se ninguém carrega nenhum item necessário (até
        alguém pegar, nenhum item necessário anda). Consistente."""
        if self.objetivo:
            return 0
        k, N, GASTA = self.k, self.N, self.GASTA
        total = 0
        carregado = False
        busca = INF
        for j, tabela in self.necessarios:
            loc = s[k + j]
            if loc == GASTA:
                continue
            if loc >= N:
                d = tabela[s[loc - N]]
                carregado = True
            else:
                d = tabela[loc]
                if not carregado:
                    for a in range(k):
                        da = self._dist_ate(a, loc)[s[a]]
                        if da < busca:
                            busca = da
            if d >= INF:
                return INF
            total += d
        if not carregado:
            if busca >= INF:
                return INF
            total += busca
        return total


def _reconstruir(problema: Problema, pai: dict, ultimo: tuple, acao_final: int) -> list[str]:
    acoes = [acao_final]
    s = ultimo
    while True:
        anterior = pai[s]
        if anterior is None:
            break
        s, acao = anterior
        acoes.append(acao)
    acoes.reverse()
    return [problema.notacao(a) for a in acoes]


def buscar(problema: Problema, orcamento: Orcamento | None = None, inicio: tuple | None = None) -> Solucao:
    """A* com fila por baldes (f inteiro); dentro do mesmo f, o mais fundo
    primeiro. Levanta SemSolucao ou OrcamentoEsgotado."""
    orcamento = orcamento or Orcamento()
    comeco = time.monotonic()
    s0 = inicio if inicio is not None else problema.inicial
    h0 = problema.h(s0)
    if h0 >= INF:
        raise SemSolucao(1)
    melhor_g = {s0: 0}
    pai: dict = {s0: None}
    baldes: list[list] = [[] for _ in range(h0 + 1)]
    baldes[h0].append((0, s0))
    fechados = set()
    f = h0
    h = problema.h
    sucessores = problema.sucessores
    explorados = 0
    while f < len(baldes):
        balde = baldes[f]
        if not balde:
            f += 1
            continue
        g, s = balde.pop()
        if s in fechados or melhor_g[s] != g:
            continue
        fechados.add(s)
        explorados += 1
        if explorados % 4096 == 0:
            gasto = time.monotonic() - comeco
            if explorados >= orcamento.max_estados or gasto >= orcamento.max_segundos:
                raise OrcamentoEsgotado(explorados, gasto)
        g1 = g + 1
        for t, acao in sucessores(s):
            if t is None:
                passos = _reconstruir(problema, pai, s, acao)
                return Solucao(len(passos), passos, explorados, time.monotonic() - comeco)
            anterior = melhor_g.get(t)
            if anterior is not None and anterior <= g1:
                continue
            ht = h(t)
            if ht >= INF:
                continue
            melhor_g[t] = g1
            pai[t] = (s, acao)
            ft = g1 + ht
            while ft >= len(baldes):
                baldes.append([])
            baldes[ft].append((g1, t))
    raise SemSolucao(explorados)


def resolver(fase: motor.Fase, orcamento: Orcamento | None = None, **variante) -> Solucao:
    return buscar(Problema(fase, **variante), orcamento)


def tem_solucao(fase: motor.Fase, orcamento: Orcamento | None = None, **variante) -> bool:
    """Decide se há solução (esgota a busca se não houver)."""
    try:
        resolver(fase, orcamento, **variante)
        return True
    except SemSolucao:
        return False


def tem_solucao_a_partir(fase: motor.Fase, estado: motor.Estado, orcamento: Orcamento | None = None) -> bool:
    problema = Problema(fase)
    try:
        buscar(problema, orcamento, problema.de_estado(estado))
        return True
    except SemSolucao:
        return False
