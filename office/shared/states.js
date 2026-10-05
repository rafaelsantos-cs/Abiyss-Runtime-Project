// Estados lógicos e atividades físicas.
//
// DUAS camadas, de propósito:
//
// 1. ESTADOS LÓGICOS — vêm do runtime do Abiyss e são copiados LITERALMENTE
//    do kernel Rust (branch claude/determined-carson-6qrkl4, commit 2318c40):
//      - sub-agente: kernel/src/subagentes.rs  (EstadoSubagente::como_texto)
//      - nível:      kernel/src/orquestrador/mod.rs (Nivel)
//      - goal:       kernel/src/goals.rs (EstadoGoal::como_texto)
//      - fase:       kernel/src/ritmo.rs (Fase::como_texto)
//      - eventos:    kernel/src/eventos.rs (TIPO_*)
//      - origem das chamadas ao modelo: kernel/src/orquestrador/mod.rs
//        (Origem::como_texto) — usada em runtime-sqlite.js
//    O escritório NUNCA inventa um valor novo para essas listas.
//
// 2. ATIVIDADES FÍSICAS — pertencem só ao escritório (o que o corpo do IMMo
//    ou do Abiyss está fazendo na cena: andando, sentado, dormindo...).
//    Elas são DERIVADAS dos estados lógicos + regras de comportamento.
//
// "DSR" não existe no repositório (nenhuma branch, nenhum documento). Por
// isso a camada de sincronização deste projeto é chamada de "DSR provisório"
// e fica isolada em server/dsr/. Veja docs/STATES.md.

// --- Runtime: sub-agentes ---------------------------------------------------

export const SUBAGENTE_ESTADOS = Object.freeze([
  'pendente', 'executando', 'concluido', 'falhou', 'cancelado', 'expirado',
]);
export const SUBAGENTE_FINAIS = Object.freeze(['concluido', 'falhou', 'cancelado', 'expirado']);
export const NIVEIS = Object.freeze(['ultra', 'medium', 'low']);
/** Status possíveis dentro do relatório estruturado (Relatorio.status). */
export const RELATORIO_STATUS = Object.freeze(['concluido', 'parcial', 'falhou']);

// --- Runtime: goals ---------------------------------------------------------

export const GOAL_ESTADOS = Object.freeze([
  'proposto', 'comprometido', 'executando', 'validando', 'concluido', 'bloqueado', 'abandonado',
]);
/** Transições permitidas (EstadoGoal::proximos_permitidos). */
export const GOAL_TRANSICOES = Object.freeze({
  proposto: ['comprometido', 'abandonado'],
  comprometido: ['executando', 'bloqueado', 'abandonado'],
  executando: ['validando', 'bloqueado', 'abandonado'],
  validando: ['concluido', 'executando', 'bloqueado', 'abandonado'],
  bloqueado: ['comprometido', 'executando', 'abandonado'],
  concluido: [],
  abandonado: [],
});

// --- Runtime: ritmo, eventos, daemon ---------------------------------------
// (A origem das chamadas ao modelo — conversa | autonomo | sono — é lida
// direto em runtime-sqlite.js.)

/** Fase do dia (texto exato do runtime, com acento). */
export const FASES = Object.freeze(['vigília', 'descanso', 'sono']);
/** Tipos de evento da fila (eventos.rs, TIPO_*). */
export const EVENTO_TIPOS = Object.freeze(['cron', 'subagente', 'skill', 'kernel', 'sono', 'usuario']);
/** Daemon: rodando/parado como em `abiyss status`; "desconhecido" = o escritório ainda não tem dados. */
export const DAEMON_ESTADOS = Object.freeze(['rodando', 'parado', 'desconhecido']);

// --- Escritório: atividades físicas ----------------------------------------

export const IMMO_ACTIVITIES = Object.freeze({
  entering: 'entrando',
  walking: 'andando',
  sitting_down: 'sentando',
  standing_up: 'levantando',
  working: 'trabalhando',
  seated_idle: 'sentado (ocioso)',
  idle: 'ocioso',
  wandering: 'passeando',
  resting: 'descansando',
  sleeping: 'dormindo',
  coffee: 'tomando café',
  water: 'bebendo água',
  chatting: 'conversando',
  window: 'olhando a janela',
  reading_board: 'lendo o quadro',
  reporting: 'entregando relatório',
  waiting: 'aguardando passagem',
});

export const ABIYSS_ACTIVITIES = Object.freeze({
  offline: 'desligado',
  waking: 'despertando',
  thinking: 'pensando (heartbeat)',
  delegating: 'delegando',
  receiving: 'recebendo relatório',
  conversing: 'conversando com o dono',
  supervising: 'supervisionando',
  moving: 'deslocando',
  observing: 'olhando a janela',
  resting: 'em descanso',
  sleeping: 'dormindo (sono)',
  idle: 'no console',
});

/** Cores de estado (usadas por cliente e documentação). */
export const COLORS = Object.freeze({
  nivel: { ultra: 0xb26bff, medium: 0x3d8bff, low: 0x2fcf8f },
  free: 0xe8edf5,
  resting: 0xffc857,
  sleeping: 0x6c6cff,
  reporting: 0xffd166,
  falhou: 0xff4d5e,
  expirado: 0xff9f43,
  cancelado: 0x9aa3b2,
  concluido: 0x3ddc84,
  abiyss: 0x48e5ff,
  abiyssSleep: 0x8f6bff,
  offline: 0x5b6070,
});

/** Ícone sugerido para cada atividade (o cliente desenha os ícones). */
export const ACTIVITY_ICON = Object.freeze({
  entering: 'walk', walking: 'walk', wandering: 'walk', waiting: 'wait',
  sitting_down: null, standing_up: null,
  working: 'gear', seated_idle: 'dots', idle: null,
  resting: 'rest', sleeping: 'zzz', coffee: 'cup', water: 'drop',
  chatting: 'chat', window: 'eye', reading_board: 'board', reporting: 'report',
});

export function isFinalSubagente(estado) {
  return SUBAGENTE_FINAIS.includes(estado);
}
