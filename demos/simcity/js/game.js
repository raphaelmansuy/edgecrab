// =============================================================================
// SimCity Builder — game engine (Canvas 2D, no external deps)
// Grid-based city sim: zoning, roads, services, power, budget, population,
// happiness, and a real-time simulation loop.
// =============================================================================

'use strict';

// -----------------------------------------------------------------------------
// Configuration & tool definitions
// -----------------------------------------------------------------------------

const GRID_W = 32;          // tiles across
const GRID_H = 32;          // tiles down
const TILE = 28;            // base tile size in px (scaled to fit)
const TICK_MS = 1000;       // simulation step interval (1 sim-second)
const START_MONEY = 50000;

// Tool catalog. cost = build cost (bulldozer is refund-ish removal cost).
// maintenance = per-tick upkeep. category drives the simulation model.
const TOOLS = {
  residential: { name: 'Residential Zone', cost: 500,  color: '#e74c3c', emoji: '🏠', category: 'zone' },
  commercial:  { name: 'Commercial Zone',  cost: 500,  color: '#3498db', emoji: '🏢', category: 'zone' },
  industrial:  { name: 'Industrial Zone',  cost: 500,  color: '#f39c12', emoji: '🏭', category: 'zone' },
  road:        { name: 'Road',             cost: 100,  color: '#34495e', emoji: '🛣️', category: 'road' },
  park:        { name: 'Park',             cost: 200,  color: '#27ae60', emoji: '🌳', category: 'service', service: 'park', radius: 6, maintenance: 5 },
  police:      { name: 'Police Station',   cost: 1500, color: '#1f6f9c', emoji: '🚓', category: 'service', service: 'police', radius: 7, maintenance: 30 },
  hospital:    { name: 'Hospital',         cost: 2500, color: '#c0392b', emoji: '🏥', category: 'service', service: 'hospital', radius: 8, maintenance: 50 },
  school:      { name: 'School',           cost: 2000, color: '#8e44ad', emoji: '🏫', category: 'service', service: 'school', radius: 7, maintenance: 40 },
  power:       { name: 'Power Plant',      cost: 3000, color: '#f1c40f', emoji: '⚡', category: 'power', radius: 14, maintenance: 80 },
  bulldozer:   { name: 'Bulldozer',        cost: 10,   color: '#7f8c8d', emoji: '🧹', category: 'bulldoze' },
};

// -----------------------------------------------------------------------------
// State
// -----------------------------------------------------------------------------

const state = {
  grid: [],            // grid[y][x] = tile
  money: START_MONEY,
  population: 0,
  jobs: 0,
  happiness: 100,      // 0..100
  taxRate: 5,          // percent
  tool: 'residential',
  running: false,
  paused: false,
  speed: 1,            // 1x / 2x / 4x
  day: 1,
  stats: { r: 0, c: 0, i: 0, roads: 0, powered: 0, unemployed: 0, unserved: 0 },
  history: [],         // population history for sparkline-ish stats
};

function makeTile() {
  return {
    type: 'empty',       // empty | residential | commercial | industrial | road | park | police | hospital | school | power
    level: 0,            // development level 0..3 (for zones)
    pop: 0,              // residents on this tile
    jobs: 0,             // jobs offered by this tile
    powered: false,
    serviced: { park: false, police: false, hospital: false, school: false },
    value: 0,            // land value 0..1
  };
}

function initGrid() {
  state.grid = [];
  for (let y = 0; y < GRID_H; y++) {
    const row = [];
    for (let x = 0; x < GRID_W; x++) row.push(makeTile());
    state.grid.push(row);
  }
}

// -----------------------------------------------------------------------------
// Canvas setup & camera
// -----------------------------------------------------------------------------

const canvas = document.getElementById('game-canvas');
const ctx = canvas.getContext('2d');

const cam = { x: 0, y: 0, scale: 1 };

