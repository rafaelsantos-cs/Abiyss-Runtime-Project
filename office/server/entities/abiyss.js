// Abiyss: a entidade central do escritório.
//
// Duas camadas de estado, ambas derivadas do runtime:
//
// MODO FÍSICO (onde está, o que o corpo faz):
//   offline  ← daemon parado / runtime inacessível → pousa no pedestal, apagado
//   sleep    ← sono (consolidação noturna)          → no pedestal, brilho lento
//   console  ← conversa com o dono, ou IMMo trazendo relatório → no console
//   free     ← vigília/descanso sem nada urgente     → supervisiona, olha a
//              janela, lê o quadro de goals, volta ao console
//
// MENTE (sobreposição visual, não muda a posição):
//   thinking   ← um ciclo do heartbeat que chamou o modelo (tabela `ciclos`)
//   delegating ← sub-agente novo atribuído a um IMMo
//   receiving  ← IMMo entregando relatório
//   conversing ← chamadas recentes com origem "conversa"
//
// O runtime grava o ciclo só quando ele termina; por isso o "pensando" é
// mostrado logo depois que o ciclo aparece no banco, com a duração real do
// ciclo (limitada entre 2,5 e 8 s para ficar legível).

import { Agent } from './agent.js';
import { POI_BY_ID } from '../../shared/layout.js';

const S = 1000;

export class Abiyss extends Agent {
  constructor({ x, z, rng }) {
    super({ id: 'abiyss', kind: 'abiyss', name: 'Abiyss', x, z, heading: 0, maxSpeed: 0.95, priority: 100, rng });
    this.pose = 'float';
    this.poseFrom = 'float';
    this.poi = 'abiyss-home';
    this.mode = null;
    this.mind = null; // { kind, untilMs, label }
    this.pendingThoughts = [];
    this.lastFree = null;
  }

  /** Sinais do runtime (eventos de domínio da ponte). */
  onRuntimeEvent(world, ev) {
    switch (ev.type) {
      case 'ciclo':
        if (ev.ciclo.chamouModelo) {
          this.pendingThoughts.push(ev.ciclo);
          if (this.pendingThoughts.length > 3) this.pendingThoughts.shift();
        }
        break;
      case 'subagente.atribuido':
        if (!ev.bootstrap) this.#setMind(world, 'delegating', 2.5, `delegou o sub-agente #${ev.subId} (${ev.nivel})`);
        break;
      default:
        break;
    }
  }

  onReport(world, immo, task) {
    this.#setMind(world, 'receiving', 4.5, `recebendo o relatório #${task.id} de ${immo.name}`);
  }

