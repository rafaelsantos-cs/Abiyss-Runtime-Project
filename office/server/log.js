// Logger mínimo, sem dependências.
//
// Formato: `2026-10-05T15:00:00.000Z INFO  [modulo] mensagem {"campo":1}`.
// Vai para o stderr e, opcionalmente, para um arquivo (append).

import fs from 'node:fs';
import path from 'node:path';

const LEVELS = { debug: 10, info: 20, warn: 30, error: 40, silent: 100 };

const state = {
  level: LEVELS.info,
  file: null,
  /** Últimas linhas, para /api/health e para o painel de debug. */
  recent: [],
};

export function configureLog({ level = 'info', file = null } = {}) {
  if (!(level in LEVELS)) throw new Error(`nível de log desconhecido: ${level}`);
  state.level = LEVELS[level];
  if (file) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    state.file = file;
  } else {
    state.file = null;
  }
}

function write(levelName, scope, message, fields) {
  if (LEVELS[levelName] < state.level) return;
  let extra = '';
  if (fields !== undefined) {
    try {
      extra = ' ' + JSON.stringify(fields, (_k, v) => (v instanceof Error ? { message: v.message, stack: v.stack } : v));
    } catch {
      extra = ' [campos não serializáveis]';
    }
  }
  const line = `${new Date().toISOString()} ${levelName.toUpperCase().padEnd(5)} [${scope}] ${message}${extra}`;
  process.stderr.write(line + '\n');
  state.recent.push(line);
  if (state.recent.length > 200) state.recent.shift();
  if (state.file) {
    try {
      fs.appendFileSync(state.file, line + '\n');
    } catch {
      // Falha ao gravar o log não pode derrubar a simulação.
    }
  }
}

/** Cria um logger com escopo (`createLogger('dsr')`). */
export function createLogger(scope) {
  return {
    debug: (m, f) => write('debug', scope, m, f),
    info: (m, f) => write('info', scope, m, f),
    warn: (m, f) => write('warn', scope, m, f),
    error: (m, f) => write('error', scope, m, f),
  };
}

export function recentLogLines(n = 50) {
  return state.recent.slice(-n);
}
