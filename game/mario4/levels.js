// ═══════════════════════════════════════════════════════════════
//  levels.js  –  tile layout, enemies, coins for each level
// ═══════════════════════════════════════════════════════════════

// Tile types:
// 1=ground 2=brick 3=?block 4=pipe-top 5=pipe-body 6=used-block
// 7=spike  8=ice   9=lava-top 10=lava-body  11=cloud-platform

function buildGround(cols, gaps=[]) {
  const t = [];
  const gapSet = new Set(gaps);
  for (let c = 0; c < cols; c++) {
    if (gapSet.has(c)) continue;
    t.push([c,11,1],[c,12,1]);
  }
  return t;
}

// ─── Level 1: Grassy Plains ────────────────────────────────────
function generateLevel1Tiles() {
  const t = buildGround(105, [14,15,19,20,36,37,55,56,57,77,78]);
  t.push(
    // Platforms
    [4,9,2],[5,9,3],[6,9,2],
    [10,7,2],[11,7,3],[12,7,2],[13,7,2],
    [17,8,3],[18,8,2],
    [22,9,2],[23,9,3],[24,9,2],[25,9,2],
    [29,7,2],[30,7,3],[31,7,2],
    [34,9,3],[35,9,2],
    [38,9,2],[39,9,3],[40,9,2],
    [43,7,3],[44,7,2],[45,7,3],
    [49,8,2],[50,8,3],[51,8,2],
    [53,9,2],[54,9,3],
    [58,8,2],[59,8,3],[60,8,2],[61,8,2],
    [64,9,2],[65,9,3],[66,9,2],
    [70,7,2],[71,7,3],[72,7,2],[73,7,2],
    [76,9,3],[77,9,2],  // over gap
    [79,9,2],[80,9,3],
    [83,8,2],[84,8,3],[85,8,2],
    [88,7,2],[89,7,3],[90,7,2],
    [94,9,3],[95,9,2],[96,9,3],
    // Pipes
    [21,10,4],[21,11,5],[21,12,5],
    [38,9,4],[38,10,5],[38,11,5],[38,12,5],  // overrides platform above intentionally
    [66,8,4],[66,9,5],[66,10,5],[66,11,5],[66,12,5],
  );
  return t;
}

// ─── Level 2: Underground Caves ───────────────────────────────
function generateLevel2Tiles() {
  const t = buildGround(115, [11,12,23,24,25,40,41,42,60,61,80,81,82]);
  // Ceiling
  for (let c = 0; c < 115; c++) t.push([c,0,1],[c,1,2]);
  t.push(
    [3,9,2],[4,9,3],[5,9,2],
    [8,7,2],[9,7,3],[10,7,2],
    [13,8,3],[14,8,2],[15,8,2],
    [19,9,3],[20,9,2],[21,9,2],
    [26,8,2],[27,8,3],[28,8,2],
    [31,7,2],[32,7,3],[33,7,2],[34,7,2],
    [37,9,3],[38,9,2],
    [43,8,2],[44,8,3],[45,8,2],[46,8,3],
    [49,9,3],[50,9,2],
    [53,7,2],[54,7,3],[55,7,2],[56,7,3],
    [62,8,2],[63,8,3],[64,8,2],
    [67,9,2],[68,9,3],[69,9,2],[70,9,2],
    [74,8,3],[75,8,2],[76,8,2],
    [83,9,3],[84,9,2],[85,9,3],
    [87,7,2],[88,7,2],[89,7,3],
    [92,8,2],[93,8,3],[94,8,2],
    [98,9,3],[99,9,2],
    [102,8,3],[103,8,2],[104,8,3],
    // Pipes (shorter due to ceiling)
    [30,9,4],[30,10,5],[30,11,5],[30,12,5],
    [56,8,4],[56,9,5],[56,10,5],[56,11,5],[56,12,5],
    [82,9,4],[82,10,5],[82,11,5],[82,12,5],
  );
  return t;
}

// ─── Level 3: Sky Castle ──────────────────────────────────────
function generateLevel3Tiles() {
  const t = [];
  // Floating cloud platforms instead of ground rows
  const cloudPlats = [
    [0,11],[1,11],[2,11],[3,11],[4,11],[5,11],         // Start pad
    [8,11],[9,11],[10,11],
    [12,10],[13,10],[14,10],
    [16,9],[17,9],[18,9],
    [20,10],[21,10],
    [23,11],[24,11],[25,11],[26,11],
    [29,10],[30,10],[31,10],
    [33,9],[34,9],
    [36,10],[37,10],[38,10],
    [40,11],[41,11],[42,11],[43,11],
    [46,10],[47,10],[48,10],
    [50,9],[51,9],
    [53,10],[54,10],[55,10],
    [57,11],[58,11],[59,11],[60,11],
    [63,10],[64,10],[65,10],
    [67,9],[68,9],
    [70,10],[71,10],[72,10],
    [74,11],[75,11],[76,11],[77,11],
    [80,10],[81,10],
    [83,9],[84,9],[85,9],
    [88,10],[89,10],[90,10],
    [92,11],[93,11],[94,11],[95,11],[96,11],   // Castle approach
    [98,11],[99,11],[100,11],[101,11],[102,11],[103,11],[104,11],[105,11], // Castle ground
  ];
  cloudPlats.forEach(([c,r]) => t.push([c,r,11])); // 11 = cloud tile
  // Extra platforms with blocks
  t.push(
    [4,8,3],[5,8,2],[6,8,3],
    [9,7,3],[10,7,2],
    [17,7,3],[18,7,2],
    [24,8,3],[25,8,3],
    [33,7,3],[34,7,2],
    [41,9,3],[42,9,3],
    [50,7,3],[51,7,2],
    [58,8,3],[59,8,3],
    [67,7,3],[68,7,2],
    [75,8,3],[76,8,2],[77,8,3],
    [83,7,3],[84,7,2],
    [92,8,2],[93,8,3],[94,8,2],
    [98,9,3],[99,9,3],[100,9,3],
    // Pipes on castle ground
    [96,10,4],[96,11,5],[96,12,5],
    [103,9,4],[103,10,5],[103,11,5],[103,12,5],
  );
  return t;
}

