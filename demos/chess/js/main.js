/**
 * 3D Chess — main controller (input, UI, loop)
 */
import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import {
  createGame,
  legalMoves,
  makeMove,
  undoMove,
  squareName,
  pieceSymbol,
  allLegalMoves,
} from "./chess.js";
import {
  createSceneKit,
  syncPieces,
  showMoveMarkers,
  showLastMove,
  clearMarkers,
  worldToSquare,
  squareToWorld,
  findPieceMesh,
  animatePieceMove,
  updateBoardLabels,
} from "./board.js";
import { setPieceHighlight as setHighlight } from "./pieces.js";

function showBootError(err) {
  const box = document.getElementById("boot-error");
  const msg = document.getElementById("boot-error-msg");
  if (msg) msg.textContent = String(err?.message || err || "Unknown error");
  if (box) box.hidden = false;
  console.error("[3D Chess]", err);
}

const canvas = document.getElementById("chess-canvas");
if (!canvas) throw new Error("Missing #chess-canvas");

// Fail fast if WebGL is unavailable
{
  const test = document.createElement("canvas");
  const gl =
    test.getContext("webgl2", { failIfMajorPerformanceCaveat: false }) ||
    test.getContext("webgl", { failIfMajorPerformanceCaveat: false });
  if (!gl) showBootError(new Error("WebGL is not available in this browser."));
}

const el = {
  turn: document.getElementById("turn-indicator"),
  status: document.getElementById("game-status"),
  moves: document.getElementById("move-list"),
  capW: document.getElementById("captured-white"),
  capB: document.getElementById("captured-black"),
  hover: document.getElementById("hover-square"),
  announce: document.getElementById("announce"),
  overlay: document.getElementById("overlay"),
  overlayTitle: document.getElementById("overlay-title"),
  overlayMsg: document.getElementById("overlay-message"),
  promo: document.getElementById("promotion"),
  orbitToggle: document.getElementById("orbit-toggle"),
  aiToggle: document.getElementById("ai-toggle"),
};

let kit;
try {
  kit = createSceneKit(canvas);
} catch (err) {
  showBootError(err);
  throw err;
}
const { renderer, scene, camera, board, piecesRoot, markers } = kit;

// Ensure first frame paints even if rAF is throttled (headless / background)
renderer.render(scene, camera);

const controls = new OrbitControls(camera, canvas);
controls.enableDamping = true;
controls.dampingFactor = 0.08;
controls.maxPolarAngle = Math.PI * 0.48;
controls.minDistance = 6;
controls.maxDistance = 22;
controls.target.set(0, 0, 0);
controls.update();

let game = createGame();
let flipped = false;
let selected = null; // {row,col}
let legal = [];
let busy = false;
let pendingPromo = null; // {from,to}
let showHint = true;
let aiEnabled = false;
let raycaster = new THREE.Raycaster();
let pointer = new THREE.Vector2();

const PIECE_VAL = { p: 100, n: 320, b: 330, r: 500, q: 900, k: 20000 };

function scoreMove(gameState, from, to) {
  let score = Math.random() * 8;
  const target = gameState.board[to.row][to.col];
  const mover = gameState.board[from.row][from.col];
  if (target) score += (PIECE_VAL[target.type] || 0) * 10 - (PIECE_VAL[mover?.type] || 0);
  if (to.enPassant) score += 90;
  if (to.castle) score += 40;
  if (mover?.type === "p" && (to.row === 0 || to.row === 7)) score += 800;
  // center control
  const cr = Math.abs(to.row - 3.5);
  const cc = Math.abs(to.col - 3.5);
  score += (4 - cr) * 3 + (4 - cc) * 3;
  return score;
}

function pickAiMove(gameState) {
  const moves = allLegalMoves(gameState.board, gameState);
  if (!moves.length) return null;
  let best = null;
  let bestScore = -Infinity;
  for (const m of moves) {
    const s = scoreMove(gameState, m.from, m.to);
    if (s > bestScore) {
      bestScore = s;
      best = m;
    }
  }
  return best;
}

