#!/usr/bin/env node
// Teste de integração com o RUNTIME REAL do Abiyss (kernel em Rust).
//
// Sobe scripts/runtime-live.sh (mock-nim + daemon real + escritório em modo
// runtime), abre o cliente num Chromium headless e verifica, com asserções:
//   1. o escritório lê o banco real (migração conhecida, daemon "rodando");
//   2. um IMMo assume um sub-agente que EXISTE no banco como "executando",
//      vai à própria mesa e trabalha;
//   3. quando o sub-agente termina no banco, o IMMo leva o relatório ao Abiyss;
//   4. ciclos reais do heartbeat aparecem como "pensando" no Abiyss;
//   5. ao parar o daemon real (SIGTERM), o escritório mostra "parado" e o
//      Abiyss desliga.
// Gera screenshots 12–15 e artifacts/screenshots/runtime-integration.json.
//
// Uso: ABIYSS_BIN=/caminho/target/release/abiyss node scripts/runtime-integration.mjs

import { spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';

import { ROOT, launchBrowser, waitForState } from './lib/harness.mjs';

const BIN = process.env.ABIYSS_BIN ?? (process.argv.includes('--binario') ? process.argv[process.argv.indexOf('--binario') + 1] : null);
if (!BIN || !fs.existsSync(BIN)) {
  console.error('defina ABIYSS_BIN (ou --binario) com o binário do runtime (cargo build --release no repositório do runtime)');
  process.exit(2);
}
const OUT = path.join(ROOT, 'artifacts', 'screenshots');
fs.mkdirSync(OUT, { recursive: true });
const log = (m) => process.stdout.write(`${new Date().toISOString().slice(11, 19)} ${m}\n`);
const results = [];
const check = (name, ok, detail = '') => {
  results.push({ name, ok, detail });
  log(`${ok ? '✔' : '✘'} ${name}${detail ? ` — ${detail}` : ''}`);
  if (!ok) process.exitCode = 1;
};

const freePort = () => new Promise((r) => {
  const s = net.createServer();
  s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => r(p)); });
});

const port = await freePort();
const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'abiyss-office-it-'));
const live = spawn('bash', [path.join(ROOT, 'scripts', 'runtime-live.sh'), '--binario', BIN, '--pasta', dir, '--porta', String(port), '--atraso-ms', '15000', '--heartbeat', '8', '--delegar-cada', '10'], {
  stdio: ['ignore', 'pipe', 'pipe'], detached: true,
});
let daemonPid = null;
let output = '';
const onData = (d) => {
  output += d.toString();
  const m = output.match(/daemon pid (\d+)/);
  if (m) daemonPid = Number(m[1]);
};
live.stdout.on('data', onData);
live.stderr.on('data', onData);

