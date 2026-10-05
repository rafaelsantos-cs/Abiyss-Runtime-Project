// Móveis procedurais (só primitivas). Cada construtor recebe um grupo já
// posicionado/rotacionado no lugar do móvel (frente = +z local) e um
// "kit" de formas. Peças que mudam em tempo real (telas, luzes, ponteiros)
// são marcadas como dinâmicas e registradas em `dyn`.

import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { PALETTE as P, mat, uniqueMat, canvasTexture, FONT, glassMat } from './materials.js';
import { IMMO_DEFS } from '../../shared/layout.js';

const DESK_H = 0.62;
const SEAT_H = 0.36;

/** Kit de formas que adiciona filhos a `g`. */
export function kit(g) {
  const add = (geo, material, x, y, z, o = {}) => {
    const m = new THREE.Mesh(geo, typeof material === 'number' ? mat(material) : material);
    m.position.set(x, y, z);
    if (o.rx) m.rotation.x = o.rx;
    if (o.ry) m.rotation.y = o.ry;
    if (o.rz) m.rotation.z = o.rz;
    if (o.scale) m.scale.set(...o.scale);
    m.castShadow = o.cast ?? true;
    m.receiveShadow = o.receive ?? true;
    m.userData.static = !o.dynamic;
    (o.parent ?? g).add(m);
    return m;
  };
  return {
    box: (w, h, d, material, x, y, z, o) => add(new THREE.BoxGeometry(w, h, d), material, x, y, z, o),
    cyl: (rt, rb, h, seg, material, x, y, z, o) => add(new THREE.CylinderGeometry(rt, rb, h, seg), material, x, y, z, o),
    ico: (r, detail, material, x, y, z, o) => add(new THREE.IcosahedronGeometry(r, detail), material, x, y, z, o),
    cone: (r, h, seg, material, x, y, z, o) => add(new THREE.ConeGeometry(r, h, seg), material, x, y, z, o),
    torus: (r, t, rs, ts, material, x, y, z, o) => add(new THREE.TorusGeometry(r, t, rs, ts), material, x, y, z, o),
    plane: (w, h, material, x, y, z, o) => add(new THREE.PlaneGeometry(w, h), material, x, y, z, { cast: false, ...o }),
    add,
  };
}

/** Gerador determinístico simples (decoração estável entre recarregamentos). */
function seeded(seed) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

const ownerColor = (deskId) => IMMO_DEFS.find((d) => d.desk === deskId)?.color ?? 0x888888;

