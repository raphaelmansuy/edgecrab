// ═══════════════════════════════════════════════════════════════
//  render.js  –  all drawing functions
// ═══════════════════════════════════════════════════════════════
const canvas = document.getElementById('gameCanvas');
const ctx = canvas.getContext('2d');

// Screen flash overlay
let flashAlpha = 0, flashColor = '#ffffff';

// ── Stars (for dark levels) ──────────────────────────────────
const starField = Array.from({length:80}, () => ({
  x: Math.random()*900, y: Math.random()*300,
  r: 0.5 + Math.random()*1.5, twinkle: Math.random()*Math.PI*2
}));

function drawBackground(ld) {
  const g = ctx.createLinearGradient(0,0,0,H);
  g.addColorStop(0, ld.bg[0]);
  g.addColorStop(1, ld.bg[1]);
  ctx.fillStyle = g;
  ctx.fillRect(0,0,W,H);

  // Night stars
  if (ld.id === 2) {
    starField.forEach(s => {
      s.twinkle += 0.05;
      const alpha = 0.4 + Math.sin(s.twinkle)*0.4;
      ctx.save();
      ctx.globalAlpha = alpha;
      ctx.fillStyle = '#ffffff';
      ctx.beginPath();
      ctx.arc(s.x, s.y, s.r, 0, Math.PI*2);
      ctx.fill();
      ctx.restore();
    });
  }

  // Clouds (parallax 0.35)
  ctx.fillStyle = ld.cloudColor || 'rgba(255,255,255,0.75)';
  const cloudOffsets = [80,260,480,750,1100,1550,2050,2700,3400,4200];
  cloudOffsets.forEach((cx, i) => {
    const px = ((cx - cameraX*0.35) % (mapPixelW + 500) + mapPixelW + 500) % (mapPixelW + 500);
    drawCloud(px, 45 + (i%4)*38, ld.id === 3);
  });

  // Hills (parallax 0.2)
  if (ld.id !== 2) {
    ctx.fillStyle = ld.id === 3 ? 'rgba(180,220,255,0.25)' : 'rgba(55,130,55,0.25)';
    for (let i = 0; i < 10; i++) {
      const px = ((i*500 - cameraX*0.2) % (mapPixelW+700) + mapPixelW+700) % (mapPixelW+700);
      drawHill(px, H - TILE, 90 + i*20);
    }
  }
}

function drawCloud(x, y, puff=false) {
  ctx.beginPath();
  if (puff) {
    ctx.arc(x,y,24,0,Math.PI*2); ctx.arc(x+28,y-12,30,0,Math.PI*2);
    ctx.arc(x+58,y,22,0,Math.PI*2); ctx.arc(x+14,y+8,16,0,Math.PI*2);
    ctx.arc(x+44,y+8,16,0,Math.PI*2);
  } else {
    ctx.arc(x,y,22,0,Math.PI*2); ctx.arc(x+26,y-9,28,0,Math.PI*2);
    ctx.arc(x+54,y,20,0,Math.PI*2);
  }
  ctx.fill();
}
function drawHill(x, y, r) {
  ctx.beginPath(); ctx.arc(x, y, r, Math.PI, 0); ctx.fill();
}

// ── Tiles ────────────────────────────────────────────────────
function drawTiles(ld) {
  const startCol = Math.max(0, Math.floor(cameraX/TILE));
  const endCol   = Math.min(currentLevel.mapW, startCol + Math.ceil(W/TILE) + 2);
  for (let row = 0; row < 14; row++) {
    for (let col = startCol; col < endCol; col++) {
      const type = getTile(col, row);
      if (type) {
        const bounce = type === 3 ? getAnimBounce(col, row) : 0;
        drawTile(type, col*TILE - cameraX, row*TILE + bounce, ld);
      }
    }
  }
}

