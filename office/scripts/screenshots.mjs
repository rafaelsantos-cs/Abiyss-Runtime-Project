#!/usr/bin/env node
// Screenshots REAIS do software rodando: sobe o servidor, abre o cliente
// num Chromium headless (WebGL por SwiftShader, sem GPU), espera condições
// da simulação (não tempos fixos) e captura. Cada imagem vem com um JSON
// do estado no instante da captura (prova do que estava acontecendo).
//
// Uso: node scripts/screenshots.mjs [--out artifacts/screenshots] [--only 01,03]
//
// Requer o Chromium do Playwright (npx playwright install chromium, ou
// PLAYWRIGHT_BROWSERS_PATH apontando para um já instalado).

import fs from 'node:fs';
import path from 'node:path';

import { ROOT, startServer, stopServer, launchBrowser, waitForState } from './lib/harness.mjs';

const args = process.argv.slice(2);
const outDir = path.resolve(ROOT, args.includes('--out') ? args[args.indexOf('--out') + 1] : 'artifacts/screenshots');
const only = args.includes('--only') ? args[args.indexOf('--only') + 1].split(',') : null;
fs.mkdirSync(outDir, { recursive: true });

const VIEW = { width: 1600, height: 900 };
const log = (m) => process.stdout.write(`${new Date().toISOString().slice(11, 19)} ${m}\n`);

async function openPage(browser, port, query = '') {
  const page = await browser.newPage({ viewport: VIEW });
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  await page.goto(`http://127.0.0.1:${port}/${query}`);
  await page.waitForFunction(() => window.__office && window.__office.ready, null, { timeout: 60000 });
  page.errors = errors;
  return page;
}

async function shot(page, name, note, settleMs = 2200) {
  await page.waitForTimeout(settleMs); // transição da câmera + alguns quadros
  const file = path.join(outDir, `${name}.png`);
  await page.screenshot({ path: file });
  const state = await page.evaluate(() => window.__office.state());
  fs.writeFileSync(path.join(outDir, `${name}.json`), JSON.stringify({ note, capturedAt: new Date().toISOString(), state }, null, 1));
  const acts = state.entities.map((e) => `${e.id}:${e.act}${e.logical?.estado ? `(${e.logical.estado})` : ''}`).join(' ');
  log(`📸 ${name} — ${note}\n         ${acts}`);
  if (page.errors.length) log(`   erros na página: ${page.errors.join(' | ')}`);
}

const want = (id) => !only || only.includes(id);

// Predicados (avaliados na página sobre window.__office.state()).
const P = {
  working3: `(s) => s.entities.filter((e) => e.act === 'working').length >= 3`,
  working2: `(s) => s.entities.filter((e) => e.act === 'working').length >= 2`,
  sofa: `(s) => s.entities.some((e) => e.kind === 'immo' && e.act === 'resting' && (e.poi || '').startsWith('sofa'))`,
  walking: `(s) => s.entities.some((e) => e.kind === 'immo' && ['walking', 'wandering'].includes(e.act))`,
  reporting: `(s) => s.entities.some((e) => e.act === 'reporting')`,
  abiyssHome: `(s) => { const a = s.entities.find((e) => e.kind === 'abiyss'); return a && a.poi === 'abiyss-home' && a.act === 'idle'; }`,
  thinking: `(s) => { const a = s.entities.find((e) => e.kind === 'abiyss'); return a && a.mind === 'thinking'; }`,
  mixed: `(s) => { const im = s.entities.filter((e) => e.kind === 'immo'); return new Set(im.map((e) => e.act)).size >= 3 && im.some((e) => e.act === 'working'); }`,
  sleeping2: `(s) => s.entities.filter((e) => e.act === 'sleeping').length >= 2`,
};

async function walkerId(page) {
  return page.evaluate(() => {
    const s = window.__office.state();
    const e = s.entities.find((x) => x.kind === 'immo' && ['walking', 'wandering'].includes(x.act));
    return e ? e.id : null;
  });
}

