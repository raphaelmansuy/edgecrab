---
title: Node.js SDK (Native)
description: Complete guide to the EdgeCrab Node.js native SDK — Agent class, streaming, tool definitions, session management, and TypeScript types.
sidebar:
  order: 4
---

The native Node.js SDK (`edgecrab`) embeds the full EdgeCrab agent runtime
directly in your Node.js process via [napi-rs](https://napi.rs/).
No server required — every tool, model router, and session feature works
out of the box.

> For the HTTP-client SDK (`edgecrab-sdk`) that connects to a running
> EdgeCrab server, see the [Node.js HTTP SDK](/integrations/node-sdk/) page.

## Installation

```bash
npm install edgecrab
# or
pnpm add edgecrab
# or
yarn add edgecrab
```

Prebuilt binaries are shipped for:

| Platform         | Architecture |
| ---------------- | ------------ |
| macOS            | x64, arm64   |
| Linux (glibc)   | x64, arm64   |
| Linux (musl)    | x64, arm64   |
| Windows (MSVC)  | x64          |

## Quick Start

```typescript
import { Agent } from 'edgecrab';

const agent = new Agent({ model: 'openai/gpt-4o' });

const reply = await agent.chat('Explain Rust ownership in 3 sentences');
console.log(reply);
```

## Creating an Agent

### Minimal

```typescript
import { Agent } from 'edgecrab';

const agent = new Agent(); // uses your configured default model
```

### With Options

```typescript
const agent = new Agent({
  model: 'openai/gpt-4o',
  maxIterations: 50,
  temperature: 0.7,
  streaming: true,
  instructions: 'You are a Rust expert.',
  toolsets: ['file', 'web'],
  disabledTools: ['shell_exec'],
  quietMode: true,
});
```

### Resume a Session

```typescript
const agent = new Agent({
  model: 'copilot/gpt-5-mini',
  sessionId: 'abc123-def456',
});
```

## Chatting

### Simple Chat

```typescript
const reply = await agent.chat('What is the capital of France?');
console.log(reply); // "The capital of France is Paris."
```

### Chat in a Working Directory

```typescript
const reply = await agent.chatInCwd(
  'Summarize the README',
  '/path/to/project'
);
```

### Full Conversation Result

```typescript
const result = await agent.run('Fix the failing tests');
console.log(result.response);      // final assistant text
console.log(result.apiCalls);      // number of LLM calls
console.log(result.totalCost);     // USD cost
console.log(result.inputTokens);   // total input tokens
console.log(result.outputTokens);  // total output tokens
console.log(result.interrupted);   // was the run interrupted?
console.log(result.budgetExhausted); // did we hit max iterations?
```

### Conversation with System Prompt & History

```typescript
const result = await agent.runConversation(
  'Now add authentication',
  'You are a senior backend engineer.',
  ['Explain the current architecture', 'What frameworks are used?']
);
```

## Streaming

```typescript
const events = await agent.stream('Write a web server in Rust');
for (const event of events) {
  switch (event.eventType) {
    case 'token':
      process.stdout.write(event.data);
      break;
    case 'tool_exec':
      console.log(`\n🔧 ${event.data}`);
      break;
    case 'tool_result':
      console.log(`✅ ${event.data}`);
      break;
    case 'error':
      console.error(`❌ ${event.data}`);
      break;
  }
}
```

## Interrupting

```typescript
// In another context (e.g. signal handler, timeout)
agent.interrupt();

console.log(agent.isCancelled); // true
```

## Session Management

### New Session

```typescript
await agent.newSession();
```

### Session Info

```typescript
const sid = await agent.sessionId;   // current session ID
const model = await agent.model;     // current model name
```

### Conversation History

```typescript
const history = await agent.getHistory();
// [{ role: "User", content: "Hello" }, { role: "Assistant", content: "Hi!" }]
```

### Export Session

```typescript
const snapshot = await agent.export();
console.log(snapshot.session_id);
console.log(snapshot.message_count);
console.log(snapshot.messages);
```

### List & Search Sessions

```typescript
const sessions = agent.listSessions(10);
sessions.forEach(s =>
  console.log(`${s.sessionId} — ${s.title ?? 'untitled'} (${s.messageCount} msgs)`)
);

const hits = agent.searchSessions('authentication', 5);
hits.forEach(h =>
  console.log(`[${h.score.toFixed(2)}] ${h.snippet}`)
);
```

## Forking

Create an isolated copy of the agent for parallel work:

```typescript
const fork = await agent.fork();
const [a, b] = await Promise.all([
  fork.chat('Approach A: use JWT'),
  agent.chat('Approach B: use sessions'),
]);
```

## Tool Introspection

```typescript
const tools = await agent.toolNames();
console.log(tools); // ["read_file", "write_file", "shell_exec", ...]

const summary = await agent.toolsetSummary();
// [["file", 5], ["web", 3], ["shell", 2]]
```

## Custom Tools

Define JavaScript/TypeScript tools for the agent:

```typescript
import { Tool } from 'edgecrab';

const weatherTool = Tool.create({
  name: 'get_weather',
  description: 'Get current weather for a city',
  parameters: {
    type: 'object',
    properties: {
      city: { type: 'string', description: 'City name' },
    },
    required: ['city'],
  },
  handler: async ({ city }) => {
    const res = await fetch(`https://wttr.in/${city}?format=j1`);
    return res.json();
  },
});

console.log(weatherTool.toSchema()); // OpenAI-compatible function schema
```

## Message & Role Helpers

```typescript
import { Message, Role } from 'edgecrab';

const msg = Message.user('Hello!');
console.log(msg.role);    // "user"
console.log(msg.content); // "Hello!"

const sys = Message.system('You are helpful.');
const ast = Message.assistant('Sure, I can help.');
```

## Model Catalog

```typescript
import { ModelCatalog } from 'edgecrab';

const catalog = new ModelCatalog();
console.log(catalog.providerIds());
// ["anthropic", "openai", "google", ...]

const models = catalog.modelsForProvider('openai');
// [["gpt-4o", "GPT-4o"], ...]

const ctx = catalog.contextWindow('openai', 'gpt-4o');
console.log(ctx); // 200000
```

## Utilities

```typescript
import { edgecrabHome, ensureEdgecrabHome } from 'edgecrab';

console.log(edgecrabHome());        // "/Users/you/.edgecrab"
console.log(ensureEdgecrabHome());   // creates dir if needed
```

## Configuration

```typescript
// Set streaming mode at runtime
await agent.setStreaming(true);

// Set reasoning effort (for models that support it)
await agent.setReasoningEffort('high');
```

## TypeScript Types

```typescript
import type {
  AgentOptions,
  ConversationResult,
  StreamEvent,
  SessionSummary,
  SessionSearchHit,
  ToolDefinition,
} from 'edgecrab';
```

## Two SDK Modes

| Feature          | Native (`edgecrab`)      | HTTP Client (`edgecrab-sdk`) |
| ---------------- | ----------------------- | --------------------------- |
| Runtime          | Embedded (napi-rs)      | HTTP to EdgeCrab server     |
| Performance      | Direct function calls   | Network round-trips         |
| Server required  | No                      | Yes                         |
| All tools        | Yes                     | Server-configured           |
| Session storage  | Local SQLite            | Server-side                 |
| Package size     | ~30 MB (native binary)  | ~50 KB                      |

Choose **native** for scripts, CLIs, and standalone apps.
Choose **HTTP client** when connecting to a shared EdgeCrab server.
