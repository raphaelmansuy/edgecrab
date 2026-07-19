import * as THREE from 'three';
import {
  ui, prefs, applyQuality, initSettings, announce, formatTime,
  showToast, flashCombo, AudioEngine,
} from './ui.js';

// ---- constants ----
const WORLD_SIZE = 160;
const TOTAL_FISH = 20;
const GOAL_NEED = 15;
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
const magnetMat = new THREE.MeshStandardMaterial({
  color: 0x66ffcc, emissive: 0x00ffaa, emissiveIntensity: 0.7, roughness: 0.25, metalness: 0.5,
});
const turboMat = new THREE.MeshStandardMaterial({
  color: 0xaaccff, emissive: 0x4488ff, emissiveIntensity: 0.85, roughness: 0.2, metalness: 0.55,
});

// PLACEHOLDER_CONTINUE
