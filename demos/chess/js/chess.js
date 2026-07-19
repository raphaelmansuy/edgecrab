/**
 * Chess rules engine — legal moves, check, castling, en passant, promotion.
 * Board: row 0 = rank 8, row 7 = rank 1; col 0 = file a, col 7 = file h.
 * Pieces: { type: 'k'|'q'|'r'|'b'|'n'|'p', color: 'w'|'b' }
 */

const FILES = "abcdefgh";
const PIECE_VALUES = { p: 1, n: 3, b: 3, r: 5, q: 9, k: 0 };

export function cloneBoard(board) {
  return board.map((row) => row.map((p) => (p ? { ...p } : null)));
}

export function emptyBoard() {
  return Array.from({ length: 8 }, () => Array(8).fill(null));
}

export function startingBoard() {
  const b = emptyBoard();
  const back = ["r", "n", "b", "q", "k", "b", "n", "r"];
  for (let c = 0; c < 8; c++) {
    b[0][c] = { type: back[c], color: "b" };
    b[1][c] = { type: "p", color: "b" };
    b[6][c] = { type: "p", color: "w" };
    b[7][c] = { type: back[c], color: "w" };
  }
  return b;
}

export function squareName(row, col) {
  return FILES[col] + (8 - row);
}

export function parseSquare(name) {
  if (!name || name.length < 2) return null;
  const col = FILES.indexOf(name[0].toLowerCase());
  const rank = Number(name[1]);
  if (col < 0 || rank < 1 || rank > 8) return null;
  return { row: 8 - rank, col };
}

export function inBounds(r, c) {
  return r >= 0 && r < 8 && c >= 0 && c < 8;
}

function enemy(color) {
  return color === "w" ? "b" : "w";
}

function findKing(board, color) {
  for (let r = 0; r < 8; r++) {
    for (let c = 0; c < 8; c++) {
      const p = board[r][c];
      if (p && p.type === "k" && p.color === color) return { row: r, col: c };
    }
  }
  return null;
}

function isSquareAttacked(board, row, col, byColor) {
  // Pawns
  const pr = byColor === "w" ? row + 1 : row - 1;
  for (const dc of [-1, 1]) {
    const c = col + dc;
    if (inBounds(pr, c)) {
      const p = board[pr][c];
      if (p && p.color === byColor && p.type === "p") return true;
    }
  }
  // Knights
  const kn = [
    [-2, -1], [-2, 1], [-1, -2], [-1, 2],
    [1, -2], [1, 2], [2, -1], [2, 1],
  ];
  for (const [dr, dc] of kn) {
    const r = row + dr;
    const c = col + dc;
    if (!inBounds(r, c)) continue;
    const p = board[r][c];
    if (p && p.color === byColor && p.type === "n") return true;
  }
  // King
  for (let dr = -1; dr <= 1; dr++) {
    for (let dc = -1; dc <= 1; dc++) {
      if (!dr && !dc) continue;
      const r = row + dr;
      const c = col + dc;
      if (!inBounds(r, c)) continue;
      const p = board[r][c];
      if (p && p.color === byColor && p.type === "k") return true;
    }
  }
  // Sliding: bishop/queen diagonals, rook/queen orthogonals
  const rays = [
    { dr: -1, dc: 0, types: ["r", "q"] },
    { dr: 1, dc: 0, types: ["r", "q"] },
    { dr: 0, dc: -1, types: ["r", "q"] },
    { dr: 0, dc: 1, types: ["r", "q"] },
    { dr: -1, dc: -1, types: ["b", "q"] },
    { dr: -1, dc: 1, types: ["b", "q"] },
    { dr: 1, dc: -1, types: ["b", "q"] },
    { dr: 1, dc: 1, types: ["b", "q"] },
  ];
  for (const ray of rays) {
    let r = row + ray.dr;
    let c = col + ray.dc;
    while (inBounds(r, c)) {
      const p = board[r][c];
      if (p) {
        if (p.color === byColor && ray.types.includes(p.type)) return true;
        break;
      }
      r += ray.dr;
      c += ray.dc;
    }
  }
  return false;
}

export function isInCheck(board, color) {
  const k = findKing(board, color);
  if (!k) return false;
  return isSquareAttacked(board, k.row, k.col, enemy(color));
}

