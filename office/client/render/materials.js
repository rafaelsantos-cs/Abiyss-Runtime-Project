// Paleta e materiais compartilhados (poucos materiais = poucas trocas de
// estado na GPU). Tudo procedural: nenhum arquivo de imagem externo.

import * as THREE from 'three';

export const PALETTE = {
  wall: 0xefe9df,
  wallOut: 0xe4ddd1,
  trim: 0xb9b0a3,
  baseboard: 0x7d6e60,
  frame: 0x3b4150,
  glass: 0x9fd8ff,
  wood: 0xc9a47a,
  woodDark: 0x8a6448,
  deskTop: 0xe7dccb,
  metal: 0x9aa3b0,
  metalDark: 0x4a505e,
  black: 0x1e2230,
  screenOff: 0x111522,
  white: 0xf5f5f2,
  sofa: 0x3f7f86,
  sofaB: 0xd49a3a,
  cushion: 0x5e9aa0,
  plantPot: 0xc96f4a,
  leaf: 0x4c9a5b,
  leafDark: 0x3a7d48,
  grass: 0x7aa35b,
  grassDark: 0x6a9450,
  path: 0xc8c0b2,
  asphalt: 0x4a4d55,
  trunk: 0x7a5a3c,
  tree: 0x4f8f4a,
  treeDark: 0x3f7a3c,
  hill: 0x6f8f5c,
  hillFar: 0x8aa5a0,
  abiyssDark: 0x1c2033,
  accent: 0x48e5ff,
  violet: 0xb26bff,
  kitchen: 0xdfe6ea,
  fridge: 0xe9eef2,
  rack: 0x252a3a,
};

const cache = new Map();

/** Material Lambert com sombreamento chapado (estética low-poly). */
export function mat(color, opts = {}) {
  const key = JSON.stringify([color, opts]);
  let m = cache.get(key);
  if (!m) {
    m = new THREE.MeshLambertMaterial({ color, flatShading: opts.flat ?? true, ...stripCustom(opts) });
    cache.set(key, m);
  }
  return m;
}

function stripCustom(opts) {
  const { flat, ...rest } = opts;
  return rest;
}

/** Material único (não compartilhado) — para o que muda em tempo real. */
export function uniqueMat(color, opts = {}) {
  return new THREE.MeshLambertMaterial({ color, flatShading: opts.flat ?? true, ...stripCustom(opts) });
}

export function glassMat(opacity = 0.18, color = PALETTE.glass) {
  const key = `glass:${opacity}:${color}`;
  let m = cache.get(key);
  if (!m) {
    m = new THREE.MeshLambertMaterial({ color, transparent: true, opacity, depthWrite: false, side: THREE.DoubleSide });
    cache.set(key, m);
  }
  return m;
}

/** Textura desenhada em canvas. */
export function canvasTexture(w, h, draw, { repeat = null } = {}) {
  const canvas = document.createElement('canvas');
  canvas.width = w;
  canvas.height = h;
  const ctx = canvas.getContext('2d');
  draw(ctx, w, h);
  const tex = new THREE.CanvasTexture(canvas);
  tex.colorSpace = THREE.SRGBColorSpace;
  tex.anisotropy = 4;
  if (repeat) {
    tex.wrapS = THREE.RepeatWrapping;
    tex.wrapT = THREE.RepeatWrapping;
    tex.repeat.set(repeat[0], repeat[1]);
  }
  return { canvas, ctx, tex };
}

export function hex(c) {
  return `#${c.toString(16).padStart(6, '0')}`;
}

export const FONT = '"DejaVu Sans", "Liberation Sans", sans-serif';
