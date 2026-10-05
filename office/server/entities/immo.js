// IMMo: trabalhador autônomo do escritório.
//
// ESTADO LÓGICO (vem do runtime, via ponte): o sub-agente que o IMMo
// representa e o `estado` dele (pendente/executando/concluido/...).
//
// MODO (derivado do estado lógico, por código determinístico):
//   work   ← sub-agente `executando`  → vai à mesa, senta, trabalha
//   report ← concluido/falhou/expirado → leva o relatório ao Abiyss
//   cancel ← cancelado                 → para, levanta, fica livre
//   free   ← sem sub-agente            → comportamento autônomo
//   enter  ← primeira entrada pela porta (início "do zero")
//
// No modo livre, a escolha da atividade é ponderada por: fase do ritmo
// (vigília/descanso/sono), energia (cansa trabalhando, recupera
// descansando), temperatura real (frio → café; calor → água), variedade
// (evita repetir a última atividade) e anti-sincronia (evita que todos
// façam a mesma coisa). Tudo com gerador semeado: reprodutível.

import { Agent } from './agent.js';
import { POI_BY_ID, POIS } from '../../shared/layout.js';
import { isFinalSubagente } from '../../shared/states.js';

const S = 1000;

const SLEEP_SPOTS = ['sofa-a-lie', 'sofa-b-lie', 'beanbag-1', 'beanbag-2'];
const SOFA_SEATS = ['sofa-a-1', 'sofa-a-2', 'sofa-a-3', 'sofa-b-1', 'sofa-b-2'];
const WINDOWS = POIS.filter((p) => p.kind === 'window').map((p) => p.id);
const WANDER = POIS.filter((p) => p.kind === 'wander').map((p) => p.id);
const STOOLS = ['stool-1', 'stool-2'];

export class Immo extends Agent {
  /**
   * @param {{def: object, index: number, x: number, z: number, rng: import('../world/rng.js').Rng}} opts
   */
  constructor({ def, index, x, z, rng }) {
    super({ id: def.id, kind: 'immo', name: def.name, x, z, maxSpeed: rng.range(1.0, 1.3), priority: 10 - index, rng });
    this.def = def;
    this.index = index;
    this.desk = def.desk;
    this.energy = rng.range(0.55, 0.95);
    this.mode = 'free';
    this.taskId = null;
    this.spawnAtMs = null;
    /** Roteiro de abertura da demonstração (atividades livres forçadas). */
    this.script = [];
    this.lastFree = null;
  }

  /** Entra pela porta a partir de `atMs` (tempo de simulação). */
  scheduleEntrance(world, atMs) {
    this.present = false;
    const out = POI_BY_ID.outside;
    this.mover.x = out.x;
    this.mover.z = out.z;
    this.mover.heading = Math.PI;
    this.spawnAtMs = atMs;
    this.mode = 'enter';
  }

  // --- ciclo -----------------------------------------------------------------

  think(world, dt) {
    if (!this.present) {
      if (world.simMs >= this.spawnAtMs) {
        this.present = true;
        world.note('info', `${this.name} chegou ao escritório`);
        const side = `${this.desk}-side`;
        this.startPlan(world, 'entrar no escritório', [
          { type: 'goto', poi: 'entrance', activity: 'entering' },
          { type: 'goto', poi: side, activity: 'walking' },
          { type: 'face', poi: side },
          { type: 'stay', activity: 'idle', duration: 2 + this.rng.next() * 2 },
        ], [side]);
      }
      return;
    }
    this.#updateEnergy(dt);

    const task = world.bridge.taskOf(this.id);
    let desired = 'free';
    if (task && task.estado === 'executando') desired = 'work';
    else if (task && isFinalSubagente(task.estado)) desired = task.estado === 'cancelado' ? 'cancel' : 'report';

    const sameTask = task && this.taskId === task.id;
    if (desired === 'work' && (this.mode !== 'work' || !sameTask || this.idle)) this.#planWork(world, task);
    else if (desired === 'report' && (this.mode !== 'report' || !sameTask || this.idle)) this.#planReport(world, task);
    else if (desired === 'cancel' && (this.mode !== 'cancel' || !sameTask)) this.#planCancel(world, task);
    else if (desired === 'free' && ['work', 'report', 'cancel'].includes(this.mode)) {
      this.mode = 'free';
      this.taskId = null;
      this.#decideFree(world);
    } else if (this.idle) {
      this.mode = this.mode === 'enter' ? 'free' : this.mode;
      if (this.mode === 'free') this.#decideFree(world);
    }
  }

