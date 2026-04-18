// Tutorial 4 — Session-Aware Support Bot.
//
// FTS5 search finds relevant prior tickets in <10ms, injects them as
// context so the agent resolves issues faster using org memory.
// See: site/src/content/docs/tutorials/04-session-aware-support.md

import { Agent } from 'edgecrab';

const userMessage = 'My K8s pods keep OOMKilling under load.';

// 1. Search prior sessions using Agent.searchSessions() — no LLM call made
let priorContext = '';
try {
  // A lightweight agent instance for session DB access only.
  // searchSessions() queries the local SQLite DB, never the LLM.
  const searcher = new Agent({ model: 'copilot/gpt-5-mini', quietMode: true });
  const hits = searcher.searchSessions(userMessage, 3);
  if (hits.length > 0) {
    priorContext = '\n\n--- RELEVANT PRIOR TICKETS ---\n';
    for (const h of hits) {
      priorContext += `[session=${h.sessionId.slice(0, 8)}, score=${h.score.toFixed(2)}]\n${h.snippet}\n\n`;
    }
    console.log(`[search] found ${hits.length} prior session(s)`);
  } else {
    console.log('[search] no matching prior sessions — starting cold');
  }
} catch (e) {
  console.log(`[search] session DB unavailable (${e}), starting cold`);
}

// 2. Spin up agent with prior context pre-loaded
const agent = new Agent({
  model: 'copilot/gpt-5-mini',
  maxIterations: 6,
  quietMode: true,
  instructions:
    'You are a senior support engineer. If prior tickets contain the answer, ' +
    'reference them by session ID. Always state the ROOT CAUSE before the FIX.',
});

const result = await agent.run(`User ticket: ${userMessage}${priorContext}`);

console.log(`Resolution:\n${result.response}`);
console.log(`\nSession:  ${result.sessionId}`);
console.log(`Cost:     $${result.totalCost.toFixed(6)}`);