function pushSlide(board, from, color, dr, dc, moves) {
  let r = from.row + dr;
  let c = from.col + dc;
  while (inBounds(r, c)) {
    const t = board[r][c];
    if (!t) {
      moves.push({ row: r, col: c });
    } else {
      if (t.color !== color) moves.push({ row: r, col: c, capture: true });
      break;
    }
    r += dr;
    c += dc;
  }
}

function pseudoMoves(board, from, state) {
  const piece = board[from.row][from.col];
  if (!piece) return [];
  const { type, color } = piece;
  const moves = [];
  const dir = color === "w" ? -1 : 1;

  if (type === "p") {
    const fr = from.row + dir;
    if (inBounds(fr, from.col) && !board[fr][from.col]) {
      moves.push({ row: fr, col: from.col });
      const start = color === "w" ? 6 : 1;
      const fr2 = from.row + 2 * dir;
      if (from.row === start && !board[fr2][from.col]) {
        moves.push({ row: fr2, col: from.col, doublePawn: true });
      }
    }
    for (const dc of [-1, 1]) {
      const c = from.col + dc;
      if (!inBounds(fr, c)) continue;
      const t = board[fr][c];
      if (t && t.color !== color) {
        moves.push({ row: fr, col: c, capture: true });
      }
      // en passant
      if (state.ep && state.ep.row === fr && state.ep.col === c) {
        moves.push({ row: fr, col: c, capture: true, enPassant: true });
      }
    }
  } else if (type === "n") {
    for (const [dr, dc] of [
      [-2, -1], [-2, 1], [-1, -2], [-1, 2],
      [1, -2], [1, 2], [2, -1], [2, 1],
    ]) {
      const r = from.row + dr;
      const c = from.col + dc;
      if (!inBounds(r, c)) continue;
      const t = board[r][c];
      if (!t || t.color !== color) {
        moves.push({ row: r, col: c, capture: !!t });
      }
    }
  } else if (type === "b") {
    for (const [dr, dc] of [[-1, -1], [-1, 1], [1, -1], [1, 1]]) {
      pushSlide(board, from, color, dr, dc, moves);
    }
  } else if (type === "r") {
    for (const [dr, dc] of [[-1, 0], [1, 0], [0, -1], [0, 1]]) {
      pushSlide(board, from, color, dr, dc, moves);
    }
  } else if (type === "q") {
    for (const [dr, dc] of [
      [-1, 0], [1, 0], [0, -1], [0, 1],
      [-1, -1], [-1, 1], [1, -1], [1, 1],
    ]) {
      pushSlide(board, from, color, dr, dc, moves);
    }
  } else if (type === "k") {
    for (let dr = -1; dr <= 1; dr++) {
      for (let dc = -1; dc <= 1; dc++) {
        if (!dr && !dc) continue;
        const r = from.row + dr;
        const c = from.col + dc;
        if (!inBounds(r, c)) continue;
        const t = board[r][c];
        if (!t || t.color !== color) {
          moves.push({ row: r, col: c, capture: !!t });
        }
      }
    }
    // castling
    const rights = state.castling[color];
    const back = color === "w" ? 7 : 0;
    if (from.row === back && from.col === 4 && !isInCheck(board, color)) {
      if (rights.k) {
        if (!board[back][5] && !board[back][6]) {
          const rook = board[back][7];
          if (rook && rook.type === "r" && rook.color === color) {
            if (
              !isSquareAttacked(board, back, 5, enemy(color)) &&
              !isSquareAttacked(board, back, 6, enemy(color))
            ) {
              moves.push({ row: back, col: 6, castle: "k" });
            }
          }
        }
      }
      if (rights.q) {
        if (!board[back][1] && !board[back][2] && !board[back][3]) {
          const rook = board[back][0];
          if (rook && rook.type === "r" && rook.color === color) {
            if (
              !isSquareAttacked(board, back, 3, enemy(color)) &&
              !isSquareAttacked(board, back, 2, enemy(color))
            ) {
              moves.push({ row: back, col: 2, castle: "q" });
            }
          }
        }
      }
    }
  }
  return moves;
}