function drawTile(type, px, py, ld) {
  ctx.save();
  switch (type) {
    case 1: { // Ground
      ctx.fillStyle = ld.groundColor;
      ctx.fillRect(px, py, TILE, TILE);
      ctx.fillStyle = '#3d9e3d';
      ctx.fillRect(px, py, TILE, 6);
      ctx.fillStyle = 'rgba(255,255,255,0.12)';
      ctx.fillRect(px, py+6, TILE, 3);
      ctx.strokeStyle = 'rgba(0,0,0,0.2)';
      ctx.strokeRect(px+.5, py+.5, TILE-1, TILE-1);
      break;
    }
    case 2: { // Brick
      ctx.fillStyle = ld.brickColor;
      ctx.fillRect(px, py, TILE, TILE);
      ctx.fillStyle = 'rgba(0,0,0,0.22)';
      ctx.fillRect(px, py+TILE/2-1, TILE, 2);
      ctx.fillRect(px+TILE/2-1, py, 2, TILE/2);
      ctx.fillRect(px+TILE*.75-1, py+TILE/2, 2, TILE/2);
      ctx.fillRect(px+TILE*.25-1, py+TILE/2, 2, TILE/2);
      ctx.fillStyle = 'rgba(255,255,255,0.15)';
      ctx.fillRect(px, py, TILE, 3);
      break;
    }
    case 3: { // ? block — animated
      const pulse = Math.sin(frameCount*.12)*.18+.82;
      ctx.fillStyle = `hsl(45,100%,${Math.round(pulse*56)}%)`;
      ctx.fillRect(px, py, TILE, TILE);
      ctx.strokeStyle = '#c87000'; ctx.lineWidth = 2;
      ctx.strokeRect(px+1, py+1, TILE-2, TILE-2);
      ctx.fillStyle = '#fff';
      ctx.font = 'bold 22px sans-serif'; ctx.textAlign = 'center';
      ctx.fillText('?', px+TILE/2, py+TILE*.74);
      break;
    }
    case 6: { // Used block
      ctx.fillStyle = '#888';
      ctx.fillRect(px, py, TILE, TILE);
      ctx.strokeStyle = '#555'; ctx.lineWidth = 2;
      ctx.strokeRect(px+1, py+1, TILE-2, TILE-2);
      break;
    }
    case 4: { // Pipe top
      const g = ctx.createLinearGradient(px-4, 0, px+TILE+4, 0);
      g.addColorStop(0,'#0e7a0e'); g.addColorStop(0.4,'#22cc22'); g.addColorStop(1,'#0e7a0e');
      ctx.fillStyle = g;
      ctx.fillRect(px-4, py, TILE+8, TILE);
      ctx.strokeStyle = '#064006'; ctx.lineWidth = 2;
      ctx.strokeRect(px-4, py, TILE+8, TILE);
      break;
    }
    case 5: { // Pipe body
      const g2 = ctx.createLinearGradient(px, 0, px+TILE, 0);
      g2.addColorStop(0,'#0e7a0e'); g2.addColorStop(0.3,'#22cc22'); g2.addColorStop(1,'#0e7a0e');
      ctx.fillStyle = g2;
      ctx.fillRect(px, py, TILE, TILE);
      ctx.strokeStyle = '#064006'; ctx.lineWidth = 1;
      ctx.strokeRect(px, py, TILE, TILE);
      break;
    }
    case 11: { // Cloud platform
      ctx.fillStyle = 'rgba(255,255,255,0.88)';
      ctx.beginPath();
      ctx.arc(px+8,  py+TILE/2, 10, 0, Math.PI*2);
      ctx.arc(px+20, py+TILE/2-5, 14, 0, Math.PI*2);
      ctx.arc(px+34, py+TILE/2, 11, 0, Math.PI*2);
      ctx.fill();
      ctx.strokeStyle = 'rgba(150,200,255,0.5)';
      ctx.lineWidth = 1; ctx.stroke();
      break;
    }
  }
  ctx.restore();
}

