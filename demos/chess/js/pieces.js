/**
 * Procedural chess pieces built from Three.js primitives.
 */
import * as THREE from "three";

const GEO_CACHE = new Map();

function geo(key, factory) {
  if (!GEO_CACHE.has(key)) GEO_CACHE.set(key, factory());
  return GEO_CACHE.get(key);
}

function mat(color, opts = {}) {
  return new THREE.MeshStandardMaterial({
    color,
    roughness: opts.roughness ?? 0.35,
    metalness: opts.metalness ?? 0.25,
    emissive: opts.emissive ?? 0x000000,
    emissiveIntensity: opts.emissiveIntensity ?? 0,
  });
}

function addMesh(group, geometry, material, y, scale = [1, 1, 1]) {
  const m = new THREE.Mesh(geometry, material);
  m.position.y = y;
  m.scale.set(scale[0], scale[1], scale[2]);
  m.castShadow = true;
  m.receiveShadow = true;
  group.add(m);
  return m;
}

function basePedestal(group, material) {
  const base = geo("base", () => new THREE.CylinderGeometry(0.32, 0.36, 0.1, 24));
  const ring = geo("ring", () => new THREE.TorusGeometry(0.28, 0.035, 10, 28));
  addMesh(group, base, material, 0.05);
  const r = addMesh(group, ring, material, 0.12);
  r.rotation.x = Math.PI / 2;
}

function stem(group, material, h = 0.45, r = 0.1) {
  const g = geo(`stem-${h}-${r}`, () => new THREE.CylinderGeometry(r * 0.85, r * 1.15, h, 16));
  addMesh(group, g, material, 0.12 + h / 2);
  return 0.12 + h;
}

