// Agente físico (IMMo ou Abiyss): corpo no mundo + executor de planos.
//
// Um PLANO é uma lista de PASSOS com estado explícito — a máquina de
// estados de cada entidade:
//
//   goto  → navega até um POI (A* + desvio de outras entidades)
//   point → navega até um ponto qualquer (ex.: dar passagem)
//   face  → gira no lugar
//   pose  → senta / deita / encolhe (transição animada)
//   stand → levanta
//   stay  → permanece fazendo uma atividade (com duração ou condição)
//   wait  → espera parado
//   call  → efeito instantâneo (registrar, liberar relatório...)
//
// O "cérebro" (immo.js / abiyss.js) escolhe planos; este arquivo só os
// executa. Nada aqui depende da renderização.

import { Mover, headingOf } from '../navigation/mover.js';
import { POI_BY_ID } from '../../shared/layout.js';
import { FREE } from '../navigation/grid.js';

const POSE_TIME = 0.9;
const SOLID_RADIUS = 0.75;

export class Agent {
  constructor({ id, kind, name, x, z, heading = 0, maxSpeed = 1.15, priority = 1, rng }) {
    this.id = id;
    this.kind = kind;
    this.name = name;
    this.mover = new Mover({ x, z, heading, maxSpeed });
    this.priority = priority;
    this.rng = rng;
    this.pose = 'stand';
    this.poseFrom = 'stand';
    this.poseT = 1;
    this.activity = 'idle';
    this.intent = null;
    this.plan = [];
    this.stepIdx = 0;
    this.step = null;
    /** POI onde a entidade está (ou para onde vai). */
    this.poi = null;
    this.dest = null;
    this.allowedMask = 0;
    this.present = true;
    this.makeWay = false;
    this.leaving = null;
    this.stats = { replans: 0, stuck: 0, yields: 0, failedSteps: 0 };
    this.lastActivity = null;
  }

  get x() { return this.mover.x; }
  get z() { return this.mover.z; }

  get moving() {
    return this.mover.hasPath && this.mover.speed > 0.02;
  }

  /** Entidades em pé ocupam espaço para quem anda (sentadas estão em assentos). */
  get solid() {
    return this.present && (this.pose === 'stand' || this.pose === 'float');
  }

  /** POI onde o corpo está agora (a menos de 35 cm), ou null. */
  hereAt() {
    const p = this.poi ? POI_BY_ID[this.poi] : null;
    return p && Math.hypot(this.x - p.x, this.z - p.z) < 0.35 ? this.poi : null;
  }

  /**
   * Troca o plano atual. As reservas antigas são soltas, menos a do lugar
   * onde o corpo está (ela é solta quando ele se afastar).
   */
  setPlan(world, intent, steps) {
    const here = this.hereAt();
    world.releaseAll(this, here);
    if (!here) this.poi = null;
    this.leaving = here;
    this.intent = intent;
    this.plan = steps;
    this.stepIdx = 0;
    this.step = null;
    this.makeWay = false;
    this.mover.clearPath();
    this.dest = null;
  }

  /**
   * Começa um plano só se todos os POIs puderem ser reservados.
   * @returns {boolean}
   */
  startPlan(world, intent, steps, reserve = []) {
    for (const poi of reserve) if (!world.canReserve(poi, this)) return false;
    this.setPlan(world, intent, steps);
    for (const poi of reserve) world.reserve(poi, this);
    return true;
  }

  get idle() {
    return this.stepIdx >= this.plan.length;
  }

