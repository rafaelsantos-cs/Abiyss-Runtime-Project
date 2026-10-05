// Simulação: liga a FONTE (runtime real ou demo) ao MUNDO, em passos fixos.
//
// Usada tanto pelo servidor (em tempo real) quanto pelos testes e pelo
// soak (o mais rápido possível). A leitura da fonte acontece a cada
// `pollIntervalMs` de tempo de simulação.

import { World } from './world.js';
import { SimClock } from './clock.js';

export class Simulation {
  /**
   * @param {object} opts
   * @param {object} opts.source  RuntimeSqliteSource | DemoRuntimeSource
   * @param {number} opts.seed
   * @param {string} opts.timeZone
   * @param {number} [opts.tickHz]
   * @param {number} [opts.pollIntervalMs]
   * @param {number|null} [opts.clockStartMs]  instante inicial do mundo (demo)
   * @param {number} [opts.timeScale]
   * @param {object} [opts.privacy]
   */
  constructor({ source, seed, timeZone, tickHz = 10, pollIntervalMs = 1000, clockStartMs = null, timeScale = 1, privacy, realNow = Date.now }) {
    this.source = source;
    this.dt = 1 / tickHz;
    this.pollIntervalMs = pollIntervalMs;
    this.clock = new SimClock({ startMs: clockStartMs, timeScale, realNow });
    this.world = new World({ seed, clock: this.clock, timeZone, privacy });
    this.sinceLastPoll = Infinity;
    this.realNow = realNow;
    this.environment = null;
  }

  /** Fonte "demo" segue o relógio do mundo; o runtime real usa a hora real. */
  #sourceNow() {
    return this.source.name === 'demo' ? this.world.wallMs : this.realNow();
  }

  poll() {
    const snap = this.source.poll(this.#sourceNow());
    this.world.ingest(snap, this.source.status());
    this.sinceLastPoll = 0;
    return snap;
  }

  /** Um passo fixo da simulação. */
  stepOnce() {
    if (this.sinceLastPoll >= this.pollIntervalMs) this.poll();
    if (this.environment) this.world.env = this.environment.current(this.world.wallMs);
    this.world.step(this.dt);
    this.sinceLastPoll += this.dt * 1000;
  }

  /** Roda `seconds` de simulação (headless), chamando `onTick` a cada passo. */
  run(seconds, onTick = null) {
    const n = Math.round(seconds / this.dt);
    for (let i = 0; i < n; i++) {
      this.stepOnce();
      if (onTick) onTick(this.world, i);
    }
  }
}
