// Modelo do Abiyss: a inteligência central do escritório.
//
// Não é humano: é um núcleo de obsidiana facetado que flutua, com um "olho"
// luminoso (indica para onde está olhando), dois anéis orbitais (giram mais
// rápido quando ele pensa), satélites (= eventos pendentes na fila do
// runtime) e um halo no chão (marca a posição e pulsa a cada heartbeat).

import * as THREE from 'three';
import { PALETTE as P } from '../materials.js';

export function createAbiyssModel() {
  const root = new THREE.Group();
  root.name = 'abiyss';
  const floating = new THREE.Group();
  floating.position.y = 1.45;
  floating.scale.setScalar(1.18);
  root.add(floating);

  const coreMat = new THREE.MeshStandardMaterial({ color: 0x161a2b, metalness: 0.55, roughness: 0.32, flatShading: true, emissive: 0x0a1630, emissiveIntensity: 0.6 });
  const coreGeo = new THREE.OctahedronGeometry(0.42, 0);
  const core = new THREE.Mesh(coreGeo, coreMat);
  core.scale.set(1, 1.35, 1);
  core.castShadow = true;
  floating.add(core);
  const edgeMat = new THREE.LineBasicMaterial({ color: P.accent, transparent: true, opacity: 0.9 });
  const edges = new THREE.LineSegments(new THREE.EdgesGeometry(coreGeo), edgeMat);
  core.add(edges);

  // Olho (frente = +z local).
  const eyeMat = new THREE.MeshBasicMaterial({ color: P.accent });
  const eye = new THREE.Mesh(new THREE.IcosahedronGeometry(0.075, 1), eyeMat);
  eye.position.set(0, 0.04, 0.33);
  floating.add(eye);
  const iris = new THREE.Mesh(new THREE.RingGeometry(0.09, 0.115, 16), new THREE.MeshBasicMaterial({ color: P.accent, transparent: true, opacity: 0.6, side: THREE.DoubleSide }));
  iris.position.set(0, 0.04, 0.335);
  floating.add(iris);

  // Anéis orbitais.
  const ringMat = new THREE.MeshBasicMaterial({ color: P.accent, transparent: true, opacity: 0.85 });
  const ring1 = new THREE.Mesh(new THREE.TorusGeometry(0.68, 0.018, 4, 40), ringMat);
  ring1.rotation.x = Math.PI / 2.4;
  floating.add(ring1);
  const ring2 = new THREE.Mesh(new THREE.TorusGeometry(0.82, 0.012, 4, 44), ringMat);
  ring2.rotation.x = Math.PI / 1.7;
  ring2.rotation.y = 0.6;
  floating.add(ring2);

  // Coroa: três lascas acima do núcleo (o "chefe").
  const shardMat = new THREE.MeshBasicMaterial({ color: P.accent, transparent: true, opacity: 0.9 });
  const crown = new THREE.Group();
  crown.position.y = 0.78;
  floating.add(crown);
  for (let i = 0; i < 3; i++) {
    const s = new THREE.Mesh(new THREE.TetrahedronGeometry(0.07, 0), shardMat);
    const a = (i / 3) * Math.PI * 2;
    s.position.set(Math.cos(a) * 0.16, 0, Math.sin(a) * 0.16);
    s.scale.set(0.7, 1.6, 0.7);
    crown.add(s);
  }

  // Satélites (eventos pendentes).
  const satMat = new THREE.MeshBasicMaterial({ color: 0xffe08a });
  const satellites = [];
  for (let i = 0; i < 6; i++) {
    const s = new THREE.Mesh(new THREE.TetrahedronGeometry(0.05, 0), satMat);
    s.visible = false;
    floating.add(s);
    satellites.push(s);
  }

  // Halo no chão.
  const haloMat = new THREE.MeshBasicMaterial({ color: P.accent, transparent: true, opacity: 0.35, blending: THREE.AdditiveBlending, depthWrite: false, side: THREE.DoubleSide });
  const halo = new THREE.Mesh(new THREE.RingGeometry(0.42, 0.78, 32), haloMat);
  halo.rotation.x = -Math.PI / 2;
  halo.position.y = 0.02;
  root.add(halo);
  const pulseMat = haloMat.clone();
  const pulse = new THREE.Mesh(new THREE.RingGeometry(0.7, 0.78, 40), pulseMat);
  pulse.rotation.x = -Math.PI / 2;
  pulse.position.y = 0.025;
  pulse.visible = false;
  root.add(pulse);

  const light = new THREE.PointLight(P.accent, 2.2, 6, 1.6);
  light.position.y = 1.4;
  root.add(light);

  return { root, floating, core, coreMat, edges, edgeMat, eye, eyeMat, iris, ring1, ring2, ringMat, crown, shardMat, satellites, halo, haloMat, pulse, pulseMat, light };
}
