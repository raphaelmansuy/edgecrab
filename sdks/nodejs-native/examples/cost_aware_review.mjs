// Tutorial 1 — Cost-Aware Code Review.
//
// Two-tier triage: cheap model flags risk, expensive model does deep review.
// See: site/src/content/docs/tutorials/01-cost-aware-review.md

import { Agent } from 'edgecrab';

const DIFFS = [
  'Renamed `count` to `total_count` in stats.rs',
  'Added new auth middleware with JWT + refresh token logic',
  "Updated README typo: 'recieve' -> 'receive'",
];

const agent = new Agent({
  model: 'copilot/gpt-5-mini',
  maxIterations: 4,
  quietMode: true,
  instructions:
    'You are a strict code reviewer. For each diff, output ONE line: ' +
    'RISK=<1-10> SUMMARY=<10 words max>.',
});

let totalCost = 0;
let escalated = 0;

for (const diff of DIFFS) {
  // Tier 1 — cheap triage
  const tri = await agent.run(`Diff:\n${diff}`);
  totalCost += tri.totalCost;
  const risk = parseInt(tri.response.match(/RISK=(\d+)/)?.[1] ?? '5', 10);

  const head = tri.response.split('\n')[0] ?? '';
  console.log(`[tier1] risk=${risk} | ${head}`);

  if (risk < 7) {
    console.log('  → APPROVED\n');
    continue;
  }

  // Tier 2 — hot-swap flagship, same session
  escalated++;
  await agent.setModel('copilot/claude-sonnet-4.6');
  const deep = await agent.run(
    'The diff was flagged high-risk. Give a detailed review and 2 concrete fix suggestions.'
  );
  totalCost += deep.totalCost;
  const head2 = deep.response.split('\n')[0] ?? '';
  console.log(`  [tier2] → ${head2}\n`);

  await agent.setModel('copilot/gpt-5-mini');
}

console.log('─'.repeat(40));
console.log(`Total diffs:   ${DIFFS.length}`);
console.log(`Escalated:     ${escalated} (${Math.round((100 * escalated) / DIFFS.length)}%)`);
console.log(`Total cost:    $${totalCost.toFixed(6)}`);
console.log(`Per diff:      $${(totalCost / DIFFS.length).toFixed(6)}`);
