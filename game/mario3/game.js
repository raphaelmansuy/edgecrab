// ═══════════════════════════════════════════════════════════
//  SUPER MARIO 3 — 2D Platformer  |  game.js
// ═══════════════════════════════════════════════════════════
'use strict';

// ── Constants ──────────────────────────────────────────────
const TILE   = 40;
const CANVAS_W = 800;
const CANVAS_H = 500;
const GRAVITY  = 0.55;
const MAX_FALL = 18;

// ── Canvas setup ───────────────────────────────────────────
const canvas = document.getElementById('gameCanvas');
const ctx    = canvas.getContext('2d');
canvas.width  = CANVAS_W;
canvas.height = CANVAS_H;

// ── Audio ───────────────────────────────────────────────────
const AC = new (window.AudioContext || window.webkitAudioContext)();
function beep(freq, dur=0.08, type='square', vol=0.18, delay=0) {
  try {
    const o = AC.createOscillator(), g = AC.createGain();
    o.connect(g); g.connect(AC.destination);
    o.type = type; o.frequency.value = freq;
    g.gain.setValueAtTime(vol, AC.currentTime + delay);
    g.gain.exponentialRampToValueAtTime(0.001, AC.currentTime + delay + dur);
    o.start(AC.currentTime + delay);
    o.stop(AC.currentTime + delay + dur + 0.01);
  } catch(e){}
}
function playJump()    { beep(380,0.06,'square',0.2); beep(520,0.07,'square',0.18,0.06); }
function playLand()    { beep(160,0.05,'sine',0.12); }
function playCoin()    { beep(988,0.05,'square',0.15); beep(1318,0.08,'square',0.13,0.05); }
function playKill()    { beep(200,0.06,'sawtooth',0.15); beep(130,0.1,'sawtooth',0.12,0.06); }
function playPowerup() { [523,659,784,1047].forEach((f,i)=>beep(f,0.1,'square',0.15,i*0.09)); }
function playHurt()    { beep(200,0.08,'sawtooth',0.25); beep(130,0.12,'sawtooth',0.2,0.08); }
function playFlagpole(){ [784,988,1175,1568].forEach((f,i)=>beep(f,0.15,'square',0.18,i*0.12)); }

// ── Input ───────────────────────────────────────────────────
const keys = {};
window.addEventListener('keydown', e => {
  keys[e.key] = true;
  if ([' ','ArrowUp','ArrowDown'].includes(e.key)) e.preventDefault();
  if (e.key === 'Enter') handleEnterKey();
  if (e.key === 'p' || e.key === 'P') togglePause();
});
window.addEventListener('keyup', e => { keys[e.key] = false; });
document.querySelectorAll('.mBtn').forEach(btn => {
  const k = btn.dataset.key;
  btn.addEventListener('touchstart', e=>{e.preventDefault();keys[k]=true;},{passive:false});
  btn.addEventListener('touchend',   e=>{e.preventDefault();keys[k]=false;},{passive:false});
  btn.addEventListener('mousedown',  ()=>keys[k]=true);
  btn.addEventListener('mouseup',    ()=>keys[k]=false);
});

// ── Game state ──────────────────────────────────────────────
let state = 'start';
let score=0, lives=3, coins=0, timeLeft=300;
let timerInterval=null, currentLevel=0, cameraX=0;
let particles=[], floatTexts=[], enemies=[], coinItems=[], powerups=[];
let player, levelData;
let bgStars=[], bgClouds=[];

// ── UI ──────────────────────────────────────────────────────
const elScore=document.getElementById('score');
const elLives=document.getElementById('lives');
const elCoins=document.getElementById('coins');
const elTimer=document.getElementById('timer');
const elWorld=document.getElementById('world');
function updateHUD(){elScore.textContent=score;elLives.textContent=lives;elCoins.textContent=coins;elTimer.textContent=Math.ceil(timeLeft);}

// ── Levels ──────────────────────────────────────────────────
const LEVELS = [
  { name:'1-1', timeLimit:300, bgColor:'#5c94fc', groundColor:'#e07030', map:buildLevel1() },
  { name:'1-2', timeLimit:250, bgColor:'#000014', groundColor:'#5860ac', map:buildLevel2() },
  { name:'1-3', timeLimit:200, bgColor:'#5c94fc', groundColor:'#e07030', map:buildLevel3() },
];

