// Câmera: órbita livre (girar, aproximar, arrastar), visões prontas com
// transição suave, seguir uma entidade e "corte" das paredes externas do
// lado da câmera (para ver o interior, como em jogos de gerenciamento).

import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';

/** Visões: alvo + direção (azimute a partir de +z, elevação) + distância. */
export const PRESETS = {
  overview: { target: [12, 0, 9.4], az: 0.5, el: 0.95, dist: 34 },
  abiyss: { target: [20, 0.9, 3.6], az: 0.35, el: 0.62, dist: 11.5 },
  desks: { target: [7.6, 0.6, 3.6], az: 0.25, el: 0.72, dist: 12.5 },
  lounge: { target: [20, 0.5, 12.4], az: -0.45, el: 0.72, dist: 12 },
  kitchen: { target: [4, 0.5, 13], az: 0.6, el: 0.7, dist: 10 },
  top: { target: [12, 0, 8.2], az: 0.0001, el: 1.52, dist: 34 },
};

function offsetFrom(p) {
  return new THREE.Vector3(
    Math.sin(p.az) * Math.cos(p.el) * p.dist,
    Math.sin(p.el) * p.dist,
    Math.cos(p.az) * Math.cos(p.el) * p.dist,
  );
}

export class CameraController {
  constructor(camera, dom) {
    this.camera = camera;
    this.controls = new OrbitControls(camera, dom);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.08;
    this.controls.minDistance = 3;
    this.controls.maxDistance = 70;
    this.controls.maxPolarAngle = 1.48;
    this.controls.screenSpacePanning = false;
    this.tween = null;
    this.followId = null;
    this.preset('overview', true);
  }

  preset(name, instant = false) {
    const p = PRESETS[name];
    if (!p) return;
    this.followId = null;
    const target = new THREE.Vector3(...p.target);
    const pos = target.clone().add(offsetFrom(p));
    this.#go(target, pos, instant);
  }

  /** Câmera livre por parâmetros (usada pelos scripts de screenshot). */
  orbit({ target, az, el, dist }, instant = true) {
    this.followId = null;
    const t = new THREE.Vector3(...target);
    this.#go(t, t.clone().add(offsetFrom({ az, el, dist })), instant);
  }

  #go(target, pos, instant) {
    if (instant) {
      this.controls.target.copy(target);
      this.camera.position.copy(pos);
      this.tween = null;
      this.controls.update();
      return;
    }
    this.tween = {
      t: 0, fromT: this.controls.target.clone(), fromP: this.camera.position.clone(), toT: target, toP: pos,
    };
  }

  follow(id, getPos) {
    this.followId = id;
    this.getPos = getPos;
    const p = getPos(id);
    if (!p) return;
    const target = new THREE.Vector3(p.x, 0.7, p.z);
    const pos = target.clone().add(offsetFrom({ az: 0.5, el: 0.7, dist: 8 }));
    this.#go(target, pos, false);
  }

  update(dt) {
    if (this.tween) {
      const tw = this.tween;
      tw.t = Math.min(1, tw.t + dt / 1.1);
      const k = tw.t * tw.t * (3 - 2 * tw.t);
      this.controls.target.lerpVectors(tw.fromT, tw.toT, k);
      this.camera.position.lerpVectors(tw.fromP, tw.toP, k);
      if (tw.t >= 1) this.tween = null;
    } else if (this.followId && this.getPos) {
      const p = this.getPos(this.followId);
      if (p) {
        const desired = new THREE.Vector3(p.x, 0.7, p.z);
        // Suavização independente do FPS (a câmera não fica para trás em máquinas lentas).
        const delta = desired.sub(this.controls.target).multiplyScalar(1 - Math.exp(-dt * 4));
        this.controls.target.add(delta);
        this.camera.position.add(delta);
      }
    }
    this.controls.update();
  }

  /**
   * Corta (abaixa) as paredes externas que ficam entre a câmera e o
   * interior. Mantém um "rodapé" de 35 cm para a planta continuar legível.
   */
  updateCutaway(walls, dt) {
    const cam = this.camera.position;
    for (const w of walls) {
      const dx = cam.x - w.center.x;
      const dz = cam.z - w.center.z;
      const outside = dx * w.outward[0] + dz * w.outward[1] > 0.5;
      const target = outside ? 1 : 0;
      w.cut += (target - w.cut) * Math.min(1, dt * 6);
      w.group.scale.y = 1 - w.cut * 0.87;
    }
  }
}
