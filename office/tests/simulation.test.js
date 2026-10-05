import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { Simulation } from '../server/world/simulation.js';
import { DemoRuntimeSource } from '../server/dsr/demo-runtime.js';
import { emptySnapshot } from '../server/dsr/snapshot.js';
import { saveState, loadState } from '../server/world/persistence.js';
import { configureLog } from '../server/log.js';
import { POI_BY_ID } from '../shared/layout.js';

configureLog({ level: 'silent' });

const TZ = 'America/Sao_Paulo';

function demoSim({ seed = 7, start = '2026-10-05T10:00:00-03:00', fresh = true } = {}) {
  const source = new DemoRuntimeSource({ seed, horasAtivas: '07:00-23:00', timeZone: TZ });
  const sim = new Simulation({ source, seed: 20260927, timeZone: TZ, clockStartMs: Date.parse(start) });
  if (fresh) sim.world.startFresh({ demoOpening: true });
  return sim;
}

/** Fonte de teste: o teste controla o instantâneo. */
class ScriptedSource {
  constructor() {
    this.name = 'roteiro';
    this.snap = emptySnapshot('roteiro');
    this.snap.ok = true;
    this.snap.daemon.estado = 'rodando';
  }
  get key() { return 'roteiro'; }
  poll(now) { return { ...structuredClone(this.snap), capturedAtMs: now }; }
  status() { return { source: 'roteiro', connected: true }; }
}

function scriptedSim(start = '2026-10-05T10:00:00-03:00') {
  const source = new ScriptedSource();
  const sim = new Simulation({ source, seed: 11, timeZone: TZ, clockStartMs: Date.parse(start) });
  return { sim, source };
}

const sub = (id, estado, nivel = 'medium') => ({
  id, estado, nivel, tarefa: `tarefa ${id}`, goalId: null, origem: 'heartbeat', criadoMs: 0, iniciadoMs: 0,
  terminadoMs: ['pendente', 'executando'].includes(estado) ? null : Date.now(), prazoSegundos: 600,
  relatorio: estado === 'concluido' ? { status: 'concluido', resumo: 'ok', confianca: 0.8 } : null, tokens: 0, rodadas: 0,
});

test('soak de 2 h (demo): sem atravessar paredes, sem travar e com comportamento variado', () => {
  const sim = demoSim();
  const w = sim.world;
  const seen = new Map(w.immos().map((m) => [m.id, new Set()]));
  const returnedToDesk = new Set();
  let allSame = 0;
  let samples = 0;
  const abiyssActs = new Set();
  const abiyssMinds = new Set();
  sim.run(7200, (world, i) => {
    for (const m of world.immos()) {
      if (!m.present) continue;
      seen.get(m.id).add(m.activity);
      if (m.poi === m.desk && ['working', 'seated_idle'].includes(m.activity) && seen.get(m.id).has('resting')) returnedToDesk.add(m.id);
    }
    abiyssActs.add(world.abiyss.activity);
    if (world.abiyss.mind) abiyssMinds.add(world.abiyss.mind.kind);
    if (i % 10 === 0 && world.simMs > 60_000) {
      samples++;
      const acts = world.immos().map((m) => m.activity);
      if (acts.every((a) => a === acts[0]) && acts[0] !== 'working') allSame++;
    }
  });
  const st = w.view().stats;
  assert.equal(st.violations, 0, `invariante violada: ${JSON.stringify(w.lastViolation)}`);
  assert.equal(st.recoveries, 0, 'nenhum teletransporte de recuperação');
  assert.equal(st.stuck, 0, 'ninguém fica travado');
  assert.equal(st.overlaps, 0, `entidades se sobrepondo: ${JSON.stringify(w.lastOverlap)}`);
  for (const [id, acts] of seen) {
    assert.ok(acts.has('working'), `${id} nunca trabalhou`);
    assert.ok(acts.has('walking') || acts.has('wandering'), `${id} nunca andou`);
    assert.ok(acts.size >= 6, `${id} com pouca variedade: ${[...acts]}`);
  }
  const anyRest = [...seen.values()].some((s) => s.has('resting'));
  assert.ok(anyRest, 'alguém descansou no sofá/pufe');
  assert.ok(returnedToDesk.size >= 1, 'alguém voltou à mesa depois de descansar');
  assert.ok(allSame / samples < 0.05, `todos fazendo a mesma coisa em ${(100 * allSame / samples).toFixed(1)}% do tempo`);
  for (const a of ['supervising', 'idle', 'moving']) assert.ok(abiyssActs.has(a), `Abiyss nunca ficou ${a}`);
  for (const m of ['thinking', 'delegating', 'receiving']) assert.ok(abiyssMinds.has(m), `Abiyss nunca mostrou ${m}`);
});

