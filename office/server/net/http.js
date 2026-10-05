// Servidor HTTP sem dependências: arquivos estáticos do cliente, API JSON e
// sincronização em tempo real por Server-Sent Events (SSE).
//
// Rotas:
//   GET  /                     cliente 3D (client/index.html)
//   GET  /client/* /shared/*   módulos ES do cliente e o código compartilhado
//   GET  /vendor/three/*       three.js (somente build/ e examples/jsm/)
//   GET  /api/health           saúde: fonte, clima, mundo, clientes, log recente
//   GET  /api/state            visão completa do mundo (JSON)
//   GET  /api/events           fluxo SSE (hello, tick, meta)
//   POST /api/demo/delegate    (só fonte demo) força uma delegação: ?nivel=ultra|medium|low

import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';

import { PROJECT_ROOT } from '../config.js';
import { createLogger, recentLogLines } from '../log.js';
import { PROTOCOL_VERSION, splitView } from '../../shared/protocol.js';
import { LAYOUT_VERSION } from '../../shared/layout.js';

const log = createLogger('http');

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
};

const STATIC_ROOTS = [
  { prefix: '/client/', dir: path.join(PROJECT_ROOT, 'client') },
  { prefix: '/shared/', dir: path.join(PROJECT_ROOT, 'shared') },
  { prefix: '/vendor/three/build/', dir: path.join(PROJECT_ROOT, 'node_modules', 'three', 'build') },
  { prefix: '/vendor/three/examples/jsm/', dir: path.join(PROJECT_ROOT, 'node_modules', 'three', 'examples', 'jsm') },
];

function sendJson(res, status, body) {
  const text = JSON.stringify(body);
  res.writeHead(status, { 'content-type': MIME['.json'], 'cache-control': 'no-store', 'content-length': Buffer.byteLength(text) });
  res.end(text);
}

function sendFile(res, file) {
  fs.stat(file, (err, st) => {
    if (err || !st.isFile()) return sendJson(res, 404, { erro: 'não encontrado' });
    res.writeHead(200, {
      'content-type': MIME[path.extname(file)] ?? 'application/octet-stream',
      'content-length': st.size,
      'cache-control': file.includes(`${path.sep}node_modules${path.sep}`) ? 'public, max-age=3600' : 'no-cache',
    });
    fs.createReadStream(file).pipe(res);
  });
}

/** Resolve um caminho estático sem permitir escapar da pasta (../). */
export function resolveStatic(urlPath) {
  let decoded;
  try {
    decoded = decodeURIComponent(urlPath);
  } catch {
    return null;
  }
  if (decoded.includes('\0')) return null;
  for (const { prefix, dir } of STATIC_ROOTS) {
    if (!decoded.startsWith(prefix)) continue;
    const rel = decoded.slice(prefix.length);
    const full = path.resolve(dir, rel);
    if (full !== dir && !full.startsWith(dir + path.sep)) return null;
    return full;
  }
  return null;
}

