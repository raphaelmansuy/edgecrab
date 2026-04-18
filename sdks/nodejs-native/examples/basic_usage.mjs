// EdgeCrab Node.js SDK — Basic usage examples

import { Agent, Tool, Message, Role } from "edgecrab";

// ── 1. Simple chat ──────────────────────────────────────────────────

async function simpleChat() {
  const agent = new Agent({ model: "copilot/gpt-5-mini" });
  const reply = await agent.chat("What is the capital of France?");
  console.log("Reply:", reply);
}

// ── 2. Streaming ────────────────────────────────────────────────────

async function streamingExample() {
  const agent = new Agent({ model: "copilot/gpt-5-mini" });
  const events = await agent.stream("Write a haiku about Rust programming");
  for (const event of events) {
    if (event.eventType === "token") {
      process.stdout.write(event.data);
    }
  }
  console.log(); // newline
}

// ── 3. Full conversation result ─────────────────────────────────────

async function conversationResult() {
  const agent = new Agent({
    model: "copilot/gpt-5-mini",
    maxIterations: 10,
  });
  const result = await agent.run("What are the SOLID principles?");
  console.log(`Response: ${result.response.slice(0, 200)}...`);
  console.log(`Model: ${result.model}`);
  console.log(`API calls: ${result.apiCalls}`);
  console.log(`Input tokens: ${result.inputTokens}`);
  console.log(`Output tokens: ${result.outputTokens}`);
  console.log(`Cost: $${result.totalCost.toFixed(4)}`);
}

// ── 4. Memory ───────────────────────────────────────────────────────

async function memoryExample() {
  const agent = new Agent({ model: "copilot/gpt-5-mini" });
  const mem = agent.memory;

  // Write
  await mem.write("memory", "User prefers TypeScript over JavaScript");

  // Read
  const content = await mem.read("memory");
  console.log("Memory:", content);

  // Entries
  const entries = await mem.entries("memory");
  console.log("Entries:", entries);

  // Remove
  const removed = await mem.remove("memory", "TypeScript");
  console.log("Removed:", removed);
}

// ── 5. Custom tools ─────────────────────────────────────────────────

async function customToolExample() {
  const weatherTool = Tool.create({
    name: "get_weather",
    description: "Get the current weather for a city",
    parameters: {
      type: "object",
      properties: {
        city: { type: "string", description: "City name" },
      },
      required: ["city"],
    },
    handler: async ({ city }) => {
      // In production, call a real weather API
      return JSON.stringify({ city, temp: 22, condition: "sunny" });
    },
  });

  console.log("Tool created:", weatherTool.name);
}

// ── 6. Fork for parallel tasks ──────────────────────────────────────

async function forkExample() {
  const agent = new Agent({ model: "copilot/gpt-5-mini" });
  await agent.chat("Remember: I'm building a REST API in Node.js.");

  const forked = await agent.fork();
  const reply = await forked.chat("What middleware should I use?");
  console.log("Forked reply:", reply);
}

// ── Run all examples ────────────────────────────────────────────────

async function main() {
  console.log("=== Simple Chat ===");
  await simpleChat();

  console.log("\n=== Streaming ===");
  await streamingExample();

  console.log("\n=== Conversation Result ===");
  await conversationResult();

  console.log("\n=== Memory ===");
  await memoryExample();

  console.log("\n=== Custom Tool ===");
  await customToolExample();

  console.log("\n=== Fork ===");
  await forkExample();
}

main().catch(console.error);
