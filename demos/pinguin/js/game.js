import * as THREE from 'three';
import {
  ui, prefs, applyQuality, initSettings, announce, formatTime,
  showToast, flashCombo, AudioEngine,
} from './ui.js';

// ---- constants ----
const WORLD_SIZE = 160;
const TOTAL_FISH = 40;
const GOAL_NEED = 30;
const GOAL_Z = -WORLD_SIZE * 0.42;
const TOTAL_TIME = 180;
const COMBO_WINDOW = 2.6;
const MAGNET_RADIUS = 14;
const MAGNET_DURATION = 8;
const TURBO_DURATION = 5;

const canvas = document.getElementById('game-canvas');

// ---- state ----
let running = false;
let finished = false;
let paused = false;
let startTime = 0;
let pauseStart = 0;
let pausedMs = 0;
let fishCollected = 0;
let bonusFishCollected = 0;
let score = 0;
let combo = 0;
let bestCombo = 1;
let comboTimer = 0;
let lastTime = performance.now();
let magnetTimer = 0;
let turboTimer = 0;
let jumpQueued = false;
let slidePressed = false;

const keys = {
  ArrowUp: false, ArrowDown: false, ArrowLeft: false, ArrowRight: false,
  w: false, a: false, s: false, d: false,
};

// ---- renderer / scene ----
const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: false });
renderer.setSize(window.innerWidth, window.innerHeight);
renderer.shadowMap.enabled = true;
renderer.shadowMap.type = THREE.PCFSoftShadowMap;

const scene = new THREE.Scene();
scene.background = new THREE.Color(0x0b1d2e);
scene.fog = new THREE.FogExp2(0x0b1d2e, 0.016);

applyQuality(renderer, null, prefs.quality);

const camera = new THREE.PerspectiveCamera(58, window.innerWidth / window.innerHeight, 0.1, 400);
camera.position.set(0, 16, 24);

const ambient = new THREE.AmbientLight(0x6f9fff, 0.55);
scene.add(ambient);
const hemi = new THREE.HemisphereLight(0xbbe6ff, 0x243b55, 0.6);
scene.add(hemi);
const sun = new THREE.DirectionalLight(0xfff5d6, 1.15);
sun.position.set(40, 80, -30);
sun.castShadow = true;
sun.shadow.mapSize.set(2048, 2048);
sun.shadow.camera.left = -90;
sun.shadow.camera.right = 90;
sun.shadow.camera.top = 90;
sun.shadow.camera.bottom = -90;
sun.shadow.camera.near = 1;
sun.shadow.camera.far = 220;
scene.add(sun);

// soft rim light for pop
const rim = new THREE.DirectionalLight(0x66ccff, 0.35);
rim.position.set(-30, 20, 40);
scene.add(rim);

function randRange(min, max) { return Math.random() * (max - min) + min; }
function clamp(v, min, max) { return Math.max(min, Math.min(max, v)); }

const iceMat = new THREE.MeshStandardMaterial({
  color: 0xcff1ff, roughness: 0.18, metalness: 0.08, flatShading: true,
});
const snowMat = new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.92, metalness: 0 });
const rockMat = new THREE.MeshStandardMaterial({ color: 0x4a5568, roughness: 0.85, flatShading: true });
const fishMat = new THREE.MeshStandardMaterial({
  color: 0xffb347, emissive: 0xff8c00, emissiveIntensity: 0.6, roughness: 0.3, metalness: 0.35,
});
const bonusMat = new THREE.MeshStandardMaterial({
  color: 0xff6699, emissive: 0xff3366, emissiveIntensity: 0.85, roughness: 0.25, metalness: 0.4,
});
// Rainbow fish for extra excitement!
const rainbowMat = new THREE.MeshStandardMaterial({
  color: 0xffffff, emissive: 0xffffff, emissiveIntensity: 0.9, roughness: 0.2, metalness: 0.6,
});
const magnetMat = new THREE.MeshStandardMaterial({
  color: 0x66ffcc, emissive: 0x00ffaa, emissiveIntensity: 0.7, roughness: 0.25, metalness: 0.5,
});
const turboMat = new THREE.MeshStandardMaterial({
  color: 0xaaccff, emissive: 0x4488ff, emissiveIntensity: 0.85, roughness: 0.2, metalness: 0.55,
});