function resizeCanvas() {
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.floor(window.innerWidth * dpr);
  canvas.height = Math.floor(window.innerHeight * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  fitCamera();
}

function fitCamera() {
  // Scale so the whole grid fits with margin, then center it.
  const margin = 60;
  const availW = window.innerWidth - margin * 2;
  const availH = window.innerHeight - margin * 2 - 180; // leave room for HUD/toolbar
  const fit = Math.min(availW / (GRID_W * TILE), availH / (GRID_H * TILE));
  cam.scale = Math.max(0.4, Math.min(2.2, fit));
  const worldW = GRID_W * TILE * cam.scale;
  const worldH = GRID_H * TILE * cam.scale;
  cam.x = (window.innerWidth - worldW) / 2;
  cam.y = (window.innerHeight - worldH) / 2 + 10;
}

// -----------------------------------------------------------------------------
// Coordinate helpers
// -----------------------------------------------------------------------------

function screenToTile(sx, sy) {
  const wx = (sx - cam.x) / cam.scale;
  const wy = (sy - cam.y) / cam.scale;
  const x = Math.floor(wx / TILE);
  const y = Math.floor(wy / TILE);
  if (x < 0 || y < 0 || x >= GRID_W || y >= GRID_H) return null;
  return { x, y };
}

// -----------------------------------------------------------------------------
// Rendering
// -----------------------------------------------------------------------------

function draw() {
  ctx.clearRect(0, 0, window.innerWidth, window.innerHeight);
  ctx.save();
  ctx.translate(cam.x, cam.y);
  ctx.scale(cam.scale, cam.scale);

  // Tiles
  for (let y = 0; y < GRID_H; y++) {
    for (let x = 0; x < GRID_W; x++) {
      drawTile(x, y);
    }
  }

  // Grid lines
  ctx.strokeStyle = 'rgba(255,255,255,0.06)';
  ctx.lineWidth = 1;
  for (let x = 0; x <= GRID_W; x++) {
    ctx.beginPath();
    ctx.moveTo(x * TILE, 0);
    ctx.lineTo(x * TILE, GRID_H * TILE);
    ctx.stroke();
  }
  for (let y = 0; y <= GRID_H; y++) {
    ctx.beginPath();
    ctx.moveTo(0, y * TILE);
    ctx.lineTo(GRID_W * TILE, y * TILE);
    ctx.stroke();
  }

  ctx.restore();

  // Hover highlight (drawn in screen space)
  if (hover) {
    const t = hover;
    ctx.save();
    ctx.translate(cam.x, cam.y);
    ctx.scale(cam.scale, cam.scale);
    const tool = TOOLS[state.tool];
    ctx.lineWidth = 2 / cam.scale;
    ctx.strokeStyle = state.tool === 'bulldozer' ? '#e74c3c' : (canAfford(tool) ? '#2ecc71' : '#e74c3c');
    ctx.strokeRect(t.x * TILE + 1, t.y * TILE + 1, TILE - 2, TILE - 2);
    ctx.restore();
  }
}

function drawTile(x, y) {
  const t = state.grid[y][x];
  const px = x * TILE;
  const py = y * TILE;

  // base ground
  let base = '#3a5f3a';
  if (t.type === 'empty') {
    // subtle checker for empty land
    base = (x + y) % 2 === 0 ? '#2f6b3a' : '#2c6336';
  } else if (t.type === 'road') {
    base = '#34495e';
  } else {
    base = shadeFor(t);
  }

  ctx.fillStyle = base;
  ctx.fillRect(px, py, TILE, TILE);

  if (t.type === 'empty') return;

  // power indicator (small dot for powered zones)
  if ((t.type === 'residential' || t.type === 'commercial' || t.type === 'industrial') && !t.powered) {
    ctx.fillStyle = 'rgba(0,0,0,0.35)';
    ctx.fillRect(px, py, TILE, TILE);
  }

  // road detail
  if (t.type === 'road') {
    ctx.strokeStyle = 'rgba(255,255,255,0.25)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(px + TILE / 2, py);
    ctx.lineTo(px + TILE / 2, py + TILE);
    ctx.moveTo(px, py + TILE / 2);
    ctx.lineTo(px + TILE, py + TILE / 2);
    ctx.stroke();
    return;
  }

  // building body for zones (height by level)
  if (t.type === 'residential' || t.type === 'commercial' || t.type === 'industrial') {
    const lvl = t.level;
    const pad = 3 + (3 - lvl) * 1.5;
    const bw = TILE - pad * 2;
    const bh = (TILE - pad * 2) * (0.5 + lvl * 0.18);
    const bx = px + pad;
    const by = py + (TILE - pad) - bh;
    ctx.fillStyle = buildingColor(t.type, lvl);
    ctx.fillRect(bx, by, bw, bh);
    // windows
    ctx.fillStyle = 'rgba(255,255,255,0.35)';
    for (let r = 0; r < lvl + 1; r++) {
      for (let c = 0; c < 2; c++) {
        ctx.fillRect(bx + 2 + c * (bw / 2), by + 2 + r * (bh / (lvl + 2)), bw / 3, bh / (lvl + 3));
      }
    }
    return;
  }

  // service / power buildings: colored block + emoji glyph (drawn via fillText)
  ctx.fillStyle = buildingColor(t.type, 1);
  ctx.fillRect(px + 3, py + 3, TILE - 6, TILE - 6);
  const glyph = TOOLS[t.type]?.emoji || '?';
  ctx.font = `${Math.floor(TILE * 0.6)}px serif`;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(glyph, px + TILE / 2, py + TILE / 2 + 1);
}

function shadeFor(t) {
  return '#2c6336';
}

function buildingColor(type, level) {
  const base = {
    residential: '#e74c3c',
    commercial: '#3498db',
    industrial: '#f39c12',
    park: '#27ae60',
    police: '#1f6f9c',
    hospital: '#c0392b',
    school: '#8e44ad',
    power: '#f1c40f',
  }[type] || '#888';
  return base;
}

// -----------------------------------------------------------------------------
// Build / bulldoze logic
// -----------------------------------------------------------------------------

function canAfford(tool) {
  return state.money >= tool.cost;
}

function isRoadAdjacent(x, y) {
  const dirs = [[1, 0], [-1, 0], [0, 1], [0, -1]];
  for (const [dx, dy] of dirs) {
    const nx = x + dx, ny = y + dy;
    if (nx < 0 || ny < 0 || nx >= GRID_W || ny >= GRID_H) continue;
    if (state.grid[ny][nx].type === 'road') return true;
  }
  return false;
}

function placeTool(x, y) {
  const tool = TOOLS[state.tool];
  const tile = state.grid[y][x];

  if (state.tool === 'bulldozer') {
    if (tile.type === 'empty') return;
    const refund = Math.floor((TOOLS[tile.type]?.cost || 0) * 0.25);
    tile.type = 'empty';
    tile.level = 0; tile.pop = 0; tile.jobs = 0; tile.powered = false;
    tile.value = 0; tile.serviced = { park: false, police: false, hospital: false, school: false };
    spend(-refund);
    toast(`Demolished (+$${refund})`);
    recalcStats();
    return;
  }

  // Can't overwrite non-empty tiles
  if (tile.type !== 'empty') return;

  if (!canAfford(tool)) {
    toast('Not enough funds!');
    flashMoney();
    return;
  }

  // Zones require adjacent road to develop later (allowed to place anywhere,
  // but only grow when connected to the road network).
  tile.type = state.tool;
  tile.level = (tool.category === 'zone') ? 0 : 1;
  spend(tool.cost);
  recalcStats();
}

function spend(amount) {
  state.money -= amount;
  updateHUD();
}

// -----------------------------------------------------------------------------
// Simulation
// -----------------------------------------------------------------------------

function neighborsOf(x, y) {
  const res = [];
  const dirs = [[1, 0], [-1, 0], [0, 1], [0, -1]];
  for (const [dx, dy] of dirs) {
    const nx = x + dx, ny = y + dy;
    if (nx < 0 || ny < 0 || nx >= GRID_W || ny >= GRID_H) continue;
    res.push(state.grid[ny][nx]);
  }
  return res;
}

// Flood-fill connectivity from roads to determine zoned tiles connected to network.
function computeRoadConnectivity() {
  const connected = Array.from({ length: GRID_H }, () => new Array(GRID_W).fill(false));
  const visited = Array.from({ length: GRID_H }, () => new Array(GRID_W).fill(false));
  const queue = [];
  for (let y = 0; y < GRID_H; y++) {
    for (let x = 0; x < GRID_W; x++) {
      if (state.grid[y][x].type === 'road') {
        queue.push([x, y]);
        visited[y][x] = true;
        connected[y][x] = true;
      }
    }
  }
  while (queue.length) {
    const [x, y] = queue.shift();
    for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
      const nx = x + dx, ny = y + dy;
      if (nx < 0 || ny < 0 || nx >= GRID_W || ny >= GRID_H) continue;
      if (visited[ny][nx]) continue;
      // a zone tile is connected if adjacent to a road-connected tile
      if (state.grid[ny][nx].type !== 'empty') {
        connected[ny][nx] = true;
        visited[ny][nx] = true;
        // roads propagate further
        if (state.grid[ny][nx].type === 'road') queue.push([nx, ny]);
      }
    }
  }
  return connected;
}

