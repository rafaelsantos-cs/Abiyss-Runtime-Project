#!/usr/bin/env node
// Abiyss Office — ponto de entrada do servidor.
//
//   fonte (runtime real | demo) → ponte DSR → mundo (simulação) → SSE → cliente 3D
//
// Uso: node server/main.js [--source auto|runtime|demo] [--fresh] [--port N] ...
// (veja --help e o README).

import fs from 'node:fs';
import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';

import { loadConfig, ConfigError, helpText, readRuntimeToml, PROJECT_ROOT } from './config.js';
import { configureLog, createLogger } from './log.js';
import { RuntimeSqliteSource } from './dsr/runtime-sqlite.js';
import { DemoRuntimeSource } from './dsr/demo-runtime.js';
import { Simulation } from './world/simulation.js';
import { loadState, saveState } from './world/persistence.js';
import { Environment } from './environment/environment.js';
import { OfficeServer } from './net/http.js';
import { splitView } from '../shared/protocol.js';

const log = createLogger('main');
const VERSION = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')).version;

/** Abre o navegador padrão (Windows, macOS ou Linux). Falha só gera aviso. */
function openBrowser(url) {
  const [cmd, args] = process.platform === 'win32' ? ['cmd', ['/c', 'start', '""', url]]
    : process.platform === 'darwin' ? ['open', [url]] : ['xdg-open', [url]];
  try {
    // No Windows, `start "" URL` precisa chegar ao cmd sem as aspas escapadas pelo Node.
    const child = spawn(cmd, args, { stdio: 'ignore', detached: true, windowsHide: true, windowsVerbatimArguments: process.platform === 'win32' });
    child.on('error', () => log.warn(`não consegui abrir o navegador; abra ${url}`));
    child.unref();
  } catch {
    log.warn(`não consegui abrir o navegador; abra ${url}`);
  }
}

function chooseSource(config) {
  const mode = config.source.mode;
  if (mode === 'demo') return 'demo';
  if (mode === 'runtime') return 'runtime';
  if (fs.existsSync(config.source.runtime.db)) {
    log.info('modo auto: banco do runtime encontrado', { db: config.source.runtime.db });
    return 'runtime';
  }
  log.info('modo auto: banco do runtime não encontrado; usando a demonstração', { procurado: config.source.runtime.db });
  return 'demo';
}

