import { test } from 'node:test';
import assert from 'node:assert/strict';

import { NavGrid, FREE, WALL, OUT } from '../server/navigation/grid.js';
import { PathFinder, pathLength } from '../server/navigation/astar.js';
import { Mover } from '../server/navigation/mover.js';
import { POIS, POI_BY_ID, WALLS, DESK_PODS } from '../shared/layout.js';

const grid = new NavGrid();
const pf = new PathFinder(grid);

function allowedPair(a, b) {
  return (grid.allowedFor(a) | grid.allowedFor(b)) >>> 0;
}

/** Todo segmento do caminho tem linha de visada livre com a máscara usada. */
function assertPathClear(path, allowed, label) {
  for (let i = 1; i < path.length; i++) {
    const a = path[i - 1];
    const b = path[i];
    assert.ok(grid.losClear(a.x, a.z, b.x, b.z, allowed), `${label}: segmento ${i} atravessa obstáculo`);
  }
}

test('grade construída com áreas livres, paredes e assentos', () => {
  const s = grid.stats();
  assert.ok(s.free > 20000, `poucas células livres: ${s.free}`);
  assert.ok(s.wall > 1000);
  assert.ok(s.seats >= 13, 'cadeiras, sofás, pufes e banquetas viram assentos');
});

test('paredes e janelas bloqueiam; portas deixam passar', () => {
  // Meio de uma janela da parede norte (x=2.2): bloqueado.
  assert.equal(grid.baseAt(2.2, 0), WALL);
  // Parede oeste do vidro da sala do Abiyss, fora da porta.
  assert.equal(grid.baseAt(16, 2.0), WALL);
  // Centro da porta da sala do Abiyss e da porta principal: livres.
  assert.equal(grid.baseAt(16, 5.0), FREE);
  assert.equal(grid.baseAt(11.2, 16), FREE);
  // Fora do prédio (sem ser a calçada): proibido.
  assert.equal(grid.baseAt(5, 17.5), OUT);
});

test(`todo POI é alcançável a partir de todo POI (${POIS.length * POIS.length} pares)`, () => {
  const fails = [];
  for (const a of POIS) {
    for (const b of POIS) {
      const allowed = allowedPair(a.id, b.id);
      const path = pf.find(a.x, a.z, b.x, b.z, { allowed });
      if (!path) fails.push(`${a.id} → ${b.id}`);
      else assertPathClear(path, allowed, `${a.id} → ${b.id}`);
    }
  }
  assert.deepEqual(fails, []);
});

test('assento só é atravessável por quem reservou', () => {
  const d1 = POI_BY_ID['desk-1'];
  assert.equal(grid.isFreeAt(d1.x, d1.z, 0), false);
  assert.equal(grid.isFreeAt(d1.x, d1.z, grid.allowedFor('desk-1')), true);
  // Caminhos de passagem não cortam cadeiras de outras mesas.
  const path = pf.find(POI_BY_ID['entrance'].x, POI_BY_ID['entrance'].z, POI_BY_ID['win-n1'].x, POI_BY_ID['win-n1'].z);
  assert.ok(path);
  assertPathClear(path, 0, 'entrada → janela');
});

test('para sair da sala do Abiyss é preciso passar pela porta', () => {
  const r = POI_BY_ID['report'];
  const c = POI_BY_ID['center'];
  const path = pf.find(r.x, r.z, c.x, c.z);
  assert.ok(path);
  // Algum ponto da polilinha (amostrada) cruza x=16 perto da porta (z≈5).
  let crossed = false;
  for (let i = 1; i < path.length; i++) {
    const a = path[i - 1];
    const b = path[i];
    if ((a.x - 16) * (b.x - 16) <= 0 && a.x !== b.x) {
      const t = (16 - a.x) / (b.x - a.x);
      const z = a.z + (b.z - a.z) * t;
      assert.ok(Math.abs(z - 5.0) < 0.5, `cruzou o vidro em z=${z.toFixed(2)}`);
      crossed = true;
    }
  }
  assert.ok(crossed, 'o caminho deveria cruzar x=16');
});