function buildLevel1(){
  const W=60,H=14,m=Array.from({length:H},()=>Array(W).fill(0));
  for(let x=0;x<W;x++){m[H-1][x]=1;if(x!==10&&x!==11&&x!==12&&x!==30&&x!==31)m[H-2][x]=1;}
  for(let x=13;x<=14;x++){m[H-1][x]=0;m[H-2][x]=0;}
  [6,7,9,10].forEach(x=>m[H-6][x]=2); m[H-6][8]=3;
  for(let x=15;x<=22;x++) m[H-4][x]=1;
  m[H-7][18]=3;
  for(let x=23;x<=24;x++){m[H-1][x]=0;m[H-2][x]=0;}
  [27,28,29].forEach(x=>m[H-6][x]=2); m[H-6][30]=3; m[H-6][31]=3;
  for(let s=0;s<5;s++) for(let r=s;r<H-1;r++) m[r][W-7-s]=9;
  m[H-3][35]=5; m[H-2][35]=4; m[H-3][36]=5; m[H-2][36]=4;
  m[H-2][W-3]=7;
  return m;
}
function buildLevel2(){
  const W=55,H=14,m=Array.from({length:H},()=>Array(W).fill(0));
  for(let x=0;x<W;x++){m[0][x]=1;m[H-1][x]=1;m[H-2][x]=1;}
  [[2,4,8],[4,10,18],[6,22,30],[3,32,38],[5,40,48]].forEach(([y,x1,x2])=>{for(let x=x1;x<=x2;x++) m[H-1-y][x]=1;});
  m[H-7][5]=3; m[H-7][15]=3; m[H-7][25]=3; m[H-7][42]=3;
  [8,9,10,30,31,32].forEach(x=>m[H-5][x]=2);
  m[H-2][W-3]=7;
  return m;
}
function buildLevel3(){
  const W=58,H=14,m=Array.from({length:H},()=>Array(W).fill(0));
  [[H-4,0,6],[H-6,8,14],[H-4,16,22],[H-7,24,30],[H-5,32,38],[H-4,40,46],[H-6,48,54]].forEach(([row,x1,x2])=>{for(let x=x1;x<=x2;x++) m[row][x]=8;});
  for(let x=0;x<4;x++){m[H-1][x]=1;m[H-2][x]=1;} for(let x=W-5;x<W;x++){m[H-1][x]=1;m[H-2][x]=1;}
  m[H-9][5]=3; m[H-9][12]=3; m[H-9][26]=3; m[H-9][44]=3;
  m[H-3][W-3]=7;
  return m;
}

// ── Player ──────────────────────────────────────────────────
class Player {
  constructor(x,y){
    this.x=x;this.y=y;this.w=28;this.h=36;
    this.vx=0;this.vy=0;this.onGround=false;this.facingRight=true;
    this.big=false;this.jumpHeld=0;this.coyoteTime=0;this.jumpBuffer=0;
    this.running=false;this.walkFrame=0;this.walkTimer=0;
    this.wasOnGround=false;this.invincible=0;
  }
  get bigH(){return this.big?52:36;}
  update(){
    const RUN=6.2,WALK=3.8,ACC=0.45,DEC=0.3,JF=-11.5,JH=0.6,MJH=14;
    this.running=keys['z']||keys['x']||keys['Shift']||keys['Z']||keys['X'];
    const maxSpd=this.running?RUN:WALK;
    const left=keys['ArrowLeft']||keys['a']||keys['A'];
    const right=keys['ArrowRight']||keys['d']||keys['D'];
    if(right){this.vx=Math.min(this.vx+ACC,maxSpd);this.facingRight=true;}
    else if(left){this.vx=Math.max(this.vx-ACC,-maxSpd);this.facingRight=false;}
    else{if(this.vx>0)this.vx=Math.max(0,this.vx-DEC);else if(this.vx<0)this.vx=Math.min(0,this.vx+DEC);}
    if(this.onGround)this.coyoteTime=6;else if(this.coyoteTime>0)this.coyoteTime--;
    const jk=keys['ArrowUp']||keys[' ']||keys['w']||keys['W'];
    if(jk)this.jumpBuffer=6;else if(this.jumpBuffer>0)this.jumpBuffer--;
    if(this.jumpBuffer>0&&this.coyoteTime>0){this.vy=JF;this.jumpHeld=1;this.coyoteTime=0;this.jumpBuffer=0;playJump();}
    if(jk&&this.jumpHeld>0&&this.jumpHeld<MJH){this.vy+=JH*(1-this.jumpHeld/MJH);this.jumpHeld++;}
    else if(!jk)this.jumpHeld=0;
    this.vy=Math.min(this.vy+GRAVITY,MAX_FALL);
    this.wasOnGround=this.onGround;this.onGround=false;
    this.moveAndCollide();
    if(!this.wasOnGround&&this.onGround)playLand();
    if(this.onGround&&Math.abs(this.vx)>0.2){this.walkTimer++;if(this.walkTimer>(this.running?5:8)){this.walkFrame=(this.walkFrame+1)%3;this.walkTimer=0;}}else{this.walkFrame=0;this.walkTimer=0;}
    if(this.invincible>0)this.invincible--;
    if(this.x<0){this.x=0;this.vx=0;}
    const mw=levelData.map[0].length*TILE;
    if(this.x+this.w>mw){this.x=mw-this.w;this.vx=0;}
  }
  moveAndCollide(){this.x+=this.vx;this.resolveCollision('h');this.y+=this.vy;this.resolveCollision('v');}
  resolveCollision(axis){
    const map=levelData.map,H=map.length,W=map[0].length,ph=this.bigH;
    const x0=Math.floor(this.x/TILE),x1=Math.floor((this.x+this.w-1)/TILE);
    const y0=Math.floor(this.y/TILE),y1=Math.floor((this.y+ph-1)/TILE);
    for(let ty=Math.max(0,y0);ty<=Math.min(H-1,y1);ty++){
      for(let tx=Math.max(0,x0);tx<=Math.min(W-1,x1);tx++){
        const t=map[ty][tx];if(!isSolid(t))continue;
        const tL=tx*TILE,tT=ty*TILE,tR=tL+TILE,tB=tT+TILE;
        if(axis==='h'){if(this.vx>0){this.x=tL-this.w;this.vx=0;}else if(this.vx<0){this.x=tR;this.vx=0;}}
        else{if(this.vy>0){this.y=tT-ph;this.vy=0;this.onGround=true;}
          else if(this.vy<0){this.y=tB;this.vy=0;hitBlockFromBelow(tx,ty);}}
      }
    }
  }
  draw(){
    const sx=Math.round(this.x-cameraX),ph=this.bigH,sy=Math.round(this.y);
    if(this.invincible>0&&Math.floor(this.invincible/4)%2===0)return;
    ctx.save();
    if(!this.facingRight){ctx.translate(sx+this.w/2,sy+ph/2);ctx.scale(-1,1);ctx.translate(-this.w/2,-ph/2);}
    else ctx.translate(sx,sy);
    drawPlayer(ctx,0,0,this.w,ph,this.walkFrame,this.big,!this.onGround,this.running);
    ctx.restore();
  }
}

