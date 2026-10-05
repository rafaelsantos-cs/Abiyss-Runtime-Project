// Abiyss Office — cliente 3D.
//
// O cliente só OBSERVA: recebe do servidor o estado autoritativo (SSE),
// interpola e desenha. Opções pela URL:
//   ?q=low        qualidade baixa (sombras menores, sem antialias)
//   ?debug=1      abre o painel de debug
//   ?labels=0|1|2 rótulos: 0 nenhum, 1 nomes, 2 nomes + estado
//   ?view=NOME    visão inicial (overview, abiyss, desks, lounge, top)

import { SyncClient } from './net/sync-client.js';
import { OfficeScene } from './render/scene.js';
import { buildOffice } from './render/office-builder.js';
import { EntityViews, immoStateColor } from './render/characters/views.js';
import { CameraController, PRESETS } from './render/camera.js';
import { Displays } from './render/displays.js';
import { FxLayer, PathOverlay } from './render/fx.js';
import { WeatherFx } from './render/weather-fx.js';
import { Hud, renderLegend } from './ui/hud.js';
import { DebugPanel } from './ui/debug.js';

const params = new URLSearchParams(location.search);
const quality = params.get('q') === 'low' ? 'low' : 'high';

const container = document.getElementById('viewport');
const office = new OfficeScene(container, quality);
const { dyn } = buildOffice(office.scene);
const views = new EntityViews(office.scene);
const cam = new CameraController(office.camera, office.renderer.domElement);
const displays = new Displays(dyn);
const fx = new FxLayer(office.scene);
const paths = new PathOverlay(office.scene);
const weatherFx = new WeatherFx(office.scene);
const hud = new Hud();
const debug = new DebugPanel(document.getElementById('debug'));
const sync = new SyncClient();

let labels = params.has('labels') ? Number(params.get('labels')) : 2;
let lastSample = null;
const perf = { fps: 0, frameMs: 0, calls: 0, triangles: 0 };
let frames = 0;
let fpsT0 = performance.now();
let followIdx = -1;

// --- conexão -----------------------------------------------------------------
sync.addEventListener('status', (e) => hud.updateConnection(e.detail));
sync.addEventListener('meta', (e) => {
  hud.updateMeta(e.detail, sync.hello);
  office.applyEnvironment(e.detail.env, dyn);
  weatherFx.apply(e.detail.env?.weather);
  displays.updateMeta(e.detail, lastSample);
});
sync.addEventListener('first-frame', () => {
  document.getElementById('loading').classList.add('done');
  hud.show();
  document.getElementById('controls').hidden = false;
  window.__office.ready = true;
});
sync.connect();
setTimeout(() => {
  if (!window.__office.ready) document.getElementById('loading-text').textContent = 'sem resposta do servidor… tentando de novo';
}, 6000);

// --- controles ---------------------------------------------------------------------
const setLabels = (v) => {
  labels = v;
  document.getElementById('btn-labels').classList.toggle('on', labels > 0);
};
const toggleDebug = (v) => {
  debug.toggle(v);
  paths.setVisible(debug.visible);
  document.getElementById('btn-debug').classList.toggle('on', debug.visible);
};
const toggleLegend = (v) => {
  const el = document.getElementById('legend');
  el.hidden = v === undefined ? !el.hidden : !v;
  if (!el.hidden) renderLegend(el);
  document.getElementById('btn-legend').classList.toggle('on', !el.hidden);
};
const followNext = () => {
  const ids = ['abiyss', 'immo-1', 'immo-2', 'immo-3', 'immo-4'];
  followIdx = (followIdx + 1) % ids.length;
  cam.follow(ids[followIdx], (id) => views.positionOf(id));
  document.getElementById('btn-follow').classList.add('on');
};
const stopFollow = () => {
  cam.followId = null;
  followIdx = -1;
  document.getElementById('btn-follow').classList.remove('on');
};

