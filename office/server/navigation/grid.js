// Grade de navegação (espaço de configuração) construída a partir do layout.
//
// Cada célula de 10 cm guarda:
//   - `base`: LIVRE, PAREDE, MÓVEL ou FORA (do prédio);
//   - `seat`: máscara de bits dos assentos cuja área (inflada) cobre a célula.
//
// Obstáculos são "inflados" pelo raio do agente: se o CENTRO da entidade está
// numa célula livre, o corpo inteiro não encosta em parede nem em móvel.
// Assentos (cadeiras, sofás, pufes, banquetas) são áreas proibidas para quem
// passa, mas liberadas para quem reservou aquele assento.

import {
  AGENT_RADIUS, BUILDING, FURNITURE, POIS, SIDEWALK, WALLS, DOORS,
  blockingRects, solidWallSpans,
} from '../../shared/layout.js';

export const CELL = 0.1;
export const FREE = 0;
export const WALL = 1;
export const FURN = 2;
export const OUT = 3;

const ORIGIN_X = -0.6;
const ORIGIN_Z = -0.6;
const COLS = Math.ceil((BUILDING.width + 1.2) / CELL);
const ROWS = Math.ceil((SIDEWALK.z1 + 0.6 + 0.6) / CELL);

export class NavGrid {
  constructor({ radius = AGENT_RADIUS } = {}) {
    this.radius = radius;
    this.cols = COLS;
    this.rows = ROWS;
    this.originX = ORIGIN_X;
    this.originZ = ORIGIN_Z;
    this.base = new Uint8Array(COLS * ROWS).fill(OUT);
    this.seat = new Uint32Array(COLS * ROWS);
    /** poiId → índice do bit de assento. */
    this.seatBit = new Map();
    /** poiId → retângulo inflado do assento. */
    this.seatRect = new Map();
    /** poiId → máscara (assento + lugares travados junto). */
    this.seatLocksMask = new Map();
    this.#build();
  }

  // --- coordenadas -----------------------------------------------------

  cellOf(x, z) {
    const c = Math.floor((x - this.originX) / CELL);
    const r = Math.floor((z - this.originZ) / CELL);
    if (c < 0 || r < 0 || c >= this.cols || r >= this.rows) return -1;
    return r * this.cols + c;
  }

  centerOf(i) {
    const c = i % this.cols;
    const r = (i - c) / this.cols;
    return { x: this.originX + (c + 0.5) * CELL, z: this.originZ + (r + 0.5) * CELL };
  }

  // --- consultas ---------------------------------------------------------

  /** A célula é atravessável com a máscara de assentos permitidos? */
  isFree(i, allowed = 0) {
    if (i < 0 || this.base[i] !== FREE) return false;
    return (this.seat[i] & ~allowed) === 0;
  }

  isFreeAt(x, z, allowed = 0) {
    return this.isFree(this.cellOf(x, z), allowed);
  }

  /** Categoria da célula (FREE/WALL/FURN/OUT); fora da grade = OUT. */
  baseAt(x, z) {
    const i = this.cellOf(x, z);
    return i < 0 ? OUT : this.base[i];
  }

  /** Máscara dos assentos que cobrem a posição. */
  seatsAt(x, z) {
    const i = this.cellOf(x, z);
    return i < 0 ? 0 : this.seat[i];
  }

  /** Máscara de bits para uma lista de POIs de assento (ignora os que não são assento). */
  maskFor(poiIds) {
    let m = 0;
    for (const id of poiIds) {
      const b = this.seatBit.get(id);
      if (b !== undefined) m |= 1 << b;
    }
    return m >>> 0;
  }