// ── Enemy: Goomba ───────────────────────────────────────────
class Goomba {
  constructor(x,y){this.x=x;this.y=y;this.w=TILE-4;this.h=TILE-4;this.vx=-1.2;this.vy=0;this.onGround=false;this.alive=true;this.squished=false;this.squishTimer=0;this.walkFrame=0;this.walkTimer=0;}
  update(){
    if(this.squished){this.squishTimer--;if(this.squishTimer<=0)this.alive=false;return;}
    this.vy=Math.min(this.vy+GRAVITY,MAX_FALL);this.x+=this.vx;this.y+=this.vy;this.onGround=false;
    const map=levelData.map,H=map.length,W=map[0].length;
    for(let ty=Math.max(0,Math.floor(this.y/TILE));ty<=Math.min(H-1,Math.floor((this.y+this.h-1)/TILE));ty++){
      for(let tx=Math.max(0,Math.floor(this.x/TILE));tx<=Math.min(W-1,Math.floor((this.x+this.w-1)/TILE));tx++){
        if(!isSolid(map[ty][tx]))continue;
        const tL=tx*TILE,tT=ty*TILE,tR=tL+TILE;
        if(this.vy>=0&&this.y+this.h>tT&&this.y<tT+TILE&&this.x+this.w>tL+2&&this.x<tR-2){this.y=tT-this.h;this.vy=0;this.onGround=true;}
        else if(this.vx>0&&this.x+this.w>=tL){this.x=tL-this.w;this.vx*=-1;}
        else if(this.vx<0&&this.x<=tR){this.x=tR;this.vx*=-1;}
      }
    }
    if(this.onGround){const nX=this.vx>0?Math.floor((this.x+this.w+1)/TILE):Math.floor((this.x-1)/TILE);if(nX>=0&&nX<levelData.map[0].length&&!isSolid(levelData.map[Math.min(H-1,Math.floor((this.y+this.h+1)/TILE))][nX]))this.vx*=-1;}
    if(this.x<0||(this.x+this.w)>levelData.map[0].length*TILE)this.vx*=-1;
    this.walkTimer++;if(this.walkTimer>8){this.walkFrame=(this.walkFrame+1)%2;this.walkTimer=0;}
  }
  squish(){this.squished=true;this.squishTimer=20;playKill();}
  draw(){if(!this.alive)return;const sx=Math.round(this.x-cameraX),sy=Math.round(this.y);if(sx<-TILE||sx>CANVAS_W+TILE)return;drawGoomba(sx,sy,this.w,this.h,this.walkFrame,this.squished);}
}

// ── Enemy: Koopa ─────────────────────────────────────────────
class Koopa {
  constructor(x,y){this.x=x;this.y=y;this.w=TILE-4;this.h=TILE+8;this.vx=-1.4;this.vy=0;this.onGround=false;this.alive=true;this.shelled=false;this.shellKicked=false;this.shellTimer=0;this.walkFrame=0;this.walkTimer=0;}
  update(){
    if(this.shelled&&!this.shellKicked){this.shellTimer--;if(this.shellTimer<=0){this.shelled=false;this.vx=-1.4;}return;}
    this.vy=Math.min(this.vy+GRAVITY,MAX_FALL);this.x+=this.vx;this.y+=this.vy;this.onGround=false;
    const map=levelData.map,H=map.length,W=map[0].length;
    for(let ty=Math.max(0,Math.floor(this.y/TILE));ty<=Math.min(H-1,Math.floor((this.y+this.h-1)/TILE));ty++){
      for(let tx=Math.max(0,Math.floor(this.x/TILE));tx<=Math.min(W-1,Math.floor((this.x+this.w-1)/TILE));tx++){
        if(!isSolid(map[ty][tx]))continue;
        const tL=tx*TILE,tT=ty*TILE,tR=tL+TILE;
        if(this.vy>=0&&this.y+this.h>tT&&this.y<tT+TILE&&this.x+this.w>tL+2&&this.x<tR-2){this.y=tT-this.h;this.vy=0;this.onGround=true;}
        else if(this.vx>0&&this.x+this.w>=tL){this.x=tL-this.w;this.vx*=-1;}
        else if(this.vx<0&&this.x<=tR){this.x=tR;this.vx*=-1;}
      }
    }
    if(this.x<0||(this.x+this.w)>levelData.map[0].length*TILE)this.vx*=-1;
    this.walkTimer++;if(this.walkTimer>9){this.walkFrame=(this.walkFrame+1)%4;this.walkTimer=0;}
  }
  stomp(){if(!this.shelled){this.shelled=true;this.shellTimer=200;this.vx=0;playKill();}else if(!this.shellKicked){this.shellKicked=true;this.vx=player.x<this.x?8:-8;}}
  draw(){if(!this.alive)return;const sx=Math.round(this.x-cameraX),sy=Math.round(this.y);if(sx<-TILE||sx>CANVAS_W+TILE)return;drawKoopa(sx,sy,this.w,this.h,this.walkFrame,this.shelled);}
}

