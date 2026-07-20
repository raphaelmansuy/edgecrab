// game.js — main controller: input, UI binding, game loop, disasters, persistence.
import { City } from "./city.js";
import { Renderer } from "./render.js";
import { BUILDINGS, BANKRUPT_LIMIT } from "./config.js";

const MONTHS_PER_SEC = {
  paused: 0,
  normal: 0.6,   // ~1 month per 1.6s
  fast: 2.0,
  ultra: 6.0,
};
const SPEEDS = ["normal", "fast", "ultra", "paused"];

class Game {
  constructor() {
    this.canvas = document.getElementById("game-canvas");
    this.city = new City();
    this.renderer = new Renderer(this.canvas, this.city);
    this.tool = "residential";
    this.speedIdx = 0; // normal
    this.acc = 0;       // month accumulator
    this.lastTs = 0;
    this.dragging = false;
    this.lastPlaced = null;
    this.bindUI();
    this.bindInput();
    this.updateHUD();
    requestAnimationFrame((t) => this.loop(t));
  }

  get speed() { return SPEEDS[this.speedIdx]; }

  bindUI() {
    // toolbar
    this.toolButtons = Array.from(document.querySelectorAll(".tool-btn"));
    this.toolButtons.forEach((btn) => {
      btn.addEventListener("click", () => this.selectTool(btn.dataset.tool));
    });
    this.buildName = document.getElementById("build-name");
    this.buildCost = document.getElementById("build-cost");
    this.buildDesc = document.getElementById("build-desc");

    document.getElementById("start-btn").addEventListener("click", () => {
      document.getElementById("overlay").classList.add("hidden");
    });

    document.getElementById("speed-btn").addEventListener("click", () => this.cycleSpeed());
    document.getElementById("pause-btn").addEventListener("click", () => this.togglePause());
    document.getElementById("disaster-btn").addEventListener("click", () => this.triggerDisaster());
    document.getElementById("save-btn").addEventListener("click", () => this.save());
    document.getElementById("load-btn").addEventListener("click", () => this.load());
    document.getElementById("reset-btn").addEventListener("click", () => this.reset());

    window.addEventListener("resize", () => this.renderer.resize());
  }

  bindInput() {
    const c = this.canvas;
    const tileFromEvent = (e) => {
      const r = c.getBoundingClientRect();
      const sx = (e.touches ? e.touches[0].clientX : e.clientX) - r.left;
      const sy = (e.touches ? e.touches[0].clientY : e.clientY) - r.top;
      return this.renderer.screenToTile(sx, sy);
    };

    c.addEventListener("mousemove", (e) => {
      const { x, y } = tileFromEvent(e);
      this.renderer.hoverX = x;
      this.renderer.hoverY = y;
      if (this.dragging && (this.tool === "road" || this.tool === "bulldozer")) {
        this.tryPlace(x, y);
      }
    });
    c.addEventListener("mousedown", (e) => {
      const { x, y } = tileFromEvent(e);
      this.dragging = true;
      this.tryPlace(x, y);
    });
    window.addEventListener("mouseup", () => { this.dragging = false; this.lastPlaced = null; });
    c.addEventListener("mouseleave", () => { this.renderer.hoverX = -1; this.renderer.hoverY = -1; });

    // touch
    c.addEventListener("touchstart", (e) => {
      e.preventDefault();
      const { x, y } = tileFromEvent(e);
      this.dragging = true;
      this.tryPlace(x, y);
    }, { passive: false });
    c.addEventListener("touchmove", (e) => {
      e.preventDefault();
      const { x, y } = tileFromEvent(e);
      this.renderer.hoverX = x; this.renderer.hoverY = y;
      if (this.dragging) this.tryPlace(x, y);
    }, { passive: false });
    c.addEventListener("touchend", () => { this.dragging = false; this.lastPlaced = null; });

    // keyboard
    window.addEventListener("keydown", (e) => {
      const map = {
        "1": "residential", "2": "commercial", "3": "industrial", "4": "road",
        "5": "park", "6": "power", "7": "police", "8": "hospital", "9": "school",
        "0": "bulldozer", "b": "bulldozer", "B": "bulldozer",
      };
      if (map[e.key]) { this.selectTool(map[e.key]); return; }
      if (e.key === " ") { e.preventDefault(); this.cycleSpeed(); }
      if (e.key === "p" || e.key === "P") this.togglePause();
    });
  }

  selectTool(tool) {
    this.tool = tool;
    this.toolButtons.forEach((b) => b.classList.toggle("active", b.dataset.tool === tool));
    const b = BUILDINGS[tool];
    this.buildName.textContent = b.name;
    this.buildCost.textContent = tool === "bulldozer" ? "free" : `$${b.cost}`;
    this.buildDesc.textContent = b.desc;
  }

  tryPlace(x, y) {
    // avoid re-placing same tile during a drag
    const key = `${x},${y}`;
    if (this.lastPlaced === key) return;
    this.lastPlaced = key;
    const res = this.city.place(this.tool, x, y);
    if (!res.ok) {
      if (res.reason === "Not enough funds" || res.reason === "Tile occupied" || res.reason === "Out of bounds") {
        // silently ignore during drag; only toast on click for funds
      }
      return;
    }
    if (this.tool === "bulldozer" && res.refund) {
      // small feedback
    }
    this.updateHUD();
  }

