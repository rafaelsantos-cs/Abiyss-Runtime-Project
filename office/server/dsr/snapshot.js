// DSR provisório — formato normalizado do estado do runtime.
//
// "DSR"/"DSR Sync" é citado como a arquitetura de estados/sincronização do
// ecossistema, mas NÃO existe nenhuma especificação nem código com esse nome
// no repositório (verificado em todas as branches em 2026-10-05). Este módulo
// é a camada provisória, isolada, que normaliza o que o runtime realmente
// grava (SQLite) num formato estável para o escritório. Quando o DSR oficial
// existir, só esta pasta (server/dsr/) precisa mudar.
//
// Regras:
// - valores de estado são os do runtime, sem tradução (ver shared/states.js);
// - nomes de campo em camelCase, espelhando as colunas do runtime;
// - o escritório só LÊ o runtime; nunca escreve nele.

import { GOAL_ESTADOS, SUBAGENTE_ESTADOS, NIVEIS, FASES, DAEMON_ESTADOS } from '../../shared/states.js';

export const DSR_SCHEMA = 'abiyss-office/dsr-provisorio@1';

export function emptySnapshot(source) {
  return {
    schema: DSR_SCHEMA,
    source,
    ok: false,
    error: null,
    capturedAtMs: 0,
    runtimeSchemaVersion: null,
    daemon: { estado: 'desconhecido', pid: null, iniciadoMs: null, sinalDeVidaMs: null, paradoMs: null },
    ritmo: { fase: 'vigília', horasAtivas: '07:00-23:00', origemFase: 'relogio' },
    subagentes: [],
    goals: { lista: [], foco: null, contagem: {} },
    ciclos: [],
    eventos: { pendentes: 0, recentes: [] },
    conversa: { ultimaChamadaMs: null },
    sono: { ativo: false, evidencia: null },
  };
}

/**
 * Goal em foco — porte de goals::em_foco: executando > validando >
 * comprometido > proposto; depois maior prioridade; depois menor id.
 */
export function goalEmFoco(goals) {
  const ordem = { executando: 0, validando: 1, comprometido: 2, proposto: 3 };
  let melhor = null;
  for (const g of goals) {
    const o = ordem[g.estado];
    if (o === undefined) continue;
    const chave = [o, -g.prioridade, g.id];
    if (!melhor || chave[0] < melhor.chave[0]
      || (chave[0] === melhor.chave[0] && (chave[1] < melhor.chave[1]
      || (chave[1] === melhor.chave[1] && chave[2] < melhor.chave[2])))) {
      melhor = { chave, g };
    }
  }
  return melhor ? melhor.g : null;
}

/** Validação defensiva: devolve a lista de problemas (vazia = ok). */
export function validateSnapshot(s) {
  const p = [];
  if (s.schema !== DSR_SCHEMA) p.push(`schema inesperado: ${s.schema}`);
  if (!DAEMON_ESTADOS.includes(s.daemon.estado)) p.push(`daemon.estado inválido: ${s.daemon.estado}`);
  if (!FASES.includes(s.ritmo.fase)) p.push(`ritmo.fase inválida: ${s.ritmo.fase}`);
  for (const sa of s.subagentes) {
    if (!SUBAGENTE_ESTADOS.includes(sa.estado)) p.push(`subagente ${sa.id}: estado inválido ${sa.estado}`);
    if (!NIVEIS.includes(sa.nivel)) p.push(`subagente ${sa.id}: nível inválido ${sa.nivel}`);
  }
  for (const g of s.goals.lista) {
    if (!GOAL_ESTADOS.includes(g.estado)) p.push(`goal ${g.id}: estado inválido ${g.estado}`);
  }
  return p;
}

/** Resumo pequeno (HUD, debug e /api/health). */
export function summarize(s) {
  const porEstado = {};
  for (const sa of s.subagentes) porEstado[sa.estado] = (porEstado[sa.estado] ?? 0) + 1;
  const ultimoCiclo = s.ciclos[0] ?? null;
  return {
    source: s.source,
    ok: s.ok,
    error: s.error,
    capturedAtMs: s.capturedAtMs,
    runtimeSchemaVersion: s.runtimeSchemaVersion,
    daemon: s.daemon.estado,
    fase: s.ritmo.fase,
    horasAtivas: s.ritmo.horasAtivas,
    sono: s.sono.ativo,
    subagentes: porEstado,
    goalFoco: s.goals.foco ? { id: s.goals.foco.id, titulo: s.goals.foco.titulo, estado: s.goals.foco.estado } : null,
    goals: s.goals.lista.slice(0, 6).map((g) => ({ id: g.id, titulo: g.titulo, estado: g.estado, prioridade: g.prioridade })),
    goalsContagem: s.goals.contagem,
    eventosPendentes: s.eventos.pendentes,
    eventosRecentes: s.eventos.recentes.slice(0, 6),
    ultimoCiclo: ultimoCiclo && {
      id: ultimoCiclo.id, fimMs: ultimoCiclo.fimMs, chamouModelo: ultimoCiclo.chamouModelo,
      motivo: ultimoCiclo.motivo, erro: ultimoCiclo.erro,
    },
    conversaMs: s.conversa.ultimaChamadaMs,
  };
}
