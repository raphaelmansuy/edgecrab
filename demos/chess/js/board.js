/**
 * 3D board, lights, markers, camera helpers.
 */
import * as THREE from "three";
import { createPieceMesh, setPieceHighlight } from "./pieces.js";
import { squareName } from "./chess.js";

export const TILE = 1;
export const BOARD_Y = 0;

export function squareToWorld(row, col, flipped = false) {
  let r = row;
  let c = col;
  if (flipped) {
    r = 7 - row;
    c = 7 - col;
  }
  const x = (c - 3.5) * TILE;
  const z = (r - 3.5) * TILE;
  return new THREE.Vector3(x, BOARD_Y, z);
}

export function worldToSquare(x, z, flipped = false) {
  const c = Math.floor(x / TILE + 4);
  const r = Math.floor(z / TILE + 4);
  if (r < 0 || r > 7 || c < 0 || c > 7) return null;
  if (flipped) return { row: 7 - r, col: 7 - c };
  return { row: r, col: c };
}

export function createBoardGroup() {
  const root = new THREE.Group();
  root.name = "board";

  const lightMat = new THREE.MeshStandardMaterial({
    color: 0xd7c4a3,
    roughness: 0.7,
    metalness: 0.05,
  });
  const darkMat = new THREE.MeshStandardMaterial({
    color: 0x5a3d2b,
    roughness: 0.75,
    metalness: 0.08,
  });
  const frameMat = new THREE.MeshStandardMaterial({
    color: 0x2a1c12,
    roughness: 0.6,
    metalness: 0.15,
  });

  const tileGeo = new THREE.BoxGeometry(TILE * 0.98, 0.12, TILE * 0.98);
  const tiles = [];

  for (let row = 0; row < 8; row++) {
    for (let col = 0; col < 8; col++) {
      const isLight = (row + col) % 2 === 1;
      const mesh = new THREE.Mesh(tileGeo, isLight ? lightMat : darkMat);
      const pos = squareToWorld(row, col, false);
      mesh.position.set(pos.x, -0.06, pos.z);
      mesh.receiveShadow = true;
      mesh.userData = {
        kind: "tile",
        row,
        col,
        name: squareName(row, col),
        baseColor: isLight ? 0xd7c4a3 : 0x5a3d2b,
      };
      root.add(mesh);
      tiles.push(mesh);
    }
  }

  // Frame / rim
  const rim = new THREE.Mesh(
    new THREE.BoxGeometry(TILE * 8.6, 0.18, TILE * 8.6),
    frameMat,
  );
  rim.position.y = -0.16;
  rim.receiveShadow = true;
  rim.castShadow = true;
  root.add(rim);

  // Felt table under board
  const table = new THREE.Mesh(
    new THREE.CylinderGeometry(7.2, 7.4, 0.25, 48),
    new THREE.MeshStandardMaterial({
      color: 0x0e3d2c,
      roughness: 0.85,
      metalness: 0.05,
    }),
  );
  table.position.y = -0.35;
  table.receiveShadow = true;
  root.add(table);

  // File / rank labels (simple sprites via canvas)
  const labelGroup = new THREE.Group();
  labelGroup.name = "labels";
  root.add(labelGroup);

  root.userData.tiles = tiles;
  root.userData.labelGroup = labelGroup;
  return root;
}

function makeLabelSprite(text) {
  const canvas = document.createElement("canvas");
  canvas.width = 64;
  canvas.height = 64;
  const ctx = canvas.getContext("2d");
  ctx.clearRect(0, 0, 64, 64);
  ctx.fillStyle = "rgba(220, 230, 255, 0.85)";
  ctx.font = "bold 36px system-ui, sans-serif";
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillText(text, 32, 34);
  const tex = new THREE.CanvasTexture(canvas);
  const mat = new THREE.SpriteMaterial({ map: tex, transparent: true, depthTest: true });
  const spr = new THREE.Sprite(mat);
  spr.scale.set(0.35, 0.35, 0.35);
  return spr;
}