function inRadius(sx, sy, tx, ty, r) {
  return Math.abs(sx - tx) <= r && Math.abs(sy - ty) <= r;
}

function findServices() {
  const svc = { park: [], police: [], hospital: [], school: [] };
  const power = [];
  for (let y = 0; y < GRID_H; y++) {
    for (let x = 0; x < GRID_W; x++) {
      const t = state.grid[y][x];
      if (t.type === 'power') power.push({ x, y, r: TOOLS.power.radius });
      else if (TOOLS[t.type]?.category === 'service') {
        svc[TOOLS[t.type].service].push({ x, y, r: TOOLS[t.type].radius });
      }
    }
  }
  return { svc, power };
}

function isPowered(x, y, powerList) {
  for (const p of powerList) {
    if (inRadius(p.x, p.y, x, y, p.r)) return true;
  }
  return false;
}

function isServiced(x, y, list) {
  for (const s of list) {
    if (inRadius(s.x, s.y, x, y, s.r)) return true;
  }
  return false;
}

// Per-tick simulation step. Grows zones, computes pop/jobs/power/services/happiness.
function simulateStep() {
  const connected = computeRoadConnectivity();
  const { svc, power } = findServices();

  let pop = 0, jobs = 0, poweredCount = 0, unserved = 0;
  let rCount = 0, cCount = 0, iCount = 0, roadCount = 0;

  // First pass: determine power & service coverage + grow/demote zones.
  for (let y = 0; y < GRID_H; y++) {
    for (let x = 0; x < GRID_W; x++) {
      const t = state.grid[y][x];
      if (t.type === 'empty' || t.type === 'road' || t.type === 'park' ||
          t.type === 'police' || t.type === 'hospital' || t.type === 'school') {
        if (t.type === 'road') roadCount++;
        continue;
      }

      const net = connected[y][x];
      const pwr = isPowered(x, y, power);
      t.powered = pwr;

      // service coverage for residential/commercial/industrial zones
      const s = {
        park: isServiced(x, y, svc.park),
        police: isServiced(x, y, svc.police),
        hospital: isServiced(x, y, svc.hospital),
        school: isServiced(x, y, svc.school),
      };
      t.serviced = s;

      // Land value: parks & services nearby raise value; pollution (industrial) lowers.
      let val = 0.3;
      if (s.park) val += 0.15;
      if (s.school) val += 0.1;
      if (s.police) val += 0.05;
      if (s.hospital) val += 0.1;
      t.value = Math.max(0, Math.min(1, val));

      if (t.type === 'residential') {
        rCount++;
        if (net && pwr) {
          poweredCount++;
          // grow toward capacity based on value & service coverage
          const want = 1 + Math.round(t.value * 3); // level target 1..4 -> clamp 0..3
          if (t.level < Math.min(3, want)) t.level = Math.min(3, want);
          const cap = (t.level + 1) * 8; // residents per level
          t.pop = cap;
          pop += t.pop;
          if (!(s.police && s.school)) unserved++;
        } else {
          // no power / not connected -> decline
          t.level = Math.max(0, t.level - 1);
          t.pop = 0;
        }
      } else if (t.type === 'commercial') {
        cCount++;
        if (net && pwr) {
          poweredCount++;
          const want = 1 + Math.round(t.value * 2);
          if (t.level < Math.min(3, want)) t.level = Math.min(3, want);
          const j = (t.level + 1) * 6;
          t.jobs = j;
          jobs += t.jobs;
        } else {
          t.level = Math.max(0, t.level - 1);
          t.jobs = 0;
        }
      } else if (t.type === 'industrial') {
        iCount++;
        if (net && pwr) {
          poweredCount++;
          const j = (t.level + 1) * 10;
          t.jobs = j;
          jobs += t.jobs;
        } else {
          t.level = Math.max(0, t.level - 1);
          t.jobs = 0;
        }
      }
    }
  }

  // Happiness model
  const totalZones = rCount + cCount + iCount;
  let happy = 100;
  if (totalZones > 0) {
    const serviceRatio = 1 - (unserved / Math.max(1, rCount));
    happy -= (1 - serviceRatio) * 35;          // missing services
    const jobBalance = jobs / Math.max(1, pop || 1);
    if (jobBalance < 0.4) happy -= (0.4 - jobBalance) * 80;  // not enough jobs
    if (jobBalance > 2.5) happy -= (jobBalance - 2.5) * 10;  // too industrial
    if (poweredCount < (rCount + cCount + iCount)) happy -= 20; // blackouts
    // tax burden
    happy -= Math.max(0, (state.taxRate - 8)) * 2.5;
    happy += Math.min(8, Math.floor(rCount > 0 ? (svc.park.length * 2) : 0));
  }
  state.happiness = Math.max(0, Math.min(100, Math.round(happy)));

  // Economy: collect taxes, pay maintenance
  const income = Math.round(pop * (state.taxRate / 100) * 2.2); // tax per resident
  let upkeep = 0;
  for (let y = 0; y < GRID_H; y++) {
    for (let x = 0; x < GRID_W; x++) {
      const t = state.grid[y][x];
      upkeep += TOOLS[t.type]?.maintenance || 0;
    }
  }

  state.money += income - upkeep;
  state.population = pop;
  state.jobs = jobs;
  state.day += 1;
  state.stats = {
    r: rCount, c: cCount, i: iCount, roads: roadCount,
    powered: poweredCount, unemployed: Math.max(0, Math.round(pop - jobs)),
    unserved,
  };
  state.history.push(pop);
  if (state.history.length > 60) state.history.shift();

  updateHUD();
  if (state.money < 0) {
    toast('⚠️ Budget deficit! Build tax base or lower spending.');
  }
}

