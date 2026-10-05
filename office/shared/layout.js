// Layout do escritório do Abiyss — FONTE ÚNICA da geometria.
//
// O servidor usa este arquivo para montar a grade de navegação (paredes,
// portas, móveis, pontos de interesse) e o cliente usa o MESMO arquivo para
// construir a cena 3D. Assim, o que o IMMo "enxerga" para andar é exatamente
// o que aparece na tela.
//
// Unidades: metros. Eixos: x para leste, z para sul, y para cima.
// O prédio ocupa x ∈ [0, 24] e z ∈ [0, 16].
//
// Convenção de orientação: um ângulo θ (rotação em torno de y) faz a "frente"
// apontar para (sin θ, cos θ) no plano xz. θ = 0 → sul (+z), θ = π → norte,
// θ = π/2 → leste (+x), θ = −π/2 → oeste.
//
// Para simplificar a navegação, móveis só usam rotações múltiplas de 90°.

export const LAYOUT_VERSION = 1;

const PI = Math.PI;
export const FACING = Object.freeze({ S: 0, N: PI, E: PI / 2, W: -PI / 2 });

export const BUILDING = Object.freeze({
  width: 24,
  depth: 16,
  wallHeight: 2.8,
  wallThickness: 0.2,
  glassHeight: 2.8,
});

/** Raio de navegação de todas as entidades (m). */
export const AGENT_RADIUS = 0.3;

/** Áreas (para piso, rótulos e regras de comportamento). */
export const ROOMS = Object.freeze([
  { id: 'workspace', name: 'Área de trabalho', x0: 0, z0: 0, x1: 16, z1: 10.8, floor: 'wood' },
  { id: 'abiyss', name: 'Sala do Abiyss', x0: 16, z0: 0, x1: 24, z1: 8, floor: 'dark' },
  { id: 'lounge', name: 'Área de descanso', x0: 16, z0: 8, x1: 24, z1: 16, floor: 'carpet' },
  { id: 'kitchen', name: 'Copa', x0: 0, z0: 10.8, x1: 6.4, z1: 16, floor: 'tile' },
  { id: 'hall', name: 'Entrada', x0: 6.4, z0: 10.8, x1: 16, z1: 16, floor: 'wood' },
]);

/** Calçada externa (única área navegável fora do prédio). */
export const SIDEWALK = Object.freeze({ x0: 9.6, z0: 16.1, x1: 12.8, z1: 18.8 });

// ---------------------------------------------------------------------------
// Paredes. `openings` usam a distância ao longo do segmento a partir de (x1,z1).
// ---------------------------------------------------------------------------

const win = (at, width = 1.8, sill = 0.9, top = 2.2) => ({ kind: 'window', at, width, sill, top });
const door = (at, width) => ({ kind: 'door', at, width, top: 2.2 });

export const WALLS = Object.freeze([
  {
    id: 'wall-north', kind: 'exterior', x1: 0, z1: 0, x2: 24, z2: 0, outward: [0, -1],
    openings: [win(2.2), win(6.4), win(10.4)],
  },
  {
    id: 'wall-east', kind: 'exterior', x1: 24, z1: 0, x2: 24, z2: 16, outward: [1, 0],
    openings: [win(2.5), win(5.5), win(11.0), win(13.9)],
  },
  {
    id: 'wall-south', kind: 'exterior', x1: 0, z1: 16, x2: 24, z2: 16, outward: [0, 1],
    openings: [win(3.0, 1.6, 1.15, 2.1), win(7.6, 1.6), door(11.2, 1.6), win(19.0), win(22.0)],
  },
  {
    id: 'wall-west', kind: 'exterior', x1: 0, z1: 0, x2: 0, z2: 16, outward: [-1, 0],
    openings: [win(2.5, 1.6), win(9.4, 1.6), win(13.2, 1.6)],
  },
  // Sala do Abiyss: divisórias de vidro.
  { id: 'glass-abiyss-west', kind: 'glass', x1: 16, z1: 0, x2: 16, z2: 8, openings: [door(5.0, 1.4)] },
  { id: 'glass-abiyss-south', kind: 'glass', x1: 16, z1: 8, x2: 24, z2: 8, openings: [] },
]);

