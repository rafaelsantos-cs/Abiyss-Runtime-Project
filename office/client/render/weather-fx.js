// Efeitos de clima: chuva (só FORA do prédio — não há teto, então a chuva
// não pode cair dentro do escritório), relâmpagos, nuvens e chão molhado.

import * as THREE from 'three';
import { BUILDING } from '../../shared/layout.js';

const AREA = { x0: -16, x1: 40, z0: -14, z1: 32, top: 7 };
const MARGIN = 2.2; // a chuva não cai perto do prédio (sem teto, ela pareceria cair dentro)
const DROPS = 1400;

export class WeatherFx {
  constructor(scene) {
    this.scene = scene;
    // Chuva: segmentos de reta.
    const pos = new Float32Array(DROPS * 6);
    this.drops = [];
    let s = 99;
    const rnd = () => {
      s = (s * 1664525 + 1013904223) >>> 0;
      return s / 4294967296;
    };
    this.rnd = rnd;
    for (let i = 0; i < DROPS; i++) {
      let x;
      let z;
      do {
        x = AREA.x0 + rnd() * (AREA.x1 - AREA.x0);
        z = AREA.z0 + rnd() * (AREA.z1 - AREA.z0);
      } while (x > -MARGIN && x < BUILDING.width + MARGIN && z > -MARGIN && z < BUILDING.depth + MARGIN);
      this.drops.push({ x, z, y: rnd() * AREA.top, v: 8 + rnd() * 4 });
    }
    const geo = new THREE.BufferGeometry();
    geo.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    this.rain = new THREE.LineSegments(geo, new THREE.LineBasicMaterial({ color: 0xaec4dc, transparent: true, opacity: 0.55 }));
    this.rain.frustumCulled = false;
    this.rain.visible = false;
    scene.add(this.rain);

    // Nuvens: blocos low-poly lá no alto.
    this.clouds = [];
    const cloudMat = new THREE.MeshLambertMaterial({ color: 0xffffff, transparent: true, opacity: 0.85, flatShading: true });
    this.cloudMat = cloudMat;
    for (let i = 0; i < 12; i++) {
      const g = new THREE.Group();
      const n = 3 + Math.floor(rnd() * 3);
      for (let k = 0; k < n; k++) {
        const m = new THREE.Mesh(new THREE.IcosahedronGeometry(3 + rnd() * 3, 0), cloudMat);
        m.position.set(k * 3.5 - n * 1.6, rnd() * 1.5, rnd() * 2.5);
        m.scale.y = 0.55;
        g.add(m);
      }
      g.position.set(-60 + rnd() * 140, 34 + rnd() * 10, -50 + rnd() * 110);
      g.userData.speed = 0.6 + rnd() * 0.8;
      scene.add(g);
      this.clouds.push(g);
    }
    this.intensity = 0;
    this.thunder = false;
    this.flash = 0;
    this.nextFlash = 3;
    this.wind = { x: 0.6, z: 0.2 };
  }

  apply(weather) {
    if (!weather) return;
    this.intensity = weather.rain ?? 0;
    this.thunder = !!weather.thunder;
    // Vento: direção de onde vem (graus, meteorológico) → para onde sopra.
    const a = THREE.MathUtils.degToRad((weather.windDirDeg ?? 90) + 180);
    const k = Math.min(1.5, (weather.windKmh ?? 8) / 20);
    this.wind = { x: Math.sin(a) * k, z: -Math.cos(a) * k };
    const cover = (weather.cloudCover ?? 30) / 100;
    const visibleClouds = Math.round(cover * this.clouds.length);
    this.clouds.forEach((c, i) => { c.visible = i < visibleClouds; });
    const dark = weather.condition === 'storm' ? 0.45 : weather.condition === 'rain' ? 0.65 : weather.condition === 'overcast' ? 0.8 : 1;
    this.cloudMat.color.setScalar(dark);
  }

  /** @returns {number} brilho extra do relâmpago (0..1) */
  update(dt) {
    // Chuva.
    const n = Math.round(DROPS * Math.min(1, this.intensity));
    this.rain.visible = n > 0;
    if (n > 0) {
      const arr = this.rain.geometry.attributes.position.array;
      const len = 0.35 + this.intensity * 0.35;
      for (let i = 0; i < DROPS; i++) {
        const d = this.drops[i];
        if (i >= n) {
          arr.fill(0, i * 6, i * 6 + 6);
          continue;
        }
        d.y -= d.v * dt;
        d.x += this.wind.x * dt * 2;
        d.z += this.wind.z * dt * 2;
        if (d.y < 0) {
          d.y = AREA.top;
          d.x += (this.rnd() - 0.5) * 2;
          d.z += (this.rnd() - 0.5) * 2;
        }
        // Mantém fora do prédio e dentro da área.
        if (d.x > -MARGIN && d.x < BUILDING.width + MARGIN && d.z > -MARGIN && d.z < BUILDING.depth + MARGIN) d.x = d.x < BUILDING.width / 2 ? -MARGIN - 0.5 : BUILDING.width + MARGIN + 0.5;
        if (d.x < AREA.x0) d.x = AREA.x1;
        if (d.x > AREA.x1) d.x = AREA.x0;
        if (d.z < AREA.z0) d.z = AREA.z1;
        if (d.z > AREA.z1) d.z = AREA.z0;
        arr[i * 6] = d.x;
        arr[i * 6 + 1] = d.y;
        arr[i * 6 + 2] = d.z;
        arr[i * 6 + 3] = d.x - this.wind.x * len * 0.3;
        arr[i * 6 + 4] = d.y + len;
        arr[i * 6 + 5] = d.z - this.wind.z * len * 0.3;
      }
      this.rain.geometry.attributes.position.needsUpdate = true;
    }
    // Nuvens andam com o vento.
    for (const c of this.clouds) {
      c.position.x += (this.wind.x + 0.2) * c.userData.speed * dt;
      c.position.z += this.wind.z * c.userData.speed * dt;
      if (c.position.x > 90) c.position.x = -70;
      if (c.position.x < -70) c.position.x = 90;
      if (c.position.z > 70) c.position.z = -50;
      if (c.position.z < -50) c.position.z = 70;
    }
    // Relâmpagos.
    if (this.thunder) {
      this.nextFlash -= dt;
      if (this.nextFlash <= 0) {
        this.flash = 1;
        this.nextFlash = 4 + this.rnd() * 9;
      }
    }
    this.flash = Math.max(0, this.flash - dt * 5);
    return this.flash > 0.5 || (this.flash > 0.2 && this.flash < 0.35) ? this.flash * 1.6 : 0;
  }
}