// ── Player ───────────────────────────────────────────────────
function drawPlayer() {
   if (player.invincible > 0 && player.star === 0 && Math.floor(player.invincible/4)%2===0 && !player.dead) return;
   ctx.save();
   const sx = player.x - cameraX + cameraShakeX, sy = player.y + cameraShakeY;
   const ph = player.big || player.fire ? 44 : 36;
   const pw = player.w;
   ctx.translate(sx + pw/2, sy + ph/2);
   // Squash & stretch
   const sq = Math.max(0.5, Math.min(1.4, player.squash));
   ctx.scale(2 - sq, sq);
   if (!player.facingRight) ctx.scale(-1, 1);
   if (player.dead) ctx.rotate(frameCount * 0.15);

   // Star rainbow tint
   if (player.star > 0) {
     ctx.shadowColor = `hsl(${frameCount*8%360},100%,60%)`;
     ctx.shadowBlur = 14;
   }

   // Shadow
   ctx.fillStyle = 'rgba(0,0,0,0.15)';
   ctx.beginPath(); ctx.ellipse(0, ph/2+3, pw/2, 5, 0, 0, Math.PI*2); ctx.fill();

   // Leg animation
   const lf = player.animFrame===1 ? 5 : player.animFrame===2 ? -2 : 0;
   ctx.fillStyle = '#2980b9';
   ctx.fillRect(-pw/2+2, ph/2-10+lf, pw/2-2, 10);
   ctx.fillRect(2, ph/2-10-lf, pw/2-2, 10);
   ctx.fillStyle = '#5d4037';
   ctx.fillRect(-pw/2, ph/2+lf-2, pw/2+2, 5);
   ctx.fillRect(0, ph/2-lf-2, pw/2+2, 5);

   // Body — fire = white+orange, star = rainbow, normal = red
   const bodyCol = player.fire ? '#e8e8e8' : player.star > 0 ? `hsl(${frameCount*12%360},100%,60%)` : '#e74c3c';
   ctx.fillStyle = bodyCol;
   ctx.fillRect(-pw/2, -ph/2+12, pw, ph-22);
   // Overalls
   ctx.fillStyle = player.fire ? '#e67e22' : '#2980b9';
   ctx.fillRect(-pw/2+2, -ph/2+18, pw-4, ph-30);
   // Head
   ctx.fillStyle = '#f5cba7'; ctx.fillRect(-pw/2+3, -ph/2, pw-6, 16);
   // Hat
   ctx.fillStyle = player.fire ? '#e67e22' : '#e74c3c';
   ctx.fillRect(-pw/2, -ph/2-5, pw, 7);
   ctx.fillRect(-pw/2+4, -ph/2-12, pw-8, 8);
   // Eye
   ctx.fillStyle = '#000'; ctx.fillRect(pw/2-7, -ph/2+3, 4, 4);
   // Moustache
   ctx.fillStyle = '#5d4037'; ctx.fillRect(-pw/2+4, -ph/2+10, pw/2+2, 3);

   // Fire flower aura
   if (player.fire && !player.dead) {
     ctx.globalAlpha = 0.35 + Math.sin(frameCount*0.2)*0.15;
     ctx.fillStyle = '#ff9900';
     ctx.beginPath(); ctx.arc(0, 0, pw/2+6, 0, Math.PI*2); ctx.fill();
     ctx.globalAlpha = 1;
   }

   ctx.restore();
}

// ── Enemies ──────────────────────────────────────────────────
function drawEnemies() {
  enemies.forEach(e => {
    if (!e.alive) return;
    const sx = e.x - cameraX;
    if (e.type==='goomba')   drawGoomba(sx, e.y, e.w, e.h, e.animFrame, e.stomped, e.facingRight);
    else if (e.type==='koopa') drawKoopa(sx, e.y, e.w, e.h, e.animFrame, e.isShell, e.facingRight);
    else if (e.type==='piranha') drawPiranha(sx, e.y, e.animFrame);
  });
}

