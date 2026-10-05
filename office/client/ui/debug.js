// Painel de debug (tecla D): estado do Abiyss e de cada IMMo, destinos,
// métricas de renderização e de sincronização, fonte do runtime e o
// registro de acontecimentos. Feito para desenvolvimento.

import { esc } from './hud.js';
import { IMMO_ACTIVITIES, ABIYSS_ACTIVITIES } from '../../shared/states.js';

const fmtMs = (ms) => {
  if (ms === null || ms === undefined) return '—';
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return `${h}h${String(m).padStart(2, '0')}m${String(s % 60).padStart(2, '0')}s`;
};

const ago = (ms, now) => (ms ? `${Math.round((now - ms) / 1000)} s atrás` : '—');

export class DebugPanel {
  constructor(el) {
    this.el = el;
    this.visible = false;
    this.lastRender = 0;
  }

  toggle(v = !this.visible) {
    this.visible = v;
    this.el.hidden = !v;
  }

  update({ sample, meta, sync, perf, hello }) {
    if (!this.visible) return;
    const now = performance.now();
    if (now - this.lastRender < 400) return;
    this.lastRender = now;
    const rt = meta?.runtime ?? {};
    const st = rt.status ?? {};
    const stats = meta?.stats ?? {};
    const ents = sample ? [...sample.entities.values()] : [];
    const ab = ents.find((e) => e.kind === 'abiyss');
    const kv = (rows) => `<div class="kv">${rows.map(([k, v]) => `<span>${esc(k)}</span><span>${v}</span>`).join('')}</div>`;
    const wall = sample?.frame?.wallMs ?? Date.now();
    const immoRows = ents.filter((e) => e.kind === 'immo').map((e) => {
      const lg = e.logical ?? {};
      return `<tr><td>${esc(e.name)}</td><td>${lg.subagenteId ? `#${lg.subagenteId} ${esc(lg.nivel)}<br><b>${esc(lg.estado)}</b>` : '—'}</td>` +
        `<td>${esc(IMMO_ACTIVITIES[e.act] ?? e.act)}<br><span class="muted">${esc(e.mode)} · ${esc(e.pose)}</span></td>` +
        `<td>${esc(e.dest?.poi ?? e.poi ?? '—')}${e.dest ? `<br><span class="muted">(${e.dest.x}, ${e.dest.z})</span>` : ''}</td>` +
        `<td>${Math.round((e.energy ?? 0) * 100)}%</td></tr>`;
    }).join('');
    this.el.innerHTML = `
      <h3>Renderização</h3>
      ${kv([
        ['FPS', perf.fps.toFixed(0)],
        ['quadro (ms)', perf.frameMs.toFixed(1)],
        ['draw calls / triângulos', `${perf.calls} / ${perf.triangles}`],
        ['entidades presentes', String(stats.entities ?? ents.filter((e) => e.present).length)],
      ])}
      <h3>Simulação e sincronização</h3>
      ${kv([
        ['conexão', esc(sync.status)],
        ['quadros/s recebidos', sync.framesPerSecond.toFixed(1)],
        ['silêncio', `${Math.round(sync.silenceMs)} ms`],
        ['tick do servidor', String(sample?.frame?.tick ?? '—')],
        ['tempo de simulação', fmtMs(sample?.simMs)],
        ['escala de tempo', `${sync.rate.toFixed(2)}× ${hello?.server?.clockReal === false ? '(relógio simulado)' : '(hora real)'}`],
        ['tick médio no servidor', `${stats.tickMsAvg ?? '—'} ms`],
        ['invariantes violadas', String(stats.violations ?? '—')],
        ['sobreposições / travamentos', `${stats.overlaps ?? '—'} / ${stats.stuck ?? '—'}`],
        ['replanejamentos', String(stats.replans ?? '—')],
      ])}
      <h3>Fonte do runtime (DSR provisório)</h3>
      ${kv([
        ['fonte', esc(rt.source ?? '—')],
        ['leitura', rt.ok ? '<span style="color:var(--ok)">ok</span>' : `<span style="color:var(--bad)">${esc(rt.error ?? 'falha')}</span>`],
        ['banco', esc(st.db ?? '—')],
        ['migração do banco', esc(rt.runtimeSchemaVersion ?? '—')],
        ['leituras / falhas', `${st.polls ?? '—'} / ${st.failures ?? 0}`],
        ['última leitura', ago(rt.capturedAtMs, wall)],
        ['daemon', esc(rt.daemon ?? '—')],
        ['fase (horas ativas)', `${esc(rt.fase)} (${esc(rt.horasAtivas)})`],
        ['sub-agentes', esc(JSON.stringify(rt.subagentes ?? {}))],
        ['sem IMMo (transbordo)', esc((rt.overflow ?? []).join(', ') || '—')],
        ['goal em foco', rt.goalFoco ? `#${rt.goalFoco.id} ${esc(rt.goalFoco.titulo)} (${esc(rt.goalFoco.estado)})` : '—'],
        ['eventos pendentes', String(rt.eventosPendentes ?? 0)],
        ['último heartbeat', rt.ultimoCiclo ? `#${rt.ultimoCiclo.id} ${esc(rt.ultimoCiclo.motivo)}` : '—'],
        ['conversa com o dono', ago(rt.conversaMs, wall)],
      ])}
      <h3>Abiyss</h3>
      ${ab ? kv([
        ['modo / atividade', `${esc(ab.mode)} / ${esc(ABIYSS_ACTIVITIES[ab.act] ?? ab.act)}`],
        ['mente', esc(ab.mind ? `${ab.mind.kind}: ${ab.mind.label}` : '—')],
        ['intenção', esc(ab.intent ?? '—')],
        ['posição', `(${ab.x.toFixed(2)}, ${ab.z.toFixed(2)}) · POI ${esc(ab.poi ?? '—')}`],
        ['destino', esc(ab.dest ? `${ab.dest.poi ?? 'ponto'} (${ab.dest.x}, ${ab.dest.z})` : '—')],
      ]) : '—'}
      <h3>IMMos</h3>
      <table><tr><th>IMMo</th><th>sub-agente</th><th>atividade</th><th>destino</th><th>energia</th></tr>${immoRows}</table>
      <h3>Clima</h3>
      ${meta?.env ? kv([
        ['fonte', esc(`${meta.env.weather.source}${meta.env.weather.estimated ? ' (estimado)' : ''}`)],
        ['erro da API', esc(meta.env.weather.error ?? '—')],
        ['sol (elev./azim.)', `${meta.env.sun.elevation}° / ${meta.env.sun.azimuth}°`],
        ['luz', esc(`${meta.env.light.phase} · lâmpadas ${meta.env.light.lampsOn ? 'acesas' : 'apagadas'}`)],
        ['nuvens / chuva', `${meta.env.weather.cloudCover}% / ${meta.env.weather.rain}`],
      ]) : '—'}
      <h3>Acontecimentos</h3>
      <div class="journal">${(meta?.journal ?? []).slice().reverse().map((j) => `<div>${esc(j.text)}</div>`).join('')}</div>`;
  }
}