// ---- penguin ----
function createPenguin() {
  const group = new THREE.Group();
  const bodyMat = new THREE.MeshStandardMaterial({ color: 0x1a2639, roughness: 0.45 });
  const body = new THREE.Mesh(new THREE.CapsuleGeometry(1, 2.4, 8, 12), bodyMat);
  body.position.y = 1.8;
  body.castShadow = true;
  group.add(body);

  const belly = new THREE.Mesh(
    new THREE.CapsuleGeometry(0.72, 2.0, 8, 12),
    new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.5 }),
  );
  belly.position.set(0, 1.7, 0.45);
  belly.scale.set(0.85, 0.9, 0.55);
  group.add(belly);

  const head = new THREE.Mesh(new THREE.SphereGeometry(0.82, 24, 24), bodyMat);
  head.position.y = 3.3;
  head.castShadow = true;
  group.add(head);

  const eyeWhiteMat = new THREE.MeshBasicMaterial({ color: 0xffffff });
  const pupilMat = new THREE.MeshBasicMaterial({ color: 0x111111 });
  [[-0.28, 3.45, 0.62], [0.28, 3.45, 0.62]].forEach(([x, y, z]) => {
    const ew = new THREE.Mesh(new THREE.SphereGeometry(0.2, 16, 16), eyeWhiteMat);
    ew.position.set(x, y, z);
    const p = new THREE.Mesh(new THREE.SphereGeometry(0.09, 12, 12), pupilMat);
    p.position.set(x, y, z + 0.16);
    group.add(ew, p);
  });

  const beak = new THREE.Mesh(
    new THREE.ConeGeometry(0.18, 0.55, 16),
    new THREE.MeshStandardMaterial({ color: 0xffa500, roughness: 0.4 }),
  );
  beak.rotation.x = Math.PI / 2;
  beak.position.set(0, 3.35, 0.95);
  group.add(beak);

  const flipperL = new THREE.Mesh(new THREE.BoxGeometry(0.25, 1.4, 0.6), bodyMat);
  flipperL.position.set(-1.05, 2.1, 0.1);
  flipperL.rotation.z = 0.18;
  flipperL.castShadow = true;
  const flipperR = new THREE.Mesh(new THREE.BoxGeometry(0.25, 1.4, 0.6), bodyMat);
  flipperR.position.set(1.05, 2.1, 0.1);
  flipperR.rotation.z = -0.18;
  flipperR.castShadow = true;
  group.add(flipperL, flipperR);

  const footMat = new THREE.MeshStandardMaterial({ color: 0xffa500, roughness: 0.6 });
  const footL = new THREE.Mesh(new THREE.BoxGeometry(0.55, 0.2, 0.85), footMat);
  footL.position.set(-0.45, 0.2, 0.35);
  const footR = new THREE.Mesh(new THREE.BoxGeometry(0.55, 0.2, 0.85), footMat);
  footR.position.set(0.45, 0.2, 0.35);
  group.add(footL, footR);

  // scarf for personality
  const scarf = new THREE.Mesh(
    new THREE.TorusGeometry(0.55, 0.12, 8, 20),
    new THREE.MeshStandardMaterial({ color: 0xef476f, roughness: 0.6 }),
  );
  scarf.position.set(0, 2.75, 0.1);
  scarf.rotation.x = Math.PI / 2;
  group.add(scarf);

  group.userData = { flipperL, flipperR, footL, footR, body, scarf };
  return group;
}

const penguin = createPenguin();
scene.add(penguin);

// ground
const ground = new THREE.Mesh(
  new THREE.CylinderGeometry(WORLD_SIZE * 0.65, WORLD_SIZE * 0.65, 1, 64),
  iceMat,
);
ground.position.y = -0.5;
ground.receiveShadow = true;
scene.add(ground);

// snow patches
for (let i = 0; i < 22; i++) {
  const size = randRange(4, 14);
  const h = randRange(0.25, 1.3);
  const mesh = new THREE.Mesh(new THREE.CylinderGeometry(size, size * 1.12, h, 9), snowMat);
  const angle = Math.random() * Math.PI * 2;
  const dist = randRange(16, WORLD_SIZE * 0.55);
  mesh.position.set(Math.cos(angle) * dist, h * 0.5 - 0.4, Math.sin(angle) * dist);
  mesh.rotation.y = randRange(0, Math.PI);
  mesh.receiveShadow = true;
  scene.add(mesh);
}

// obstacles
const obstacles = [];
function addObstacles(count) {
  for (let i = 0; i < count; i++) {
    const isRock = Math.random() > 0.35;
    const w = randRange(1.1, 3.0);
    const geo = isRock
      ? new THREE.DodecahedronGeometry(w * 0.8, 0)
      : new THREE.BoxGeometry(w, randRange(1.4, 3.2), w);
    const mat = isRock
      ? rockMat
      : new THREE.MeshStandardMaterial({
        color: 0x9fdfff, roughness: 0.2, transparent: true, opacity: 0.9, flatShading: true,
      });
    const mesh = new THREE.Mesh(geo, mat);
    let pos;
    let attempts = 0;
    do {
      const angle = Math.random() * Math.PI * 2;
      const dist = randRange(16, WORLD_SIZE * 0.56);
      pos = new THREE.Vector3(Math.cos(angle) * dist, w * 0.5, Math.sin(angle) * dist);
      attempts++;
    } while (
      (pos.distanceTo(new THREE.Vector3(0, 0, GOAL_Z)) < 14
        || pos.distanceTo(new THREE.Vector3(0, 0, WORLD_SIZE * 0.45)) < 12)
      && attempts < 30
    );
    mesh.position.copy(pos);
    mesh.rotation.set(randRange(0, Math.PI), randRange(0, Math.PI), randRange(0, Math.PI * 0.3));
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    const radius = isRock ? w * 0.85 : w * 0.75;
    obstacles.push({ mesh, radius, position: pos.clone() });
    scene.add(mesh);
  }
}
addObstacles(34);

