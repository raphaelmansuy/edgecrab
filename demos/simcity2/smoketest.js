// smoketest.js — headless verification of the SimCity 2 simulation core.
// Runs the pure logic modules (no DOM) to prove the city actually simulates.
import { City } from "./js/city.js";
import { BUILDINGS, GRID_W } from "./js/config.js";

let failures = 0;
function assert(cond, msg) {
  if (cond) {
    console.log("  ✅ " + msg);
  } else {
    console.log("  ❌ " + msg);
    failures++;
  }
}

console.log("=== SimCity 2 smoke test ===");

const city = new City();
assert(city.money === 50000, "starts with $50,000 treasury");
assert(city.population === 0, "starts with zero population");

// Build a small functioning district:
// power plant + roads + a residential + commercial + industrial + services
const place = (type, x, y) => {
  const r = city.place(type, x, y);
  if (!r.ok) throw new Error(`place ${type}@${x},${y} failed: ${r.reason}`);
  return r;
};

place("power", 5, 5);
place("road", 6, 6);
place("road", 7, 6);
place("road", 8, 6);
place("residential", 6, 7);
place("residential", 7, 7);
place("commercial", 8, 7);
place("industrial", 9, 7);
place("park", 6, 8);
place("police", 10, 8);
place("hospital", 4, 8);
place("school", 11, 8);

assert(city.money < 50000, "building deducted treasury");
assert(city.powerSupply > 0, "power plant supplies power");

// Run 24 months of simulation.
let ticks = 0;
for (let i = 0; i < 24; i++) {
  const r = city.tick();
  assert(typeof r.balance === "number", `month ${i + 1} tick returned balance`);
  ticks++;
}
assert(ticks === 24, "ran 24 monthly ticks");

console.log(`  ℹ️  after 24mo: pop=${city.population} jobs=${city.jobs} happy=${city.happiness} money=${city.money}`);
assert(city.population > 0, "population grew above zero");
assert(city.jobs > 0, "jobs were created");
assert(city.happiness > 0 && city.happiness <= 100, "happiness within 0..100");
assert(city.month === 24, "date advanced 24 months");

// Power deficit behaviour
const city2 = new City();
city2.place("residential", 2, 2); // no power plant
for (let i = 0; i < 6; i++) city2.tick();
assert(city2.population === 0, "residential with no power produces no population");

// Bulldozer refund
const city3 = new City();
const before = city3.money;
city3.place("park", 3, 3);
const afterBuild = city3.money;
city3.place("bulldozer", 3, 3);
assert(afterBuild < before, "park deducted funds");
assert(city3.money > afterBuild, "bulldozer refunded funds");
assert(city3.tileAt(3, 3) === null, "bulldozer cleared the tile");

// Save/load round trip
const snap = city.serialize();
const city4 = new City();
const ok = city4.load(snap);
assert(ok, "load() accepted serialized city");
assert(city4.population === city.population, "loaded population matches snapshot");

// Bankruptcy game over
const city5 = new City();
city5.money = -25000;
for (let i = 0; i < 4; i++) city5.tick();
assert(city5.gameOver === true, "sustained bankruptcy triggers game over");

console.log("");
if (failures === 0) {
  console.log("✅ ALL SMOKE TESTS PASSED");
  process.exit(0);
} else {
  console.log(`❌ ${failures} ASSERTION(S) FAILED`);
  process.exit(1);
}