// ── Coin item ───────────────────────────────────────────────
class CoinItem {
  constructor(x,y){this.x=x;this.y=y;this.vy=-7;this.timer=30;this.alive=true;}
  update(){this.vy+=0.5;this.y+=this.vy;this.timer--;if(this.timer<=0)this.alive=false;}
  draw(){const sx=Math.round(this.x-cameraX),sy=Math.round(this.y);ctx.fillStyle='#f4d03f';ctx.beginPath();ctx.arc(sx+10,sy+10,8,0,Math.PI*2);ctx.fill();ctx.fillStyle='#f39c12';ctx.fillRect(sx+7,sy+6,6,2);ctx.fillRect(sx+7,sy+12,6,2);}
}

// ── Powerup ─────────────────────────────────────────────────
class Powerup {
  constructor(x,y,type='mushroom'){this.x=x;this.y=y;this.type=type;this.vx=2;this.vy=-3;this.onGround=false;this.alive=true;}
  update(){
    this.vy=Math.min(this.vy+GRAVITY,MAX_FALL);this.x+=this.vx;this.y+=this.vy;this.onGround=false;
    const map=levelData.map,H=map.length,W=map[0].length;
    for(let ty=Math.max(0,Math.floor(this.y/TILE));ty<=Math.min(H-1,Math.floor((this.y+TILE-1)/TILE));ty++){
      for(let tx=Math.max(0,Math.floor(this.x/TILE));tx<=Math.min(W-1,Math.floor((this.x+TILE-1)/TILE));tx++){
        if(!isSolid(map[ty][tx]))continue;
        const tL=tx*TILE,tT=ty*TILE,tR=tL+TILE;
        if(this.vy>=0&&this.y+TILE>=tT&&this.y<tT+TILE){this.y=tT-TILE;this.vy=0;this.onGround=true;}
        else if(this.vx>0&&this.x+TILE>=tL){this.x=tL-TILE;this.vx*=-1;}
        else if(this.vx<0&&this.x<=tR){this.x=tR;this.vx*=-1;}
      }
    }
    if(this.x<0||(this.x+TILE)>levelData.map[0].length*TILE)this.vx*=-1;
  }
  draw(){const sx=Math.round(this.x-cameraX),sy=Math.round(this.y);if(this.type==='mushroom')drawMushroom(sx,sy);else drawStar(sx,sy);}
}

// ── Particles & float text ───────────────────────────────────
function spawnParticles(x,y,color='#f4d03f',count=6){for(let i=0;i<count;i++){const a=Math.random()*Math.PI*2,s=2+Math.random()*3;particles.push({x,y,vx:Math.cos(a)*s,vy:Math.sin(a)*s-2,life:30+Math.random()*20,maxLife:50,color,r:3+Math.random()*3});}}
function spawnFloatText(x,y,text,color='#fff'){floatTexts.push({x,y,text,color,life:60,vy:-1.2});}

// ── Tile helpers ─────────────────────────────────────────────
function isSolid(t){return t===1||t===2||t===3||t===4||t===5||t===7||t===8||t===9;}
function hitBlockFromBelow(tx,ty){
  const map=levelData.map,t=map[ty][tx],wx=tx*TILE,wy=ty*TILE;
  if(t===2){if(player.big){map[ty][tx]=0;spawnParticles(wx+TILE/2,wy+TILE/2,'#c0392b',8);score+=50;spawnFloatText(wx-cameraX,wy,'50','#f39c12');}else spawnParticles(wx+TILE/2,wy+TILE/2,'#e07030',4);beep(220,0.1,'square',0.15);}
  else if(t===3){map[ty][tx]=9;beep(523,0.05,'square',0.2);beep(659,0.07,'square',0.18,0.06);if(!player.big){coinItems.push(new CoinItem(wx,wy-TILE));coins++;score+=200;spawnFloatText(wx-cameraX,wy,'200','#f4d03f');playCoin();}else{powerups.push(new Powerup(wx,wy-TILE*2,'mushroom'));}}
}

