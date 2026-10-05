#!/usr/bin/env node
// Verificação rápida: sintaxe de todos os módulos (servidor, cliente,
// compartilhado, scripts e testes) com `node --check`, e confere que os
// imports relativos apontam para arquivos que existem.

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const DIRS = ['server', 'client', 'shared', 'scripts', 'tests'];

function walk(dir, out = []) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) walk(p, out);
    else if (/\.(m?js)$/.test(e.name)) out.push(p);
  }
  return out;
}

let failures = 0;
const files = DIRS.flatMap((d) => walk(path.join(ROOT, d)));
for (const f of files) {
  const r = spawnSync(process.execPath, ['--check', f], { encoding: 'utf8' });
  if (r.status !== 0) {
    failures++;
    process.stderr.write(`✘ ${path.relative(ROOT, f)}\n${r.stderr}\n`);
  }
  const src = fs.readFileSync(f, 'utf8');
  for (const m of src.matchAll(/(?:import|export)[^'"]*from\s+['"](\.{1,2}\/[^'"]+)['"]/g)) {
    const target = path.resolve(path.dirname(f), m[1]);
    if (!fs.existsSync(target)) {
      failures++;
      process.stderr.write(`✘ ${path.relative(ROOT, f)}: import inexistente ${m[1]}\n`);
    }
  }
}
process.stdout.write(`${files.length} arquivos verificados, ${failures} problema(s)\n`);
process.exit(failures ? 1 : 0);