// ice pillars
for (let i = 0; i < 14; i++) {
  const h = randRange(3, 7);
  const r = randRange(0.45, 1.0);
  const mesh = new THREE.Mesh(
    new THREE.ConeGeometry(r, h, 6),
    new THREE.MeshStandardMaterial({ color: 0xbee7ff, roughness: 0.25, flatShading: true }),
  );
  const angle = Math.random() * Math.PI * 2;
  const dist = randRange(24, WORLD_SIZE * 0.56);
  mesh.position.set(Math.cos(angle) * dist, h * 0.5 - 0.2, Math.sin(angle) * dist);
  mesh.castShadow = true;
  obstacles.push({ mesh, radius: r * 1.15, position: mesh.position.clone() });
  scene.add(mesh);
}

// ---- collectibles ----
const fishMeshes = [];
const fishGroup = new THREE.Group();
scene.add(fishGroup);
const powerups = [];
const powerGroup = new THREE.Group();
scene.add(powerGroup);

// floating score pops (CSS-free 3D sprites via sprites later — simple toast for now)
function createFishMesh(mat, scale = 1.4) {
  const group = new THREE.Group();
  const body = new THREE.Mesh(new THREE.CapsuleGeometry(0.22, 0.5, 8, 12), mat);
  body.rotation.z = Math.PI / 2;
  group.add(body);
  const tail = new THREE.Mesh(new THREE.ConeGeometry(0.18, 0.35, 4), mat);
  tail.position.x = -0.45;
  tail.rotation.z = -Math.PI / 2;
  group.add(tail);
  const eye = new THREE.Mesh(
    new THREE.SphereGeometry(0.06),
    new THREE.MeshBasicMaterial({ color: 0x000000 }),
  );
  eye.position.set(0.12, 0.08, 0.16);
  group.add(eye);
  group.scale.setScalar(scale);
  return group;
}

function randomIcePos(minDistFromPenguin = 10) {
  let pos;
  let attempts = 0;
  do {
    const angle = Math.random() * Math.PI * 2;
    const dist = randRange(12, WORLD_SIZE * 0.58);
    pos = new THREE.Vector3(Math.cos(angle) * dist, 1.5, Math.sin(angle) * dist);
    attempts++;
  } while (
    (pos.distanceTo(penguin.position) < minDistFromPenguin
      || pos.distanceTo(new THREE.Vector3(0, 0, GOAL_Z)) < 10)
    && attempts < 40
  );
  return pos;
}

function resetFish() {
  while (fishGroup.children.length) fishGroup.remove(fishGroup.children[0]);
  fishMeshes.length = 0;

  for (let i = 0; i < TOTAL_FISH; i++) {
    const fish = createFishMesh(fishMat, 1.45);
    fish.position.copy(randomIcePos(10));
    fish.userData = {
      collected: false,
      bobOffset: Math.random() * Math.PI * 2,
      bonus: false,
      baseY: 1.45,
    };
    fishMeshes.push(fish);
    fishGroup.add(fish);
  }

  for (let i = 0; i < 8; i++) {
    const fish = createFishMesh(bonusMat, 1.85);
    fish.position.copy(randomIcePos(14));
    fish.position.y = 2.1;
    fish.userData = {
      collected: false,
      bobOffset: Math.random() * Math.PI * 2,
      bonus: true,
      baseY: 2.1,
    };
    fishMeshes.push(fish);
    fishGroup.add(fish);
  }

  // Rainbow fish - ultra rare!
  for (let i = 0; i < 3; i++) {
    const fish = createFishMesh(rainbowMat, 2.1);
    fish.position.copy(randomIcePos(20));
    fish.position.y = 3.0;
    fish.userData = {
      collected: false,
      bobOffset: Math.random() * Math.PI * 2,
      bonus: true,
      rainbow: true,
      baseY: 3.0,
    };
    fishMeshes.push(fish);
    fishGroup.add(fish);
  }
}

function createPowerMesh(kind) {
  const group = new THREE.Group();
  const mat = kind === 'magnet' ? magnetMat : turboMat;
  const core = new THREE.Mesh(new THREE.OctahedronGeometry(0.55, 0), mat);
  group.add(core);
  const ring = new THREE.Mesh(
    new THREE.TorusGeometry(0.7, 0.08, 8, 24),
    mat,
  );
  ring.rotation.x = Math.PI / 2;
  group.add(ring);
  group.userData = { kind, core, ring };
  return group;
}

function resetPowerups() {
  while (powerGroup.children.length) powerGroup.remove(powerGroup.children[0]);
  powerups.length = 0;
  const kinds = ['magnet', 'turbo', 'magnet', 'turbo', 'turbo', 'magnet', 'turbo'];
  for (const kind of kinds) {
    const mesh = createPowerMesh(kind);
    const pos = randomIcePos(20);
    pos.y = 1.8;
    mesh.position.copy(pos);
    mesh.userData.collected = false;
    mesh.userData.bobOffset = Math.random() * Math.PI * 2;
    mesh.userData.baseY = 1.8;
    // Add rotation animation speed
    mesh.userData.spinSpeed = randRange(0.8, 1.6);
    powerups.push(mesh);
    powerGroup.add(mesh);
  }
}

