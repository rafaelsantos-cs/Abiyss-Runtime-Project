// Modelo low-poly do IMMo: um pequeno trabalhador robótico, estilizado.
//
// Hierarquia (para animar por pivôs):
//   root (posição/orientação no mundo)
//    └─ body (sentar/deitar/encolher: altura e rotação)
//        ├─ torso, crachá
//        ├─ headPivot → cabeça, visor, olhos, antena (+ lâmpada = cor do estado)
//        ├─ armL/armR (ombros)   └─ mão (+ caneca / relatório opcionais)
//        └─ legL/legR (quadris)
// Altura total ≈ 1,2 m (as mesas e cadeiras são na mesma escala).

import * as THREE from 'three';
import { mat, uniqueMat, PALETTE as P } from '../materials.js';

const IMMO_DIMS = { hipY: 0.38, torsoH: 0.42, headR: 0.2 };

export function createImmoModel(def) {
  const root = new THREE.Group();
  root.name = def.id;
  const body = new THREE.Group();
  root.add(body);
  const color = def.color;
  const bodyMat = mat(color);
  const darkMat = mat(new THREE.Color(color).offsetHSL(0, -0.05, -0.18).getHex());
  const shell = mat(0xe9edf2);
  const visorMat = mat(0x1a1f2e);

  const mesh = (geo, m, x, y, z, parent = body) => {
    const o = new THREE.Mesh(geo, m);
    o.position.set(x, y, z);
    o.castShadow = true;
    o.receiveShadow = false;
    parent.add(o);
    return o;
  };

  // Tronco (cilindro facetado) e "barriga" mais clara.
  mesh(new THREE.CylinderGeometry(0.17, 0.2, IMMO_DIMS.torsoH, 7), bodyMat, 0, IMMO_DIMS.hipY + IMMO_DIMS.torsoH / 2, 0);
  mesh(new THREE.CylinderGeometry(0.205, 0.205, 0.06, 7), darkMat, 0, IMMO_DIMS.hipY + 0.03, 0);
  const badge = mesh(new THREE.BoxGeometry(0.13, 0.1, 0.02), shell, 0, IMMO_DIMS.hipY + 0.27, 0.185);
  badge.castShadow = false;

  // Cabeça.
  const headPivot = new THREE.Group();
  headPivot.position.set(0, IMMO_DIMS.hipY + IMMO_DIMS.torsoH + 0.02, 0);
  body.add(headPivot);
  const head = mesh(new THREE.IcosahedronGeometry(IMMO_DIMS.headR, 1), shell, 0, 0.17, 0, headPivot);
  head.scale.set(1.1, 0.92, 1);
  mesh(new THREE.BoxGeometry(0.3, 0.11, 0.08), visorMat, 0, 0.18, 0.15, headPivot);
  const eyeMat = uniqueMat(0x0d1018, { emissive: P.accent, emissiveIntensity: 1 });
  const eyeL = mesh(new THREE.BoxGeometry(0.05, 0.045, 0.02), eyeMat, -0.065, 0.185, 0.195, headPivot);
  const eyeR = mesh(new THREE.BoxGeometry(0.05, 0.045, 0.02), eyeMat, 0.065, 0.185, 0.195, headPivot);
  eyeL.castShadow = false;
  eyeR.castShadow = false;
  mesh(new THREE.CylinderGeometry(0.008, 0.008, 0.16, 4), mat(P.metalDark), 0.05, 0.42, -0.02, headPivot);
  const bulbMat = uniqueMat(0x222222, { emissive: 0xffffff, emissiveIntensity: 1 });
  const bulb = mesh(new THREE.IcosahedronGeometry(0.035, 0), bulbMat, 0.05, 0.51, -0.02, headPivot);
  bulb.castShadow = false;
  // "Orelhas" (fones) com a cor do IMMo.
  mesh(new THREE.CylinderGeometry(0.05, 0.05, 0.04, 6), bodyMat, -0.22, 0.17, 0, headPivot).rotation.z = Math.PI / 2;
  mesh(new THREE.CylinderGeometry(0.05, 0.05, 0.04, 6), bodyMat, 0.22, 0.17, 0, headPivot).rotation.z = Math.PI / 2;

  // Braços.
  const arm = (side) => {
    const pivot = new THREE.Group();
    pivot.position.set(side * 0.215, IMMO_DIMS.hipY + IMMO_DIMS.torsoH - 0.05, 0);
    body.add(pivot);
    mesh(new THREE.CapsuleGeometry(0.045, 0.2, 2, 6), darkMat, 0, -0.14, 0, pivot);
    const hand = mesh(new THREE.IcosahedronGeometry(0.05, 0), shell, 0, -0.28, 0, pivot);
    return { pivot, hand };
  };
  const armL = arm(-1);
  const armR = arm(1);

  // Pernas.
  const leg = (side) => {
    const pivot = new THREE.Group();
    pivot.position.set(side * 0.09, IMMO_DIMS.hipY, 0);
    body.add(pivot);
    mesh(new THREE.CylinderGeometry(0.05, 0.045, IMMO_DIMS.hipY - 0.05, 6), darkMat, 0, -(IMMO_DIMS.hipY - 0.05) / 2, 0, pivot);
    mesh(new THREE.BoxGeometry(0.1, 0.06, 0.16), mat(0x2b3040), 0, -IMMO_DIMS.hipY + 0.03, 0.03, pivot);
    return pivot;
  };
  const legL = leg(-1);
  const legR = leg(1);

  // Acessórios (aparecem conforme a atividade).
  const mug = mesh(new THREE.CylinderGeometry(0.035, 0.03, 0.08, 8), mat(0xffffff), 0, -0.3, 0.05, armR.pivot);
  mug.visible = false;
  const paper = mesh(new THREE.BoxGeometry(0.16, 0.2, 0.01), mat(0xfffdf5), 0, -0.3, 0.08, armR.pivot);
  paper.visible = false;

  return {
    root, body, headPivot, armL, armR, legL, legR, eyeMat, bulbMat, eyeL, eyeR, mug, paper,
    color,
  };
}
