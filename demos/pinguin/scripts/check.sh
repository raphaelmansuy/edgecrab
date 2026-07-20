#!/usr/bin/env bash
set -euo pipefail
BASEDIR="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${PORT:-8080}"

echo "🔍 Checking Pinguin Slide Party scaffold..."

for f in index.html styles.css js/game.js js/ui.js; do
  if [[ ! -f "$BASEDIR/$f" ]]; then
    echo "❌ Missing file: $f"
    exit 1
  fi
  echo "✅ $f present"
done

if grep -q 'importmap' "$BASEDIR/index.html"; then
  echo "✅ Import map found"
else
  echo "❌ Missing import map"
  exit 1
fi

if grep -q 'js/game.js' "$BASEDIR/index.html"; then
  echo "✅ Game module linked"
else
  echo "❌ Game module link missing"
  exit 1
fi

# Core fun systems present
for needle in "COMBO_WINDOW" "createPenguin" "activatePower" "updateCompass" "belly-slide\|Belly-slide\|slideBoost"; do
  if grep -qE "$needle" "$BASEDIR/js/game.js"; then
    echo "✅ Game feature: $needle"
  else
    echo "❌ Missing game feature: $needle"
    exit 1
  fi
done

if grep -q 'AudioEngine' "$BASEDIR/js/ui.js" && grep -q 'collect(' "$BASEDIR/js/ui.js"; then
  echo "✅ Audio SFX helpers present"
else
  echo "❌ Audio helpers incomplete"
  exit 1
fi

python3 -m http.server "$PORT" --directory "$BASEDIR" &
SERVER_PID=$!
trap 'kill "$SERVER_PID" >/dev/null 2>&1 || true' EXIT

echo "🌐 Server PID $SERVER_PID on port $PORT"
sleep 1

HTTP_STATUS=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$PORT/")
if [[ "$HTTP_STATUS" -ne 200 ]]; then
  echo "❌ Server returned HTTP $HTTP_STATUS"
  exit 1
fi
echo "✅ Server responds HTTP 200"

for needle in "Pinguin Slide Party" "Start Sliding" "Fish" "game-canvas" "combo-panel"; do
  if curl -s "http://localhost:$PORT/" | grep -q "$needle"; then
    echo "✅ Found in HTML: $needle"
  else
    echo "❌ Missing in HTML: $needle"
    exit 1
  fi
done

JS_STATUS=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$PORT/js/game.js")
if [[ "$JS_STATUS" -ne 200 ]]; then
  echo "❌ game.js HTTP $JS_STATUS"
  exit 1
fi
echo "✅ game.js serves HTTP 200"

echo ""
echo "🎮 Open http://localhost:$PORT to play"