// goal arch
let arch = null;
let archOpen = false;
function createGoal() {
  if (arch) scene.remove(arch);
  arch = new THREE.Group();
  const colMat = new THREE.MeshStandardMaterial({
    color: 0x88e1ff,
    emissive: 0x00aaff,
    emissiveIntensity: 0.35,
    roughness: 0.2,
    transparent: true,
    opacity: 0.55,
  });
  const colGeo = new THREE.CylinderGeometry(0.8, 0.9, 8, 16);
  const left = new THREE.Mesh(colGeo, colMat);
  left.position.set(-3.5, 4, 0);
  const right = new THREE.Mesh(colGeo, colMat);
  right.position.set(3.5, 4, 0);
  arch.add(left, right);

  const top = new THREE.Mesh(new THREE.TorusGeometry(3.5, 0.6, 12, 48, Math.PI), colMat);
  top.position.set(0, 8, 0);
  arch.add(top);

  const gate = new THREE.Mesh(
    new THREE.PlaneGeometry(6.2, 7.2),
    new THREE.MeshBasicMaterial({
      color: 0x33ccff,
      transparent: true,
      opacity: 0.12,
      side: THREE.DoubleSide,
    }),
  );
  gate.position.set(0, 4, 0);
  arch.add(gate);

  const particles = new THREE.Group();
  for (let i = 0; i < 36; i++) {
    const p = new THREE.Mesh(new THREE.OctahedronGeometry(0.12), colMat);
    const a = Math.random() * Math.PI;
    const r = randRange(3.6, 5.0);
    p.position.set(Math.cos(a) * r, randRange(2, 7.5), Math.sin(a) * r * 0.25);
    p.userData = { speed: randRange(0.5, 1.5), phase: Math.random() * Math.PI * 2 };
    particles.add(p);
  }
  arch.add(particles);
  arch.userData = { particles, colMat, gate };
  arch.position.set(0, 0, GOAL_Z);
  archOpen = false;
  scene.add(arch);
}

function setArchOpen(open) {
  if (!arch || archOpen === open) return;
  archOpen = open;
  const { colMat, gate } = arch.userData;
  colMat.emissiveIntensity = open ? 1.1 : 0.35;
  colMat.opacity = open ? 0.92 : 0.55;
  if (gate?.material) gate.material.opacity = open ? 0.28 : 0.1;
  if (open) {
    showToast(ui.powerToast, '🚪 Ice arch UNLOCKED! Race to it!', 2200);
    announce('Ice arch unlocked');
    if (ui.hintText) ui.hintText.textContent = 'Arch is open — slide through the glowing portal!';
  }
}

// snow
const snowCount = 700;
const snowGeo = new THREE.BufferGeometry();
const snowPositions = new Float32Array(snowCount * 3);
const snowSpeeds = new Float32Array(snowCount);
for (let i = 0; i < snowCount; i++) {
  snowPositions[i * 3] = randRange(-WORLD_SIZE * 0.7, WORLD_SIZE * 0.7);
  snowPositions[i * 3 + 1] = randRange(2, 55);
  snowPositions[i * 3 + 2] = randRange(-WORLD_SIZE * 0.7, WORLD_SIZE * 0.7);
  snowSpeeds[i] = randRange(0.1, 0.4);
}
snowGeo.setAttribute('position', new THREE.BufferAttribute(snowPositions, 3));
const snowSystem = new THREE.Points(
  snowGeo,
  new THREE.PointsMaterial({ color: 0xffffff, size: 0.32, transparent: true, opacity: 0.78 }),
);
scene.add(snowSystem);

// slide spark trail (simple points near penguin)
const sparkCount = 48;
const sparkGeo = new THREE.BufferGeometry();
const sparkPos = new Float32Array(sparkCount * 3);
const sparkLife = new Float32Array(sparkCount);
sparkGeo.setAttribute('position', new THREE.BufferAttribute(sparkPos, 3));
const sparkSystem = new THREE.Points(
  sparkGeo,
  new THREE.PointsMaterial({
    color: 0x9fe8ff,
    size: 0.28,
    transparent: true,
    opacity: 0.85,
    depthWrite: false,
  }),
);
scene.add(sparkSystem);
let sparkIdx = 0;

// physics
 const pState = {
   speed: 0,
   maxSpeed: 22,
   slideBoost: 0,
   acceleration: 35,
   friction: 0.88,
   turnSpeed: 3.8,
   angle: Math.PI,
   radius: 0.7,
   vy: 0,
   y: 0,
  grounded: true,
  waddle: 0,
};

const audio = new AudioEngine();
const _tmpV = new THREE.Vector3();
const _goalV = new THREE.Vector3(0, 0, GOAL_Z);

// ---- gameplay ----
function resetGame() {
  penguin.position.set(0, 0, WORLD_SIZE * 0.45);
  penguin.rotation.set(0, Math.PI, 0);
  pState.speed = 0;
  pState.angle = Math.PI;
  pState.slideBoost = 0;
  pState.vy = 0;
  pState.y = 0;
  pState.grounded = true;
  pState.waddle = 0;

  fishCollected = 0;
  bonusFishCollected = 0;
  score = 0;
  combo = 0;
  bestCombo = 1;
  comboTimer = 0;
  magnetTimer = 0;
  turboTimer = 0;
  pausedMs = 0;
  startTime = performance.now();
  running = true;
  finished = false;
  paused = false;
  jumpQueued = false;

  resetFish();
  resetPowerups();
  createGoal();

  ui.overlay.classList.add('hidden');
  ui.finishScreen.classList.remove('show');
  if (ui.hintText) {
    ui.hintText.textContent = `Collect ${GOAL_NEED} fish to unlock the arch · chain combos for points!`;
  }
  audio.resume();
  announce('Adventure started');
  updateHUD();
}

