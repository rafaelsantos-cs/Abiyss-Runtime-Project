import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { DatabaseSync } from 'node:sqlite';

import { Janela, fasePeloRelogio, minutoDeTexto } from '../server/dsr/ritmo.js';
import { RuntimeSqliteSource, KNOWN_SCHEMA } from '../server/dsr/runtime-sqlite.js';
import { DemoRuntimeSource } from '../server/dsr/demo-runtime.js';
import { RuntimeBridge } from '../server/dsr/bridge.js';
import { validateSnapshot, goalEmFoco, emptySnapshot } from '../server/dsr/snapshot.js';
import { GOAL_TRANSICOES, SUBAGENTE_ESTADOS } from '../shared/states.js';
import { readRuntimeHorasAtivas } from '../server/config.js';

const FIXTURE = new URL('./fixtures/runtime-migrations.sql', import.meta.url);
const TZ = 'America/Sao_Paulo';
const IMMOS = ['immo-1', 'immo-2', 'immo-3', 'immo-4'];

// ---------------------------------------------------------------------------
// ritmo.rs — os mesmos casos dos testes do kernel.
// ---------------------------------------------------------------------------

test('ritmo: janelas com e sem meia-noite (paridade com ritmo.rs)', () => {
  const dia = Janela.deTexto('07:00-23:00');
  assert.ok(!dia.contem(6 * 60 + 59));
  assert.ok(dia.contem(7 * 60));
  assert.ok(dia.contem(22 * 60 + 59));
  assert.ok(!dia.contem(23 * 60));
  const noite = Janela.deTexto('22:00-06:00');
  assert.ok(noite.contem(23 * 60));
  assert.ok(noite.contem(0));
  assert.ok(noite.contem(5 * 60 + 59));
  assert.ok(!noite.contem(6 * 60));
  assert.ok(!noite.contem(12 * 60));
  const sempre = Janela.deTexto('00:00-00:00');
  assert.ok(sempre.contem(0) && sempre.contem(13 * 60));
  assert.throws(() => Janela.deTexto('7h-23h'));
  assert.throws(() => Janela.deTexto('24:00-01:00'));
  assert.throws(() => Janela.deTexto('07:60-08:00'));
  assert.equal(minutoDeTexto(' 09:30 '), 570);
});

test('ritmo: fase pelo relógio', () => {
  assert.equal(fasePeloRelogio('07:00-23:00', 9 * 60), 'vigília');
  assert.equal(fasePeloRelogio('07:00-23:00', 3 * 60), 'descanso');
});

test('lê horas_ativas da seção [ritmo] de um abiyss.toml', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'office-toml-'));
  const f = path.join(dir, 'abiyss.toml');
  fs.writeFileSync(f, '[daemon]\nhoras_ativas = "01:00-02:00"\n\n[ritmo]\n# comentário\nhoras_ativas = "08:30-22:00"  # fim\n');
  assert.equal(readRuntimeHorasAtivas(f), '08:30-22:00');
  assert.equal(readRuntimeHorasAtivas(path.join(dir, 'nao-existe.toml')), null);
});

// ---------------------------------------------------------------------------
// Adaptador do banco real (esquema copiado do kernel).
// ---------------------------------------------------------------------------

function createRuntimeDb(file, upTo = KNOWN_SCHEMA) {
  const sql = fs.readFileSync(FIXTURE, 'utf8');
  const blocks = sql.split(/^-- @migracao \d+$/m).slice(1);
  assert.equal(blocks.length, KNOWN_SCHEMA, 'fixture deve ter todas as migrações conhecidas');
  const db = new DatabaseSync(file);
  db.exec('PRAGMA journal_mode = WAL');
  blocks.slice(0, upTo).forEach((b, i) => {
    db.exec('BEGIN');
    db.exec(b);
    db.exec(`PRAGMA user_version = ${i + 1}`);
    db.exec('COMMIT');
  });
  return db;
}