export class OfficeServer {
  /**
   * @param {object} opts
   * @param {() => object} opts.getView      visão completa do mundo
   * @param {() => object} opts.getHealth    informações de saúde
   * @param {object} [opts.demoControl]      { delegate(nivel) } quando a fonte é demo
   * @param {object} opts.info               { version, source }
   */
  constructor({ getView, getHealth, demoControl = null, info }) {
    this.getView = getView;
    this.getHealth = getHealth;
    this.demoControl = demoControl;
    this.info = info;
    this.clients = new Set();
    this.sent = 0;
    this.dropped = 0;
    this.server = http.createServer((req, res) => this.#handle(req, res));
    this.server.keepAliveTimeout = 5000;
    this.heartbeat = null;
  }

  listen(port, host) {
    return new Promise((resolve, reject) => {
      this.server.once('error', reject);
      this.server.listen(port, host, () => {
        this.server.off('error', reject);
        this.heartbeat = setInterval(() => this.#comment('ping'), 15000);
        this.heartbeat.unref?.();
        resolve(this.server.address());
      });
    });
  }

  close() {
    if (this.heartbeat) clearInterval(this.heartbeat);
    for (const c of this.clients) {
      try { c.res.end(); } catch { /* ignorar */ }
    }
    this.clients.clear();
    return new Promise((resolve) => this.server.close(() => resolve()));
  }

  #handle(req, res) {
    const url = new URL(req.url ?? '/', 'http://localhost');
    const p = url.pathname;
    try {
      if (req.method === 'GET' && (p === '/' || p === '/index.html')) return sendFile(res, path.join(PROJECT_ROOT, 'client', 'index.html'));
      if (req.method === 'GET' && p === '/favicon.ico') { res.writeHead(204); return res.end(); }
      if (p === '/api/health' && req.method === 'GET') return sendJson(res, 200, { ...this.getHealth(), clients: this.clients.size, sseSent: this.sent, sseDropped: this.dropped, log: recentLogLines(20) });
      if (p === '/api/state' && req.method === 'GET') return sendJson(res, 200, { protocol: PROTOCOL_VERSION, layoutVersion: LAYOUT_VERSION, ...this.getView() });
      if (p === '/api/events' && req.method === 'GET') return this.#openStream(req, res);
      if (p === '/api/demo/delegate' && req.method === 'POST') {
        if (!this.demoControl) return sendJson(res, 409, { erro: 'disponível só com a fonte demo (o escritório nunca escreve no runtime real)' });
        const nivel = url.searchParams.get('nivel') ?? 'low';
        if (!['ultra', 'medium', 'low'].includes(nivel)) return sendJson(res, 400, { erro: 'nivel deve ser ultra, medium ou low' });
        this.demoControl.delegate(nivel);
        return sendJson(res, 202, { ok: true, nivel });
      }
      if (req.method === 'GET') {
        const file = resolveStatic(p);
        if (file) return sendFile(res, file);
      }
      if (p.startsWith('/api/')) return sendJson(res, req.method === 'GET' ? 404 : 405, { erro: 'rota desconhecida' });
      return sendJson(res, 404, { erro: 'não encontrado' });
    } catch (e) {
      log.error('erro ao atender requisição', { rota: p, erro: e.message });
      if (!res.headersSent) sendJson(res, 500, { erro: 'erro interno' });
      else res.end();
    }
  }

  #openStream(req, res) {
    res.writeHead(200, {
      'content-type': 'text/event-stream; charset=utf-8',
      'cache-control': 'no-store',
      connection: 'keep-alive',
      'x-accel-buffering': 'no',
    });
    res.write('retry: 2000\n\n');
    const client = { res, blocked: false, id: Math.random().toString(36).slice(2, 8) };
    this.clients.add(client);
    res.on('drain', () => { client.blocked = false; });
    req.on('close', () => {
      this.clients.delete(client);
      log.info('cliente desconectado', { cliente: client.id, ativos: this.clients.size });
    });
    log.info('cliente conectado', { cliente: client.id, ativos: this.clients.size });
    this.#write(client, 'hello', { protocol: PROTOCOL_VERSION, layoutVersion: LAYOUT_VERSION, server: this.info });
    // Estado completo imediatamente (o cliente não espera o próximo quadro).
    const { tick, meta } = splitView(this.getView());
    this.#write(client, 'meta', meta);
    this.#write(client, 'tick', tick);
  }

  #write(client, event, data) {
    if (client.blocked) {
      this.dropped++;
      return;
    }
    const ok = client.res.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
    this.sent++;
    if (!ok) client.blocked = true; // cliente lento: pula quadros até esvaziar
  }

  #comment(text) {
    for (const c of this.clients) {
      if (!c.blocked) c.res.write(`: ${text}\n\n`);
    }
  }

  broadcast(event, data) {
    if (this.clients.size === 0) return;
    const payload = `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
    for (const c of this.clients) {
      if (c.blocked) { this.dropped++; continue; }
      const ok = c.res.write(payload);
      this.sent++;
      if (!ok) c.blocked = true;
    }
  }
}
