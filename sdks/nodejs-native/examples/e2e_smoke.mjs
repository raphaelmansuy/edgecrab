/**
 * EdgeCrab Node.js SDK — E2E Smoke Test
 *
 * Exercises the core Node.js SDK API against local Ollama.
 *
 * Run:
 *   node examples/e2e_smoke.mjs
 */
import { Agent, ModelCatalog, Session } from '../index.js';

const MODEL = 'ollama/gemma4:latest';
const CORE_FEATURES = [
  'ModelCatalog.providerIds',
  'ModelCatalog.modelsForProvider',
  'ModelCatalog.contextWindow',
  'ModelCatalog.pricing',
  'ModelCatalog.flatCatalog',
  'ModelCatalog.defaultModelFor',
  'ModelCatalog.estimateCost',
  'Agent.chat',
  'Agent.run',
  'Agent.runConversation',
  'Agent.stream',
  'Agent.getHistory',
  'Agent.newSession',
  'Agent.chatInCwd',
  'Agent.fork',
  'Agent.batch',
  'Agent.model',
  'Agent.setModel',
  'Agent.sessionSnapshot',
  'Agent.export',
  'Agent.toolNames',
  'Agent.toolsetSummary',
  'Agent.compress',
  'Agent.searchSessions',
  'Agent.listSessions',
  'Session.getMessages',
  'Session.renameSession',
  'Session.pruneSessions',
  'Session.stats',
  'MemoryManager.readWrite',
];
const covered = new Set();

function ok(label) {
  console.log(`  ✓ ${label}`);
}

function section(title) {
  console.log(`\n── ${title} ──`);
}

function mark(feature, label = feature) {
  covered.add(feature);
  ok(label);
}

function expectContains(text, expected) {
  const actual = String(text).trim().toUpperCase();
  if (!actual.includes(expected.toUpperCase())) {
    throw new Error(`Expected ${JSON.stringify(text)} to include ${expected}`);
  }
}