function drawGoomba(sx, sy, w, h, frame, stomped, fr) {
  if (stomped) {
    // Squished — flattened pancake
    ctx.fillStyle='#c87830'; ctx.fillRect(sx, sy+20, w, 8);
    ctx.fillStyle='#e8a050'; ctx.fillRect(sx+2, sy+16, w-4, 6);
    return;
  }
  ctx.save();
  if (!fr) { ctx.translate(sx+w/2, 0); ctx.scale(-1,1); ctx.translate(-(sx+w/2),0); }
  ctx.fillStyle='#c87830'; ctx.fillRect(sx, sy+16, w, h-16);
  ctx.fillStyle='#e8a050';
  ctx.beginPath(); ctx.arc(sx+w/2, sy+12, 13, 0, Math.PI*2); ctx.fill();
  ctx.fillStyle='#fff';
  ctx.fillRect(sx+4, sy+5, 7, 7); ctx.fillRect(sx+17, sy+5, 7, 7);
  ctx.fillStyle='#000';
  ctx.fillRect(sx+6, sy+7, 4, 4); ctx.fillRect(sx+19, sy+7, 4, 4);
  // Angry eyebrows
  ctx.fillStyle='#8b0000';
  ctx.beginPath(); ctx.moveTo(sx+4,sy+5); ctx.lineTo(sx+11,sy+8); ctx.lineWidth=2; ctx.strokeStyle='#8b0000'; ctx.stroke();
  ctx.beginPath(); ctx.moveTo(sx+24,sy+5); ctx.lineTo(sx+17,sy+8); ctx.stroke();
  ctx.fillStyle='#8b0000'; ctx.fillRect(sx+6, sy+14, 16, 2);
  const fo = frame===1 ? 3 : 0;
  ctx.fillStyle='#5d4037';
  ctx.fillRect(sx+2, sy+h-6-fo, 10, 8); ctx.fillRect(sx+w-12, sy+h-6+fo, 10, 8);
  ctx.restore();
}

function drawKoopa(sx, sy, w, h, frame, isShell, fr) {
  if (isShell) {
    const g = ctx.createLinearGradient(sx, sy, sx+30, sy+24);
    g.addColorStop(0,'#1e8449'); g.addColorStop(0.5,'#52c77a'); g.addColorStop(1,'#1e8449');
    ctx.fillStyle = g;
    ctx.fillRect(sx, sy, 30, 24);
    ctx.fillStyle='rgba(255,255,255,0.3)';
    ctx.beginPath(); ctx.arc(sx+8, sy+7, 4, 0, Math.PI*2); ctx.fill();
    ctx.strokeStyle='#145a32'; ctx.lineWidth=1; ctx.strokeRect(sx, sy, 30, 24);
    return;
  }
  ctx.save();
  if (!fr) { ctx.translate(sx+w/2,0); ctx.scale(-1,1); ctx.translate(-(sx+w/2),0); }
  ctx.fillStyle='#27ae60'; ctx.fillRect(sx+2, sy+12, w-4, h-12);
  ctx.fillStyle='#1e8449'; ctx.fillRect(sx+6, sy+16, w-12, h-20);
  ctx.fillStyle='#a0d080';
  ctx.beginPath(); ctx.arc(sx+w/2, sy+12, 13, 0, Math.PI*2); ctx.fill();
  ctx.fillStyle='#fff'; ctx.fillRect(sx+w/2+2, sy+7, 7, 7);
  ctx.fillStyle='#000'; ctx.fillRect(sx+w/2+4, sy+9, 3, 3);
  const fo = frame===1 ? 3 : 0;
  ctx.fillStyle='#1a5c20';
  ctx.fillRect(sx+2, sy+h-8-fo, 10, 10); ctx.fillRect(sx+w-12, sy+h-8+fo, 10, 10);
  // Shell on back
  ctx.fillStyle='#f0c040'; ctx.beginPath(); ctx.ellipse(sx+w/2, sy+h/2, 8, 12, 0, 0, Math.PI*2); ctx.fill();
  ctx.restore();
}

function drawPiranha(sx, sy, frame) {
  const bite = (frame % 8) < 4;
  ctx.fillStyle='#1a8a1a'; ctx.fillRect(sx+6, sy+28, 20, 10);
  ctx.fillStyle='#e74c3c'; ctx.fillRect(sx+4, sy, 24, 32);
  if (bite) {
    ctx.fillStyle='#c0392b';
    ctx.beginPath(); ctx.moveTo(sx+4,sy+14); ctx.lineTo(sx+28,sy+8); ctx.lineTo(sx+28,sy+20); ctx.closePath(); ctx.fill();
  }
  // Teeth
  ctx.fillStyle='#fff';
  for (let i=0;i<3;i++) ctx.fillRect(sx+5+i*8, sy+16, 5, 6);
  // Eyes
  ctx.fillStyle='#fff'; ctx.fillRect(sx+6, sy+3, 8, 8); ctx.fillRect(sx+18, sy+3, 8, 8);
  ctx.fillStyle='#000'; ctx.fillRect(sx+8, sy+5, 4, 4); ctx.fillRect(sx+20, sy+5, 4, 4);
  // Spots
  ctx.fillStyle='rgba(0,100,0,0.35)';
  ctx.beginPath(); ctx.arc(sx+12, sy+22, 3, 0, Math.PI*2); ctx.fill();
  ctx.beginPath(); ctx.arc(sx+20, sy+25, 2.5, 0, Math.PI*2); ctx.fill();
}

