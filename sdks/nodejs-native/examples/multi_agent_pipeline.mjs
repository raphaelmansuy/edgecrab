// Tutorial 3 — Multi-Agent Pipeline.
//
// Coordinator forks three specialist agents. All run in parallel.
// Wall-clock time = slowest specialist, not the sum. Cheaper models
// handle mechanical tasks; the flagship model handles design judgment.
// See: site/src/content/docs/tutorials/03-multi-agent-pipeline.md
//
// ASCII Architecture:
//
//   ┌─────────────────────────────────────┐
//   │         Coordinator Agent           │
//   │    (claude-sonnet-4.6 + context)    │
//   └────────────┬───────────────────────┘
//                │ fork() × 3
//        ┌───────┼───────┐
//        ▼       ▼       ▼
//  ┌──────────┐ ┌──────┐ ┌──────────┐
//  │API Design│ │ curl │ │  pytest  │
//  │ sonnet   │ │ mini │ │  mini    │
//  └──────────┘ └──────┘ └──────────┘
//        │       │       │
//        └───────┴───────┘
//                │
//        ┌───────▼────────┐
//        │  Coordinator   │
//        │  synthesizes   │
//        └────────────────┘

import { Agent } from 'edgecrab';

const t0 = performance.now();

// ── 1. Coordinator establishes shared context ─────────────────────────────
const coordinator = new Agent({
  model: 'copilot/claude-sonnet-4.6',
  maxIterations: 5,
  quietMode: true,
  instructions:
    'You are a technical lead coordinating an API design project. ' +
    'Be precise and use concrete examples.',
});

await coordinator.chat(
  "We're designing a REST API for a bookmark manager. " +
  'Users can create, list, tag, and share bookmarks. ' +
  'Respond with one sentence confirming you understand the project scope.'
);

// ── 2. Fork three specialists — each inherits the coordinator context ─────
const apiDesigner  = await coordinator.fork();
const exampleWriter = await coordinator.fork();
const testWriter    = await coordinator.fork();

// Cheaper model for mechanical tasks — cuts cost by ~10×
await exampleWriter.setModel('copilot/gpt-5-mini');
await testWriter.setModel('copilot/gpt-5-mini');

// ── 3. Run all specialists in parallel ───────────────────────────────────
const [endpoints, examples, tests] = await Promise.all([
  apiDesigner.run(
    'Design the REST endpoints. Return a markdown table with columns: ' +
    'Method | Path | Description | Request Body | Response'
  ),
  exampleWriter.run(
    'Write 5 curl examples covering CRUD operations and tagging. ' +
    'Use http://localhost:8080 as the base URL.'
  ),
  testWriter.run(
    'Write 5 pytest test cases using httpx for the bookmarks API. ' +
    'Test: create, list, get-by-id, add-tag, and share.'
  ),
]);

const elapsed = (performance.now() - t0) / 1000;

// ── 4. Display specialist results ────────────────────────────────────────
console.log('=== API Endpoints ===');
console.log(endpoints.response.slice(0, 800));

console.log('\n=== Curl Examples ===');
console.log(examples.response.slice(0, 600));

console.log('\n=== Test Cases ===');
console.log(tests.response.slice(0, 600));

// ── 5. Coordinator synthesizes the final document ────────────────────────
const summary = await coordinator.run(
  'Here is our API design:\n\n' +
  endpoints.response.slice(0, 500) +
  '\n\nWrite a one-paragraph executive summary and list 3 next steps.'
);

console.log('\n=== Executive Summary ===');
console.log(summary.response);

// ── 6. Metrics ───────────────────────────────────────────────────────────
const totalCost =
  endpoints.totalCost + examples.totalCost + tests.totalCost + summary.totalCost;

console.log('\n' + '─'.repeat(40));
console.log(`Wall-clock:     ${elapsed.toFixed(2)}s (parallel specialist phase)`);
console.log(`Total cost:     $${totalCost.toFixed(6)}`);
console.log(
  `  coordinator:  $${(summary.totalCost).toFixed(6)}`
);
console.log(
  `  specialists:  $${(endpoints.totalCost + examples.totalCost + tests.totalCost).toFixed(6)}`
);