/** Portas como objetos (para o cliente desenhar folhas e o servidor nomear). */
export const DOORS = Object.freeze([
  { id: 'door-main', wall: 'wall-south', x: 11.2, z: 16, width: 1.6, axis: 'x', label: 'Entrada principal' },
  { id: 'door-abiyss', wall: 'glass-abiyss-west', x: 16, z: 5.0, width: 1.4, axis: 'z', label: 'Porta da sala do Abiyss' },
]);

// ---------------------------------------------------------------------------
// Móveis. `w` é a largura no eixo x LOCAL, `d` a profundidade no z LOCAL
// (antes da rotação). `block`: 'full' (todo o retângulo bloqueia),
// 'none' (decoração no chão / parede) ou 'sofa' (encosto + braços bloqueiam;
// os assentos são máscaras de assento dos POIs).
// ---------------------------------------------------------------------------

const F = [];
const add = (type, id, x, z, rot, w, d, extra = {}) =>
  F.push({ type, id, x, z, rot, w, d, block: 'full', ...extra });

// Ilhas de mesas (pods): duas mesas encostadas, um IMMo de cada lado.
export const DESK_PODS = Object.freeze([
  { id: 'pod-a', cx: 4.6, cz: 3.4, desks: [1, 2] },
  { id: 'pod-b', cx: 10.4, cz: 3.4, desks: [3, 4] },
]);

for (const pod of DESK_PODS) {
  const [west, east] = pod.desks;
  // A mesa tem largura 1.6 (x local) e profundidade 0.8; o IMMo senta no +z local.
  add('desk', `desk-${west}`, pod.cx - 0.4, pod.cz, FACING.W, 1.6, 0.8, { desk: west });
  add('desk', `desk-${east}`, pod.cx + 0.4, pod.cz, FACING.E, 1.6, 0.8, { desk: east });
  add('chair', `chair-${west}`, pod.cx - 1.3, pod.cz, FACING.E, 0.5, 0.5, { block: 'none', seatOf: `desk-${west}` });
  add('chair', `chair-${east}`, pod.cx + 1.3, pod.cz, FACING.W, 0.5, 0.5, { block: 'none', seatOf: `desk-${east}` });
  add('deskDivider', `${pod.id}-divider`, pod.cx, pod.cz, 0, 0.06, 1.6, { block: 'none' });
}

// Área de trabalho
add('filingCabinet', 'cabinet-1', 0.4, 6.0, FACING.E, 0.9, 0.55);
add('filingCabinet', 'cabinet-2', 0.4, 7.0, FACING.E, 0.9, 0.55);
add('waterCooler', 'water-cooler', 0.4, 4.7, FACING.E, 0.4, 0.4);
add('bookshelf', 'shelf-work', 12.9, 0.35, FACING.S, 1.6, 0.4);
add('printer', 'printer', 14.6, 0.55, FACING.S, 0.9, 0.6);
add('whiteboard', 'goals-board', 13.4, 5.4, FACING.S, 2.0, 0.3);
add('standTable', 'stand-table', 8.0, 8.4, 0, 0.7, 0.7);
add('rug', 'rug-center', 8.0, 7.9, 0, 4.4, 2.8, { block: 'none', color: 0x7f8fa6 });
add('plant', 'plant-w1', 0.5, 0.5, 0, 0.5, 0.5, { size: 'large' });
add('plant', 'plant-w2', 15.5, 0.5, 0, 0.5, 0.5, { size: 'large' });
add('plant', 'plant-w3', 0.5, 10.3, 0, 0.5, 0.5, { size: 'large' });
add('plant', 'plant-w4', 15.45, 10.4, 0, 0.5, 0.5, { size: 'medium' });
add('wallClock', 'wall-clock', 8.4, 0.12, FACING.S, 0.6, 0.05, { block: 'none', y: 2.25 });
add('wallArt', 'art-north', 4.3, 0.13, FACING.S, 1.3, 0.04, { block: 'none', y: 1.65, art: 0 });
add('wallArt', 'art-west', 0.13, 6.5, FACING.E, 1.1, 0.04, { block: 'none', y: 1.75, art: 1 });
add('wallArt', 'art-lounge', 23.87, 12.45, FACING.W, 0.8, 0.04, { block: 'none', y: 1.65, art: 2 });