async function maybeAiMove() {
  if (!aiEnabled || busy || pendingPromo) return;
  if (game.turn !== "b") return;
  if (game.status === "checkmate" || game.status === "stalemate" || game.status === "draw") {
    return;
  }
  const choice = pickAiMove(game);
  if (!choice) return;
  await new Promise((r) => setTimeout(r, 280));
  if (!aiEnabled || game.turn !== "b" || busy) return;
  await commitMove(choice.from, choice.to, "q");
}

function announce(msg) {
  el.announce.textContent = msg;
}

function refreshUI() {
  const whiteTurn = game.turn === "w";
  el.turn.textContent = whiteTurn ? "White" : "Black";
  el.turn.classList.toggle("white-turn", whiteTurn);
  el.turn.classList.toggle("black-turn", !whiteTurn);

  el.status.classList.remove("check", "mate", "end");
  let statusText = "Playing";
  if (game.status === "check") {
    statusText = "Check!";
    el.status.classList.add("check");
  } else if (game.status === "checkmate") {
    statusText = "Checkmate";
    el.status.classList.add("mate");
  } else if (game.status === "stalemate") {
    statusText = "Stalemate";
    el.status.classList.add("end");
  } else if (game.status === "draw") {
    statusText = "Draw";
    el.status.classList.add("end");
  }
  el.status.textContent = statusText;

  // Captured: show what each side lost (opponent's captures)
  el.capW.textContent = game.captured.b.map((t) => pieceSymbol(t, "w")).join(" ") || "—";
  el.capB.textContent = game.captured.w.map((t) => pieceSymbol(t, "b")).join(" ") || "—";

  // Move list from history snapshots
  el.moves.innerHTML = "";
  const sans = game.history.map((h) => h.move?.san).filter(Boolean);
  for (let i = 0; i < sans.length; i += 2) {
    const li = document.createElement("li");
    const n = (i >> 1) + 1;
    const w = sans[i] || "";
    const b = sans[i + 1] || "";
    li.textContent = b ? `${n}. ${w}  ${b}` : `${n}. ${w}`;
    el.moves.appendChild(li);
  }
  el.moves.scrollTop = el.moves.scrollHeight;

  // End overlay
  if (game.status === "checkmate" || game.status === "stalemate" || game.status === "draw") {
    el.overlay.hidden = false;
    if (game.status === "checkmate") {
      el.overlayTitle.textContent = "Checkmate";
      el.overlayMsg.textContent = game.winner === "w" ? "White wins" : "Black wins";
    } else if (game.status === "stalemate") {
      el.overlayTitle.textContent = "Stalemate";
      el.overlayMsg.textContent = "Draw — no legal moves";
    } else {
      el.overlayTitle.textContent = "Draw";
      el.overlayMsg.textContent = "50-move rule";
    }
  } else {
    el.overlay.hidden = true;
  }
}

function paintMarkers() {
  clearMarkers(markers);
  if (showHint && game.history.length) {
    const last = game.history[game.history.length - 1].move;
    if (last) showLastMove(markers, last.from, last.to, flipped);
  }
  if (selected && legal.length) {
    showMoveMarkers(markers, legal, flipped);
  }
}

function paintCheck() {
  for (const child of piecesRoot.children) {
    setHighlight(child, null);
  }
  if (game.status === "check" || game.status === "checkmate") {
    for (let r = 0; r < 8; r++) {
      for (let c = 0; c < 8; c++) {
        const p = game.board[r][c];
        if (p && p.type === "k" && p.color === game.turn) {
          const m = findPieceMesh(piecesRoot, r, c);
          if (m) setHighlight(m, "check");
        }
      }
    }
  }
  if (selected) {
    const m = findPieceMesh(piecesRoot, selected.row, selected.col);
    if (m) setHighlight(m, "select");
  }
}

function fullSync() {
  syncPieces(piecesRoot, game.board, flipped);
  updateBoardLabels(board, flipped);
  paintMarkers();
  paintCheck();
  refreshUI();
}

function clearSelection() {
  selected = null;
  legal = [];
  paintMarkers();
  paintCheck();
}