  /**
   * Linha de visada livre entre dois pontos: percorre EXATAMENTE todas as
   * células que o segmento toca (travessia de grade de Amanatides–Woo, com
   * as duas células nos cruzamentos de quina). `blocked(i)` opcional marca
   * células temporariamente ocupadas.
   */
  losClear(ax, az, bx, bz, allowed = 0, blocked = null) {
    const ok = (c, r) => {
      if (c < 0 || r < 0 || c >= this.cols || r >= this.rows) return false;
      const i = r * this.cols + c;
      return this.isFree(i, allowed) && !(blocked && blocked(i));
    };
    const fx0 = (ax - this.originX) / CELL;
    const fz0 = (az - this.originZ) / CELL;
    const fx1 = (bx - this.originX) / CELL;
    const fz1 = (bz - this.originZ) / CELL;
    let c = Math.floor(fx0);
    let r = Math.floor(fz0);
    const cEnd = Math.floor(fx1);
    const rEnd = Math.floor(fz1);
    if (!ok(c, r)) return false;
    const dx = fx1 - fx0;
    const dz = fz1 - fz0;
    const stepC = dx > 0 ? 1 : dx < 0 ? -1 : 0;
    const stepR = dz > 0 ? 1 : dz < 0 ? -1 : 0;
    const tDeltaC = stepC !== 0 ? Math.abs(1 / dx) : Infinity;
    const tDeltaR = stepR !== 0 ? Math.abs(1 / dz) : Infinity;
    let tMaxC = stepC > 0 ? (Math.floor(fx0) + 1 - fx0) * tDeltaC : stepC < 0 ? (fx0 - Math.floor(fx0)) * tDeltaC : Infinity;
    let tMaxR = stepR > 0 ? (Math.floor(fz0) + 1 - fz0) * tDeltaR : stepR < 0 ? (fz0 - Math.floor(fz0)) * tDeltaR : Infinity;
    let guard = 0;
    while ((c !== cEnd || r !== rEnd) && guard++ < 10000) {
      if (Math.abs(tMaxC - tMaxR) < 1e-9) {
        // Passa exatamente pela quina: as duas vizinhas precisam estar livres.
        if (!ok(c + stepC, r) || !ok(c, r + stepR)) return false;
        c += stepC;
        r += stepR;
        tMaxC += tDeltaC;
        tMaxR += tDeltaR;
      } else if (tMaxC < tMaxR) {
        c += stepC;
        tMaxC += tDeltaC;
      } else {
        r += stepR;
        tMaxR += tDeltaR;
      }
      if (tMaxC > 1 + 1e-9 && tMaxR > 1 + 1e-9 && (c !== cEnd || r !== rEnd)) {
        // Erro numérico no fim do segmento: confere a célula final e para.
        return ok(c, r) && ok(cEnd, rEnd);
      }
      if (!ok(c, r)) return false;
    }
    return true;
  }

  /** Célula livre mais próxima (busca em largura), ou -1. */
  nearestFree(x, z, allowed = 0, maxRadius = 3) {
    const start = this.cellOf(x, z);
    if (start < 0) return -1;
    if (this.isFree(start, allowed)) return start;
    const maxSteps = Math.ceil(maxRadius / CELL);
    const seen = new Set([start]);
    let frontier = [start];
    for (let step = 0; step < maxSteps && frontier.length; step++) {
      const next = [];
      for (const i of frontier) {
        const c = i % this.cols;
        for (const [dc, dr] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
          const nc = c + dc;
          if (nc < 0 || nc >= this.cols) continue;
          const j = i + dr * this.cols + dc;
          if (j < 0 || j >= this.base.length || seen.has(j)) continue;
          seen.add(j);
          if (this.isFree(j, allowed)) return j;
          next.push(j);
        }
      }
      frontier = next;
    }
    return -1;
  }

  // --- construção ----------------------------------------------------------