function applyMoveRaw(board, from, to, promo) {
  const next = cloneBoard(board);
  const piece = next[from.row][from.col];
  let captured = null;

  if (to.enPassant) {
    const capRow = piece.color === "w" ? to.row + 1 : to.row - 1;
    captured = next[capRow][to.col];
    next[capRow][to.col] = null;
  } else if (next[to.row][to.col]) {
    captured = next[to.row][to.col];
  }

  next[to.row][to.col] = piece;
  next[from.row][from.col] = null;

  if (to.castle === "k") {
    const back = from.row;
    next[back][5] = next[back][7];
    next[back][7] = null;
  } else if (to.castle === "q") {
    const back = from.row;
    next[back][3] = next[back][0];
    next[back][0] = null;
  }

  if (piece.type === "p" && (to.row === 0 || to.row === 7)) {
    piece.type = promo || "q";
  }

  return { board: next, captured };
}

export function legalMoves(board, from, state) {
  const piece = board[from.row]?.[from.col];
  if (!piece || piece.color !== state.turn) return [];
  const raw = pseudoMoves(board, from, state);
  const out = [];
  for (const to of raw) {
    const { board: nb } = applyMoveRaw(board, from, to, "q");
    if (!isInCheck(nb, piece.color)) out.push(to);
  }
  return out;
}

export function allLegalMoves(board, state) {
  const list = [];
  for (let r = 0; r < 8; r++) {
    for (let c = 0; c < 8; c++) {
      const p = board[r][c];
      if (!p || p.color !== state.turn) continue;
      const from = { row: r, col: c };
      for (const to of legalMoves(board, from, state)) {
        list.push({ from, to });
      }
    }
  }
  return list;
}

export function createGame() {
  return {
    board: startingBoard(),
    turn: "w",
    castling: {
      w: { k: true, q: true },
      b: { k: true, q: true },
    },
    ep: null,
    halfmove: 0,
    fullmove: 1,
    history: [],
    captured: { w: [], b: [] },
    status: "playing", // playing | check | checkmate | stalemate | draw
    winner: null,
  };
}

function updateCastling(state, from, to, piece, captured) {
  const c = { w: { ...state.castling.w }, b: { ...state.castling.b } };
  if (piece.type === "k") {
    c[piece.color] = { k: false, q: false };
  }
  if (piece.type === "r") {
    if (from.row === 7 && from.col === 0) c.w.q = false;
    if (from.row === 7 && from.col === 7) c.w.k = false;
    if (from.row === 0 && from.col === 0) c.b.q = false;
    if (from.row === 0 && from.col === 7) c.b.k = false;
  }
  if (captured && captured.type === "r") {
    if (to.row === 7 && to.col === 0) c.w.q = false;
    if (to.row === 7 && to.col === 7) c.w.k = false;
    if (to.row === 0 && to.col === 0) c.b.q = false;
    if (to.row === 0 && to.col === 7) c.b.k = false;
  }
  return c;
}

function needsPromotion(piece, to) {
  return piece.type === "p" && (to.row === 0 || to.row === 7);
}

export function makeMove(game, from, to, promo = "q") {
  if (
    game.status === "checkmate" ||
    game.status === "stalemate" ||
    game.status === "draw"
  ) {
    return { ok: false, reason: "game over" };
  }
  const piece = game.board[from.row]?.[from.col];
  if (!piece || piece.color !== game.turn) {
    return { ok: false, reason: "not your piece" };
  }
  const legal = legalMoves(game.board, from, game);
  const match = legal.find((m) => m.row === to.row && m.col === to.col);
  if (!match) return { ok: false, reason: "illegal" };

  if (needsPromotion(piece, match) && !["q", "r", "b", "n"].includes(promo)) {
    return { ok: false, reason: "need promotion", needsPromo: true };
  }

  const snapshot = {
    board: cloneBoard(game.board),
    turn: game.turn,
    castling: {
      w: { ...game.castling.w },
      b: { ...game.castling.b },
    },
    ep: game.ep ? { ...game.ep } : null,
    halfmove: game.halfmove,
    fullmove: game.fullmove,
    status: game.status,
    winner: game.winner,
    captured: {
      w: [...game.captured.w],
      b: [...game.captured.b],
    },
    move: null,
  };

  const moving = { ...piece };
  const { board: nextBoard, captured } = applyMoveRaw(
    game.board,
    from,
    match,
    needsPromotion(piece, match) ? promo : null,
  );

  let ep = null;
  if (match.doublePawn) {
    ep = { row: (from.row + match.row) >> 1, col: from.col };
  }

  const san = toSAN(game.board, from, match, moving, captured, promo, nextBoard);

  if (captured) {
    game.captured[moving.color].push(captured.type);
  }

  game.board = nextBoard;
  game.castling = updateCastling(game, from, match, moving, captured);
  game.ep = ep;
  game.halfmove =
    moving.type === "p" || captured ? 0 : game.halfmove + 1;
  if (game.turn === "b") game.fullmove += 1;
  game.turn = enemy(game.turn);

  const replyMoves = allLegalMoves(game.board, game);
  const inCheck = isInCheck(game.board, game.turn);
  if (replyMoves.length === 0) {
    if (inCheck) {
      game.status = "checkmate";
      game.winner = enemy(game.turn);
    } else {
      game.status = "stalemate";
      game.winner = null;
    }
  } else if (inCheck) {
    game.status = "check";
    game.winner = null;
  } else if (game.halfmove >= 100) {
    game.status = "draw";
    game.winner = null;
  } else {
    game.status = "playing";
    game.winner = null;
  }

  const record = {
    from: { ...from },
    to: { row: match.row, col: match.col },
    san,
    piece: moving.type,
    color: moving.color,
    promo: needsPromotion(moving, match) ? promo : null,
    captured: captured ? captured.type : null,
  };
  snapshot.move = record;
  game.history.push(snapshot);

  return { ok: true, move: record, needsPromo: false };
}

