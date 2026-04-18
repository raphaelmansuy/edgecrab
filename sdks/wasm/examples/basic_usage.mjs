/**
 * EdgeCrab WASM SDK — Basic Usage
 *
 * Shows the essential patterns: chat, streaming, custom tools, memory,
 * and forking — all running inside a WASM runtime via Node.js.
 *
 * Works with any OpenAI-compatible endpoint (local Ollama, OpenAI, etc.)
 *
 * Run:
 *   cd sdks/wasm
 *   wasm-pack build --target nodejs --out-dir pkg-node
 *   node examples/basic_usage.mjs
 */
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const { Agent, Tool, MemoryManager } = require('../pkg-node/edgecrab_wasm.js');

// ── Configuration ───────────────────────────────────────────────────
// Point at your local Ollama, or swap for any OpenAI-compatible endpoint
const MODEL = process.env.MODEL || 'ollama/gemma4:latest';
const BASE_URL = process.env.OPENAI_BASE_URL || process.env.ANTHROPIC_BASE_URL || 'http://localhost:11434/v1';
const API_KEY = process.env.OPENAI_API_KEY || process.env.ANTHROPIC_AUTH_TOKEN || 'ollama';

// ── 1. Simple chat ──────────────────────────────────────────────────

async function simpleChat() {
  console.log('── 1. Simple chat ──');
  const agent = new Agent(MODEL, {
    baseUrl: BASE_URL,
    apiKey: API_KEY,
    instructions: 'Be concise and helpful.',
  });

  const reply = await agent.chat('What is the capital of France?');
  console.log(`  Reply: ${reply}\n`);
}

// ── 2. Streaming tokens ─────────────────────────────────────────────

async function streamingExample() {
  console.log('── 2. Streaming ──');
  const agent = new Agent(MODEL, {
    baseUrl: BASE_URL,
    apiKey: API_KEY,
    streaming: true,
  });

  const events = await agent.stream('Write a haiku about WebAssembly');
  for (const event of events) {
    if (event.eventType === 'token') {
      process.stdout.write(event.data);
    }
  }
  console.log('\n');
}

// ── 3. Custom tool (weather lookup) ─────────────────────────────────

async function customToolExample() {
  console.log('── 3. Custom tool ──');
  const agent = new Agent(MODEL, {
    baseUrl: BASE_URL,
    apiKey: API_KEY,
    maxIterations: 5,
    instructions: 'Use the get_weather tool when asked about weather.',
  });

  const weatherTool = Tool.create({
    name: 'get_weather',
    description: 'Get the current weather for a city',
    parameters: {
      type: 'object',
      properties: {
        city: { type: 'string', description: 'City name' },
      },
      required: ['city'],
    },
    handler: async ({ city }) => {
      // In production, call a real weather API
      return JSON.stringify({ city, temp_c: 22, condition: 'sunny' });
    },
  });

  agent.addTool(weatherTool);
  console.log(`  Registered tool: get_weather (${agent.toolCount()} total)`);

  const reply = await agent.chat("What's the weather in Paris?");
  console.log(`  Reply: ${reply}\n`);
}

// ── 4. Memory: read and write ───────────────────────────────────────

async function memoryExample() {
  console.log('── 4. Memory ──');
  const agent = new Agent(MODEL, {
    baseUrl: BASE_URL,
    apiKey: API_KEY,
  });

  const mem = agent.memory;

  // Write a preference
  mem.write('memory', 'User prefers TypeScript over JavaScript');

  // Read it back
  const content = mem.read('memory');
  console.log(`  Stored: ${content}`);

  // List entries
  const entries = mem.entries('memory');
  console.log(`  Entries: ${entries.length}`);

  // Serialize → restore
  const json = mem.toJSON();
  const restored = MemoryManager.fromJSON(json);
  console.log(`  Restored memory has ${restored.entries('memory').length} entries\n`);
}

// ── 5. Fork for parallel exploration ────────────────────────────────

async function forkExample() {
  console.log('── 5. Fork ──');
  const agent = new Agent(MODEL, {
    baseUrl: BASE_URL,
    apiKey: API_KEY,
    instructions: 'Be concise.',
  });

  await agent.chat('Remember: I am building a web app with Rust + WASM.');

  const fork = agent.fork();
  const reply = await fork.chat('What bundler should I use?');
  console.log(`  Forked reply: ${reply}`);
  console.log(`  Original history: ${agent.getHistory().length} messages`);
  console.log(`  Fork history:     ${fork.getHistory().length} messages\n`);
}

// ── Run all examples ────────────────────────────────────────────────

async function main() {
  console.log('EdgeCrab WASM SDK — Basic Usage Examples');
  console.log(`Model: ${MODEL}`);
  console.log('═══════════════════════════════════════\n');

  await simpleChat();
  await streamingExample();
  await customToolExample();
  await memoryExample();
  await forkExample();

  console.log('═══════════════════════════════════════');
  console.log('All examples completed successfully ✓');
}

main().catch((err) => {
  console.error('Example failed:', err);
  process.exit(1);
});
