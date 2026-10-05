// Indicadores sobre as entidades: um ícone pequeno (o que está fazendo) e um
// rótulo discreto (nome + sub-agente). Tudo desenhado em canvas: nenhuma
// fonte de emoji ou imagem externa é necessária.

import * as THREE from 'three';
import { FONT, hex } from './materials.js';

const iconCache = new Map();

function roundRect(ctx, x, y, w, h, r) {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

/** Desenha o símbolo `key` em branco, centrado em (64, 60). */
function drawGlyph(ctx, key) {
  ctx.strokeStyle = '#ffffff';
  ctx.fillStyle = '#ffffff';
  ctx.lineWidth = 8;
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';
  const c = { x: 64, y: 58 };
  switch (key) {
    case 'gear': {
      ctx.beginPath();
      for (let i = 0; i < 8; i++) {
        const a = (i / 8) * Math.PI * 2;
        ctx.moveTo(c.x + Math.cos(a) * 18, c.y + Math.sin(a) * 18);
        ctx.lineTo(c.x + Math.cos(a) * 30, c.y + Math.sin(a) * 30);
      }
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(c.x, c.y, 18, 0, Math.PI * 2);
      ctx.stroke();
      break;
    }
    case 'dots':
    case 'chat':
      if (key === 'chat') {
        roundRect(ctx, 30, 32, 68, 44, 14);
        ctx.stroke();
        ctx.beginPath();
        ctx.moveTo(44, 76); ctx.lineTo(38, 92); ctx.lineTo(58, 76);
        ctx.stroke();
      }
      for (const dx of [-14, 0, 14]) {
        ctx.beginPath();
        ctx.arc(c.x + dx, c.y - (key === 'chat' ? 4 : 0), 5, 0, Math.PI * 2);
        ctx.fill();
      }
      break;
    case 'rest':
      ctx.beginPath();
      ctx.moveTo(30, 74); ctx.lineTo(98, 74); ctx.moveTo(34, 74); ctx.lineTo(34, 50);
      ctx.moveTo(94, 74); ctx.lineTo(94, 58); ctx.moveTo(34, 58); ctx.lineTo(94, 58);
      ctx.moveTo(40, 86); ctx.lineTo(40, 74); ctx.moveTo(88, 86); ctx.lineTo(88, 74);
      ctx.stroke();
      break;
    case 'zzz':
      ctx.font = `bold 40px ${FONT}`;
      ctx.textAlign = 'center';
      ctx.fillText('Z', 50, 76);
      ctx.font = `bold 28px ${FONT}`;
      ctx.fillText('z', 76, 56);
      ctx.font = `bold 20px ${FONT}`;
      ctx.fillText('z', 92, 40);
      break;
    case 'cup':
      ctx.beginPath();
      ctx.moveTo(38, 44); ctx.lineTo(42, 84); ctx.lineTo(78, 84); ctx.lineTo(82, 44); ctx.closePath();
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(86, 62, 10, -Math.PI / 2, Math.PI / 2);
      ctx.stroke();
      ctx.lineWidth = 5;
      ctx.beginPath();
      ctx.moveTo(52, 36); ctx.quadraticCurveTo(46, 28, 54, 20);
      ctx.moveTo(68, 36); ctx.quadraticCurveTo(62, 28, 70, 20);
      ctx.stroke();
      break;
    case 'drop':
      ctx.beginPath();
      ctx.moveTo(64, 24);
      ctx.quadraticCurveTo(92, 62, 80, 80);
      ctx.arc(64, 70, 22, 0.4, Math.PI - 0.4);
      ctx.quadraticCurveTo(36, 62, 64, 24);
      ctx.fill();
      break;
    case 'eye':
      ctx.beginPath();
      ctx.moveTo(26, 58); ctx.quadraticCurveTo(64, 22, 102, 58); ctx.quadraticCurveTo(64, 94, 26, 58);
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(64, 58, 10, 0, Math.PI * 2);
      ctx.fill();
      break;
    case 'board':
      ctx.strokeRect(30, 34, 68, 46);
      ctx.lineWidth = 5;
      ctx.beginPath();
      ctx.moveTo(40, 48); ctx.lineTo(86, 48); ctx.moveTo(40, 62); ctx.lineTo(74, 62);
      ctx.moveTo(48, 80); ctx.lineTo(40, 94); ctx.moveTo(80, 80); ctx.lineTo(88, 94);
      ctx.stroke();
      break;
    case 'report':
      ctx.strokeRect(40, 26, 48, 64);
      ctx.lineWidth = 5;
      ctx.beginPath();
      ctx.moveTo(50, 44); ctx.lineTo(78, 44); ctx.moveTo(50, 58); ctx.lineTo(78, 58); ctx.moveTo(50, 72); ctx.lineTo(68, 72);
      ctx.stroke();
      break;
    case 'check':
      ctx.lineWidth = 11;
      ctx.beginPath();
      ctx.moveTo(36, 60); ctx.lineTo(56, 80); ctx.lineTo(94, 38);
      ctx.stroke();
      break;
    case 'cross':
      ctx.lineWidth = 11;
      ctx.beginPath();
      ctx.moveTo(42, 36); ctx.lineTo(86, 80); ctx.moveTo(86, 36); ctx.lineTo(42, 80);
      ctx.stroke();
      break;
    case 'hourglass':
      ctx.beginPath();
      ctx.moveTo(42, 30); ctx.lineTo(86, 30); ctx.lineTo(64, 58); ctx.lineTo(86, 86); ctx.lineTo(42, 86); ctx.lineTo(64, 58); ctx.closePath();
      ctx.stroke();
      break;
    case 'walk':
      ctx.beginPath();
      ctx.arc(66, 32, 8, 0, Math.PI * 2);
      ctx.fill();
      ctx.beginPath();
      ctx.moveTo(64, 44); ctx.lineTo(58, 66); ctx.lineTo(46, 88); ctx.moveTo(58, 66); ctx.lineTo(74, 88);
      ctx.moveTo(62, 50); ctx.lineTo(80, 60); ctx.moveTo(62, 50); ctx.lineTo(46, 58);
      ctx.stroke();
      break;
    case 'wait':
      ctx.beginPath();
      ctx.arc(64, 58, 28, 0, Math.PI * 2);
      ctx.moveTo(64, 58); ctx.lineTo(64, 40); ctx.moveTo(64, 58); ctx.lineTo(78, 64);
      ctx.stroke();
      break;
    case 'think':
      for (const [x, y, r] of [[64, 50, 22], [40, 82, 7], [30, 96, 4]]) {
        ctx.beginPath();
        ctx.arc(x, y, r, 0, Math.PI * 2);
        ctx.stroke();
      }
      ctx.font = `bold 26px ${FONT}`;
      ctx.textAlign = 'center';
      ctx.fillText('…', 64, 58);
      break;
    case 'send':
      ctx.beginPath();
      ctx.moveTo(30, 60); ctx.lineTo(96, 36); ctx.lineTo(74, 92); ctx.lineTo(62, 68); ctx.closePath();
      ctx.stroke();
      break;
    case 'inbox':
      ctx.beginPath();
      ctx.moveTo(30, 64); ctx.lineTo(44, 36); ctx.lineTo(84, 36); ctx.lineTo(98, 64); ctx.lineTo(98, 88); ctx.lineTo(30, 88); ctx.closePath();
      ctx.moveTo(30, 64); ctx.lineTo(50, 64); ctx.lineTo(56, 74); ctx.lineTo(72, 74); ctx.lineTo(78, 64); ctx.lineTo(98, 64);
      ctx.stroke();
      break;
    case 'speak':
      roundRect(ctx, 28, 30, 72, 46, 16);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(84, 76); ctx.lineTo(92, 94); ctx.lineTo(70, 76);
      ctx.stroke();
      break;
    case 'power':
      ctx.beginPath();
      ctx.arc(64, 62, 26, -Math.PI / 2 + 0.6, Math.PI * 1.5 - 0.6);
      ctx.moveTo(64, 28); ctx.lineTo(64, 58);
      ctx.stroke();
      break;
    case 'moon':
      ctx.beginPath();
      ctx.arc(64, 58, 28, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalCompositeOperation = 'destination-out';
      ctx.beginPath();
      ctx.arc(78, 48, 24, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalCompositeOperation = 'source-over';
      break;
    default:
      ctx.beginPath();
      ctx.arc(64, 58, 10, 0, Math.PI * 2);
      ctx.fill();
  }
}

export function iconTexture(key, color) {
  const k = `${key}:${color}`;
  let tex = iconCache.get(k);
  if (tex) return tex;
  const canvas = document.createElement('canvas');
  canvas.width = 128;
  canvas.height = 128;
  const ctx = canvas.getContext('2d');
  roundRect(ctx, 10, 6, 108, 104, 30);
  ctx.fillStyle = hex(color);
  ctx.globalAlpha = 0.92;
  ctx.fill();
  ctx.globalAlpha = 1;
  ctx.lineWidth = 4;
  ctx.strokeStyle = 'rgba(255,255,255,0.65)';
  ctx.stroke();
  ctx.beginPath();
  ctx.moveTo(54, 110); ctx.lineTo(64, 124); ctx.lineTo(74, 110);
  ctx.fillStyle = hex(color);
  ctx.fill();
  drawGlyph(ctx, key);
  tex = new THREE.CanvasTexture(canvas);
  tex.colorSpace = THREE.SRGBColorSpace;
  iconCache.set(k, tex);
  return tex;
}

function labelTexture(text, sub, color) {
  const canvas = document.createElement('canvas');
  canvas.width = 512;
  canvas.height = 112;
  const ctx = canvas.getContext('2d');
  ctx.font = `bold 40px ${FONT}`;
  const w1 = ctx.measureText(text).width;
  ctx.font = `30px ${FONT}`;
  const w2 = sub ? ctx.measureText(sub).width : 0;
  const w = Math.min(500, Math.max(w1, w2) + 40);
  roundRect(ctx, (512 - w) / 2, 4, w, sub ? 104 : 58, 18);
  ctx.fillStyle = 'rgba(14,18,32,0.78)';
  ctx.fill();
  ctx.fillStyle = hex(color);
  ctx.fillRect((512 - w) / 2 + 14, 22, 8, 22);
  ctx.textAlign = 'center';
  ctx.fillStyle = '#ffffff';
  ctx.font = `bold 40px ${FONT}`;
  ctx.fillText(text, 256 + 8, 47);
  if (sub) {
    ctx.fillStyle = '#c3cbe0';
    ctx.font = `30px ${FONT}`;
    ctx.fillText(sub.length > 30 ? `${sub.slice(0, 29)}…` : sub, 256, 92);
  }
  const tex = new THREE.CanvasTexture(canvas);
  tex.colorSpace = THREE.SRGBColorSpace;
  return tex;
}

/** Ícone + rótulo acima de uma entidade. */
export class Indicator {
  constructor(parent, height) {
    this.group = new THREE.Group();
    this.group.position.y = height;
    parent.add(this.group);
    this.icon = new THREE.Sprite(new THREE.SpriteMaterial({ transparent: true, depthWrite: false, depthTest: false }));
    this.icon.scale.set(0.42, 0.42, 1);
    this.icon.renderOrder = 10;
    this.group.add(this.icon);
    this.label = new THREE.Sprite(new THREE.SpriteMaterial({ transparent: true, depthWrite: false, depthTest: false }));
    this.label.scale.set(1.5, 0.33, 1);
    this.label.position.y = 0.42;
    this.label.renderOrder = 11;
    this.group.add(this.label);
    this.iconKey = null;
    this.labelKey = null;
  }

  setIcon(key, color) {
    const k = key ? `${key}:${color}` : null;
    if (k === this.iconKey) return;
    this.iconKey = k;
    this.icon.visible = !!key;
    if (key) {
      this.icon.material.map = iconTexture(key, color);
      this.icon.material.needsUpdate = true;
    }
  }

  setLabel(text, sub, color) {
    const k = `${text}|${sub}|${color}`;
    if (k === this.labelKey) return;
    this.labelKey = k;
    this.label.material.map?.dispose();
    this.label.material.map = labelTexture(text, sub, color);
    this.label.material.needsUpdate = true;
    this.label.scale.set(1.5, 0.328, 1);
    this.label.position.y = sub ? 0.42 : 0.3;
  }

  setLabelVisible(v) {
    this.label.visible = v;
  }
}
