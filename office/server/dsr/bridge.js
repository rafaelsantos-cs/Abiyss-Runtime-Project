// Ponte runtime → escritório.
//
// Recebe instantâneos normalizados (snapshot.js), compara com o anterior e
// produz EVENTOS DE DOMÍNIO ("sub-agente 12 começou", "ciclo 40 terminou"...).
// Também decide, de forma determinística, QUAL IMMo representa cada
// sub-agente em execução.
//
// Mapeamento IMMo ↔ sub-agente:
// - o runtime executa no máximo `[subagentes] max_simultaneos` sub-agentes
//   (padrão 4); o escritório tem 4 IMMos, um por mesa — cada IMMo é uma
//   "vaga" do executor de sub-agentes;
// - um sub-agente que passa a `executando` é atribuído ao IMMo livre que
//   está há mais tempo sem tarefa (empate: menor índice);
// - o IMMo mantém o sub-agente até o mundo confirmar que ele terminou de
//   representá-lo (relatório entregue) — `release()`;
// - sobrando sub-agentes sem IMMo (max_simultaneos > 4), eles ficam em
//   `overflow` e aparecem no painel.

import { isFinalSubagente } from '../../shared/states.js';
import { createLogger } from '../log.js';

const log = createLogger('dsr:bridge');

export class RuntimeBridge {
  /** @param {{immoIds: string[]}} opts */
  constructor({ immoIds }) {
    this.immoIds = [...immoIds];
    this.reset(null);
  }

  reset(sourceKey) {
    this.sourceKey = sourceKey;
    this.bootstrapped = false;
    /** id → último registro visto */
    this.subs = new Map();
    /** immoId → id do sub-agente que ele representa */
    this.immoSub = new Map();
    /** immoId → número de sequência da última atribuição */
    this.lastAssigned = new Map(this.immoIds.map((id) => [id, 0]));
    this.seq = 0;
    this.overflow = [];
    this.lastCicloId = 0;
    this.lastEventoId = 0;
    this.prev = { daemon: null, fase: null, focoId: undefined, conversaMs: null, ok: null };
    this.lastSnapshot = null;
  }

  /** Sub-agente atual do IMMo (registro mais recente) ou null. */
  taskOf(immoId) {
    const id = this.immoSub.get(immoId);
    if (id === undefined) return null;
    return this.subs.get(id) ?? null;
  }

  /** O mundo terminou de representar o sub-agente (ex.: relatório entregue). */
  release(immoId, subId) {
    if (this.immoSub.get(immoId) === subId) this.immoSub.delete(immoId);
  }

