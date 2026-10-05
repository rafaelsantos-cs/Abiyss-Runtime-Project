// Fonte "runtime": lê o banco SQLite REAL do runtime do Abiyss (data/abiyss.db).
//
// - Conexão SOMENTE LEITURA (`readOnly` + `PRAGMA query_only`): o escritório
//   nunca escreve no banco do runtime.
// - Cada leitura roda numa transação de leitura (instantâneo consistente no
//   modo WAL, sem bloquear o daemon que escreve).
// - Só consultas pequenas e indexadas, uma vez por `pollIntervalMs`.
// - Banco ausente, travado ou antigo não derruba o escritório: a fonte fica
//   "desconectada" e tenta de novo.
//
// Tabelas usadas (migrações 3, 4 e 8 do kernel): estado_daemon, subagentes,
// goals, ciclos, fila_eventos, chamadas_modelo. A tabela `sonos` (sessão E5,
// ainda não publicada) é lida só se existir — formato provisório.

import fs from 'node:fs';

import { createLogger } from '../log.js';
import { emptySnapshot, goalEmFoco } from './snapshot.js';
import { fasePeloRelogio, FASE } from './ritmo.js';
import { localParts } from '../world/clock.js';

const log = createLogger('dsr:runtime');

/** Migração mínima: 3 (daemon/goals/eventos) + 4 (sub-agentes). */
export const MIN_SCHEMA = 4;
/** Última migração conhecida por este adaptador (kernel 2318c40). */
export const KNOWN_SCHEMA = 10;

/** Estados de `sonos.estado` tratados como "dormindo" (provisório: E5 não publicada). */
const SONO_ATIVO = ['executando', 'rodando', 'em_andamento', 'andamento', 'dormindo'];

function parseRelatorio(texto) {
  if (!texto) return null;
  try {
    const r = JSON.parse(texto);
    return {
      status: typeof r.status === 'string' ? r.status : 'parcial',
      resumo: typeof r.resumo === 'string' ? r.resumo : '',
      confianca: Number.isFinite(r.confianca) ? r.confianca : 0,
    };
  } catch {
    return null;
  }
}

export class RuntimeSqliteSource {
  /**
   * @param {object} opts
   * @param {string} opts.dbPath
   * @param {string} opts.horasAtivas  `[ritmo] horas_ativas` do runtime
   * @param {string} opts.timeZone     fuso do runtime (TZ do systemd)
   * @param {number} [opts.cronVerificacaoSegundos]
   * @param {number} [opts.recentFinishedMinutes]
   */
  constructor({ dbPath, horasAtivas, timeZone, cronVerificacaoSegundos = 30, recentFinishedMinutes = 10 }) {
    this.name = 'runtime-sqlite';
    this.dbPath = dbPath;
    this.horasAtivas = horasAtivas;
    this.timeZone = timeZone;
    this.cronVerificacaoSegundos = cronVerificacaoSegundos;
    this.recentFinishedMs = recentFinishedMinutes * 60_000;
    this.db = null;
    this.sqlite = null;
    this.lastError = null;
    this.lastOkMs = null;
    this.polls = 0;
    this.failures = 0;
    this.hasSonos = false;
    this.warnedSchema = false;
  }

  /** Identificador estável da fonte (para o estado salvo do escritório). */
  get key() {
    return `runtime-sqlite:${this.dbPath}`;
  }

  async start() {
    try {
      this.sqlite = await import('node:sqlite');
    } catch (e) {
      throw new Error(`node:sqlite indisponível (Node >= 22.13 necessário): ${e.message}`);
    }
  }