function updateHUD() {
  const elapsed = (performance.now() - startTime - pausedMs) / 1000;
  const remaining = Math.max(0, TOTAL_TIME - elapsed);
  if (ui.fishEl) ui.fishEl.textContent = `${fishCollected} / ${TOTAL_FISH}`;
  if (ui.timeEl) {
    ui.timeEl.textContent = formatTime(remaining);
    ui.timeEl.style.color = remaining < 30 ? '#ff6b6b' : '';
  }
  if (ui.scoreEl) ui.scoreEl.textContent = String(score);
  if (ui.comboEl) ui.comboEl.textContent = `×${Math.max(1, combo)}`;
  if (ui.comboFill) {
    const pct = combo > 0 ? clamp((comboTimer / COMBO_WINDOW) * 100, 0, 100) : 0;
    ui.comboFill.style.width = `${pct}%`;
  }
  if (ui.comboPanel) {
    ui.comboPanel.classList.toggle('combo-hot', combo >= 3);
    ui.comboPanel.classList.toggle('combo-idle', combo < 3);
  }
}

function addScore(base, isBonus = false) {
  combo = Math.min(combo + 1, 12);
  comboTimer = COMBO_WINDOW;
  if (combo > bestCombo) bestCombo = combo;
  const mult = Math.max(1, combo);
  const gained = Math.round(base * mult * (isBonus ? 1.5 : 1));
  score += gained;
  if (combo >= 4) flashCombo();
  showToast(ui.pickupToast, isBonus ? `💎 +${gained} BONUS!` : `🐟 +${gained}${combo > 1 ? `  ×${combo}` : ''}`, 700);
  return gained;
}

function collectFish(fish) {
  if (fish.userData.collected) return;
  fish.userData.collected = true;
  fish.visible = false;
  fishCollected += 1;
  const isBonus = !!fish.userData.bonus;
  const isRainbow = !!fish.userData.rainbow;
  if (isRainbow) {
    bonusFishCollected += 1;
    audio.rainbow();
    showToast(ui.powerToast, '🌈 RAINBOW FISH! +500! 🌈', 1800);
  } else if (isBonus) {
    bonusFishCollected += 1;
    audio.bonus();
  } else {
    audio.collect(combo + 1);
  }
  addScore(isRainbow ? 500 : (isBonus ? 250 : 100), isBonus || isRainbow);
  announce(`${isRainbow ? 'Rainbow fish' : isBonus ? 'Bonus fish' : 'Fish'} ${fishCollected} of ${TOTAL_FISH}`);

  if (fishCollected >= GOAL_NEED) setArchOpen(true);
  if (fishCollected >= TOTAL_FISH && ui.hintText) {
    ui.hintText.textContent = 'All fish! Dash through the glowing ice arch!';
  }
}

function activatePower(kind) {
  if (kind === 'magnet') {
    magnetTimer = MAGNET_DURATION;
    showToast(ui.powerToast, '🧲 MAGNET — fish fly to you!', 1600);
    announce('Magnet activated');
  } else {
    turboTimer = TURBO_DURATION;
    showToast(ui.powerToast, '⚡ TURBO SLIDE!', 1400);
    announce('Turbo activated');
  }
  audio.powerup();
}

function checkCollisions() {
  const pPos = penguin.position;

  // obstacles (skip while airborne a bit)
  if (pState.y < 1.2) {
    for (const obs of obstacles) {
      const dx = pPos.x - obs.position.x;
      const dz = pPos.z - obs.position.z;
      const dist = Math.hypot(dx, dz);
      const min = pState.radius + obs.radius;
      if (dist > 0.001 && dist < min) {
        const nx = dx / dist;
        const nz = dz / dist;
        pPos.x = obs.position.x + nx * min;
        pPos.z = obs.position.z + nz * min;
        pState.speed *= -0.4;
        audio.bump();
      }
    }
  }

  // world bound
  const bound = WORLD_SIZE * 0.6;
  const dFromCenter = Math.hypot(pPos.x, pPos.z);
  if (dFromCenter > bound) {
    const nx = pPos.x / dFromCenter;
    const nz = pPos.z / dFromCenter;
    pPos.x = nx * bound;
    pPos.z = nz * bound;
    pState.speed *= 0.45;
  }

  // fish
  const pickupR = magnetTimer > 0 ? 2.8 : 2.4;
  for (const fish of fishMeshes) {
    if (fish.userData.collected) continue;
    if (pPos.distanceTo(fish.position) < pickupR) collectFish(fish);
  }

  // powerups
  for (const p of powerups) {
    if (p.userData.collected) continue;
    if (pPos.distanceTo(p.position) < 2.5) {
      p.userData.collected = true;
      p.visible = false;
      activatePower(p.userData.kind);
    }
  }

  // goal
  if (archOpen && pPos.distanceTo(_goalV) < 5.8 && pState.y < 3) {
    endGame(true);
  }
}

