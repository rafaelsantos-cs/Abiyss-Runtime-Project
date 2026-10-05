#!/usr/bin/env node
// Monta um .zip pronto para rodar em outro computador SEM `npm install`:
// leva o código, a documentação, os lançadores e só os arquivos do three.js
// que o cliente usa. No destino basta ter Node.js >= 22.13.
//
// Uso: node scripts/empacotar.mjs   →   dist/abiyss-office-<versão>.zip

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const pkg = JSON.parse(fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8'));
const NAME = `abiyss-office-${pkg.version}`;
const DIST = path.join(ROOT, 'dist');
const STAGE = path.join(DIST, NAME);

const COPY = [
  'package.json', 'package-lock.json', 'README.md', 'iniciar-windows.bat', 'iniciar.sh', '.gitignore',
  'config', 'shared', 'server', 'client', 'docs', 'scripts', 'tests',
  'artifacts/screenshots/01-visao-geral.png', 'artifacts/screenshots/02-abiyss-sala.png',
  'artifacts/screenshots/03-immos-trabalhando.png', 'artifacts/screenshots/09-noite-chuva.png',
  'artifacts/screenshots/15-runtime-real-daemon-parado.png',
];
// Só o que o cliente importa (ver o import map em client/index.html).
const THREE = [
  'package.json', 'LICENSE', 'build/three.module.js', 'build/three.core.js',
  'examples/jsm/controls/OrbitControls.js', 'examples/jsm/utils/BufferGeometryUtils.js',
];

function copy(rel, fromRoot = ROOT, toRoot = STAGE) {
  const src = path.join(fromRoot, rel);
  if (!fs.existsSync(src)) throw new Error(`não encontrado: ${src}`);
  fs.mkdirSync(path.dirname(path.join(toRoot, rel)), { recursive: true });
  fs.cpSync(src, path.join(toRoot, rel), { recursive: true });
}

fs.rmSync(STAGE, { recursive: true, force: true });
fs.mkdirSync(STAGE, { recursive: true });
for (const rel of COPY) copy(rel);
const threeSrc = path.join(ROOT, 'node_modules', 'three');
for (const rel of THREE) copy(rel, threeSrc, path.join(STAGE, 'node_modules', 'three'));
const license = path.join(ROOT, '..', 'LICENSE');
if (fs.existsSync(license)) fs.copyFileSync(license, path.join(STAGE, 'LICENSE'));
fs.chmodSync(path.join(STAGE, 'iniciar.sh'), 0o755);

const zip = path.join(DIST, `${NAME}.zip`);
fs.rmSync(zip, { force: true });
const r = spawnSync('zip', ['-r', '-q', '-X', `${NAME}.zip`, NAME], { cwd: DIST, stdio: 'inherit' });
if (r.status !== 0) {
  // Sem o comando zip: usa o módulo zipfile do Python.
  const py = spawnSync('python3', ['-m', 'zipfile', '-c', `${NAME}.zip`, NAME], { cwd: DIST, stdio: 'inherit' });
  if (py.status !== 0) throw new Error('não consegui criar o .zip (instale zip ou python3)');
}
const mb = (fs.statSync(zip).size / 1024 / 1024).toFixed(1);
process.stdout.write(`pacote: ${path.relative(ROOT, zip)} (${mb} MB)\n`);
