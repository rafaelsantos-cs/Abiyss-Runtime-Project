// Gerador pseudoaleatório determinístico (mulberry32) e utilidades.
//
// Toda "aleatoriedade" da simulação passa por aqui com semente explícita:
// a mesma semente + os mesmos eventos produzem o mesmo comportamento.

export function hashString(text) {
  // FNV-1a 32 bits.
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

export class Rng {
  constructor(seed) {
    this.state = (Number(seed) >>> 0) || 0x9e3779b9;
  }

  /** Número em [0, 1). */
  next() {
    this.state = (this.state + 0x6d2b79f5) >>> 0;
    let t = this.state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  }

  range(min, max) {
    return min + (max - min) * this.next();
  }

  int(min, maxInclusive) {
    return Math.floor(this.range(min, maxInclusive + 1));
  }

  chance(p) {
    return this.next() < p;
  }

  pick(list) {
    return list[Math.floor(this.next() * list.length)];
  }

  /** Escolha ponderada: [{value, weight}] → value (null se tudo zero). */
  weighted(items) {
    const total = items.reduce((s, it) => s + Math.max(0, it.weight), 0);
    if (total <= 0) return null;
    let r = this.next() * total;
    for (const it of items) {
      r -= Math.max(0, it.weight);
      if (r < 0) return it.value;
    }
    return items[items.length - 1].value;
  }

  /** Sub-gerador independente (ex.: um por entidade). */
  fork(label) {
    return new Rng((this.state ^ hashString(String(label))) >>> 0);
  }
}
