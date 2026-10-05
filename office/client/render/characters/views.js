// Representação visual das entidades: recebe o estado interpolado vindo do
// servidor e anima os modelos. NADA aqui decide comportamento: posição,
// atividade e pose são do servidor; o cliente só traduz em movimento de
// pivôs, cores e ícones.

import * as THREE from 'three';
import { IMMO_DEFS } from '../../../shared/layout.js';
import { COLORS, ACTIVITY_ICON, IMMO_ACTIVITIES, ABIYSS_ACTIVITIES } from '../../../shared/states.js';
import { createImmoModel } from './immo-model.js';
import { createAbiyssModel } from './abiyss-model.js';
import { Indicator } from '../indicators.js';

const smooth = (t) => t * t * (3 - 2 * t);
const lerp = (a, b, t) => a + (b - a) * t;

/** Parâmetros de pose do IMMo (ver immo-model.js). */
const POSES = {
  stand: { x: 0, y: 0, rx: 0, rz: 0, leg: 0 },
  float: { x: 0, y: 0, rx: 0, rz: 0, leg: 0 },
  sit: { x: 0, y: -0.02, rx: 0, rz: 0, leg: -1.45 },
  lie: { x: 0.55, y: 0.52, rx: 0, rz: Math.PI / 2, leg: 0 },
  curl: { x: 0, y: -0.14, rx: -0.55, rz: 0, leg: -1.2 },
};

const WALKING = new Set(['walking', 'entering', 'wandering', 'moving']);

export function immoStateColor(e) {
  const lg = e.logical;
  if (lg?.estado === 'executando') return COLORS.nivel[lg.nivel] ?? COLORS.free;
  if (lg?.estado === 'concluido') return COLORS.concluido;
  if (lg?.estado === 'falhou') return COLORS.falhou;
  if (lg?.estado === 'expirado') return COLORS.expirado;
  if (lg?.estado === 'cancelado') return COLORS.cancelado;
  if (e.act === 'sleeping') return COLORS.sleeping;
  if (e.act === 'resting') return COLORS.resting;
  return COLORS.free;
}

function immoIcon(e) {
  const lg = e.logical;
  if (e.act === 'reporting' || (lg?.estado && ['concluido', 'falhou', 'expirado'].includes(lg.estado) && e.act !== 'working')) {
    return { key: lg?.estado === 'concluido' ? 'check' : lg?.estado === 'falhou' ? 'cross' : lg?.estado === 'expirado' ? 'hourglass' : 'report', color: immoStateColor(e) };
  }
  if (lg?.estado === 'cancelado') return { key: 'cross', color: COLORS.cancelado };
  if (e.act === 'working') return { key: 'gear', color: immoStateColor(e) };
  if (lg?.estado === 'executando' && WALKING.has(e.act)) return { key: 'walk', color: immoStateColor(e) };
  const key = ACTIVITY_ICON[e.act] ?? null;
  const color = e.act === 'sleeping' ? COLORS.sleeping : e.act === 'resting' ? COLORS.resting : 0x5b6a85;
  return { key, color };
}

function immoSubLabel(e) {
  const lg = e.logical;
  if (lg?.subagenteId) return `#${lg.subagenteId} · ${lg.nivel} · ${lg.estado}`;
  return IMMO_ACTIVITIES[e.act] ?? e.act;
}

class ImmoView {
  constructor(scene, def) {
    this.def = def;
    this.m = createImmoModel(def);
    scene.add(this.m.root);
    this.ind = new Indicator(this.m.root, 1.62);
    this.phase = Math.random() * 10;
    this.look = 0;
    this.lastAct = null;
    this.actTime = 0;
  }

