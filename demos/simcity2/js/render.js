// render.js — canvas drawing for the city. Pure drawing, reads City state.
import { GRID_W, GRID_H, TILE, BUILDINGS } from "./config.js";

export class Renderer {
  constructor(canvas, city) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.city = city;
    this.zoom = 1;
    this.offsetX = 0;
    this.offsetY = 0;
    this.hoverX = -1;
    this.hoverY = -1;
    this.resize();
  }

  resize() {
    const dpr = window.devicePixelRatio || 1;
    this.canvas.width = Math.floor(window.innerWidth * dpr);
    this.canvas.height = Math.floor(window.innerHeight * dpr);
    this.canvas.style.width = window.innerWidth + "px";
    this.canvas.style.height = window.innerHeight + "px";
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    this.vw = window.innerWidth;
    this.vh = window.innerHeight;
    // center the grid
    this.baseTile = TILE * this.zoom;
    this.worldW = GRID_W * this.baseTile;
    this.worldH = GRID_H * this.baseTile;
    if (this.offsetX === 0 && this.offsetY === 0) {
      this.offsetX = (this.vw - this.worldW) / 2;
      this.offsetY = (this.vh - this.worldH) / 2;
    }
  }

  screenToTile(sx, sy) {
    const x = Math.floor((sx - this.offsetX) / this.baseTile);
    const y = Math.floor((sy - this.offsetY) / this.baseTile);
    return { x, y };
  }

  draw(hoverTool) {
    const ctx = this.ctx;
    ctx.clearRect(0, 0, this.vw, this.vh);

    // background
    ctx.fillStyle = "#0b1020";
    ctx.fillRect(0, 0, this.vw, this.vh);

    const bt = this.baseTile;
    // grass + grid
    for (let y = 0; y < GRID_H; y++) {
      for (let x = 0; x < GRID_W; x++) {
        const px = this.offsetX + x * bt;
        const py = this.offsetY + y * bt;
        if (px > this.vw || py > this.vh || px + bt < 0 || py + bt < 0) continue;
        // checker grass
        ctx.fillStyle = (x + y) % 2 === 0 ? "#15341f" : "#123018";
        ctx.fillRect(px, py, bt, bt);
      }
    }

    // tiles
    for (let y = 0; y < GRID_H; y++) {
      for (let x = 0; x < GRID_W; x++) {
        const t = this.city.grid[y * GRID_W + x];
        if (!t) continue;
        const px = this.offsetX + x * bt;
        const py = this.offsetY + y * bt;
        this.drawTile(t, px, py, bt);
      }
    }

    // grid lines (subtle)
    ctx.strokeStyle = "rgba(255,255,255,0.04)";
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let x = 0; x <= GRID_W; x++) {
      const px = this.offsetX + x * bt;
      ctx.moveTo(px, this.offsetY);
      ctx.lineTo(px, this.offsetY + this.worldH);
    }
    for (let y = 0; y <= GRID_H; y++) {
      const py = this.offsetY + y * bt;
      ctx.moveTo(this.offsetX, py);
      ctx.lineTo(this.offsetX + this.worldW, py);
    }
    ctx.stroke();

    // hover highlight
    if (this.hoverX >= 0 && this.hoverY >= 0 && this.hoverX < GRID_W && this.hoverY < GRID_H) {
      const px = this.offsetX + this.hoverX * bt;
      const py = this.offsetY + this.hoverY * bt;
      const b = hoverTool ? BUILDINGS[hoverTool] : null;
      ctx.save();
      ctx.globalAlpha = 0.5;
      ctx.fillStyle = hoverTool === "bulldozer" ? "#ff6b6b" : (b ? b.color : "#ffffff");
      ctx.fillRect(px, py, bt, bt);
      ctx.restore();
      ctx.strokeStyle = hoverTool === "bulldozer" ? "#ff6b6b" : "#ffffff";
      ctx.lineWidth = 2;
      ctx.strokeRect(px + 1, py + 1, bt - 2, bt - 2);
    }

    // disaster flash
    if (this.city.disasterFlash > 0) {
      ctx.fillStyle = `rgba(255,80,40,${0.25 * (this.city.disasterFlash / 20)})`;
      ctx.fillRect(0, 0, this.vw, this.vh);
    }
  }

  drawTile(t, px, py, bt) {
    const ctx = this.ctx;
    const b = BUILDINGS[t.type];
    if (t.type === "road") {
      ctx.fillStyle = b.color;
      ctx.fillRect(px + 2, py + 2, bt - 4, bt - 4);
      ctx.strokeStyle = "rgba(255,255,255,0.25)";
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(px + bt / 2, py + 2);
      ctx.lineTo(px + bt / 2, py + bt - 2);
      ctx.moveTo(px + 2, py + bt / 2);
      ctx.lineTo(px + bt - 2, py + bt / 2);
      ctx.stroke();
      return;
    }

    // base block
    ctx.fillStyle = b.color;
    ctx.fillRect(px + 2, py + 2, bt - 4, bt - 4);
    ctx.strokeStyle = "rgba(0,0,0,0.35)";
    ctx.lineWidth = 1;
    ctx.strokeRect(px + 2, py + 2, bt - 4, bt - 4);

    // fill indicator for zones (grows with population/jobs)
    if (t.type === "residential" || t.type === "commercial" || t.type === "industrial") {
      const f = t.fill || 0;
      if (f > 0.02) {
        ctx.fillStyle = "rgba(255,255,255,0.55)";
        const h = (bt - 8) * f;
        ctx.fillRect(px + 5, py + bt - 5 - h, bt - 10, h);
      }
      // status dot
      if (!t.powered) {
        ctx.fillStyle = "#ff6b6b";
        ctx.beginPath();
        ctx.arc(px + bt - 7, py + 7, 3, 0, Math.PI * 2);
        ctx.fill();
      } else if (!t.roadOk) {
        ctx.fillStyle = "#ffb454";
        ctx.beginPath();
        ctx.arc(px + bt - 7, py + 7, 3, 0, Math.PI * 2);
        ctx.fill();
      }
    }

    // icon glyph for services
    if (["power","police","hospital","school","park"].includes(t.type)) {
      ctx.fillStyle = "rgba(255,255,255,0.9)";
      ctx.font = `${Math.floor(bt * 0.5)}px serif`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(b.icon, px + bt / 2, py + bt / 2 + 1);
    }
  }
}