// ── Pixel-art renderers ──────────────────────────────────────
function drawPlayer(ctx,x,y,w,h,frame,big,jumping,running){
  const s=w/28;
  ctx.fillStyle='#e63946';ctx.fillRect(x+4*s,y+(big?14:8)*s,20*s,(big?20:16)*s);
  ctx.fillStyle='#f4a261';ctx.fillRect(x+6*s,y+(big?4:2)*s,16*s,(big?12:10)*s);
  ctx.fillStyle='#e63946';ctx.fillRect(x+4*s,y+(big?1:0)*s,20*s,(big?5:4)*s);ctx.fillRect(x+7*s,y,14*s,(big?3:2)*s);
  ctx.fillStyle='#1d1d1d';ctx.fillRect(x+10*s,y+(big?8:4)*s,3*s,3*s);
  ctx.fillStyle='#fff';ctx.fillRect(x+11*s,y+(big?8:4)*s,2*s,2*s);
  ctx.fillStyle='#3d1e00';ctx.fillRect(x+7*s,y+(big?12:8)*s,14*s,2*s);
  ctx.fillStyle='#2855a0';ctx.fillRect(x+4*s,y+(big?22:16)*s,20*s,(big?6:5)*s);
  const legY=y+(big?28:21)*s,legH=(big?10:8)*s;
  const lO=jumping?2*s:(frame===1?-3*s:frame===2?3*s:0);
  const rO=jumping?-2*s:(frame===1?3*s:frame===2?-3*s:0);
  ctx.fillStyle='#2855a0';ctx.fillRect(x+4*s,legY+lO,9*s,legH);ctx.fillRect(x+15*s,legY+rO,9*s,legH);
  ctx.fillStyle='#3d1e00';ctx.fillRect(x+3*s,legY+lO+legH,11*s,3*s);ctx.fillRect(x+14*s,legY+rO+legH,11*s,3*s);
  const armY=y+(big?18:13)*s,aO=jumping?-3*s:(frame===1?-2*s:frame===2?2*s:0);
  ctx.fillStyle='#e63946';ctx.fillRect(x,armY+aO,5*s,(big?10:8)*s);ctx.fillRect(x+23*s,armY-aO,5*s,(big?10:8)*s);
}
function drawGoomba(x,y,w,h,frame,squished){
  if(squished){ctx.fillStyle='#8B4513';ctx.fillRect(x,y+h-10,w,10);return;}
  ctx.fillStyle='#8B4513';ctx.fillRect(x+2,y+h*0.35,w-4,h*0.5);
  ctx.fillStyle='#A0522D';ctx.fillRect(x,y,w,h*0.5);
  ctx.fillStyle='#fff';ctx.fillRect(x+4,y+6,8,8);ctx.fillRect(x+w-12,y+6,8,8);
  ctx.fillStyle='#000';ctx.fillRect(x+5,y+8,5,5);ctx.fillRect(x+w-11,y+8,5,5);
  ctx.fillStyle='#000';ctx.fillRect(x+3,y+4,9,2);ctx.fillRect(x+w-12,y+4,9,2);
  ctx.fillStyle='#5D3311';const fO=frame===0?-2:2;
  ctx.fillRect(x+1,y+h-8+fO,w/2-2,8);ctx.fillRect(x+w/2+1,y+h-8-fO,w/2-2,8);
}
function drawKoopa(x,y,w,h,frame,shelled){
  if(shelled){ctx.fillStyle='#27ae60';ctx.beginPath();ctx.ellipse(x+w/2,y+h/2,w/2,h/2.5,0,0,Math.PI*2);ctx.fill();ctx.fillStyle='#2ecc71';ctx.beginPath();ctx.ellipse(x+w/2,y+h/2,w/3,h/4,0,0,Math.PI*2);ctx.fill();return;}
  ctx.fillStyle='#27ae60';ctx.fillRect(x+4,y+h*0.25,w-8,h*0.55);
  ctx.fillStyle='#f9ca24';ctx.fillRect(x+6,y,w-12,h*0.35);
  ctx.fillStyle='#000';ctx.fillRect(x+9,y+5,4,4);ctx.fillRect(x+w-13,y+5,4,4);
  ctx.fillStyle='#f9ca24';const lO=frame%2===0?-2:2;
  ctx.fillRect(x,y+h-14+lO,w/2,10);ctx.fillRect(x+w/2,y+h-14-lO,w/2,10);
}
function drawMushroom(x,y){
  ctx.fillStyle='#e74c3c';ctx.beginPath();ctx.arc(x+20,y+14,18,Math.PI,0);ctx.fill();
  ctx.fillStyle='#fff';[[x+10,y+8,6],[x+26,y+6,5],[x+22,y+16,4]].forEach(([cx,cy,r])=>{ctx.beginPath();ctx.arc(cx,cy,r,0,Math.PI*2);ctx.fill();});
  ctx.fillStyle='#f4d03f';ctx.fillRect(x+12,y+14,16,16);
  ctx.fillStyle='#e67e22';ctx.fillRect(x+10,y+12,3,3);ctx.fillRect(x+27,y+12,3,3);
}
function drawStar(x,y){
  ctx.fillStyle='#f4d03f';ctx.save();ctx.translate(x+20,y+20);ctx.beginPath();
  for(let i=0;i<5;i++){const a=i*Math.PI*2/5-Math.PI/2,b=a+Math.PI/5;i===0?ctx.moveTo(Math.cos(a)*18,Math.sin(a)*18):ctx.lineTo(Math.cos(a)*18,Math.sin(a)*18);ctx.lineTo(Math.cos(b)*8,Math.sin(b)*8);}
  ctx.closePath();ctx.fill();ctx.restore();
}

