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
    """Passou do orçamento de estados ou de tempo sem decidir. `por_tempo`
    diz qual: o de estados é determinístico; o de tempo depende da máquina."""

    def __init__(self, explorados: int, segundos: float, por_tempo: bool = False):
        motivo = "tempo" if por_tempo else "estados"
        super().__init__(f"orçamento de {motivo} esgotado ({explorados} estados, {segundos:.1f} s)")
        self.explorados = explorados
        self.segundos = segundos
        self.por_tempo = por_tempo


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

    # -- heurística: relaxamentos ------------------------------------------
    #
    # Tudo aqui SUPERESTIMA o que é possível (portas de placa abertas, chaves
    # que chegam), para as distâncias serem limites INFERIORES. Três
    # refinamentos que continuam corretos e deixam a busca muito menor:
    #
    # - pontes contadas: quem não nada só atravessa água com ponte, e cada
    #   bloco que ainda existe vira no máximo uma ponte; um caminho que
    #   precisa de mais células de água sem ponte do que blocos restantes é
    #   impossível;
    # - porta exclusiva: se uma placa da porta só pode ser ativada por um
    #   avatar (ninguém mais chega nela, nenhum bloco chega nela) e não é
    #   vizinha da porta, esse avatar nunca atravessa a porta (teria de
    #   estar na placa e na porta ao mesmo tempo); se ninguém ativa a placa,
    #   a porta nunca abre;
    # - sem troca (variante): a chave só anda por onde o dono anda.

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

    def _tabela(self, ponto: int, custo, nb: int, ate: bool) -> list[int]:
        """Distâncias com pontes contadas. `custo(v)`: -1 = não entra, 0 =
        entra, 1 = entra gastando um bloco (água sem ponte). `ate=True`:
        distância de cada célula ATÉ `ponto`; senão, DE `ponto` até cada uma."""
        N, viz = self.N, self.viz
        dist = [INF] * N
        usados = [nb + 1] * N
        dist[ponto] = 0
        if ate:
            usados[ponto] = 0
            fila = deque([(ponto, 0, 0)])
        else:
            usados[ponto] = 0
            fila = deque([(ponto, 0, 0)])
        while fila:
            v, u_v, d = fila.popleft()
            if ate:
                # Quem está num vizinho u entra em v: paga o custo de v.
                c = custo(v)
                if c < 0:
                    continue
                nu = u_v + c
                if nu > nb:
                    continue
                for u in viz[v]:
                    if u >= 0 and nu < usados[u]:
                        usados[u] = nu
                        if d + 1 < dist[u]:
                            dist[u] = d + 1
                        fila.append((u, nu, d + 1))
            else:
                for w in viz[v]:
                    if w < 0:
                        continue
                    c = custo(w)
                    if c < 0:
                        continue
                    nu = u_v + c
                    if nu <= nb and nu < usados[w]:
                        usados[w] = nu
                        if d + 1 < dist[w]:
                            dist[w] = d + 1
                        fila.append((w, nu, d + 1))
        return dist

    def _custo_avatar(self, a: int, pontes: int, bloqueadas=frozenset(), afundaveis: int = -1):
        """`afundaveis`: máscara da água que algum bloco ainda alcança (-1 =
        toda). Água sem ponte e fora dela não vira ponte nunca."""
        letra, tipo = self.letras[a], self.tipo
        proibidas = self.exclusivas[a] | self.trancada_sem_chave | bloqueadas

        def custo(v):
            t = tipo[v]
            if t == PAREDE or v in proibidas:
                return -1
            if t == FRESTA:
                return 0 if letra == "C" else -1
            if t == AGUA and letra != "V" and not (pontes >> v) & 1:
                return 1 if (afundaveis >> v) & 1 else -1
            return 0

        return custo

    def _custo_item(self, pontes: int, bloqueadas=frozenset(), afundaveis: int = -1):
        """Um item anda no colo de qualquer avatar: por frestas (Coruja), na
        água onde o Vau alcança; água fora do alcance do Vau só com ponte."""
        tipo = self.tipo
        proibidas = self.exclusiva_todos | self.trancada_sem_chave | bloqueadas
        alcance_v = self.alcance_v

        def custo(v):
            t = tipo[v]
            if t == PAREDE or v in proibidas:
                return -1
            if t == AGUA and not (pontes >> v) & 1 and v not in alcance_v:
                return 1 if (afundaveis >> v) & 1 else -1
            return 0

        return custo

    def afundaveis(self, blocos: int, pontes: int) -> int:
        """Máscara da água sem ponte que algum destes blocos ainda alcança
        (blocos não passam por portas de placa, frestas nem paredes)."""
        if not self.empurra:
            return 0
        mascara = 0
        lista = bits(blocos)
        # Com dois blocos ou mais, um bloco pode passar por cima da ponte que
        # outro fizer antes: a água vira caminho (relaxado). Com um só, a
        # água sem ponte é onde ele afunda e para.
        varios = len(lista) >= 2
        for b in lista:
            chave = (b, pontes, varios)
            m = self._agua_do_bloco.get(chave)
            if m is None:
                m = 0
                dist = self.dist_bloco_de(b, pontes, varios)
                for w in self.agua:
                    if dist[w] < INF and not (pontes >> w) & 1:
                        m |= 1 << w
                self._agua_do_bloco[chave] = m
            mascara |= m
        return mascara

    def dist_bloco_de(self, origem: int, pontes: int, atravessa_agua: bool = False) -> list[int]:
        """Por onde um bloco em `origem` pode andar (relaxado)."""
        tipo = self.tipo

        def passa(v):
            t = tipo[v]
            return t in (CHAO, PLACA, PONTE, AGUA) or (t == TRANCADA and self.trancada_chave.get(v, -1) >= 0)

        # Na água sem ponte o bloco afunda e para: ela é destino, não caminho.
        dist = [INF] * self.N
        dist[origem] = 0
        fila = deque([origem])
        while fila:
            u = fila.popleft()
            if tipo[u] == AGUA and u != origem and not (pontes >> u) & 1 and not atravessa_agua:
                continue
            for v in self.viz[u]:
                if v >= 0 and dist[v] == INF and passa(v):
                    dist[v] = dist[u] + 1
                    fila.append(v)
        return dist

    def dist_av(self, a: int, alvo: int, nb: int, pontes: int, afundaveis: int = -1) -> list[int]:
        """Distância relaxada de cada célula até `alvo` para o avatar `a`."""
        chave = (a, alvo, nb, pontes, afundaveis)
        tabela = self._cache_av.get(chave)
        if tabela is None:
            tabela = self._tabela(alvo, self._custo_avatar(a, pontes, afundaveis=afundaveis), nb, ate=True)
            self._cache_av[chave] = tabela
        return tabela

    def dist_item(self, alvo: int, nb: int, pontes: int, afundaveis: int = -1) -> list[int]:
        """Quantas células um item ainda anda até `alvo` (cada jogada move
        no máximo UM item UMA célula)."""
        chave = (alvo, nb, pontes, afundaveis)
        tabela = self._cache_item.get(chave)
        if tabela is None:
            tabela = self._tabela(alvo, self._custo_item(pontes, afundaveis=afundaveis), nb, ate=True)
            self._cache_item[chave] = tabela
        return tabela

    def dist_bloco(self, alvo: int, nb: int | None = None, pontes: int = 0) -> list[int]:
        """Empurrões que um bloco precisa para chegar a `alvo` (relaxado:
        sem olhar quem empurra). Um bloco que entra em água sem ponte
        afunda; para passar por ela, OUTRO bloco tem de virar ponte antes:
        cada célula de água no caminho gasta um dos outros `nb - 1` blocos."""
        if nb is None:
            nb = self.total_blocos
        outros = max(0, nb - 1)
        chave = (alvo, outros, pontes)
        tabela = self._dist_bloco.get(chave)
        if tabela is None:
            tipo = self.tipo

            def custo(v):
                t = tipo[v]
                if t == AGUA:
                    return 0 if (pontes >> v) & 1 else 1
                if t in (CHAO, PLACA, PONTE) or (t == TRANCADA and self.trancada_chave.get(v, -1) >= 0):
                    return 0
                return -1

            tabela = self._tabela(alvo, custo, outros, ate=True)
            self._dist_bloco[chave] = tabela
        return tabela

    def _alcance(self, a: int, bloqueadas=frozenset()) -> set[int]:
        dist = self._tabela(self.inicial[a], self._custo_avatar(a, 0, bloqueadas), self.total_blocos, ate=False)
        return {i for i, d in enumerate(dist) if d < INF}

    def _alcancaveis(self, bloqueadas=frozenset()) -> set[int]:
        vistas: set[int] = set()
        for a in range(self.k):
            vistas |= self._alcance(a, bloqueadas)
        return vistas

    def _portas_exclusivas(self) -> list[frozenset[int]]:
        k = self.k
        self.exclusivas = [frozenset() for _ in range(k)]
        blocos = bits(self.inicial[-3])
        while True:
            alcance = [self._alcance(a) for a in range(k)]
            novas = [set(e) for e in self.exclusivas]
            for celula, placas in self.portas:
                for pl in placas:
                    if any((self.dist_bloco(pl)[b] < INF) if self.empurra else b == pl for b in blocos):
                        continue
                    quem = [a for a in range(k) if pl in alcance[a]]
                    if not quem:
                        for e in novas:
                            e.add(celula)
                    elif len(quem) == 1 and pl not in self.viz[celula]:
                        novas[quem[0]].add(celula)
            novas_f = [frozenset(e) for e in novas]
            if novas_f == self.exclusivas:
                return novas_f
            self.exclusivas = novas_f

    def _preparar_heuristica(self, ha_blocos: bool) -> None:
        tipo = self.tipo
        # Sem a Rocha, nenhum bloco sai do lugar: não há ponte nova.
        self.empurra = "R" in self.letras
        self.ha_blocos = ha_blocos and self.empurra
        self.total_blocos = self.inicial[-3].bit_count() if self.empurra else 0
        self.trancada_sem_chave = frozenset(c for c, j in self.trancada_chave.items() if j < 0)
        self._cache_av: dict = {}
        self._cache_item: dict = {}
        self._dist_bloco: dict = {}
        self._agua_do_bloco: dict = {}
        self._memo_portas: dict = {}
        self.agua = [i for i in range(self.N) if tipo[i] == AGUA]
        self.exclusivas = self._portas_exclusivas()
        self.exclusiva_todos = frozenset.intersection(*self.exclusivas) if self.exclusivas else frozenset()
        v = self.letras.index("V") if "V" in self.letras else -1
        self.alcance_v = frozenset(self._alcance(v)) if v >= 0 else frozenset()
        # Itens que precisam viajar e para onde: a Chave Dourada até o
        # portão; cada chave colorida cuja porta é obrigatória, até a porta.
        trancadas, de_placa = self._portas_obrigatorias()
        self.necessarios = [(0, self.portao)] + [(self.trancada_chave[c], c) for c in sorted(trancadas)]
        # Portas de placa obrigatórias: (índice, célula, placas, células atrás).
        self.portas_obrigatorias = []
        livres = {i for i in range(self.N) if tipo[i] != PAREDE}
        for n, (celula, placas) in enumerate(self.portas):
            if celula in de_placa:
                atras = frozenset(livres - self._alcancaveis(frozenset({celula})) - {celula})
                self.portas_obrigatorias.append((n, celula, placas, atras))

    def _portas_obrigatorias(self) -> tuple[set[int], set[int]]:
        """Portas que toda solução precisa atravessar (ponto fixo): com a
        porta virando parede, o problema relaxado fica impossível. Devolve
        (trancadas, de placa). Para as trancadas obrigatórias, a chave
        também precisa conseguir chegar até elas."""
        trancadas: set[int] = set()
        de_placa: set[int] = set()
        locais_iniciais = self.inicial[self.k : self.k + self.m]
        candidatas = [c for c, j in self.trancada_chave.items() if j >= 0] + [c for c, _ in self.portas]
        mudou = True
        while mudou:
            mudou = False
            for celula in candidatas:
                if celula in trancadas or celula in de_placa:
                    continue
                bloqueadas = frozenset({celula})
                alcance = self._alcancaveis(bloqueadas)

                def item_chega(item: int, alvo: int) -> bool:
                    origem = locais_iniciais[item]
                    if origem not in alcance:
                        return False
                    custo = self._custo_item(0, bloqueadas)
                    return self._tabela(alvo, custo, self.total_blocos, ate=True)[origem] < INF

                possivel = item_chega(0, self.portao)
                for outra in trancadas:
                    possivel = possivel and item_chega(self.trancada_chave[outra], outra)
                if not possivel:
                    (trancadas if celula in self.trancada_chave else de_placa).add(celula)
                    mudou = True
        return trancadas, de_placa

    def h(self, s: tuple) -> int:
        """Maior de dois limites inferiores (os dois admissíveis):

        - itens: a soma do caminho que cada item necessário ainda precisa
          percorrer (cada jogada move no máximo um item uma célula), mais o
          caminho até o item mais perto se ninguém carrega nenhum (até
          alguém pegar, nenhum item necessário anda);
        - portas de placa: para uma porta obrigatória fechada, alguém tem de
          passar com todas as placas ativas, seguradas por OUTROS avatares
          (ou blocos). Soma, por avatares distintos, cada um até a sua placa
          e quem passa até a porta; mais o que falta depois da porta.
        """
        if self.objetivo:
            return 0
        k, m, N, GASTA = self.k, self.m, self.N, self.GASTA
        nb = s[-3].bit_count() if self.empurra else 0
        pontes = s[-2]
        af = self.afundaveis(s[-3], pontes)
        donos = s[k + m : k + 2 * m] if self.sem_troca else None
        total = 0
        carregado = False
        busca = INF
        for j, alvo in self.necessarios:
            loc = s[k + j]
            if loc == GASTA:
                continue
            if loc >= N:
                a = loc - N
                if donos is None:
                    d = self.dist_item(alvo, nb, pontes, af)[s[a]]
                else:
                    d = self.dist_av(a, alvo, nb, pontes, af)[s[a]]
                carregado = True
            else:
                if donos is None:
                    d = self.dist_item(alvo, nb, pontes, af)[loc]
                elif donos[j] >= 0:
                    d = self.dist_av(donos[j], alvo, nb, pontes, af)[loc]
                else:
                    d = INF
                    for a in range(k):
                        if self.dist_av(a, loc, nb, pontes, af)[s[a]] < INF:
                            d = min(d, self.dist_av(a, alvo, nb, pontes, af)[loc])
                if not carregado:
                    for a in range(k):
                        da = self.dist_av(a, loc, nb, pontes, af)[s[a]]
                        if da < busca:
                            busca = da
            if d >= INF:
                return INF
            total += d
        if not carregado:
            if busca >= INF:
                return INF
            total += busca
        if self.portas_obrigatorias:
            porta = self._h_portas(s, nb, pontes, af)
            if porta > total:
                return porta
        return total

    def _h_portas(self, s: tuple, nb: int, pontes: int, af: int) -> int:
        """Só depende das posições, da Chave Dourada, dos blocos, das pontes e
        das portas: guardado por isso (muitos estados só diferem nos outros
        itens)."""
        chave_memo = (s[: self.k + 1], s[-3], s[-2], s[-1])
        valor = self._memo_portas.get(chave_memo)
        if valor is None:
            valor = self._h_portas_calcular(s, nb, pontes, af)
            if len(self._memo_portas) > 500_000:
                self._memo_portas.clear()
            self._memo_portas[chave_memo] = valor
        return valor

    def _h_portas_calcular(self, s: tuple, nb: int, pontes: int, af: int) -> int:
        k, N = self.k, self.N
        chave = s[k]
        pos = s[:k]
        blocos, portas = s[-3], s[-1]
        melhor_total = 0
        for n, celula, placas, atras in self.portas_obrigatorias:
            if (portas >> n) & 1 or any(p in atras for p in pos):
                continue
            chave_atras = chave < N and chave in atras
            portao_atras = self.portao in atras
            if not chave_atras and not portao_atras:
                continue
            inativas = [pl for pl in placas if pl not in pos and not (blocos >> pl) & 1]
            lista_blocos = bits(blocos) if inativas and blocos else []
            melhor = INF
            for passa in range(k):
                if chave_atras:
                    # Depois da porta, alguém vai até a chave.
                    depois = self.dist_av(passa, chave, nb, pontes, af)[celula]
                else:
                    depois = 0
                if depois >= INF:
                    continue
                custo = self._papeis(inativas, passa, celula, pos, lista_blocos, nb, pontes, af)
                if custo + depois < melhor:
                    melhor = custo + depois
            if melhor < INF:
                # A chave ainda anda dali até o portão (ou da porta até ele).
                origem = chave if chave_atras else celula
                melhor += self.dist_item(self.portao, nb, pontes, af)[origem]
            if melhor > melhor_total:
                melhor_total = melhor
        return melhor_total

    def _papeis(
        self, placas: list[int], passa: int, porta: int, pos: tuple, blocos: list[int], nb: int, pontes: int, af: int
    ) -> int:
        """Menor custo para: `passa` chegar à porta e cada placa inativa
        ganhar um avatar (outro, um por placa) ou um bloco. Soma por avatar
        (avatares distintos nunca dividem jogadas). A Rocha que empurra
        blocos paga, para cada bloco, ir até ele, empurrar até a placa e
        dali seguir para o próprio destino (o máximo entre os blocos)."""
        k = self.k
        rocha = self.letras.index("R") if "R" in self.letras else -1
        melhor = INF
        destinos0 = [-1] * k
        destinos0[passa] = porta
        # Por placa: só os blocos que conseguem chegar nela.
        candidatos = {pl: [b for b in blocos if self.dist_bloco(pl, nb, pontes)[b] < INF] for pl in placas}

        def fechar(destinos: list[int], empurrar: list[tuple[int, int]]) -> int:
            total = 0
            for a in range(k):
                alvo = destinos[a]
                base = self.dist_av(a, alvo, nb, pontes, af)[pos[a]] if alvo >= 0 else 0
                if a == rocha and empurrar:
                    for b, pl in empurrar:
                        ida = self.dist_av(a, b, nb, pontes, af)[pos[a]]
                        custo = max(ida - 1, 0) + self.dist_bloco(pl, nb, pontes)[b]
                        if alvo >= 0:
                            custo += max(self.dist_av(a, alvo, nb, pontes, af)[pl] - 1, 0)
                        if custo > base:
                            base = custo
                if base >= INF:
                    return INF
                total += base
            return total

        def atribuir(i: int, destinos: list[int], livres_blocos: list[int], empurrar: list[tuple[int, int]]):
            nonlocal melhor
            if i == len(placas):
                if rocha < 0 and empurrar:
                    return
                custo = fechar(destinos, empurrar)
                if custo < melhor:
                    melhor = custo
                return
            pl = placas[i]
            for a in range(k):
                if destinos[a] < 0:
                    destinos[a] = pl
                    atribuir(i + 1, destinos, livres_blocos, empurrar)
                    destinos[a] = -1
            for b in candidatos[pl]:
                if b in livres_blocos:
                    atribuir(i + 1, destinos, [x for x in livres_blocos if x != b], empurrar + [(b, pl)])

        atribuir(0, destinos0, blocos, [])
        return melhor


