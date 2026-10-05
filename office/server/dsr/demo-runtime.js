// Fonte "demo": um emulador DETERMINÍSTICO do runtime do Abiyss.
//
// Serve para o escritório funcionar sozinho (sem o daemon em Rust, sem chaves
// do NIM). Ele reproduz a SEMÂNTICA do kernel e entrega instantâneos no MESMO
// formato do adaptador do banco real, então o resto do escritório não sabe
// (nem precisa saber) de onde veio o estado:
//
// - heartbeat: no máximo uma "chamada ao modelo" por ciclo, só se houver
//   evento novo, mudança no goal em foco ou revisão periódica vencida; o
//   ciclo é registrado ao terminar (como `registrar_ciclo`);
// - executor de sub-agentes: `pendente` → `executando` em até 5 s, no
//   máximo 4 simultâneos, prioridade Ultra > Medium > Low e depois id;
// - fim do sub-agente: concluido | falhou | expirado | cancelado, com
//   relatório estruturado e evento `subagente` na fila;
// - goals: só transições permitidas por `EstadoGoal::proximos_permitidos`;
// - ritmo: vigília/descanso pelo relógio (mesma regra) e sono numa janela
//   noturna (como o `[sono] inicio = "03:00"` da sessão E5).
//
// Os intervalos são encurtados em relação ao runtime real (heartbeat de
// 300 s vira ~30 s) para dar para ver o escritório "trabalhando" em poucos
// minutos. Isso está documentado em docs/STATES.md.

import { emptySnapshot, goalEmFoco } from './snapshot.js';
import { fasePeloRelogio, FASE, Janela } from './ritmo.js';
import { GOAL_TRANSICOES } from '../../shared/states.js';
import { Rng } from '../world/rng.js';
import { localParts } from '../world/clock.js';

const S = 1000;

export const DEMO_TAREFAS = [
  'Revisar o diário de ontem e listar lições',
  'Resumir o changelog do qmd',
  'Contar palavras dos relatórios da semana',
  'Verificar links vencidos em 02_external',
  'Comparar a latência p95 dos modelos',
  'Organizar as referências da skill de pesquisa',
  'Ler a documentação do Piper (TTS)',
  'Rascunhar o plano do canal do Discord',
  'Checar o uso de disco do cofre',
  'Classificar as propostas de memória pendentes',
  'Pesquisar parâmetros do Whisper para ARM',
  'Montar o resumo dos goals ativos',
  'Validar o backup noturno do banco',
  'Listar skills candidatas a emergir',
];

const DEMO_GOALS = [
  { titulo: 'Migrar a memória do Hermes', prioridade: 2, estado: 'executando' },
  { titulo: 'Calibrar o orçamento diário', prioridade: 1, estado: 'comprometido' },
  { titulo: 'Organizar as skills de pesquisa', prioridade: 0, estado: 'proposto' },
];

const NOVOS_GOALS = [
  'Preparar o canal do WhatsApp',
  'Revalidar fontes externas vencidas',
  'Medir o custo de tokens por goal',
  'Escrever a skill de backup',
  'Estudar o roteiro de voz (STT/TTS)',
];

const DURACAO = { low: [45, 90], medium: [60, 140], ultra: [80, 200] };
const PRAZO = { low: 600, medium: 900, ultra: 1800 };

export class DemoRuntimeSource {
  /**
   * @param {object} opts
   * @param {number} opts.seed
   * @param {string} opts.horasAtivas
   * @param {string} opts.timeZone
   * @param {{inicio: string, minutos: number}} [opts.sono] janela do sono
   * @param {number} [opts.maxSimultaneos]
   */
  constructor({ seed, horasAtivas, timeZone, sono = { inicio: '03:00', minutos: 45 }, maxSimultaneos = 4 }) {
    this.name = 'demo';
    this.seed = seed;
    this.horasAtivas = horasAtivas;
    this.timeZone = timeZone;
    this.sonoJanela = (() => {
      const i = Janela.deTexto(`${sono.inicio}-${sono.inicio}`).inicio;
      const f = (i + sono.minutos) % (24 * 60);
      return new Janela(i, f);
    })();
    this.maxSimultaneos = maxSimultaneos;
    this.rng = new Rng(seed);
    this.started = false;
    this.polls = 0;
    this.runId = Date.now().toString(36);
  }