// ── Coins ────────────────────────────────────────────────────
function drawCoins() {
  coins.forEach(c => {
    if (!c.active) return;
    const sx = c.x - cameraX, bY = c.y + Math.sin(c.bobTimer)*4;
    ctx.save();
    const gCoin = ctx.createRadialGradient(sx+8, bY+8, 2, sx+8, bY+10, 9);
    gCoin.addColorStop(0,'#fff7a0'); gCoin.addColorStop(0.5,'#ffd700'); gCoin.addColorStop(1,'#c87000');
    ctx.fillStyle = gCoin;
    ctx.beginPath(); ctx.arc(sx+8, bY+10, 9, 0, Math.PI*2); ctx.fill();
    ctx.strokeStyle='#c87000'; ctx.lineWidth=1.5; ctx.stroke();
    ctx.fillStyle='rgba(255,255,255,0.7)';
    ctx.beginPath(); ctx.arc(sx+5, bY+7, 2.5, 0, Math.PI*2); ctx.fill();
    ctx.restore();
  });
}

// ── Powerups ─────────────────────────────────────────────────
function drawPowerups() {
  powerups.forEach(p => {
    if (!p.active) return;
    const sx = p.x - cameraX;
    ctx.save();
    if (p.type === 'mushroom') {
      // Red mushroom
      ctx.fillStyle='#e74c3c';
      ctx.beginPath(); ctx.arc(sx+p.w/2, p.y+p.h/2-2, p.w/2, Math.PI, 0); ctx.fill();
      ctx.fillStyle='#fff'; ctx.fillRect(sx+2, p.y+p.h/2-2, p.w-4, p.h/2);
      ctx.fillStyle='#fff';
      ctx.beginPath(); ctx.arc(sx+8, p.y+p.h/2-8, 4, 0, Math.PI*2); ctx.fill();
      ctx.beginPath(); ctx.arc(sx+20, p.y+p.h/2-10, 4, 0, Math.PI*2); ctx.fill();
    } else if (p.type === 'fireflower') {
      // Orange fire flower
      const pha = frameCount * 0.2;
      for (let i=0;i<5;i++) {
        ctx.fillStyle = i%2===0 ? '#ff6600' : '#ffcc00';
        const a = pha + i * Math.PI*2/5;
        ctx.beginPath(); ctx.arc(sx+p.w/2+Math.cos(a)*9, p.y+p.h/2+Math.sin(a)*9, 5, 0, Math.PI*2); ctx.fill();
      }
      ctx.fillStyle='#27ae60'; ctx.fillRect(sx+p.w/2-3, p.y+p.h/2, 6, p.h/2);
      ctx.fillStyle='#ff4500';
      ctx.beginPath(); ctx.arc(sx+p.w/2, p.y+p.h/2, 8, 0, Math.PI*2); ctx.fill();
    } else if (p.type === 'star') {
      // Spinning star
      ctx.translate(sx+p.w/2, p.y+p.h/2);
      ctx.rotate(frameCount * 0.08);
      ctx.fillStyle = `hsl(${frameCount*6%360},100%,60%)`;
      drawStar5(ctx, 0, 0, 14, 6);
    }
    ctx.restore();
  });
}

function drawStar5(c, x, y, outer, inner) {
  c.beginPath();
  for (let i=0;i<10;i++) {
    const r = i%2===0 ? outer : inner;
    const a = (i * Math.PI/5) - Math.PI/2;
    i===0 ? c.moveTo(x+Math.cos(a)*r, y+Math.sin(a)*r)
           : c.lineTo(x+Math.cos(a)*r, y+Math.sin(a)*r);
  }
  c.closePath(); c.fill();
}

