#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail=0
need() {
  if [[ ! -f "$1" ]]; then
    echo "MISSING: $1"
    fail=1
  else
    echo "OK file: $1 ($(wc -c <"$1" | tr -d ' ') bytes)"
  fi
}

need index.html
need styles.css
need js/main.js
need js/chess.js
need js/board.js
need js/pieces.js
need README.md

for pat in "chess-canvas" "three" "importmap" "btn-new" "promo-choices"; do
  if grep -q "$pat" index.html; then
    echo "OK html: $pat"
  else
    echo "MISSING html marker: $pat"
    fail=1
  fi
done

for pat in "createGame" "legalMoves" "makeMove" "castling" "enPassant"; do
  if grep -q "$pat" js/chess.js; then
    echo "OK engine: $pat"
  else
    echo "MISSING engine: $pat"
    fail=1
  fi
done

for pat in "createSceneKit" "OrbitControls" "syncPieces" "createPieceMesh"; do
  if grep -rq "$pat" js; then
    echo "OK js: $pat"
  else
    echo "MISSING js: $pat"
    fail=1
  fi
done

if [[ "$fail" -ne 0 ]]; then
  echo "SMOKE FAILED"
  exit 1
fi

if command -v node >/dev/null 2>&1 && [[ -f scripts/test-engine.mjs ]]; then
  echo "Running engine unit tests..."
  if node scripts/test-engine.mjs; then
    echo "OK engine tests"
  else
    echo "ENGINE TESTS FAILED"
    exit 1
  fi
fi

echo "SMOKE PASSED"