  #setMind(world, kind, seconds, label) {
    // "Pensando" tem prioridade visual sobre os outros.
    if (this.mind && this.mind.kind === 'thinking' && kind !== 'thinking' && this.mind.untilMs > world.simMs) return;
    this.mind = { kind, untilMs: world.simMs + seconds * S, label };
  }

  think(world) {
    const rt = world.runtime;
    // Mente.
    if (this.mind && this.mind.untilMs <= world.simMs) this.mind = null;
    if (!this.mind && this.pendingThoughts.length) {
      const c = this.pendingThoughts.shift();
      const dur = Math.min(8, Math.max(2.5, ((c.fimMs ?? 0) - (c.inicioMs ?? 0)) / S));
      this.mind = { kind: 'thinking', untilMs: world.simMs + dur * S, label: `heartbeat: ${c.motivo}` };
    }
    const conversando = rt.conversa.ultimaChamadaMs !== null && world.wallMs - rt.conversa.ultimaChamadaMs < 45 * S;
    if (!this.mind && conversando) this.mind = { kind: 'conversing', untilMs: world.simMs + 2 * S, label: 'conversando com o dono (abiyss chat)' };

    // Modo físico.
    const online = rt.ok && rt.daemon.estado === 'rodando';
    let desired = 'free';
    if (!online) desired = 'offline';
    else if (rt.sono.ativo || rt.ritmo.fase === 'sono') desired = 'sleep';
    else if (conversando || world.immos().some((i) => i.mode === 'report' && i.present)) desired = 'console';

    if (desired !== this.mode) {
      this.mode = desired;
      this.#planMode(world);
    } else if (this.idle) {
      this.#planMode(world);
    }
  }

  #home(activity, until) {
    return [
      { type: 'stand' },
      { type: 'goto', poi: 'abiyss-home', activity: 'moving' },
      { type: 'face', poi: 'abiyss-home' },
      { type: 'stay', activity, until, yieldable: false },
    ];
  }

  #planMode(world) {
    switch (this.mode) {
      case 'offline':
        world.note('info', 'Abiyss desligado (daemon do runtime parado ou inacessível)');
        this.startPlan(world, 'aguardar o daemon', this.#home('offline', (w) => w.runtime.ok && w.runtime.daemon.estado === 'rodando'), ['abiyss-home']);
        break;
      case 'sleep':
        world.note('info', 'Abiyss dormindo (sono: consolidação noturna)');
        this.startPlan(world, 'dormir (sono)', this.#home('sleeping', (w) => !(w.runtime.sono.ativo || w.runtime.ritmo.fase === 'sono')), ['abiyss-home']);
        break;
      case 'console':
        this.startPlan(world, 'atender no console', this.#home('idle', (w) => this.#wantsConsole(w) === false), ['abiyss-home']);
        break;
      default:
        this.#decideFree(world);
    }
  }

  #wantsConsole(world) {
    const rt = world.runtime;
    const conversando = rt.conversa.ultimaChamadaMs !== null && world.wallMs - rt.conversa.ultimaChamadaMs < 45 * S;
    return conversando || world.immos().some((i) => i.mode === 'report' && i.present);
  }

  #decideFree(world) {
    const fase = world.runtime.ritmo.fase;
    const working = world.immos().filter((i) => i.activity === 'working');
    const resting = world.immos().filter((i) => ['resting', 'sleeping'].includes(i.activity));
    const w = fase === 'descanso'
      ? { console: 45, window: 40, board: 10, supervise: working.length ? 20 : 0 }
      : { console: 30, supervise: working.length ? 38 : 0, board: 12, window: 13, center: 8, lounge: resting.length ? 7 : 0 };
    const items = Object.entries(w).map(([value, weight]) => ({ value, weight: value === this.lastFree ? weight * 0.35 : weight }));
    const r = (a, b) => a + this.rng.next() * (b - a);
    for (let tries = 0; tries < 5; tries++) {
      const key = this.rng.weighted(items);
      if (!key) break;
      if (this.#planFree(world, key, working, r)) {
        this.lastFree = key;
        return;
      }
      items.forEach((it) => { if (it.value === key) it.weight = 0; });
    }
    this.startPlan(world, 'monitorar no console', this.#home('idle', undefined).map((st) => (st.type === 'stay' ? { ...st, duration: 10, until: undefined } : st)), ['abiyss-home']);
  }

  #visit(world, intent, poi, activity, duration) {
    return this.startPlan(world, intent, [
      { type: 'stand' },
      { type: 'goto', poi, activity: 'moving' },
      { type: 'face', poi },
      { type: 'stay', activity, duration, facing: POI_BY_ID[poi].facing },
    ], [poi]);
  }

  #planFree(world, key, working, r) {
    switch (key) {
      case 'console':
        return this.startPlan(world, 'monitorar no console', [
          { type: 'stand' },
          { type: 'goto', poi: 'abiyss-home', activity: 'moving' },
          { type: 'face', poi: 'abiyss-home' },
          { type: 'stay', activity: 'idle', duration: r(18, 40) },
        ], ['abiyss-home']);
      case 'supervise': {
        // Visita os pods onde há IMMos trabalhando.
        const pods = new Set(working.map((i) => (['desk-1', 'desk-2'].includes(i.desk) ? 'pod-a-inspect' : 'pod-b-inspect')));
        const list = [...pods].filter((p) => world.canReserve(p, this));
        if (!list.length) return false;
        if (list.length > 1 && this.rng.chance(0.5)) list.reverse();
        const steps = [{ type: 'stand' }];
        for (const p of list) {
          steps.push({ type: 'goto', poi: p, activity: 'moving' });
          steps.push({ type: 'face', poi: p });
          steps.push({ type: 'stay', activity: 'supervising', duration: r(6, 11), facing: POI_BY_ID[p].facing });
        }
        return this.startPlan(world, 'supervisionar os IMMos', steps, list);
      }
      case 'board':
        return this.#visit(world, 'revisar o quadro de goals', 'board', 'supervising', r(6, 10));
      case 'window':
        return this.#visit(world, 'olhar a janela', 'abiyss-window', fase(world) === 'descanso' ? 'resting' : 'observing', r(15, 30));
      case 'center':
        return this.#visit(world, 'circular pelo escritório', 'center', 'supervising', r(5, 8));
      case 'lounge':
        return world.canReserve('wander-7', this) && this.#visit(world, 'ver a área de descanso', 'wander-7', 'observing', r(5, 9));
      default:
        return false;
    }
  }

  view() {
    const v = super.view();
    v.mode = this.mode;
    v.mind = this.mind ? { kind: this.mind.kind, label: this.mind.label } : null;
    return v;
  }
}

function fase(world) {
  return world.runtime.ritmo.fase;
}