// Copa
add('counter', 'kitchen-counter', 2.5, 15.55, FACING.N, 4.6, 0.7, { coffeeAt: 1.4, sinkAt: 3.0 });
add('fridge', 'fridge', 0.45, 11.8, FACING.E, 0.7, 0.7);
add('roundTable', 'kitchen-table', 3.2, 12.6, 0, 0.9, 0.9);
add('stool', 'stool-1', 2.4, 12.6, FACING.E, 0.4, 0.4, { block: 'none' });
add('stool', 'stool-2', 4.0, 12.6, FACING.W, 0.4, 0.4, { block: 'none' });
add('plant', 'plant-k1', 5.9, 15.5, 0, 0.5, 0.5, { size: 'medium' });

// Entrada
add('coatRack', 'coat-rack', 9.5, 15.5, 0, 0.4, 0.4);
add('plant', 'plant-e1', 13.2, 15.5, 0, 0.5, 0.5, { size: 'large' });
add('rug', 'door-mat', 11.2, 15.35, 0, 1.6, 0.9, { block: 'none', color: 0x3b4252 });
add('signTotem', 'sign-totem', 14.4, 14.2, FACING.S, 1.0, 0.3);

// Sala do Abiyss
add('console', 'abiyss-console', 20.0, 3.2, FACING.N, 3.0, 0.9);
add('dais', 'abiyss-dais', 20.0, 1.9, 0, 1.4, 1.4, { block: 'none' });
add('statusWall', 'status-wall', 20.0, 0.12, FACING.S, 3.4, 0.05, { block: 'none', y: 1.9 });
add('serverRack', 'server-rack', 17.0, 0.45, FACING.S, 0.7, 0.55);
add('bookshelf', 'shelf-abiyss', 22.9, 0.35, FACING.S, 1.6, 0.4);
add('armchair', 'abiyss-armchair', 22.8, 6.9, FACING.N, 0.8, 0.8);
add('plant', 'plant-a1', 16.55, 7.45, 0, 0.5, 0.5, { size: 'large' });
add('plant', 'plant-a2', 23.45, 7.45, 0, 0.5, 0.5, { size: 'medium' });
add('rug', 'rug-abiyss', 20.0, 3.8, 0, 5.2, 4.4, { block: 'none', color: 0x2b2f45 });

// Área de descanso
add('sofa', 'sofa-a', 20.0, 15.35, FACING.N, 2.6, 0.9, { block: 'sofa', seats: 3 });
add('sofa', 'sofa-b', 23.4, 11.4, FACING.W, 1.8, 0.9, { block: 'sofa', seats: 2 });
add('coffeeTable', 'coffee-table', 20.0, 13.0, 0, 1.4, 0.7);
add('tvCabinet', 'lounge-tv', 20.0, 8.35, FACING.S, 2.0, 0.45);
add('beanbag', 'beanbag-1', 17.2, 9.7, 0, 0.8, 0.8, { block: 'none', color: 0xd9825b });
add('beanbag', 'beanbag-2', 18.6, 9.6, 0, 0.8, 0.8, { block: 'none', color: 0x6c8ebf });
add('floorLamp', 'floor-lamp', 23.45, 15.45, 0, 0.4, 0.4);
add('planter', 'planter-divider', 16.2, 14.55, 0, 0.4, 2.7);
add('plant', 'plant-l1', 23.45, 8.55, 0, 0.5, 0.5, { size: 'large' });
add('rug', 'rug-lounge', 20.0, 12.9, 0, 4.4, 3.2, { block: 'none', color: 0xb98b6e });

export const FURNITURE = Object.freeze(F.map((f) => Object.freeze(f)));

// ---------------------------------------------------------------------------
// Pontos de interesse (POI). `seat` = o ponto fica dentro de uma máquina de
// assento (só quem reservou o POI pode entrar). `locks` = outros POIs que
// ficam ocupados junto (deitar no sofá ocupa os três lugares).
// ---------------------------------------------------------------------------

const P = [];
const poi = (id, kind, x, z, facing, extra = {}) => P.push({ id, kind, x, z, facing, ...extra });

