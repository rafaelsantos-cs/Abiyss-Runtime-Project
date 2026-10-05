// Seguidor de caminho com aceleração, curvas suaves e chegada gradual.
//
// A POSIÇÃO anda exatamente sobre a polilinha (que já foi validada contra
// paredes e móveis), então suavizar o movimento nunca faz atravessar nada.
// O que fica orgânico é a velocidade (acelera, freia nas curvas e na
// chegada) e a orientação (gira com velocidade angular limitada).

export function wrapAngle(a) {
  while (a > Math.PI) a -= 2 * Math.PI;
  while (a < -Math.PI) a += 2 * Math.PI;
  return a;
}

/** Ângulo de orientação que olha na direção (dx, dz). */
export function headingOf(dx, dz) {
  return Math.atan2(dx, dz);
}

export class Mover {
  constructor({ x, z, heading = 0, maxSpeed = 1.2, accel = 1.8, decel = 3.2, turnRate = 4.5 }) {
    this.x = x;
    this.z = z;
    this.heading = heading;
    this.speed = 0;
    this.maxSpeed = maxSpeed;
    this.accel = accel;
    this.decel = decel;
    this.turnRate = turnRate;
    /** @type {{x:number,z:number}[]|null} */
    this.path = null;
    this.idx = 0;
    /** Distância total percorrida (métrica e detecção de travamento). */
    this.odometer = 0;
  }

  setPath(points) {
    if (!points || points.length < 2) {
      this.path = null;
      return;
    }
    this.path = points;
    this.idx = 1;
  }

  clearPath() {
    this.path = null;
    this.speed = 0;
  }

  get hasPath() {
    return this.path !== null;
  }

  /** Distância restante até o fim do caminho. */
  remaining() {
    if (!this.path) return 0;
    let d = Math.hypot(this.path[this.idx].x - this.x, this.path[this.idx].z - this.z);
    for (let k = this.idx + 1; k < this.path.length; k++) {
      d += Math.hypot(this.path[k].x - this.path[k - 1].x, this.path[k].z - this.path[k - 1].z);
    }
    return d;
  }

  /** Direção atual do movimento (vetor unitário) ou null. */
  direction() {
    if (!this.path) return null;
    const t = this.path[this.idx];
    const dx = t.x - this.x;
    const dz = t.z - this.z;
    const len = Math.hypot(dx, dz);
    if (len < 1e-6) return null;
    return { x: dx / len, z: dz / len };
  }

  /** Gira na direção de um ângulo (sem andar). Devolve true quando alinhado. */
  turnTo(target, dt) {
    const diff = wrapAngle(target - this.heading);
    const maxStep = this.turnRate * dt;
    if (Math.abs(diff) <= maxStep) {
      this.heading = wrapAngle(target);
      return true;
    }
    this.heading = wrapAngle(this.heading + Math.sign(diff) * maxStep);
    return false;
  }

  /**
   * Avança um passo.
   * @param {number} dt segundos
   * @param {number} limit fator de velocidade extra (0 = parar, por exemplo para dar passagem)
   * @returns {'idle'|'moving'|'arrived'}
   */
  step(dt, limit = 1) {
    if (!this.path) {
      this.speed = Math.max(0, this.speed - this.decel * dt);
      return 'idle';
    }
    const dir = this.direction();
    if (dir) this.turnTo(headingOf(dir.x, dir.z), dt);
    const desired = dir ? headingOf(dir.x, dir.z) : this.heading;
    const angleDiff = Math.abs(wrapAngle(desired - this.heading));
    const corner = angleDiff > 1.3 ? 0.15 : 1 - 0.7 * (angleDiff / 1.3);
    const arrive = Math.min(1, Math.max(0.3, this.remaining() / 0.9));
    const target = this.maxSpeed * Math.min(corner, arrive) * Math.max(0, limit);
    if (this.speed < target) this.speed = Math.min(target, this.speed + this.accel * dt);
    else this.speed = Math.max(target, this.speed - this.decel * dt);

    let budget = this.speed * dt;
    while (budget > 0 && this.path) {
      const t = this.path[this.idx];
      const dx = t.x - this.x;
      const dz = t.z - this.z;
      const d = Math.hypot(dx, dz);
      if (d <= budget) {
        this.x = t.x;
        this.z = t.z;
        this.odometer += d;
        budget -= d;
        this.idx++;
        if (this.idx >= this.path.length) {
          this.path = null;
          this.speed = 0;
          return 'arrived';
        }
      } else {
        this.x += (dx / d) * budget;
        this.z += (dz / d) * budget;
        this.odometer += budget;
        budget = 0;
      }
    }
    return 'moving';
  }
}