function seedRuntimeDb(db, now) {
  const ins = (sql, ...args) => db.prepare(sql).run(...args);
  ins("INSERT INTO estado_daemon (chave, valor) VALUES ('pid', '999'), ('iniciado_ms', ?), ('sinal_de_vida_ms', ?)", String(now - 600_000), String(now - 10_000));
  ins("INSERT INTO goals (titulo, nucleo, prioridade, estado, criado_ms, atualizado_ms) VALUES ('Goal A', 'n', 1, 'comprometido', ?, ?)", now, now);
  ins("INSERT INTO goals (titulo, nucleo, prioridade, estado, criado_ms, atualizado_ms) VALUES ('Goal B', 'n', 0, 'executando', ?, ?)", now, now);
  ins("INSERT INTO goals (titulo, nucleo, prioridade, estado, criado_ms, atualizado_ms) VALUES ('Velho', 'n', 5, 'concluido', ?, ?)", now - 3 * 86400_000, now - 3 * 86400_000);
  const sub = (nivel, estado, criado, iniciado, terminado, relatorio) => ins(
    `INSERT INTO subagentes (nivel, tarefa, contexto, prazo_segundos, goal_id, origem, estado, criado_ms, iniciado_ms, terminado_ms, relatorio)
     VALUES (?, ?, '', 600, 2, 'heartbeat', ?, ?, ?, ?, ?)`,
    nivel, `tarefa ${nivel} ${estado}`, estado, criado, iniciado, terminado, relatorio,
  );
  sub('ultra', 'executando', now - 60_000, now - 55_000, null, null);
  sub('low', 'pendente', now - 5_000, null, null, null);
  sub('medium', 'concluido', now - 300_000, now - 290_000, now - 120_000, JSON.stringify({ status: 'concluido', resumo: 'ok', artefatos: [], confianca: 0.9, duvidas: [] }));
  sub('low', 'concluido', now - 3 * 3600_000, now - 3 * 3600_000, now - 3 * 3600_000 + 5000, null); // fora da janela
  ins(`INSERT INTO ciclos (inicio_ms, fim_ms, chamou_modelo, motivo, goal_foco, resultado, tokens)
       VALUES (?, ?, 1, '1 evento(s) novo(s) na fila', 2, 'sub-agente 1 (ultra) delegado', 4000)`, now - 70_000, now - 62_000);
  ins("INSERT INTO fila_eventos (momento_ms, tipo, origem, conteudo, consumido_ms) VALUES (?, 'cron', 'bom-dia', 'acorde', ?)", now - 80_000, now - 62_000);
  ins("INSERT INTO fila_eventos (momento_ms, tipo, origem, conteudo, origem_externa) VALUES (?, 'subagente', '3', '{}', 'subagente:3')", now - 120_000);
  ins(`INSERT INTO chamadas_modelo (momento_ms, pool, origem, modelo, tentativa, status, duracao_ms)
       VALUES (?, 'cerebro', 'conversa', 'm', 1, 'ok', 800)`, now - 20_000);
}

