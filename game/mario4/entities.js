// ═══════════════════════════════════════════════════════════════
//  entities.js  –  player, enemies, coins, powerups, fireballs
// ═══════════════════════════════════════════════════════════════

// ── Particles / Floating Text ────────────────────────────────
function spawnParticles(x, y, color, count = 8) {
  for (let i = 0; i < count; i++) {
    const angle = (Math.PI * 2 * i) / count + Math.random() * 0.5;
    const speed = 2 + Math.random() * 5;
    particles.push({
      x, y, vx: Math.cos(angle)*speed, vy: Math.sin(angle)*speed - 2.5,
      life: 1, decay: 0.022 + Math.random()*0.018, r: 3 + Math.random()*5, color
    });
  }
}
function spawnFloatingText(x, y, text, color = '#ffd700') {
  floatingTexts.push({ x, y, text, color, vy: -1.8, life: 1, decay: 0.02 });
}

// ── Player ─────────────────────────────────────────────────
function resetPlayer() {
   player.x = 60; player.y = 340;
   player.vx = 0; player.vy = 0;
   player.onGround = false; player.dead = false; player.invincible = 80;
   player.jumpBufferTimer = 0; player.coyoteTimer = 0;
   player.big = false; player.h = 36;
   player.star = 0; player.fire = false; player.fireTimer = 0;
   player.onWall = false; player.wallSlideDir = 0;
   player.dashCooldown = 0; player.dashDir = 0;
   player.prevY = player.y;        // for landing detection
   player.trailTimer = 0;           // speed trail cooldown
   player.squash = 1;               // squash/stretch scalar
}

