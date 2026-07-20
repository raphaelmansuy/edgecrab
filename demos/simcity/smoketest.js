// Headless smoke test for simcity/js/game.js
// Provides minimal DOM/Canvas/window stubs so boot() can run, then drives the
// simulation directly to confirm population/jobs/taxes/power behave.
//
// Run with:  node smoketest.js   (or `make smoke`)

const fs = require('fs');
const path = require('path');
const vm = require('vm');

function makeEl() {
  return {
    textContent: '', hidden: false, style: {},
    classList: { add() {}, remove() {}, toggle() {} },
    addEventListener() {}, dataset: {},
    parentElement: { addEventListener() {} },
  };
}

const elements = {};
function getEl(id) { return elements[id] || (elements[id] = makeEl()); }

// canvas + 2d ctx stub (Proxy so any drawing call is a no-op)
const ctxStub = new Proxy({}, {
  get(_t, prop) {
    const noops = ['setTransform', 'clearRect', 'save', 'translate', 'scale',
      'restore', 'fillRect', 'strokeRect', 'beginPath', 'moveTo', 'lineTo',
      'stroke', 'fillText', 'closePath', 'arc', 'fill'];
    if (noops.includes(prop)) return () => {};
    return ''; // fillStyle, strokeStyle, font, etc.
  },
  set() { return true; },
});

const canvasStub = {
  getContext: () => ctxStub,
  width: 0, height: 0,
  getBoundingClientRect: () => ({ left: 0, top: 0 }),
  addEventListener() {},
};

const documentStub = {
  getElementById: (id) => (id === 'game-canvas' ? canvasStub : getEl(id)),
  querySelectorAll: () => [],
  addEventListener() {},
};

const windowStub = {
  innerWidth: 1280, innerHeight: 800, devicePixelRatio: 1,
  addEventListener() {},
};

const sandbox = {
  document: documentStub,
  window: windowStub,
  requestAnimationFrame: () => 0,
  setTimeout: () => 0,
  clearTimeout: () => {},
  console,
  Math, Array, Object, JSON, parseInt, isNaN,
};
sandbox.globalThis = sandbox;

// game.js is a module-style script (top-level const/let are block-scoped to the
// script). Append an export shim that publishes the symbols we need for testing,
// then boot() runs (it also calls requestAnimationFrame, which is a no-op here).
let code = fs.readFileSync(path.join(__dirname, 'js', 'game.js'), 'utf8');
code += '\n;globalThis.__exports = { state, TOOLS, placeTool, simulateStep };\n';

vm.createContext(sandbox);
vm.runInContext(code, sandbox, { filename: 'game.js' });

const exp = sandbox.__exports;
if (!exp) {
  console.log('FAIL: game.js did not publish __exports');
  process.exit(1);
}
const { state, TOOLS, placeTool, simulateStep } = exp;

function setTool(name) { state.tool = name; }

// --- Build a small functioning city -----------------------------------------
// Power plant at center
setTool('power'); placeTool(16, 16);
// Road line across the middle
setTool('road');
for (let x = 4; x < 28; x++) placeTool(x, 16);
// Zoning just above/below the road (within power radius 14)
setTool('residential');
for (let x = 6; x < 26; x += 2) placeTool(x, 15);
for (let x = 6; x < 26; x += 2) placeTool(x, 14);
setTool('commercial');
for (let x = 6; x < 22; x += 2) placeTool(x, 17);
setTool('industrial');
for (let x = 22; x < 28; x += 2) placeTool(x, 17);
// A few services
setTool('park'); placeTool(10, 14);
setTool('school'); placeTool(14, 14);
setTool('police'); placeTool(20, 14);

console.log('After building:');
console.log('  money=', state.money,
  'tiles r/c/i=', state.stats.r, state.stats.c, state.stats.i,
  'roads=', state.stats.roads);

// --- Run the simulation ------------------------------------------------------
const startMoney = state.money;
for (let i = 0; i < 40; i++) simulateStep();

console.log('After 40 ticks:');
console.log('  day=', state.day);
console.log('  population=', state.population);
console.log('  jobs=', state.jobs);
console.log('  happiness=', state.happiness);
console.log('  money=', state.money, '(start', startMoney + ')');
console.log('  powered zones=', state.stats.powered);

// --- Assertions --------------------------------------------------------------
let ok = true;
function assert(cond, msg) {
  if (!cond) { ok = false; console.log('FAIL:', msg); }
  else console.log('PASS:', msg);
}
assert(state.population > 0, 'population grew above zero');
assert(state.jobs > 0, 'jobs created');
assert(state.stats.powered > 0, 'some zones powered');
assert(state.history.length > 0, 'history recorded');
assert(state.happiness >= 0 && state.happiness <= 100, 'happiness within 0..100');
assert(TOOLS.power.cost === 3000, 'power plant cost wired to TOOLS');

console.log(ok ? '\nSMOKE_TEST_RESULT=PASS' : '\nSMOKE_TEST_RESULT=FAIL');
process.exit(ok ? 0 : 1);