// -----------------------------------------------------------------------------
// HUD / UI wiring
// -----------------------------------------------------------------------------

const el = {
  money: document.getElementById('money-value'),
  pop: document.getElementById('pop-value'),
  happy: document.getElementById('happy-value'),
  tax: document.getElementById('tax-value'),
  buildName: document.getElementById('build-name'),
  buildCost: document.getElementById('build-cost'),
  overlay: document.getElementById('overlay'),
  startBtn: document.getElementById('start-btn'),
  toast: document.getElementById('toast'),
  toolbar: document.getElementById('toolbar'),
  moneyPanel: document.getElementById('money-panel'),
};

function fmtMoney(n) {
  const v = Math.round(n);
  return (v < 0 ? '-$' : '$') + Math.abs(v).toLocaleString('en-US');
}

function updateHUD() {
  el.money.textContent = fmtMoney(state.money);
  el.pop.textContent = state.population.toLocaleString('en-US');
  el.happy.textContent = state.happiness + '%';
  el.tax.textContent = state.taxRate + '%';
}

function selectTool(name) {
  state.tool = name;
  document.querySelectorAll('.tool-btn').forEach(b => {
    b.classList.toggle('active', b.dataset.tool === name);
  });
  const tool = TOOLS[name];
  el.buildName.textContent = tool.name;
  el.buildCost.textContent = name === 'bulldozer' ? `$${tool.cost} / tile` : `$${tool.cost}`;
}

