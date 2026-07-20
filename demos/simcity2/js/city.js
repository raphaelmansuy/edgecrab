// city.js — the simulation core. Pure state + logic, no rendering.
import {
  GRID_W, GRID_H, BUILDINGS, START_MONEY, DEFAULT_TAX, MONTHS,
  HAPPY, BANKRUPT_LIMIT,
} from "./config.js";

const idx = (x, y) => y * GRID_W + x;
const inBounds = (x, y) => x >= 0 && y >= 0 && x < GRID_W && y < GRID_H;

export class City {
  constructor() {
    this.reset();
  }

  reset() {
    // grid: array of tile objects or null (empty grass).
    this.grid = new Array(GRID_W * GRID_H).fill(null);
    this.money = START_MONEY;
    this.taxRate = DEFAULT_TAX;
    this.month = 0; // total months elapsed
    this.year = 2000;
    this.happiness = 100;
    this.population = 0;
    this.jobs = 0;
    this.powerSupply = 0;
    this.powerDemand = 0;
    this.monthlyBalance = 0;
    this.bankruptStreak = 0;
    this.gameOver = false;
    this.disasterFlash = 0;
  }

  get dateLabel() {
    const m = MONTHS[this.month % 12];
    return `${m} ${this.year}`;
  }

  tileAt(x, y) {
    if (!inBounds(x, y)) return null;
    return this.grid[idx(x, y)];
  }

  // Manhattan-ish adjacency: is (x,y) within radius of any tile of given type?
  hasNearby(x, y, type, radius) {
    for (let dy = -radius; dy <= radius; dy++) {
      for (let dx = -radius; dx <= radius; dx++) {
        const nx = x + dx, ny = y + dy;
        if (!inBounds(nx, ny)) continue;
        const t = this.grid[idx(nx, ny)];
        if (t && t.type === type) return true;
      }
    }
    return false;
  }

  countNearby(x, y, type, radius) {
    let c = 0;
    for (let dy = -radius; dy <= radius; dy++) {
      for (let dx = -radius; dx <= radius; dx++) {
        const nx = x + dx, ny = y + dy;
        if (!inBounds(nx, ny)) continue;
        const t = this.grid[idx(nx, ny)];
        if (t && t.type === type) c++;
      }
    }
    return c;
  }

  isRoadConnected(x, y) {
    // BFS from tile over road tiles; connected if any road within small neighborhood
    // already links to the network. Cheap heuristic: adjacent road OR reachable.
    return this.hasNearby(x, y, "road", 1);
  }

  canPlace(type, x, y) {
    if (!inBounds(x, y)) return { ok: false, reason: "Out of bounds" };
    const t = this.grid[idx(x, y)];
    if (type === "bulldozer") {
      if (!t) return { ok: false, reason: "Nothing to raze" };
      return { ok: true };
    }
    if (t) return { ok: false, reason: "Tile occupied" };
    const b = BUILDINGS[type];
    if (this.money < b.cost) return { ok: false, reason: "Not enough funds" };
    return { ok: true };
  }

  place(type, x, y) {
    const check = this.canPlace(type, x, y);
    if (!check.ok) return check;
    if (type === "bulldozer") {
      const t = this.grid[idx(x, y)];
      const refund = Math.round((BUILDINGS[t.type]?.cost || 0) * 0.25);
      this.money += refund;
      this.grid[idx(x, y)] = null;
      return { ok: true, refund };
    }
    const b = BUILDINGS[type];
    this.money -= b.cost;
    this.grid[idx(x, y)] = {
      type,
      // dynamic fill levels (0..1) for zones
      fill: 0,
      powered: false,
      roadOk: false,
    };
    return { ok: true };
  }