function updateMagnet(dt) {
  if (magnetTimer <= 0) return;
  magnetTimer -= dt;
  for (const fish of fishMeshes) {
    if (fish.userData.collected) continue;
    const d = penguin.position.distanceTo(fish.position);
    if (d < MAGNET_RADIUS && d > 0.2) {
      _tmpV.copy(penguin.position).sub(fish.position).normalize().multiplyScalar(dt * (18 + (MAGNET_RADIUS - d) * 1.2));
      fish.position.add(_tmpV);
      fish.position.y = fish.userData.baseY;
    }
  }
}

function emitSparks(sliding) {
  if (!sliding && Math.abs(pState.speed) < 12) return;
  const n = sliding ? 5 : 2;
  for (let i = 0; i < n; i++) {
    const i3 = sparkIdx * 3;
    sparkPos[i3] = penguin.position.x + randRange(-0.4, 0.4);
    sparkPos[i3 + 1] = 0.15 + randRange(0, 0.3);
    sparkPos[i3 + 2] = penguin.position.z + randRange(-0.4, 0.4);
    sparkLife[sparkIdx] = 1;
    sparkIdx = (sparkIdx + 1) % sparkCount;
  }
  sparkSystem.geometry.attributes.position.needsUpdate = true;
}

function updatePenguin(dt) {
  const forward = (keys.ArrowUp || keys.w) ? 1 : (keys.ArrowDown || keys.s) ? -1 : 0;
  const turn = (keys.ArrowLeft || keys.a) ? 1 : (keys.ArrowRight || keys.d) ? -1 : 0;
  const turbo = turboTimer > 0;
  if (turbo) turboTimer -= dt;

  const maxSp = pState.maxSpeed + (turbo ? 15 : 0);
  const sliding = slidePressed && Math.abs(pState.speed) > 2 && pState.grounded;

  if (forward !== 0) {
    const accel = pState.acceleration * (sliding ? 1.5 : 1) * (turbo ? 1.6 : 1);
    pState.speed += forward * accel * dt;
  }

  if (sliding) {
    pState.slideBoost = THREE.MathUtils.lerp(pState.slideBoost, turbo ? 18 : 14, dt * 3);
    pState.speed = clamp(pState.speed, -maxSp * 0.4, maxSp + pState.slideBoost);
    // ice drift — less friction
    pState.speed *= Math.pow(0.955, dt * 60);
  } else {
    pState.slideBoost = THREE.MathUtils.lerp(pState.slideBoost, 0, dt * 3);
    pState.speed = clamp(pState.speed, -maxSp * 0.4, maxSp);
    pState.speed *= Math.pow(pState.friction, dt * 60);
  }
  }
  if (Math.abs(pState.speed) < 0.03) pState.speed = 0;

  // jump
  if (jumpQueued && pState.grounded) {
    pState.vy = 14 + Math.min(Math.abs(pState.speed) * 0.15, 4);
    pState.grounded = false;
    audio.jump();
  }
  jumpQueued = false;

  if (!pState.grounded) {
    pState.vy -= 28 * dt;
    pState.y += pState.vy * dt;
    if (pState.y <= 0) {
      pState.y = 0;
      pState.vy = 0;
      pState.grounded = true;
      audio.land();
    }
  }

  const turnFactor = sliding ? 0.62 : 1.05;
  const steerSign = Math.sign(pState.speed || 1);
  pState.angle += turn * pState.turnSpeed * turnFactor * dt * steerSign;
  penguin.rotation.y = pState.angle;

  const vx = Math.sin(pState.angle) * pState.speed;
  const vz = Math.cos(pState.angle) * pState.speed;
  penguin.position.x += vx * dt;
  penguin.position.z += vz * dt;
  penguin.position.y = pState.y;

  // animation
  const walking = Math.abs(pState.speed) > 0.35 && pState.grounded && !sliding;
  pState.waddle += dt * (Math.abs(pState.speed) * 0.55 + (walking ? 3 : 1.2));
  const flipperAmp = sliding ? 0.08 : walking ? 0.5 : 0.12;
  const { flipperL, flipperR, footL, footR } = penguin.userData;
  flipperL.rotation.z = 0.18 + Math.sin(pState.waddle) * flipperAmp;
  flipperR.rotation.z = -0.18 - Math.sin(pState.waddle) * flipperAmp;
  if (footL && footR) {
    const step = walking ? Math.sin(pState.waddle * 1.2) * 0.25 : 0;
    footL.position.z = 0.35 + step;
    footR.position.z = 0.35 - step;
  }

  const tilt = sliding ? 0.55 : turn * 0.12 * Math.min(1, Math.abs(pState.speed) / 8);
  penguin.rotation.z = THREE.MathUtils.lerp(penguin.rotation.z, tilt, dt * 7);
  penguin.rotation.x = THREE.MathUtils.lerp(
    penguin.rotation.x,
    sliding ? -1.05 : (!pState.grounded ? -0.2 : 0),
    dt * 8,
  );

  if (sliding || turbo) emitSparks(sliding);

  // combo decay
  if (combo > 0) {
    comboTimer -= dt;
    if (comboTimer <= 0) {
      combo = 0;
      comboTimer = 0;
    }
  }

  updateMagnet(dt);
  checkCollisions();
  audio.update(pState.speed);
}

