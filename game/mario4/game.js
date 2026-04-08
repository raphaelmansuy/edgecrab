// ═══════════════════════════════════════════════════════════════
//  game.js  –  main loop, level loader, HUD, overlay, touch
// ═══════════════════════════════════════════════════════════════

// ── Level Loader ─────────────────────────────────────────────
function loadLevel(idx) {
  currentLevel = LEVELS[idx];
  mapPixelW    = currentLevel.mapW * TILE;
  buildTileMap(currentLevel);
  spawnEnemies(currentLevel);
  spawnCoins(currentLevel);
  powerups  = [];
  fireballs = [];
  particles = [];
  floatingTexts = [];
  animBlocks = [];
  comboCount = 0; comboTimer = 0;
  resetPlayer();
  cameraX = 0;
  setupFlag(currentLevel);
  updateHUD();
}

// ── Camera ───────────────────────────────────────────────────
function updateCamera() {
  // Lead the player by 1/3 of screen ahead in movement direction
  const lead = player.facingRight ? W * 0.38 : W * 0.28;
  const target = player.x - lead;
  cameraX += (target - cameraX) * 0.10;  // Slightly smoother
  cameraX = Math.max(0, Math.min(cameraX, mapPixelW - W));

  // ── Camera shake decay ────────────────────────────────────
  if (cameraShakeIntensity > 0) {
    cameraShakeX = (Math.random() - 0.5) * cameraShakeIntensity;
    cameraShakeY = (Math.random() - 0.5) * cameraShakeIntensity;
    cameraShakeIntensity *= 0.92;
  } else {
    cameraShakeX = 0; cameraShakeY = 0;
  }
}

// ── Particle / Text tick ─────────────────────────────────────
function updateParticles() {
  particles.forEach(p => { p.x+=p.vx; p.y+=p.vy; p.vy+=0.22; p.life-=p.decay; });
  particles = particles.filter(p => p.life > 0);
}
function updateFloatingTexts() {
  floatingTexts.forEach(t => { t.y+=t.vy; t.life-=t.decay; });
  floatingTexts = floatingTexts.filter(t => t.life > 0);
}

// ── HUD ──────────────────────────────────────────────────────
function updateHUD() {
  const lStr = lives > 0 ? ('♥ '.repeat(Math.min(lives,5))).trim() : '☠';
  document.getElementById('lives-display').textContent = lStr;
  document.getElementById('score-display').textContent = `SCORE: ${String(score).padStart(7,'0')}`;
  document.getElementById('level-display').textContent = `${currentLevel ? currentLevel.name : 'LEVEL '+level}`;
}

// ── Pause ────────────────────────────────────────────────────
function togglePause() {
  paused = !paused;
  gameState = paused ? 'paused' : 'playing';
}

// ── Overlay helper ───────────────────────────────────────────
function showOverlay(type) {
  const ov  = document.getElementById('overlay');
  const h1  = ov.querySelector('h1');
  const sub = ov.querySelector('.sub');
  const btn = document.getElementById('startBtn');
  ov.style.display = 'flex';
  if (type === 'gameover') {
    h1.textContent  = 'GAME OVER'; h1.style.color = '#e74c3c';
    sub.textContent = `Final Score: ${String(score).padStart(7,'0')}`;
    btn.textContent = '↺  TRY AGAIN'; btn.onclick = startGame;
  } else if (type === 'win') {
    h1.textContent  = '🎉  YOU WIN!'; h1.style.color = '#ffd700';
    sub.textContent = `Final Score: ${String(score).padStart(7,'0')}  ★  Congratulations, Raphael!`;
    btn.textContent = '↺  PLAY AGAIN'; btn.onclick = startGame;
  }
}

// ── Start ────────────────────────────────────────────────────
function startGame() {
  getAudio(); // unlock audio context on first user gesture
  document.getElementById('overlay').style.display = 'none';
  score = 0; lives = 3; level = 1; paused = false;
  loadLevel(0); gameState = 'playing';
}

// ── Touch Controls Setup ─────────────────────────────────────
(function setupTouchControls() {
  const btnSz = 60, pad = 16;
  const BTNS = [
    { id:'left',  cx: pad+btnSz/2,          cy: H-pad-btnSz/2 },
    { id:'right', cx: pad+btnSz*1.5+10,     cy: H-pad-btnSz/2 },
    { id:'run',   cx: W-pad-btnSz*1.5-10,   cy: H-pad-btnSz/2 },
    { id:'jump',  cx: W-pad-btnSz/2,         cy: H-pad-btnSz/2 },
    { id:'fire',  cx: W-pad-btnSz/2,         cy: H-pad-btnSz*1.5-10 },
  ];

  function touchBtn(tx, ty) {
    return BTNS.find(b => Math.hypot(tx-b.cx, ty-b.cy) < 36);
  }

  function applyTouch(e, val) {
    const rect = canvas.getBoundingClientRect();
    const scaleX = W / rect.width, scaleY = H / rect.height;
    Array.from(e.changedTouches).forEach(t => {
      const tx = (t.clientX - rect.left) * scaleX;
      const ty = (t.clientY - rect.top)  * scaleY;
      const b = touchBtn(tx, ty);
      if (b) touch[b.id] = val;
    });
  }

  canvas.addEventListener('touchstart', e => { e.preventDefault(); applyTouch(e, true);  }, {passive:false});
  canvas.addEventListener('touchend',   e => { e.preventDefault(); applyTouch(e, false); }, {passive:false});
  canvas.addEventListener('touchcancel',e => { e.preventDefault(); applyTouch(e, false); }, {passive:false});
})();

// ── Main Loop ────────────────────────────────────────────────
function gameLoop() {
  requestAnimationFrame(gameLoop);
  if (gameState !== 'playing' && gameState !== 'paused' && gameState !== 'flagcapture') return;
  frameCount++;

  // Update
  if (gameState === 'playing' || gameState === 'flagcapture') {
    updatePlayer();
    updateEnemies();
    updateCoins();
    updatePowerups();
    updateFireballs();
    updateParticles();
    updateFloatingTexts();
    updateCombo();
    updateAnimBlocks();
    if (gameState === 'playing') {
      checkPlayerEnemyCollision();
      checkFlagCollision();
    }
    updateCamera();
    updateHUD();
  }

  // Draw
  drawBackground(currentLevel);
  drawTiles(currentLevel);
  drawAnimBlocks();
  drawFlag();
  drawCoins();
  drawPowerups();
  drawFireballs();
  drawEnemies();
  drawPlayer();
  drawParticles();
  drawFloatingTexts();
  drawCombo();
  drawScreenFlash();
  drawStarTimer();
  drawProgressBar();
  drawTouchControls();
  if (gameState === 'paused') drawPauseOverlay();
}

document.getElementById('startBtn').onclick = startGame;
requestAnimationFrame(gameLoop);