async function waitHttp() {
  for (let i = 0; i < 120; i++) {
    try {
      const r = await fetch(`http://127.0.0.1:${port}/api/health`);
      if (r.ok) return;
    } catch { /* ainda subindo */ }
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error(`escritório não subiu:\n${output}`);
}

const dbFile = path.join(dir, 'data', 'abiyss.db');
const dbQuery = (sql, ...args) => {
  const db = new DatabaseSync(dbFile, { readOnly: true });
  try { return db.prepare(sql).all(...args); } finally { db.close(); }
};

const browser = await launchBrowser();
try {
  await waitHttp();
  const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });
  await page.goto(`http://127.0.0.1:${port}/?labels=2`);
  await page.waitForFunction(() => window.__office && window.__office.ready, null, { timeout: 60000 });

  // 1. Fonte real.
  const st0 = await (await fetch(`http://127.0.0.1:${port}/api/state`)).json();
  check('lê o banco real do runtime', st0.runtime.source === 'runtime-sqlite' && st0.runtime.ok, `migração ${st0.runtime.runtimeSchemaVersion}`);
  await waitForState(page, `(s) => s.meta && s.meta.runtime.daemon === 'rodando'`, { label: 'daemon rodando', timeoutMs: 30000 });
  check('daemon real aparece como rodando', true);

  // 2. IMMo trabalhando num sub-agente que existe no banco como executando.
  await page.evaluate(() => window.__office.preset('desks'));
  await waitForState(page, `(s) => s.entities.some((e) => e.act === 'working' && e.logical && e.logical.estado === 'executando')`, { label: 'IMMo trabalhando', timeoutMs: 120000 });
  const working = await page.evaluate(() => window.__office.state().entities.find((e) => e.act === 'working' && e.logical?.estado === 'executando'));
  const row = dbQuery('SELECT id, nivel, estado FROM subagentes WHERE id = ?', working.logical.subagenteId)[0];
  check('IMMo trabalha num sub-agente real', !!row && ['executando', 'concluido'].includes(row.estado) && row.nivel === working.logical.nivel,
    `${working.id} ↔ subagentes.id=${row?.id} (${row?.nivel}, banco: ${row?.estado})`);
  check('IMMo trabalha na própria mesa', working.poi === `desk-${working.id.split('-')[1]}`, working.poi);
  await page.waitForTimeout(1500);
  await page.screenshot({ path: path.join(OUT, '13-runtime-real-mesas.png') });
  await page.evaluate(() => window.__office.preset('overview'));
  await page.waitForTimeout(2000);
  await page.screenshot({ path: path.join(OUT, '12-runtime-real-visao-geral.png') });

  // 3. Relatório depois que o banco marca o fim.
  await page.evaluate(() => window.__office.preset('abiyss'));
  await waitForState(page, `(s) => s.entities.some((e) => e.act === 'reporting')`, { label: 'relatório ao Abiyss', timeoutMs: 120000 });
  const rep = await page.evaluate(() => window.__office.state().entities.find((e) => e.act === 'reporting'));
  const repRow = dbQuery('SELECT estado, relatorio FROM subagentes WHERE id = ?', rep.logical.subagenteId)[0];
  check('relatório só depois do fim no banco', !!repRow && ['concluido', 'falhou', 'expirado'].includes(repRow.estado), `subagentes.id=${rep.logical.subagenteId} → ${repRow?.estado}`);
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(OUT, '14-runtime-real-relatorio.png') });

  // 4. Heartbeat real → "pensando".
  await waitForState(page, `(s) => { const a = s.entities.find((e) => e.kind === 'abiyss'); return a && a.mind === 'thinking'; }`, { label: 'Abiyss pensando', timeoutMs: 120000 });
  const ciclos = dbQuery('SELECT COUNT(*) AS n FROM ciclos WHERE chamou_modelo = 1')[0].n;
  check('ciclos reais do heartbeat viram "pensando"', ciclos > 0, `${ciclos} ciclo(s) com chamada ao modelo no banco`);

  // 5. Parar o daemon real.
  check('PID do daemon conhecido', !!daemonPid, String(daemonPid));
  process.kill(daemonPid, 'SIGTERM');
  await page.evaluate(() => window.__office.preset('overview'));
  await waitForState(page, `(s) => s.meta && s.meta.runtime.daemon === 'parado'`, { label: 'daemon parado', timeoutMs: 30000 });
  await waitForState(page, `(s) => s.entities.find((e) => e.kind === 'abiyss').act === 'offline'`, { label: 'Abiyss desligado', timeoutMs: 60000 });
  check('daemon parado → Abiyss desliga', true);
  await page.waitForTimeout(2000);
  await page.screenshot({ path: path.join(OUT, '15-runtime-real-daemon-parado.png') });

  const finalState = await (await fetch(`http://127.0.0.1:${port}/api/state`)).json();
  check('nenhuma invariante violada', finalState.stats.violations === 0 && finalState.stats.recoveries === 0, JSON.stringify({ v: finalState.stats.violations, r: finalState.stats.recoveries, stuck: finalState.stats.stuck }));
  check('leitura do runtime sem erros até o fim', finalState.runtime.ok !== false && !finalState.runtime.error, 'conexão somente leitura (a não escrita é provada em tests/dsr.test.js por hash do arquivo)');
  fs.writeFileSync(path.join(OUT, 'runtime-integration.json'), JSON.stringify({
    when: new Date().toISOString(), runtimeBinary: BIN, results, journal: finalState.journal, runtime: finalState.runtime,
  }, null, 1));
} catch (e) {
  check('execução sem erro', false, e.message);
} finally {
  await browser.close();
  try { process.kill(-live.pid, 'SIGTERM'); } catch { /* já parou */ }
}
log(`${results.filter((r) => r.ok).length}/${results.length} verificações ok`);