// ── Tile renderer ─────────────────────────────────────────────
function drawTile(ctx,t,x,y){
  switch(t){
    case 1:ctx.fillStyle=levelData.groundColor||'#e07030';ctx.fillRect(x,y,TILE,TILE);ctx.fillStyle='rgba(0,0,0,0.2)';ctx.fillRect(x,y,TILE,2);ctx.fillStyle='rgba(255,255,255,0.15)';ctx.fillRect(x,y+2,TILE,2);break;
    case 2:ctx.fillStyle='#c0392b';ctx.fillRect(x,y,TILE,TILE);ctx.fillStyle='#922b21';ctx.fillRect(x,y,TILE,2);ctx.fillRect(x,y,2,TILE);ctx.fillStyle='#e74c3c';ctx.fillRect(x+2,y+2,TILE/2-3,TILE/2-3);ctx.fillRect(x+TILE/2+1,y+TILE/2+1,TILE/2-3,TILE/2-3);break;
    case 3:{const p=Math.sin(Date.now()/200)*0.12+0.88;ctx.fillStyle=`rgba(230,180,0,${p})`;ctx.fillRect(x,y,TILE,TILE);ctx.fillStyle='#fff';ctx.font='bold 22px monospace';ctx.textAlign='center';ctx.textBaseline='middle';ctx.fillText('?',x+TILE/2,y+TILE/2);ctx.strokeStyle='rgba(0,0,0,0.3)';ctx.lineWidth=2;ctx.strokeRect(x+1,y+1,TILE-2,TILE-2);break;}
    case 4:ctx.fillStyle='#27ae60';ctx.fillRect(x+4,y,TILE-8,TILE);ctx.fillStyle='#2ecc71';ctx.fillRect(x+6,y,4,TILE);ctx.fillStyle='#1e8449';ctx.fillRect(x+TILE-8,y,4,TILE);break;
    case 5:ctx.fillStyle='#27ae60';ctx.fillRect(x,y+6,TILE,TILE-6);ctx.fillStyle='#2ecc71';ctx.fillRect(x+2,y+8,6,TILE-8);ctx.fillStyle='#1e8449';ctx.fillRect(x+TILE-8,y+6,6,TILE-6);break;
    case 7:ctx.fillStyle='#95a5a6';ctx.fillRect(x+TILE/2-2,y,4,TILE);ctx.fillStyle='#27ae60';ctx.beginPath();ctx.moveTo(x+TILE/2+2,y+2);ctx.lineTo(x+TILE/2+18,y+12);ctx.lineTo(x+TILE/2+2,y+22);ctx.closePath();ctx.fill();break;
    case 8:ctx.fillStyle='#ecf0f1';ctx.fillRect(x,y+8,TILE,TILE-8);ctx.fillStyle='#bdc3c7';ctx.fillRect(x,y+8,TILE,4);ctx.fillStyle='#fff';ctx.beginPath();ctx.arc(x+TILE/2,y+10,12,Math.PI,0);ctx.fill();break;
    case 9:ctx.fillStyle='#95a5a6';ctx.fillRect(x,y,TILE,TILE);ctx.fillStyle='#7f8c8d';ctx.fillRect(x,y,TILE,2);ctx.fillRect(x,y,2,TILE);ctx.fillRect(x,y+TILE-2,TILE,2);ctx.fillRect(x+TILE-2,y,2,TILE);break;
  }
}

