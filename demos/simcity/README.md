# 🏙️ SimCity Builder (Canvas 2D)

A browser city-building simulation: zone residential, commercial, and industrial
districts, lay roads, place services and power, then watch your population, jobs,
budget, and happiness evolve in real time.

Pure HTML5 Canvas 2D — **no external dependencies**, no build step, no network
fetches. Works offline from a static server.

## Features

- **Zoning**: residential 🏠, commercial 🏢, industrial 🏭 with 4 development levels
- **Infrastructure**: roads 🛣️ with flood-fill network connectivity
- **Services**: park 🌳, police 🚓, hospital 🏥, school 🏫 (radius-based coverage)
- **Power**: power plant ⚡ with coverage radius; unpowered zones decline
- **Economy**: tax income vs. maintenance upkeep, dynamic budget
- **Happiness model**: driven by services, jobs/population balance, blackouts, and tax burden
- **Bulldozer**: demolish with partial refund
- **Simulation loop**: real-time ticks, 1x / 2x / 4x speed, pause
- **HUD**: funds, population, happiness, tax rate, build palette, toasts

## Run locally

```bash
cd demos/simcity
make serve        # starts http://127.0.0.1:8000/ (python3 http.server)
# or directly:
python3 -m http.server 8000
# open http://127.0.0.1:8000/
```

A static server is required (not `file://`) because the game loads as an ES module.

## Controls

| Input | Action |
|--------|--------|
| Click / drag on grid | Build with selected tool |
| `1`–`9` | Select tool (residential → power) |
| `0` | Bulldozer |
| `Space` | Pause / resume |
| Double-click Funds panel | Cycle speed 1x → 2x → 4x |
| Click Tax Rate panel | Cycle tax 0% / 5% / 10% / 15% / 20% |
| Start Building! button | Dismiss intro overlay and begin |

## How a city grows

A zoned tile only develops when it is **both**:

1. Connected to the road network (adjacent to a road-connected tile), and
2. Powered (within a power plant's radius).

Services (park / police / hospital / school) raise land value and happiness.
Balance residential population against commercial + industrial jobs, keep taxes
moderate, and avoid blackouts to keep citizens happy.

## Files

```
demos/simcity/
├── index.html      # Shell + UI (HUD, toolbar, intro overlay)
├── styles.css      # Neon HUD theme
├── Makefile        # serve / open / stop / smoke / clean
├── js/
│   └── game.js     # Full engine: grid, rendering, build, simulation loop
├── smoketest.js    # Headless Node smoke test (DOM/Canvas stubs + assertions)
└── README.md
```

## Smoke test

```bash
make smoke
# or:
node smoketest.js
```

Builds a small functioning city, runs 40 simulation ticks, and asserts that
population/jobs grow, zones are powered, history records, and happiness stays in
`0..100`. Expects `SMOKE_TEST_RESULT=PASS`.

## Notes

- Grid is 32×32 tiles; starting funds are $50,000.
- Tool costs: residential/commercial/industrial $500, road $100, park $200,
  police $1,500, hospital $2,500, school $2,000, power $3,000, bulldozer $10/tile.
- Services and power charge per-tick maintenance; taxes fund the gap.
- Game logic is deterministic given inputs — the headless smoke test reproduces
  a known-good city without a browser.
