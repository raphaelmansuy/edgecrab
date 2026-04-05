---
title: Node.js SDK
description: Integrate EdgeCrab into Node.js and TypeScript applications with the edgecrab-sdk. Agent abstraction, streaming, tool customization, and CLI.
sidebar:
  order: 3
---

The EdgeCrab Node.js SDK (`edgecrab-sdk`) is a TypeScript-first package with streaming support, tool customization, and a built-in CLI. Compatible with Node.js 18+.

---

## Installation

```bash
npm install edgecrab-sdk
# or
pnpm add edgecrab-sdk
# or
yarn add edgecrab-sdk
```

---

## Quick Start

```typescript
import { Agent } from 'edgecrab-sdk';

const agent = new Agent({
  model: 'anthropic/claude-sonnet-4-20250514',
});

const reply = await agent.chat('Explain Rust ownership in 3 sentences');
console.log(reply);
```

---

## Agent Options

```typescript
import { Agent } from 'edgecrab-sdk';

const agent = new Agent({
  model: 'openai/gpt-4o',                    // provider/model string
  systemPrompt: 'You are a Rust expert.',    // optional system prompt
  apiKey: process.env.OPENAI_API_KEY,        // optional; falls back to env var
  maxLoopDepth: 20,                          // max ReAct iterations (default: 20)
  toolsets: ['file', 'web'],                 // enable specific toolsets
  sessionName: 'my-project',                // named session (persisted)
  baseUrl: undefined,                        // custom OpenAI-compatible endpoint
});
```

---

## Multi-Turn Conversations

```typescript
import { Agent } from 'edgecrab-sdk';

const agent = new Agent({ model: 'openai/gpt-4o' });

await agent.chat('Explain the main function in src/main.rs');
await agent.chat('Now add robust error handling to it');
// Each call sees the full conversation history
```

---

## Streaming

```typescript
import { Agent } from 'edgecrab-sdk';

const agent = new Agent({ model: 'anthropic/claude-opus-4-5' });

for await (const chunk of agent.stream('Write a Rust async HTTP client')) {
  process.stdout.write(chunk);
}
console.log();
```

---

## Tool Events

```typescript
import { Agent, ToolCallEvent, ToolResultEvent, TextChunkEvent } from 'edgecrab-sdk';

const agent = new Agent({ model: 'openai/gpt-4o' });

for await (const event of agent.streamEvents('Fix the failing tests in src/')) {
  if (event instanceof ToolCallEvent) {
    console.log(`Tool call: ${event.name}(${JSON.stringify(event.args)})`);
  } else if (event instanceof ToolResultEvent) {
    console.log(`Result: ${event.result.slice(0, 100)}`);
  } else if (event instanceof TextChunkEvent) {
    process.stdout.write(event.text);
  }
}
```

---

## Custom Tools

Register JavaScript/TypeScript functions as tools:

```typescript
import { Agent, defineTool } from 'edgecrab-sdk';
import { z } from 'zod';

const getCurrentTime = defineTool({
  name: 'get_current_time',
  description: 'Get the current UTC time',
  parameters: z.object({}),
  execute: async () => new Date().toISOString(),
});

const readFile = defineTool({
  name: 'read_project_config',
  description: 'Read the project config file',
  parameters: z.object({ env: z.enum(['dev', 'staging', 'prod']) }),
  execute: async ({ env }) => {
    const fs = await import('fs/promises');
    return fs.readFile(`./config.${env}.json`, 'utf-8');
  },
});

const agent = new Agent({
  model: 'openai/gpt-4o',
  extraTools: [getCurrentTime, readFile],
});
```

---

## Session Persistence

```typescript
import { Agent } from 'edgecrab-sdk';

// Named session — history is persisted in ~/.edgecrab/state.db
const agent = new Agent({
  model: 'openai/gpt-4o',
  sessionName: 'my-project',
});

await agent.chat('Explain the architecture');

// In a later run — continues where it left off
const agent2 = new Agent({
  model: 'openai/gpt-4o',
  sessionName: 'my-project',
});
await agent2.chat('Now add authentication'); // Has full previous context
```