// ── Fireballs ────────────────────────────────────────────────
function drawFireballs() {
  fireballs.forEach(fb => {
    if (!fb.active) return;
    const sx = fb.x - cameraX;
    ctx.save();
    ctx.translate(sx + fb.w/2, fb.y + fb.h/2);
    ctx.rotate(frameCount * 0.3);
    ctx.shadowColor = '#ff6600'; ctx.shadowBlur = 10;
    const gFb = ctx.createRadialGradient(0,0,1,0,0,fb.w/2);
    gFb.addColorStop(0,'#fff7a0'); gFb.addColorStop(0.5,'#ff9900'); gFb.addColorStop(1,'#ff3300');
    ctx.fillStyle = gFb;
    ctx.beginPath(); ctx.arc(0, 0, fb.w/2, 0, Math.PI*2); ctx.fill();
    ctx.restore();
  });
}

// ── Flag ─────────────────────────────────────────────────────
function drawFlag() {
  const sx = flag.x - cameraX;
  // Pole
  const grad = ctx.createLinearGradient(sx-3,0,sx+3,0);
  grad.addColorStop(0,'#aaa'); grad.addColorStop(0.5,'#eee'); grad.addColorStop(1,'#aaa');
  ctx.fillStyle = grad; ctx.fillRect(sx-2, TILE*2, 4, H-TILE*3);
  // Ball on top
  ctx.fillStyle='#ffd700';
  ctx.beginPath(); ctx.arc(sx, TILE*2, 8, 0, Math.PI*2); ctx.fill();
  ctx.strokeStyle='#c87000'; ctx.lineWidth=1; ctx.stroke();
  // Flag banner
  const by = flag.captured
    ? Math.min(flag.slideY+=2, H-TILE*2-28)
    : TILE*2+4;
  if (!flag.captured) flag.slideY = TILE*2+4;
  const gf = ctx.createLinearGradient(sx, by, sx+38, by+26);
  gf.addColorStop(0,'#27ae60'); gf.addColorStop(1,'#1a7a40');
  ctx.fillStyle = gf;
  ctx.beginPath(); ctx.moveTo(sx,by); ctx.lineTo(sx+40,by+13); ctx.lineTo(sx,by+28); ctx.fill();
  // Star on flag
  ctx.fillStyle='#ffd700'; drawStar5(ctx, sx+16, by+14, 8, 3);
  // Base
  ctx.fillStyle='#888'; ctx.fillRect(sx-10, H-TILE*2-4, 22, 8);
}

// ── Progress bar ─────────────────────────────────────────────
function drawProgressBar() {
  const barW = W - 120, barH = 6, bx = 60, by = H - 14;
  const prog = Math.min(1, (player.x + cameraX) / mapPixelW);
  ctx.fillStyle = 'rgba(0,0,0,0.4)';
  ctx.fillRect(bx, by, barW, barH);
  const g = ctx.createLinearGradient(bx, 0, bx+barW, 0);
  g.addColorStop(0,'#27ae60'); g.addColorStop(0.7,'#f1c40f'); g.addColorStop(1,'#e74c3c');
  ctx.fillStyle = g;
  ctx.fillRect(bx, by, barW*prog, barH);
  // Player marker
  ctx.fillStyle='#fff'; ctx.fillRect(bx + barW*prog - 2, by-2, 4, barH+4);
  // Flag marker
  const fp = (flag.x) / mapPixelW;
  ctx.fillStyle='#ffd700'; ctx.fillRect(bx + barW*fp - 2, by-3, 4, barH+6);
}

// ── Combo display ─────────────────────────────────────────────
function drawCombo() {
  if (comboCount < 2 || comboTimer <= 0) return;
  ctx.save();
  ctx.globalAlpha = Math.min(1, comboTimer/30);
  ctx.font = `bold ${18+comboCount*2}px monospace`;
  ctx.textAlign = 'center';
  ctx.fillStyle = '#ffe000';
  ctx.shadowColor='#ff6600'; ctx.shadowBlur=8;
  ctx.fillText(`${comboCount}x COMBO!`, W/2, 60);
  ctx.restore();
}