function flashMoney() {
  el.moneyPanel.style.transition = 'transform 0.1s';
  el.moneyPanel.style.transform = 'scale(1.15)';
  setTimeout(() => { el.moneyPanel.style.transform = 'scale(1)'; }, 120);
}

let toastTimer = null;
function toast(msg) {
  el.toast.textContent = msg;
  el.toast.hidden = false;
  el.toast.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    el.toast.classList.remove('show');
    setTimeout(() => { el.toast.hidden = true; }, 250);
  }, 1800);
}

function recalcStats() {
  // lightweight recount for immediate HUD feedback on build/demolish
  let r = 0, c = 0, i = 0, roads = 0;
  for (let y = 0; y < GRID_H; y++) {
    for (let x = 0; x < GRID_W; x++) {
      const t = state.grid[y][x];
      if (t.type === 'residential') r++;
      else if (t.type === 'commercial') c++;
      else if (t.type === 'industrial') i++;
      else if (t.type === 'road') roads++;
    }
  }
  state.stats.r = r; state.stats.c = c; state.stats.i = i; state.stats.roads = roads;
}

// -----------------------------------------------------------------------------
// Input handling
// -----------------------------------------------------------------------------

let hover = null;
let painting = false;

function pointerTile(e) {
  const rect = canvas.getBoundingClientRect();
  const sx = (e.clientX ?? (e.touches && e.touches[0].clientX)) - rect.left;
  const sy = (e.clientY ?? (e.touches && e.touches[0].clientY)) - rect.top;
  return screenToTile(sx, sy);
}

