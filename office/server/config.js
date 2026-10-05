// Configuração: arquivo JSON + variáveis de ambiente + argumentos da CLI.
//
// Precedência (a última vence): config/office.config.json → --config ARQ →
// variáveis OFFICE_* → argumentos da linha de comando.
// Caminhos relativos são resolvidos a partir da pasta do projeto (office/).

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const PROJECT_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
export const DEFAULT_CONFIG_FILE = path.join(PROJECT_ROOT, 'config', 'office.config.json');

export class ConfigError extends Error {}

const HELP = `Abiyss Office — frontend 3D do runtime do Abiyss

Uso: node server/main.js [opções]

  --config ARQ           arquivo de configuração extra (JSON, mesclado sobre o padrão)
  --source MODO          auto | runtime | demo           (OFFICE_SOURCE)
  --runtime-db ARQ       banco SQLite do runtime (data/abiyss.db)  (ABIYSS_DB)
  --runtime-config ARQ   abiyss.toml do runtime (lê [ritmo] horas_ativas)
  --host HOST            endereço de escuta (padrão 127.0.0.1)    (OFFICE_HOST)
  --port N               porta HTTP (padrão 8090)                 (OFFICE_PORT)
  --seed N               semente da simulação (determinismo)
  --fresh                ignora o estado salvo: os IMMos entram pela porta
  --no-persist           não grava data/office-state.json
  --time-scale X         acelera a simulação (só no modo demo)
  --clock ISO            relógio do escritório começa neste instante (só demo),
                         ex.: 2026-10-05T21:30:00-03:00
  --weather CODIGO       força o clima: clear|cloudy|overcast|fog|drizzle|rain|storm
  --log-level NIVEL      debug | info | warn | error          (OFFICE_LOG_LEVEL)
  --log-file ARQ         também grava o log neste arquivo
  -h, --help             mostra esta ajuda
`;

export function helpText() {
  return HELP;
}

function isObject(v) {
  return v !== null && typeof v === 'object' && !Array.isArray(v);
}

export function deepMerge(base, over) {
  if (!isObject(over)) return over === undefined ? base : over;
  const out = isObject(base) ? { ...base } : {};
  for (const [k, v] of Object.entries(over)) {
    out[k] = isObject(v) && isObject(out[k]) ? deepMerge(out[k], v) : v;
  }
  return out;
}

function readJson(file) {
  let text;
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch (e) {
    throw new ConfigError(`não consegui ler ${file}: ${e.message}`);
  }
  try {
    return JSON.parse(text);
  } catch (e) {
    throw new ConfigError(`JSON inválido em ${file}: ${e.message}`);
  }
}

/** Interpreta argv (sem node e script). Devolve { overrides, flags }. */
export function parseArgs(argv) {
  const o = {};
  const flags = { help: false, fresh: false, configFile: null };
  const need = (i, name) => {
    if (i + 1 >= argv.length || argv[i + 1].startsWith('--')) throw new ConfigError(`${name} precisa de um valor`);
    return argv[i + 1];
  };
  const num = (v, name) => {
    const n = Number(v);
    if (!Number.isFinite(n)) throw new ConfigError(`${name}: número inválido '${v}'`);
    return n;
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    switch (a) {
      case '-h': case '--help': flags.help = true; break;
      case '--fresh': flags.fresh = true; break;
      case '--no-persist': o.simulation = { ...o.simulation, persist: false }; break;
      case '--config': flags.configFile = need(i, a); i++; break;
      case '--source': o.source = { ...o.source, mode: need(i, a) }; i++; break;
      case '--runtime-db': o.source = deepMerge(o.source, { runtime: { db: need(i, a) } }); i++; break;
      case '--runtime-config': o.source = deepMerge(o.source, { runtime: { config: need(i, a) } }); i++; break;
      case '--host': o.server = { ...o.server, host: need(i, a) }; i++; break;
      case '--port': o.server = { ...o.server, port: num(need(i, a), a) }; i++; break;
      case '--seed': o.simulation = { ...o.simulation, seed: num(need(i, a), a) }; i++; break;
      case '--time-scale': o.simulation = { ...o.simulation, timeScale: num(need(i, a), a) }; i++; break;
      case '--clock': o.clock = { start: need(i, a) }; i++; break;
      case '--weather': o.weather = { ...o.weather, override: need(i, a) }; i++; break;
      case '--log-level': o.log = { ...o.log, level: need(i, a) }; i++; break;
      case '--log-file': o.log = { ...o.log, file: need(i, a) }; i++; break;
      default: throw new ConfigError(`opção desconhecida: ${a} (use --help)`);
    }
  }
  return { overrides: o, flags };
}

function envOverrides(env) {
  const o = {};
  if (env.OFFICE_SOURCE) o.source = { mode: env.OFFICE_SOURCE };
  if (env.ABIYSS_DB) o.source = deepMerge(o.source, { runtime: { db: env.ABIYSS_DB } });
  if (env.OFFICE_HOST) o.server = { host: env.OFFICE_HOST };
  if (env.OFFICE_PORT) o.server = { ...o.server, port: Number(env.OFFICE_PORT) };
  if (env.OFFICE_LOG_LEVEL) o.log = { level: env.OFFICE_LOG_LEVEL };
  return o;
}