async function daytime(browser) {
  log('dia: demo com abertura roteirizada (relógio real, escala 2×)');
  const srv = await startServer(['--source', 'demo', '--fresh', '--no-persist', '--port', '0', '--time-scale', '2', '--clock', '2026-10-05T10:40:00-03:00']);
  try {
    const page = await openPage(browser, srv.port, '?labels=2');
    if (want('00')) {
      await page.evaluate(() => window.__office.orbit({ target: [11.2, 0.4, 14.2], az: 0.28, el: 0.72, dist: 12 }));
      await waitForState(page, `(s) => s.entities.filter((e) => e.act === 'entering').length >= 2`, { label: 'IMMos entrando' });
      await shot(page, '00-chegada', 'abertura: os IMMos chegam pela calçada e entram pela porta principal', 500);
    }
    await page.evaluate(() => window.__office.labels(1));
    if (want('01') || want('03')) {
      await waitForState(page, P.working3, { label: 'três IMMos trabalhando' });
      if (want('01')) { await page.evaluate(() => window.__office.preset('overview')); await shot(page, '01-visao-geral', 'visão geral do escritório'); }
      if (want('03')) {
        await page.evaluate(() => { window.__office.labels(2); window.__office.preset('desks'); });
        await shot(page, '03-immos-trabalhando', 'IMMos trabalhando nas mesas (sub-agentes em execução)');
      }
    }
    if (want('04')) {
      await page.evaluate(() => window.__office.labels(2));
      await waitForState(page, P.sofa, { label: 'IMMo descansando no sofá' });
      await page.evaluate(() => window.__office.preset('lounge'));
      await shot(page, '04-descanso-sofa', 'um IMMo descansando no sofá');
    }
    if (want('05')) {
      await waitForState(page, P.walking, { label: 'IMMo caminhando' });
      const id = await walkerId(page);
      await page.evaluate((x) => { window.__office.labels(2); window.__office.follow(x); }, id);
      await shot(page, '05-immo-caminhando', `IMMo caminhando (${id}), câmera seguindo`, 900);
    }
    if (want('02')) {
      await page.evaluate(() => { window.__office.labels(2); window.__office.preset('abiyss'); });
      await waitForState(page, P.reporting, { label: 'IMMo entregando relatório ao Abiyss' });
      await shot(page, '02-abiyss-sala', 'Abiyss em sua sala recebendo um relatório', 400);
    }
    if (want('06')) {
      await waitForState(page, P.mixed, { label: 'vários estados ao mesmo tempo' });
      await page.evaluate(() => { window.__office.labels(2); window.__office.preset('overview'); window.__office.debug(true); });
      await shot(page, '06-estados-simultaneos', 'vários estados simultâneos + painel de debug');
      await page.evaluate(() => window.__office.debug(false));
    }
    if (want('07')) {
      await page.evaluate(() => { window.__office.labels(2); window.__office.follow('abiyss'); });
      await waitForState(page, P.thinking, { label: 'Abiyss pensando (heartbeat)', timeoutMs: 240000 });
      await shot(page, '07-abiyss-pensando', 'Abiyss pensando: ciclo do heartbeat que chamou o modelo', 300);
    }
    if (want('08')) {
      await page.evaluate(() => { window.__office.labels(1); window.__office.preset('top'); window.__office.debug(true); window.__office.paths(true); });
      await page.waitForTimeout(1500);
      await waitForState(page, `(s) => s.entities.filter((e) => ['walking', 'wandering', 'moving'].includes(e.act)).length >= 2`, { label: 'duas entidades andando (caminhos)' });
      await shot(page, '08-planta-caminhos', 'planta com caminhos e destinos (debug)', 300);
    }
    await page.close();
  } finally {
    await stopServer(srv);
  }
}

async function night(browser) {
  if (!want('09') && !want('10')) return;
  log('noite: demo às 23:20 (fase descanso) com chuva');
  const srv = await startServer(['--source', 'demo', '--fresh', '--no-persist', '--port', '0', '--time-scale', '4', '--clock', '2026-10-05T23:20:00-03:00', '--weather', 'rain']);
  try {
    const page = await openPage(browser, srv.port, '?labels=2');
    await waitForState(page, P.sleeping2, { label: 'dois IMMos dormindo', timeoutMs: 300000 });
    if (want('09')) {
      await page.evaluate(() => window.__office.preset('overview'));
      await shot(page, '09-noite-chuva', 'noite chuvosa: lâmpadas acesas, IMMos dormindo');
    }
    if (want('10')) {
      await page.evaluate(() => window.__office.preset('lounge'));
      await shot(page, '10-dormindo', 'IMMos dormindo na área de descanso (fase descanso)');
    }
    await page.close();
  } finally {
    await stopServer(srv);
  }
}

async function dusk(browser) {
  if (!want('11')) return;
  log('fim de tarde: sol baixo');
  const srv = await startServer(['--source', 'demo', '--fresh', '--no-persist', '--port', '0', '--time-scale', '2', '--clock', '2026-10-05T17:25:00-03:00', '--weather', 'clear']);
  try {
    const page = await openPage(browser, srv.port, '?labels=1');
    await waitForState(page, P.working2, { label: 'dois trabalhando' });
    await page.evaluate(() => window.__office.orbit({ target: [12, 0, 8.5], az: -0.75, el: 0.62, dist: 32 }));
    await shot(page, '11-por-do-sol', 'fim de tarde em Contagem: luz baixa e quente vinda do oeste');
    await page.close();
  } finally {
    await stopServer(srv);
  }
}

const browser = await launchBrowser();
try {
  await daytime(browser);
  await night(browser);
  await dusk(browser);
} finally {
  await browser.close();
}
log(`pronto: ${outDir}`);