function onDown(e) {
  if (!state.running) return;
  const t = pointerTile(e);
  if (!t) return;
  painting = true;
  placeTool(t.x, t.y);
}

function onMove(e) {
  const t = pointerTile(e);
  hover = t;
  if (painting && t && state.running) {
    placeTool(t.x, t.y);
  }
}

function onUp() { painting = false; }

canvas.addEventListener('mousedown', onDown);
canvas.addEventListener('mousemove', onMove);
window.addEventListener('mouseup', onUp);
canvas.addEventListener('touchstart', (e) => { e.preventDefault(); onDown(e); }, { passive: false });
canvas.addEventListener('touchmove', (e) => { e.preventDefault(); onMove(e); }, { passive: false });
window.addEventListener('touchend', onUp);

// Keyboard: 1-9 tools, space pause, +/- speed
const toolOrder = ['residential', 'commercial', 'industrial', 'road', 'park', 'police', 'hospital', 'school', 'power', 'bulldozer'];
window.addEventListener('keydown', (e) => {
  if (e.key >= '1' && e.key <= '9') {
    const idx = parseInt(e.key, 10) - 1;
    if (toolOrder[idx]) selectTool(toolOrder[idx]);
  } else if (e.key === '0') {
    selectTool('bulldozer');
  } else if (e.key === ' ') {
    e.preventDefault();
    state.paused = !state.paused;
    toast(state.paused ? '⏸ Paused' : '▶ Resumed');
  }
});

el.toolbar.addEventListener('click', (e) => {
  const btn = e.target.closest('.tool-btn');
  if (btn) selectTool(btn.dataset.tool);
});

// Tax control: click tax panel to cycle 0/5/10/15/20
el.tax.parentElement.addEventListener('click', () => {
  const steps = [0, 5, 10, 15, 20];
  const idx = steps.indexOf(state.taxRate);
  state.taxRate = steps[(idx + 1) % steps.length];
  updateHUD();
  toast(`Tax rate: ${state.taxRate}%`);
});

// Speed control: double-click money panel cycles speed
el.money.parentElement.addEventListener('dblclick', () => {
  state.speed = state.speed === 1 ? 2 : state.speed === 2 ? 4 : 1;
  toast(`Speed: ${state.speed}x`);
});

// -----------------------------------------------------------------------------
// Main loop
// -----------------------------------------------------------------------------

let lastTick = 0;
function loop(ts) {
  draw();
  const interval = TICK_MS / state.speed;
  if (state.running && !state.paused && ts - lastTick >= interval) {
    lastTick = ts;
    simulateStep();
  }
  requestAnimationFrame(loop);
}

function startGame() {
  state.running = true;
  el.overlay.classList.add('hidden');
  toast('Welcome, Mayor! 🏙️');
}

// -----------------------------------------------------------------------------
// Boot
// -----------------------------------------------------------------------------

function boot() {
  initGrid();
  resizeCanvas();
  selectTool('residential');
  updateHUD();
  recalcStats();
  window.addEventListener('resize', () => { resizeCanvas(); });
  el.startBtn.addEventListener('click', startGame);
  requestAnimationFrame(loop);
}

boot();