function updatePlayer() {
   const left  = keys['ArrowLeft']  || keys['KeyA'] || touch.left;
   const right = keys['ArrowRight'] || keys['KeyD'] || touch.right;
   const running = keys['ShiftLeft'] || keys['ShiftRight'] || keys['KeyZ'] || touch.run;
   const jumping  = keys['Space'] || keys['ArrowUp'] || keys['KeyW'] || touch.jump;
   const firing   = keys['KeyX'] || keys['KeyF'] || touch.fire;

   if (player.dead) {
     player.vy += GRAVITY;
     player.y  += player.vy;
     return;
   }

   const spd = running ? RUN_SPEED : WALK_SPEED;

   // ── Horizontal movement with better acceleration ────
   if (left) {
     player.vx -= player.onGround ? 1.8 : 1.2;
     if (player.vx < -spd) player.vx = -spd;
     player.facingRight = false;
   } else if (right) {
     player.vx += player.onGround ? 1.8 : 1.2;
     if (player.vx > spd) player.vx = spd;
     player.facingRight = true;
   } else {
     player.vx *= player.onGround ? FRICTION : AIR_FRICTION;
     if (Math.abs(player.vx) < 0.1) player.vx = 0;
   }

   // Jump buffer / coyote time (improved responsiveness)
   if (jumping) player.jumpBufferTimer = 12;
   else player.jumpBufferTimer--;
   if (player.onGround) player.coyoteTimer = 12; else player.coyoteTimer--;

   // ── Jump with squash anticipation + dust ──────────────────
   const wasOnGround = player.onGround;
   if (player.jumpBufferTimer > 0 && player.coyoteTimer > 0) {
      player.vy = JUMP_FORCE;
      player.jumping = true; player.onGround = false;
      player.jumpBufferTimer = 0; player.coyoteTimer = 0;
      sfxJump();
      cameraShakeIntensity = 1.5;
      // Squash before jump stretch
      player.squash = 0.7;
      // Jump dust
      spawnParticles(player.x + player.w/2, player.y + player.h, '#e8d8b8', 6);
   }

   // ── Wall jump ───────────────────────────────────────────────
   if (player.onWall && jumping && player.coyoteTimer <= 0) {
      player.vy = JUMP_FORCE * 0.9;
      player.vx = -player.wallSlideDir * 9;
      player.jumping = true;
      player.jumpBufferTimer = 0; player.coyoteTimer = 8;
      player.facingRight = player.wallSlideDir < 0;
      sfxJump();
      cameraShakeIntensity = 2;
      player.squash = 0.7;
      spawnParticles(
        player.x + (player.wallSlideDir < 0 ? player.w : 0),
        player.y + player.h/2,
        '#e8d8b8', 6
      );
   }

   // Variable jump height (more control)
   if (player.jumping && !jumping && player.vy < -4) player.vy += 2.5;
   if (player.vy >= 0) player.jumping = false;

    // ── Wall slide detection + dust ────────────────────────────
    player.onWall = false; player.wallSlideDir = 0;
    if (!player.onGround && player.vy > 0) {
      const checkDist = 18;
      const wallLeft = Math.floor((player.x - checkDist) / TILE);
      const wallRight = Math.floor((player.x + player.w + checkDist) / TILE);
      const wallRow = Math.floor((player.y + player.h / 2) / TILE);

      if (isSolid(getTile(wallLeft, wallRow)) && player.vx < 0) {
        player.onWall = true; player.wallSlideDir = -1;
        player.vy *= 0.92;
        if (player.vy > WALL_SLIDE_SPEED) player.vy = WALL_SLIDE_SPEED;
        // Wall slide dust
        if (frameCount % 8 === 0) {
           spawnParticles(player.x, player.y + player.h/2, '#e8d8b8', 2);
        }
      } else if (isSolid(getTile(wallRight, wallRow)) && player.vx > 0) {
        player.onWall = true; player.wallSlideDir = 1;
        player.vy *= 0.92;
        if (player.vy > WALL_SLIDE_SPEED) player.vy = WALL_SLIDE_SPEED;
        if (frameCount % 8 === 0) {
           spawnParticles(player.x + player.w, player.y + player.h/2, '#e8d8b8', 2);
        }
      }
    }

   // Gravity
   player.vy += GRAVITY;
   if (player.vy > MAX_FALL) player.vy = MAX_FALL;

   // Map bounds
   if (player.x < 0) { player.x = 0; player.vx = 0; }
   if (player.x + player.w > mapPixelW) { player.x = mapPixelW - player.w; player.vx = 0; }

   resolveEntityTiles(player);

   // ── Landing impact ───────────────────────────────────────────
   if (player.onGround && !wasOnGround && player.vy >= 0) {
      const intensity = Math.min(Math.abs(player.vy) / 12, 1);
      if (intensity > 0.25) {
         spawnParticles(player.x + player.w/2, player.y + player.h, '#e8d8b8', Math.ceil(4 + intensity * 5));
         cameraShakeIntensity = Math.max(cameraShakeIntensity, 2 * intensity);
         if (intensity > 0.5) sfxKick(); // heavy landing
      }
      player.squash = 0.65; // squash on landing
   }

   // ── Squash/stretch recovery ───────────────────────────────────
   player.squash += (1 - player.squash) * 0.18;
   if (Math.abs(player.squash - 1) < 0.005) player.squash = 1;

   // ── Speed trail particles ────────────────────────────────────
   const speed = Math.abs(player.vx);
   if (player.onGround && speed > WALK_SPEED * 1.3) {
      player.trailTimer++;
      if (player.trailTimer % 4 === 0) {
         particles.push({
            x: player.x + player.w/2 - player.facingRight ? 10 : -6,
            y: player.y + player.h - 6,
            vx: (Math.random() - 0.5) * 0.8,
            vy: -0.5 - Math.random() * 1.2,
            life: 0.6, decay: 0.04,
            r: 3 + Math.random() * 4, color: '#e8d8b8'
         });
      }
   }

   if (player.y > H + 120) killPlayer();

   if (player.invincible > 0) player.invincible--;
   if (player.star > 0) { player.star--; if (player.star === 0) player.invincible = 0; }

   // Fire cooldown
   if (player.fireTimer > 0) player.fireTimer--;
   if (firing && player.fire && player.fireTimer === 0) {
     shootFireball();
     player.fireTimer = 18;  // Faster fire rate
   }

   // Dash cooldown
   if (player.dashCooldown > 0) player.dashCooldown--;

   // Animation
   player.animTimer++;
   if (player.onGround && Math.abs(player.vx) > 0.3) {
     if (player.animTimer > 5) { player.animFrame = (player.animFrame + 1) % 3; player.animTimer = 0; } // Faster animation
   } else {
     player.animFrame = player.onGround ? 0 : 2;
   }
}

