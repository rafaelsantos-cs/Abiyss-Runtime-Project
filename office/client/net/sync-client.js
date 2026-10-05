// Cliente de sincronização: recebe o estado AUTORITATIVO do servidor por SSE
// e oferece uma visão interpolada para a renderização.
//
// O cliente não simula nada: ele só suaviza entre quadros. A reprodução fica
// ~150 ms atrás do quadro mais novo para sempre ter dois quadros para
// interpolar (o mesmo princípio de jogos em rede).

const DELAY_MS = 150;

function lerpAngle(a, b, t) {
  let d = b - a;
  while (d > Math.PI) d -= 2 * Math.PI;
  while (d < -Math.PI) d += 2 * Math.PI;
  return a + d * t;
}

export class SyncClient extends EventTarget {
  constructor(url = '/api/events') {
    super();
    this.url = url;
    this.frames = [];
    this.meta = null;
    this.hello = null;
    this.status = 'conectando';
    this.lastMessageAt = 0;
    this.playSim = null;
    this.rate = 1;
    this.received = 0;
    this.receivedWindow = [];
    this.errors = 0;
    this.lastFrameRecvAt = 0;
  }

  connect() {
    this.es = new EventSource(this.url);
    this.es.addEventListener('open', () => this.#setStatus('conectado'));
    this.es.addEventListener('error', () => {
      this.errors++;
      this.#setStatus(this.es.readyState === EventSource.CLOSED ? 'desconectado' : 'reconectando');
    });
    this.es.addEventListener('hello', (e) => {
      this.hello = JSON.parse(e.data);
      this.#touch();
      this.dispatchEvent(new CustomEvent('hello', { detail: this.hello }));
    });
    this.es.addEventListener('meta', (e) => {
      this.meta = JSON.parse(e.data);
      this.#touch();
      this.dispatchEvent(new CustomEvent('meta', { detail: this.meta }));
    });
    this.es.addEventListener('tick', (e) => {
      const f = JSON.parse(e.data);
      f.recvAt = performance.now();
      this.#touch();
      this.received++;
      this.receivedWindow.push(f.recvAt);
      while (this.receivedWindow.length && f.recvAt - this.receivedWindow[0] > 2000) this.receivedWindow.shift();
      const prev = this.frames[this.frames.length - 1];
      if (prev && f.simMs < prev.simMs) {
        // Servidor reiniciou: recomeça a linha do tempo.
        this.frames = [];
        this.playSim = null;
      }
      // Taxa de reprodução: a escala anunciada pelo servidor; sem ela, estima
      // por uma janela longa (quadros chegam em rajadas quando o navegador
      // está lento, então a taxa instantânea não serve).
      const announced = this.hello?.server?.timeScale;
      if (Number.isFinite(announced)) this.rate = announced;
      else if (this.frames.length > 10) {
        const first = this.frames[0];
        if (f.recvAt - first.recvAt > 1500) this.rate = Math.min(60, Math.max(0.05, (f.simMs - first.simMs) / (f.recvAt - first.recvAt)));
      }
      this.frames.push(f);
      if (this.frames.length > 40) this.frames.shift();
      this.lastFrameRecvAt = f.recvAt;
      if (this.frames.length === 1) this.dispatchEvent(new CustomEvent('first-frame', { detail: f }));
    });
  }

  #touch() {
    this.lastMessageAt = performance.now();
    if (this.status !== 'conectado') this.#setStatus('conectado');
  }

  #setStatus(s) {
    if (this.status === s) return;
    this.status = s;
    this.dispatchEvent(new CustomEvent('status', { detail: s }));
  }

  get framesPerSecond() {
    return this.receivedWindow.length / 2;
  }

  /** Atraso desde a última mensagem (ms). */
  get silenceMs() {
    return this.lastMessageAt ? performance.now() - this.lastMessageAt : Infinity;
  }

  /**
   * Avança o relógio de reprodução e devolve a visão interpolada:
   * { frame (o mais novo), entities: Map(id → estado interpolado), simMs }
   */
  sample(dtMs) {
    const n = this.frames.length;
    if (n === 0) return null;
    const newest = this.frames[n - 1];
    const sinceNewest = performance.now() - newest.recvAt;
    const target = newest.simMs + (sinceNewest - DELAY_MS) * this.rate;
    if (this.playSim === null || Math.abs(this.playSim - target) > 1500 * this.rate) this.playSim = target;
    else {
      this.playSim += dtMs * this.rate;
      this.playSim += (target - this.playSim) * 0.08;
    }
    // Dois quadros em volta do instante de reprodução.
    let a = this.frames[0];
    let b = newest;
    for (let i = n - 1; i > 0; i--) {
      if (this.frames[i - 1].simMs <= this.playSim) {
        a = this.frames[i - 1];
        b = this.frames[i];
        break;
      }
    }
    const span = b.simMs - a.simMs;
    const t = span > 0 ? Math.min(1, Math.max(0, (this.playSim - a.simMs) / span)) : 1;
    const prevById = new Map(a.entities.map((e) => [e.id, e]));
    const entities = new Map();
    for (const eb of b.entities) {
      const ea = prevById.get(eb.id);
      if (!ea || !ea.present) {
        entities.set(eb.id, { ...eb });
        continue;
      }
      // Estados discretos (atividade, pose) vêm do quadro mais próximo.
      const near = t < 0.5 ? ea : eb;
      entities.set(eb.id, {
        ...near,
        x: ea.x + (eb.x - ea.x) * t,
        z: ea.z + (eb.z - ea.z) * t,
        h: lerpAngle(ea.h, eb.h, t),
        spd: ea.spd + (eb.spd - ea.spd) * t,
        poseT: near === eb ? eb.poseT : ea.poseT,
        path: eb.path,
        dest: eb.dest,
        logical: eb.logical,
        mind: eb.mind,
      });
    }
    return { frame: newest, entities, simMs: this.playSim, fx: newest.fx, clock: newest.clock };
  }
}
