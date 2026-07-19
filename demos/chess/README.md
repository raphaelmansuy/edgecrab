# ♔ 3D Chess (Three.js + HTML5)

A full browser chess game with a procedural 3D board and pieces, legal-move generation, and polished UI.

## Features

- **Full rules**: sliding pieces, knights, pawns, castling, en passant, promotion
- **Check / checkmate / stalemate** detection (incl. 50-move draw)
- **3D scene**: Three.js board, soft shadows, orbit camera, move animations (incl. castling rook)
- **UX**: click-to-move, legal-move markers, last-move highlight, undo, flip board
- **Optional AI**: lightweight material/center heuristic plays Black
- **HUD**: turn, status, captured pieces, SAN move list, promotion picker

## Run locally

```bash
cd demos/chess
python3 -m http.server 8000
# open http://127.0.0.1:8000/
```

ES modules + import map load Three.js from unpkg — a static server is required (not `file://`).

## Controls

| Input | Action |
|--------|--------|
| Click piece → square | Move |
| Drag / scroll | Orbit / zoom camera |
| `N` | New game |
| `U` | Undo |
| `F` | Flip board |
| `Esc` | Clear selection / cancel promo |
| Hint button | Toggle last-move highlight |
| AI Black | Computer plays Black |
| Orbit checkbox | Enable/disable camera drag |

## Files

```
demos/chess/
├── index.html          # Shell + UI
├── styles.css          # Glass HUD theme
├── package.json        # "type": "module" for Node tests
├── js/
│   ├── main.js         # Input, UI, AI, render loop
│   ├── chess.js        # Rules engine
│   ├── board.js        # Scene, board, markers
│   └── pieces.js       # Procedural piece meshes
├── scripts/
│   ├── check.sh        # Smoke test
│   └── test-engine.mjs # Rules unit tests
└── README.md
```

## Smoke test

```bash
./scripts/check.sh
# or engine only:
node scripts/test-engine.mjs
```

## Notes

- White moves first; board starts from White’s view.
- Promotion opens a modal (queen / rook / bishop / knight).
- Pieces are built from primitives (no external models).
- AI is intentionally simple (captures + center + promotion bias) — fun for casual play, not a strong engine.