function updateCamera(dt) {
  const back = 12 + Math.min(Math.abs(pState.speed) * 0.25, 6);
  const height = 9 + (slidePressed ? 1.5 : 0) + pState.y * 0.35;
  const lookAhead = 5 + Math.min(Math.abs(pState.speed) * 0.2, 4);
  const ang = pState.angle;
  const desired = new THREE.Vector3(
    penguin.position.x - Math.sin(ang) * back,
    penguin.position.y + height,
    penguin.position.z - Math.cos(ang) * back,
  );
  // slight shoulder offset
  desired.x += Math.cos(ang) * 2.2;
  desired.z -= Math.sin(ang) * 2.2;

  const lerp = clamp(dt * (4.2 + Math.abs(pState.speed) * 0.08), 0, 1);
  camera.position.lerp(desired, lerp);
  const look = new THREE.Vector3(
    penguin.position.x + Math.sin(ang) * lookAhead,
    penguin.position.y + 1.8 + pState.y * 0.2,
    penguin.position.z + Math.cos(ang) * lookAhead,
  );
  camera.lookAt(look);
}

function updateCompass() {
  if (!ui.compassArrow) return;
  let target = null;
  let label = 'fish';

  if (archOpen) {
    target = _goalV;
    label = 'arch';
  } else {
    let best = Infinity;
    for (const f of fishMeshes) {
      if (f.userData.collected) continue;
      const d = penguin.position.distanceToSquared(f.position);
      if (d < best) {
        best = d;
        target = f.position;
      }
    }
  }

  if (!target) {
    ui.compassArrow.style.transform = 'rotate(0deg)';
    if (ui.compassLabel) ui.compassLabel.textContent = 'done';
    return;
  }

  // screen-space angle relative to camera forward on XZ
  const camForward = new THREE.Vector3();
  camera.getWorldDirection(camForward);
  const camYaw = Math.atan2(camForward.x, camForward.z);
  const toTarget = Math.atan2(target.x - penguin.position.x, target.z - penguin.position.z);
  let deg = THREE.MathUtils.radToDeg(toTarget - camYaw);
  ui.compassArrow.style.transform = `rotate(${-deg}deg)`;
  if (ui.compassLabel) ui.compassLabel.textContent = label;
}

function updateEnvironment(dt, time) {
  for (const fish of fishMeshes) {
    if (fish.userData.collected) continue;
    fish.position.y = fish.userData.baseY + Math.sin(time * 2.8 + fish.userData.bobOffset) * 0.35;
    fish.rotation.y += dt * 1.8;
  }
  for (const p of powerups) {
    if (p.userData.collected) continue;
    p.position.y = p.userData.baseY + Math.sin(time * 3.2 + p.userData.bobOffset) * 0.4;
    p.rotation.y += dt * 2.4;
    if (p.userData.ring) p.userData.ring.rotation.z += dt * 3;
  }

  const positions = snowSystem.geometry.attributes.position.array;
  for (let i = 0; i < snowCount; i++) {
    positions[i * 3 + 1] -= snowSpeeds[i];
    positions[i * 3] += Math.sin(time + i) * dt * 0.18;
    if (positions[i * 3 + 1] < 0) {
      positions[i * 3 + 1] = 55;
      positions[i * 3] = penguin.position.x + randRange(-55, 55);
      positions[i * 3 + 2] = penguin.position.z + randRange(-55, 55);
    }
  }
  snowSystem.geometry.attributes.position.needsUpdate = true;

  // spark fade (drift back)
  for (let i = 0; i < sparkCount; i++) {
    if (sparkLife[i] > 0) {
      sparkLife[i] -= dt * 2.5;
      sparkPos[i * 3 + 1] += dt * 0.8;
    }
  }
  sparkSystem.geometry.attributes.position.needsUpdate = true;

  if (arch) {
    arch.userData.particles.children.forEach((p) => {
      p.position.y += Math.sin(time * p.userData.speed + p.userData.phase) * dt * 0.55;
      p.rotation.x += dt;
      p.rotation.y += dt * 0.7;
    });
    if (archOpen) {
      arch.userData.colMat.emissiveIntensity = 0.9 + Math.sin(time * 4) * 0.25;
    }
  }
}

function starRating(won, elapsed) {
  if (!won) return '☆ ☆ ☆';
  let stars = 1;
  if (fishCollected >= TOTAL_FISH) stars++;
  if (elapsed <= 120 || bestCombo >= 6 || bonusFishCollected >= 3) stars++;
  return ['★ ☆ ☆', '★ ★ ☆', '★ ★ ★'][stars - 1] || '★ ☆ ☆';
}

