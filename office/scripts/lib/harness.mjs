// Utilidades para subir o servidor e dirigir um navegador headless.

import { spawn } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');

/** Sobe `server/main.js` e resolve com { port, child, lines } quando estiver no ar. */
export function startServer(args, { echo = false } = {}) {
  const child = spawn(process.execPath, ['--disable-warning=ExperimentalWarning', path.join(ROOT, 'server', 'main.js'), ...args], {
    stdio: ['ignore', 'ignore', 'pipe'],
  });
  const lines = [];
  let buf = '';
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`servidor não subiu:\n${lines.join('\n')}`)), 20000);
    child.stderr.on('data', (d) => {
      buf += d.toString();
      let i;
      while ((i = buf.indexOf('\n')) >= 0) {
        const line = buf.slice(0, i);
        buf = buf.slice(i + 1);
        lines.push(line);
        if (echo) process.stderr.write(`  [srv] ${line}\n`);
        const m = line.match(/escritório no ar: http:\/\/[^:]+:(\d+)\//);
        if (m) {
          clearTimeout(timer);
          resolve({ port: Number(m[1]), child, lines });
        }
      }
    });
    child.on('exit', (code) => {
      clearTimeout(timer);
      reject(new Error(`servidor saiu (${code}):\n${lines.join('\n')}`));
    });
  });
}

export async function stopServer(srv) {
  if (!srv || srv.child.exitCode !== null) return;
  srv.child.kill('SIGTERM');
  await new Promise((r) => srv.child.once('exit', r));
}

export async function launchBrowser() {
  const { chromium } = await import('playwright');
  const opts = { args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader', '--ignore-gpu-blocklist'] };
  if (process.env.CHROMIUM_PATH) opts.executablePath = process.env.CHROMIUM_PATH;
  return chromium.launch(opts);
}

/** Espera uma condição avaliada na página (string de função sobre window.__office.state()). */
export async function waitForState(page, predicateSrc, { timeoutMs = 180000, label = 'condição' } = {}) {
  const t0 = Date.now();
  for (;;) {
    const ok = await page.evaluate(`(() => { const s = window.__office && window.__office.state(); if (!s) return false; return (${predicateSrc})(s); })()`);
    if (ok) return true;
    if (Date.now() - t0 > timeoutMs) throw new Error(`tempo esgotado esperando: ${label}`);
    await page.waitForTimeout(500);
  }
}
