// Tutorial 5 — Safe SQL Agent.
//
// JS-layer enforcement: allow-listed tables, read-only guard.
// The host application pre-validates every SQL query before the agent
// processes it. Writes and unknown tables are blocked at the JS layer —
// no LLM can override it.
//
// Note: Custom tool injection is a Rust SDK feature. Python/Node.js
// enforce safety via the validator wrapper pattern shown here.
// See: site/src/content/docs/tutorials/05-custom-tool-safe-sql.md

import { Agent } from 'edgecrab';

const ALLOWED = new Set(['orders', 'customers', 'products']);

// ── Validation layer (runs in JS, before the agent sees results) ──────────────

function validateSql(query) {
  // 1. Allow-list check
  if (![...ALLOWED].some(t => query.toLowerCase().includes(t))) {
    throw new Error(`Query must reference one of [${[...ALLOWED]}]. Query rejected.`);
  }
  // 2. Block writes
  const upper = query.trimStart().toUpperCase();
  if (!upper.startsWith('SELECT') && !upper.startsWith('WITH')) {
    process.stderr.write(`BLOCKED WRITE: ${query}\n`);
    throw new Error('Writes require operator approval. Request denied.');
  }
  // 3. Stub result — plug in your DB pool here
  return JSON.stringify({
    query,
    rows: [{ count: 42, total_revenue: 189_432.5 }],
    executed_at: '2026-04-17T12:00:00Z',
  });
}

// ── Helper: run one scenario with pre-validation ──────────────────────────────

async function runScenario(agent, userRequest, sqlToValidate) {
  console.log(`Request: ${userRequest}`);
  try {
    const rows = validateSql(sqlToValidate);
    const result = await agent.run(
      `User request: ${userRequest}\n\nQuery result: ${rows}\n\nSummarise in one sentence.`
    );
    console.log(`  Response: ${result.response}`);
    console.log(`  Cost:     $${result.totalCost.toFixed(6)}\n`);
    return result.totalCost;
  } catch (e) {
    console.log(`  BLOCKED: ${e.message}\n`);
    return 0;
  }
}

// ── Main ──────────────────────────────────────────────────────────────────────

const agent = new Agent({
  model: 'copilot/gpt-5-mini',
  maxIterations: 4,
  quietMode: true,
  instructions:
    'You are a data analyst. You receive pre-validated SQL results. ' +
    'Summarise in plain English. Never suggest running additional queries.',
});

let totalCost = 0;

// Safe: read-only on allowed table
totalCost += await runScenario(
  agent,
  'How many orders were placed this month?',
  'SELECT COUNT(*) AS count FROM orders WHERE month = CURRENT_MONTH',
);

// Blocked: write mutation
totalCost += await runScenario(
  agent,
  'Delete all orders from last year.',
  'DELETE FROM orders WHERE year = 2025',
);

// Blocked: unknown table
totalCost += await runScenario(
  agent,
  'Show me all users.',
  'SELECT * FROM users',
);

console.log(`Total cost: $${totalCost.toFixed(6)}`);

