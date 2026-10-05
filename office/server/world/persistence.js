// Persistência do estado do escritório (data/office-state.json).
//
// Gravação atômica: escreve num arquivo temporário e renomeia. Um arquivo
// corrompido nunca impede o escritório de subir: ele é ignorado (e
// preservado com sufixo .corrompido para análise).

import fs from 'node:fs';
import path from 'node:path';

import { createLogger } from '../log.js';

const log = createLogger('persistencia');

export function saveState(file, data) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const tmp = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(data));
  fs.renameSync(tmp, file);
}

/** @returns {object|null} */
export function loadState(file) {
  let text;
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch (e) {
    if (e.code !== 'ENOENT') log.warn('não consegui ler o estado salvo', { arquivo: file, erro: e.message });
    return null;
  }
  try {
    const data = JSON.parse(text);
    if (typeof data !== 'object' || data === null || !Array.isArray(data.agents)) throw new Error('formato inesperado');
    return data;
  } catch (e) {
    const bad = `${file}.corrompido`;
    log.warn('estado salvo inválido; começando do zero', { arquivo: file, erro: e.message, copia: bad });
    try { fs.renameSync(file, bad); } catch { /* ignorar */ }
    return null;
  }
}
