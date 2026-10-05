import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import net from 'node:net';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { OfficeServer, resolveStatic } from '../server/net/http.js';
import { loadConfig, ConfigError } from '../server/config.js';
import { configureLog } from '../server/log.js';

configureLog({ level: 'silent' });
const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('caminhos estáticos não escapam das pastas permitidas', () => {
  assert.ok(resolveStatic('/client/main.js').endsWith(path.join('client', 'main.js')));
  assert.ok(resolveStatic('/vendor/three/build/three.module.js').includes(path.join('node_modules', 'three', 'build')));
  assert.equal(resolveStatic('/client/../package.json'), null);
  assert.equal(resolveStatic('/client/%2e%2e/package.json'), null);
  assert.equal(resolveStatic('/vendor/three/package.json'), null);
  assert.equal(resolveStatic('/etc/passwd'), null);
});

test('configuração: precedência, validação e caminhos', () => {
  const { config, flags } = loadConfig({ argv: ['--source', 'demo', '--port', '9999', '--fresh', '--weather', 'rain'], env: { OFFICE_PORT: '7000' } });
  assert.equal(config.source.mode, 'demo');
  assert.equal(config.server.port, 9999, 'CLI vence o ambiente');
  assert.equal(flags.fresh, true);
  assert.equal(config.weather.override, 'rain');
  assert.ok(path.isAbsolute(config.simulation.stateFile));
  assert.throws(() => loadConfig({ argv: ['--source', 'xyz'] }), ConfigError);
  assert.throws(() => loadConfig({ argv: ['--weather', 'granizo'] }), ConfigError);
  assert.throws(() => loadConfig({ argv: ['--nada'] }), ConfigError);
  assert.throws(() => loadConfig({ argv: ['--port'] }), ConfigError);
});

test('servidor: SSE envia hello/meta/tick e a API responde', async () => {
  const view = { tick: 1, simMs: 100, wallMs: 1, clock: {}, entities: [{ id: 'abiyss' }], fx: [], runtime: { ok: true }, journal: [], env: null, stats: {} };
  const srv = new OfficeServer({ getView: () => view, getHealth: () => ({ ok: true }), info: { version: 't' } });
  const addr = await srv.listen(0, '127.0.0.1');
  const base = `http://127.0.0.1:${addr.port}`;
  try {
    const h = await (await fetch(`${base}/api/health`)).json();
    assert.equal(h.ok, true);
    const st = await (await fetch(`${base}/api/state`)).json();
    assert.equal(st.protocol, 1);
    assert.equal(st.entities[0].id, 'abiyss');
    assert.equal((await fetch(`${base}/api/nada`)).status, 404);
    assert.equal((await fetch(`${base}/api/demo/delegate`, { method: 'POST' })).status, 409, 'sem fonte demo, nada de comandos');

    const ctrl = new AbortController();
    const res = await fetch(`${base}/api/events`, { signal: ctrl.signal });
    assert.equal(res.headers.get('content-type').split(';')[0], 'text/event-stream');
    const reader = res.body.getReader();
    let text = '';
    const dec = new TextDecoder();
    srv.broadcast('tick', { tick: 2 });
    while (!text.includes('"tick":2')) {
      const { value, done } = await reader.read();
      if (done) break;
      text += dec.decode(value);
    }
    ctrl.abort();
    assert.match(text, /event: hello/);
    assert.match(text, /event: meta/);
    assert.match(text, /event: tick\ndata: \{"tick":2\}/);
  } finally {
    await srv.close();
  }
});

function startOffice(args, onLine) {
  const child = spawn(process.execPath, ['--disable-warning=ExperimentalWarning', path.join(ROOT, 'server', 'main.js'), ...args], { stdio: ['ignore', 'ignore', 'pipe'] });
  let buf = '';
  const lines = [];
  const ready = new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`não subiu:\n${lines.join('\n')}`)), 15000);
    child.stderr.on('data', (d) => {
      buf += d.toString();
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i);
        buf = buf.slice(i + 1);
        lines.push(line);
        onLine?.(line);
        const m = line.match(/escritório no ar: http:\/\/[^:]+:(\d+)\//);
        if (m) { clearTimeout(timer); resolve(Number(m[1])); }
      }
    });
    child.on('exit', (code) => { clearTimeout(timer); reject(new Error(`saiu com ${code}:\n${lines.join('\n')}`)); });
  });
  const exited = new Promise((resolve) => child.on('exit', (code) => resolve(code)));
  return { child, ready, exited, lines };
}

test('ponta a ponta: sobe, simula, para com o estado salvo e reinicia restaurando', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'office-e2e-'));
  const cfg = path.join(dir, 'cfg.json');
  fs.writeFileSync(cfg, JSON.stringify({ simulation: { stateFile: path.join(dir, 'state.json') }, weather: { cacheFile: null, provider: 'none' } }));
  const common = ['--config', cfg, '--source', 'demo', '--port', '0', '--log-level', 'info'];

  const first = startOffice([...common, '--fresh']);
  const port = await first.ready;
  await new Promise((r) => setTimeout(r, 2500));
  const health = await (await fetch(`http://127.0.0.1:${port}/api/health`)).json();
  assert.equal(health.source.source, 'demo');
  assert.ok(health.tick > 10, 'a simulação está andando');
  const state = await (await fetch(`http://127.0.0.1:${port}/api/state`)).json();
  assert.equal(state.entities.length, 5);
  const html = await (await fetch(`http://127.0.0.1:${port}/`)).text();
  assert.match(html, /Abiyss Office/);
  assert.equal((await fetch(`http://127.0.0.1:${port}/api/demo/delegate?nivel=ultra`, { method: 'POST' })).status, 202);
  first.child.kill('SIGTERM');
  assert.equal(await first.exited, 0, 'para limpo com SIGTERM');
  assert.ok(fs.existsSync(path.join(dir, 'state.json')), 'estado salvo ao parar');

  const second = startOffice(common);
  await second.ready;
  assert.ok(second.lines.some((l) => l.includes('estado anterior restaurado')), 'restaura ao reiniciar');
  second.child.kill('SIGTERM');
  assert.equal(await second.exited, 0);
});

function runToExit(args) {
  const child = spawn(process.execPath, ['--disable-warning=ExperimentalWarning', path.join(ROOT, 'server', 'main.js'), ...args], { stdio: ['ignore', 'ignore', 'pipe'] });
  let err = '';
  child.stderr.on('data', (d) => { err += d; });
  return new Promise((r) => child.on('exit', (code) => r({ code, err })));
}

test('ponta a ponta: porta ocupada e configuração inválida falham com mensagem clara', async () => {
  const bad = await runToExit(['--source', 'nada']);
  assert.equal(bad.code, 2);
  assert.match(bad.err, /source.mode inválido/);

  const blocker = net.createServer();
  await new Promise((r) => blocker.listen(0, '127.0.0.1', r));
  const port = blocker.address().port;
  try {
    const busy = await runToExit(['--source', 'demo', '--no-persist', '--port', String(port)]);
    assert.equal(busy.code, 1);
    assert.match(busy.err, /não consegui abrir a porta HTTP/);
  } finally {
    blocker.close();
  }
});
