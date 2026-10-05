// Constrói a cena estática do escritório a partir de shared/layout.js:
// pisos, paredes (janelas, portas, divisórias de vidro), móveis, exterior
// (gramado, calçada, rua, árvores, morros) e as luzes internas.
//
// Peças estáticas são unidas por material (mergeGeometries) para reduzir
// as chamadas de desenho; peças dinâmicas ficam soltas e são registradas
// em `dyn` para o restante do cliente animar.

import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';

import { BUILDING, FURNITURE, ROOMS, WALLS, SIDEWALK, wallPoint } from '../../shared/layout.js';
import { PALETTE as P, mat, glassMat, canvasTexture } from './materials.js';
import { FURNITURE_BUILDERS, kit, frostedBand } from './furniture.js';

const H = BUILDING.wallHeight;
const T = BUILDING.wallThickness;

function floorTexture(kind) {
  if (kind === 'wood') {
    return canvasTexture(512, 512, (ctx, w, h) => {
      const rows = 8;
      for (let r = 0; r < rows; r++) {
        let x = (r % 2) * -128;
        while (x < w) {
          const len = 256;
          const shade = 0.92 + ((r * 7 + x) % 5) * 0.025;
          ctx.fillStyle = `rgb(${Math.round(201 * shade)},${Math.round(164 * shade)},${Math.round(122 * shade)})`;
          ctx.fillRect(x, r * (h / rows), len, h / rows);
          ctx.fillStyle = 'rgba(80,50,30,0.25)';
          ctx.fillRect(x, r * (h / rows), 2, h / rows);
          x += len;
        }
        ctx.fillStyle = 'rgba(80,50,30,0.3)';
        ctx.fillRect(0, r * (h / rows), w, 2);
      }
    });
  }
  if (kind === 'tile') {
    return canvasTexture(256, 256, (ctx, w, h) => {
      ctx.fillStyle = '#e7e2d8';
      ctx.fillRect(0, 0, w, h);
      ctx.fillStyle = '#dcd5c8';
      ctx.fillRect(0, 0, w / 2, h / 2);
      ctx.fillRect(w / 2, h / 2, w / 2, h / 2);
      ctx.strokeStyle = 'rgba(120,110,95,0.35)';
      ctx.lineWidth = 3;
      ctx.strokeRect(0, 0, w, h);
      ctx.beginPath();
      ctx.moveTo(w / 2, 0); ctx.lineTo(w / 2, h); ctx.moveTo(0, h / 2); ctx.lineTo(w, h / 2);
      ctx.stroke();
    });
  }
  if (kind === 'carpet') {
    return canvasTexture(256, 256, (ctx, w, h) => {
      ctx.fillStyle = '#9db3a8';
      ctx.fillRect(0, 0, w, h);
      for (let i = 0; i < 1400; i++) {
        const v = 150 + ((i * 97) % 40);
        ctx.fillStyle = `rgba(${v - 20},${v},${v - 10},0.25)`;
        ctx.fillRect((i * 53) % w, (i * 29) % h, 2, 2);
      }
    });
  }
  // dark: sala do Abiyss
  return canvasTexture(256, 256, (ctx, w, h) => {
    ctx.fillStyle = '#2e3346';
    ctx.fillRect(0, 0, w, h);
    ctx.strokeStyle = 'rgba(72,229,255,0.16)';
    ctx.lineWidth = 2;
    ctx.strokeRect(1, 1, w - 2, h - 2);
    ctx.fillStyle = 'rgba(255,255,255,0.03)';
    ctx.fillRect(0, 0, w / 2, h / 2);
    ctx.fillRect(w / 2, h / 2, w / 2, h / 2);
  });
}

