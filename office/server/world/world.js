// Mundo do escritório: estado autoritativo da simulação.
//
//   fonte (runtime real ou demo) → ponte (eventos de domínio) → MUNDO → clientes
//
// O mundo NÃO sabe nada de renderização. Ele avança em passos fixos
// (`step(dt)`), mantém as entidades, as reservas de lugares, o relógio, o
// último instantâneo do runtime e um registro de acontecimentos; e produz
// `view()` — o que os clientes 3D recebem.

import { NavGrid } from '../navigation/grid.js';
import { PathFinder } from '../navigation/astar.js';
import { POIS, POI_BY_ID, IMMO_DEFS } from '../../shared/layout.js';
import { Immo } from '../entities/immo.js';
import { Abiyss } from '../entities/abiyss.js';
import { RuntimeBridge } from '../dsr/bridge.js';
import { emptySnapshot, summarize } from '../dsr/snapshot.js';
import { COLORS } from '../../shared/states.js';
import { Rng } from './rng.js';
import { localParts } from './clock.js';
import { createLogger } from '../log.js';

const log = createLogger('world');
const S = 1000;
export const WORLD_STATE_VERSION = 1;

/** Conflitos de reserva: o POI, os que ele trava e os que travam ele. */
const CONFLICTS = new Map(POIS.map((p) => {
  const set = new Set([p.id, ...(p.locks ?? [])]);
  for (const q of POIS) if (q.locks?.includes(p.id)) set.add(q.id);
  return [p.id, [...set]];
}));

export class World {
  /**
   * @param {object} opts
   * @param {number} opts.seed
   * @param {import('./clock.js').SimClock} opts.clock
   * @param {string} opts.timeZone
   * @param {{maxTaskChars?: number, showTaskText?: boolean}} [opts.privacy]
   */
  constructor({ seed, clock, timeZone, privacy = {} }) {
    this.seed = seed;
    this.clock = clock;
    this.timeZone = timeZone;
    this.privacy = { showTaskText: true, maxTaskChars: 90, ...privacy };
    this.rng = new Rng(seed);
    this.grid = new NavGrid();
    this.pathfinder = new PathFinder(this.grid);
    this.bridge = new RuntimeBridge({ immoIds: IMMO_DEFS.map((d) => d.id) });
    this.runtime = emptySnapshot('nenhuma');
    this.runtimeSummary = summarize(this.runtime);
    this.sourceStatus = { source: 'nenhuma', connected: false };
    this.env = null;
    this.tick = 0;
    this.reservations = new Map();
    this.fx = [];
    this.fxSeq = 0;
    this.journal = [];
    this.journalSeq = 0;
    this.phaseChangedAtSim = -1e12;
    this.lastChatSim = -1e12;
    this.stats = { violations: 0, overlaps: 0, recoveries: 0, tickMsAvg: 0, maxTickMs: 0 };
    this.lastViolation = null;

    const home = POI_BY_ID['abiyss-home'];
    this.abiyss = new Abiyss({ x: home.x, z: home.z, rng: this.rng.fork('abiyss') });
    this.reserve('abiyss-home', this.abiyss);
    this.immoList = IMMO_DEFS.map((def, index) => {
      const side = POI_BY_ID[`${def.desk}-side`];
      return new Immo({ def, index, x: side.x, z: side.z, rng: this.rng.fork(def.id) });
    });
    this.agents = [this.abiyss, ...this.immoList];
  }

  get simMs() {
    return this.clock.simMs;
  }

  get wallMs() {
    return this.clock.wallMs();
  }

  immos() {
    return this.immoList;
  }

  agent(id) {
    return this.agents.find((a) => a.id === id) ?? null;
  }

  // --- início --------------------------------------------------------------------

  /**
   * Começo "do zero": os IMMos chegam pela calçada e entram, um a um.
   * @param {{demoOpening?: boolean}} opts
   */
  startFresh({ demoOpening = false } = {}) {
    this.immoList.forEach((immo, i) => {
      immo.scheduleEntrance(this, this.simMs + (1.5 + i * 2.6) * S);
      if (demoOpening) immo.script = ['wander', 'sofa', 'desk_idle'];
    });
    // Energias iniciais variadas (um IMMo chega cansado: vai querer o sofá).
    const energies = [0.92, 0.85, 0.78, 0.32];
    this.immoList.forEach((immo, i) => { immo.energy = energies[i]; });
    this.note('info', 'escritório aberto: os IMMos estão chegando');
  }