  #open() {
    if (this.db) return;
    if (!fs.existsSync(this.dbPath)) throw new Error(`banco do runtime não encontrado: ${this.dbPath}`);
    const db = new this.sqlite.DatabaseSync(this.dbPath, { readOnly: true });
    try {
      db.exec('PRAGMA query_only = 1');
      db.exec('PRAGMA busy_timeout = 2000');
      const v = db.prepare('PRAGMA user_version').get().user_version;
      if (v < MIN_SCHEMA) {
        throw new Error(`banco do runtime na migração ${v}; o escritório precisa da ${MIN_SCHEMA}+ (daemon e sub-agentes)`);
      }
      if (v > KNOWN_SCHEMA && !this.warnedSchema) {
        log.warn('banco do runtime mais novo que o conhecido; lendo só as colunas conhecidas', { versao: v, conhecida: KNOWN_SCHEMA });
        this.warnedSchema = true;
      }
      this.schemaVersion = v;
      this.hasSonos = !!db.prepare("SELECT 1 AS x FROM sqlite_master WHERE type='table' AND name='sonos'").get();
    } catch (e) {
      db.close();
      throw e;
    }
    this.db = db;
    log.info('conectado ao banco do runtime (somente leitura)', { db: this.dbPath, migracao: this.schemaVersion });
  }

  #close() {
    if (!this.db) return;
    try { this.db.close(); } catch { /* ignorar */ }
    this.db = null;
  }

  stop() {
    this.#close();
  }

  /**
   * Lê o estado atual. Nunca lança: em caso de erro devolve um snapshot com
   * `ok=false` e a mensagem.
   * @param {number} nowMs instante real (o runtime usa o relógio real)
   */
  poll(nowMs = Date.now()) {
    this.polls++;
    const snap = emptySnapshot(this.name);
    snap.capturedAtMs = nowMs;
    snap.ritmo.horasAtivas = this.horasAtivas;
    snap.ritmo.fase = fasePeloRelogio(this.horasAtivas, localParts(nowMs, this.timeZone).minuteOfDay);
    try {
      this.#open();
      this.#read(snap, nowMs);
      snap.ok = true;
      this.lastOkMs = nowMs;
      if (this.lastError) log.info('leitura do runtime normalizada');
      this.lastError = null;
    } catch (e) {
      this.failures++;
      const msg = e?.message ?? String(e);
      if (msg !== this.lastError) log.warn('falha ao ler o runtime', { erro: msg });
      this.lastError = msg;
      snap.ok = false;
      snap.error = msg;
      this.#close();
    }
    return snap;
  }

  #read(snap, nowMs) {
    const db = this.db;
    db.exec('BEGIN');
    try {
      snap.runtimeSchemaVersion = this.schemaVersion;

      // Daemon (tabela estado_daemon: chave/valor).
      const kv = {};
      for (const row of db.prepare('SELECT chave, valor FROM estado_daemon').all()) kv[row.chave] = row.valor;
      const num = (k) => (kv[k] !== undefined && Number.isFinite(Number(kv[k])) ? Number(kv[k]) : null);
      const d = snap.daemon;
      d.pid = num('pid');
      d.iniciadoMs = num('iniciado_ms');
      d.sinalDeVidaMs = num('sinal_de_vida_ms');
      d.paradoMs = num('parado_ms');
      // Mesma regra do `abiyss status --verificar`: sinal de vida mais velho
      // que 3 verificações de cron = o loop não está andando.
      const limite = this.cronVerificacaoSegundos * 3 * 1000;
      if (d.iniciadoMs === null && d.sinalDeVidaMs === null) d.estado = 'desconhecido';
      else if (d.paradoMs !== null && d.iniciadoMs !== null && d.paradoMs >= d.iniciadoMs) d.estado = 'parado';
      else if (d.sinalDeVidaMs !== null && nowMs - d.sinalDeVidaMs <= limite) d.estado = 'rodando';
      else d.estado = 'parado';

      // Sub-agentes: vivos + terminados recentemente.
      const corte = nowMs - this.recentFinishedMs;
      snap.subagentes = db.prepare(
        `SELECT id, nivel, tarefa, goal_id, origem, estado, criado_ms, iniciado_ms, terminado_ms,
                prazo_segundos, relatorio, tokens, rodadas
           FROM subagentes
          WHERE estado IN ('pendente', 'executando') OR terminado_ms >= ?
          ORDER BY id DESC LIMIT 64`,
      ).all(corte).map((r) => ({
        id: r.id, nivel: r.nivel, estado: r.estado, tarefa: r.tarefa, goalId: r.goal_id, origem: r.origem,
        criadoMs: r.criado_ms, iniciadoMs: r.iniciado_ms, terminadoMs: r.terminado_ms,
        prazoSegundos: r.prazo_segundos, relatorio: parseRelatorio(r.relatorio), tokens: r.tokens, rodadas: r.rodadas,
      }));

      // Goals: ativos + finalizados nas últimas 24 h.
      const lista = db.prepare(
        `SELECT id, titulo, estado, prioridade, atualizado_ms FROM goals
          WHERE estado NOT IN ('concluido', 'abandonado') OR atualizado_ms >= ?
          ORDER BY prioridade DESC, id ASC LIMIT 50`,
      ).all(nowMs - 24 * 3600_000).map((g) => ({
        id: g.id, titulo: g.titulo, estado: g.estado, prioridade: g.prioridade, atualizadoMs: g.atualizado_ms,
      }));
      const contagem = {};
      for (const r of db.prepare('SELECT estado, COUNT(*) AS n FROM goals GROUP BY estado').all()) contagem[r.estado] = r.n;
      snap.goals = { lista, foco: goalEmFoco(lista), contagem };

      // Ciclos do heartbeat (o runtime grava o ciclo inteiro ao terminar).
      snap.ciclos = db.prepare(
        `SELECT id, inicio_ms, fim_ms, chamou_modelo, motivo, goal_foco, resultado, erro, tokens
           FROM ciclos ORDER BY id DESC LIMIT 8`,
      ).all().map((c) => ({
        id: c.id, inicioMs: c.inicio_ms, fimMs: c.fim_ms, chamouModelo: c.chamou_modelo === 1,
        motivo: c.motivo, goalFoco: c.goal_foco, resultado: c.resultado, erro: c.erro, tokens: c.tokens,
      }));

      // Fila de eventos (sem o conteúdo: pode ser grande e é dado externo).
      snap.eventos.pendentes = db.prepare('SELECT COUNT(*) AS n FROM fila_eventos WHERE consumido_ms IS NULL').get().n;
      snap.eventos.recentes = db.prepare(
        'SELECT id, momento_ms, tipo, origem, consumido_ms FROM fila_eventos ORDER BY id DESC LIMIT 12',
      ).all().map((e) => ({ id: e.id, momentoMs: e.momento_ms, tipo: e.tipo, origem: e.origem, consumido: e.consumido_ms !== null }));

      // Conversa com o dono e chamadas do sono (últimos 10 minutos; índice por momento).
      const recentes = db.prepare(
        `SELECT origem, MAX(momento_ms) AS ultimo FROM chamadas_modelo
          WHERE momento_ms >= ? GROUP BY origem`,
      ).all(nowMs - 10 * 60_000);
      for (const r of recentes) {
        if (r.origem === 'conversa') snap.conversa.ultimaChamadaMs = r.ultimo;
        if (r.origem === 'sono' && nowMs - r.ultimo < 3 * 60_000) {
          snap.sono = { ativo: true, evidencia: 'chamadas_modelo com origem "sono" nos últimos 3 min' };
        }
      }

      // Sono (E5, provisório): tabela `sonos` ainda não publicada no kernel.
      if (this.hasSonos && !snap.sono.ativo) {
        try {
          const s = db.prepare('SELECT estado FROM sonos ORDER BY id DESC LIMIT 1').get();
          if (s && SONO_ATIVO.includes(String(s.estado))) {
            snap.sono = { ativo: true, evidencia: `sonos.estado = ${s.estado}` };
          }
        } catch {
          // Formato diferente do esperado: ignora (a fase segue pelo relógio).
        }
      }
      if (snap.sono.ativo) {
        snap.ritmo.fase = FASE.SONO;
        snap.ritmo.origemFase = 'sono';
      }
      db.exec('COMMIT');
    } catch (e) {
      try { db.exec('ROLLBACK'); } catch { /* ignorar */ }
      throw e;
    }
  }

  status() {
    return {
      source: this.name,
      db: this.dbPath,
      connected: this.db !== null && this.lastError === null,
      lastError: this.lastError,
      lastOkMs: this.lastOkMs,
      polls: this.polls,
      failures: this.failures,
      schemaVersion: this.schemaVersion ?? null,
    };
  }
}