  /** Executa o plano por `dt` segundos. */
  run(world, dt) {
    // Transição de pose (sentar/levantar) anda sempre.
    if (this.poseT < 1) this.poseT = Math.min(1, this.poseT + dt / POSE_TIME);
    let guard = 0;
    while (this.stepIdx < this.plan.length && guard++ < 6) {
      const def = this.plan[this.stepIdx];
      if (!this.step) this.step = { def, t: 0, started: false };
      const st = this.step;
      const r = this.#runStep(world, st, dt);
      if (r === 'running') return;
      if (r === 'failed') {
        this.stats.failedSteps++;
        world.note('debug', `${this.name}: passo ${def.type} falhou (${st.reason ?? 'sem motivo'})`);
        this.plan = [];
        this.stepIdx = 0;
        this.step = null;
        this.mover.clearPath();
        this.dest = null;
        return;
      }
      this.stepIdx++;
      this.step = null;
      dt = 0; // passos seguintes começam neste tick sem consumir tempo
    }
  }

  #setPose(pose) {
    if (this.pose === pose) return;
    this.poseFrom = this.pose;
    this.pose = pose;
    this.poseT = 0;
  }

  #runStep(world, st, dt) {
    const d = st.def;
    switch (d.type) {
      case 'call':
        d.fn(world, this);
        return 'done';
      case 'stand': {
        if (this.pose === 'stand' || this.pose === 'float') {
          if (!st.started) return 'done';
          if (this.poseT >= 1) return 'done';
          return 'running';
        }
        st.started = true;
        this.activity = 'standing_up';
        this.#setPose(this.kind === 'abiyss' ? 'float' : 'stand');
        return 'running';
      }
      case 'pose': {
        if (!st.started) {
          st.started = true;
          const p = POI_BY_ID[d.poi];
          this.activity = 'sitting_down';
          if (p) this.mover.heading = p.facing;
          this.#setPose(d.pose);
        }
        return this.poseT >= 1 ? 'done' : 'running';
      }
      case 'face': {
        const target = d.angle ?? POI_BY_ID[d.poi]?.facing ?? this.mover.heading;
        if (d.activity) this.activity = d.activity;
        st.t += dt;
        return this.mover.turnTo(target, dt) || st.t > 2 ? 'done' : 'running';
      }
      case 'wait':
        this.activity = d.activity ?? 'waiting';
        st.t += dt;
        return st.t >= d.duration ? 'done' : 'running';
      case 'stay': {
        if (!st.started) {
          st.started = true;
          this.activity = d.activity;
          this.makeWay = false;
          if (d.onStart) d.onStart(world, this);
        }
        this.activity = d.activity;
        st.t += dt;
        if (d.facing !== undefined) this.mover.turnTo(d.facing, dt);
        if (d.until && d.until(world, this)) return 'done';
        if (d.duration !== undefined && st.t >= d.duration) return 'done';
        if (this.makeWay && d.yieldable !== false) {
          this.makeWay = false;
          return 'done';
        }
        return 'running';
      }
      case 'goto':
      case 'point':
        return this.#runGoto(world, st, dt);
      default:
        st.reason = `passo desconhecido ${d.type}`;
        return 'failed';
    }
  }

  #target(d) {
    if (d.type === 'goto') {
      const p = POI_BY_ID[d.poi];
      return p ? { x: p.x, z: p.z, poi: d.poi } : null;
    }
    return { x: d.x, z: d.z, poi: null };
  }

  #plan(world, st, blockedFn = null) {
    const d = st.def;
    const target = this.#target(d);
    if (!target) return false;
    const grid = world.grid;
    this.allowedMask = ((d.poi ? grid.allowedFor(d.poi) : 0) | grid.seatsAt(this.x, this.z) | (d.allow ?? 0)) >>> 0;
    const path = world.pathfinder.find(this.x, this.z, target.x, target.z, { allowed: this.allowedMask, blocked: blockedFn });
    if (!path) return false;
    this.mover.setPath(path);
    this.dest = target;
    return true;
  }

  #runGoto(world, st, dt) {
    const d = st.def;
    if (!st.started) {
      st.started = true;
      st.blockedT = 0;
      st.stillT = 0;
      st.replans = 0;
      st.lastOdo = this.mover.odometer;
      st.total = 0;
      const target = this.#target(d);
      if (!target) { st.reason = 'destino inexistente'; return 'failed'; }
      if (Math.hypot(this.x - target.x, this.z - target.z) < 0.02) {
        this.#arrive(world, d);
        return 'done';
      }
      if (!this.#plan(world, st)) { st.reason = 'sem caminho'; return 'failed'; }
    }
    this.activity = d.activity ?? 'walking';
    if (this.pose !== 'stand' && this.pose !== 'float') this.#setPose(this.kind === 'abiyss' ? 'float' : 'stand');

    // Soltar o lugar de onde saiu, quando já se afastou.
    if (this.leaving && this.leaving !== d.poi) {
      const lp = POI_BY_ID[this.leaving];
      if (!lp || Math.hypot(this.x - lp.x, this.z - lp.z) > 1.2) {
        world.releaseOne(this.leaving, this);
        if (this.poi === this.leaving) this.poi = null;
        this.leaving = null;
      }
    }

    // Desvio de quem está à frente.
    const dir = this.mover.direction();
    let limit = 1;
    let blocker = null;
    if (dir) {
      for (const o of world.agents) {
        if (o === this || !o.solid) continue;
        const rx = o.x - this.x;
        const rz = o.z - this.z;
        const dist = Math.hypot(rx, rz);
        if (dist > 1.35 || dist < 1e-6) continue;
        const ahead = (dir.x * rx + dir.z * rz) / dist;
        // Espaço pessoal: muito perto e indo na direção do outro (mesmo de lado) → para.
        if (dist < 0.6 && ahead > 0.05) {
          limit = 0;
          blocker = o;
          break;
        }
        if (ahead < 0.5) continue;
        // Quem está parado no próprio destino não bloqueia a chegada (fila).
        if (dist < SOLID_RADIUS) {
          limit = 0;
          blocker = o;
          break;
        }
        limit = Math.min(limit, (dist - SOLID_RADIUS) / 0.6);
      }
      // Quase parado atrás de alguém conta como bloqueado (evita impasse lento).
      if (limit < 0.2 && !blocker) {
        blocker = nearestAhead(world, this, dir);
        limit = 0;
      }
    }
    if (limit === 0) {
      st.blockedT += dt;
      this.activity = 'waiting';
      if (st.blockedT > 0.7 && st.replans < 4 && (st.lastReplanT === undefined || st.total - st.lastReplanT > 0.7)) {
        st.replans++;
        st.lastReplanT = st.total;
        this.stats.replans++;
        const others = world.agents.filter((o) => o !== this && o.solid);
        const blocked = (i) => {
          const c = world.grid.centerOf(i);
          for (const o of others) if (Math.hypot(c.x - o.x, c.z - o.z) < 0.7) return true;
          return false;
        };
        if (this.#plan(world, st, blocked)) st.blockedT = 0;
      }
      if (st.blockedT > 2.5 && blocker) {
        if (!blocker.moving && blocker.step?.def.type === 'stay') {
          blocker.makeWay = true;
        } else if (this.priority < blocker.priority && !st.yielded) {
          // Dá passagem: anda um pouco para o lado/trás e tenta de novo.
          const side = this.#sidestep(world, blocker);
          if (side) {
            st.yielded = true;
            this.stats.yields++;
            this.plan.splice(this.stepIdx, 0,
              { type: 'point', x: side.x, z: side.z, activity: 'waiting' },
              { type: 'wait', duration: 1.2 + this.rng.next(), activity: 'waiting' });
            this.step = null;
            this.mover.clearPath();
            return 'running';
          }
        }
      }
      if (st.blockedT > 14) { st.reason = 'bloqueado por muito tempo'; return 'failed'; }
    } else {
      st.blockedT = 0;
    }
    st.total += dt;

    const status = this.mover.step(dt, limit);
    if (status === 'arrived') {
      this.#arrive(world, d);
      return 'done';
    }
    // Travamento: sem progresso por 15 s (sem contar esperas legítimas curtas).
    if (this.mover.odometer - st.lastOdo > 0.3) {
      st.lastOdo = this.mover.odometer;
      st.stillT = 0;
    } else {
      st.stillT += dt;
      if (st.stillT > 15) {
        this.stats.stuck++;
        world.note('warn', `${this.name} travado indo para ${d.poi ?? `${d.x.toFixed(1)},${d.z.toFixed(1)}`}`);
        st.reason = 'travado';
        return 'failed';
      }
    }
    if (status === 'idle') {
      // Caminho perdido (ex.: limpo por interrupção): replaneja.
      if (!this.#plan(world, st)) { st.reason = 'sem caminho'; return 'failed'; }
    }
    return 'running';
  }

  #arrive(world, d) {
    this.mover.clearPath();
    this.dest = null;
    if (d.type === 'goto') this.poi = d.poi;
  }

  /** Ponto livre para o lado (ou para trás) longe de quem bloqueia. */
  #sidestep(world, blocker) {
    const dir = this.mover.direction() ?? { x: Math.sin(this.mover.heading), z: Math.cos(this.mover.heading) };
    const cands = [
      { x: -dir.z, z: dir.x }, { x: dir.z, z: -dir.x }, { x: -dir.x, z: -dir.z },
    ];
    for (const dist of [1.0, 1.4]) {
      for (const c of cands) {
        const x = this.x + c.x * dist;
        const z = this.z + c.z * dist;
        if (!world.grid.isFreeAt(x, z, 0)) continue;
        if (Math.hypot(x - blocker.x, z - blocker.z) < 1.0) continue;
        if (!world.grid.losClear(this.x, this.z, x, z, this.allowedMask)) continue;
        return { x, z };
      }
    }
    return null;
  }

  /** Visão para o cliente. */
  view() {
    const p = this.mover.path;
    let path = null;
    if (p) {
      path = [[round(this.x), round(this.z)]];
      for (let k = this.mover.idx; k < p.length; k++) path.push([round(p[k].x), round(p[k].z)]);
    }
    return {
      id: this.id,
      kind: this.kind,
      name: this.name,
      present: this.present,
      x: round(this.x),
      z: round(this.z),
      h: Math.round(this.mover.heading * 1000) / 1000,
      spd: Math.round(this.mover.speed * 100) / 100,
      act: this.activity,
      pose: this.pose,
      poseFrom: this.poseFrom,
      poseT: Math.round(this.poseT * 100) / 100,
      poi: this.poi,
      intent: this.intent,
      dest: this.dest ? { poi: this.dest.poi, x: round(this.dest.x), z: round(this.dest.z) } : null,
      path,
    };
  }

  /** Verifica se o corpo está em área permitida. */
  checkPlacement(world) {
    const g = world.grid;
    const i = g.cellOf(this.x, this.z);
    if (i < 0 || g.base[i] !== FREE) return 'parede/móvel/fora';
    const seats = g.seat[i];
    if ((seats & ~this.allowedMask) !== 0) {
      // Está numa área de assento: precisa ser de um assento que ele reservou ou está deixando.
      for (const [poiId, owner] of world.reservations) {
        const bit = g.seatBit.get(poiId);
        if (bit !== undefined && (seats & (1 << bit)) && owner !== this.id) return `assento de outro (${poiId})`;
      }
    }
    return null;
  }
}

function nearestAhead(world, self, dir) {
  let best = null;
  let bestD = Infinity;
  for (const o of world.agents) {
    if (o === self || !o.solid) continue;
    const rx = o.x - self.x;
    const rz = o.z - self.z;
    const d = Math.hypot(rx, rz);
    if (d < bestD && (dir.x * rx + dir.z * rz) / Math.max(d, 1e-6) >= 0.5) {
      best = o;
      bestD = d;
    }
  }
  return best;
}

export function faceToward(agent, x, z) {
  return headingOf(x - agent.x, z - agent.z);
}

function round(v) {
  return Math.round(v * 100) / 100;
}