// ── Background ───────────────────────────────────────────────
function initBackground(){
  bgStars=[];for(let i=0;i<60;i++)bgStars.push({x:Math.random()*levelData.map[0].length*TILE,y:Math.random()*(CANVAS_H*0.55),r:Math.random()*2+0.5,t:Math.random()*Math.PI*2});
  bgClouds=[];const cc=Math.floor(levelData.map[0].length/8);
  for(let i=0;i<cc;i++)bgClouds.push({x:i*8*TILE+Math.random()*4*TILE,y:30+Math.random()*80,w:80+Math.random()*80,speed:0.2+Math.random()*0.3});
}
function drawBackground(){
  const g=ctx.createLinearGradient(0,0,0,CANVAS_H);
  if(levelData.bgColor==='#000014'){g.addColorStop(0,'#000014');g.addColorStop(1,'#0a0a30');}
  else{g.addColorStop(0,'#5c94fc');g.addColorStop(0.6,'#87ceeb');g.addColorStop(1,'#c3e8ff');}
  ctx.fillStyle=g;ctx.fillRect(0,0,CANVAS_W,CANVAS_H);
  if(levelData.bgColor==='#000014'){bgStars.forEach(s=>{s.t+=0.02;const a=0.5+Math.sin(s.t)*0.4;ctx.fillStyle=`rgba(255,255,255,${a})`;ctx.beginPath();ctx.arc(s.x-cameraX*0.1,s.y,s.r,0,Math.PI*2);ctx.fill();});}
  else{bgClouds.forEach(c=>drawCloud(c.x-cameraX*0.4,c.y,c.w));drawHills();}
}
function drawCloud(x,y,w){ctx.fillStyle='rgba(255,255,255,0.9)';ctx.beginPath();ctx.arc(x+w*0.25,y+18,18,0,Math.PI*2);ctx.fill();ctx.beginPath();ctx.arc(x+w*0.5,y+12,24,0,Math.PI*2);ctx.fill();ctx.beginPath();ctx.arc(x+w*0.75,y+18,18,0,Math.PI*2);ctx.fill();ctx.fillRect(x+w*0.1,y+18,w*0.8,18);}
function drawHills(){ctx.fillStyle='rgba(100,200,80,0.35)';const hx=-cameraX*0.25;ctx.beginPath();ctx.moveTo(0,CANVAS_H*0.7);for(let x=0;x<=CANVAS_W;x+=10)ctx.lineTo(x,CANVAS_H*0.7+Math.sin((x+hx)*0.008)*40-20);ctx.lineTo(CANVAS_W,CANVAS_H);ctx.lineTo(0,CANVAS_H);ctx.closePath();ctx.fill();}

function drawMap(){
  const map=levelData.map,H=map.length,W=map[0].length;
  const s=Math.max(0,Math.floor(cameraX/TILE)-1),e=Math.min(W-1,Math.ceil((cameraX+CANVAS_W)/TILE)+1);
  for(let ty=0;ty<H;ty++)for(let tx=s;tx<=e;tx++){const t=map[ty][tx];if(t!==0)drawTile(ctx,t,tx*TILE-cameraX,ty*TILE);}
}

// ── Collision helpers ─────────────────────────────────────────
function overlaps(a,b){return a.x<b.x+b.w&&a.x+a.w>b.x&&a.y<b.y+b.h&&a.y+a.h>b.y;}
function checkPlayerEnemyCollision(){
  if(player.invincible>0)return;
  const ph=player.bigH,pe={x:player.x,y:player.y,w:player.w,h:ph};
  for(const e of enemies){
    if(!e.alive||e.squished)continue;
    if(overlaps(pe,{x:e.x,y:e.y,w:e.w,h:e.h})){
      if(player.vy>0&&player.y+ph-player.vy<=e.y+4){
        if(e instanceof Goomba)e.squish();else if(e instanceof Koopa)e.stomp();
        player.vy=-8;score+=100;spawnFloatText(e.x-cameraX,e.y,'100','#fff');spawnParticles(e.x+e.w/2,e.y+e.h/2,'#e74c3c');
      }else hurtPlayer();
    }
  }
}
function checkPlayerPowerup(){for(const p of powerups){if(!p.alive)continue;if(overlaps({x:player.x,y:player.y,w:player.w,h:player.bigH},{x:p.x,y:p.y,w:TILE,h:TILE})){p.alive=false;if(p.type==='mushroom')player.big=true;score+=1000;spawnFloatText(p.x-cameraX,p.y,'1000','#e74c3c');spawnParticles(p.x+TILE/2,p.y+TILE/2,'#e74c3c',10);playPowerup();}}}
function checkPlayerCoins(){for(const c of coinItems){if(!c.alive)continue;if(overlaps({x:player.x,y:player.y,w:player.w,h:player.bigH},{x:c.x,y:c.y,w:20,h:20})){c.alive=false;coins++;score+=200;spawnFloatText(c.x-cameraX,c.y,'COIN','#f4d03f');playCoin();}}}
function checkPlayerFlag(){const map=levelData.map,ph=player.bigH,tx=Math.floor((player.x+player.w/2)/TILE),ty=Math.floor((player.y+ph/2)/TILE);if(ty>=0&&ty<map.length&&tx>=0&&tx<map[0].length&&map[ty][tx]===7)levelComplete();}
function hurtPlayer(){
  if(player.invincible>0)return;
  if(player.big){player.big=false;player.invincible=120;playHurt();}
  else{lives--;updateHUD();if(lives<=0){gameOver();return;}playHurt();spawnParticles(player.x+player.w/2,player.y+player.h/2,'#e74c3c',12);resetPlayer();}
}
function updateCamera(){const t=player.x-CANVAS_W/3,mx=levelData.map[0].length*TILE-CANVAS_W;cameraX+=(t-cameraX)*0.12;cameraX=Math.max(0,Math.min(mx,cameraX));}

