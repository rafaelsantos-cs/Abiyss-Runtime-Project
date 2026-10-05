// Efeitos de curta duração vindos do servidor (lista `fx` do quadro):
//   orb    — delegação: uma esfera de luz voa do Abiyss até o IMMo
//   report — relatório: uma folha voa do IMMo até o Abiyss
//   pulse  — heartbeat: anel que se expande no chão (desenhado pelo Abiyss)
//   chime  — cron disparou: anel dourado acima do Abiyss
// E as linhas de caminho/destino do modo debug.

import * as THREE from 'three';

export class FxLayer {
  constructor(scene) {
    this.scene = scene;
    this.active = new Map();
    this.orbGeo = new THREE.IcosahedronGeometry(0.12, 1);
    this.paperGeo = new THREE.BoxGeometry(0.22, 0.01, 0.3);
    this.ringGeo = new THREE.RingGeometry(0.45, 0.52, 36);
  }

  #create(f) {
    let obj;
    if (f.type === 'orb') {
      obj = new THREE.Mesh(this.orbGeo, new THREE.MeshBasicMaterial({ color: f.color }));
      const glow = new THREE.Mesh(this.orbGeo, new THREE.MeshBasicMaterial({ color: f.color, transparent: true, opacity: 0.35, blending: THREE.AdditiveBlending, depthWrite: false }));
      glow.scale.setScalar(2.2);
      obj.add(glow);
    } else if (f.type === 'report') {
      obj = new THREE.Mesh(this.paperGeo, new THREE.MeshBasicMaterial({ color: 0xfffdf2 }));
      const tag = new THREE.Mesh(new THREE.BoxGeometry(0.06, 0.012, 0.06), new THREE.MeshBasicMaterial({ color: f.color }));
      tag.position.set(0.06, 0.006, -0.1);
      obj.add(tag);
    } else if (f.type === 'chime') {
      obj = new THREE.Mesh(this.ringGeo, new THREE.MeshBasicMaterial({ color: f.color, transparent: true, side: THREE.DoubleSide, depthWrite: false, blending: THREE.AdditiveBlending }));
    } else {
      return null;
    }
    this.scene.add(obj);
    return obj;
  }

  update(sample, positionOf) {
    if (!sample) return;
    const seen = new Set();
    for (const f of sample.fx ?? []) {
      seen.add(f.id);
      let obj = this.active.get(f.id);
      if (obj === undefined) {
        obj = this.#create(f);
        this.active.set(f.id, obj);
      }
      if (!obj) continue;
      const p = Math.min(1, Math.max(0, (sample.simMs - f.startSim) / f.durationMs));
      const from = positionOf(f.from);
      const to = f.to ? positionOf(f.to) : null;
      if (!from) continue;
      if (f.type === 'orb' || f.type === 'report') {
        if (!to) continue;
        const fromY = f.type === 'orb' ? 1.5 : 1.0;
        const toY = f.type === 'orb' ? 1.3 : 1.4;
        obj.position.set(
          from.x + (to.x - from.x) * p,
          fromY + (toY - fromY) * p + Math.sin(Math.PI * p) * 1.4,
          from.z + (to.z - from.z) * p,
        );
        obj.rotation.y += 0.2;
        obj.visible = p < 1;
      } else if (f.type === 'chime') {
        obj.position.set(from.x, 2.9, from.z);
        obj.rotation.x = -Math.PI / 2;
        obj.scale.setScalar(1 + p * 2.5);
        obj.material.opacity = 1 - p;
      }
    }
    for (const [id, obj] of this.active) {
      if (seen.has(id)) continue;
      if (obj) {
        obj.removeFromParent();
        obj.traverse((o) => o.material?.dispose?.());
      }
      this.active.delete(id);
    }
  }
}

/** Linhas de caminho e marcador de destino (modo debug). */
export class PathOverlay {
  constructor(scene) {
    this.scene = scene;
    this.lines = new Map();
    this.visible = false;
    this.group = new THREE.Group();
    this.group.visible = false;
    scene.add(this.group);
  }

  setVisible(v) {
    this.visible = v;
    this.group.visible = v;
  }

  update(sample, colorOf) {
    if (!this.visible || !sample) return;
    for (const [id, e] of sample.entities) {
      let rec = this.lines.get(id);
      if (!rec) {
        const geo = new THREE.BufferGeometry();
        geo.setAttribute('position', new THREE.BufferAttribute(new Float32Array(64 * 3), 3));
        const line = new THREE.Line(geo, new THREE.LineBasicMaterial({ color: colorOf(e), transparent: true, opacity: 0.9, depthTest: false }));
        line.renderOrder = 5;
        const marker = new THREE.Mesh(new THREE.RingGeometry(0.18, 0.26, 20), new THREE.MeshBasicMaterial({ color: colorOf(e), side: THREE.DoubleSide, depthTest: false }));
        marker.rotation.x = -Math.PI / 2;
        marker.renderOrder = 5;
        this.group.add(line, marker);
        rec = { line, marker };
        this.lines.set(id, rec);
      }
      const path = e.path;
      const arr = rec.line.geometry.attributes.position.array;
      if (path && path.length > 1 && e.present) {
        const n = Math.min(64, path.length);
        arr[0] = e.x; arr[1] = 0.06; arr[2] = e.z;
        for (let i = 1; i < n; i++) {
          arr[i * 3] = path[i][0];
          arr[i * 3 + 1] = 0.06;
          arr[i * 3 + 2] = path[i][1];
        }
        rec.line.geometry.setDrawRange(0, n);
        rec.line.geometry.attributes.position.needsUpdate = true;
        rec.line.visible = true;
        rec.marker.visible = true;
        rec.marker.position.set(path[n - 1][0], 0.06, path[n - 1][1]);
      } else {
        rec.line.visible = false;
        rec.marker.visible = false;
      }
    }
  }
}