export const FURNITURE_BUILDERS = {
  desk(g, f, k, dyn) {
    const legMat = mat(P.white);
    k.box(f.w, 0.04, f.d, P.deskTop, 0, DESK_H - 0.02, 0);
    k.box(0.04, DESK_H - 0.04, f.d - 0.08, legMat, -f.w / 2 + 0.04, (DESK_H - 0.04) / 2, 0);
    k.box(0.04, DESK_H - 0.04, f.d - 0.08, legMat, f.w / 2 - 0.04, (DESK_H - 0.04) / 2, 0);
    k.box(f.w - 0.1, 0.22, 0.02, legMat, 0, DESK_H - 0.16, -f.d / 2 + 0.06);
    // Monitor (atrás, voltado para quem senta em +z).
    k.cyl(0.02, 0.02, 0.14, 6, P.metalDark, 0, DESK_H + 0.07, -0.24);
    k.box(0.22, 0.015, 0.12, P.metalDark, 0, DESK_H + 0.008, -0.24);
    k.box(0.66, 0.4, 0.035, P.black, 0, DESK_H + 0.33, -0.25);
    const screen = k.plane(0.6, 0.34, uniqueMat(P.screenOff, { emissive: 0x000000 }), 0, DESK_H + 0.33, -0.231, { dynamic: true, receive: false });
    k.box(0.44, 0.016, 0.14, P.metalDark, 0, DESK_H + 0.008, 0.1);
    k.box(0.07, 0.016, 0.1, P.metalDark, 0.32, DESK_H + 0.008, 0.12);
    // Caneca, papéis, livros ou planta.
    k.cyl(0.035, 0.03, 0.08, 8, ownerColor(f.id), -0.6, DESK_H + 0.04, 0.12);
    k.box(0.22, 0.01, 0.3, P.white, 0.52, DESK_H + 0.006, 0.05, { ry: 0.2 });
    if (f.desk % 2 === 1) {
      k.cyl(0.05, 0.04, 0.09, 8, P.plantPot, 0.62, DESK_H + 0.045, -0.26);
      k.ico(0.08, 0, P.leaf, 0.62, DESK_H + 0.15, -0.26);
    } else {
      ['#c0504d', '#4f81bd', '#9bbb59'].forEach((c, i) => k.box(0.16, 0.035, 0.22, new THREE.Color(c).getHex(), 0.6, DESK_H + 0.018 + i * 0.036, -0.24, { ry: i * 0.15 }));
    }
    // Luminária de mesa.
    k.cyl(0.06, 0.07, 0.02, 8, P.metalDark, -0.62, DESK_H + 0.01, -0.28);
    k.box(0.02, 0.32, 0.02, P.metalDark, -0.62, DESK_H + 0.17, -0.28, { rz: 0.15 });
    const lamp = k.cone(0.07, 0.1, 8, uniqueMat(0xf6f0dc, { emissive: 0x000000 }), -0.58, DESK_H + 0.33, -0.22, { dynamic: true, rx: 0.5 });
    // LED de estado da mesa (cor do estado do IMMo).
    const led = k.box(0.05, 0.05, 0.05, uniqueMat(0x333a48, { emissive: 0x000000 }), 0.72, DESK_H + 0.025, -0.33, { dynamic: true });
    dyn.desks.set(`desk-${f.desk}`, { screen, lamp, led });
    dyn.lamps.push(lamp);
  },

  chair(g, f, k) {
    const c = ownerColor(f.seatOf);
    k.cyl(0.24, 0.24, 0.03, 5, P.metalDark, 0, 0.03, 0);
    k.cyl(0.025, 0.025, SEAT_H - 0.06, 6, P.metal, 0, (SEAT_H - 0.06) / 2 + 0.03, 0);
    k.box(0.44, 0.07, 0.42, c, 0, SEAT_H - 0.03, 0);
    k.box(0.42, 0.42, 0.06, P.black, 0, SEAT_H + 0.24, -0.21, { rx: -0.08 });
    k.box(0.44, 0.06, 0.07, c, 0, SEAT_H + 0.44, -0.23);
  },

  deskDivider(g, f, k) {
    k.box(0.035, 0.34, f.d, 0x8fa6b8, 0, DESK_H + 0.17, 0);
  },

  filingCabinet(g, f, k) {
    k.box(f.w, 1.05, f.d, 0x7c8796, 0, 0.525, 0);
    for (let i = 0; i < 3; i++) {
      k.box(f.w - 0.08, 0.3, 0.012, 0x8e99a8, 0, 0.2 + i * 0.33, f.d / 2 + 0.006);
      k.box(0.18, 0.025, 0.02, P.metalDark, 0, 0.3 + i * 0.33, f.d / 2 + 0.02);
    }
  },

  waterCooler(g, f, k) {
    k.box(0.34, 0.9, 0.34, P.white, 0, 0.45, 0);
    k.cyl(0.13, 0.13, 0.38, 10, new THREE.MeshLambertMaterial({ color: 0x7cc4ff, transparent: true, opacity: 0.6 }), 0, 1.1, 0);
    k.box(0.06, 0.04, 0.04, 0x3a7bd5, -0.06, 0.62, 0.18);
    k.box(0.06, 0.04, 0.04, 0xd5503a, 0.06, 0.62, 0.18);
  },

  bookshelf(g, f, k) {
    const h = 1.9;
    const wood = mat(P.woodDark);
    k.box(0.04, h, f.d, wood, -f.w / 2 + 0.02, h / 2, 0);
    k.box(0.04, h, f.d, wood, f.w / 2 - 0.02, h / 2, 0);
    k.box(f.w, 0.04, f.d, wood, 0, h - 0.02, 0);
    k.box(f.w, 0.02, f.d, wood, 0, 0.02, 0);
    k.box(f.w, h, 0.02, mat(0xa07a58), 0, h / 2, -f.d / 2 + 0.01);
    const rnd = seeded(f.x * 1000 + f.z);
    const colors = [0xc0504d, 0x4f81bd, 0x9bbb59, 0x8064a2, 0xf79646, 0x2c4d75, 0xe0d4b4];
    for (let s = 0; s < 4; s++) {
      const y = 0.05 + s * 0.46;
      k.box(f.w - 0.08, 0.025, f.d - 0.04, wood, 0, y, 0);
      let x = -f.w / 2 + 0.08;
      while (x < f.w / 2 - 0.12) {
        const bw = 0.04 + rnd() * 0.05;
        const bh = 0.24 + rnd() * 0.14;
        if (rnd() > 0.15) k.box(bw, bh, f.d * 0.7, colors[Math.floor(rnd() * colors.length)], x + bw / 2, y + 0.012 + bh / 2, 0.02, { cast: false });
        x += bw + 0.008;
      }
    }
  },

  printer(g, f, k) {
    k.box(f.w, 0.6, f.d, 0x8d96a3, 0, 0.3, 0);
    k.box(0.56, 0.28, 0.42, 0xd9dde3, 0, 0.74, 0);
    k.box(0.4, 0.02, 0.2, P.white, 0, 0.89, 0.12);
    k.box(0.04, 0.02, 0.02, 0x3ddc84, 0.22, 0.86, 0.21, { cast: false });
  },

  whiteboard(g, f, k, dyn) {
    k.box(0.05, 1.85, 0.05, P.metal, -f.w / 2 + 0.05, 0.925, 0);
    k.box(0.05, 1.85, 0.05, P.metal, f.w / 2 - 0.05, 0.925, 0);
    k.box(0.4, 0.04, 0.4, P.metalDark, -f.w / 2 + 0.05, 0.02, 0);
    k.box(0.4, 0.04, 0.4, P.metalDark, f.w / 2 - 0.05, 0.02, 0);
    k.box(f.w, 1.12, 0.05, P.metal, 0, 1.31, 0);
    const disp = canvasTexture(768, 420, () => {});
    const m = new THREE.MeshBasicMaterial({ map: disp.tex });
    k.plane(f.w - 0.08, 1.04, m, 0, 1.31, 0.027, { dynamic: true });
    dyn.displays.whiteboard = disp;
  },

  standTable(g, f, k) {
    k.cyl(0.38, 0.38, 0.04, 10, P.deskTop, 0, 0.92, 0);
    k.cyl(0.03, 0.03, 0.9, 6, P.metalDark, 0, 0.46, 0);
    k.cyl(0.22, 0.24, 0.03, 10, P.metalDark, 0, 0.015, 0);
    k.cyl(0.035, 0.03, 0.08, 8, 0xffffff, 0.12, 0.98, 0.05);
  },

  rug(g, f, k) {
    k.box(f.w, 0.012, f.d, f.color ?? 0x888888, 0, 0.006, 0, { cast: false });
    k.box(f.w - 0.3, 0.013, f.d - 0.3, new THREE.Color(f.color ?? 0x888888).offsetHSL(0, 0, 0.05).getHex(), 0, 0.007, 0, { cast: false });
  },

  plant(g, f, k) {
    const big = f.size === 'large';
    const ph = big ? 0.42 : 0.3;
    k.cyl(big ? 0.2 : 0.16, big ? 0.15 : 0.12, ph, 8, P.plantPot, 0, ph / 2, 0);
    k.cyl(big ? 0.19 : 0.15, big ? 0.19 : 0.15, 0.02, 8, 0x5a4232, 0, ph, 0, { cast: false });
    k.ico(big ? 0.34 : 0.26, 0, P.leaf, 0, ph + (big ? 0.38 : 0.26), 0);
    k.ico(big ? 0.26 : 0.2, 0, P.leafDark, big ? 0.12 : 0.08, ph + (big ? 0.72 : 0.48), -0.05);
    if (big) k.ico(0.2, 0, P.leaf, -0.14, ph + 0.62, 0.08);
  },

  wallClock(g, f, k, dyn) {
    const y = f.y ?? 2.2;
    k.cyl(0.29, 0.29, 0.05, 20, P.black, 0, y, 0, { rx: Math.PI / 2 });
    k.cyl(0.26, 0.26, 0.052, 20, P.white, 0, y, 0.002, { rx: Math.PI / 2 });
    const hands = new THREE.Group();
    hands.position.set(0, y, 0.035);
    g.add(hands);
    const hour = new THREE.Group();
    const minute = new THREE.Group();
    hands.add(hour, minute);
    const hm = new THREE.Mesh(new THREE.BoxGeometry(0.03, 0.15, 0.01), mat(P.black));
    hm.position.y = 0.065;
    hour.add(hm);
    const mm = new THREE.Mesh(new THREE.BoxGeometry(0.02, 0.22, 0.01), mat(P.black));
    mm.position.y = 0.1;
    minute.add(mm);
    for (let i = 0; i < 12; i++) {
      const a = (i / 12) * Math.PI * 2;
      k.box(0.015, 0.04, 0.01, P.black, Math.sin(a) * 0.22, y + Math.cos(a) * 0.22, 0.03, { rz: -a, cast: false });
    }
    dyn.clock = { hour, minute };
  },

  counter(g, f, k) {
    k.box(f.w, 0.84, f.d, P.kitchen, 0, 0.42, 0);
    k.box(f.w + 0.02, 0.04, f.d + 0.02, P.woodDark, 0, 0.86, 0);
    for (let i = 0; i < 6; i++) k.box(0.7, 0.7, 0.012, 0xcfd8de, -f.w / 2 + 0.42 + i * 0.75, 0.42, f.d / 2 + 0.006);
    // Frontão baixo (azulejo) junto à parede: nada "flutua" quando a parede é cortada.
    k.box(f.w, 0.3, 0.03, 0xcfe0e6, 0, 1.03, -f.d / 2 + 0.015);
    // Máquina de café (em x = coffeeAt no mundo → local).
    const cx = (f.x - (f.coffeeAt ?? f.x)) ; // rot N: x local = −(x mundo − centro)
    k.box(0.32, 0.4, 0.3, P.black, cx, 1.08, -0.05);
    k.box(0.12, 0.06, 0.04, 0xff6b4a, cx, 1.18, 0.11, { cast: false });
    k.cyl(0.04, 0.035, 0.07, 8, P.white, cx, 0.915, 0.08);
    const sx = (f.x - (f.sinkAt ?? f.x));
    k.box(0.5, 0.02, 0.36, 0x9aa5b1, sx, 0.885, 0);
    k.cyl(0.015, 0.015, 0.25, 6, P.metal, sx, 1.0, -0.2);
    k.box(0.015, 0.015, 0.14, P.metal, sx, 1.12, -0.14);
  },

  fridge(g, f, k) {
    k.box(f.w, 1.8, f.d, P.fridge, 0, 0.9, 0);
    k.box(f.w - 0.02, 0.012, 0.01, 0xb8c2cc, 0, 1.2, f.d / 2 + 0.005);
    k.box(0.03, 0.4, 0.04, P.metal, f.w / 2 - 0.08, 1.45, f.d / 2 + 0.02);
    k.box(0.03, 0.3, 0.04, P.metal, f.w / 2 - 0.08, 0.85, f.d / 2 + 0.02);
  },

  roundTable(g, f, k) {
    k.cyl(0.45, 0.45, 0.04, 12, P.deskTop, 0, 0.6, 0);
    k.cyl(0.03, 0.03, 0.58, 6, P.metalDark, 0, 0.3, 0);
    k.cyl(0.24, 0.26, 0.03, 10, P.metalDark, 0, 0.015, 0);
    k.cyl(0.04, 0.035, 0.08, 8, 0xffffff, -0.1, 0.66, 0.1);
  },

  stool(g, f, k) {
    k.cyl(0.17, 0.17, 0.05, 10, 0xd0763b, 0, SEAT_H, 0);
    k.cyl(0.02, 0.02, SEAT_H, 6, P.metalDark, 0, SEAT_H / 2, 0);
    k.cyl(0.14, 0.15, 0.02, 10, P.metalDark, 0, 0.01, 0);
  },

  coatRack(g, f, k) {
    k.cyl(0.02, 0.02, 1.7, 6, P.woodDark, 0, 0.85, 0);
    k.cyl(0.18, 0.2, 0.03, 8, P.woodDark, 0, 0.015, 0);
    k.box(0.2, 0.5, 0.14, 0x34495e, 0.08, 1.3, 0.04, { rz: 0.1 });
    k.box(0.16, 0.4, 0.12, 0xa0522d, -0.07, 1.32, -0.05, { rz: -0.12 });
  },

  signTotem(g, f, k) {
    k.box(f.w, 1.6, f.d, P.abiyssDark, 0, 0.8, 0);
    const t = canvasTexture(512, 820, (ctx, w, h) => {
      ctx.fillStyle = '#1c2033';
      ctx.fillRect(0, 0, w, h);
      ctx.strokeStyle = '#48e5ff';
      ctx.lineWidth = 10;
      ctx.save();
      ctx.translate(w / 2, 260);
      ctx.rotate(Math.PI / 4);
      ctx.strokeRect(-90, -90, 180, 180);
      ctx.restore();
      ctx.fillStyle = '#48e5ff';
      ctx.font = `bold 96px ${FONT}`;
      ctx.textAlign = 'center';
      ctx.fillText('ABIYSS', w / 2, 560);
      ctx.fillStyle = '#a6afc3';
      ctx.font = `44px ${FONT}`;
      ctx.fillText('office', w / 2, 640);
    });
    k.plane(f.w - 0.1, 1.5, new THREE.MeshBasicMaterial({ map: t.tex }), 0, 0.82, f.d / 2 + 0.003);
  },

  wallArt(g, f, k) {
    const h = f.w * 0.72;
    k.box(f.w + 0.08, h + 0.08, 0.04, P.woodDark, 0, f.y, 0, { cast: false });
    const palettes = [
      ['#f6d6a8', '#e98b5a', '#5d7d6a', '#3e5a52'],
      ['#cfe3f2', '#7aa6c8', '#556f8f', '#2f3e57'],
      ['#f3e3c9', '#d7a86e', '#8e6c8a', '#4b3f63'],
    ][f.art % 3];
    const t = canvasTexture(256, 184, (ctx, w, hh) => {
      const grad = ctx.createLinearGradient(0, 0, 0, hh);
      grad.addColorStop(0, palettes[0]);
      grad.addColorStop(1, palettes[1]);
      ctx.fillStyle = grad;
      ctx.fillRect(0, 0, w, hh);
      ctx.fillStyle = '#fff3d6';
      ctx.beginPath();
      ctx.arc(w * (0.3 + 0.2 * f.art), hh * 0.35, 18, 0, Math.PI * 2);
      ctx.fill();
      // Serras de Minas em camadas.
      [[palettes[2], 0.55], [palettes[3], 0.72]].forEach(([c, base], layer) => {
        ctx.fillStyle = c;
        ctx.beginPath();
        ctx.moveTo(0, hh);
        for (let x = 0; x <= w; x += 32) ctx.lineTo(x, hh * base - ((x * (7 + layer * 3) + f.art * 50) % 37));
        ctx.lineTo(w, hh);
        ctx.closePath();
        ctx.fill();
      });
    });
    k.plane(f.w, h, new THREE.MeshBasicMaterial({ map: t.tex }), 0, f.y, 0.022);
  },

  console(g, f, k, dyn) {
    const dark = mat(0x23263a);
    k.box(f.w, 0.6, f.d, dark, 0, 0.3, 0);
    k.box(f.w + 0.06, 0.04, f.d + 0.06, mat(0x2f3450), 0, 0.62, 0);
    const strip = uniqueMat(P.accent, { emissive: P.accent, emissiveIntensity: 0.8 });
    k.box(f.w, 0.02, 0.02, strip, 0, 0.6, f.d / 2 + 0.03, { dynamic: true, cast: false });
    k.box(f.w, 0.02, 0.02, strip, 0, 0.6, -f.d / 2 - 0.03, { dynamic: true, cast: false });
    // Telas holográficas voltadas para o Abiyss (z local +).
    const holo = new THREE.MeshBasicMaterial({ color: P.accent, transparent: true, opacity: 0.28, side: THREE.DoubleSide, depthWrite: false, blending: THREE.AdditiveBlending });
    const holos = [];
    [[-0.95, 0.35], [0, 0], [0.95, -0.35]].forEach(([x, ry]) => {
      holos.push(k.plane(0.8, 0.5, holo, x, 1.15, -0.05, { dynamic: true, ry }));
    });
    // Bandeja de entrada (cartões = sub-agentes pendentes).
    k.box(0.36, 0.04, 0.26, 0x3a405a, -1.25, 0.66, 0.15);
    const cards = [];
    for (let i = 0; i < 5; i++) {
      cards.push(k.box(0.3, 0.012, 0.2, mat(0xfff2c2), -1.25, 0.69 + i * 0.016, 0.15, { dynamic: true, ry: (i % 2 ? 0.08 : -0.06), cast: false }));
    }
    dyn.console = { strip, holo, holos, cards };
  },

  dais(g, f, k, dyn) {
    k.cyl(0.75, 0.8, 0.08, 6, mat(0x2a2f48), 0, 0.04, 0);
    const ring = uniqueMat(P.accent, { emissive: P.accent, emissiveIntensity: 0.9 });
    k.torus(0.62, 0.015, 4, 6, ring, 0, 0.085, 0, { rx: Math.PI / 2, dynamic: true, cast: false });
    dyn.daisRing = ring;
  },

  statusWall(g, f, k, dyn) {
    const y = f.y ?? 1.9;
    k.box(f.w + 0.12, 1.82, 0.06, P.black, 0, y, -0.01);
    const disp = canvasTexture(1024, 540, () => {});
    k.plane(f.w, 1.7, new THREE.MeshBasicMaterial({ map: disp.tex }), 0, y, 0.025, { dynamic: true });
    dyn.displays.status = disp;
  },

  serverRack(g, f, k, dyn) {
    k.box(f.w, 1.9, f.d, P.rack, 0, 0.95, 0);
    const mats = [0, 1, 2].map((i) => uniqueMat(0x223, { emissive: [0x3ddc84, P.accent, 0xffb547][i], emissiveIntensity: 0.9 }));
    const geos = [[], [], []];
    for (let r = 0; r < 8; r++) {
      k.box(f.w - 0.08, 0.16, 0.01, 0x323a52, 0, 0.3 + r * 0.2, f.d / 2 + 0.005, { cast: false });
      for (let c = 0; c < 3; c++) {
        const geo = new THREE.BoxGeometry(0.03, 0.03, 0.01);
        geo.translate(-0.22 + c * 0.06, 0.3 + r * 0.2, f.d / 2 + 0.012);
        geos[(r + c) % 3].push(geo);
      }
    }
    geos.forEach((list, i) => {
      const m = new THREE.Mesh(mergeGeometries(list, false), mats[i]);
      m.userData.static = false;
      g.add(m);
    });
    dyn.rackLeds = mats;
  },

  armchair(g, f, k) {
    const c = 0x35507a;
    k.box(f.w, 0.3, f.d, c, 0, 0.2, 0);
    k.box(f.w - 0.2, 0.1, f.d - 0.2, 0x46679a, 0, 0.38, 0.05);
    k.box(f.w, 0.5, 0.18, c, 0, 0.55, -f.d / 2 + 0.09);
    k.box(0.12, 0.28, f.d, c, -f.w / 2 + 0.06, 0.45, 0);
    k.box(0.12, 0.28, f.d, c, f.w / 2 - 0.06, 0.45, 0);
  },

  sofa(g, f, k) {
    const c = f.id === 'sofa-a' ? P.sofa : P.sofaB;
    const cushion = new THREE.Color(c).offsetHSL(0, 0, 0.07).getHex();
    k.box(f.w, 0.24, f.d, c, 0, 0.16, 0);
    const n = f.seats ?? 2;
    const inner = f.w - 0.36;
    for (let i = 0; i < n; i++) {
      const w = inner / n - 0.02;
      k.box(w, 0.12, f.d - 0.3, cushion, -inner / 2 + (i + 0.5) * (inner / n), 0.33, 0.1);
    }
    k.box(f.w, 0.5, 0.26, c, 0, 0.53, -f.d / 2 + 0.13);
    k.box(0.18, 0.44, f.d, c, -f.w / 2 + 0.09, 0.3, 0);
    k.box(0.18, 0.44, f.d, c, f.w / 2 - 0.09, 0.3, 0);
    k.box(0.32, 0.26, 0.12, 0xf2d0a4, -f.w / 2 + 0.4, 0.5, -0.18, { rx: -0.3 });
  },

  coffeeTable(g, f, k) {
    k.box(f.w, 0.05, f.d, P.wood, 0, 0.32, 0);
    for (const [x, z] of [[-1, -1], [1, -1], [-1, 1], [1, 1]]) k.box(0.04, 0.3, 0.04, P.woodDark, x * (f.w / 2 - 0.06), 0.15, z * (f.d / 2 - 0.06));
    k.box(0.3, 0.02, 0.22, 0x4f81bd, -0.3, 0.355, 0.05, { ry: 0.3 });
    k.cyl(0.07, 0.06, 0.1, 8, P.plantPot, 0.35, 0.39, 0);
    k.ico(0.1, 0, P.leaf, 0.35, 0.52, 0);
  },

  tvCabinet(g, f, k, dyn) {
    k.box(f.w, 0.45, f.d, P.woodDark, 0, 0.225, 0);
    k.box(0.3, 0.04, 0.2, P.metalDark, 0, 0.47, -0.05);
    k.box(0.04, 0.2, 0.04, P.metalDark, 0, 0.58, -0.08);
    k.box(1.56, 0.9, 0.05, P.black, 0, 1.12, -0.08);
    const disp = canvasTexture(640, 360, () => {});
    k.plane(1.48, 0.83, new THREE.MeshBasicMaterial({ map: disp.tex }), 0, 1.12, -0.052, { dynamic: true });
    dyn.displays.tv = disp;
  },

  beanbag(g, f, k) {
    k.ico(0.42, 1, mat(f.color ?? 0xd9825b), 0, 0.2, 0, { scale: [1, 0.5, 1] });
    k.ico(0.3, 1, mat(new THREE.Color(f.color ?? 0xd9825b).offsetHSL(0, 0, -0.05).getHex()), 0, 0.36, -0.12, { scale: [1, 0.7, 0.6] });
  },

  floorLamp(g, f, k, dyn) {
    k.cyl(0.16, 0.18, 0.03, 10, P.metalDark, 0, 0.015, 0);
    k.cyl(0.018, 0.018, 1.5, 6, P.metalDark, 0, 0.76, 0);
    const shade = k.cyl(0.14, 0.22, 0.26, 10, uniqueMat(0xf3e3c3, { emissive: 0x000000 }), 0, 1.58, 0, { dynamic: true });
    dyn.lamps.push(shade);
  },

  planter(g, f, k) {
    k.box(f.w, 0.5, f.d, P.wood, 0, 0.25, 0);
    k.box(f.w - 0.06, 0.02, f.d - 0.06, 0x5a4232, 0, 0.5, 0, { cast: false });
    for (let i = 0; i < 4; i++) {
      const z = -f.d / 2 + 0.35 + i * ((f.d - 0.7) / 3);
      k.ico(0.2, 0, i % 2 ? P.leafDark : P.leaf, 0, 0.72, z);
      k.cone(0.12, 0.5, 5, P.leaf, 0.02, 0.9, z + 0.1);
    }
  },
};

/** Faixa jateada com o nome, colada na divisória de vidro. */
export function frostedBand(text) {
  const t = canvasTexture(1024, 128, (ctx, w, h) => {
    ctx.fillStyle = 'rgba(255,255,255,0.55)';
    ctx.fillRect(0, 0, w, h);
    ctx.fillStyle = '#1c2033';
    ctx.font = `bold 64px ${FONT}`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(text, w / 2, h / 2 + 4);
  });
  return new THREE.MeshBasicMaterial({ map: t.tex, transparent: true, side: THREE.DoubleSide, depthWrite: false });
}

export { glassMat };