export function updateBoardLabels(boardGroup, flipped) {
  const g = boardGroup.userData.labelGroup;
  while (g.children.length) {
    const c = g.children.pop();
    if (c.material?.map) c.material.map.dispose();
    c.material?.dispose();
  }
  const files = "abcdefgh";
  for (let i = 0; i < 8; i++) {
    const fileIdx = flipped ? 7 - i : i;
    const rankIdx = flipped ? i : 7 - i;
    const f = makeLabelSprite(files[fileIdx]);
    const posF = squareToWorld(7, i, false);
    f.position.set(posF.x, 0.02, posF.z + 0.55);
    g.add(f);

    const r = makeLabelSprite(String(rankIdx + 1));
    const posR = squareToWorld(i, 0, false);
    r.position.set(posR.x - 0.55, 0.02, posR.z);
    g.add(r);
  }
}

export function createMarkers() {
  const group = new THREE.Group();
  group.name = "markers";

  const moveMat = new THREE.MeshBasicMaterial({
    color: 0x5ddea8,
    transparent: true,
    opacity: 0.45,
    depthWrite: false,
  });
  const captureMat = new THREE.MeshBasicMaterial({
    color: 0xff6b7a,
    transparent: true,
    opacity: 0.5,
    depthWrite: false,
  });
  const lastMat = new THREE.MeshBasicMaterial({
    color: 0xc9a227,
    transparent: true,
    opacity: 0.35,
    depthWrite: false,
  });
  const hoverMat = new THREE.MeshBasicMaterial({
    color: 0x6ea8ff,
    transparent: true,
    opacity: 0.3,
    depthWrite: false,
  });

  const dotGeo = new THREE.CircleGeometry(0.15, 24);
  const ringGeo = new THREE.RingGeometry(0.32, 0.45, 28);
  const squareGeo = new THREE.PlaneGeometry(TILE * 0.96, TILE * 0.96);

  group.userData = { moveMat, captureMat, lastMat, hoverMat, dotGeo, ringGeo, squareGeo };
  return group;
}

export function clearMarkers(markers) {
  while (markers.children.length) {
    markers.remove(markers.children[0]);
  }
}

export function showMoveMarkers(markers, moves, flipped) {
  // Does not clear — caller composes last-move + legal markers.
  const { moveMat, captureMat, dotGeo, ringGeo } = markers.userData;
  for (const m of moves) {
    const pos = squareToWorld(m.row, m.col, flipped);
    if (m.capture || m.enPassant) {
      const ring = new THREE.Mesh(ringGeo, captureMat);
      ring.rotation.x = -Math.PI / 2;
      ring.position.set(pos.x, 0.04, pos.z);
      markers.add(ring);
    } else {
      const dot = new THREE.Mesh(dotGeo, moveMat);
      dot.rotation.x = -Math.PI / 2;
      dot.position.set(pos.x, 0.04, pos.z);
      markers.add(dot);
    }
  }
}

export function showLastMove(markers, from, to, flipped) {
  const { lastMat, squareGeo } = markers.userData;
  for (const sq of [from, to]) {
    if (!sq) continue;
    const pos = squareToWorld(sq.row, sq.col, flipped);
    const pl = new THREE.Mesh(squareGeo, lastMat);
    pl.rotation.x = -Math.PI / 2;
    pl.position.set(pos.x, 0.03, pos.z);
    markers.add(pl);
  }
}