  update(e, dt, t, showLabels, indScale = 1) {
    const m = this.m;
    m.root.visible = !!e.present;
    this.ind.group.scale.setScalar(indScale);
    if (!e.present) return;
    m.root.position.set(e.x, 0, e.z);
    m.root.rotation.y = e.h;
    if (e.act !== this.lastAct) {
      this.lastAct = e.act;
      this.actTime = 0;
    }
    this.actTime += dt;

    // Pose (com transição sentar/levantar).
    const from = POSES[e.poseFrom] ?? POSES.stand;
    const to = POSES[e.pose] ?? POSES.stand;
    const k = smooth(Math.min(1, Math.max(0, e.poseT ?? 1)));
    const p = {};
    for (const key of Object.keys(POSES.stand)) p[key] = lerp(from[key], to[key], k);
    m.body.position.set(p.x, p.y, 0);
    m.body.rotation.set(p.rx, 0, p.rz);
    let legL = p.leg;
    let legR = p.leg;
    let armL = 0;
    let armR = 0;
    let headPitch = 0;
    let headYaw = 0;
    let bob = 0;
    m.mug.visible = false;
    m.paper.visible = false;

    const act = e.act;
    if (WALKING.has(act) && e.spd > 0.02) {
      this.phase += dt * (4 + e.spd * 6);
      const s = Math.sin(this.phase);
      legL = 0.55 * s * Math.min(1, e.spd / 0.6);
      legR = -legL;
      armL = -0.45 * s * Math.min(1, e.spd / 0.6);
      armR = -armL;
      bob = Math.abs(Math.cos(this.phase)) * 0.035;
    } else if (act === 'working') {
      armL = -1.15 + Math.sin(t * 17 + 1) * 0.07;
      armR = -1.15 + Math.sin(t * 19) * 0.07;
      headPitch = -0.18 + Math.sin(t * 1.3) * 0.04;
      headYaw = Math.sin(t * 0.6) * 0.12;
    } else if (act === 'seated_idle') {
      armL = -0.55;
      armR = -0.55;
      headYaw = Math.sin(t * 0.5 + this.phase) * 0.5;
      headPitch = 0.05;
    } else if (act === 'resting') {
      armL = -0.3;
      armR = -0.3;
      headPitch = 0.22;
      bob = Math.sin(t * 1.4) * 0.008;
    } else if (act === 'sleeping') {
      armL = e.pose === 'lie' ? -0.2 : -0.4;
      armR = armL;
      headPitch = e.pose === 'lie' ? 0 : -0.35;
      headYaw = e.pose === 'lie' ? 0.3 : 0;
      bob = Math.sin(t * 0.9) * 0.012;
    } else if (act === 'coffee' || act === 'water') {
      m.mug.visible = true;
      const sip = (Math.sin(t * 0.9 + this.phase) + 1) / 2;
      armR = -0.6 - sip * 1.6;
      armL = -0.2;
      headPitch = sip > 0.8 ? 0.15 : 0;
    } else if (act === 'chatting') {
      armR = -0.4 + Math.max(0, Math.sin(t * 2.2 + this.phase)) * -0.9;
      armL = -0.15;
      headPitch = Math.sin(t * 3.1 + this.phase) * 0.08;
      headYaw = Math.sin(t * 0.7 + this.phase) * 0.15;
    } else if (act === 'reporting') {
      m.paper.visible = true;
      armR = -1.45;
      armL = -0.2;
      headPitch = 0.12;
    } else if (act === 'window' || act === 'reading_board' || act === 'observing') {
      headPitch = act === 'reading_board' ? 0.18 : 0.08;
      headYaw = Math.sin(t * 0.35 + this.phase) * 0.3;
      armL = 0.12;
      armR = 0.12;
    } else if (act === 'sitting_down' || act === 'standing_up') {
      armL = -0.4;
      armR = -0.4;
    } else {
      // Ocioso / esperando: balanço leve e olhar em volta.
      headYaw = Math.sin(t * 0.45 + this.phase) * 0.6;
      bob = Math.sin(t * 1.6 + this.phase) * 0.006;
      armL = Math.sin(t * 1.1 + this.phase) * 0.05;
      armR = -armL;
    }
    m.body.position.y += bob;
    m.legL.rotation.x = legL;
    m.legR.rotation.x = legR;
    m.armL.pivot.rotation.x = armL;
    m.armR.pivot.rotation.x = armR;
    m.headPivot.rotation.set(headPitch, headYaw, 0);

    // Cores de estado: lâmpada da antena e olhos.
    const sc = immoStateColor(e);
    m.bulbMat.emissive.setHex(sc);
    m.bulbMat.emissiveIntensity = act === 'working' ? 1.4 + Math.sin(t * 6) * 0.4 : 1;
    const eyeColor = act === 'working' ? (COLORS.nivel[e.logical?.nivel] ?? 0x48e5ff) : 0x48e5ff;
    m.eyeMat.emissive.setHex(eyeColor);
    const closed = act === 'sleeping';
    const blink = !closed && Math.sin(t * 0.8 + this.phase * 3) > 0.985;
    const eyeScale = closed || blink ? 0.15 : 1;
    m.eyeL.scale.y = eyeScale;
    m.eyeR.scale.y = eyeScale;
    m.eyeMat.emissiveIntensity = closed ? 0.25 : 1;

    // Indicadores (acima da cabeça; deitado, um pouco mais baixo).
    this.ind.group.position.y = e.pose === 'lie' ? 1.05 : e.pose === 'curl' || e.pose === 'sit' ? 1.5 : 1.62;
    this.ind.group.position.x = 0;
    const icon = immoIcon(e);
    this.ind.setIcon(icon.key, icon.color);
    this.ind.icon.position.y = act === 'sleeping' ? Math.sin(t * 1.2) * 0.05 : 0;
    this.ind.setLabel(this.def.name, showLabels === 'full' ? immoSubLabel(e) : null, this.def.color);
    this.ind.setLabelVisible(!!showLabels);
  }
}