def bits(mascara: int) -> list[int]:
    """Os índices dos bits ligados."""
    saida = []
    while mascara:
        baixo = mascara & -mascara
        saida.append(baixo.bit_length() - 1)
        mascara ^= baixo
    return saida


FIM = ("fim",)


def buscar(problema: Problema, orcamento: Orcamento | None = None, inicio: tuple | None = None) -> Solucao:
    """A* com fila por baldes (f inteiro); dentro do mesmo f, o mais fundo
    primeiro. A heurística é admissível mas nem sempre consistente, então o
    objetivo só vale quando sai da fila e um estado pode ser reexpandido se
    aparecer de novo com g menor. Levanta SemSolucao ou OrcamentoEsgotado."""
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
        if melhor_g[s] != g:
            continue
        if s is FIM:
            acoes = []
            while pai[s] is not None:
                s, acao = pai[s]
                acoes.append(acao)
            passos = [problema.notacao(a) for a in reversed(acoes)]
            return Solucao(len(passos), passos, explorados, time.monotonic() - comeco)
        explorados += 1
        if explorados % 4096 == 0:
            gasto = time.monotonic() - comeco
            if explorados >= orcamento.max_estados:
                raise OrcamentoEsgotado(explorados, gasto)
            if gasto >= orcamento.max_segundos:
                raise OrcamentoEsgotado(explorados, gasto, por_tempo=True)
        g1 = g + 1
        for t, acao in sucessores(s):
            if t is None:
                t, ht = FIM, 0
            else:
                ht = -1
            anterior = melhor_g.get(t)
            if anterior is not None and anterior <= g1:
                continue
            if ht < 0:
                ht = h(t)
                if ht >= INF:
                    continue
            melhor_g[t] = g1
            pai[t] = (s, acao)
            # Pathmax: o f do filho nunca fica abaixo do f do pai (com a
            # heurística inconsistente, ele cairia num balde já passado).
            ft = max(g1 + ht, f)
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


def estados_que(problema: Problema, predicado, orcamento: Orcamento | None = None, inicio: tuple | None = None):
    """Busca em largura a partir do início: gera (plano, estado) de cada
    estado alcançável que cumpre `predicado`, do mais perto ao mais longe.
    Para quando o orçamento de estados acaba (sem erro); o de tempo levanta
    OrcamentoEsgotado(por_tempo=True)."""
    orcamento = orcamento or Orcamento()
    comeco = time.monotonic()
    s0 = inicio if inicio is not None else problema.inicial
    pai: dict = {s0: None}
    fila = deque([s0])
    explorados = 0
    while fila:
        s = fila.popleft()
        explorados += 1
        if explorados >= orcamento.max_estados:
            return
        if explorados % 4096 == 0 and time.monotonic() - comeco >= orcamento.max_segundos:
            raise OrcamentoEsgotado(explorados, time.monotonic() - comeco, por_tempo=True)
        if predicado(s):
            acoes = []
            t = s
            while pai[t] is not None:
                t, acao = pai[t]
                acoes.append(acao)
            yield [problema.notacao(a) for a in reversed(acoes)], s
        for t, acao in problema.sucessores(s):
            if t is not None and t not in pai:
                pai[t] = (s, acao)
                fila.append(t)