async function main() {
  let loaded;
  try {
    loaded = loadConfig({ argv: process.argv.slice(2) });
  } catch (e) {
    if (e instanceof ConfigError) {
      process.stderr.write(`erro: ${e.message}\n`);
      process.exit(2);
    }
    throw e;
  }
  const { config, flags } = loaded;
  if (flags.help) {
    process.stdout.write(helpText());
    return;
  }
  configureLog(config.log);
  log.info(`Abiyss Office ${VERSION} iniciando`, { node: process.version, raiz: PROJECT_ROOT });

  const kind = chooseSource(config);
  const tz = config.location.timezone;
  let source;
  let horasAtivas = config.ritmo.horasAtivas;
  let clockStartMs = config.clock.start ? Date.parse(config.clock.start) : null;
  let timeScale = config.simulation.timeScale;

  if (kind === 'runtime') {
    const toml = readRuntimeToml(config.source.runtime.config);
    let cronSeg = config.source.runtime.cronVerificacaoSegundos;
    if (toml) {
      if (toml.horasAtivas) horasAtivas = toml.horasAtivas;
      if (toml.cronVerificacaoSegundos) cronSeg = toml.cronVerificacaoSegundos;
      log.info('configuração lida do abiyss.toml do runtime', { arquivo: config.source.runtime.config, horasAtivas, cronVerificacaoSegundos: cronSeg, maxSimultaneos: toml.maxSimultaneos });
      if (toml.maxSimultaneos && toml.maxSimultaneos > 4) {
        log.warn('o runtime permite mais sub-agentes simultâneos que IMMos no escritório (4); os excedentes aparecem como transbordo', { maxSimultaneos: toml.maxSimultaneos });
      }
    } else {
      log.warn('abiyss.toml do runtime não encontrado; usando horas ativas da configuração do escritório', { procurado: config.source.runtime.config, horasAtivas });
    }
    if (clockStartMs !== null || timeScale !== 1) {
      log.warn('--clock e --time-scale são ignorados com o runtime real (o escritório segue a hora real)');
      clockStartMs = null;
      timeScale = 1;
    }
    source = new RuntimeSqliteSource({
      dbPath: config.source.runtime.db,
      horasAtivas,
      timeZone: tz,
      cronVerificacaoSegundos: cronSeg,
      recentFinishedMinutes: config.source.runtime.recentFinishedMinutes,
    });
  } else {
    source = new DemoRuntimeSource({ seed: config.source.demo.seed, horasAtivas, timeZone: tz });
  }
  await source.start();

  const sim = new Simulation({
    source,
    seed: config.simulation.seed,
    timeZone: tz,
    tickHz: config.simulation.tickHz,
    pollIntervalMs: config.source.pollIntervalMs,
    clockStartMs,
    timeScale,
    privacy: config.privacy,
  });
  const world = sim.world;

  // Estado salvo (a menos de --fresh).
  let restored = false;
  if (!flags.fresh && config.simulation.persist) {
    const data = loadState(config.simulation.stateFile);
    if (data) restored = world.restore(data, source.key);
  }
  if (!restored) {
    world.bridge.reset(source.key);
    world.startFresh({ demoOpening: kind === 'demo' });
  }

  const environment = new Environment({ location: config.location, weatherConfig: config.weather });
  environment.start();
  sim.environment = environment;
  sim.poll();

  const startedAt = Date.now();
  const server = new OfficeServer({
    getView: () => world.view(),
    getHealth: () => ({
      ok: true,
      version: VERSION,
      uptimeS: Math.round((Date.now() - startedAt) / 1000),
      source: source.status(),
      weather: environment.status(),
      tick: world.tick,
      stats: world.view().stats,
    }),
    demoControl: kind === 'demo' ? { delegate: (n) => source.delegate(n) } : null,
    info: { version: VERSION, source: kind, timeScale, clockReal: sim.clock.isReal },
  });
  let address;
  try {
    address = await server.listen(config.server.port, config.server.host);
  } catch (e) {
    log.error('não consegui abrir a porta HTTP', { host: config.server.host, porta: config.server.port, erro: e.message });
    process.exit(1);
  }
  log.info(`escritório no ar: http://${address.address}:${address.port}/`, { fonte: kind, horasAtivas, restaurado: restored });
  if (flags.open) openBrowser(`http://${address.address === '0.0.0.0' || address.address === '::' ? '127.0.0.1' : address.address}:${address.port}/`);

  // Laço da simulação: passos fixos, acompanhando o relógio real × escala.
  const dtMs = 1000 / config.simulation.tickHz;
  let last = performance.now();
  let budget = 0;
  const loop = setInterval(() => {
    const now = performance.now();
    budget += (now - last) * timeScale;
    last = now;
    let steps = 0;
    while (budget >= dtMs && steps < 60) {
      sim.stepOnce();
      budget -= dtMs;
      steps++;
    }
    if (steps === 60) budget = 0; // atrasado demais: descarta em vez de acumular
  }, dtMs);

  // Transmissão: quadros de entidades em broadcastHz, metadados 1×/s.
  let metaCountdown = 0;
  const broadcastEvery = 1000 / config.server.broadcastHz;
  const broadcast = setInterval(() => {
    const { tick, meta } = splitView(world.view());
    server.broadcast('tick', tick);
    if (--metaCountdown <= 0) {
      server.broadcast('meta', meta);
      metaCountdown = config.server.broadcastHz;
    }
  }, broadcastEvery);

  const persist = () => {
    if (!config.simulation.persist) return;
    try {
      saveState(config.simulation.stateFile, world.toJSON());
    } catch (e) {
      log.warn('não consegui salvar o estado', { erro: e.message });
    }
  };
  const persistTimer = setInterval(persist, config.simulation.persistIntervalSec * 1000);

  let stopping = false;
  const stop = async (signal) => {
    if (stopping) return;
    stopping = true;
    log.info(`recebi ${signal}; parando com o estado salvo`);
    clearInterval(loop);
    clearInterval(broadcast);
    clearInterval(persistTimer);
    persist();
    environment.stop();
    source.stop();
    await server.close();
    log.info('parado');
    process.exit(0);
  };
  process.on('SIGINT', () => stop('SIGINT'));
  process.on('SIGTERM', () => stop('SIGTERM'));
}

process.on('uncaughtException', (e) => {
  log.error('exceção não tratada', { erro: e.message, stack: e.stack });
});
process.on('unhandledRejection', (e) => {
  log.error('promessa rejeitada sem tratamento', { erro: e?.message ?? String(e) });
});

main().catch((e) => {
  log.error('falha ao iniciar', { erro: e.message, stack: e.stack });
  process.exit(1);
});
