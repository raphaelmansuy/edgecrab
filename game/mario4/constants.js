// ═══════════════════════════════════════════════════════════════
//  SUPER MARIO 4 — EdgeCrab Edition  |  constants.js
// ═══════════════════════════════════════════════════════════════
const TILE = 40;
const GRAVITY = 0.56;  // Slightly reduced for floatier feel
const JUMP_FORCE = -14.2;  // Stronger jump for better control
const WALK_SPEED = 3.8;
const RUN_SPEED = 7.0;  // Slightly faster
const FRICTION = 0.82;  // Better grip on ground
const AIR_FRICTION = 0.94;  // Tighter air control
const MAX_FALL = 17;  // Slightly lower max fall for control
const WALL_SLIDE_SPEED = 1.5;  // How fast player slides down walls
const DASH_FORCE = 2.2;  // Dash acceleration
const W = 900, H = 520;

// ── Camera shake for impact feedback ──────────────────────────
let cameraShakeX = 0, cameraShakeY = 0, cameraShakeIntensity = 0;

// ── Web Audio Engine ──────────────────────────────────────────
let audioCtx = null;
function getAudio() {
  if (!audioCtx) {
    try { audioCtx = new (window.AudioContext || window.webkitAudioContext)(); } catch(e) {}
  }
  return audioCtx;
}
function playTone(freq, type='square', dur=0.08, vol=0.18, delay=0) {
  const ac = getAudio(); if (!ac) return;
  try {
    const o = ac.createOscillator(), g = ac.createGain();
    o.connect(g); g.connect(ac.destination);
    o.type = type; o.frequency.setValueAtTime(freq, ac.currentTime + delay);
    g.gain.setValueAtTime(vol, ac.currentTime + delay);
    g.gain.exponentialRampToValueAtTime(0.001, ac.currentTime + delay + dur);
    o.start(ac.currentTime + delay);
    o.stop(ac.currentTime + delay + dur + 0.01);
  } catch(e) {}
}
function sfxJump()   { playTone(320,'square',0.12,0.22); playTone(520,'square',0.07,0.15,0.06); }
function sfxCoin()   { playTone(880,'sine',0.08,0.25); playTone(1100,'sine',0.06,0.2,0.06); }
function sfxStomp()  { playTone(180,'square',0.1,0.3); }
function sfxDie()    { [400,330,280,200].forEach((f,i)=>playTone(f,'sawtooth',0.12,0.3,i*0.09)); }
function sfxPowerup(){ [300,400,500,700,'sine'].length && [300,400,500,700].forEach((f,i)=>playTone(f,'sine',0.1,0.28,i*0.07)); }
function sfxBrick()  { playTone(200,'sawtooth',0.08,0.25); }
function sfxFlag()   { [523,659,784,1047].forEach((f,i)=>playTone(f,'sine',0.18,0.3,i*0.1)); }
function sfxFireball(){ playTone(600,'sawtooth',0.06,0.2); }
function sfxStar()   { [523,784,1047,1568].forEach((f,i)=>playTone(f,'sine',0.12,0.25,i*0.05)); }
function sfxHurt()   { playTone(160,'sawtooth',0.15,0.35); }
function sfxKick()   { playTone(250,'square',0.09,0.28); playTone(150,'square',0.07,0.2,0.05); }
function sfxCombo(n) { playTone(300+n*120,'sine',0.1,0.3); }

// ── Keyboard Input ────────────────────────────────────────────
const keys = {};
document.addEventListener('keydown', e => {
  keys[e.code] = true;
  if (['Space','ArrowUp','ArrowDown','ArrowLeft','ArrowRight'].includes(e.code)) e.preventDefault();
  if (e.code === 'KeyP' && (gameState === 'playing' || gameState === 'paused')) togglePause();
});
document.addEventListener('keyup', e => { keys[e.code] = false; });

// ── Touch Input ───────────────────────────────────────────────
const touch = { left:false, right:false, jump:false, run:false, fire:false };

// ── Globals ───────────────────────────────────────────────────
let gameState = 'menu';
let score = 0, lives = 3, level = 1;
let frameCount = 0;
let cameraX = 0;
let particles = [];
let floatingTexts = [];
let fireballs = [];
let tileMap = {};
let enemies = [];
let coins = [];
let powerups = [];
let currentLevel = null;
let mapPixelW = 0;
let paused = false;
let comboCount = 0;
let comboTimer = 0;

// Animated blocks (? block bounce)
let animBlocks = [];

const flag = { x:0, y:0, captured:false, slideY:0 };

const player = {
  x:60, y:360, w:28, h:36,
  vx:0, vy:0, onGround:false, facingRight:true,
  animFrame:0, animTimer:0, jumping:false, dead:false,
  invincible:0, big:false, jumpBufferTimer:0, coyoteTimer:0,
  star:0, fire:false, fireTimer:0,
  onWall:false, wallSlideDir:0,
  dashCooldown:0, dashDir:0,
  squash:1, trailTimer:0, prevY:360,
};