function killPlayer() {
  if (player.dead || player.invincible > 0) return;
  if (player.star > 0) return; // star = invincible
  if (player.big || player.fire) {
    // Shrink rather than die
    player.big = false; player.fire = false; player.h = 36;
    player.invincible = 120;
    sfxHurt();
    spawnParticles(player.x + 14, player.y + 18, '#ff8888', 14);
    return;
  }
  player.dead = true; player.vy = -12;
  spawnParticles(player.x + 14, player.y + 18, '#e74c3c', 20);
  sfxDie();
  lives--; updateHUD();
  setTimeout(() => {
    if (lives <= 0) { gameState = 'gameover'; showOverlay('gameover'); }
    else { loadLevel(level - 1); gameState = 'playing'; updateHUD(); }
  }, 1800);
}

// ── Fireballs ────────────────────────────────────────────────
function shootFireball() {
  sfxFireball();
  fireballs.push({
    x: player.x + (player.facingRight ? player.w : -12),
    y: player.y + player.h / 2 - 6,
    w: 12, h: 12,
    vx: player.facingRight ? 10 : -10,
    vy: -3, active: true, life: 120,
  });
}

function updateFireballs() {
  fireballs.forEach(fb => {
    if (!fb.active) return;
    fb.vy += GRAVITY * 0.5;
    if (fb.vy > 8) fb.vy = 8;
    fb.x += fb.vx;
    fb.life--;
    if (fb.life <= 0 || fb.x < 0 || fb.x > mapPixelW) { fb.active = false; return; }
    resolveFireballTiles(fb);
    // Hit enemies
    enemies.forEach(e => {
      if (!e.alive || !fb.active) return;
      if (aabb(fb, e)) {
        e.alive = false;
        fb.active = false;
        score += 200;
        addCombo();
        spawnParticles(e.x + e.w/2, e.y + e.h/2, '#ff6600', 12);
        spawnFloatingText(e.x, e.y - 20, '+200 🔥', '#ff6600');
      }
    });
  });
  fireballs = fireballs.filter(fb => fb.active);
}

// ── Enemies ─────────────────────────────────────────────────
function spawnEnemies(levelData) {
  enemies = levelData.enemies.map(e => ({
    type: e.type,
    x: e.tx * TILE, y: e.ty * TILE,
    w: e.type === 'koopa' ? 30 : 28,
    h: e.type === 'koopa' ? 38 : 28,
    vx: e.type === 'piranha' ? 0 : -1.6, vy: 0,
    onGround: false, alive: true, stomped: false, stompTimer: 0,
    animFrame: 0, animTimer: 0,
    spawnX: e.tx * TILE,
    piranhaBaseY: e.ty * TILE - TILE,
    piranhaTimer: Math.random() * Math.PI * 2,
    shellVx: 0, isShell: false,
    facingRight: false,
    patrolRange: 5 * TILE, // goombas/koopas patrol ±5 tiles from spawn
  }));
}