const ABIYSS_STYLE = {
  default: { color: 0x48e5ff, ring: 0.6, light: 2.2 },
  thinking: { color: 0x7ff3ff, ring: 3.2, light: 3.2 },
  delegating: { color: 0xffd166, ring: 1.8, light: 3.0 },
  receiving: { color: 0x3ddc84, ring: 1.4, light: 2.8 },
  conversing: { color: 0x9ab8ff, ring: 1.0, light: 2.6 },
  sleeping: { color: 0x8f6bff, ring: 0.12, light: 0.9 },
  resting: { color: 0x6fb7d6, ring: 0.3, light: 1.6 },
  offline: { color: 0x5b6070, ring: 0, light: 0 },
};

class AbiyssView {
  constructor(scene) {
    this.m = createAbiyssModel();
    scene.add(this.m.root);
    this.ind = new Indicator(this.m.root, 2.55);
    this.color = new THREE.Color(0x48e5ff);
    this.target = new THREE.Color();
    this.pulseT = 1;
    this.lastMind = null;
  }

  update(e, dt, t, showLabels, runtime, indScale = 1) {
    const m = this.m;
    this.ind.group.scale.setScalar(indScale);
    m.root.position.set(e.x, 0, e.z);
    m.root.rotation.y = e.h;
    const mind = e.mind?.kind ?? null;
    const styleKey = e.act === 'offline' ? 'offline' : e.act === 'sleeping' ? 'sleeping' : mind ?? (e.act === 'resting' ? 'resting' : 'default');
    const style = ABIYSS_STYLE[styleKey] ?? ABIYSS_STYLE.default;
    this.target.setHex(style.color);
    this.color.lerp(this.target, Math.min(1, dt * 4));
    m.edgeMat.color.copy(this.color);
    m.eyeMat.color.copy(this.color);
    m.iris.material.color.copy(this.color);
    m.ringMat.color.copy(this.color);
    m.shardMat.color.copy(this.color);
    m.haloMat.color.copy(this.color);
    m.light.color.copy(this.color);
    m.coreMat.emissive.copy(this.color).multiplyScalar(styleKey === 'offline' ? 0 : 0.18);

    const offline = styleKey === 'offline';
    const sleeping = styleKey === 'sleeping';
    const baseY = offline ? 0.92 : sleeping ? 1.12 : 1.45;
    const bobAmp = offline ? 0 : sleeping ? 0.03 : 0.06;
    m.floating.position.y = lerp(m.floating.position.y, baseY + Math.sin(t * (sleeping ? 0.7 : 1.4)) * bobAmp, Math.min(1, dt * 3));
    m.core.rotation.y += dt * (offline ? 0 : sleeping ? 0.15 : mind === 'thinking' ? 2.4 : 0.45);
    m.ring1.rotation.z += dt * style.ring;
    m.ring2.rotation.z -= dt * style.ring * 0.7;
    m.crown.rotation.y += dt * (offline ? 0 : 0.8);
    const eyePulse = mind === 'thinking' ? 1 + Math.sin(t * 10) * 0.25 : sleeping ? 0.6 : 1;
    m.eye.scale.setScalar(offline ? 0.6 : eyePulse);
    m.eyeMat.color.multiplyScalar(offline ? 0.3 : 1);
    m.light.intensity = lerp(m.light.intensity, style.light, Math.min(1, dt * 3));
    m.haloMat.opacity = offline ? 0.05 : 0.22 + Math.sin(t * (mind === 'thinking' ? 6 : 1.5)) * 0.1;

    // Satélites = eventos pendentes na fila do runtime.
    const n = Math.min(6, runtime?.eventosPendentes ?? 0);
    m.satellites.forEach((s, i) => {
      s.visible = i < n && !offline;
      if (!s.visible) return;
      const a = t * 1.1 + (i / Math.max(1, n)) * Math.PI * 2;
      s.position.set(Math.cos(a) * 0.62, Math.sin(a * 1.7) * 0.18, Math.sin(a) * 0.62);
      s.rotation.y += dt * 3;
    });

    // Pulso no chão a cada heartbeat que chamou o modelo.
    if (mind === 'thinking' && this.lastMind !== 'thinking') this.pulseT = 0;
    this.lastMind = mind;
    if (this.pulseT < 1) {
      this.pulseT += dt / 1.6;
      m.pulse.visible = true;
      const s = 1 + this.pulseT * 2.2;
      m.pulse.scale.set(s, s, 1);
      m.pulseMat.opacity = 0.5 * (1 - this.pulseT);
      m.pulseMat.color.copy(this.color);
    } else {
      m.pulse.visible = false;
    }

    const iconKey = offline ? 'power' : sleeping ? 'moon' : mind === 'thinking' ? 'think' : mind === 'delegating' ? 'send'
      : mind === 'receiving' ? 'inbox' : mind === 'conversing' ? 'speak' : (e.act === 'supervising' || e.act === 'observing') ? 'eye' : null;
    this.ind.setIcon(iconKey, style.color);
    this.ind.group.position.y = m.floating.position.y + 1.05;
    const sub = e.mind?.label ? shorten(e.mind.label) : (ABIYSS_ACTIVITIES[e.act] ?? e.act);
    this.ind.setLabel('Abiyss', showLabels === 'full' ? sub : null, 0x48e5ff);
    this.ind.setLabelVisible(!!showLabels);
  }
}

function shorten(s) {
  return s.length > 34 ? `${s.slice(0, 33)}…` : s;
}

export class EntityViews {
  constructor(scene) {
    this.immos = new Map(IMMO_DEFS.map((d) => [d.id, new ImmoView(scene, d)]));
    this.abiyss = new AbiyssView(scene);
  }

  /** Posição atual (mundo) de uma entidade, para câmera e efeitos. */
  positionOf(id) {
    if (id === 'abiyss') return this.abiyss.m.root.position;
    return this.immos.get(id)?.m.root.position ?? null;
  }

  update(sample, dt, t, showLabels, runtime, camera) {
    if (!sample) return;
    for (const [id, e] of sample.entities) {
      // Ícones e rótulos crescem com a distância da câmera (legíveis na visão geral).
      const pos = this.positionOf(id);
      const dist = camera && pos ? camera.position.distanceTo(pos) : 12;
      const s = Math.min(2.6, Math.max(1, dist / 11));
      if (id === 'abiyss') this.abiyss.update(e, dt, t, showLabels, runtime, s);
      else this.immos.get(id)?.update(e, dt, t, showLabels, s);
    }
  }
}