// ── Game flow ─────────────────────────────────────────────────
function initLevel(idx){
  levelData=LEVELS[idx];elWorld.textContent=levelData.name;timeLeft=levelData.timeLimit;cameraX=0;
  particles=[];floatTexts=[];coinItems=[];powerups=[];enemies=[];
  spawnLevelEnemies(idx);initBackground();resetPlayer();
}
function spawnLevelEnemies(idx){
  if(idx===0){[18,22,27,34,40,45].forEach(tx=>enemies.push(new Goomba(tx*TILE,(LEVELS[0].map.length-3)*TILE)));[50,54].forEach(tx=>enemies.push(new Koopa(tx*TILE,(LEVELS[0].map.length-3)*TILE)));}
  else if(idx===1){[10,16,24,34,42].forEach(tx=>enemies.push(new Goomba(tx*TILE,9*TILE)));[28,38].forEach(tx=>enemies.push(new Koopa(tx*TILE,9*TILE)));}
  else{[12,20,30,40,46].forEach(tx=>enemies.push(new Goomba(tx*TILE,8*TILE)));}
}
function resetPlayer(){const map=levelData.map;player=new Player(2*TILE,(map.length-3)*TILE-52);}

function startGame(){
  score=0;lives=3;coins=0;currentLevel=0;updateHUD();hideAllOverlays();startTimer();initLevel(0);state='playing';
  if(AC.state==='suspended')AC.resume();
  requestAnimationFrame(loop);
}
function togglePause(){
  if(state==='playing'){state='paused';showOverlay('pauseScreen');stopTimer();}
  else if(state==='paused'){state='playing';hideAllOverlays();startTimer();requestAnimationFrame(loop);}
}
function gameOver(){state='gameover';stopTimer();document.getElementById('finalScore').textContent='Score: '+score;showOverlay('gameOverScreen');}
function levelComplete(){
  state='levelcomplete';stopTimer();const tb=Math.ceil(timeLeft)*50;score+=tb;playFlagpole();
  document.getElementById('levelScore').textContent='Score: '+score;
  document.getElementById('timeBonus').textContent='Time Bonus: '+tb;
  showOverlay('levelCompleteScreen');
}
function nextLevel(){currentLevel++;if(currentLevel>=LEVELS.length){currentLevel=0;score=0;}hideAllOverlays();initLevel(currentLevel);startTimer();state='playing';requestAnimationFrame(loop);}
function startTimer(){stopTimer();timerInterval=setInterval(()=>{if(state!=='playing')return;timeLeft--;if(timeLeft<=0){timeLeft=0;hurtPlayer();timeLeft=levelData.timeLimit;}updateHUD();},1000);}
function stopTimer(){if(timerInterval){clearInterval(timerInterval);timerInterval=null;}}
function showOverlay(id){document.querySelectorAll('.overlay').forEach(o=>o.classList.add('hidden'));document.getElementById(id).classList.remove('hidden');}
function hideAllOverlays(){document.querySelectorAll('.overlay').forEach(o=>o.classList.add('hidden'));}
function handleEnterKey(){if(state==='start')startGame();else if(state==='paused')togglePause();}

// ── Main loop ─────────────────────────────────────────────────
function loop(){
  if(state!=='playing')return;
  update();render();updateHUD();
  requestAnimationFrame(loop);
}
function update(){
  player.update();
  enemies.forEach(e=>e.update());coinItems.forEach(c=>c.update());powerups.forEach(p=>p.update());
  enemies=enemies.filter(e=>e.alive);coinItems=coinItems.filter(c=>c.alive);powerups=powerups.filter(p=>p.alive);
  particles.forEach(p=>{p.x+=p.vx;p.y+=p.vy;p.vy+=0.15;p.life--;});particles=particles.filter(p=>p.life>0);
  floatTexts.forEach(t=>{t.y+=t.vy;t.life--;});floatTexts=floatTexts.filter(t=>t.life>0);
  checkPlayerEnemyCollision();checkPlayerPowerup();checkPlayerCoins();checkPlayerFlag();
  updateCamera();
  if(player.y>CANVAS_H+100)hurtPlayer();
}
function render(){
  ctx.clearRect(0,0,CANVAS_W,CANVAS_H);
  drawBackground();drawMap();
  enemies.forEach(e=>e.draw());coinItems.forEach(c=>c.draw());powerups.forEach(p=>p.draw());
  player.draw();
  particles.forEach(p=>{const a=p.life/p.maxLife;ctx.globalAlpha=a;ctx.fillStyle=p.color;ctx.beginPath();ctx.arc(Math.round(p.x-cameraX),Math.round(p.y),p.r,0,Math.PI*2);ctx.fill();});
  ctx.globalAlpha=1;
  floatTexts.forEach(t=>{ctx.globalAlpha=Math.min(1,t.life/20);ctx.fillStyle=t.color;ctx.font='bold 16px monospace';ctx.textAlign='center';ctx.textBaseline='top';ctx.fillText(t.text,Math.round(t.x),Math.round(t.y));});
  ctx.globalAlpha=1;
}

// ── Button wiring ─────────────────────────────────────────────
document.getElementById('startBtn').addEventListener('click',startGame);
document.getElementById('resumeBtn').addEventListener('click',togglePause);
document.getElementById('restartBtn').addEventListener('click',()=>{hideAllOverlays();startGame();});
document.getElementById('playAgainBtn').addEventListener('click',startGame);
document.getElementById('nextLevelBtn').addEventListener('click',nextLevel);

// ── Boot ──────────────────────────────────────────────────────
showOverlay('startScreen');
updateHUD();