test('entrar pela calçada usa a porta principal', () => {
  const o = POI_BY_ID['outside'];
  const d = POI_BY_ID['desk-4'];
  const path = pf.find(o.x, o.z, d.x, d.z, { allowed: grid.allowedFor('desk-4') });
  assert.ok(path);
  for (let i = 1; i < path.length; i++) {
    const a = path[i - 1];
    const b = path[i];
    if ((a.z - 16) * (b.z - 16) <= 0 && a.z !== b.z) {
      const t = (16 - a.z) / (b.z - a.z);
      const x = a.x + (b.x - a.x) * t;
      assert.ok(x > 10.4 && x < 12.0, `atravessou a parede sul fora da porta (x=${x.toFixed(2)})`);
    }
  }
});

test('caminhos são determinísticos', () => {
  const a = POI_BY_ID['coffee'];
  const b = POI_BY_ID['abiyss-window'];
  const p1 = pf.find(a.x, a.z, b.x, b.z);
  const p2 = pf.find(a.x, a.z, b.x, b.z);
  assert.deepEqual(p1, p2);
  assert.ok(pathLength(p1) > 20);
});

test('o Mover segue o caminho sem sair da área livre e chega', () => {
  const a = POI_BY_ID['outside'];
  const b = POI_BY_ID['sofa-a-lie'];
  const allowed = grid.allowedFor('sofa-a-lie');
  const path = pf.find(a.x, a.z, b.x, b.z, { allowed });
  const m = new Mover({ x: a.x, z: a.z, maxSpeed: 1.3 });
  m.setPath(path);
  let state = 'moving';
  let t = 0;
  while (state !== 'arrived' && t < 120) {
    state = m.step(0.1);
    t += 0.1;
    assert.ok(grid.isFreeAt(m.x, m.z, allowed), `saiu da área livre em (${m.x.toFixed(2)}, ${m.z.toFixed(2)})`);
  }
  assert.equal(state, 'arrived');
  assert.ok(Math.hypot(m.x - b.x, m.z - b.z) < 1e-6);
  // Velocidade realista: nem teletransporte nem lentidão absurda.
  const len = pathLength(path);
  assert.ok(t > len / 1.3, 'chegou rápido demais');
  assert.ok(t < len / 0.4, 'demorou demais');
});

test('bloqueio dinâmico desvia de outra entidade no corredor', () => {
  const a = POI_BY_ID['wander-3'];
  const b = POI_BY_ID['center'];
  const direct = pf.find(a.x, a.z, b.x, b.z);
  // Uma "entidade" parada no meio do caminho direto.
  const mid = direct[Math.floor(direct.length / 2)] ?? direct[1];
  const ox = (direct[0].x + direct[direct.length - 1].x) / 2;
  const oz = (direct[0].z + direct[direct.length - 1].z) / 2;
  const blocked = (i) => {
    const c = grid.centerOf(i);
    return Math.hypot(c.x - ox, c.z - oz) < 0.6;
  };
  const around = pf.find(a.x, a.z, b.x, b.z, { blocked });
  assert.ok(around, 'deveria achar um desvio');
  for (const p of around) assert.ok(Math.hypot(p.x - ox, p.z - oz) >= 0.55 || p === around[0] || p === around[around.length - 1]);
  assert.ok(mid);
});

test('as quatro mesas existem e cada uma tem assento', () => {
  const desks = POIS.filter((p) => p.kind === 'desk');
  assert.equal(desks.length, 4);
  assert.equal(DESK_PODS.length * 2, 4);
  for (const d of desks) assert.ok(grid.seatBit.has(d.id));
  assert.ok(WALLS.some((w) => w.kind === 'glass'));
});