  // --- reservas ----------------------------------------------------------------------

  canReserve(poiId, agent) {
    const conflicts = CONFLICTS.get(poiId);
    if (!conflicts) return false;
    for (const id of conflicts) {
      const owner = this.reservations.get(id);
      if (owner !== undefined && owner !== agent.id) return false;
    }
    return true;
  }

  reserve(poiId, agent) {
    if (!this.canReserve(poiId, agent)) return false;
    this.reservations.set(poiId, agent.id);
    return true;
  }

  releaseOne(poiId, agent) {
    if (this.reservations.get(poiId) === agent.id) this.reservations.delete(poiId);
  }

  releaseAll(agent, except = null) {
    for (const [poi, owner] of [...this.reservations]) {
      if (owner === agent.id && poi !== except) this.reservations.delete(poi);
    }
  }

  // --- runtime -------------------------------------------------------------------------

  /** Aplica um instantâneo da fonte (runtime real ou demo). */
  ingest(snapshot, sourceStatus) {
    const events = this.bridge.ingest(snapshot);
    if (snapshot.ok) {
      const prevFase = this.runtime.ritmo.fase;
      this.runtime = snapshot;
      if (prevFase !== snapshot.ritmo.fase) this.phaseChangedAtSim = this.simMs;
    } else {
      // Mantém o último estado bom, mas marca a fonte como com erro.
      this.runtime = { ...this.runtime, ok: false, error: snapshot.error };
    }
    this.runtimeSummary = summarize(this.runtime);
    if (sourceStatus) this.sourceStatus = sourceStatus;
    for (const ev of events) this.#onEvent(ev);
    return events;
  }

