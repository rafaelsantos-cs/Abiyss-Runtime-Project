// A* em grade (8 vizinhos, sem cortar quinas) + suavização por linha de visada.
//
// Tudo determinístico: mesma grade + mesmos pontos = mesmo caminho.

const SQRT2 = Math.SQRT2;

/** Fila de prioridade (heap binário) sobre índices de célula. */
class MinHeap {
  constructor() {
    this.items = [];
    this.keys = [];
  }

  get size() {
    return this.items.length;
  }

  push(item, key) {
    const a = this.items;
    const k = this.keys;
    a.push(item);
    k.push(key);
    let i = a.length - 1;
    while (i > 0) {
      const p = (i - 1) >> 1;
      // Desempate pelo índice: ordem estável e determinística.
      if (k[p] < key || (k[p] === key && a[p] <= item)) break;
      a[i] = a[p]; k[i] = k[p];
      i = p;
    }
    a[i] = item; k[i] = key;
  }

  pop() {
    const a = this.items;
    const k = this.keys;
    const top = a[0];
    const lastItem = a.pop();
    const lastKey = k.pop();
    if (a.length > 0) {
      let i = 0;
      const n = a.length;
      for (;;) {
        const l = 2 * i + 1;
        if (l >= n) break;
        const r = l + 1;
        let m = l;
        if (r < n && (k[r] < k[l] || (k[r] === k[l] && a[r] < a[l]))) m = r;
        if (k[m] > lastKey || (k[m] === lastKey && a[m] >= lastItem)) break;
        a[i] = a[m]; k[i] = k[m];
        i = m;
      }
      a[i] = lastItem; k[i] = lastKey;
    }
    return top;
  }
}

export class PathFinder {
  /** @param {import('./grid.js').NavGrid} grid */
  constructor(grid) {
    this.grid = grid;
    const n = grid.cols * grid.rows;
    this.g = new Float32Array(n);
    this.came = new Int32Array(n);
    this.stamp = new Uint32Array(n);
    this.closed = new Uint32Array(n);
    this.generation = 0;
    this.lastExpanded = 0;
  }

  /**
   * Caminho de (sx,sz) até (gx,gz).
   * @param {object} opts
   * @param {number} [opts.allowed] máscara de assentos permitidos
   * @param {(i:number)=>boolean} [opts.blocked] células ocupadas agora (outras entidades)
   * @param {number} [opts.maxExpanded] limite de segurança
   * @returns {{x:number,z:number}[]|null} pontos (inclui início e fim exatos)
   */
  find(sx, sz, gx, gz, { allowed = 0, blocked = null, maxExpanded = 120000 } = {}) {
    const grid = this.grid;
    let start = grid.cellOf(sx, sz);
    const exactStart = grid.isFree(start, allowed);
    if (!exactStart) {
      // Entidade levemente fora da área livre (ex.: arredondamento): parte da
      // célula livre mais próxima.
      start = grid.nearestFree(sx, sz, allowed, 1.5);
      if (start < 0) return null;
    }
    // A célula de partida nunca conta como bloqueada (a entidade está nela).
    const passable = (i) => grid.isFree(i, allowed) && (i === start || !(blocked && blocked(i)));
    const goal = grid.cellOf(gx, gz);
    if (goal < 0 || !passable(goal)) return null;

    const cells = this.#search(start, goal, passable, maxExpanded);
    if (!cells) return null;
    const pts = this.#smooth(cells, allowed, blocked, start);
    const los = (a, b) => grid.losClear(a.x, a.z, b.x, b.z, allowed, (i) => i !== start && blocked !== null && blocked(i));

    const out = [];
    const s0 = exactStart ? { x: sx, z: sz } : pts[0];
    out.push(s0);
    const goalPt = { x: gx, z: gz };
    if (pts.length === 1) {
      if (s0.x !== gx || s0.z !== gz) out.push(goalPt);
      return out;
    }
    if (exactStart && !los(s0, pts[1])) out.push(pts[0]);
    for (let k = 1; k < pts.length - 1; k++) out.push(pts[k]);
    if (!los(out[out.length - 1], goalPt)) out.push(pts[pts.length - 1]);
    out.push(goalPt);
    return out;
  }

  #search(start, goal, passable, maxExpanded) {
    const grid = this.grid;
    const cols = grid.cols;
    const gen = ++this.generation;
    const { g, came, stamp, closed } = this;
    const gc = goal % cols;
    const gr = (goal - gc) / cols;
    const h = (i) => {
      const c = i % cols;
      const r = (i - c) / cols;
      const dx = Math.abs(c - gc);
      const dz = Math.abs(r - gr);
      return (dx + dz) + (SQRT2 - 2) * Math.min(dx, dz);
    };
    const open = new MinHeap();
    stamp[start] = gen;
    g[start] = 0;
    came[start] = -1;
    open.push(start, h(start));
    let expanded = 0;
    while (open.size) {
      const cur = open.pop();
      if (closed[cur] === gen) continue;
      closed[cur] = gen;
      if (cur === goal) break;
      if (++expanded > maxExpanded) {
        this.lastExpanded = expanded;
        return null;
      }
      const c = cur % cols;
      for (let dz = -1; dz <= 1; dz++) {
        for (let dx = -1; dx <= 1; dx++) {
          if (dx === 0 && dz === 0) continue;
          const nc = c + dx;
          if (nc < 0 || nc >= cols) continue;
          const nb = cur + dz * cols + dx;
          if (nb < 0 || nb >= grid.base.length) continue;
          if (closed[nb] === gen || !passable(nb)) continue;
          if (dx !== 0 && dz !== 0) {
            // Sem cortar quinas: os dois vizinhos ortogonais precisam estar livres.
            if (!passable(cur + dx) || !passable(cur + dz * cols)) continue;
          }
          const cost = g[cur] + (dx !== 0 && dz !== 0 ? SQRT2 : 1);
          if (stamp[nb] !== gen || cost < g[nb]) {
            stamp[nb] = gen;
            g[nb] = cost;
            came[nb] = cur;
            open.push(nb, cost + h(nb));
          }
        }
      }
    }
    this.lastExpanded = expanded;
    if (closed[goal] !== gen) return null;
    const path = [];
    for (let i = goal; i !== -1; i = came[i]) path.push(i);
    path.reverse();
    return path;
  }

  /** "Puxa a corda": mantém só os pontos necessários com linha de visada. */
  #smooth(cells, allowed, blocked, start) {
    const grid = this.grid;
    const blk = blocked ? (i) => i !== start && blocked(i) : null;
    const pts = cells.map((i) => grid.centerOf(i));
    if (pts.length <= 2) return pts;
    const out = [pts[0]];
    let anchor = 0;
    while (anchor < pts.length - 1) {
      let best = anchor + 1;
      // Procura o ponto mais distante ainda visível a partir da âncora.
      for (let k = pts.length - 1; k > anchor + 1; k--) {
        if (grid.losClear(pts[anchor].x, pts[anchor].z, pts[k].x, pts[k].z, allowed, blk)) {
          best = k;
          break;
        }
      }
      out.push(pts[best]);
      anchor = best;
    }
    return out;
  }
}

/** Comprimento de uma polilinha. */
export function pathLength(points) {
  let len = 0;
  for (let i = 1; i < points.length; i++) {
    len += Math.hypot(points[i].x - points[i - 1].x, points[i].z - points[i - 1].z);
  }
  return len;
}