// ── Star timer bar ────────────────────────────────────────────
function drawStarTimer() {
  if (player.star <= 0) return;
  const barW = 160, bx = (W-barW)/2, by = 72, barH=8;
  ctx.fillStyle='rgba(0,0,0,0.5)'; ctx.fillRect(bx, by, barW, barH);
  const hue = frameCount*8%360;
  ctx.fillStyle=`hsl(${hue},100%,60%)`;
  ctx.fillRect(bx, by, barW*(player.star/480), barH);
  ctx.fillStyle='#ffe000'; ctx.font='bold 10px monospace'; ctx.textAlign='center';
  ctx.fillText('⭐ STAR POWER ⭐', W/2, by-4);
}

// ── Touch Buttons ─────────────────────────────────────────────
function drawTouchControls() {
   if (!('ontouchstart' in window) && !navigator.maxTouchPoints) return;
   const alpha = 0.50;  // More visible
   const btnSz = 70;    // Larger buttons
   // D-pad
   function btn(x, y, label, id) {
     ctx.save();
     ctx.globalAlpha = alpha;
     ctx.fillStyle = '#2c3e50';  // Darker background
     ctx.beginPath(); ctx.arc(x+btnSz/2, y+btnSz/2, btnSz/2, 0, Math.PI*2); ctx.fill();
     ctx.strokeStyle = 'rgba(255,255,255,0.3)';
     ctx.lineWidth = 2;
     ctx.stroke();
     ctx.fillStyle = '#fff';
     ctx.globalAlpha = 1;
     ctx.font = 'bold 24px sans-serif'; ctx.textAlign='center'; ctx.textBaseline='middle';
     ctx.fillText(label, x+btnSz/2, y+btnSz/2);
     ctx.restore();
   }
   const pad = 20, bot = H - pad - btnSz;
   btn(pad, bot, '◀', 'left');
   btn(pad + btnSz + 14, bot, '▶', 'right');
   // Jump / Fire / Run on right
   btn(W - pad - btnSz, bot - btnSz - 14, '🔥', 'fire');
   btn(W - pad - btnSz*2 - 14, bot, 'RUN', 'run');
   btn(W - pad - btnSz, bot, 'JUMP', 'jump');
}

// ── Pause Overlay ────────────────────────────────────────────
function drawPauseOverlay() {
  ctx.fillStyle='rgba(10,10,30,0.75)'; ctx.fillRect(0,0,W,H);
  ctx.fillStyle='#ffd700'; ctx.font='bold 32px monospace'; ctx.textAlign='center';
  ctx.fillText('⏸  PAUSED', W/2, H/2-20);
  ctx.fillStyle='#aed6f1'; ctx.font='13px monospace';
  ctx.fillText('Press  P  to resume', W/2, H/2+20);
}

// ── Particles / Texts ────────────────────────────────────────
function drawParticles() {
  particles.forEach(p => {
    ctx.save(); ctx.globalAlpha = p.life;
    ctx.fillStyle = p.color;
    ctx.beginPath(); ctx.arc(p.x - cameraX, p.y, p.r, 0, Math.PI*2); ctx.fill();
    ctx.restore();
  });
}

function drawFloatingTexts() {
  floatingTexts.forEach(t => {
    ctx.save(); ctx.globalAlpha = t.life;
    ctx.fillStyle = t.color;
    ctx.font = 'bold 13px monospace'; ctx.textAlign = 'left';
    ctx.shadowColor = 'rgba(0,0,0,0.8)'; ctx.shadowBlur = 4;
    ctx.fillText(t.text, t.x - cameraX, t.y);
    ctx.restore();
  });
}

// ── Animated block overlay (bounce handled in drawTiles) ────────
function drawAnimBlocks() { /* bounce rendered inline in drawTiles */ }

// ── Screen flash ────────────────────────────────────────────────
function drawScreenFlash() {
  if (flashAlpha <= 0) return;
  ctx.save();
  ctx.globalAlpha = flashAlpha;
  ctx.fillStyle = flashColor;
  ctx.fillRect(0, 0, W, H);
  ctx.restore();
  flashAlpha -= 0.08;
  if (flashAlpha < 0) flashAlpha = 0;
}

function triggerFlash(color = '#ffffff', intensity = 0.35) {
  flashColor = color;
  flashAlpha = intensity;
}