export const WEATHER_OVERRIDES = ['clear', 'cloudy', 'overcast', 'fog', 'drizzle', 'rain', 'storm'];

/** Valida e normaliza. Lança ConfigError com mensagem clara. */
export function validateConfig(c) {
  const errs = [];
  const check = (cond, msg) => { if (!cond) errs.push(msg); };
  check(Number.isInteger(c.server.port) && c.server.port >= 0 && c.server.port < 65536, 'server.port inválida');
  check(typeof c.server.host === 'string' && c.server.host.length > 0, 'server.host inválido');
  check(c.server.broadcastHz > 0 && c.server.broadcastHz <= 30, 'server.broadcastHz deve estar em (0, 30]');
  check(c.simulation.tickHz >= 5 && c.simulation.tickHz <= 60, 'simulation.tickHz deve estar em [5, 60]');
  check(Number.isFinite(c.simulation.seed), 'simulation.seed inválida');
  check(c.simulation.timeScale > 0 && c.simulation.timeScale <= 50, 'simulation.timeScale deve estar em (0, 50]');
  check(['auto', 'runtime', 'demo'].includes(c.source.mode), `source.mode inválido '${c.source.mode}' (auto|runtime|demo)`);
  check(c.source.pollIntervalMs >= 200, 'source.pollIntervalMs deve ser >= 200');
  check(/^\d{2}:\d{2}-\d{2}:\d{2}$/.test(c.ritmo.horasAtivas), 'ritmo.horasAtivas deve ser HH:MM-HH:MM');
  check(Math.abs(c.location.latitude) <= 90 && Math.abs(c.location.longitude) <= 180, 'location fora do globo');
  try {
    new Intl.DateTimeFormat('en-GB', { timeZone: c.location.timezone });
  } catch {
    errs.push(`location.timezone desconhecido: ${c.location.timezone}`);
  }
  check(c.weather.override === null || WEATHER_OVERRIDES.includes(c.weather.override),
    `weather.override inválido (use ${WEATHER_OVERRIDES.join('|')})`);
  check(['debug', 'info', 'warn', 'error', 'silent'].includes(c.log.level), 'log.level inválido');
  if (c.clock.start !== null) {
    check(!Number.isNaN(Date.parse(c.clock.start)), `clock.start não é uma data ISO: ${c.clock.start}`);
  }
  if (errs.length) throw new ConfigError('configuração inválida:\n  - ' + errs.join('\n  - '));
  return c;
}

function resolvePath(p) {
  if (p === null || p === undefined) return p;
  return path.isAbsolute(p) ? p : path.resolve(PROJECT_ROOT, p);
}

/**
 * Carrega a configuração final.
 * @param {{argv?: string[], env?: object, base?: object}} opts
 */
export function loadConfig({ argv = [], env = process.env, base } = {}) {
  const { overrides, flags } = parseArgs(argv);
  let cfg = base ?? readJson(DEFAULT_CONFIG_FILE);
  if (flags.configFile) cfg = deepMerge(cfg, readJson(path.resolve(flags.configFile)));
  cfg = deepMerge(cfg, envOverrides(env));
  cfg = deepMerge(cfg, overrides);
  cfg.simulation.persist = cfg.simulation.persist !== false;
  validateConfig(cfg);
  cfg.simulation.stateFile = resolvePath(cfg.simulation.stateFile);
  cfg.weather.cacheFile = resolvePath(cfg.weather.cacheFile);
  cfg.source.runtime.db = resolvePath(cfg.source.runtime.db);
  cfg.source.runtime.config = resolvePath(cfg.source.runtime.config);
  if (cfg.log.file) cfg.log.file = resolvePath(cfg.log.file);
  return { config: cfg, flags };
}

/**
 * Lê valores simples de um abiyss.toml (sem parser TOML completo: só
 * `chave = "texto"` ou `chave = número` dentro de seções). Devolve null se o
 * arquivo não existir.
 */
export function readRuntimeToml(tomlFile) {
  let text;
  try {
    text = fs.readFileSync(tomlFile, 'utf8');
  } catch {
    return null;
  }
  const out = {};
  let section = '';
  for (const raw of text.split('\n')) {
    const line = raw.replace(/#.*$/, '').trim();
    if (!line) continue;
    const sec = line.match(/^\[([^\]]+)\]$/);
    if (sec) { section = sec[1].trim(); continue; }
    const kv = line.match(/^([A-Za-z0-9_]+)\s*=\s*(?:"([^"]*)"|(-?\d+(?:\.\d+)?))$/);
    if (kv) out[`${section}.${kv[1]}`] = kv[2] !== undefined ? kv[2] : Number(kv[3]);
  }
  const horas = out['ritmo.horas_ativas'];
  return {
    horasAtivas: typeof horas === 'string' && /^\d{2}:\d{2}-\d{2}:\d{2}$/.test(horas) ? horas : null,
    cronVerificacaoSegundos: Number.isFinite(out['daemon.cron_verificacao_segundos']) ? out['daemon.cron_verificacao_segundos'] : null,
    maxSimultaneos: Number.isFinite(out['subagentes.max_simultaneos']) ? out['subagentes.max_simultaneos'] : null,
  };
}

/** Compatibilidade: só `[ritmo] horas_ativas`. */
export function readRuntimeHorasAtivas(tomlFile) {
  return readRuntimeToml(tomlFile)?.horasAtivas ?? null;
}