  // ---- Monthly simulation tick ----
  tick() {
    if (this.gameOver) return;

    let supply = 0, demand = 0;
    let popCap = 0, jobCap = 0;

    // First pass: power, road, and capacities.
    for (let y = 0; y < GRID_H; y++) {
      for (let x = 0; x < GRID_W; x++) {
        const t = this.grid[idx(x, y)];
        if (!t) continue;
        const b = BUILDINGS[t.type];
        if (b.power > 0) supply += b.power;
        else if (b.power < 0) demand += -b.power;

        if (t.type === "residential") {
          t.roadOk = !b.needsRoad || this.isRoadConnected(x, y);
          t.powered = !b.needsPower || supply > 0; // provisional; refine below
          if (t.roadOk) popCap += b.popCap;
        } else if (t.type === "commercial" || t.type === "industrial") {
          t.roadOk = !b.needsRoad || this.isRoadConnected(x, y);
          if (t.roadOk) jobCap += b.jobCap;
        }
      }
    }

    // Refine: a tile is only powered if total supply >= demand across the city.
    const powerDeficit = demand > supply;
    this.powerSupply = supply;
    this.powerDemand = demand;

    // Population & jobs grow toward capacity when powered+road.
    let pop = 0, jobs = 0;
    for (let y = 0; y < GRID_H; y++) {
      for (let x = 0; x < GRID_W; x++) {
        const t = this.grid[idx(x, y)];
        if (!t) continue;
        const b = BUILDINGS[t.type];
        const serviced = t.roadOk && !powerDeficit;
        if (t.type === "residential") {
          // grow toward popCap if serviced, shrink otherwise
          const target = serviced ? b.popCap : 0;
          t.fill += (target - t.fill * b.popCap) * 0.15 / b.popCap;
          t.fill = Math.max(0, Math.min(1, t.fill));
          t.powered = !powerDeficit && t.roadOk;
          pop += Math.round(t.fill * b.popCap);
        } else if (t.type === "commercial" || t.type === "industrial") {
          const target = serviced ? b.jobCap : 0;
          t.fill += (target - t.fill * b.jobCap) * 0.15 / b.jobCap;
          t.fill = Math.max(0, Math.min(1, t.fill));
          t.powered = !powerDeficit && t.roadOk;
          jobs += Math.round(t.fill * b.jobCap);
        }
      }
    }
    this.population = pop;
    this.jobs = jobs;

    // ---- Happiness ----
    let happy = HAPPY.base;
    let parks = 0, police = 0, hospitals = 0, schools = 0;
    let unpoweredZones = 0, noRoadZones = 0, industrialNear = 0;
    let crimeHot = 0;
    for (let y = 0; y < GRID_H; y++) {
      for (let x = 0; x < GRID_W; x++) {
        const t = this.grid[idx(x, y)];
        if (!t) continue;
        if (t.type === "park") parks++;
        if (t.type === "police") police++;
        if (t.type === "hospital") hospitals++;
        if (t.type === "school") schools++;
        if ((t.type === "residential" || t.type === "commercial" || t.type === "industrial")) {
          if (powerDeficit && t.roadOk) unpoweredZones++;
          if (!t.roadOk) noRoadZones++;
          if (this.countNearby(x, y, "industrial", 2) > 0) industrialNear++;
        }
        if (t.type === "residential") {
          // crime rises without police coverage
          if (!this.hasNearby(x, y, "police", BUILDINGS.police.coverage)) crimeHot++;
        }
      }
    }
    happy += parks * HAPPY.perPark;
    happy += unpoweredZones * HAPPY.perUnpowered;
    happy += noRoadZones * HAPPY.perNoRoad;
    happy += industrialNear * HAPPY.perIndustrialNear;
    happy += Math.min(police, 6) * HAPPY.perPolice;
    happy += Math.min(hospitals, 5) * HAPPY.perHospital;
    happy += Math.min(schools, 5) * HAPPY.perSchool;
    if (crimeHot > 0) happy += HAPPY.crimePenalty;
    const unemployed = Math.max(0, pop - jobs);
    const unempPct = pop > 0 ? (unemployed / pop) * 100 : 0;
    happy += unempPct * HAPPY.unemploymentPenalty;
    this.happiness = Math.max(0, Math.min(100, Math.round(happy)));

    // ---- Budget ----
    let upkeep = 0;
    for (const t of this.grid) {
      if (t) upkeep += BUILDINGS[t.type].upkeep || 0;
    }
    // Tax income: residents & businesses pay based on population & jobs.
    const taxIncome = Math.round((pop * 1.2 + jobs * 1.6) * (this.taxRate / 7));
    const balance = taxIncome - upkeep;
    this.money += balance;
    this.monthlyBalance = balance;

    // advance date
    this.month++;
    if (this.month % 12 === 0) this.year++;

    // ---- Lose / win checks ----
    if (this.money < BANKRUPT_LIMIT) {
      this.bankruptStreak++;
      if (this.bankruptStreak >= 3) this.gameOver = true;
    } else {
      this.bankruptStreak = 0;
    }
    if (this.disasterFlash > 0) this.disasterFlash--;

    return { balance, pop, jobs, happiness: this.happiness, powerDeficit };
  }

  serialize() {
    return JSON.stringify({
      grid: this.grid,
      money: this.money,
      taxRate: this.taxRate,
      month: this.month,
      year: this.year,
      happiness: this.happiness,
      population: this.population,
      jobs: this.jobs,
      powerSupply: this.powerSupply,
      powerDemand: this.powerDemand,
    });
  }

  load(json) {
    try {
      const d = JSON.parse(json);
      this.grid = d.grid;
      this.money = d.money;
      this.taxRate = d.taxRate;
      this.month = d.month;
      this.year = d.year;
      this.happiness = d.happiness;
      this.population = d.population;
      this.jobs = d.jobs;
      this.powerSupply = d.powerSupply;
      this.powerDemand = d.powerDemand;
      this.gameOver = false;
      this.bankruptStreak = 0;
      return true;
    } catch (e) {
      return false;
    }
  }
}