function sha256(file) {
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

test('adaptador lê o banco do runtime (esquema real) sem escrever nele', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'office-db-'));
  const file = path.join(dir, 'abiyss.db');
  const now = Date.parse('2026-10-05T14:00:00-03:00');
  const db = createRuntimeDb(file);
  seedRuntimeDb(db, now);
  db.exec('PRAGMA wal_checkpoint(TRUNCATE)');
  const before = sha256(file);

  const src = new RuntimeSqliteSource({ dbPath: file, horasAtivas: '07:00-23:00', timeZone: TZ });
  await src.start();
  const snap = src.poll(now);
  assert.equal(snap.ok, true, snap.error);
  assert.deepEqual(validateSnapshot(snap), []);
  assert.equal(snap.runtimeSchemaVersion, KNOWN_SCHEMA);
  assert.equal(snap.daemon.estado, 'rodando');
  assert.equal(snap.daemon.pid, 999);
  assert.equal(snap.ritmo.fase, 'vigília');
  assert.deepEqual(snap.subagentes.map((s) => s.estado), ['concluido', 'pendente', 'executando'], 'ordem por id decrescente');
  assert.equal(snap.subagentes.length, 3, 'o concluído de 3 h atrás fica fora da janela');
  const concl = snap.subagentes.find((s) => s.estado === 'concluido');
  assert.deepEqual(concl.relatorio, { status: 'concluido', resumo: 'ok', confianca: 0.9 });
  assert.equal(snap.goals.foco.titulo, 'Goal B', 'executando vem antes de comprometido');
  assert.equal(snap.goals.lista.length, 2, 'goal concluído há 3 dias fica fora');
  assert.deepEqual(snap.goals.contagem, { comprometido: 1, executando: 1, concluido: 1 });
  assert.equal(snap.ciclos.length, 1);
  assert.equal(snap.ciclos[0].chamouModelo, true);
  assert.equal(snap.eventos.pendentes, 1);
  assert.equal(snap.eventos.recentes[0].tipo, 'subagente');
  assert.equal(snap.conversa.ultimaChamadaMs, now - 20_000);

  // O adaptador recusa escrita (somente leitura) e não alterou o arquivo.
  assert.throws(() => src.db.exec("INSERT INTO estado_daemon (chave, valor) VALUES ('x', 'y')"));
  src.stop();
  db.close();
  assert.equal(sha256(file), before, 'o arquivo do banco não pode mudar');
});

test('adaptador: daemon parado, sono pelas chamadas e banco ausente', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'office-db-'));
  const file = path.join(dir, 'abiyss.db');
  const now = Date.parse('2026-10-05T03:20:00-03:00');
  const db = createRuntimeDb(file);
  db.prepare("INSERT INTO estado_daemon (chave, valor) VALUES ('iniciado_ms', ?), ('sinal_de_vida_ms', ?), ('parado_ms', ?)")
    .run(String(now - 9000_000), String(now - 5000_000), String(now - 4000_000));
  db.prepare("INSERT INTO chamadas_modelo (momento_ms, pool, origem, modelo, tentativa, status, duracao_ms) VALUES (?, 'cerebro', 'sono', 'm', 1, 'ok', 1)").run(now - 30_000);
  const src = new RuntimeSqliteSource({ dbPath: file, horasAtivas: '07:00-23:00', timeZone: TZ });
  await src.start();
  const snap = src.poll(now);
  assert.equal(snap.ok, true, snap.error);
  assert.equal(snap.daemon.estado, 'parado');
  assert.equal(snap.ritmo.fase, 'sono');
  assert.equal(snap.sono.ativo, true);
  src.stop();
  db.close();

  const missing = new RuntimeSqliteSource({ dbPath: path.join(dir, 'nada.db'), horasAtivas: '07:00-23:00', timeZone: TZ });
  await missing.start();
  const s2 = missing.poll(now);
  assert.equal(s2.ok, false);
  assert.match(s2.error, /não encontrado/);
  assert.equal(s2.ritmo.fase, 'descanso', 'mesmo sem banco, a fase segue o relógio');
});

test('adaptador recusa banco antigo demais', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'office-db-'));
  const file = path.join(dir, 'abiyss.db');
  createRuntimeDb(file, 2).close();
  const src = new RuntimeSqliteSource({ dbPath: file, horasAtivas: '07:00-23:00', timeZone: TZ });
  await src.start();
  const snap = src.poll(Date.now());
  assert.equal(snap.ok, false);
  assert.match(snap.error, /migração 2/);
});

// ---------------------------------------------------------------------------
// Ponte (eventos de domínio e atribuição de IMMos).
// ---------------------------------------------------------------------------

function snapWith(subs, extra = {}) {
  const s = emptySnapshot('teste');
  s.ok = true;
  s.capturedAtMs = extra.now ?? 1000;
  s.daemon.estado = 'rodando';
  s.subagentes = subs;
  Object.assign(s, extra.patch ?? {});
  return s;
}
const sub = (id, estado, nivel = 'low', extra = {}) => ({
  id, estado, nivel, tarefa: `t${id}`, goalId: null, origem: 'heartbeat', criadoMs: 0, iniciadoMs: 0,
  terminadoMs: estado === 'executando' || estado === 'pendente' ? null : 900, prazoSegundos: 600,
  relatorio: null, tokens: 0, rodadas: 0, ...extra,
});

