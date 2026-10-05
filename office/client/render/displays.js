// Telas "dentro do mundo": o escritório mostra o estado real do runtime nas
// próprias paredes — painel de status na sala do Abiyss, quadro de goals,
// TV da área de descanso (hora e clima de Contagem), relógio de parede,
// monitores e LEDs das mesas, bandeja de entrada do console.

import { COLORS, IMMO_ACTIVITIES } from '../../shared/states.js';
import { FONT, hex } from './materials.js';
import { immoStateColor } from './characters/views.js';

const GOAL_COLOR = {
  proposto: '#9aa3b2', comprometido: '#4da3ff', executando: '#48e5ff', validando: '#b26bff',
  concluido: '#3ddc84', bloqueado: '#ff9f43', abandonado: '#6b7280',
};

function fit(ctx, text, max) {
  if (ctx.measureText(text).width <= max) return text;
  let t = text;
  while (t.length > 1 && ctx.measureText(`${t}…`).width > max) t = t.slice(0, -1);
  return `${t}…`;
}

export class Displays {
  constructor(dyn) {
    this.dyn = dyn;
    this.lastMetaTick = -1;
  }

  /** Telas de texto (1×/s, quando chega metadado novo). */
  updateMeta(meta, sample) {
    if (!meta || meta.tick === this.lastMetaTick) return;
    this.lastMetaTick = meta.tick;
    this.#statusWall(meta, sample);
    this.#whiteboard(meta);
    this.#tv(meta);
  }