async function main() {
  console.log('EdgeCrab Node.js SDK — E2E Smoke Test');
  console.log(`Model: ${MODEL}`);
  console.log('═══════════════════════════════════════');

  section('1. ModelCatalog');
  const catalog = new ModelCatalog();
  const providers = catalog.providerIds();
  if (!providers.includes('ollama')) {
    throw new Error(`ollama missing from providers: ${providers}`);
  }
  mark('ModelCatalog.providerIds', `${providers.length} providers in catalog`);

  const models = catalog.modelsForProvider('openai');
  if (!Array.isArray(models) || models.length === 0) throw new Error('modelsForProvider empty');
  mark('ModelCatalog.modelsForProvider', `modelsForProvider('openai') → ${models.length} models`);

  const ctxWindow = catalog.contextWindow('openai', 'gpt-4o');
  mark('ModelCatalog.contextWindow', `contextWindow → ${ctxWindow}`);

  const pricing = catalog.pricing('openai', 'gpt-4o');
  mark('ModelCatalog.pricing', `pricing → ${JSON.stringify(pricing)}`);

  const flatCatalog = catalog.flatCatalog();
  if (!Array.isArray(flatCatalog) || flatCatalog.length === 0) throw new Error('flatCatalog empty');
  mark('ModelCatalog.flatCatalog', `flatCatalog → ${flatCatalog.length} total models`);

  const defaultModel = catalog.defaultModelFor('openai');
  if (!defaultModel) throw new Error('defaultModelFor returned empty value');
  mark('ModelCatalog.defaultModelFor', `defaultModelFor('openai') → ${defaultModel}`);

  const estimate = catalog.estimateCost('openai', 'gpt-4o', 1000, 200);
  mark('ModelCatalog.estimateCost', `estimateCost → ${JSON.stringify(estimate)}`);

  section('2. Core agent methods');
  const agent = new Agent({ model: MODEL, maxIterations: 3, quietMode: true,
    skipContextFiles: true, skipMemory: true });

  const reply = await agent.chat('Reply with exactly: PONG');
  expectContains(reply, 'PONG');
  mark('Agent.chat', `chat → ${reply.slice(0, 60).replace(/\n/g, ' ')}`);

  const run = await agent.run('Reply with exactly: OK');
  expectContains(run.response, 'OK');
  mark('Agent.run', `run → ${run.response.slice(0, 40).replace(/\n/g, ' ')}`);

  const conv = await agent.runConversation(
    'Reply with exactly: CONV_OK',
    'You are terse. Obey exactly.',
    ['Previous note: keep replies short.'],
  );
  expectContains(conv.response, 'CONV_OK');
  mark('Agent.runConversation', `runConversation → ${conv.response.slice(0, 40).replace(/\n/g, ' ')}`);

  await agent.setReasoningEffort('low');
  await agent.setStreaming(true);
  const events = await agent.stream('Reply with exactly: STREAM_OK');
  const streamed = events.filter(e => e.eventType === 'token').map(e => e.data).join('');
  expectContains(streamed || JSON.stringify(events), 'STREAM_OK');
  mark('Agent.stream', `stream → ${String(streamed).slice(0, 40).replace(/\n/g, ' ')}`);

  const history = await agent.getHistory();
  if (!history || history.length === 0) throw new Error('history empty after usage');
  mark('Agent.getHistory', `${history.length} messages in history`);

  const cwdReply = await agent.chatInCwd('Reply with exactly: CWD_OK', process.cwd());
  expectContains(cwdReply, 'CWD_OK');
  mark('Agent.chatInCwd', `chatInCwd → ${cwdReply.slice(0, 40).replace(/\n/g, ' ')}`);

  section('3. Batch, fork, and session state');
  const forked = await agent.fork();
  const [forkA, forkB] = await Promise.all([
    forked.chat('Reply with exactly: FORK_A'),
    agent.chat('Reply with exactly: FORK_B'),
  ]);
  expectContains(forkA, 'FORK_A');
  expectContains(forkB, 'FORK_B');
  mark('Agent.fork', `fork replies → ${forkA.slice(0, 10)} / ${forkB.slice(0, 10)}`);

  const batch = await agent.batch(['Reply with exactly: YES', 'Reply with exactly: NO']);
  if (batch.length !== 2) throw new Error(`batch returned ${batch.length} items`);
  expectContains(batch[0], 'YES');
  expectContains(batch[1], 'NO');
  mark('Agent.batch', `batch → ${batch.map(v => String(v).trim()).join(', ')}`);

  const snapshot = await agent.sessionSnapshot();
  if (!snapshot || typeof snapshot !== 'object') throw new Error('sessionSnapshot invalid');
  mark('Agent.sessionSnapshot', `sessionSnapshot → ${snapshot.messageCount ?? snapshot.message_count ?? 'ok'} messages`);

  const exported = await agent.export();
  if (!exported || !Array.isArray(exported.messages)) throw new Error('export invalid');
  mark('Agent.export', `export → ${exported.messages.length} messages`);

  section('4. Hot-swap, reset, and metadata');
  const modelBefore = await agent.model;
  if (!modelBefore) throw new Error('model getter empty');
  mark('Agent.model', `model getter → ${modelBefore}`);

  await agent.setModel(MODEL);
  mark('Agent.setModel', `setModel → ${MODEL}`);

  const toolNames = await agent.toolNames();
  if (!Array.isArray(toolNames)) throw new Error('toolNames returned a non-array value');
  mark('Agent.toolNames', `toolNames → ${toolNames.length} tools`);

  const toolsetSummary = await agent.toolsetSummary();
  if (!Array.isArray(toolsetSummary)) throw new Error('toolsetSummary returned a non-array value');
  mark('Agent.toolsetSummary', `toolsetSummary → ${toolsetSummary.length} groups`);

  const sid = await agent.sessionId;
  if (!sid) throw new Error('sessionId empty');
  ok(`sessionId → ${sid.slice(0, 12)}…`);

  await agent.newSession();
  const resetHistory = await agent.getHistory();
  if (resetHistory.length !== 0) throw new Error('newSession did not clear history');
  mark('Agent.newSession', 'newSession cleared history');

  section('5. Session store and memory');
  const hits = agent.searchSessions('PONG', 5);
  mark('Agent.searchSessions', `searchSessions → ${hits.length} hits`);

  const sessions = agent.listSessions(5);
  mark('Agent.listSessions', `listSessions → ${sessions.length} sessions`);

  const sessionStore = new Session();
  const standaloneMessages = sessionStore.getMessages(sid);
  if (!Array.isArray(standaloneMessages)) throw new Error('getMessages failed');
  mark('Session.getMessages', `getMessages → ${standaloneMessages.length} messages`);

  sessionStore.renameSession(sid, 'Node SDK E2E');
  mark('Session.renameSession', 'renameSession applied');

  const pruned = sessionStore.pruneSessions(36500);
  mark('Session.pruneSessions', `pruneSessions → ${pruned} deleted`);

  const stats = sessionStore.stats();
  mark('Session.stats', `session stats → ${stats.totalSessions} sessions / ${stats.totalMessages} messages`);

  const mem = agent.memory;
  const token = `node-sdk-${Date.now()}`;
  await mem.write('memory', token);
  const memContent = await mem.read('memory');
  if (!memContent.includes(token)) throw new Error('memory write/read failed');
  const entries = await mem.entries('memory');
  if (!Array.isArray(entries)) throw new Error('memory entries failed');
  const removed = await mem.remove('memory', token);
  if (!removed) throw new Error('memory remove failed');
  mark('MemoryManager.readWrite', `memory round-trip → ${entries.length} entries`);

  await agent.compress();
  mark('Agent.compress', 'compress completed');

  const pct = (covered.size / CORE_FEATURES.length) * 100;
  console.log(`\nCore API coverage: ${covered.size}/${CORE_FEATURES.length} (${pct.toFixed(1)}%)`);
  if (pct < 80) {
    throw new Error(`Coverage below target: ${pct.toFixed(1)}%`);
  }

  console.log('\n═══════════════════════════════════════');
  console.log('E2E smoke test PASSED ✓');
  console.log('Node.js SDK coverage target PASSED ✓');
}

main().catch(err => {
  console.error('E2E FAILED:', err);
  process.exit(1);
});