export function createSceneKit(canvas) {
  const renderer = new THREE.WebGLRenderer({
    canvas,
    antialias: true,
    alpha: false,
    powerPreference: "high-performance",
  });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  renderer.setSize(window.innerWidth, window.innerHeight, false);
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  if ("outputColorSpace" in renderer) {
    renderer.outputColorSpace = THREE.SRGBColorSpace;
  } else if ("outputEncoding" in renderer) {
    renderer.outputEncoding = THREE.sRGBEncoding;
  }
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.05;

  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x0b1020);
  scene.fog = new THREE.Fog(0x0b1020, 18, 42);

  const camera = new THREE.PerspectiveCamera(
    42,
    window.innerWidth / window.innerHeight,
    0.1,
    100,
  );
  camera.position.set(0, 9.5, 10.5);
  camera.lookAt(0, 0, 0);

  // Lights
  const hemi = new THREE.HemisphereLight(0xb8c8ff, 0x2a1a10, 0.85);
  scene.add(hemi);

  const sun = new THREE.DirectionalLight(0xfff2dd, 1.35);
  sun.position.set(6, 14, 8);
  sun.castShadow = true;
  sun.shadow.mapSize.set(2048, 2048);
  sun.shadow.camera.near = 1;
  sun.shadow.camera.far = 40;
  sun.shadow.camera.left = -10;
  sun.shadow.camera.right = 10;
  sun.shadow.camera.top = 10;
  sun.shadow.camera.bottom = -10;
  sun.shadow.bias = -0.0002;
  scene.add(sun);

  const fill = new THREE.DirectionalLight(0x6ea8ff, 0.35);
  fill.position.set(-8, 6, -4);
  scene.add(fill);

  const board = createBoardGroup();
  scene.add(board);
  updateBoardLabels(board, false);

  const piecesRoot = new THREE.Group();
  piecesRoot.name = "pieces";
  scene.add(piecesRoot);

  const markers = createMarkers();
  scene.add(markers);

  return { renderer, scene, camera, board, piecesRoot, markers, sun };
}

export function syncPieces(piecesRoot, board, flipped) {
  // Map existing by square key
  const existing = new Map();
  for (const child of [...piecesRoot.children]) {
    const key = `${child.userData.row},${child.userData.col}`;
    existing.set(key, child);
  }

  const needed = new Set();
  for (let row = 0; row < 8; row++) {
    for (let col = 0; col < 8; col++) {
      const p = board[row][col];
      if (!p) continue;
      const key = `${row},${col}`;
      needed.add(key);
      let mesh = existing.get(key);
      if (mesh && (mesh.userData.type !== p.type || mesh.userData.color !== p.color)) {
        piecesRoot.remove(mesh);
        mesh = null;
      }
      if (!mesh) {
        mesh = createPieceMesh(p.type, p.color);
        mesh.userData.row = row;
        mesh.userData.col = col;
        piecesRoot.add(mesh);
      }
      const pos = squareToWorld(row, col, flipped);
      mesh.position.set(pos.x, 0, pos.z);
      mesh.userData.row = row;
      mesh.userData.col = col;
      setPieceHighlight(mesh, null);
    }
  }

  for (const [key, mesh] of existing) {
    if (!needed.has(key)) piecesRoot.remove(mesh);
  }
}

export function findPieceMesh(piecesRoot, row, col) {
  return piecesRoot.children.find(
    (c) => c.userData.row === row && c.userData.col === col,
  );
}

export function animatePieceMove(mesh, toPos, duration = 0.22) {
  return new Promise((resolve) => {
    const from = mesh.position.clone();
    const start = performance.now();
    const lift = 0.45;

    function frame(now) {
      const t = Math.min(1, (now - start) / (duration * 1000));
      const e = t < 0.5 ? 2 * t * t : -1 + (4 - 2 * t) * t;
      mesh.position.x = from.x + (toPos.x - from.x) * e;
      mesh.position.z = from.z + (toPos.z - from.z) * e;
      mesh.position.y = Math.sin(t * Math.PI) * lift;
      if (t < 1) requestAnimationFrame(frame);
      else {
        mesh.position.set(toPos.x, 0, toPos.z);
        resolve();
      }
    }
    requestAnimationFrame(frame);
  });
}