---

## Built-in CLI

Use EdgeCrab via npx without a full installation:

```bash
# Interactive chat
npx edgecrab-sdk chat

# Single prompt
npx edgecrab-sdk chat "Summarize the last 10 git commits"

# Specific model
npx edgecrab-sdk chat --model anthropic/claude-opus-4-5 "Explain this codebase"

# List available models
npx edgecrab-sdk models

# Check server health
npx edgecrab-sdk health
```

---

## TypeScript Types

```typescript
import type {
  Agent,
  AgentOptions,
  ChatMessage,
  StreamEvent,
  ToolCallEvent,
  ToolResultEvent,
  TextChunkEvent,
  EdgeCrabError,
  ProviderError,
  ToolError,
} from 'edgecrab-sdk';
```

All types are exported from the main package and include full JSDoc documentation.

---

## Error Handling

```typescript
import { Agent, EdgeCrabError, ProviderError, ToolError } from 'edgecrab-sdk';

const agent = new Agent({ model: 'openai/gpt-4o' });

try {
  const reply = await agent.chat('Read /etc/passwd');
  console.log(reply);
} catch (e) {
  if (e instanceof ToolError) {
    console.error('Tool failed (likely security):', e.message);
  } else if (e instanceof ProviderError) {
    console.error('LLM provider error:', e.message, 'status:', e.status);
  } else if (e instanceof EdgeCrabError) {
    console.error('EdgeCrab error:', e.message);
  } else {
    throw e;
  }
}
```

---

## CommonJS Support

The SDK ships dual CJS/ESM builds:

```javascript
// CommonJS
const { Agent } = require('edgecrab-sdk');
const agent = new Agent({ model: 'openai/gpt-4o' });

agent.chat('Hello').then(console.log);
```

---

## Full SDK Docs

See [sdks/node/README.md](https://github.com/raphaelmansuy/edgecrab/blob/main/sdks/node/README.md) for the complete TypeScript API reference.

---

## Pro Tips

- **Use `streamEvents` over `stream` when you need tool visibility**: It surfaces `ToolCallEvent` and `ToolResultEvent` for logging or UI rendering.
- **Named sessions persist across Node.js restarts**: Use `sessionName` to resume a long-running automation pipeline across multiple script invocations.
- **Use `z.object({})` for no-parameter tools**: The Zod schema is required even for tools with no arguments.
- **`baseUrl` supports any OpenAI-compatible server**: Point at a self-hosted gateway, LM Studio, or Ollama without changing any other SDK config.
- **The SDK is ESM-first but ships CJS too**: If you're in a CommonJS project, use `require('edgecrab-sdk')`. For ESM, use `import { Agent } from 'edgecrab-sdk'`.

---

## FAQ

**Does the SDK require a running EdgeCrab server?**
No. The SDK calls the LLM provider directly, same as the CLI. No local server needed.

**Can I use the SDK in a browser?**
No. The SDK relies on Node.js APIs (filesystem, subprocess) that are not available in browsers. For browser use, proxy through an EdgeCrab gateway.

**Is Deno supported?**
Not officially. The package ships CJS and ESM for Node.js. Deno's Node compatibility layer may work but is untested.

**Does the SDK respect `~/.edgecrab/config.yaml`?**
Yes. Config is loaded from the standard path unless overridden by constructor options. Set `EDGECRAB_HOME` to change the config directory.

**Can I use this from a Next.js API route or a serverless function?**
Yes, but disable file system tools: pass `toolsets: []` or `toolsets: ['web']` to prevent filesystem access in serverless environments where the filesystem is ephemeral.

---

## See Also

- [Python SDK](/integrations/python-sdk/) — async-first Python equivalent
- [ACP / VS Code](/integrations/acp/) — use EdgeCrab as a VS Code Copilot agent
- [Self-Hosting](/guides/self-hosting/) — run a shared gateway for your team
