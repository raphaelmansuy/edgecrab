import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const wasmPkg = require('../pkg-node/edgecrab_wasm.js');
const { Agent, Tool, MemoryManager } = wasmPkg;

const MODEL = 'ollama/gemma4:latest';
const BASE_URL = process.env.OPENAI_BASE_URL || process.env.ANTHROPIC_BASE_URL || 'http://localhost:11434/v1';
const API_KEY = process.env.OPENAI_API_KEY || process.env.ANTHROPIC_AUTH_TOKEN || 'ollama';

const CORE_FEATURES = [
  'Agent.constructor',
  'Agent.model',
  'Agent.sessionId',
  'Agent.chat',
  'Agent.stream',
  'Agent.getHistory',
  'Agent.setModel',
  'Agent.setStreaming',
  'Tool.create',
  'Agent.addTool',
  'Agent.toolCount',
  'Agent.toolNames',
  'Agent.fork',
  'Agent.newSession',
  'MemoryManager.readWrite',
  'MemoryManager.entriesRemove',
  'MemoryManager.toFromJSON',
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
  console.log('EdgeCrab WASM SDK — E2E Smoke Test');
  console.log(`Model: ${MODEL}`);
  console.log('═══════════════════════════════════════');

  section('1. Constructor and metadata');
  const agent = new Agent(MODEL, {
    baseUrl: BASE_URL,
    apiKey: API_KEY,
    maxIterations: 4,
    streaming: false,
    instructions: 'Be terse and obey exactly.',
  });
  mark('Agent.constructor', 'agent constructed');
  mark('Agent.model', `model → ${agent.model}`);
  mark('Agent.sessionId', `sessionId → ${agent.sessionId.slice(0, 14)}…`);

  section('2. Local memory and tool surface');
  const mem = agent.memory;
  const token = `wasm-sdk-${Date.now()}`;
  mem.write('memory', token);
  const content = mem.read('memory');
  if (!content.includes(token)) throw new Error('memory write/read failed');
  mark('MemoryManager.readWrite', 'memory write/read completed');

  const entries = mem.entries('memory');
  if (!Array.isArray(entries) || entries.length === 0) throw new Error('memory entries failed');
  const removed = mem.remove('memory', token);
  if (!removed) throw new Error('memory remove failed');
  mark('MemoryManager.entriesRemove', `entries/remove → ${entries.length} entries`);

  const json = mem.toJSON();
  const restored = MemoryManager.fromJSON(json);
  if (typeof restored.read !== 'function') throw new Error('fromJSON restore failed');
  mark('MemoryManager.toFromJSON', 'memory JSON round-trip');

  const echoTool = Tool.create({
    name: 'echo',
    description: 'Echoes text',
    parameters: {
      type: 'object',
      properties: { message: { type: 'string' } },
      required: ['message'],
    },
    handler: async (args) => JSON.stringify({ echo: args.message }),
  });
  mark('Tool.create', 'custom tool factory created');

  agent.addTool(echoTool);
  mark('Agent.addTool', 'custom tool registered');
  mark('Agent.toolCount', `toolCount → ${agent.toolCount()}`);
  const toolNames = agent.toolNames();
  if (!Array.isArray(toolNames)) throw new Error('toolNames returned a non-array value');
  mark('Agent.toolNames', `toolNames → ${toolNames.length} tools`);

  section('3. Live Ollama conversation');
  const chat = await agent.chat('Reply with exactly: PONG');
  expectContains(chat, 'PONG');
  mark('Agent.chat', `chat → ${String(chat).slice(0, 60).replace(/\n/g, ' ')}`);

  agent.setStreaming(true);
  mark('Agent.setStreaming', 'setStreaming(true)');
  const events = await agent.stream('Reply with exactly: STREAM_OK');
  const streamed = events
    .filter((e) => e.eventType === 'token')
    .map((e) => e.data)
    .join('');
  expectContains(streamed, 'STREAM_OK');
  mark('Agent.stream', `stream → ${streamed.slice(0, 40).replace(/\n/g, ' ')}`);

  const history = agent.getHistory();
  if (!Array.isArray(history) || history.length === 0) throw new Error('history empty');
  mark('Agent.getHistory', `${history.length} messages in history`);

  section('4. State changes');
  const fork = agent.fork();
  const forkReply = await fork.chat('Reply with exactly: FORK_OK');
  expectContains(forkReply, 'FORK_OK');
  mark('Agent.fork', `fork → ${String(forkReply).slice(0, 40).replace(/\n/g, ' ')}`);

  agent.setModel(MODEL);
  mark('Agent.setModel', `setModel → ${agent.model}`);

  agent.newSession();
  const resetHistory = agent.getHistory();
  if (!Array.isArray(resetHistory) || resetHistory.length !== 0) throw new Error('newSession did not clear history');
  mark('Agent.newSession', 'newSession cleared history');

  const pct = (covered.size / CORE_FEATURES.length) * 100;
  console.log(`\nCore API coverage: ${covered.size}/${CORE_FEATURES.length} (${pct.toFixed(1)}%)`);
  if (pct < 80) {
    throw new Error(`Coverage below target: ${pct.toFixed(1)}%`);
  }

  console.log('\n═══════════════════════════════════════');
  console.log('E2E smoke test PASSED ✓');
  console.log('WASM SDK coverage target PASSED ✓');
}

main().catch((err) => {
  console.error('E2E FAILED:', err);
  process.exit(1);
});