  #updateEnergy(dt) {
    const a = this.activity;
    let rate = -1 / 5400; // acordado e ocioso: ~1,5 h para esvaziar
    if (a === 'working') rate = -1 / 1500;
    else if (a === 'sleeping') rate = 1 / 150;
    else if (a === 'resting') rate = 1 / 260;
    else if (a === 'coffee' || a === 'water') rate = 1 / 400;
    this.energy = Math.min(1, Math.max(0, this.energy + rate * dt));
  }

  // --- trabalho e relatório ------------------------------------------------------

  #planWork(world, task) {
    const ok = this.startPlan(world, `trabalhar no sub-agente #${task.id} (${task.nivel})`, [
      { type: 'stand' },
      { type: 'goto', poi: this.desk, activity: 'walking' },
      { type: 'pose', poi: this.desk, pose: 'sit' },
      {
        type: 'stay', activity: 'working', yieldable: false,
        until: (w) => {
          const t = w.bridge.taskOf(this.id);
          return !t || t.id !== task.id || t.estado !== 'executando';
        },
      },
    ], [this.desk]);
    if (!ok) return;
    if (this.mode !== 'work' || this.taskId !== task.id) {
      world.note('info', `${this.name} assumiu o sub-agente #${task.id} (${task.nivel}): ${world.taskText(task)}`);
    }
    this.mode = 'work';
    this.taskId = task.id;
  }

  #planReport(world, task) {
    const changed = this.mode !== 'report' || this.taskId !== task.id;
    this.mode = 'report';
    this.taskId = task.id;
    if (changed) world.note('info', `${this.name} terminou o sub-agente #${task.id}: ${task.estado}`);
    const release = { type: 'call', fn: (w) => { w.bridge.release(this.id, task.id); } };
    if (world.canReserve('report', this)) {
      this.startPlan(world, `entregar o relatório do sub-agente #${task.id}`, [
        { type: 'stand' },
        { type: 'goto', poi: 'report', activity: 'walking' },
        { type: 'face', poi: 'report' },
        {
          type: 'stay', activity: 'reporting', duration: 4.5, yieldable: false,
          onStart: (w) => w.onReport(this, task),
        },
        release,
      ], ['report']);
    } else if (world.canReserve('report-wait', this)) {
      this.startPlan(world, 'esperar para entregar o relatório', [
        { type: 'stand' },
        { type: 'goto', poi: 'report-wait', activity: 'walking' },
        { type: 'face', poi: 'report-wait' },
        { type: 'stay', activity: 'waiting', yieldable: false, until: (w) => w.canReserve('report', this), duration: 40 },
      ], ['report-wait']);
    } else {
      // Fila cheia: entrega "pela rede" (o relatório já está na fila do runtime).
      this.startPlan(world, 'relatório enviado direto', [{ type: 'stay', activity: 'idle', duration: 1 }, release], []);
    }
  }

  #planCancel(world, task) {
    this.mode = 'cancel';
    this.taskId = task.id;
    world.note('info', `${this.name}: sub-agente #${task.id} cancelado`);
    this.startPlan(world, `sub-agente #${task.id} cancelado`, [
      { type: 'stand' },
      { type: 'stay', activity: 'idle', duration: 2.5, yieldable: false },
      { type: 'call', fn: (w) => w.bridge.release(this.id, task.id) },
    ], []);
  }

  // --- comportamento livre ----------------------------------------------------------

  #freePoi(world, ids) {
    const free = ids.filter((id) => world.canReserve(id, this));
    return free.length ? this.rng.pick(free) : null;
  }

  #decideFree(world) {
    const rt = world.runtime;
    const fase = rt.ritmo.fase;
    const offline = !rt.ok || rt.daemon.estado !== 'rodando';
    const temp = world.env?.weather?.temperatureC ?? 23;
    const tired = 1 - this.energy;
    const others = world.immos().filter((o) => o !== this && o.present);
    const doing = (act) => others.filter((o) => o.lastFree === act && o.mode === 'free').length;

    let key = null;
    if (this.script.length && world.simMs < 300 * S) key = this.script.shift();

    if (!key) {
      let w;
      if (fase === 'sono') w = { sleep: 100, sofa: 5 };
      else if (fase === 'descanso') w = { sleep: 40 + 40 * tired, sofa: 18, window: 8, coffee: 4, wander: 5, desk_idle: 5, board: 2 };
      else if (offline) w = { sleep: 15 + 30 * tired, sofa: 30, desk_idle: 15, window: 10, wander: 10, coffee: 8 };
      else {
        w = {
          desk_idle: 14, desk_side: 5, wander: 13, coffee: 9 + (temp < 18 ? 10 : 0), water: 5 + (temp > 27 ? 10 : 0),
          sofa: 10 + 35 * tired, beanbag: 3 + 6 * tired, window: 8, chat: 7, board: 6, printer: 3, shelf: 3, stool: 5,
          sleep: this.energy < 0.15 ? 25 : 0,
        };
        if (this.lastFree === 'sofa' || this.lastFree === 'sleep' || this.lastFree === 'beanbag') w.desk_idle *= 2.5;
      }
      const items = Object.entries(w).map(([value, weight]) => {
        let wt = weight;
        if (value === this.lastFree) wt *= 0.3;
        wt *= Math.pow(0.5, doing(value));
        return { value, weight: wt };
      });
      // Tenta em ordem sorteada até achar uma atividade possível.
      for (let tries = 0; tries < 6 && !key; tries++) {
        const pick = this.rng.weighted(items);
        if (pick && this.#canDo(world, pick)) key = pick;
        else items.forEach((it) => { if (it.value === pick) it.weight = 0; });
      }
      key ??= 'desk_side';
    }
    if (!this.#planFree(world, key)) {
      // Plano impossível agora (ex.: lugar reservado): fica um pouco parado.
      this.startPlan(world, 'ficar à toa', [{ type: 'stay', activity: 'idle', duration: 3 + this.rng.next() * 4 }], []);
      key = 'idle';
    }
    this.lastFree = key;
  }

  #canDo(world, key) {
    switch (key) {
      case 'desk_idle': return world.canReserve(this.desk, this);
      case 'desk_side': return world.canReserve(`${this.desk}-side`, this);
      case 'coffee': return world.canReserve('coffee', this);
      case 'water': return world.canReserve('water', this);
      case 'board': return world.canReserve('board', this);
      case 'printer': return world.canReserve('printer', this);
      case 'shelf': return world.canReserve('shelf', this);
      case 'sofa': return SOFA_SEATS.some((id) => world.canReserve(id, this));
      case 'beanbag': return ['beanbag-1', 'beanbag-2'].some((id) => world.canReserve(id, this));
      case 'sleep': return SLEEP_SPOTS.some((id) => world.canReserve(id, this));
      case 'window': return WINDOWS.some((id) => world.canReserve(id, this));
      case 'stool': return STOOLS.some((id) => world.canReserve(id, this));
      case 'chat': return world.simMs - world.lastChatSim > 120 * S && !!this.#chatPartner(world) && world.canReserve('chat-1', this) && world.canReserve('chat-2', this);
      case 'wander': return true;
      default: return false;
    }
  }

  #chatPartner(world) {
    const cands = world.immos().filter((o) => o !== this && o.present && o.mode === 'free'
      && !['sleeping', 'chatting'].includes(o.activity) && o.pose === 'stand' && o.intent !== 'conversar');
    return cands.length ? this.rng.pick(cands) : null;
  }

  #seated(world, intent, poi, pose, activity, duration, until) {
    return this.startPlan(world, intent, [
      { type: 'stand' },
      { type: 'goto', poi, activity: 'walking' },
      { type: 'pose', poi, pose },
      { type: 'stay', activity, duration, until },
      { type: 'stand' },
    ], [poi]);
  }

  #standing(world, intent, poi, activity, duration) {
    return this.startPlan(world, intent, [
      { type: 'stand' },
      { type: 'goto', poi, activity: 'walking' },
      { type: 'face', poi },
      { type: 'stay', activity, duration, facing: POI_BY_ID[poi].facing },
    ], [poi]);
  }

  #planFree(world, key) {
    const r = (a, b) => a + this.rng.next() * (b - a);
    switch (key) {
      case 'desk_idle':
        return this.#seated(world, 'ficar na mesa (ocioso)', this.desk, 'sit', 'seated_idle', r(20, 45));
      case 'desk_side':
        return this.#standing(world, 'ficar perto da mesa', `${this.desk}-side`, 'idle', r(8, 18));
      case 'coffee':
        return this.#standing(world, 'tomar café', 'coffee', 'coffee', r(12, 25));
      case 'water':
        return this.#standing(world, 'beber água', 'water', 'water', r(8, 14));
      case 'board':
        return this.#standing(world, 'ler o quadro de goals', 'board', 'reading_board', r(8, 15));
      case 'printer':
        return this.#standing(world, 'buscar impressão', 'printer', 'idle', r(6, 10));
      case 'shelf':
        return this.#standing(world, 'procurar algo na estante', 'shelf', 'idle', r(6, 12));
      case 'window': {
        const poi = this.#freePoi(world, WINDOWS);
        return !!poi && this.#standing(world, 'olhar a janela', poi, 'window', r(12, 25));
      }
      case 'sofa': {
        const poi = this.#freePoi(world, SOFA_SEATS);
        return !!poi && this.#seated(world, 'descansar no sofá', poi, 'sit', 'resting', r(35, 90));
      }
      case 'beanbag': {
        const poi = this.#freePoi(world, ['beanbag-1', 'beanbag-2']);
        return !!poi && this.#seated(world, 'descansar no pufe', poi, 'curl', 'resting', r(30, 70));
      }
      case 'stool': {
        const poi = this.#freePoi(world, STOOLS);
        return !!poi && this.#seated(world, 'pausa na copa', poi, 'sit', 'coffee', r(15, 30));
      }
      case 'sleep': {
        const poi = this.#freePoi(world, SLEEP_SPOTS);
        if (!poi) return false;
        const pose = POI_BY_ID[poi].pose;
        const nap = world.runtime.ritmo.fase === 'vigília';
        const wake = r(0, 90);
        const until = nap ? undefined : (w) => w.runtime.ritmo.fase === 'vigília' && w.simMs - w.phaseChangedAtSim > wake * S;
        return this.#seated(world, nap ? 'cochilar' : 'dormir', poi, pose, 'sleeping', nap ? r(40, 80) : 6 * 3600, until);
      }
      case 'chat': {
        const partner = this.#chatPartner(world);
        if (!partner) return false;
        if (!world.canReserve('chat-1', this) || !world.canReserve('chat-2', partner)) return false;
        // Conversa compartilhada: começa quando os dois chegam; acaba junto.
        const chat = { dur: r(15, 30) * S, startMs: null, here: new Set() };
        const plan = (poi) => [
          { type: 'stand' },
          { type: 'goto', poi, activity: 'walking' },
          { type: 'face', poi },
          {
            type: 'stay', activity: 'chatting', duration: 75, facing: POI_BY_ID[poi].facing,
            onStart: (w, a) => chat.here.add(a.id),
            until: (w) => {
              if (this.intent !== 'conversar' || partner.intent !== 'conversar') return true;
              if (chat.here.size < 2) return false;
              chat.startMs ??= w.simMs;
              return w.simMs - chat.startMs > chat.dur;
            },
          },
        ];
        if (!this.startPlan(world, 'conversar', plan('chat-1'), ['chat-1'])) return false;
        if (!partner.startPlan(world, 'conversar', plan('chat-2'), ['chat-2'])) {
          this.setPlan(world, 'ficar à toa', [{ type: 'stay', activity: 'idle', duration: 3 }]);
          return true;
        }
        partner.lastFree = 'chat';
        world.lastChatSim = world.simMs;
        world.note('info', `${this.name} e ${partner.name} foram conversar na mesa alta`);
        return true;
      }
      case 'wander': {
        const n = 2 + Math.floor(this.rng.next() * 2);
        const pts = [];
        const pool = WANDER.filter((id) => world.canReserve(id, this));
        for (let i = 0; i < n && pool.length; i++) pts.push(pool.splice(Math.floor(this.rng.next() * pool.length), 1)[0]);
        if (!pts.length) return false;
        const steps = [{ type: 'stand' }];
        for (const p of pts) {
          steps.push({ type: 'goto', poi: p, activity: 'wandering' });
          steps.push({ type: 'stay', activity: 'idle', duration: r(1.5, 4), facing: this.rng.range(-Math.PI, Math.PI) });
        }
        return this.startPlan(world, 'passear', steps, pts);
      }
      default:
        return false;
    }
  }

  view() {
    const v = super.view();
    v.energy = Math.round(this.energy * 100) / 100;
    v.mode = this.mode;
    v.color = this.def.color;
    v.desk = this.desk;
    return v;
  }
}