// ─── Coin generator ───────────────────────────────────────────
function generateCoins(lv) {
  const sets = {
    1: [[4,8],[5,8],[10,6],[11,6],[17,7],[23,8],[24,8],[29,6],[30,6],[43,6],[44,6],
        [49,7],[50,7],[58,7],[59,7],[64,8],[70,6],[71,6],[83,7],[84,7],[89,6],[90,6],[94,8],[95,8]],
    2: [[3,8],[4,8],[8,6],[9,6],[13,7],[19,8],[20,8],[27,7],[31,6],[32,6],[43,7],[44,7],
        [49,8],[53,6],[54,6],[62,7],[63,7],[67,8],[68,8],[74,7],[83,8],[84,8],[92,7],[93,7],[98,8],[99,8]],
    3: [[4,7],[5,7],[9,6],[17,6],[24,7],[33,6],[41,8],[50,6],[58,7],[67,6],[75,7],[76,7],[83,6],[92,7],[93,7],[98,8],[99,8]],
  };
  return (sets[lv]||[]).map(([tx,ty]) => ({
    x:tx*TILE+12, y:ty*TILE+6, w:16, h:20, active:true, bobTimer:Math.random()*Math.PI*2
  }));
}

// ─── Level definitions ────────────────────────────────────────
const LEVELS = [
  {
    id: 1,
    name: 'GRASSY PLAINS',
    bg: ['#5c94fc','#3a78e0'], sky2: null,
    groundColor: '#c84c0c', brickColor: '#d07040', cloudColor:'rgba(255,255,255,0.75)',
    mapW: 105,
    tiles: generateLevel1Tiles(),
    enemies: [
      {type:'goomba',tx:12,ty:10},{type:'goomba',tx:18,ty:10},
      {type:'goomba',tx:25,ty:10},{type:'koopa', tx:32,ty:10},
      {type:'goomba',tx:40,ty:10},{type:'koopa', tx:48,ty:8},
      {type:'goomba',tx:55,ty:10},{type:'goomba',tx:60,ty:10},
      {type:'koopa', tx:68,ty:10},{type:'goomba',tx:75,ty:10},
      {type:'piranha',tx:21,ty:7},{type:'piranha',tx:38,ty:5},
      {type:'goomba',tx:82,ty:10},{type:'koopa',tx:88,ty:8},
      {type:'goomba',tx:95,ty:10},{type:'goomba',tx:98,ty:8},
    ],
    coins: generateCoins(1),
    flagX: 100,
  },
  {
    id: 2,
    name: 'DARK CAVERNS',
    bg: ['#1a1a2e','#16213e'], sky2: '#0d0d1a',
    groundColor: '#445566', brickColor: '#667788', cloudColor:'rgba(100,130,200,0.3)',
    mapW: 115,
    tiles: generateLevel2Tiles(),
    enemies: [
      {type:'goomba',tx:10,ty:10},{type:'koopa', tx:16,ty:10},
      {type:'goomba',tx:22,ty:10},{type:'koopa', tx:28,ty:8},
      {type:'goomba',tx:34,ty:10},{type:'goomba',tx:38,ty:10},
      {type:'koopa', tx:45,ty:10},{type:'piranha',tx:30,ty:7},
      {type:'goomba',tx:52,ty:10},{type:'koopa', tx:60,ty:10},
      {type:'goomba',tx:68,ty:10},{type:'koopa', tx:75,ty:8},
      {type:'piranha',tx:56,ty:5},{type:'goomba',tx:82,ty:10},
      {type:'koopa', tx:90,ty:8},{type:'goomba',tx:96,ty:10},
      {type:'goomba',tx:100,ty:10},{type:'koopa',tx:104,ty:8},
    ],
    coins: generateCoins(2),
    flagX: 110,
  },
  {
    id: 3,
    name: 'SKY CASTLE',
    bg: ['#b0d8ff','#e8f4ff'], sky2: '#f0f8ff',
    groundColor: '#f5e6c8', brickColor: '#e8c878', cloudColor:'rgba(255,255,255,0.9)',
    mapW: 110,
    tiles: generateLevel3Tiles(),
    enemies: [
      {type:'koopa', tx:10,ty:10},{type:'goomba',tx:13,ty:9},
      {type:'koopa', tx:17,ty:8},{type:'goomba',tx:21,ty:9},
      {type:'koopa', tx:25,ty:10},{type:'koopa', tx:30,ty:9},
      {type:'goomba',tx:34,ty:8},{type:'goomba',tx:38,ty:9},
      {type:'koopa', tx:42,ty:10},{type:'piranha',tx:96,ty:8},
      {type:'goomba',tx:47,ty:9},{type:'koopa', tx:51,ty:8},
      {type:'goomba',tx:55,ty:10},{type:'koopa', tx:59,ty:10},
      {type:'goomba',tx:64,ty:9},{type:'koopa', tx:68,ty:8},
      {type:'goomba',tx:72,ty:10},{type:'koopa', tx:76,ty:10},
      {type:'goomba',tx:80,ty:9},{type:'koopa', tx:85,ty:8},
      {type:'goomba',tx:89,ty:10},{type:'koopa', tx:93,ty:10},
      {type:'piranha',tx:103,ty:7},{type:'koopa',tx:100,ty:8},
    ],
    coins: generateCoins(3),
    flagX: 106,
  },
];