export function undoMove(game) {
  const snap = game.history.pop();
  if (!snap) return false;
  game.board = snap.board;
  game.turn = snap.turn;
  game.castling = snap.castling;
  game.ep = snap.ep;
  game.halfmove = snap.halfmove;
  game.fullmove = snap.fullmove;
  game.status = snap.status;
  game.winner = snap.winner;
  game.captured = snap.captured;
  return true;
}

function disambiguate(board, from, to, piece) {
  if (piece.type === "p" || piece.type === "k") return "";
  const others = [];
  for (let r = 0; r < 8; r++) {
    for (let c = 0; c < 8; c++) {
      if (r === from.row && c === from.col) continue;
      const p = board[r][c];
      if (!p || p.color !== piece.color || p.type !== piece.type) continue;
      // crude: same type can move to target via pseudo (ignore pin for SAN simplicity)
      const state = {
        turn: piece.color,
        castling: { w: { k: false, q: false }, b: { k: false, q: false } },
        ep: null,
      };
      const ms = pseudoMoves(board, { row: r, col: c }, state);
      if (ms.some((m) => m.row === to.row && m.col === to.col)) {
        others.push({ row: r, col: c });
      }
    }
  }
  if (!others.length) return "";
  const sameFile = others.some((o) => o.col === from.col);
  const sameRank = others.some((o) => o.row === from.row);
  if (!sameFile) return FILES[from.col];
  if (!sameRank) return String(8 - from.row);
  return FILES[from.col] + String(8 - from.row);
}

function toSAN(board, from, to, piece, captured, promo, nextBoard) {
  if (to.castle === "k") return "O-O";
  if (to.castle === "q") return "O-O-O";
  const cap = captured || to.enPassant;
  let s = "";
  if (piece.type === "p") {
    if (cap) s += FILES[from.col] + "x";
    s += squareName(to.row, to.col);
    if (needsPromotion(piece, to)) s += "=" + (promo || "q").toUpperCase();
  } else {
    s += piece.type.toUpperCase();
    s += disambiguate(board, from, to, piece);
    if (cap) s += "x";
    s += squareName(to.row, to.col);
  }
  const mover = enemy(
    nextBoard[to.row][to.col]?.color === "w" ? "b" : "w",
  );
  // after move, side to move is enemy of mover
  const side = nextBoard[to.row][to.col].color === "w" ? "b" : "w";
  if (isInCheck(nextBoard, side)) {
    const st = {
      turn: side,
      castling: { w: { k: true, q: true }, b: { k: true, q: true } },
      ep: null,
    };
    const any = allLegalMoves(nextBoard, { ...st, castling: st.castling });
    s += any.length === 0 ? "#" : "+";
  }
  return s;
}

export function pieceSymbol(type, color) {
  const white = { k: "♔", q: "♕", r: "♖", b: "♗", n: "♘", p: "♙" };
  const black = { k: "♚", q: "♛", r: "♜", b: "♝", n: "♞", p: "♟" };
  return (color === "w" ? white : black)[type] || "?";
}

export function materialScore(captured) {
  const sum = (arr) => arr.reduce((a, t) => a + (PIECE_VALUES[t] || 0), 0);
  return sum(captured.w) - sum(captured.b);
}