document.querySelectorAll('[data-preset]').forEach((b) => b.addEventListener('click', () => {
  stopFollow();
  cam.preset(b.dataset.preset);
}));
document.getElementById('btn-follow').addEventListener('click', followNext);
document.getElementById('btn-labels').addEventListener('click', () => setLabels(labels === 0 ? 2 : labels === 2 ? 1 : 0));
document.getElementById('btn-debug').addEventListener('click', () => toggleDebug());
document.getElementById('btn-legend').addEventListener('click', () => toggleLegend());
window.addEventListener('keydown', (e) => {
  if (e.target instanceof HTMLInputElement) return;
  const keys = { 1: 'overview', 2: 'abiyss', 3: 'desks', 4: 'lounge', 5: 'top' };
  if (keys[e.key]) { stopFollow(); cam.preset(keys[e.key]); }
  else if (e.key === 'f' || e.key === 'F') followNext();
  else if (e.key === 'Escape') stopFollow();
  else if (e.key === 'l' || e.key === 'L') setLabels(labels === 0 ? 2 : labels === 2 ? 1 : 0);
  else if (e.key === 'd' || e.key === 'D') toggleDebug();
  else if (e.key === 'p' || e.key === 'P') paths.setVisible(!paths.visible);
  else if (e.key === '?') toggleLegend();
});
if (params.get('debug') === '1') toggleDebug(true);
if (params.get('view') && PRESETS[params.get('view')]) cam.preset(params.get('view'), true);

// --- laço de renderização -------------------------------------------------------------
let last = performance.now();
const clockStart = performance.now();
function frame(now) {
  const rawDt = Math.min(1, (now - last) / 1000);
  const dtMs = Math.min(100, now - last);
  last = now;
  const dt = dtMs / 1000;
  const t = (now - clockStart) / 1000;
  const sample = sync.sample(dtMs);
  if (sample) lastSample = sample;
  const labelMode = labels === 0 ? false : labels === 1 ? 'name' : 'full';
  views.update(lastSample, dt, t, labelMode, sync.meta?.runtime, office.camera);
  fx.update(lastSample, (id) => views.positionOf(id));
  paths.update(lastSample, (e) => (e.kind === 'abiyss' ? 0x48e5ff : immoStateColor(e)));
  displays.updateFrame(lastSample, sync.meta, t);
  const flash = weatherFx.update(dt);
  office.hemi.intensity = (sync.meta?.env?.light?.ambient ?? office.hemi.intensity) + flash;
  cam.update(rawDt);
  cam.updateCutaway(dyn.exteriorWalls, dt);
  if (lastSample) hud.updateClock(lastSample.clock);
  hud.updateFocus(cam.followId ? lastSample?.entities.get(cam.followId) : null);
  const r0 = performance.now();
  office.render();
  perf.frameMs = perf.frameMs * 0.9 + (performance.now() - r0) * 0.1;
  perf.calls = office.renderer.info.render.calls;
  perf.triangles = office.renderer.info.render.triangles;
  frames++;
  if (now - fpsT0 > 1000) {
    perf.fps = (frames * 1000) / (now - fpsT0);
    frames = 0;
    fpsT0 = now;
  }
  debug.update({ sample: lastSample, meta: sync.meta, sync, perf, hello: sync.hello });
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);

// --- interface para automação (screenshots e testes) ----------------------------------------
window.__office = {
  ready: false,
  sync,
  perf,
  preset: (name, instant = true) => { stopFollow(); cam.preset(name, instant); },
  orbit: (o) => { stopFollow(); cam.orbit(o, true); },
  follow: (id) => cam.follow(id, (x) => views.positionOf(x)),
  labels: setLabels,
  debug: toggleDebug,
  legend: toggleLegend,
  paths: (v) => paths.setVisible(v),
  /** Resumo serializável do estado mais recente. */
  state: () => {
    const s = lastSample;
    if (!s) return null;
    return {
      simMs: s.simMs,
      clock: s.clock,
      entities: [...s.entities.values()].map((e) => ({
        id: e.id, kind: e.kind, x: e.x, z: e.z, act: e.act, pose: e.pose, poi: e.poi, present: e.present,
        mode: e.mode, mind: e.mind?.kind ?? null, logical: e.logical, intent: e.intent,
      })),
      meta: sync.meta ? { runtime: sync.meta.runtime, stats: sync.meta.stats, env: sync.meta.env } : null,
    };
  },
};