  #statusWall(meta, sample) {
    const d = this.dyn.displays.status;
    if (!d) return;
    const { ctx, canvas: c } = d;
    const rt = meta.runtime ?? {};
    ctx.fillStyle = '#0c1020';
    ctx.fillRect(0, 0, c.width, c.height);
    ctx.strokeStyle = 'rgba(72,229,255,0.35)';
    ctx.lineWidth = 3;
    ctx.strokeRect(8, 8, c.width - 16, c.height - 16);
    ctx.fillStyle = '#48e5ff';
    ctx.font = `bold 44px ${FONT}`;
    ctx.fillText('ABIYSS · RUNTIME', 34, 66);
    const daemonOk = rt.daemon === 'rodando';
    ctx.font = `30px ${FONT}`;
    ctx.fillStyle = daemonOk ? '#3ddc84' : '#ff5d6c';
    ctx.fillText(`● daemon ${rt.daemon ?? '?'}`, 34, 116);
    ctx.fillStyle = '#c9d2e6';
    ctx.fillText(`fase: ${rt.fase ?? '?'}${rt.sono ? ' (sono)' : ''}`, 360, 116);
    ctx.fillText(`fonte: ${rt.source === 'demo' ? 'DEMO' : rt.source ?? '?'}`, 700, 116);
    // Sub-agentes por estado.
    const s = rt.subagentes ?? {};
    const items = [['executando', '#48e5ff'], ['pendente', '#ffd166'], ['concluido', '#3ddc84'], ['falhou', '#ff5d6c'], ['expirado', '#ff9f43'], ['cancelado', '#9aa3b2']];
    ctx.font = `bold 28px ${FONT}`;
    ctx.fillStyle = '#8f9ab3';
    ctx.fillText('SUB-AGENTES (10 min)', 34, 176);
    items.forEach(([k, col], i) => {
      const x = 34 + (i % 3) * 320;
      const y = 222 + Math.floor(i / 3) * 46;
      ctx.fillStyle = col;
      ctx.fillRect(x, y - 22, 16, 22);
      ctx.fillStyle = '#e8edf8';
      ctx.font = `28px ${FONT}`;
      ctx.fillText(`${k}: ${s[k] ?? 0}`, x + 28, y);
    });
    ctx.fillStyle = '#8f9ab3';
    ctx.font = `bold 28px ${FONT}`;
    ctx.fillText('GOAL EM FOCO', 34, 340);
    ctx.font = `30px ${FONT}`;
    const g = rt.goalFoco;
    ctx.fillStyle = g ? GOAL_COLOR[g.estado] ?? '#fff' : '#6b7280';
    ctx.fillText(fit(ctx, g ? `#${g.id} ${g.titulo} — ${g.estado}` : 'nenhum', c.width - 80), 34, 382);
    ctx.fillStyle = '#8f9ab3';
    ctx.font = `bold 28px ${FONT}`;
    ctx.fillText('ÚLTIMO HEARTBEAT', 34, 440);
    ctx.font = `26px ${FONT}`;
    ctx.fillStyle = '#c9d2e6';
    const u = rt.ultimoCiclo;
    ctx.fillText(fit(ctx, u ? `#${u.id} ${u.chamouModelo ? '(modelo) ' : ''}${u.motivo}` : '—', c.width - 80), 34, 478);
    ctx.fillStyle = '#ffe08a';
    ctx.fillText(`eventos na fila: ${rt.eventosPendentes ?? 0}`, 34, 518);
    if (sample) {
      const immos = [...sample.entities.values()].filter((e) => e.kind === 'immo');
      immos.forEach((e, i) => {
        const x = 620 + (i % 2) * 190;
        const y = 488 + Math.floor(i / 2) * 32;
        ctx.fillStyle = hex(immoStateColor(e));
        ctx.fillRect(x, y - 18, 14, 14);
        ctx.fillStyle = '#c9d2e6';
        ctx.font = `22px ${FONT}`;
        ctx.fillText(`${e.name}`, x + 20, y - 4);
      });
    }
    d.tex.needsUpdate = true;
  }

  #whiteboard(meta) {
    const d = this.dyn.displays.whiteboard;
    if (!d) return;
    const { ctx, canvas: c } = d;
    ctx.fillStyle = '#fbfbf8';
    ctx.fillRect(0, 0, c.width, c.height);
    ctx.fillStyle = '#2b3040';
    ctx.font = `bold 40px ${FONT}`;
    ctx.fillText('Goals', 26, 52);
    ctx.font = `22px ${FONT}`;
    ctx.fillStyle = '#7a8194';
    const cnt = meta.runtime?.goalsContagem ?? {};
    ctx.fillText(Object.entries(cnt).map(([k, v]) => `${k} ${v}`).join(' · '), 150, 50);
    const goals = meta.runtime?.goals ?? [];
    const foco = meta.runtime?.goalFoco?.id;
    goals.slice(0, 6).forEach((g, i) => {
      const y = 100 + i * 52;
      ctx.fillStyle = GOAL_COLOR[g.estado] ?? '#999';
      ctx.fillRect(26, y - 26, 12, 34);
      ctx.fillStyle = g.id === foco ? '#111827' : '#374151';
      ctx.font = `${g.id === foco ? 'bold ' : ''}30px ${FONT}`;
      ctx.fillText(fit(ctx, `#${g.id} ${g.titulo}`, 500), 50, y);
      ctx.font = `24px ${FONT}`;
      ctx.fillStyle = GOAL_COLOR[g.estado] ?? '#999';
      ctx.fillText(g.estado, 580, y);
    });
    if (!goals.length) {
      ctx.fillStyle = '#9aa3b2';
      ctx.font = `28px ${FONT}`;
      ctx.fillText('nenhum goal ativo', 26, 120);
    }
    d.tex.needsUpdate = true;
  }

  #tv(meta) {
    const d = this.dyn.displays.tv;
    if (!d) return;
    const { ctx, canvas: c } = d;
    const env = meta.env;
    const grad = ctx.createLinearGradient(0, 0, 0, c.height);
    grad.addColorStop(0, env ? hex(env.light.skyTop) : '#223');
    grad.addColorStop(1, env ? hex(env.light.skyHorizon) : '#445');
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, c.width, c.height);
    ctx.fillStyle = 'rgba(0,0,0,0.35)';
    ctx.fillRect(0, 0, c.width, c.height);
    ctx.fillStyle = '#ffffff';
    ctx.font = `bold 92px ${FONT}`;
    ctx.fillText(env?.local?.hhmm ?? '--:--', 30, 118);
    ctx.font = `30px ${FONT}`;
    ctx.fillText(env?.location?.name ?? '', 34, 164);
    if (env?.weather) {
      const w = env.weather;
      ctx.font = `bold 64px ${FONT}`;
      ctx.fillText(`${Math.round(w.temperatureC)}°C`, 34, 252);
      ctx.font = `32px ${FONT}`;
      ctx.fillText(w.label, 250, 236);
      ctx.font = `24px ${FONT}`;
      ctx.fillStyle = w.estimated ? '#ffd166' : '#9ff0c0';
      ctx.fillText(w.estimated ? 'estimado (climatologia)' : 'ao vivo · Open-Meteo', 250, 272);
      ctx.fillStyle = '#e5e7eb';
      ctx.fillText(`☀ ${env.sun.sunrise ?? '--'}  ☾ ${env.sun.sunset ?? '--'}`, 34, 320);
    }
    d.tex.needsUpdate = true;
  }

  /** Coisas que mudam a cada quadro (barato). */
  updateFrame(sample, meta, t) {
    const dyn = this.dyn;
    if (dyn.clock && sample?.clock) {
      const h = sample.clock.hours ?? 0;
      dyn.clock.hour.rotation.z = -((h % 12) / 12) * Math.PI * 2;
      dyn.clock.minute.rotation.z = -((h % 1)) * Math.PI * 2;
    }
    if (!sample) return;
    const byDesk = new Map();
    for (const e of sample.entities.values()) if (e.kind === 'immo') byDesk.set(e.desk, e);
    for (const [deskId, parts] of dyn.desks) {
      const e = byDesk.get(deskId);
      const working = e && e.act === 'working' && e.poi === deskId;
      const seated = e && e.poi === deskId && (e.act === 'seated_idle' || e.act === 'sitting_down');
      const col = e ? immoStateColor(e) : 0x333a48;
      const screen = parts.screen.material;
      if (working) {
        screen.color.setHex(0x0e1424);
        screen.emissive.setHex(COLORS.nivel[e.logical?.nivel] ?? 0x48e5ff);
        screen.emissiveIntensity = 0.55 + Math.sin(t * 7 + deskId.length) * 0.12;
      } else if (seated) {
        screen.emissive.setHex(0x6fa8dc);
        screen.emissiveIntensity = 0.35;
      } else {
        screen.emissive.setHex(0x1b2a4a);
        screen.emissiveIntensity = 0.25;
      }
      parts.led.material.emissive.setHex(col);
      parts.led.material.emissiveIntensity = working ? 1.2 : 0.6;
    }
    const ab = sample.entities.get('abiyss');
    if (dyn.console) {
      const thinking = ab?.mind?.kind === 'thinking';
      const off = ab?.act === 'offline' || ab?.act === 'sleeping';
      dyn.console.holo.opacity = off ? 0.04 : thinking ? 0.42 + Math.sin(t * 9) * 0.1 : 0.24;
      dyn.console.holos.forEach((h, i) => { h.visible = !off || i === 1; });
      dyn.console.strip.emissiveIntensity = off ? 0.15 : 0.8;
      const pend = meta?.runtime?.subagentes?.pendente ?? 0;
      dyn.console.cards.forEach((card, i) => { card.visible = i < Math.min(5, pend); });
    }
    if (dyn.daisRing) dyn.daisRing.emissiveIntensity = ab?.act === 'offline' ? 0.1 : 0.6 + Math.sin(t * 1.5) * 0.3;
    if (dyn.rackLeds) dyn.rackLeds.forEach((m, i) => { m.emissiveIntensity = Math.sin(t * (3 + i * 2.3)) > 0 ? 1 : 0.2; });
  }
}

export function activityLabel(act) {
  return IMMO_ACTIVITIES[act] ?? act;
}