function seeded(seed) {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

class StaticBatcher {
  constructor() {
    this.buckets = new Map();
  }

  /** Move a malha estática para o lote do material (com a transformação de mundo). */
  take(mesh) {
    mesh.updateWorldMatrix(true, false);
    let geo = mesh.geometry.index ? mesh.geometry.toNonIndexed() : mesh.geometry.clone();
    geo.applyMatrix4(mesh.matrixWorld);
    for (const name of Object.keys(geo.attributes)) if (!['position', 'normal', 'uv'].includes(name)) geo.deleteAttribute(name);
    const key = mesh.material.uuid + (mesh.castShadow ? 'c' : '') + (mesh.receiveShadow ? 'r' : '');
    let b = this.buckets.get(key);
    if (!b) {
      b = { material: mesh.material, cast: mesh.castShadow, receive: mesh.receiveShadow, geos: [] };
      this.buckets.set(key, b);
    }
    b.geos.push(geo);
    mesh.removeFromParent();
  }

  build() {
    const group = new THREE.Group();
    group.name = 'estatico';
    for (const b of this.buckets.values()) {
      const merged = mergeGeometries(b.geos, false);
      if (!merged) continue;
      const m = new THREE.Mesh(merged, b.material);
      m.castShadow = b.cast;
      m.receiveShadow = b.receive;
      group.add(m);
    }
    return group;
  }
}

/** Une as malhas de um grupo por material (coordenadas relativas ao grupo). */
function mergeGroup(g) {
  g.updateMatrixWorld(true);
  const inv = new THREE.Matrix4().copy(g.matrixWorld).invert();
  const buckets = new Map();
  const meshes = [];
  g.traverse((o) => { if (o.isMesh) meshes.push(o); });
  for (const m of meshes) {
    let geo = m.geometry.index ? m.geometry.toNonIndexed() : m.geometry.clone();
    geo.applyMatrix4(new THREE.Matrix4().multiplyMatrices(inv, m.matrixWorld));
    for (const name of Object.keys(geo.attributes)) if (!['position', 'normal', 'uv'].includes(name)) geo.deleteAttribute(name);
    const key = m.material.uuid;
    if (!buckets.has(key)) buckets.set(key, { material: m.material, geos: [], cast: m.castShadow, order: m.renderOrder });
    buckets.get(key).geos.push(geo);
  }
  for (const c of [...g.children]) c.removeFromParent();
  for (const b of buckets.values()) {
    const merged = new THREE.Mesh(mergeGeometries(b.geos, false), b.material);
    merged.castShadow = b.cast && !b.material.transparent;
    merged.receiveShadow = !b.material.transparent;
    merged.renderOrder = b.order;
    g.add(merged);
  }
}

export function buildOffice(scene) {
  const root = new THREE.Group();
  root.name = 'escritorio';
  scene.add(root);
  const dyn = {
    desks: new Map(),
    lamps: [],
    displays: {},
    clock: null,
    console: null,
    daisRing: null,
    rackLeds: null,
    exteriorWalls: [],
    nightLights: [],
    streetLamps: [],
  };
  const batch = new StaticBatcher();
  const furnitureRoot = new THREE.Group();
  root.add(furnitureRoot);

  // --- base e pisos -----------------------------------------------------
  const slab = new THREE.Mesh(new THREE.BoxGeometry(BUILDING.width + 0.4, 0.14, BUILDING.depth + 0.4), mat(0xd6cfc2));
  slab.position.set(BUILDING.width / 2, -0.07, BUILDING.depth / 2);
  slab.receiveShadow = true;
  root.add(slab);
  const floorTex = {};
  for (const r of ROOMS) {
    const w = r.x1 - r.x0;
    const d = r.z1 - r.z0;
    floorTex[r.floor] ??= floorTexture(r.floor);
    const tex = floorTex[r.floor].tex.clone();
    tex.needsUpdate = true;
    const scale = r.floor === 'tile' ? 0.8 : r.floor === 'wood' ? 2.4 : 2;
    tex.repeat.set(w / scale, d / scale);
    tex.wrapS = THREE.RepeatWrapping;
    tex.wrapT = THREE.RepeatWrapping;
    const floor = new THREE.Mesh(new THREE.PlaneGeometry(w, d), new THREE.MeshLambertMaterial({ map: tex }));
    floor.rotation.x = -Math.PI / 2;
    floor.position.set((r.x0 + r.x1) / 2, 0.002, (r.z0 + r.z1) / 2);
    floor.receiveShadow = true;
    root.add(floor);
  }

  // --- paredes ------------------------------------------------------------
  for (const w of WALLS) {
    const dx = w.x2 - w.x1;
    const dz = w.z2 - w.z1;
    const len = Math.hypot(dx, dz);
    const g = new THREE.Group();
    g.position.set(w.x1, 0, w.z1);
    g.rotation.y = -Math.atan2(dz, dx);
    root.add(g);
    const k = kit(g);
    const glass = w.kind === 'glass';
    const wallMat = mat(glass ? P.frame : P.wall);
    const cuts = [...w.openings].sort((a, b) => a.at - b.at);
    let cursor = 0;
    const solid = (a, b) => {
      if (b - a < 0.005) return;
      if (glass) {
        // Divisória de vidro: montantes + vidro.
        const posts = Math.max(1, Math.round((b - a) / 1.35));
        for (let i = 0; i <= posts; i++) k.box(0.06, H, 0.08, P.frame, a + ((b - a) * i) / posts, H / 2, 0, { dynamic: true });
        const pane = k.plane(b - a, H - 0.12, glassMat(0.16), (a + b) / 2, H / 2, 0, { dynamic: true, receive: false });
        pane.renderOrder = 2;
      } else {
        k.box(b - a, H, T, wallMat, (a + b) / 2, H / 2, 0, { dynamic: true });
        for (const s of [-1, 1]) k.box(b - a, 0.09, 0.012, P.baseboard, (a + b) / 2, 0.045, s * (T / 2 + 0.006), { dynamic: true, cast: false });
      }
    };
    for (const o of cuts) {
      const a = o.at - o.width / 2;
      const b = o.at + o.width / 2;
      solid(cursor, a);
      if (o.kind === 'window' && !glass) {
        k.box(o.width, o.sill, T, wallMat, o.at, o.sill / 2, 0, { dynamic: true });
        k.box(o.width, H - o.top, T, wallMat, o.at, (H + o.top) / 2, 0, { dynamic: true });
        const pane = k.plane(o.width - 0.08, o.top - o.sill - 0.08, glassMat(0.22, 0xbfe6ff), o.at, (o.sill + o.top) / 2, 0, { dynamic: true, receive: false });
        pane.renderOrder = 2;
        k.box(o.width, 0.05, T + 0.06, P.white, o.at, o.sill, 0, { dynamic: true });
        k.box(o.width, 0.04, T + 0.02, P.frame, o.at, o.top, 0, { dynamic: true });
        k.box(0.04, o.top - o.sill, T + 0.02, P.frame, a + 0.02, (o.sill + o.top) / 2, 0, { dynamic: true });
        k.box(0.04, o.top - o.sill, T + 0.02, P.frame, b - 0.02, (o.sill + o.top) / 2, 0, { dynamic: true });
        k.box(0.03, o.top - o.sill, T + 0.02, P.frame, o.at, (o.sill + o.top) / 2, 0, { dynamic: true });
      } else if (o.kind === 'door') {
        if (glass) {
          k.box(o.width, H - o.top, 0.08, P.frame, o.at, (H + o.top) / 2, 0, { dynamic: true });
          k.box(0.06, o.top, 0.08, P.frame, a, o.top / 2, 0, { dynamic: true });
          k.box(0.06, o.top, 0.08, P.frame, b, o.top / 2, 0, { dynamic: true });
          // Folha de vidro aberta (girada para dentro da sala).
          const leaf = new THREE.Group();
          leaf.position.set(b, 0, 0);
          leaf.rotation.y = -1.35;
          g.add(leaf);
          const lk = kit(leaf);
          lk.plane(o.width - 0.08, o.top - 0.04, glassMat(0.22), -(o.width - 0.08) / 2, o.top / 2, 0, { dynamic: true });
          lk.box(0.04, o.top - 0.04, 0.05, P.frame, -(o.width - 0.08), o.top / 2, 0, { dynamic: true });
        } else {
          k.box(o.width, H - o.top, T, wallMat, o.at, (H + o.top) / 2, 0, { dynamic: true });
          k.box(o.width + 0.1, 0.06, T + 0.08, P.frame, o.at, o.top, 0, { dynamic: true });
          k.box(0.06, o.top, T + 0.08, P.frame, a, o.top / 2, 0, { dynamic: true });
          k.box(0.06, o.top, T + 0.08, P.frame, b, o.top / 2, 0, { dynamic: true });
          // Porta dupla de vidro aberta para fora.
          for (const side of [-1, 1]) {
            const leaf = new THREE.Group();
            leaf.position.set(side < 0 ? a + 0.03 : b - 0.03, 0, 0);
            leaf.rotation.y = side < 0 ? 1.25 : -1.25;
            g.add(leaf);
            const lk = kit(leaf);
            const lw = o.width / 2 - 0.05;
            lk.plane(lw, o.top - 0.06, glassMat(0.25), side < 0 ? lw / 2 : -lw / 2, o.top / 2, -0.02, { dynamic: true });
            lk.box(0.04, o.top - 0.06, 0.05, P.frame, side < 0 ? lw : -lw, o.top / 2, -0.02, { dynamic: true });
          }
        }
      }
      cursor = b;
    }
    solid(cursor, len);
    if (glass) {
      k.box(len, 0.06, 0.09, P.frame, len / 2, H - 0.03, 0, { dynamic: true });
      k.box(len, 0.08, 0.09, P.frame, len / 2, 0.04, 0, { dynamic: true });
      if (w.id === 'glass-abiyss-south') {
        k.plane(3.2, 0.4, frostedBand('ABIYSS'), len / 2, 1.5, 0.05, { dynamic: true });
      }
    } else {
      k.box(len + T, 0.05, T + 0.04, P.trim, len / 2, H + 0.025, 0, { dynamic: true });
    }
    mergeGroup(g);
    if (w.kind === 'exterior') {
      const mid = wallPoint(w, len / 2);
      dyn.exteriorWalls.push({ group: g, outward: w.outward, center: { x: mid[0], z: mid[1] }, cut: 0, id: w.id });
    }
  }

  // --- móveis --------------------------------------------------------------
  for (const f of FURNITURE) {
    const builder = FURNITURE_BUILDERS[f.type];
    if (!builder) continue;
    const g = new THREE.Group();
    g.position.set(f.x, 0, f.z);
    g.rotation.y = f.rot;
    g.name = f.id;
    furnitureRoot.add(g);
    builder(g, f, kit(g), dyn);
  }
  furnitureRoot.updateMatrixWorld(true);
  const statics = [];
  furnitureRoot.traverse((o) => { if (o.isMesh && o.userData.static) statics.push(o); });
  for (const m of statics) batch.take(m);

  // --- exterior ------------------------------------------------------------
  buildOutside(root, batch, dyn);
  root.add(batch.build());

  // --- luzes internas (acesas à noite / dia escuro) ---------------------------------
  const lightSpots = [
    [7.5, 2.5, 3.6], [7.5, 2.5, 8.6], [3.0, 2.4, 13.4], [11.2, 2.4, 13.2], [20, 2.4, 12.2], [20, 2.4, 4.6],
  ];
  for (const [x, y, z] of lightSpots) {
    const l = new THREE.PointLight(0xffe2b8, 0, 9, 1.6);
    l.position.set(x, y, z);
    root.add(l);
    dyn.nightLights.push(l);
  }
  return { root, dyn };
}

function buildOutside(root, batch, dyn) {
  const outside = new THREE.Group();
  root.add(outside);
  const k = kit(outside);
  // Gramado.
  const ground = new THREE.Mesh(new THREE.PlaneGeometry(220, 220, 1, 1), mat(P.grass));
  ground.rotation.x = -Math.PI / 2;
  ground.position.set(12, -0.02, 8);
  ground.receiveShadow = true;
  root.add(ground);
  // Calçada até a porta, calçada da rua, rua e faixas.
  k.box(SIDEWALK.x1 - SIDEWALK.x0, 0.03, 21 - SIDEWALK.z0, P.path, (SIDEWALK.x0 + SIDEWALK.x1) / 2, 0.0, (SIDEWALK.z0 + 21) / 2, { cast: false });
  k.box(80, 0.03, 2, P.path, 12, 0.0, 21.5, { cast: false });
  k.box(80, 0.02, 7, P.asphalt, 12, -0.005, 26, { cast: false });
  for (let x = -26; x < 50; x += 4) k.box(2, 0.022, 0.14, 0xf2f2f2, x, 0.002, 26, { cast: false });
  k.box(80, 0.03, 2, P.path, 12, 0.0, 30.5, { cast: false });
  // Arbustos ao redor do prédio.
  const rnd = seeded(42);
  const bushes = [];
  for (let x = 0.8; x < 24; x += 1.6) bushes.push([x, -0.7], [x, 16.7]);
  for (let z = 0.8; z < 16; z += 1.6) bushes.push([-0.7, z], [24.7, z]);
  for (const [x, z] of bushes) {
    if (z > 16 && x > 9.2 && x < 13.2) continue; // entrada livre
    k.ico(0.38 + rnd() * 0.18, 0, rnd() > 0.5 ? P.leaf : P.leafDark, x, 0.3, z, { scale: [1, 0.75, 1] });
  }
  // Árvores.
  const trees = [
    [-4, 2], [-5, 8], [-4.5, 14], [-3, -4], [4, -4.5], [11, -5], [18, -4], [26, -4.5], [29, 3], [28.5, 10],
    [29.5, 16], [5, 18.8], [17.5, 18.6], [-6, 19], [31, 19.5], [-2, 34], [9, 34], [21, 34.5], [33, 33],
  ];
  for (const [x, z] of trees) {
    const s = 0.8 + rnd() * 0.6;
    k.cyl(0.12 * s, 0.16 * s, 1.4 * s, 6, P.trunk, x, 0.7 * s, z);
    k.cone(1.1 * s, 2.2 * s, 7, rnd() > 0.5 ? P.tree : P.treeDark, x, 2.2 * s, z);
    k.cone(0.8 * s, 1.6 * s, 7, P.tree, x, 3.1 * s, z);
  }
  // Morros ao longe (paisagem de Minas).
  for (let i = 0; i < 22; i++) {
    const a = (i / 22) * Math.PI * 2 + rnd() * 0.2;
    const r = 85 + rnd() * 30;
    const h = 10 + rnd() * 22;
    k.cone(18 + rnd() * 14, h, 6, rnd() > 0.5 ? P.hill : P.hillFar, 12 + Math.cos(a) * r, h / 2 - 1, 8 + Math.sin(a) * r, { cast: false, ry: rnd() * 3 });
  }
  // Postes e banco.
  for (const x of [5.5, 17.5, 29.5, -6.5]) {
    k.cyl(0.05, 0.07, 4.2, 6, P.metalDark, x, 2.1, 20.6);
    k.box(0.9, 0.06, 0.14, P.metalDark, x - 0.4, 4.15, 20.6);
    const head = k.box(0.38, 0.1, 0.24, new THREE.MeshLambertMaterial({ color: 0xfff1cc, emissive: 0x000000 }), x - 0.75, 4.08, 20.6, { dynamic: true });
    dyn.streetLamps.push(head);
  }
  k.box(1.6, 0.06, 0.45, P.wood, 14.4, 0.42, 17.6);
  k.box(1.6, 0.4, 0.06, P.wood, 14.4, 0.66, 17.82);
  for (const x of [13.75, 15.05]) k.box(0.06, 0.42, 0.42, P.metalDark, x, 0.21, 17.6);
  outside.updateMatrixWorld(true);
  const statics = [];
  outside.traverse((o) => { if (o.isMesh && o.userData.static) statics.push(o); });
  for (const m of statics) batch.take(m);
}