test('abertura da demo: entram pela porta, trabalham, e o livre passeia, descansa no sofá e volta', () => {
  const sim = demoSim();
  const w = sim.world;
  const first = new Map();
  const seq = [];
  const worked = new Set();
  sim.run(240, (world) => {
    for (const m of world.immos()) {
      if (m.present && !first.has(m.id)) first.set(m.id, { x: m.x, z: m.z });
      if (m.activity === 'working') worked.add(m.id);
    }
    const free = world.agent('immo-4');
    if (free.present && seq.at(-1) !== free.activity) seq.push(free.activity);
  });
  for (const [id, p] of first) assert.ok(p.z > 16, `${id} deveria surgir na calçada (z=${p.z})`);
  assert.ok(worked.size >= 3, `três IMMos deveriam trabalhar na abertura: ${[...worked]}`);
  assert.equal(w.view().stats.violations, 0);
  // Sequência do IMMo livre: entra → passeia → descansa (sofá) → volta à mesa.
  const iWander = seq.indexOf('wandering');
  const iRest = seq.indexOf('resting', iWander);
  const iBack = seq.findIndex((a, i) => i > iRest && (a === 'seated_idle' || a === 'working'));
  assert.ok(seq[0] === 'entering', `começa entrando: ${seq.join(' → ')}`);
  assert.ok(iWander > 0 && iRest > iWander && iBack > iRest, `sequência inesperada: ${seq.join(' → ')}`);
});

test('à noite (descanso) os IMMos livres dormem; de dia acordam', () => {
  const sim = demoSim({ start: '2026-10-06T06:30:00-03:00' });
  sim.run(1500); // até 06:55, ainda descanso
  const sleeping = sim.world.immos().filter((m) => m.activity === 'sleeping');
  assert.ok(sleeping.length >= 2, `poucos dormindo: ${sim.world.immos().map((m) => m.activity)}`);
  assert.equal(sim.world.runtime.ritmo.fase, 'descanso');
  sim.run(600); // 07:05: vigília há 5 min (cada um acorda com até 90 s de atraso)
  assert.equal(sim.world.runtime.ritmo.fase, 'vigília');
  const still = sim.world.immos().filter((m) => m.activity === 'sleeping');
  assert.equal(still.length, 0, `ainda dormindo na vigília: ${still.map((m) => m.id)}`);
});

test('sono do runtime: Abiyss dorme no pedestal e o heartbeat para', () => {
  const sim = demoSim({ start: '2026-10-05T03:05:00-03:00' });
  sim.run(400);
  assert.equal(sim.world.runtime.ritmo.fase, 'sono');
  assert.equal(sim.world.abiyss.activity, 'sleeping');
  assert.equal(sim.world.abiyss.poi, 'abiyss-home');
});

test('daemon parado: Abiyss desliga no console e IMMos não trabalham', () => {
  const { sim, source } = scriptedSim();
  source.snap.daemon.estado = 'parado';
  sim.run(120);
  assert.equal(sim.world.abiyss.activity, 'offline');
  assert.ok(sim.world.immos().every((m) => m.activity !== 'working'));
});

test('tarefa acorda o IMMo, ele vai à própria mesa, trabalha e entrega o relatório', () => {
  const { sim, source } = scriptedSim('2026-10-05T02:00:00-03:00');
  source.snap.ritmo.fase = 'descanso';
  sim.run(400);
  const w = sim.world;
  assert.ok(w.immos().some((m) => m.activity === 'sleeping'), 'alguém dormindo antes da tarefa');
  source.snap.subagentes = [sub(1, 'executando', 'ultra')];
  sim.run(90);
  const worker = w.immos().find((m) => m.taskId === 1);
  assert.ok(worker, 'algum IMMo assumiu o sub-agente');
  assert.equal(worker.activity, 'working');
  assert.equal(worker.poi, worker.desk, 'trabalha na própria mesa');
  const desk = POI_BY_ID[worker.desk];
  assert.ok(Math.hypot(worker.x - desk.x, worker.z - desk.z) < 0.05);
  source.snap.subagentes = [sub(1, 'concluido', 'ultra')];
  let reported = false;
  sim.run(120, (world) => { if (worker.activity === 'reporting') reported = true; });
  assert.ok(reported, 'levou o relatório ao Abiyss');
  assert.equal(w.bridge.taskOf(worker.id), null, 'liberado depois de entregar');
});

test('mesma semente → mesma simulação (determinismo)', () => {
  const a = demoSim();
  const b = demoSim();
  a.run(600);
  b.run(600);
  const pa = a.world.agents.map((x) => [x.id, x.x.toFixed(4), x.z.toFixed(4), x.activity]);
  const pb = b.world.agents.map((x) => [x.id, x.x.toFixed(4), x.z.toFixed(4), x.activity]);
  assert.deepEqual(pa, pb);
});

test('reinício: estado salvo restaura posições válidas e as atribuições', () => {
  const { sim, source } = scriptedSim();
  source.snap.subagentes = [sub(5, 'executando', 'low')];
  sim.run(60);
  const worker = sim.world.immos().find((m) => m.taskId === 5);
  assert.ok(worker);
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'office-state-'));
  const file = path.join(dir, 'office-state.json');
  sim.world.bridge.sourceKey = 'roteiro';
  saveState(file, sim.world.toJSON());

  const again = scriptedSim();
  again.source.snap.subagentes = [sub(5, 'executando', 'low')];
  assert.equal(again.sim.world.restore(loadState(file), 'roteiro'), true);
  again.sim.run(40);
  const w2 = again.sim.world;
  assert.equal(w2.view().stats.violations, 0);
  const sameWorker = w2.immos().find((m) => m.id === worker.id);
  assert.equal(sameWorker.taskId, 5, 'o mesmo IMMo continua com o mesmo sub-agente');
  assert.equal(sameWorker.activity, 'working');

  // Arquivo corrompido não impede a subida.
  fs.writeFileSync(file, '{ quebrado');
  assert.equal(loadState(file), null);
  assert.ok(fs.existsSync(`${file}.corrompido`));
});