  #assign(sub, events, bootstrap = false) {
    // Já atribuído?
    for (const [immo, id] of this.immoSub) if (id === sub.id) return immo;
    const candidates = this.immoIds.filter((immo) => {
      const cur = this.taskOf(immo);
      return !cur || isFinalSubagente(cur.estado);
    });
    if (candidates.length === 0) {
      if (!this.overflow.includes(sub.id)) this.overflow.push(sub.id);
      return null;
    }
    candidates.sort((a, b) => {
      const fa = this.immoSub.has(a) ? 1 : 0;
      const fb = this.immoSub.has(b) ? 1 : 0;
      if (fa !== fb) return fa - fb;
      const la = this.lastAssigned.get(a);
      const lb = this.lastAssigned.get(b);
      if (la !== lb) return la - lb;
      return this.immoIds.indexOf(a) - this.immoIds.indexOf(b);
    });
    const immo = candidates[0];
    const previous = this.taskOf(immo);
    if (previous) events.push({ type: 'relatorio.substituido', immoId: immo, subId: previous.id });
    this.immoSub.set(immo, sub.id);
    this.lastAssigned.set(immo, ++this.seq);
    this.overflow = this.overflow.filter((id) => id !== sub.id);
    events.push({ type: 'subagente.atribuido', immoId: immo, subId: sub.id, nivel: sub.nivel, bootstrap });
    return immo;
  }

  /**
   * Processa um instantâneo. Devolve a lista de eventos de domínio.
   * @param {ReturnType<import('./snapshot.js').emptySnapshot>} snap
   */
  ingest(snap) {
    const events = [];
    if (this.prev.ok !== snap.ok) {
      events.push({ type: snap.ok ? 'fonte.conectada' : 'fonte.erro', error: snap.error });
      this.prev.ok = snap.ok;
    }
    if (!snap.ok) return events; // mantém o último estado conhecido
    this.lastSnapshot = snap;

    const subsAsc = [...snap.subagentes].sort((a, b) => a.id - b.id);
    const seen = new Set();

    if (!this.bootstrapped) {
      this.lastCicloId = Math.max(0, ...snap.ciclos.map((c) => c.id));
      this.lastEventoId = Math.max(0, ...snap.eventos.recentes.map((e) => e.id));
      for (const s of subsAsc) {
        seen.add(s.id);
        const prev = this.subs.get(s.id);
        this.subs.set(s.id, s);
        // Estado salvo de uma execução anterior do escritório: mantém.
        if (prev) continue;
        if (s.estado === 'executando') this.#assign(s, events, true);
      }
      // Atribuições restauradas cujo sub-agente sumiu ou terminou há muito: solta.
      // Terminados há mais de 1 min (com o escritório desligado) não viram relatório.
      for (const [immo, id] of [...this.immoSub]) {
        const s = this.subs.get(id);
        const velho = s && isFinalSubagente(s.estado) && (s.terminadoMs ?? 0) < snap.capturedAtMs - 60_000;
        if (!s || !seen.has(id) || velho) this.immoSub.delete(immo);
      }
      this.bootstrapped = true;
      events.push({ type: 'fonte.sincronizada', subagentes: subsAsc.length });
      log.info('primeiro instantâneo do runtime', {
        fonte: snap.source, subagentes: subsAsc.length, ativos: subsAsc.filter((s) => !isFinalSubagente(s.estado)).length,
      });
    } else {
      for (const s of subsAsc) {
        seen.add(s.id);
        const prev = this.subs.get(s.id);
        this.subs.set(s.id, s);
        if (!prev) {
          events.push({ type: 'subagente.criado', sub: s });
          if (s.estado === 'executando') {
            events.push({ type: 'subagente.iniciado', sub: s });
            this.#assign(s, events);
          } else if (isFinalSubagente(s.estado)) {
            // Ciclo de vida inteiro entre duas leituras (sub-agente muito rápido).
            if (s.estado !== 'cancelado') this.#assign(s, events);
            events.push({ type: 'subagente.terminado', sub: s, immoId: this.#immoOf(s.id) });
          }
          continue;
        }
        if (prev.estado === s.estado) continue;
        if (s.estado === 'executando') {
          events.push({ type: 'subagente.iniciado', sub: s });
          this.#assign(s, events);
        } else if (isFinalSubagente(s.estado)) {
          events.push({ type: 'subagente.terminado', sub: s, immoId: this.#immoOf(s.id) });
          this.overflow = this.overflow.filter((id) => id !== s.id);
        }
      }
      // Sub-agentes atribuídos que saíram da janela de leitura.
      for (const [immo, id] of [...this.immoSub]) {
        if (!seen.has(id)) {
          const last = this.subs.get(id);
          if (last && !isFinalSubagente(last.estado)) {
            events.push({ type: 'subagente.sumiu', immoId: immo, subId: id });
          }
          if (!last || !isFinalSubagente(last.estado)) this.immoSub.delete(immo);
        }
      }
      // Tenta de novo quem ficou sem IMMo.
      for (const id of [...this.overflow]) {
        const s = this.subs.get(id);
        if (!s || s.estado !== 'executando') {
          this.overflow = this.overflow.filter((x) => x !== id);
          continue;
        }
        this.#assign(s, events);
      }
    }

    // Limpa registros antigos fora da janela (mantém os atribuídos).
    const keep = new Set(this.immoSub.values());
    for (const id of [...this.subs.keys()]) if (!seen.has(id) && !keep.has(id)) this.subs.delete(id);

    // Ciclos do heartbeat e eventos da fila (só os novos).
    for (const c of [...snap.ciclos].sort((a, b) => a.id - b.id)) {
      if (c.id > this.lastCicloId) {
        events.push({ type: 'ciclo', ciclo: c });
        this.lastCicloId = c.id;
      }
    }
    for (const e of [...snap.eventos.recentes].sort((a, b) => a.id - b.id)) {
      if (e.id > this.lastEventoId) {
        events.push({ type: 'evento', evento: e });
        this.lastEventoId = e.id;
      }
    }

    if (this.prev.daemon !== snap.daemon.estado) {
      events.push({ type: 'daemon', de: this.prev.daemon, para: snap.daemon.estado });
      this.prev.daemon = snap.daemon.estado;
    }
    if (this.prev.fase !== snap.ritmo.fase) {
      events.push({ type: 'fase', de: this.prev.fase, para: snap.ritmo.fase });
      this.prev.fase = snap.ritmo.fase;
    }
    const focoId = snap.goals.foco?.id ?? null;
    if (this.prev.focoId !== focoId) {
      events.push({ type: 'foco', goal: snap.goals.foco });
      this.prev.focoId = focoId;
    }
    const conv = snap.conversa.ultimaChamadaMs;
    if (conv !== null && conv !== this.prev.conversaMs) {
      if (this.prev.conversaMs !== null) events.push({ type: 'conversa', ms: conv });
      this.prev.conversaMs = conv;
    }
    return events;
  }

  #immoOf(subId) {
    for (const [immo, id] of this.immoSub) if (id === subId) return immo;
    return null;
  }

  /** Estado persistível (data/office-state.json). */
  toJSON() {
    return {
      sourceKey: this.sourceKey,
      subs: [...this.subs.values()],
      immoSub: [...this.immoSub],
      lastAssigned: [...this.lastAssigned],
      seq: this.seq,
      lastCicloId: this.lastCicloId,
      lastEventoId: this.lastEventoId,
    };
  }

  /** Restaura se o estado salvo é da mesma fonte; senão começa do zero. */
  restore(data, sourceKey) {
    this.reset(sourceKey);
    if (!data || data.sourceKey !== sourceKey) return false;
    for (const s of data.subs ?? []) this.subs.set(s.id, s);
    for (const [immo, id] of data.immoSub ?? []) if (this.immoIds.includes(immo)) this.immoSub.set(immo, id);
    for (const [immo, n] of data.lastAssigned ?? []) if (this.immoIds.includes(immo)) this.lastAssigned.set(immo, n);
    this.seq = data.seq ?? 0;
    // Os cursores de ciclo/evento são recalculados no primeiro instantâneo
    // (não reencena o que aconteceu com o escritório desligado).
    return true;
  }
}