function selectSquare(row, col) {
  const piece = game.board[row][col];
  if (piece && piece.color === game.turn) {
    selected = { row, col };
    legal = legalMoves(game.board, selected, game);
    paintMarkers();
    paintCheck();
    announce(`Selected ${pieceSymbol(piece.type, piece.color)} on ${squareName(row, col)}`);
    return;
  }
  clearSelection();
}

async function tryMove(toRow, toCol) {
  if (!selected || busy) return;
  const from = { ...selected };
  const dest = legal.find((m) => m.row === toRow && m.col === toCol);
  if (!dest) {
    // re-select if own piece
    const p = game.board[toRow][toCol];
    if (p && p.color === game.turn) {
      selectSquare(toRow, toCol);
      return;
    }
    clearSelection();
    return;
  }

  const piece = game.board[from.row][from.col];
  const isPromo =
    piece &&
    piece.type === "p" &&
    (toRow === 0 || toRow === 7);

  if (isPromo) {
    pendingPromo = { from, to: { row: toRow, col: toCol } };
    el.promo.hidden = false;
    announce("Choose promotion piece");
    return;
  }

  await commitMove(from, { row: toRow, col: toCol }, "q");
}

async function commitMove(from, to, promo) {
  busy = true;
  clearSelection();

  const mesh = findPieceMesh(piecesRoot, from.row, from.col);
  const target = squareToWorld(to.row, to.col, flipped);

  // Capture castling rook mesh before board mutates
  let rookAnim = null;
  const pieceBefore = game.board[from.row]?.[from.col];
  if (pieceBefore?.type === "k" && Math.abs(to.col - from.col) === 2) {
    const isKingSide = to.col > from.col;
    const rookFromCol = isKingSide ? 7 : 0;
    const rookToCol = isKingSide ? 5 : 3;
    const rookMesh = findPieceMesh(piecesRoot, from.row, rookFromCol);
    if (rookMesh) {
      rookAnim = {
        mesh: rookMesh,
        target: squareToWorld(from.row, rookToCol, flipped),
      };
    }
  }

  const result = makeMove(game, from, to, promo);
  if (!result.ok) {
    busy = false;
    announce(result.reason || "Illegal move");
    fullSync();
    return;
  }

  const anims = [];
  if (mesh) anims.push(animatePieceMove(mesh, target));
  if (rookAnim) anims.push(animatePieceMove(rookAnim.mesh, rookAnim.target, 0.2));
  if (anims.length) await Promise.all(anims);

  // Keep mesh coords in sync for next pick before full rebuild
  if (mesh) {
    mesh.userData.row = to.row;
    mesh.userData.col = to.col;
  }
  if (rookAnim) {
    const isKingSide = to.col > from.col;
    rookAnim.mesh.userData.col = isKingSide ? 5 : 3;
  }

  fullSync();
  busy = false;

  const mv = result.move;
  announce(`${mv.color === "w" ? "White" : "Black"} played ${mv.san}`);
  if (game.status === "check") announce("Check!");
  if (game.status === "checkmate") announce("Checkmate!");

  // Kick AI reply after human (White) move
  if (aiEnabled && game.turn === "b" && game.status !== "checkmate") {
    queueMicrotask(() => {
      maybeAiMove();
    });
  }
}

function onPromoChoice(type) {
  if (!pendingPromo) return;
  el.promo.hidden = true;
  const { from, to } = pendingPromo;
  pendingPromo = null;
  commitMove(from, to, type);
}

function pickSquare(event) {
  const rect = canvas.getBoundingClientRect();
  pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
  pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
  raycaster.setFromCamera(pointer, camera);

  const pieceHits = raycaster.intersectObjects(piecesRoot.children, true);
  if (pieceHits.length) {
    let obj = pieceHits[0].object;
    // Walk up via pieceRoot marker or kind flag
    while (obj) {
      if (obj.userData?.kind === "piece") {
        return { row: obj.userData.row, col: obj.userData.col, piece: true };
      }
      if (obj.userData?.pieceRoot?.userData?.kind === "piece") {
        const root = obj.userData.pieceRoot;
        return { row: root.userData.row, col: root.userData.col, piece: true };
      }
      obj = obj.parent;
    }
  }

  // plane y=0
  const plane = new THREE.Plane(new THREE.Vector3(0, 1, 0), 0);
  const hit = new THREE.Vector3();
  if (raycaster.ray.intersectPlane(plane, hit)) {
    const sq = worldToSquare(hit.x, hit.z, flipped);
    if (sq) return { ...sq, piece: false };
  }
  return null;
}

