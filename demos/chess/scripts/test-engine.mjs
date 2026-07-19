/**
 * Node smoke tests for the chess rules engine.
 */
import {
  createGame,
  legalMoves,
  makeMove,
  undoMove,
  allLegalMoves,
  parseSquare,
  squareName,
  isInCheck,
} from "../js/chess.js";

let pass = 0;
let fail = 0;

function assert(cond, msg) {
  if (cond) {
    pass++;
    console.log("✓", msg);
  } else {
    fail++;
    console.error("✗", msg);
  }
}

// 1. Starting position
const g = createGame();
assert(g.turn === "w", "White to move");
assert(g.board[7][4].type === "k" && g.board[7][4].color === "w", "White king e1");
assert(g.board[0][4].type === "k" && g.board[0][4].color === "b", "Black king e8");
const all0 = allLegalMoves(g.board, g);
assert(all0.length === 20, `Opening moves = 20 (got ${all0.length})`);

// 2. e4
const e2 = parseSquare("e2");
const e4 = parseSquare("e4");
let r = makeMove(g, e2, e4);
assert(r.ok && r.move.san === "e4", `e4 SAN=${r.move?.san}`);
assert(g.turn === "b", "Black after e4");
assert(
  g.ep && squareName(g.ep.row, g.ep.col) === "e3",
  `EP square e3 got ${g.ep && squareName(g.ep.row, g.ep.col)}`,
);

// 3. e5
r = makeMove(g, parseSquare("e7"), parseSquare("e5"));
assert(r.ok && r.move.san === "e5", `e5 SAN=${r.move?.san}`);

// 4. Nf3
r = makeMove(g, parseSquare("g1"), parseSquare("f3"));
assert(r.ok && r.move.san === "Nf3", `Nf3 SAN=${r.move?.san}`);

// 5. Undo thrice back to start
assert(undoMove(g), "undo1");
assert(undoMove(g), "undo2");
assert(undoMove(g), "undo3");
assert(g.turn === "w" && g.history.length === 0, "back to start");
assert(allLegalMoves(g.board, g).length === 20, "20 moves after undo");

// 6. Scholar's mate
const g2 = createGame();
const seq = [
  ["e2", "e4"],
  ["e7", "e5"],
  ["f1", "c4"],
  ["b8", "c6"],
  ["d1", "h5"],
  ["g8", "f6"],
  ["h5", "f7"],
];
for (const [a, b] of seq) {
  const res = makeMove(g2, parseSquare(a), parseSquare(b));
  assert(res.ok, `move ${a}${b} ok san=${res.move?.san}`);
}
assert(g2.status === "checkmate", `scholars mate status=${g2.status}`);
assert(g2.winner === "w", "White wins scholars");
assert(
  g2.history.at(-1).move.san.includes("#"),
  `mate SAN has # : ${g2.history.at(-1).move.san}`,
);

// 7. Castling kingside white
const g3 = createGame();
makeMove(g3, parseSquare("g1"), parseSquare("f3"));
makeMove(g3, parseSquare("b8"), parseSquare("c6"));
makeMove(g3, parseSquare("e2"), parseSquare("e3"));
makeMove(g3, parseSquare("a7"), parseSquare("a6"));
makeMove(g3, parseSquare("f1"), parseSquare("e2"));
makeMove(g3, parseSquare("a6"), parseSquare("a5"));
const castleMoves = legalMoves(g3.board, parseSquare("e1"), g3);
const canCastle = castleMoves.some((m) => m.castle === "k");
assert(canCastle, "White can castle kingside");
r = makeMove(g3, parseSquare("e1"), parseSquare("g1"));
assert(r.ok && r.move.san === "O-O", `O-O SAN=${r.move?.san}`);
assert(g3.board[7][6]?.type === "k", "King on g1");
assert(g3.board[7][5]?.type === "r", "Rook on f1");

// 8. En passant
const g4 = createGame();
makeMove(g4, parseSquare("e2"), parseSquare("e4"));
makeMove(g4, parseSquare("a7"), parseSquare("a6"));
makeMove(g4, parseSquare("e4"), parseSquare("e5"));
makeMove(g4, parseSquare("d7"), parseSquare("d5"));
assert(
  g4.ep && squareName(g4.ep.row, g4.ep.col) === "d6",
  `EP d6 got ${g4.ep && squareName(g4.ep.row, g4.ep.col)}`,
);
const epMoves = legalMoves(g4.board, parseSquare("e5"), g4);
assert(epMoves.some((m) => m.enPassant && m.col === 3), "en passant available");
r = makeMove(g4, parseSquare("e5"), parseSquare("d6"));
assert(r.ok, "EP capture ok");
assert(!g4.board[3][3], "black pawn removed (d5)");
assert(
  g4.board[2][3]?.type === "p" && g4.board[2][3]?.color === "w",
  "white pawn on d6",
);

// 9. Promotion
const g5 = createGame();
g5.board = Array.from({ length: 8 }, () => Array(8).fill(null));
g5.board[1][0] = { type: "p", color: "w" };
g5.board[7][0] = { type: "k", color: "w" };
g5.board[0][7] = { type: "k", color: "b" };
g5.castling = { w: { k: false, q: false }, b: { k: false, q: false } };
g5.turn = "w";
r = makeMove(g5, parseSquare("a7"), parseSquare("a8"), "q");
assert(r.ok, "promo ok");
assert(g5.board[0][0]?.type === "q", "promoted to queen");
assert(r.move.san.includes("=Q"), `promo SAN ${r.move.san}`);

// 10. Cannot ignore check
const g6 = createGame();
makeMove(g6, parseSquare("e2"), parseSquare("e4"));
makeMove(g6, parseSquare("f7"), parseSquare("f6"));
makeMove(g6, parseSquare("d1"), parseSquare("h5"));
assert(
  g6.status === "check" || isInCheck(g6.board, "b"),
  `black in check status=${g6.status}`,
);
const bad = makeMove(g6, parseSquare("a7"), parseSquare("a6"));
assert(!bad.ok, "cannot ignore check");

console.log("\nRESULT", pass, "passed,", fail, "failed");
process.exit(fail ? 1 : 0);