function updateEnemies() {
  enemies.forEach(e => {
    if (!e.alive) return;

    if (e.type === 'piranha') {
      e.piranhaTimer += 0.04;
      e.y = e.piranhaBaseY + Math.sin(e.piranhaTimer) * (TILE * 0.9);
      e.animTimer++; if (e.animTimer > 8) { e.animFrame = (e.animFrame+1)%4; e.animTimer=0; }
      return;
    }

    if (e.stomped && !e.isShell) {
      e.stompTimer--;
      if (e.stompTimer <= 0) {
        spawnParticles(e.x + e.w/2, e.y + e.h/2, '#c87830', 8);
        e.alive = false;
      }
      return;
    }

    // Shell motion
    if (e.isShell) e.vx = e.shellVx;

    // Gravity
    e.vy += GRAVITY;
    if (e.vy > MAX_FALL) e.vy = MAX_FALL;

    // Patrol: reverse at patrol boundary (only non-shell)
    if (!e.isShell) {
      if (e.x < e.spawnX - e.patrolRange) { e.vx = Math.abs(e.vx); e.facingRight = true; }
      if (e.x > e.spawnX + e.patrolRange) { e.vx = -Math.abs(e.vx); e.facingRight = false; }
    }

    resolveEntityTiles(e);
    if (e.y > H + 100) { e.alive = false; return; }
    e.animTimer++; if (e.animTimer > 8) { e.animFrame = (e.animFrame+1)%2; e.animTimer=0; }
  });
}

// ── Coins ────────────────────────────────────────────────────
function spawnCoins(ld) {
  coins = ld.coins.map(c => ({ ...c, active:true, bobTimer:Math.random()*Math.PI*2 }));
}

function updateCoins() {
  coins.forEach(c => {
    if (!c.active) return;
    c.bobTimer += 0.07;
    const cr = { x:c.x, y:c.y + Math.sin(c.bobTimer)*4, w:c.w, h:c.h };
    if (aabb(player, cr)) {
      c.active = false; score += 200;
      spawnParticles(c.x+8, c.y, '#ffd700', 8);
      spawnFloatingText(c.x, c.y, '+200', '#ffd700');
      sfxCoin();
    }
  });
}

// ── Powerups ─────────────────────────────────────────────────
function spawnPowerup(x, y, type='mushroom') {
  powerups.push({ x, y, w:28, h:28, vx:1.8, vy:-4, active:true, type });
}

function updatePowerups() {
  powerups.forEach(p => {
    if (!p.active) return;
    p.vy += GRAVITY;
    if (p.vy > MAX_FALL) p.vy = MAX_FALL;
    resolveEntityTiles(p);
    if (!aabb(player, p)) return;
    p.active = false;
     sfxPowerup();
     if (p.type === 'mushroom') {
       player.big = true; player.h = 44; score += 1000;
       spawnFloatingText(player.x, player.y, '+1000 BIG!', '#e74c3c');
       spawnParticles(player.x+14, player.y, '#ff4444', 14);
       triggerFlash('#e74c3c', 0.3);
     } else if (p.type === 'fireflower') {
       player.big = true; player.fire = true; player.h = 44; score += 1000;
       spawnFloatingText(player.x, player.y, '+1000 FIRE!', '#ff6600');
       spawnParticles(player.x+14, player.y, '#ff6600', 14);
       triggerFlash('#ff6600', 0.3);
     } else if (p.type === 'star') {
       player.star = 480; // ~8 seconds at 60fps
       score += 2000;
       sfxStar();
       spawnFloatingText(player.x, player.y, '+2000 STAR!', '#ffe000');
       spawnParticles(player.x+14, player.y, '#ffe000', 20);
       triggerFlash('#ffe000', 0.5);
     }
  });
}

// ── Combo system ─────────────────────────────────────────────
function addCombo() {
  comboCount++;
  comboTimer = 120; // reset 2-second window
  if (comboCount > 1) sfxCombo(comboCount);
}

function updateCombo() {
  if (comboTimer > 0) { comboTimer--; }
  else { comboCount = 0; }
}