for (const pod of DESK_PODS) {
  const [west, east] = pod.desks;
  poi(`desk-${west}`, 'desk', pod.cx - 1.3, pod.cz, FACING.E, { seat: 0.5, pose: 'sit', desk: west, label: `Mesa ${west}` });
  poi(`desk-${east}`, 'desk', pod.cx + 1.3, pod.cz, FACING.W, { seat: 0.5, pose: 'sit', desk: east, label: `Mesa ${east}` });
  // Ponto "em pé ao lado da mesa" (ocioso perto do posto).
  poi(`desk-${west}-side`, 'deskSide', pod.cx - 2.2, pod.cz + 0.6, FACING.E, { desk: west, label: `Ao lado da mesa ${west}` });
  poi(`desk-${east}-side`, 'deskSide', pod.cx + 2.2, pod.cz + 0.6, FACING.W, { desk: east, label: `Ao lado da mesa ${east}` });
  // Ponto de inspeção do Abiyss (atrás do pod, olhando para as duas mesas).
  poi(`${pod.id}-inspect`, 'inspect', pod.cx, pod.cz - 1.9, FACING.S, { label: `Pod ${pod.id.slice(-1).toUpperCase()}` });
}

// Sofá A: três lugares voltados para o norte.
[19.15, 20.0, 20.85].forEach((x, i) =>
  poi(`sofa-a-${i + 1}`, 'sofa', x, 15.05, FACING.N, { seat: 0.6, pose: 'sit', group: 'sofa-a', label: 'Sofá' }));
poi('sofa-a-lie', 'sofaLie', 20.0, 15.05, FACING.N, {
  seat: 0.6, pose: 'lie', group: 'sofa-a', locks: ['sofa-a-1', 'sofa-a-2', 'sofa-a-3'], label: 'Sofá (deitado)',
});
// Sofá B: dois lugares voltados para o oeste.
[11.0, 11.8].forEach((z, i) =>
  poi(`sofa-b-${i + 1}`, 'sofa', 23.15, z, FACING.W, { seat: 0.6, pose: 'sit', group: 'sofa-b', label: 'Sofá pequeno' }));
poi('sofa-b-lie', 'sofaLie', 23.15, 11.4, FACING.W, {
  seat: 0.6, pose: 'lie', group: 'sofa-b', locks: ['sofa-b-1', 'sofa-b-2'], label: 'Sofá pequeno (deitado)',
});
poi('beanbag-1', 'beanbag', 17.2, 9.7, FACING.S, { seat: 0.8, pose: 'curl', label: 'Pufe' });
poi('beanbag-2', 'beanbag', 18.6, 9.6, FACING.S, { seat: 0.8, pose: 'curl', label: 'Pufe' });

poi('coffee', 'coffee', 1.4, 14.6, FACING.S, { label: 'Máquina de café' });
poi('water', 'water', 1.15, 4.7, FACING.W, { label: 'Bebedouro' });
poi('stool-1', 'stool', 2.4, 12.6, FACING.E, { seat: 0.4, pose: 'sit', group: 'kitchen-table', label: 'Banqueta' });
poi('stool-2', 'stool', 4.0, 12.6, FACING.W, { seat: 0.4, pose: 'sit', group: 'kitchen-table', label: 'Banqueta' });
poi('chat-1', 'chat', 7.25, 8.4, FACING.E, { group: 'stand-table', label: 'Mesa alta' });
poi('chat-2', 'chat', 8.75, 8.4, FACING.W, { group: 'stand-table', label: 'Mesa alta' });
poi('board', 'board', 13.4, 6.3, FACING.N, { label: 'Quadro de goals' });
poi('printer', 'printer', 14.6, 1.35, FACING.N, { label: 'Impressora' });
poi('shelf', 'shelf', 12.9, 1.05, FACING.N, { label: 'Estante' });
poi('win-n1', 'window', 2.2, 0.75, FACING.N, { label: 'Janela norte' });
poi('win-n3', 'window', 10.4, 0.75, FACING.N, { label: 'Janela norte' });
poi('win-w2', 'window', 0.75, 9.4, FACING.W, { label: 'Janela oeste' });
poi('win-lounge', 'window', 23.25, 13.9, FACING.E, { label: 'Janela da área de descanso' });

poi('center', 'center', 8.0, 6.9, FACING.S, { label: 'Área central' });
poi('entrance', 'entrance', 11.2, 14.6, FACING.N, { label: 'Entrada' });
poi('outside', 'outside', 11.2, 18.2, FACING.N, { label: 'Calçada' });

// Sala do Abiyss
poi('abiyss-home', 'abiyssHome', 20.0, 1.9, FACING.S, { label: 'Console do Abiyss' });
poi('report', 'report', 20.0, 4.5, FACING.N, { label: 'Entrega de relatório' });
poi('report-wait', 'reportWait', 17.8, 6.4, FACING.E, { label: 'Fila do relatório' });
poi('abiyss-window', 'abiyssWindow', 23.2, 5.5, FACING.E, { label: 'Janela do Abiyss' });
poi('abiyss-door', 'waypoint', 17.0, 5.0, FACING.W, { label: 'Porta (dentro)' });

