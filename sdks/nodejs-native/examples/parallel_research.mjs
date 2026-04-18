// Tutorial 2 — Parallel Research Pipeline.
//
// Wall-clock time becomes the slowest call, not the sum. ~5× throughput.
// See: site/src/content/docs/tutorials/02-parallel-research.md

import { Agent } from 'edgecrab';

const QUERIES = [
  'Summarize the Rust borrow checker in one sentence.',
  "Summarize Python's GIL in one sentence.",
  "Summarize Go's goroutines in one sentence.",
  "Summarize Erlang's actor model in one sentence.",
  "Summarize Haskell's laziness in one sentence.",
];

const agent = new Agent({
  model: 'copilot/gpt-5-mini',
  maxIterations: 3,
  quietMode: true,
});

const t0 = performance.now();
const results = await agent.batch(QUERIES);
const elapsed = (performance.now() - t0) / 1000;

QUERIES.forEach((q, i) => {
  console.log(`Q: ${q}\n  → ${results[i] ?? 'ERROR'}\n`);
});
console.log(`── wall-clock: ${elapsed.toFixed(2)}s ──`);