test('ponte: atribuição determinística e justa, liberação e transbordo', () => {
  const b = new RuntimeBridge({ immoIds: IMMOS });
  b.ingest(snapWith([]));
  let ev = b.ingest(snapWith([sub(1, 'executando'), sub(2, 'pendente')]));
  assert.ok(ev.some((e) => e.type === 'subagente.atribuido' && e.immoId === 'immo-1' && e.subId === 1));
  ev = b.ingest(snapWith([sub(1, 'concluido'), sub(2, 'executando')]));
  assert.ok(ev.some((e) => e.type === 'subagente.terminado' && e.immoId === 'immo-1'));
  assert.ok(ev.some((e) => e.type === 'subagente.atribuido' && e.immoId === 'immo-2'), 'o IMMo-1 ainda está com o relatório');
  assert.equal(b.taskOf('immo-1').estado, 'concluido');
  b.release('immo-1', 1);
  assert.equal(b.taskOf('immo-1'), null);
  // Rodízio: o próximo vai para quem está há mais tempo sem tarefa (immo-3).
  ev = b.ingest(snapWith([sub(2, 'executando'), sub(3, 'executando')]));
  assert.ok(ev.some((e) => e.type === 'subagente.atribuido' && e.immoId === 'immo-3'));
  // Transbordo: mais sub-agentes em execução do que IMMos.
  const many = [2, 3, 4, 5, 6].map((i) => sub(i, 'executando'));
  ev = b.ingest(snapWith(many));
  assert.deepEqual(b.overflow, [6]);
  ev = b.ingest(snapWith([sub(2, 'executando'), sub(3, 'falhou'), sub(4, 'executando'), sub(5, 'executando'), sub(6, 'executando')]));
  assert.ok(ev.some((e) => e.type === 'subagente.atribuido' && e.subId === 6), 'o IMMo do sub-agente que falhou assume o transbordo');
  assert.deepEqual(b.overflow, []);
});

test('ponte: primeiro instantâneo não reencena o passado e cursores andam', () => {
  const b = new RuntimeBridge({ immoIds: IMMOS });
  const first = snapWith([sub(10, 'concluido'), sub(11, 'executando', 'ultra')], {
    patch: { ciclos: [{ id: 5, chamouModelo: true, motivo: 'm' }], eventos: { pendentes: 0, recentes: [{ id: 9, tipo: 'cron' }] } },
  });
  const ev = b.ingest(first);
  assert.ok(!ev.some((e) => e.type === 'subagente.terminado'), 'o concluído antigo não vira relatório');
  assert.ok(ev.some((e) => e.type === 'subagente.atribuido' && e.subId === 11 && e.bootstrap));
  assert.ok(!ev.some((e) => e.type === 'ciclo'));
  const next = snapWith([sub(11, 'executando', 'ultra')], {
    patch: { ciclos: [{ id: 6, chamouModelo: true, motivo: 'n' }, { id: 5 }], eventos: { pendentes: 1, recentes: [{ id: 10, tipo: 'subagente' }, { id: 9 }] } },
  });
  const ev2 = b.ingest(next);
  assert.deepEqual(ev2.filter((e) => e.type === 'ciclo').map((e) => e.ciclo.id), [6]);
  assert.deepEqual(ev2.filter((e) => e.type === 'evento').map((e) => e.evento.id), [10]);
});