function endGame(won) {
  if (finished) return;
  finished = true;
  running = false;
  const elapsed = (performance.now() - startTime - pausedMs) / 1000;
  if (won) {
    const timeBonus = Math.max(0, Math.round((TOTAL_TIME - elapsed) * 8));
    const fishBonus = fishCollected * 20;
    score += timeBonus + fishBonus;
    audio.win();
  } else {
    audio.lose();
  }

  ui.finishTitle.textContent = won ? '🎉 Slide Complete! 🐧' : '🥶 Time is up!';
  if (ui.finishTagline) {
    ui.finishTagline.textContent = won
      ? (bestCombo >= 6 ? 'Combo king of the ice!' : 'Belly-slide excellence!')
      : 'The ice waits… try chaining combos faster!';
  }
  if (ui.starsEl) ui.starsEl.textContent = starRating(won, elapsed);
  ui.statFish.textContent = `${fishCollected} / ${TOTAL_FISH}`;
  ui.statTime.textContent = formatTime(elapsed);
  ui.statBonus.textContent = String(bonusFishCollected);
  if (ui.statCombo) ui.statCombo.textContent = `×${bestCombo}`;
  if (ui.statScore) ui.statScore.textContent = String(score);
  ui.finishScreen.classList.add('show');
  audio.stop();
  announce(won ? 'Level complete' : 'Time is up');
}

function checkTimeLimit() {
  if (!running) return;
  const elapsed = (performance.now() - startTime - pausedMs) / 1000;
  if (elapsed >= TOTAL_TIME) endGame(false);
}

function togglePause() {
  if (!running || finished) return;
  paused = !paused;
  if (paused) {
    pauseStart = performance.now();
    ui.overlay.classList.remove('hidden');
    const h1 = ui.overlay.querySelector('h1');
    if (h1) h1.textContent = '⏸️ Paused';
    if (ui.startBtn) ui.startBtn.textContent = 'Resume';
    audio.stop();
    announce('Game paused');
  } else {
    pausedMs += performance.now() - pauseStart;
    ui.overlay.classList.add('hidden');
    const h1 = ui.overlay.querySelector('h1');
    if (h1) h1.textContent = '🐧 Pinguin Slide Party';
    if (ui.startBtn) ui.startBtn.textContent = 'Start Sliding!';
    audio.resume();
    announce('Game resumed');
  }
}

function animate(now) {
  requestAnimationFrame(animate);
  const dt = Math.min((now - lastTime) / 1000, 0.1);
  lastTime = now;
  const time = now * 0.001;

  if (running && !finished && !paused) {
    updatePenguin(dt);
    updateCamera(dt);
    updateEnvironment(dt, time);
    updateCompass();
    checkTimeLimit();
    updateHUD();
  } else {
    updateEnvironment(dt, time);
    if (!running) {
      // idle orbit on title
      const t = time * 0.35;
      camera.position.x = Math.sin(t) * 22;
      camera.position.z = Math.cos(t) * 22 + WORLD_SIZE * 0.2;
      camera.position.y = 14;
      camera.lookAt(penguin.position.x, 2, penguin.position.z);
    }
  }

  renderer.render(scene, camera);
}
requestAnimationFrame(animate);

// ---- input ----
window.addEventListener('keydown', (e) => {
  if (Object.prototype.hasOwnProperty.call(keys, e.key)) keys[e.key] = true;
  if (e.key === 'p' || e.key === 'P' || e.key === 'Escape') togglePause();
  if (e.key === ' ' && running) {
    slidePressed = true;
    e.preventDefault();
  }
  if ((e.key === 'Shift' || e.code === 'ShiftLeft' || e.code === 'ShiftRight') && running) {
    jumpQueued = true;
    e.preventDefault();
  }
});

window.addEventListener('keyup', (e) => {
  if (Object.prototype.hasOwnProperty.call(keys, e.key)) keys[e.key] = false;
  if (e.key === ' ') slidePressed = false;
});

function bindTouch(id, keyName, opts = {}) {
  const btn = document.getElementById(id);
  if (!btn) return;
  const set = (v) => {
    if (opts.slide) slidePressed = v;
    else if (opts.jump) { if (v) jumpQueued = true; }
    else keys[keyName] = v;
  };
  ['touchstart', 'mousedown'].forEach((evt) => {
    btn.addEventListener(evt, (e) => { e.preventDefault(); set(true); }, { passive: false });
  });
  ['touchend', 'touchcancel', 'mouseup', 'mouseleave'].forEach((evt) => {
    btn.addEventListener(evt, (e) => { e.preventDefault(); set(false); });
  });
}

bindTouch('touch-up', 'ArrowUp');
bindTouch('touch-down', 'ArrowDown');
bindTouch('touch-left', 'ArrowLeft');
bindTouch('touch-right', 'ArrowRight');
bindTouch('touch-slide', '', { slide: true });
bindTouch('touch-jump', '', { jump: true });

initSettings({
  renderer,
  restart: () => {
    // resume if paused via start button
    if (paused && running && !finished) {
      togglePause();
      return;
    }
    resetGame();
  },
});

window.addEventListener('resize', () => {
  camera.aspect = window.innerWidth / window.innerHeight;
  camera.updateProjectionMatrix();
  renderer.setSize(window.innerWidth, window.innerHeight);
});

// decorative start pose
penguin.position.set(0, 0, WORLD_SIZE * 0.45);
penguin.rotation.y = Math.PI;
createGoal();
resetFish();
resetPowerups();