  #fillRect(x0, z0, x1, z1, fn) {
    const c0 = Math.max(0, Math.floor((x0 - this.originX) / CELL));
    const c1 = Math.min(this.cols - 1, Math.floor((x1 - this.originX) / CELL));
    const r0 = Math.max(0, Math.floor((z0 - this.originZ) / CELL));
    const r1 = Math.min(this.rows - 1, Math.floor((z1 - this.originZ) / CELL));
    for (let r = r0; r <= r1; r++) {
      for (let c = c0; c <= c1; c++) {
        // Só células cujo CENTRO está dentro do retângulo.
        const cx = this.originX + (c + 0.5) * CELL;
        const cz = this.originZ + (r + 0.5) * CELL;
        if (cx < x0 || cx > x1 || cz < z0 || cz > z1) continue;
        fn(r * this.cols + c);
      }
    }
  }

  #build() {
    const t = BUILDING.wallThickness;
    const r = this.radius;
    const setFree = (i) => { this.base[i] = FREE; };

    // 1. Áreas andáveis: interior do prédio, calçada e passagens das portas externas.
    this.#fillRect(t / 2, t / 2, BUILDING.width - t / 2, BUILDING.depth - t / 2, setFree);
    this.#fillRect(SIDEWALK.x0, SIDEWALK.z0, SIDEWALK.x1, SIDEWALK.z1, setFree);
    for (const d of DOORS) {
      const half = d.width / 2;
      if (d.axis === 'x') this.#fillRect(d.x - half, d.z - 0.4, d.x + half, d.z + 0.4, setFree);
      else this.#fillRect(d.x - 0.4, d.z - half, d.x + 0.4, d.z + half, setFree);
    }

    // 2. Paredes (com as aberturas das portas), infladas pelo raio.
    for (const w of WALLS) {
      const horizontal = w.z1 === w.z2;
      for (const [a, b] of solidWallSpans(w)) {
        if (horizontal) {
          const xa = Math.min(w.x1, w.x2) + a;
          const xb = Math.min(w.x1, w.x2) + b;
          this.#fillRect(xa - r, w.z1 - t / 2 - r, xb + r, w.z1 + t / 2 + r, (i) => { this.base[i] = WALL; });
        } else {
          const za = Math.min(w.z1, w.z2) + a;
          const zb = Math.min(w.z1, w.z2) + b;
          this.#fillRect(w.x1 - t / 2 - r, za - r, w.x1 + t / 2 + r, zb + r, (i) => { this.base[i] = WALL; });
        }
      }
    }

    // 3. Móveis.
    for (const f of FURNITURE) {
      for (const rect of blockingRects(f)) {
        this.#fillRect(rect.x0 - r, rect.z0 - r, rect.x1 + r, rect.z1 + r, (i) => {
          if (this.base[i] === FREE) this.base[i] = FURN;
        });
      }
    }

    // 4. Assentos: abre o retângulo do assento (só sobre móvel, nunca parede)
    //    e marca a área inflada com o bit do assento.
    let bit = 0;
    for (const p of POIS) {
      if (!p.seat) continue;
      if (bit >= 32) throw new Error('mais de 32 assentos: aumente a máscara');
      const half = p.seat / 2;
      this.#fillRect(p.x - half, p.z - half, p.x + half, p.z + half, (i) => {
        if (this.base[i] === FURN) this.base[i] = FREE;
      });
      const rect = { x0: p.x - half - r, z0: p.z - half - r, x1: p.x + half + r, z1: p.z + half + r };
      const b = bit;
      this.#fillRect(rect.x0, rect.z0, rect.x1, rect.z1, (i) => { this.seat[i] |= (1 << b) >>> 0; });
      this.seatBit.set(p.id, b);
      this.seatRect.set(p.id, rect);
      bit++;
    }
    // Assentos compartilhados ("deitar no sofá" usa o mesmo espaço dos lugares):
    // quem reserva o assento deitado precisa poder pisar nas áreas dos lugares.
    for (const p of POIS) {
      if (!p.locks) continue;
      const own = this.seatBit.get(p.id);
      this.seatLocksMask.set(p.id, (this.maskFor(p.locks) | ((1 << own) >>> 0)) >>> 0);
    }
  }

  /**
   * Máscara permitida para quem reservou o POI: o próprio assento, os lugares
   * que ele trava (deitar ocupa o sofá todo) e os assentos que travam ele
   * (sentar num lugar também ocupa a área do "deitado").
   */
  allowedFor(poiId) {
    if (this.seatLocksMask.has(poiId)) return this.seatLocksMask.get(poiId);
    const ids = [poiId];
    for (const p of POIS) if (p.locks && p.locks.includes(poiId)) ids.push(p.id);
    return this.maskFor(ids);
  }

  /** Estatísticas (debug e testes). */
  stats() {
    const counts = [0, 0, 0, 0];
    for (const v of this.base) counts[v]++;
    return { cols: this.cols, rows: this.rows, free: counts[FREE], wall: counts[WALL], furniture: counts[FURN], outside: counts[OUT], seats: this.seatBit.size };
  }
}