test('ponte: falha da fonte mantém o último estado; persistência por fonte', () => {
  const b = new RuntimeBridge({ immoIds: IMMOS });
  b.reset('fonte-a');
  b.ingest(snapWith([sub(1, 'executando')]));
  const bad = emptySnapshot('teste');
  bad.error = 'banco travado';
  const ev = b.ingest(bad);
  assert.ok(ev.some((e) => e.type === 'fonte.erro'));
  assert.equal(b.taskOf('immo-1').id, 1, 'atribuição sobrevive a uma leitura falha');
  const saved = JSON.parse(JSON.stringify(b.toJSON()));
  const c = new RuntimeBridge({ immoIds: IMMOS });
  assert.equal(c.restore(saved, 'fonte-a'), true);
  assert.equal(c.taskOf('immo-1').id, 1);
  const d = new RuntimeBridge({ immoIds: IMMOS });
  assert.equal(d.restore(saved, 'outra-fonte'), false);
  assert.equal(d.taskOf('immo-1'), null);
});

// ---------------------------------------------------------------------------
// Emulador demo.
// ---------------------------------------------------------------------------

function runDemo(seed, seconds, start = Date.parse('2026-10-05T10:00:00-03:00')) {
  const demo = new DemoRuntimeSource({ seed, horasAtivas: '07:00-23:00', timeZone: TZ });
  const snaps = [];
  for (let t = 0; t <= seconds; t++) snaps.push(demo.poll(start + t * 1000));
  return snaps;
}

test('demo: determinístico, estados válidos e no máximo 4 em execução', () => {
  const a = runDemo(7, 1200);
  const b = runDemo(7, 1200);
  assert.deepEqual(a.at(-1), b.at(-1), 'mesma semente → mesmo estado');
  const c = runDemo(8, 1200);
  assert.notDeepEqual(a.at(-1).subagentes, c.at(-1).subagentes);
  let worked = 0;
  for (const s of a) {
    assert.deepEqual(validateSnapshot(s), []);
    const exec = s.subagentes.filter((x) => x.estado === 'executando').length;
    assert.ok(exec <= 4);
    for (const x of s.subagentes) assert.ok(SUBAGENTE_ESTADOS.includes(x.estado));
    worked = Math.max(worked, exec);
  }
  assert.ok(worked >= 2, 'a demo deveria ter mais de um sub-agente ao mesmo tempo');
  const last = a.at(-1);
  assert.ok(last.ciclos.length > 0);
  assert.ok(last.subagentes.some((x) => x.estado === 'concluido'));
});

test('demo: abertura roteirizada delega ultra+medium e depois low', () => {
  const snaps = runDemo(7, 40);
  const at15 = snaps[15].subagentes;
  assert.deepEqual(at15.map((s) => s.nivel).sort(), ['medium', 'ultra']);
  assert.ok(snaps[40].subagentes.some((s) => s.nivel === 'low'));
});

test('demo: goals só fazem transições permitidas pelo kernel', () => {
  const demo = new DemoRuntimeSource({ seed: 3, horasAtivas: '07:00-23:00', timeZone: TZ });
  const start = Date.parse('2026-10-05T09:00:00-03:00');
  const last = new Map();
  for (let t = 0; t <= 3600; t += 2) {
    const s = demo.poll(start + t * 1000);
    for (const g of s.goals.lista) {
      const prev = last.get(g.id);
      if (prev && prev !== g.estado) {
        assert.ok(GOAL_TRANSICOES[prev].includes(g.estado), `transição proibida ${prev} → ${g.estado}`);
      }
      last.set(g.id, g.estado);
    }
    assert.equal(s.goals.foco?.id ?? null, goalEmFoco(s.goals.lista)?.id ?? null);
  }
});

test('demo: sono pausa o heartbeat e muda a fase', () => {
  const snaps = runDemo(5, 900, Date.parse('2026-10-05T03:05:00-03:00'));
  assert.equal(snaps[10].ritmo.fase, 'sono');
  assert.equal(snaps[10].sono.ativo, true);
  // Durante o sono não há ciclos com chamada ao modelo depois da abertura.
  const late = snaps.at(-1).ciclos.filter((c) => c.inicioMs > Date.parse('2026-10-05T03:06:00-03:00'));
  assert.ok(late.every((c) => !c.chamouModelo));
});
