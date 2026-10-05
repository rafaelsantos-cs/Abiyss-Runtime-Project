#!/usr/bin/env node
// Soak da simulação (sem navegador): roda horas de simulação o mais rápido
// possível, em várias sementes e horários, e confere as invariantes —
// ninguém atravessa parede/móvel, ninguém trava, ninguém se sobrepõe, todos
// trabalham e voltam às mesas. Imprime um relatório em Markdown.
//
// Uso: node scripts/soak.mjs [--horas 6] [--sementes 1,2,3] [--saida arquivo.md]

import fs from 'node:fs';

import { Simulation } from '../server/world/simulation.js';
import { DemoRuntimeSource } from '../server/dsr/demo-runtime.js';
import { configureLog } from '../server/log.js';

configureLog({ level: 'silent' });
const arg = (name, def) => (process.argv.includes(name) ? process.argv[process.argv.indexOf(name) + 1] : def);
const hours = Number(arg('--horas', '6'));
const seeds = arg('--sementes', '1,2,3').split(',').map(Number);
const out = arg('--saida', null);
const TZ = 'America/Sao_Paulo';

const rows = [];
let failed = false;
for (const seed of seeds) {
  // Começa às 06:00 para atravessar descanso → vigília (e parte do dia).
  const start = Date.parse('2026-10-05T06:00:00-03:00');
  const source = new DemoRuntimeSource({ seed, horasAtivas: '07:00-23:00', timeZone: TZ });
  const sim = new Simulation({ source, seed: 1000 + seed, timeZone: TZ, clockStartMs: start });
  sim.world.startFresh({ demoOpening: true });
  const acts = new Map();
  const worked = new Set();
  const backAtDesk = new Set();
  const t0 = performance.now();
  sim.run(hours * 3600, (w) => {
    for (const a of w.agents) {
      acts.set(a.activity, (acts.get(a.activity) ?? 0) + 1);
      if (a.kind !== 'immo') continue;
      if (a.activity === 'working') worked.add(a.id);
      if (a.poi === a.desk && (a.activity === 'working' || a.activity === 'seated_idle') && a.lastFree) backAtDesk.add(a.id);
    }
  });
  const ms = performance.now() - t0;
  const st = sim.world.view().stats;
  const subs = sim.world.bridge.seq;
  const ok = st.violations === 0 && st.recoveries === 0 && st.stuck === 0 && st.overlaps === 0 && worked.size === 4 && backAtDesk.size === 4;
  failed ||= !ok;
  rows.push({ seed, ok, ms, st, worked: worked.size, back: backAtDesk.size, subs, acts });
}

const total = [...rows.flatMap((r) => [...r.acts])].reduce((m, [k, v]) => m.set(k, (m.get(k) ?? 0) + v), new Map());
const sum = [...total.values()].reduce((a, b) => a + b, 0);
let md = `# Soak da simulação\n\n${hours} h simuladas por semente, começando às 06:00 (descanso → vigília), tick de 100 ms.\n\n`;
md += '| semente | resultado | tempo real | violações | recuperações | travamentos | sobreposições | replanejamentos | IMMos que trabalharam | voltaram à mesa | sub-agentes atribuídos |\n|---|---|---|---|---|---|---|---|---|---|---|\n';
for (const r of rows) {
  md += `| ${r.seed} | ${r.ok ? '✔' : '✘'} | ${(r.ms / 1000).toFixed(1)} s | ${r.st.violations} | ${r.st.recoveries} | ${r.st.stuck} | ${r.st.overlaps} | ${r.st.replans} | ${r.worked}/4 | ${r.back}/4 | ${r.subs} |\n`;
}
md += '\n## Distribuição das atividades (todas as entidades, todas as sementes)\n\n| atividade | % do tempo |\n|---|---|\n';
for (const [k, v] of [...total].sort((a, b) => b[1] - a[1])) md += `| ${k} | ${((100 * v) / sum).toFixed(1)}% |\n`;
process.stdout.write(md);
if (out) fs.writeFileSync(out, md);
process.exit(failed ? 1 : 0);