// Pontos de passeio (andar sem objetivo rígido).
[[2.4, 9.0], [12.0, 9.2], [7.9, 1.4], [14.0, 12.8], [8.0, 11.6], [5.4, 6.4], [18.4, 11.6], [9.8, 13.4]]
  .forEach(([x, z], i) => poi(`wander-${i + 1}`, 'wander', x, z, FACING.S, { label: 'Passeio' }));

export const POIS = Object.freeze(P.map((p) => Object.freeze(p)));
export const POI_BY_ID = Object.freeze(Object.fromEntries(POIS.map((p) => [p.id, p])));

// ---------------------------------------------------------------------------
// Entidades iniciais.
// ---------------------------------------------------------------------------

export const IMMO_DEFS = Object.freeze([
  { id: 'immo-1', name: 'IMMo-1', desk: 'desk-1', color: 0xff8a5c },
  { id: 'immo-2', name: 'IMMo-2', desk: 'desk-2', color: 0xffc94d },
  { id: 'immo-3', name: 'IMMo-3', desk: 'desk-3', color: 0xe86fa8 },
  { id: 'immo-4', name: 'IMMo-4', desk: 'desk-4', color: 0x9fb4d0 },
]);

export const ABIYSS_DEF = Object.freeze({ id: 'abiyss', name: 'Abiyss', home: 'abiyss-home' });

// ---------------------------------------------------------------------------
// Utilidades geométricas compartilhadas.
// ---------------------------------------------------------------------------

/** Retângulo alinhado aos eixos (mundo) de um item com rotação múltipla de 90°. */
export function worldRect(item) {
  const quarter = Math.round(item.rot / (PI / 2));
  const swap = Math.abs(quarter) % 2 === 1;
  const w = swap ? item.d : item.w;
  const d = swap ? item.w : item.d;
  return { x0: item.x - w / 2, z0: item.z - d / 2, x1: item.x + w / 2, z1: item.z + d / 2 };
}

/** Converte um retângulo LOCAL (relativo ao centro do item) para o mundo. */
export function localRectToWorld(item, lx, lz, lw, ld) {
  const s = Math.round(Math.sin(item.rot));
  const c = Math.round(Math.cos(item.rot));
  // Local +x → mundo (cos θ, −sin θ); local +z → mundo (sin θ, cos θ).
  const cx = item.x + lx * c + lz * s;
  const cz = item.z - lx * s + lz * c;
  return worldRect({ x: cx, z: cz, w: lw, d: ld, rot: item.rot });
}

/**
 * Retângulos que bloqueiam a navegação para um móvel.
 * Sofás: encosto (fundo) e braços; os assentos ficam livres para as máscaras.
 */
export function blockingRects(item) {
  if (item.block === 'none') return [];
  if (item.block === 'sofa') {
    const back = 0.28;
    const arm = 0.18;
    return [
      localRectToWorld(item, 0, -item.d / 2 + back / 2, item.w, back),
      localRectToWorld(item, -item.w / 2 + arm / 2, 0, arm, item.d),
      localRectToWorld(item, item.w / 2 - arm / 2, 0, arm, item.d),
    ];
  }
  return [worldRect(item)];
}

/** Segmentos de parede sólidos (sem aberturas de porta). Janelas bloqueiam. */
export function solidWallSpans(wall) {
  const len = Math.hypot(wall.x2 - wall.x1, wall.z2 - wall.z1);
  const doors = wall.openings
    .filter((o) => o.kind === 'door')
    .map((o) => [o.at - o.width / 2, o.at + o.width / 2])
    .sort((a, b) => a[0] - b[0]);
  const spans = [];
  let cursor = 0;
  for (const [a, b] of doors) {
    if (a > cursor) spans.push([cursor, a]);
    cursor = Math.max(cursor, b);
  }
  if (cursor < len) spans.push([cursor, len]);
  return spans;
}

/** Ponto ao longo de uma parede. */
export function wallPoint(wall, t) {
  const len = Math.hypot(wall.x2 - wall.x1, wall.z2 - wall.z1);
  const k = t / len;
  return [wall.x1 + (wall.x2 - wall.x1) * k, wall.z1 + (wall.z2 - wall.z1) * k];
}
