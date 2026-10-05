// HUD mínimo: hora local, clima de Contagem, estado do daemon, fase do
// ritmo e a conexão. O resto da informação está na própria cena.

import { COLORS, IMMO_ACTIVITIES, ABIYSS_ACTIVITIES } from '../../shared/states.js';
import { hex } from '../render/materials.js';

const $ = (id) => document.getElementById(id);

/** Escapa texto vindo do runtime (tarefas e motivos podem vir de um modelo). */
export function esc(v) {
  return String(v ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

const WX_ICON = {
  clear: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="5" fill="#ffd166"/><g stroke="#ffd166" stroke-width="2">' +
    [0, 45, 90, 135, 180, 225, 270, 315].map((a) => `<line x1="12" y1="2" x2="12" y2="4.5" transform="rotate(${a} 12 12)"/>`).join('') + '</g></svg>',
  cloudy: '<svg viewBox="0 0 24 24"><circle cx="9" cy="9" r="4" fill="#ffd166"/><path d="M7 19h10a4 4 0 0 0 0-8 5 5 0 0 0-9.6 1.6A3.2 3.2 0 0 0 7 19z" fill="#dfe6f0"/></svg>',
  overcast: '<svg viewBox="0 0 24 24"><path d="M6 18h12a4 4 0 0 0 0-8 5.5 5.5 0 0 0-10.6 1.6A3.3 3.3 0 0 0 6 18z" fill="#b8c2cf"/></svg>',
  fog: '<svg viewBox="0 0 24 24" stroke="#c9d2de" stroke-width="2"><line x1="3" y1="8" x2="21" y2="8"/><line x1="5" y1="12" x2="19" y2="12"/><line x1="3" y1="16" x2="21" y2="16"/></svg>',
  drizzle: '<svg viewBox="0 0 24 24"><path d="M6 14h12a4 4 0 0 0 0-8 5.5 5.5 0 0 0-10.6 1.6A3.3 3.3 0 0 0 6 14z" fill="#b8c2cf"/><g stroke="#7cc4ff" stroke-width="1.6"><line x1="8" y1="17" x2="7" y2="20"/><line x1="13" y1="17" x2="12" y2="20"/></g></svg>',
  rain: '<svg viewBox="0 0 24 24"><path d="M6 13h12a4 4 0 0 0 0-8 5.5 5.5 0 0 0-10.6 1.6A3.3 3.3 0 0 0 6 13z" fill="#9aa6b5"/><g stroke="#4da3ff" stroke-width="2"><line x1="7" y1="16" x2="5.5" y2="21"/><line x1="12" y1="16" x2="10.5" y2="21"/><line x1="17" y1="16" x2="15.5" y2="21"/></g></svg>',
  storm: '<svg viewBox="0 0 24 24"><path d="M6 12h12a4 4 0 0 0 0-8 5.5 5.5 0 0 0-10.6 1.6A3.3 3.3 0 0 0 6 12z" fill="#7d8896"/><path d="M12 13l-3 5h3l-2 5 5-7h-3l2-3z" fill="#ffd166"/></svg>',
  snow: '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="6" fill="#e5f0ff"/></svg>',
};

export class Hud {
  constructor() {
    this.el = $('hud');
    this.focus = $('hud-focus');
  }

  show() {
    this.el.hidden = false;
  }

  updateConnection(status) {
    const chip = $('hud-conn');
    chip.className = `chip ${status === 'conectado' ? 'ok' : status === 'reconectando' || status === 'conectando' ? 'warn' : 'bad'}`;
    chip.lastElementChild.textContent = status;
  }

  updateMeta(meta, hello) {
    if (!meta) return;
    const rt = meta.runtime ?? {};
    const env = meta.env;
    const src = $('hud-source');
    const demo = rt.source === 'demo';
    src.textContent = demo ? 'demo' : 'runtime';
    src.className = demo ? 'tag' : 'tag live';
    src.title = demo ? 'Fonte: emulador determinístico do runtime (sem daemon real)' : 'Fonte: banco SQLite do runtime real (somente leitura)';
    const d = $('hud-daemon');
    d.textContent = `daemon ${rt.daemon ?? '?'}${rt.ok === false ? ' (sem leitura)' : ''}`;
    d.className = `chip ${rt.daemon === 'rodando' && rt.ok !== false ? 'ok' : 'bad'}`;
    const f = $('hud-fase');
    f.textContent = `${rt.fase ?? '?'}${rt.horasAtivas ? ` · ativo ${rt.horasAtivas}` : ''}`;
    f.className = `chip ${rt.fase === 'vigília' ? 'ok' : 'warn'}`;
    if (env) {
      $('hud-date').textContent = `${env.local.date} · ${env.location.name}${hello?.server?.clockReal === false ? ' · relógio simulado' : ''}`;
      const w = env.weather;
      $('hud-weather-icon').innerHTML = WX_ICON[w.condition] ?? WX_ICON.cloudy;
      const tag = w.estimated ? (w.source === 'manual' ? 'manual' : 'estimado') : 'ao vivo';
      $('hud-weather').innerHTML = `<b>${Math.round(w.temperatureC)}°C</b> ${esc(w.label)} · <span title="${esc(w.source)}${w.error ? `: ${esc(w.error)}` : ''}">${tag}</span>`;
      $('hud-weather').title = `nascer do sol ${env.sun.sunrise} · pôr do sol ${env.sun.sunset}`;
    }
  }

  updateClock(clock) {
    if (clock) $('hud-time').textContent = clock.local.slice(0, 5);
  }

  /** Painel pequeno da entidade seguida pela câmera. */
  updateFocus(e) {
    if (!e) {
      this.focus.hidden = true;
      return;
    }
    this.focus.hidden = false;
    if (e.kind === 'abiyss') {
      this.focus.innerHTML = `<b>Abiyss</b> — ${esc(ABIYSS_ACTIVITIES[e.act] ?? e.act)}${e.mind ? `<br>${esc(e.mind.label)}` : ''}<br><span class="muted">${esc(e.intent)}</span>`;
    } else {
      const lg = e.logical ?? {};
      this.focus.innerHTML = `<b style="color:${hex(e.color)}">${esc(e.name)}</b> — ${esc(IMMO_ACTIVITIES[e.act] ?? e.act)}` +
        `<br>${lg.subagenteId ? `sub-agente #${esc(lg.subagenteId)} · ${esc(lg.nivel)} · <b>${esc(lg.estado)}</b><br><span class="muted">${esc(lg.tarefa)}</span>` : '<span class="muted">sem sub-agente (livre)</span>'}` +
        `<br><span class="muted">${esc(e.intent)} · energia ${Math.round((e.energy ?? 0) * 100)}%</span>`;
    }
  }
}

export function renderLegend(el) {
  const sw = (c) => `<span class="swatch" style="background:${hex(c)}"></span>`;
  el.innerHTML = `
    <h3>Cores (antena do IMMo, LED da mesa, ícones)</h3>
    <ul>
      <li>${sw(COLORS.nivel.ultra)}trabalhando — sub-agente <b>ultra</b></li>
      <li>${sw(COLORS.nivel.medium)}trabalhando — sub-agente <b>medium</b></li>
      <li>${sw(COLORS.nivel.low)}trabalhando — sub-agente <b>low</b></li>
      <li>${sw(COLORS.concluido)}relatório concluído · ${sw(COLORS.falhou)}falhou · ${sw(COLORS.expirado)}expirado · ${sw(COLORS.cancelado)}cancelado</li>
      <li>${sw(COLORS.resting)}descansando · ${sw(COLORS.sleeping)}dormindo · ${sw(COLORS.free)}livre</li>
    </ul>
    <h3>Abiyss</h3>
    <ul>
      <li>${sw(COLORS.abiyss)}normal · anéis rápidos = <b>pensando</b> (heartbeat que chamou o modelo)</li>
      <li>${sw(0xffd166)}delegando · ${sw(COLORS.concluido)}recebendo relatório · ${sw(0x9ab8ff)}conversando com o dono</li>
      <li>${sw(COLORS.abiyssSleep)}sono (consolidação) · ${sw(COLORS.offline)}daemon parado</li>
      <li>satélites dourados = eventos pendentes na fila do runtime</li>
    </ul>
    <h3>Teclas</h3>
    <ul>
      <li><b>1–5</b> visões · <b>F</b> seguir próxima entidade · <b>Esc</b> soltar</li>
      <li><b>L</b> rótulos · <b>D</b> debug · <b>P</b> caminhos · <b>?</b> legenda</li>
      <li>Mouse: arrastar = girar · botão direito = mover · roda = zoom</li>
    </ul>`;
}
