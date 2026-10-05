// Renderizador, luzes e céu. O ambiente (sol, clima, lâmpadas) vem do
// servidor; aqui ele é só aplicado.

import * as THREE from 'three';
import { BUILDING } from '../../shared/layout.js';

export const CENTER = new THREE.Vector3(BUILDING.width / 2, 0, BUILDING.depth / 2);

export function createRenderer(container, quality) {
  const renderer = new THREE.WebGLRenderer({ antialias: quality !== 'low', powerPreference: 'high-performance', preserveDrawingBuffer: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, quality === 'low' ? 1 : 2));
  renderer.setSize(container.clientWidth, container.clientHeight);
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFShadowMap;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.05;
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  container.appendChild(renderer.domElement);
  return renderer;
}

function skyDome() {
  const geo = new THREE.SphereGeometry(190, 24, 16);
  const colors = new Float32Array(geo.attributes.position.count * 3);
  geo.setAttribute('color', new THREE.BufferAttribute(colors, 3));
  const m = new THREE.MeshBasicMaterial({ vertexColors: true, side: THREE.BackSide, fog: false, depthWrite: false });
  const dome = new THREE.Mesh(geo, m);
  dome.position.copy(CENTER);
  dome.renderOrder = -10;
  return dome;
}

function stars() {
  const n = 600;
  const pos = new Float32Array(n * 3);
  let s = 7;
  const rnd = () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 4294967296;
  };
  for (let i = 0; i < n; i++) {
    const a = rnd() * Math.PI * 2;
    const el = 0.12 + rnd() * 1.35;
    pos[i * 3] = CENTER.x + Math.cos(a) * Math.cos(el) * 170;
    pos[i * 3 + 1] = Math.sin(el) * 170;
    pos[i * 3 + 2] = CENTER.z + Math.sin(a) * Math.cos(el) * 170;
  }
  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.BufferAttribute(pos, 3));
  const m = new THREE.PointsMaterial({ color: 0xffffff, size: 1.4, sizeAttenuation: false, transparent: true, opacity: 0, fog: false, depthWrite: false });
  return new THREE.Points(geo, m);
}

export class OfficeScene {
  constructor(container, quality) {
    this.quality = quality;
    this.renderer = createRenderer(container, quality);
    this.scene = new THREE.Scene();
    this.scene.background = new THREE.Color(0x9fc6e8);
    this.scene.fog = new THREE.FogExp2(0xbfe2ff, 0.004);
    this.camera = new THREE.PerspectiveCamera(36, container.clientWidth / container.clientHeight, 0.2, 500);

    this.hemi = new THREE.HemisphereLight(0xcfe4ff, 0x6b6050, 0.9);
    this.scene.add(this.hemi);
    this.sun = new THREE.DirectionalLight(0xfff3df, 2.2);
    this.sun.castShadow = true;
    const size = quality === 'low' ? 1024 : 2048;
    this.sun.shadow.mapSize.set(size, size);
    const sc = this.sun.shadow.camera;
    sc.left = -19; sc.right = 19; sc.top = 19; sc.bottom = -19; sc.near = 1; sc.far = 120;
    this.sun.shadow.bias = -0.0004;
    this.sun.shadow.normalBias = 0.035;
    this.sun.target.position.copy(CENTER);
    this.scene.add(this.sun, this.sun.target);

    this.sky = skyDome();
    this.scene.add(this.sky);
    this.stars = stars();
    this.scene.add(this.stars);
    this.sunDisc = new THREE.Mesh(new THREE.CircleGeometry(5, 20), new THREE.MeshBasicMaterial({ color: 0xfff4d6, fog: false, transparent: true }));
    this.scene.add(this.sunDisc);

    this.envKey = null;
    this.flash = 0;
    window.addEventListener('resize', () => this.resize(container));
  }

  resize(container) {
    this.camera.aspect = container.clientWidth / container.clientHeight;
    this.camera.updateProjectionMatrix();
    this.renderer.setSize(container.clientWidth, container.clientHeight);
  }

  #paintSky(top, horizon) {
    const geo = this.sky.geometry;
    const pos = geo.attributes.position;
    const col = geo.attributes.color;
    const cTop = new THREE.Color(top);
    const cHor = new THREE.Color(horizon);
    const tmp = new THREE.Color();
    for (let i = 0; i < pos.count; i++) {
      const y = pos.getY(i) / 190;
      const t = Math.pow(Math.max(0, y), 0.55);
      tmp.copy(cHor).lerp(cTop, t);
      col.setXYZ(i, tmp.r, tmp.g, tmp.b);
    }
    col.needsUpdate = true;
  }

  /**
   * Aplica o ambiente autoritativo.
   * @param {object} env  meta.env vindo do servidor
   * @param {object} dyn  peças dinâmicas do escritório
   */
  applyEnvironment(env, dyn) {
    if (!env) return;
    const L = env.light;
    const key = `${L.skyTop}|${L.skyHorizon}`;
    if (key !== this.envKey) {
      this.#paintSky(L.skyTop, L.skyHorizon);
      this.envKey = key;
    }
    this.scene.background.setHex(L.skyHorizon);
    this.scene.fog.color.setHex(L.fogColor);
    this.scene.fog.density = L.fogDensity;

    // Sol (azimute a partir do norte, horário; norte = −z, leste = +x).
    const el = THREE.MathUtils.degToRad(env.sun.elevation);
    const az = THREE.MathUtils.degToRad(env.sun.azimuth);
    const dir = new THREE.Vector3(Math.sin(az) * Math.cos(el), Math.sin(el), -Math.cos(az) * Math.cos(el));
    if (env.sun.elevation > -2) {
      this.sun.position.copy(CENTER).addScaledVector(dir, 60);
      this.sun.color.setHex(L.sunColor);
      this.sun.intensity = L.sunIntensity;
    } else {
      // Noite: luar fraco e frio, de cima, para manter alguma leitura de volume.
      this.sun.position.set(CENTER.x - 20, 45, CENTER.z - 15);
      this.sun.color.setHex(0x9fb4ff);
      this.sun.intensity = 0.28 * (1 - Math.min(1, (env.weather?.cloudCover ?? 0) / 130));
    }
    this.hemi.intensity = L.ambient + this.flash;
    this.hemi.color.setHex(L.ambientSky);
    this.hemi.groundColor.setHex(L.ambientGround);

    this.sunDisc.visible = env.sun.elevation > -3 && (env.weather?.cloudCover ?? 0) < 85;
    this.sunDisc.position.copy(CENTER).addScaledVector(dir, 175);
    this.sunDisc.lookAt(CENTER);
    this.sunDisc.material.color.setHex(L.sunColor);
    this.stars.material.opacity = Math.max(0, 1 - L.day * 1.6) * (1 - Math.min(1, (env.weather?.cloudCover ?? 0) / 100));

    // Lâmpadas internas, luminárias e postes.
    const lamp = L.lampLevel;
    for (const l of dyn.nightLights) l.intensity = lamp * 7;
    for (const m of dyn.lamps) {
      m.material.emissive.setHex(lamp > 0 ? 0xffd08a : 0x000000);
      m.material.emissiveIntensity = 0.9 * lamp;
    }
    for (const m of dyn.streetLamps) {
      m.material.emissive.setHex(lamp > 0 ? 0xffe2a8 : 0x000000);
      m.material.emissiveIntensity = lamp;
    }
  }

  render() {
    this.renderer.render(this.scene, this.camera);
  }
}
