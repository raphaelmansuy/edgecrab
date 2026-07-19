# 🐧 Pinguin Adventure 3D

A tiny browser-based 3D penguin collectathon built with **Three.js**.

## How to play

1. Collect **20 golden fish** scattered across the Antarctic ice.
2. Reach the **glowing ice arch** to complete the level.
3. Beat the timer: you have **3 minutes**.

### Controls

- **↑ / W** — walk forward
- **↓ / S** — walk backward  
- **← / A** — turn left
- **→ / D** — turn right
- **Space** — slide (reduces turning but gives a boost)
- **P / Esc** — pause / resume

On touch devices, the on-screen D-pad and slide button appear automatically.

## Run locally

```bash
cd demos/pinguin
python3 -m http.server 8080
```

Then open [http://localhost:8080](http://localhost:8080).

## Verify

A smoke script checks that the scaffold files exist and the local server works:

```bash
bash scripts/check.sh
```

## Files

```
demos/pinguin/
├── index.html      # shell + UI panels
├── styles.css      # glass-panel HUD + touch controls
├── js/
│   ├── game.js     # Three.js scene, penguin physics, collectibles
│   └── ui.js       # HUD, audio, pause, settings persistence
└── scripts/
    └── check.sh    # local smoke test
```

## Developer notes

- Uses Three.js `0.160.0` via ES module import map from unpkg.
- Quality selector adjusts DPR and shadows; saved to `localStorage`.
- Sound toggle uses a soft oscillator for engine-like ambience.
- Pause compensates the timer so elapsed time ignores pauses.