  /**
   * O emulador recomeça do zero a cada processo (ids de sub-agente
   * reiniciam), então a chave muda a cada execução: posições são
   * restauradas, atribuições de sub-agentes não.
   */
  get key() {
    return `demo:${this.seed}:${this.runId}`;
  }

  async start() {}

  stop() {}

  // --- agenda de acontecimentos ---------------------------------------------

  #schedule(at, kind, data = {}) {
    this.agenda.push({ at: Math.round(at), kind, data, seq: this.agendaSeq++ });
  }

  #nextItem(untilMs) {
    let best = -1;
    for (let i = 0; i < this.agenda.length; i++) {
      const it = this.agenda[i];
      if (it.at > untilMs) continue;
      if (best < 0 || it.at < this.agenda[best].at || (it.at === this.agenda[best].at && it.seq < this.agenda[best].seq)) best = i;
    }
    if (best < 0) return null;
    return this.agenda.splice(best, 1)[0];
  }

  #init(nowMs) {
    this.started = true;
    this.startMs = nowMs;
    this.agenda = [];
    this.agendaSeq = 0;
    this.ids = { sub: 0, goal: 0, ciclo: 0, evento: 0 };
    this.subs = [];
    this.ciclos = [];
    this.fila = [];
    this.goals = DEMO_GOALS.map((g) => this.#newGoal(g.titulo, g.prioridade, g.estado, nowMs));
    this.daemon = { iniciadoMs: nowMs, sinalDeVidaMs: nowMs, pid: 4242 };
    this.conversaMs = null;
    this.lastModelCallMs = null;
    this.lastFocoId = null;
    this.chatUntil = 0;
    this.sonoAtivo = false;
    this.tarefaIdx = this.rng.int(0, DEMO_TAREFAS.length - 1);

    // Abertura roteirizada (determinística): delega cedo para dar para ver
    // IMMos trabalhando, um IMMo livre, relatórios e o Abiyss reagindo.
    this.#schedule(nowMs + 6 * S, 'heartbeat', { script: ['ultra', 'medium'] });
    this.#schedule(nowMs + 24 * S, 'heartbeat', { script: ['low'] });
    this.#schedule(nowMs + 60 * S, 'heartbeat');
    this.#schedule(nowMs + 5 * S, 'executor');
    this.#schedule(nowMs + 30 * S, 'sinal');
    this.#schedule(nowMs + 150 * S, 'cron', { nome: 'revisar-fila' });
    this.#schedule(nowMs + 200 * S, 'conversa');
    this.#schedule(nowMs + 260 * S, 'goal-novo');
  }

  #newGoal(titulo, prioridade, estado, ms) {
    return { id: ++this.ids.goal, titulo, prioridade, estado, atualizadoMs: ms };
  }

  #publicar(tipo, origem, ms) {
    this.fila.push({ id: ++this.ids.evento, momentoMs: ms, tipo, origem, consumidoMs: null });
  }

  #fase(ms) {
    const m = localParts(ms, this.timeZone).minuteOfDay;
    if (this.sonoJanela.contem(m)) return FASE.SONO;
    return fasePeloRelogio(this.horasAtivas, m);
  }

  #ativos() {
    return this.subs.filter((s) => s.estado === 'pendente' || s.estado === 'executando');
  }

  #delegar(nivel, ms, goalId) {
    const tarefa = DEMO_TAREFAS[this.tarefaIdx++ % DEMO_TAREFAS.length];
    const [a, b] = DURACAO[nivel];
    let dur = this.rng.range(a, b) * S;
    let prazo = PRAZO[nivel];
    const r = this.rng.next();
    let desfecho = 'concluido';
    if (r < 0.09) desfecho = 'falhou';
    else if (r < 0.15) {
      desfecho = 'expirado';
      prazo = Math.round(dur / S * 0.8);
      dur = prazo * S;
    }
    const sub = {
      id: ++this.ids.sub, nivel, estado: 'pendente', tarefa, goalId: goalId ?? null, origem: 'heartbeat',
      criadoMs: ms, iniciadoMs: null, terminadoMs: null, prazoSegundos: prazo, relatorio: null,
      tokens: 0, rodadas: 0, _dur: dur, _desfecho: desfecho,
    };
    this.subs.push(sub);
    // `delegar` acorda o executor na hora (Notify).
    this.#schedule(ms + 400, 'executor', { once: true });
    return sub;
  }

  #terminar(sub, estado, ms, resumo) {
    sub.estado = estado;
    sub.terminadoMs = ms;
    const conf = estado === 'concluido' ? Math.round(this.rng.range(0.6, 0.95) * 100) / 100 : 0;
    const status = estado === 'concluido' ? (this.rng.chance(0.15) ? 'parcial' : 'concluido') : 'falhou';
    sub.relatorio = { status, resumo, confianca: conf };
    sub.tokens = Math.round(this.rng.range(2000, 30000));
    sub.rodadas = this.rng.int(1, 8);
    this.#publicar('subagente', String(sub.id), ms);
  }

  // --- acontecimentos -------------------------------------------------------

  #run(item) {
    const ms = item.at;
    switch (item.kind) {
      case 'sinal': {
        this.daemon.sinalDeVidaMs = ms;
        this.#schedule(ms + 30 * S, 'sinal');
        break;
      }
      case 'executor': {
        const executando = this.subs.filter((s) => s.estado === 'executando').length;
        let vagas = this.maxSimultaneos - executando;
        const ordem = { ultra: 0, medium: 1, low: 2 };
        const pend = this.subs.filter((s) => s.estado === 'pendente')
          .sort((a, b) => ordem[a.nivel] - ordem[b.nivel] || a.id - b.id);
        for (const s of pend) {
          if (vagas <= 0) break;
          s.estado = 'executando';
          s.iniciadoMs = ms;
          vagas--;
          this.#schedule(ms + s._dur, 'fim-subagente', { id: s.id });
        }
        if (!item.data.once) this.#schedule(ms + 5 * S, 'executor', {});
        break;
      }
      case 'fim-subagente': {
        const s = this.subs.find((x) => x.id === item.data.id);
        if (!s || s.estado !== 'executando') break;
        const resumos = {
          concluido: `Tarefa feita: ${s.tarefa.toLowerCase()}.`,
          falhou: 'Não consegui terminar: ferramenta indisponível.',
          expirado: 'Prazo esgotado antes de terminar.',
        };
        this.#terminar(s, s._desfecho, ms, resumos[s._desfecho]);
        break;
      }
      case 'heartbeat': {
        this.#heartbeat(ms, item.data.script ?? null);
        break;
      }
      case 'ciclo-fim': {
        this.#fimDoCiclo(ms, item.data);
        break;
      }
      case 'cron': {
        this.#publicar('cron', item.data.nome, ms);
        this.#schedule(ms + this.rng.range(200, 320) * S, 'cron', { nome: this.rng.pick(['revisar-fila', 'bom-dia', 'checar-disco']) });
        break;
      }
      case 'conversa': {
        // O dono conversa por ~1 min (chamadas com origem "conversa").
        this.chatUntil = ms + this.rng.range(45, 90) * S;
        this.conversaMs = ms;
        this.#schedule(ms + 10 * S, 'conversa-msg');
        this.#schedule(ms + this.rng.range(360, 600) * S, 'conversa');
        break;
      }
      case 'conversa-msg': {
        if (ms <= this.chatUntil) {
          this.conversaMs = ms;
          this.#schedule(ms + this.rng.range(8, 16) * S, 'conversa-msg');
        }
        break;
      }
      case 'goal-novo': {
        const vivos = this.goals.filter((g) => !['concluido', 'abandonado'].includes(g.estado));
        if (vivos.length < 4) {
          const titulo = NOVOS_GOALS[(this.ids.goal) % NOVOS_GOALS.length];
          this.goals.push(this.#newGoal(titulo, this.rng.int(0, 2), 'proposto', ms));
        }
        this.#schedule(ms + this.rng.range(420, 720) * S, 'goal-novo');
        break;
      }
      default:
        break;
    }
  }

  #heartbeat(ms, script) {
    const fase = this.#fase(ms);
    if (fase === FASE.SONO) {
      // O sono pausa o heartbeat (E5).
      this.#proximoHeartbeat(ms, 60);
      return;
    }
    const novos = this.fila.filter((e) => e.consumidoMs === null).map((e) => e.id);
    const foco = goalEmFoco(this.goals);
    let motivo = null;
    if (novos.length) motivo = `${novos.length} evento(s) novo(s) na fila`;
    else if (foco && this.lastFocoId === null) motivo = `primeiro ciclo com o goal #${foco.id}`;
    else if (foco && foco.id !== this.lastFocoId) motivo = `o goal #${foco.id} mudou desde o último ciclo`;
    else if (foco && fase === FASE.VIGILIA && (this.lastModelCallMs === null || ms - this.lastModelCallMs > 120 * S)) {
      motivo = `revisão periódica do goal #${foco.id}`;
    }
    if (script) motivo = motivo ?? `primeiro ciclo com o goal #${foco?.id ?? 1}`;

    if (!motivo) {
      this.ciclos.push({
        id: ++this.ids.ciclo, inicioMs: ms, fimMs: ms + 3, chamouModelo: false,
        motivo: 'nada novo: sem eventos e sem goal que precise de atenção', goalFoco: foco?.id ?? null,
        resultado: null, erro: null, tokens: 0,
      });
      this.#proximoHeartbeat(ms, fase === FASE.VIGILIA ? 30 : 90);
      return;
    }
    // Uma chamada ao modelo (latência simulada). As ações acontecem quando a
    // resposta chega, no fim do ciclo — e só então o ciclo é gravado.
    const fim = ms + Math.round(this.rng.range(3, 8) * S);
    this.#schedule(fim, 'ciclo-fim', { inicio: ms, motivo, script, focoId: foco?.id ?? null, eventos: novos });
  }

  #fimDoCiclo(fim, { inicio, motivo, script, focoId, eventos }) {
    const fase = this.#fase(fim);
    const foco = this.goals.find((g) => g.id === focoId) ?? null;
    const resultados = [];
    const ativos = this.#ativos().length;
    let delegar = [];
    if (script) delegar = script;
    else if (fase === FASE.VIGILIA && ativos < this.maxSimultaneos && this.rng.chance(0.7)) {
      delegar = [this.rng.weighted([{ value: 'low', weight: 45 }, { value: 'medium', weight: 35 }, { value: 'ultra', weight: 20 }])];
      if (ativos <= 1 && this.rng.chance(0.35)) delegar.push('low');
    } else if (fase === FASE.DESCANSO && ativos < 2 && this.rng.chance(0.25)) {
      delegar = ['low'];
    }
    for (const nivel of delegar) {
      const sub = this.#delegar(nivel, fim, focoId);
      resultados.push(`sub-agente ${sub.id} (${nivel}) delegado`);
    }
    if (!script && foco && this.rng.chance(0.25)) {
      const opcoes = GOAL_TRANSICOES[foco.estado].filter((e) => e !== 'abandonado' || this.rng.chance(0.1));
      const para = this.#avancarGoal(foco.estado, opcoes);
      if (para) {
        foco.estado = para;
        foco.atualizadoMs = fim;
        resultados.push(`goal #${foco.id} → ${para}`);
      }
    }
    const executando = this.subs.filter((x) => x.estado === 'executando');
    if (!script && executando.length > 0 && this.rng.chance(0.04)) {
      const alvo = this.rng.pick(executando);
      this.#terminar(alvo, 'cancelado', fim, 'cancelado pelo Abiyss');
      resultados.push(`cancelamento do sub-agente ${alvo.id} solicitado`);
    }
    if (resultados.length === 0) resultados.push('aguardando (nada urgente)');
    // Só os eventos vistos no começo do ciclo são consumidos.
    for (const e of this.fila) if (eventos.includes(e.id)) e.consumidoMs = fim;
    this.lastModelCallMs = fim;
    this.lastFocoId = goalEmFoco(this.goals)?.id ?? null;
    this.ciclos.push({
      id: ++this.ids.ciclo, inicioMs: inicio, fimMs: fim, chamouModelo: true, motivo,
      goalFoco: focoId, resultado: resultados.join('\n'), erro: null,
      tokens: Math.round(this.rng.range(3000, 12000)),
    });
    if (!script) this.#proximoHeartbeat(fim, fase === FASE.VIGILIA ? 32 : 120);
  }

  #proximoHeartbeat(ms, base) {
    this.#schedule(ms + Math.round(base * this.rng.range(0.85, 1.25) * S), 'heartbeat');
  }

  /** Prefere avançar o goal (proposto → comprometido → executando → ...). */
  #avancarGoal(estado, opcoes) {
    const preferido = {
      proposto: 'comprometido', comprometido: 'executando', executando: 'validando',
      validando: 'concluido', bloqueado: 'executando',
    }[estado];
    if (preferido && opcoes.includes(preferido) && this.rng.chance(0.85)) return preferido;
    return opcoes.length ? this.rng.pick(opcoes) : null;
  }

  // --- leitura ---------------------------------------------------------------

  /** Pedido manual (POST /api/demo/delegate): um heartbeat que delega já. */
  delegate(nivel) {
    this.manual = [...(this.manual ?? []), nivel];
  }

  /** Avança o emulador até `nowMs` e devolve o instantâneo. */
  poll(nowMs) {
    this.polls++;
    if (!this.started) this.#init(nowMs);
    if (this.manual?.length) {
      this.#schedule(nowMs, 'heartbeat', { script: this.manual });
      this.manual = [];
    }
    for (let it = this.#nextItem(nowMs); it; it = this.#nextItem(nowMs)) this.#run(it);

    const snap = emptySnapshot(this.name);
    snap.ok = true;
    snap.capturedAtMs = nowMs;
    snap.runtimeSchemaVersion = null;
    snap.daemon = { estado: 'rodando', pid: this.daemon.pid, iniciadoMs: this.daemon.iniciadoMs, sinalDeVidaMs: this.daemon.sinalDeVidaMs, paradoMs: null };
    const fase = this.#fase(nowMs);
    snap.ritmo = { fase, horasAtivas: this.horasAtivas, origemFase: fase === FASE.SONO ? 'sono' : 'relogio' };
    snap.sono = fase === FASE.SONO ? { ativo: true, evidencia: 'demo: janela do sono' } : { ativo: false, evidencia: null };
    const corte = nowMs - 10 * 60 * S;
    snap.subagentes = this.subs
      .filter((s) => s.estado === 'pendente' || s.estado === 'executando' || s.terminadoMs >= corte)
      .sort((a, b) => b.id - a.id)
      .slice(0, 64)
      .map(({ _dur, _desfecho, ...pub }) => ({ ...pub, relatorio: pub.relatorio ? { ...pub.relatorio } : null }));
    const lista = this.goals
      .filter((g) => !['concluido', 'abandonado'].includes(g.estado) || g.atualizadoMs >= nowMs - 24 * 3600 * S)
      .sort((a, b) => b.prioridade - a.prioridade || a.id - b.id)
      .map((g) => ({ ...g }));
    const contagem = {};
    for (const g of this.goals) contagem[g.estado] = (contagem[g.estado] ?? 0) + 1;
    snap.goals = { lista, foco: goalEmFoco(lista), contagem };
    snap.ciclos = this.ciclos.slice(-8).reverse().map((c) => ({ ...c }));
    snap.eventos.pendentes = this.fila.filter((e) => e.consumidoMs === null).length;
    snap.eventos.recentes = this.fila.slice(-12).reverse()
      .map((e) => ({ id: e.id, momentoMs: e.momentoMs, tipo: e.tipo, origem: e.origem, consumido: e.consumidoMs !== null }));
    snap.conversa.ultimaChamadaMs = this.conversaMs;
    return snap;
  }

  status() {
    return { source: this.name, connected: true, lastError: null, polls: this.polls, seed: this.seed };
  }
}