  cycleSpeed() {
    this.speedIdx = (this.speedIdx + 1) % SPEEDS.length;
    this.updateSpeedLabel();
  }

  togglePause() {
    // toggle between paused and last non-paused
    if (this.speed === "paused") {
      this.speedIdx = this._prevSpeedIdx ?? 0;
    } else {
      this._prevSpeedIdx = this.speedIdx;
      this.speedIdx = SPEEDS.indexOf("paused");
    }
    this.updateSpeedLabel();
  }

  updateSpeedLabel() {
    const labels = { normal: "▶ Normal", fast: "⏩ Fast", ultra: "⏭ Ultra", paused: "⏸ Paused" };
    document.getElementById("speed-value").textContent = labels[this.speed];
    document.getElementById("pause-btn").textContent = this.speed === "paused" ? "▶ Resume" : "⏸ Pause";
  }

  triggerDisaster() {
    const occupied = [];
    for (let i = 0; i < this.city.grid.length; i++) {
      if (this.city.grid[i]) occupied.push(i);
    }
    if (occupied.length === 0) { this.toast("Nothing to destroy yet!", "warn"); return; }
    // meteor: destroy a random cluster
    const center = occupied[Math.floor(Math.random() * occupied.length)];
    const cx = center % 40, cy = Math.floor(center / 40);
    let destroyed = 0;
    for (let dy = -1; dy <= 1; dy++) {
      for (let dx = -1; dx <= 1; dx++) {
        const nx = cx + dx, ny = cy + dy;
        if (nx < 0 || ny < 0 || nx >= 40 || ny >= 40) continue;
        const i = ny * 40 + nx;
        if (this.city.grid[i]) { this.city.grid[i] = null; destroyed++; }
      }
    }
    this.city.disasterFlash = 20;
    this.city.happiness = Math.max(0, this.city.happiness - 15);
    this.toast(`🌋 Meteor strike! ${destroyed} tiles lost.`, "bad");
    this.updateHUD();
  }

  save() {
    try {
      localStorage.setItem("simcity2_save", this.city.serialize());
      this.toast("💾 City saved.", "good");
    } catch (e) {
      this.toast("Save failed: " + e.message, "bad");
    }
  }

  load() {
    const data = localStorage.getItem("simcity2_save");
    if (!data) { this.toast("No saved city found.", "warn"); return; }
    if (this.city.load(data)) {
      this.toast("📂 City loaded.", "good");
      this.updateHUD();
    } else {
      this.toast("Load failed.", "bad");
    }
  }

  reset() {
    this.city.reset();
    this.toast("🔄 New city started.", "good");
    this.updateHUD();
  }

  toast(msg, kind = "") {
    const el = document.getElementById("toast");
    el.textContent = msg;
    el.className = "toast " + kind;
    el.hidden = false;
    clearTimeout(this._toastT);
    this._toastT = setTimeout(() => { el.hidden = true; }, 2600);
  }

  updateHUD() {
    const c = this.city;
    document.getElementById("money-value").textContent = "$" + c.money.toLocaleString();
    document.getElementById("pop-value").textContent = c.population.toLocaleString();
    document.getElementById("jobs-value").textContent = c.jobs.toLocaleString() + " jobs";
    document.getElementById("power-value").textContent = `${c.powerSupply} / ${c.powerDemand}`;
    const ps = document.getElementById("power-state");
    if (c.powerDemand === 0) { ps.textContent = "No demand"; ps.className = "sub"; }
    else if (c.powerSupply >= c.powerDemand) { ps.textContent = "Online"; ps.className = "sub ok"; }
    else { ps.textContent = "Shortfall!"; ps.className = "sub bad"; }
    const hv = document.getElementById("happy-value");
    hv.textContent = c.happiness + "%";
    hv.style.color = c.happiness >= 60 ? "var(--good)" : c.happiness >= 35 ? "var(--warn)" : "var(--bad)";
    const bal = document.getElementById("balance-value");
    bal.textContent = (c.monthlyBalance >= 0 ? "+" : "") + "$" + c.monthlyBalance.toLocaleString() + "/mo";
    bal.className = "sub " + (c.monthlyBalance >= 0 ? "pos" : "neg");
    const r = c.grid.filter((t) => t && t.type === "residential").length;
    const cm = c.grid.filter((t) => t && t.type === "commercial").length;
    const ind = c.grid.filter((t) => t && t.type === "industrial").length;
    document.getElementById("rci-value").textContent = `R${r} C${cm} I${ind}`;
    document.getElementById("date-value").textContent = c.dateLabel;
    this.updateSpeedLabel();

    if (c.gameOver) {
      this.toast("💀 Bankrupt! The council has removed you. Press New.", "bad");
    }
  }

  loop(ts) {
    if (!this.lastTs) this.lastTs = ts;
    const dt = Math.min(0.1, (ts - this.lastTs) / 1000);
    this.lastTs = ts;

    const rate = MONTHS_PER_SEC[this.speed];
    if (rate > 0 && !this.city.gameOver) {
      this.acc += rate * dt;
      while (this.acc >= 1) {
        this.acc -= 1;
        const r = this.city.tick();
        this.updateHUD();
      }
    }

    this.renderer.draw(this.tool);
    requestAnimationFrame((t) => this.loop(t));
  }
}

window.addEventListener("DOMContentLoaded", () => {
  window.__game = new Game();
  // expose for smoketest hooks
  window.__simcity2 = { Game, City };
});