  #onEvent(ev) {
    this.abiyss.onRuntimeEvent(this, ev);
    switch (ev.type) {
      case 'subagente.atribuido': {
        if (ev.bootstrap) break;
        const immo = this.agent(ev.immoId);
        this.addFx({ type: 'orb', from: 'abiyss', to: ev.immoId, color: COLORS.nivel[ev.nivel] ?? COLORS.abiyss, durationMs: 2200 });
        if (immo) this.note('info', `Abiyss delegou o sub-agente #${ev.subId} (${ev.nivel}) → ${immo.name}`);
        break;
      }
      case 'subagente.criado':
        if (ev.sub.estado === 'pendente') this.note('debug', `sub-agente #${ev.sub.id} (${ev.sub.nivel}) na fila: ${this.taskText(ev.sub)}`);
        break;
      case 'ciclo':
        if (ev.ciclo.chamouModelo) {
          this.addFx({ type: 'pulse', from: 'abiyss', color: COLORS.abiyss, durationMs: 1600 });
          this.note('info', `heartbeat #${ev.ciclo.id}: ${ev.ciclo.motivo}${ev.ciclo.resultado ? ` → ${ev.ciclo.resultado.split('\n').join('; ')}` : ''}`);
        }
        break;
      case 'evento':
        if (ev.evento.tipo === 'cron') {
          this.addFx({ type: 'chime', from: 'abiyss', color: 0xffe08a, durationMs: 1800 });
          this.note('info', `cron disparou: ${ev.evento.origem}`);
        }
        break;
      case 'daemon':
        this.note(ev.para === 'rodando' ? 'info' : 'warn', `daemon do runtime: ${ev.de ?? '—'} → ${ev.para}`);
        break;
      case 'fase':
        this.note('info', `fase do ritmo: ${ev.de ?? '—'} → ${ev.para}`);
        break;
      case 'foco':
        if (ev.goal) this.note('info', `goal em foco: #${ev.goal.id} ${ev.goal.titulo} (${ev.goal.estado})`);
        break;
      case 'fonte.erro':
        this.note('warn', `fonte do runtime com erro: ${ev.error}`);
        break;
      case 'fonte.conectada':
        this.note('info', 'fonte do runtime conectada');
        break;
      case 'subagente.sumiu':
        this.note('warn', `sub-agente #${ev.subId} saiu da janela de leitura; ${ev.immoId} liberado`);
        break;
      default:
        break;
    }
  }

  /** IMMo chegou ao console com o relatório. */
  onReport(immo, task) {
    this.abiyss.onReport(this, immo, task);
    const color = { concluido: COLORS.concluido, falhou: COLORS.falhou, expirado: COLORS.expirado }[task.estado] ?? COLORS.reporting;
    this.addFx({ type: 'report', from: immo.id, to: 'abiyss', color, durationMs: 2000 });
    const r = task.relatorio;
    this.note('info', `${immo.name} entregou o relatório do sub-agente #${task.id}: ${task.estado}${r ? ` (${r.status}, confiança ${Math.round((r.confianca ?? 0) * 100)}%)` : ''}`);
  }

  taskText(task) {
    if (!this.privacy.showTaskText) return '(tarefa oculta)';
    const t = task.tarefa ?? '';
    return t.length > this.privacy.maxTaskChars ? `${t.slice(0, this.privacy.maxTaskChars - 1)}…` : t;
  }

  // --- efeitos e registro --------------------------------------------------------------

  addFx(fx) {
    this.fx.push({ id: ++this.fxSeq, startSim: this.simMs, ...fx });
  }

  note(level, text) {
    if (level !== 'debug') {
      this.journal.push({ seq: ++this.journalSeq, simMs: this.simMs, wallMs: this.wallMs, level, text });
      if (this.journal.length > 120) this.journal.shift();
    }
    log[level === 'warn' ? 'warn' : level === 'debug' ? 'debug' : 'info'](text);
  }

  // --- passo da simulação -----------------------------------------------------------------

  step(dt) {
    const t0 = performance.now();
    this.clock.advance(dt * S);
    this.tick++;
    for (const a of this.agents) {
      try {
        a.think(this, dt);
        if (a.present) a.run(this, dt);
      } catch (e) {
        // Um erro num agente não derruba o mundo: ele recomeça parado.
        log.error(`erro no agente ${a.id}; replanejando`, { erro: e.message, stack: e.stack });
        a.plan = [];
        a.stepIdx = 0;
        a.step = null;
        a.mover.clearPath();
      }
    }
    this.#checkInvariants();
    this.fx = this.fx.filter((f) => this.simMs - f.startSim < f.durationMs);
    const ms = performance.now() - t0;
    this.stats.tickMsAvg = this.stats.tickMsAvg * 0.98 + ms * 0.02;
    this.stats.maxTickMs = Math.max(this.stats.maxTickMs, ms);
  }

  #checkInvariants() {
    const present = this.agents.filter((a) => a.present);
    for (const a of present) {
      const v = a.checkPlacement(this);
      if (v) {
        this.stats.violations++;
        this.lastViolation = { agent: a.id, problem: v, x: a.x, z: a.z, tick: this.tick };
        if (this.stats.violations <= 5) log.error(`invariante violada: ${a.name} em (${a.x.toFixed(2)}, ${a.z.toFixed(2)}): ${v}`);
        if (v === 'parede/móvel/fora') {
          // Recuperação: volta para a célula livre mais próxima.
          const i = this.grid.nearestFree(a.x, a.z, a.allowedMask, 2);
          if (i >= 0) {
            const c = this.grid.centerOf(i);
            a.mover.x = c.x;
            a.mover.z = c.z;
            a.mover.clearPath();
            a.plan = [];
            a.stepIdx = 0;
            a.step = null;
            this.stats.recoveries++;
          }
        }
      }
    }
    for (let i = 0; i < present.length; i++) {
      for (let j = i + 1; j < present.length; j++) {
        const a = present[i];
        const b = present[j];
        if (!a.solid || !b.solid) continue;
        if (Math.hypot(a.x - b.x, a.z - b.z) < 0.3) {
          this.stats.overlaps++;
          this.lastOverlap = { a: a.id, b: b.id, aAct: a.activity, bAct: b.activity, x: a.x, z: a.z, tick: this.tick };
          log.debug('sobreposição', this.lastOverlap);
        }
      }
    }
  }

  // --- visão para os clientes --------------------------------------------------------------

  view() {
    const local = localParts(this.wallMs, this.timeZone);
    const entities = this.agents.map((a) => {
      const v = a.view();
      if (a.kind === 'immo') {
        const task = this.bridge.taskOf(a.id);
        v.logical = task
          ? { subagenteId: task.id, estado: task.estado, nivel: task.nivel, tarefa: this.taskText(task), relatorio: task.relatorio?.status ?? null, goalId: task.goalId }
          : { subagenteId: null, estado: null, nivel: null, tarefa: null };
      } else {
        v.logical = {
          daemon: this.runtime.daemon.estado,
          fase: this.runtime.ritmo.fase,
          sono: this.runtime.sono.ativo,
          eventosPendentes: this.runtime.eventos.pendentes,
          goalFoco: this.runtimeSummary.goalFoco,
        };
      }
      return v;
    });
    return {
      v: 1,
      tick: this.tick,
      simMs: Math.round(this.simMs),
      wallMs: Math.round(this.wallMs),
      clock: { local: `${local.hhmm}:${String(local.second).padStart(2, '0')}`, iso: local.iso, tz: this.timeZone, real: this.clock.isReal, hours: local.hours },
      entities,
      runtime: { ...this.runtimeSummary, status: this.sourceStatus, overflow: [...this.bridge.overflow] },
      fx: this.fx.map((f) => ({ ...f, age: Math.round(this.simMs - f.startSim) })),
      journal: this.journal.slice(-14),
      env: this.env,
      stats: {
        entities: this.agents.filter((a) => a.present).length,
        tickMsAvg: Math.round(this.stats.tickMsAvg * 1000) / 1000,
        violations: this.stats.violations,
        overlaps: this.stats.overlaps,
        recoveries: this.stats.recoveries,
        replans: this.agents.reduce((s, a) => s + a.stats.replans, 0),
        stuck: this.agents.reduce((s, a) => s + a.stats.stuck, 0),
        reservations: Object.fromEntries(this.reservations),
      },
    };
  }

  // --- persistência --------------------------------------------------------------------------

  toJSON() {
    return {
      version: WORLD_STATE_VERSION,
      savedAtMs: Date.now(),
      seed: this.seed,
      simMs: this.simMs,
      tick: this.tick,
      bridge: this.bridge.toJSON(),
      agents: this.agents.map((a) => ({
        id: a.id, x: a.x, z: a.z, heading: a.mover.heading, present: a.present,
        energy: a.energy ?? null, lastFree: a.lastFree ?? null,
      })),
    };
  }

  /**
   * Restaura posições e atribuições salvas. Devolve true se usou o estado.
   * Planos não são salvos: cada entidade decide de novo a partir de onde está.
   */
  restore(data, sourceKey) {
    if (!data || data.version !== WORLD_STATE_VERSION) return false;
    for (const s of data.agents ?? []) {
      const a = this.agent(s.id);
      if (!a || !Number.isFinite(s.x) || !Number.isFinite(s.z)) continue;
      let { x, z } = s;
      // Fora da área livre ou dentro de assento: vai para a célula livre mais próxima.
      if (!this.grid.isFreeAt(x, z, 0)) {
        const i = this.grid.nearestFree(x, z, 0, 3);
        if (i < 0) continue;
        ({ x, z } = this.grid.centerOf(i));
      }
      a.mover.x = x;
      a.mover.z = z;
      a.mover.heading = Number.isFinite(s.heading) ? s.heading : 0;
      a.present = true;
      a.poi = null;
      if (a.kind === 'immo') {
        if (Number.isFinite(s.energy)) a.energy = Math.min(1, Math.max(0, s.energy));
        a.lastFree = s.lastFree ?? null;
        a.mode = 'free';
      }
    }
    this.releaseAll(this.abiyss);
    const restoredBridge = this.bridge.restore(data.bridge, sourceKey);
    this.note('info', `estado anterior restaurado${restoredBridge ? ' (com atribuições de sub-agentes)' : ''}`);
    return true;
  }
}