export function createPieceMesh(type, color) {
  const isWhite = color === "w";
  const bodyColor = isWhite ? 0xf3efe6 : 0x2c3344;
  const accent = isWhite ? 0xd4c4a0 : 0x1a2030;
  const material = mat(bodyColor, {
    roughness: isWhite ? 0.32 : 0.45,
    metalness: isWhite ? 0.18 : 0.35,
  });
  const accentMat = mat(accent, { roughness: 0.4, metalness: 0.4 });

  const group = new THREE.Group();
  group.userData = { type, color, kind: "piece" };

  basePedestal(group, material);

  switch (type) {
    case "p": {
      const top = stem(group, material, 0.32, 0.09);
      const head = geo("pawn-head", () => new THREE.SphereGeometry(0.16, 20, 16));
      addMesh(group, head, material, top + 0.14);
      break;
    }
    case "r": {
      const top = stem(group, material, 0.38, 0.14);
      const body = geo("rook-body", () => new THREE.CylinderGeometry(0.2, 0.22, 0.28, 12));
      addMesh(group, body, material, top + 0.14);
      const battlement = geo("rook-top", () => new THREE.BoxGeometry(0.42, 0.12, 0.42));
      addMesh(group, battlement, accentMat, top + 0.34);
      for (const [x, z] of [[-0.14, -0.14], [-0.14, 0.14], [0.14, -0.14], [0.14, 0.14]]) {
        const t = geo("rook-tooth", () => new THREE.BoxGeometry(0.12, 0.12, 0.12));
        const m = addMesh(group, t, material, top + 0.46);
        m.position.x = x;
        m.position.z = z;
      }
      break;
    }
    case "n": {
      const top = stem(group, material, 0.28, 0.12);
      const neck = geo("knight-neck", () => new THREE.BoxGeometry(0.18, 0.35, 0.28));
      const n = addMesh(group, neck, material, top + 0.2);
      n.rotation.z = 0.25;
      n.position.x = 0.04;
      const head = geo("knight-head", () => new THREE.BoxGeometry(0.22, 0.18, 0.34));
      const h = addMesh(group, head, material, top + 0.42);
      h.position.x = 0.1;
      h.rotation.z = -0.35;
      const snout = geo("knight-snout", () => new THREE.BoxGeometry(0.14, 0.12, 0.2));
      const s = addMesh(group, snout, accentMat, top + 0.38);
      s.position.x = 0.22;
      const ear = geo("knight-ear", () => new THREE.ConeGeometry(0.06, 0.14, 8));
      const e = addMesh(group, ear, material, top + 0.56);
      e.position.set(0.06, 0, -0.08);
      break;
    }
    case "b": {
      const top = stem(group, material, 0.42, 0.1);
      const body = geo("bishop-body", () => new THREE.SphereGeometry(0.18, 20, 16));
      addMesh(group, body, material, top + 0.12, [1, 1.35, 1]);
      const mitre = geo("bishop-mitre", () => new THREE.ConeGeometry(0.14, 0.32, 16));
      addMesh(group, mitre, material, top + 0.42);
      const slit = geo("bishop-slit", () => new THREE.BoxGeometry(0.04, 0.18, 0.2));
      addMesh(group, slit, accentMat, top + 0.4);
      const tip = geo("bishop-tip", () => new THREE.SphereGeometry(0.05, 12, 10));
      addMesh(group, tip, material, top + 0.62);
      break;
    }
    case "q": {
      const top = stem(group, material, 0.48, 0.11);
      const body = geo("queen-body", () => new THREE.SphereGeometry(0.2, 20, 16));
      addMesh(group, body, material, top + 0.1, [1, 1.2, 1]);
      const crown = geo("queen-crown", () => new THREE.CylinderGeometry(0.16, 0.2, 0.14, 8));
      addMesh(group, crown, accentMat, top + 0.32);
      for (let i = 0; i < 5; i++) {
        const ang = (i / 5) * Math.PI * 2;
        const pt = geo("queen-point", () => new THREE.ConeGeometry(0.045, 0.14, 8));
        const m = addMesh(group, pt, material, top + 0.46);
        m.position.x = Math.cos(ang) * 0.12;
        m.position.z = Math.sin(ang) * 0.12;
      }
      const orb = geo("queen-orb", () => new THREE.SphereGeometry(0.07, 14, 12));
      addMesh(group, orb, material, top + 0.58);
      break;
    }
    case "k": {
      const top = stem(group, material, 0.5, 0.12);
      const body = geo("king-body", () => new THREE.SphereGeometry(0.2, 20, 16));
      addMesh(group, body, material, top + 0.1, [1, 1.15, 1]);
      const band = geo("king-band", () => new THREE.CylinderGeometry(0.18, 0.2, 0.1, 16));
      addMesh(group, band, accentMat, top + 0.3);
      const crossV = geo("king-cross-v", () => new THREE.BoxGeometry(0.08, 0.32, 0.08));
      addMesh(group, crossV, material, top + 0.52);
      const crossH = geo("king-cross-h", () => new THREE.BoxGeometry(0.22, 0.08, 0.08));
      addMesh(group, crossH, material, top + 0.56);
      break;
    }
    default:
      break;
  }

  // Selection outline helper (invisible until selected)
  const halo = new THREE.Mesh(
    geo("halo", () => new THREE.RingGeometry(0.34, 0.42, 32)),
    new THREE.MeshBasicMaterial({
      color: 0x6ea8ff,
      transparent: true,
      opacity: 0,
      side: THREE.DoubleSide,
      depthWrite: false,
    }),
  );
  halo.rotation.x = -Math.PI / 2;
  halo.position.y = 0.02;
  halo.name = "halo";
  group.add(halo);

  group.traverse((o) => {
    if (o.isMesh) o.userData.pieceRoot = group;
  });

  return group;
}

export function setPieceHighlight(group, mode) {
  // mode: null | 'select' | 'check'
  const halo = group.getObjectByName("halo");
  if (!halo) return;
  if (!mode) {
    halo.material.opacity = 0;
    return;
  }
  halo.material.color.set(mode === "check" ? 0xff6b7a : 0x6ea8ff);
  halo.material.opacity = mode === "check" ? 0.9 : 0.75;
}

export function disposePiece(group) {
  group.traverse((o) => {
    if (o.isMesh) {
      // shared geos — only dispose unique materials
      if (o.material && o.material.dispose) o.material.dispose();
    }
  });
}