// ── Player–Enemy collisions ───────────────────────────────────
function checkPlayerEnemyCollision() {
  if (player.dead) return;
  // Star kills everything on touch
  const starKill = player.star > 0;

  enemies.forEach(e => {
    if (!e.alive) return;
    if (!aabb(player, e)) return;

    if (starKill && player.invincible > 0) {
      // star invincibility – player invincible is set to big value
    }
    if (player.star > 0) {
      e.alive = false;
      score += 500 * comboCount || 500;
      addCombo();
      spawnParticles(e.x+e.w/2, e.y+e.h/2, '#ffe000', 14);
      spawnFloatingText(e.x, e.y-20, '+500 ⭐', '#ffe000');
      return;
    }

    if (player.invincible > 0) return;

    const stompEdge = e.y + e.h * 0.55;
    const stomping  = player.vy > 0 && player.y + player.h < stompEdge;

     if (stomping) {
        if (e.type === 'koopa' && !e.isShell) {
          e.isShell = true; e.h = 24; e.y += 14; e.shellVx = 0; e.vx = 0;
          score += 200 * (comboCount||1); addCombo();
          sfxStomp();
          spawnFloatingText(e.x, e.y-20, `+${200*(comboCount||1)} SHELL`, '#7efff5');
          cameraShakeIntensity = 3;
          triggerFlash('#7efff5', 0.25);
        } else if (e.isShell) {
          e.shellVx = player.facingRight ? 9 : -9;
          score += 400; addCombo(); sfxKick();
          spawnFloatingText(e.x, e.y-20, '+400 KICK!', '#7efff5');
          cameraShakeIntensity = 3;
          triggerFlash('#7efff5', 0.3);
        } else {
          e.alive = false; e.stomped = true; e.stompTimer = 20;
          const pts = 100 * Math.min(comboCount+1, 8);
          score += pts; addCombo(); sfxStomp();
          spawnParticles(e.x+e.w/2, e.y+e.h/2, '#c87830', 12);
          spawnFloatingText(e.x, e.y-20, `+${pts}`, '#ffd700');
          cameraShakeIntensity = 2;
          triggerFlash('#ffd700', 0.2);
        }
        player.vy = -11;  // Stronger bounce
        player.squash = 0.65; // squash on stomp too
     } else {
       if (e.isShell && e.shellVx === 0) return;
       killPlayer();
     }
  });

  // Shell chain kills
  enemies.forEach(shell => {
    if (!shell.alive || !shell.isShell || shell.shellVx === 0) return;
    enemies.forEach(e => {
      if (!e.alive || e === shell || !aabb(shell, e)) return;
      e.alive = false; score += 500; addCombo();
      spawnParticles(e.x+e.w/2, e.y, '#e74c3c', 12);
      spawnFloatingText(e.x, e.y-20, '+500 CHAIN!', '#e74c3c');
    });
  });
}

// ── Flag ─────────────────────────────────────────────────────
function setupFlag(ld) {
  flag.x = ld.flagX * TILE;
  flag.y = 3 * TILE;
  flag.captured = false;
  flag.slideY = 3 * TILE;
}

function checkFlagCollision() {
  if (flag.captured) return;
  const fr = { x: flag.x-12, y: flag.y, w:30, h: H - flag.y };
  if (!aabb(player, fr)) return;
  flag.captured = true;
  const bonus = Math.max(100, Math.floor((player.y / H) * 3000)); // higher = more bonus
  score += 3000 + bonus;
  sfxFlag();
  spawnParticles(flag.x, flag.y, '#ffd700', 30);
  spawnFloatingText(flag.x-40, flag.y-20, '+' + (3000+bonus) + ' GOAL!', '#ffd700');
  gameState = 'flagcapture';
  setTimeout(() => {
    if (level < LEVELS.length) {
      level++; loadLevel(level-1); updateHUD(); gameState = 'playing';
    } else { gameState = 'win'; showOverlay('win'); }
  }, 2400);
}
