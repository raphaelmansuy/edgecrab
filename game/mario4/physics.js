// ═══════════════════════════════════════════════════════════════
//  physics.js  –  tile map, collision resolution, tile actions
// ═══════════════════════════════════════════════════════════════

function buildTileMap(levelData) {
  tileMap = {};
  levelData.tiles.forEach(([c, r, type]) => { tileMap[`${c},${r}`] = type; });
}
function getTile(col, row) { return tileMap[`${col},${row}`] || 0; }
function setTile(col, row, type) { tileMap[`${col},${row}`] = type; }

// Type 11 = cloud platform (only solid from top)
function isSolid(type) {
  return type === 1 || type === 2 || type === 3 || type === 6 ||
         type === 4 || type === 5 || type === 8 || type === 11;
}
function isSolidFromTop(type) {
  return type === 11; // cloud: only blocks downward movement
}

// ── Animated block bounce ──────────────────────────────────────
function addAnimBlock(col, row) {
  animBlocks.push({ col, row, timer: 10, bounce: 0 });
}
function updateAnimBlocks() {
  animBlocks.forEach(b => {
    if (b.timer > 0) {
      b.timer--;
      b.bounce = Math.sin((b.timer / 10) * Math.PI) * -8;
    } else {
      b.bounce = 0;
    }
  });
  animBlocks = animBlocks.filter(b => b.timer > 0 || b.bounce !== 0);
}
function getAnimBounce(col, row) {
  const b = animBlocks.find(a => a.col === col && a.row === row);
  return b ? b.bounce : 0;
}

function resolveEntityTiles(ent) {
  const wasOnGround = ent.onGround;
  ent.onGround = false;

  // ── Horizontal – 4 sub-steps ─────────────────────────────
  for (let s = 0; s < 4; s++) {
    ent.x += ent.vx / 4;
    const l = Math.floor(ent.x / TILE);
    const r = Math.floor((ent.x + ent.w - 1) / TILE);
    const tRow = Math.floor((ent.y + 2) / TILE);
    const bRow = Math.floor((ent.y + ent.h - 1) / TILE);
    for (let row = tRow; row <= bRow; row++) {
      const tl = getTile(l, row);
      const tr = getTile(r, row);
      if (isSolid(tl) && !isSolidFromTop(tl)) {
        ent.x = (l + 1) * TILE;
        ent.vx = ent === player ? 0 : -Math.abs(ent.vx);
      }
      if (isSolid(tr) && !isSolidFromTop(tr)) {
        ent.x = r * TILE - ent.w;
        ent.vx = ent === player ? 0 : Math.abs(ent.vx);
      }
    }
  }

  // ── Vertical ────────────────────────────────────────────
  ent.y += ent.vy;
  const l2 = Math.floor(ent.x / TILE);
  const r2 = Math.floor((ent.x + ent.w - 1) / TILE);
  const t2 = Math.floor(ent.y / TILE);
  const b2 = Math.floor((ent.y + ent.h - 1) / TILE);

  for (let col = l2; col <= r2; col++) {
    // Landing (downward)
    if (ent.vy >= 0) {
      const bt = getTile(col, b2);
      if (isSolid(bt)) {
        if (isSolidFromTop(bt)) {
          // Cloud: only block if entity was above the tile last frame
          if (ent.y + ent.h - ent.vy <= b2 * TILE + 4) {
            ent.y = b2 * TILE - ent.h;
            ent.vy = 0; ent.onGround = true;
          }
        } else {
          ent.y = b2 * TILE - ent.h;
          ent.vy = 0; ent.onGround = true;
        }
      }
    }
    // Head bump (upward)
    if (ent.vy < 0) {
      const tt = getTile(col, t2);
      if (isSolid(tt) && !isSolidFromTop(tt)) {
        if (ent === player) {
          if (tt === 3) { hitQBlock(col, t2); }
          else if (tt === 2 && player.big) { breakBrick(col, t2); }
          else { sfxBrick(); }
        }
        ent.y = (t2 + 1) * TILE;
        ent.vy = 1;
      }
    }
  }
}

// ── Tile Actions ─────────────────────────────────────────────
function hitQBlock(col, row) {
  setTile(col, row, 6);
  score += 100;
  spawnFloatingText(col * TILE + 8, row * TILE, '+100', '#ffd700');
  spawnParticles(col * TILE + TILE / 2, row * TILE, '#ffd700', 10);
  addAnimBlock(col, row);
  triggerFlash('#ffd700', 0.12);
  // Pick power-up type based on player state
  const type = player.big ? (Math.random() < 0.5 ? 'fireflower' : 'star') : 'mushroom';
  spawnPowerup(col * TILE, row * TILE - TILE, type);
  sfxCoin();
}

function breakBrick(col, row) {
  setTile(col, row, 0);
  score += 50;
  spawnParticles(col * TILE + TILE / 2, row * TILE + TILE / 2, '#d07040', 16);
  spawnFloatingText(col * TILE + 6, row * TILE, '+50', '#ff9944');
  sfxBrick();
}

function aabb(a, b) {
  return a.x < b.x + b.w && a.x + a.w > b.x &&
         a.y < b.y + b.h && a.y + a.h > b.y;
}

// Fireball–tile resolution (just stop on solid)
function resolveFireballTiles(fb) {
  const col = Math.floor((fb.x + fb.w / 2) / TILE);
  const row = Math.floor((fb.y + fb.h / 2) / TILE);
  const t = getTile(col, row);
  if (isSolid(t)) { fb.active = false; }
  // Bounce off ground
  const bRow = Math.floor((fb.y + fb.h) / TILE);
  const bt = getTile(col, bRow);
  if (fb.vy >= 0 && isSolid(bt)) {
    fb.y = bRow * TILE - fb.h;
    fb.vy = -7;
  }
}