let dragMoved = false;
let downPos = null;

canvas.addEventListener("pointerdown", (e) => {
  dragMoved = false;
  downPos = { x: e.clientX, y: e.clientY };
});

canvas.addEventListener("pointermove", (e) => {
  if (downPos) {
    const dx = e.clientX - downPos.x;
    const dy = e.clientY - downPos.y;
    if (dx * dx + dy * dy > 25) dragMoved = true;
  }
  const sq = pickSquare(e);
  el.hover.textContent = sq ? squareName(sq.row, sq.col) : "—";
});

canvas.addEventListener("pointerup", (e) => {
  if (busy || pendingPromo) return;
  if (dragMoved) return;
  const sq = pickSquare(e);
  if (!sq) {
    clearSelection();
    return;
  }
  if (selected) {
    tryMove(sq.row, sq.col);
  } else {
    selectSquare(sq.row, sq.col);
  }
});

// UI buttons
document.getElementById("btn-new").addEventListener("click", () => newGame());
document.getElementById("btn-undo").addEventListener("click", () => doUndo());
document.getElementById("btn-flip").addEventListener("click", () => doFlip());
document.getElementById("btn-hint").addEventListener("click", () => {
  showHint = !showHint;
  paintMarkers();
  announce(showHint ? "Last-move highlight on" : "Last-move highlight off");
});
document.getElementById("overlay-new").addEventListener("click", () => newGame());
el.orbitToggle.addEventListener("change", () => {
  controls.enabled = el.orbitToggle.checked;
});

if (el.aiToggle) {
  el.aiToggle.addEventListener("change", () => {
    aiEnabled = el.aiToggle.checked;
    announce(aiEnabled ? "AI plays Black" : "AI off — hotseat");
    if (aiEnabled) maybeAiMove();
  });
}

document.getElementById("promo-choices").addEventListener("click", (e) => {
  const btn = e.target.closest("[data-piece]");
  if (!btn) return;
  onPromoChoice(btn.dataset.piece);
});

function newGame() {
  game = createGame();
  selected = null;
  legal = [];
  pendingPromo = null;
  el.promo.hidden = true;
  el.overlay.hidden = true;
  fullSync();
  announce("New game. White to move.");
}

function doUndo() {
  if (busy) return;
  if (!undoMove(game)) {
    announce("Nothing to undo");
    return;
  }
  clearSelection();
  fullSync();
  announce("Move undone");
}

function doFlip() {
  flipped = !flipped;
  clearSelection();
  fullSync();
  // swing camera
  camera.position.z *= -1;
  camera.position.x *= -1;
  controls.update();
  announce(flipped ? "Board flipped (Black view)" : "Board flipped (White view)");
}

window.addEventListener("keydown", (e) => {
  if (e.target.matches("input, textarea, select")) return;
  const k = e.key.toLowerCase();
  if (k === "n") newGame();
  if (k === "u") doUndo();
  if (k === "f") doFlip();
  if (k === "escape") {
    if (!el.promo.hidden) {
      el.promo.hidden = true;
      pendingPromo = null;
    }
    clearSelection();
  }
});

function onResize() {
  const w = window.innerWidth;
  const h = window.innerHeight;
  camera.aspect = w / h;
  camera.updateProjectionMatrix();
  renderer.setSize(w, h, false);
}
window.addEventListener("resize", onResize);

function loop() {
  requestAnimationFrame(loop);
  controls.update();
  renderer.render(scene, camera);
}

try {
  fullSync();
  onResize();
  loop();
  // Extra paints help headless / first-frame capture
  renderer.render(scene, camera);
  setTimeout(() => renderer.render(scene, camera), 50);
  setTimeout(() => renderer.render(scene, camera), 250);
  announce("3D Chess ready. White to move.");
} catch (err) {
  showBootError(err);
}
