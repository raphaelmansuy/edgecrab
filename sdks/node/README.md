# edgecrab-sdk

[![npm](https://img.shields.io/npm/v/edgecrab-sdk.svg)](https://www.npmjs.com/package/edgecrab-sdk)
[![Node](https://img.shields.io/badge/node-18%2B-green.svg)](https://nodejs.org)

Node.js SDK for **EdgeCrab** — a Rust-native autonomous coding agent.

## Install

```bash
npm install edgecrab-sdk
```

## Quick Start

### Agent API (recommended)

```typescript
import { Agent } from 'edgecrab-sdk';

const agent = new Agent({
  model: 'anthropic/claude-sonnet-4-20250514',
  systemPrompt: 'You are a helpful coding assistant',
  apiKey: 'your-api-key', // optional
});

// Simple chat with conversation history
const reply = await agent.chat('Explain Rust ownership');
console.log(reply);

// Continue the conversation
const followUp = await agent.chat('Give me an example');
console.log(followUp);
```

### Full conversation run

```typescript
const result = await agent.run('Refactor this function for better error handling');
console.log(result.response);
console.log(`Turns: ${result.turnsUsed}, Tokens: ${result.usage.total_tokens}`);
```

### Streaming

```typescript
for await (const token of agent.stream('Write a haiku about Rust')) {
  process.stdout.write(token);
}
```

### Low-level client

```typescript
import { EdgeCrabClient } from 'edgecrab-sdk';

const client = new EdgeCrabClient({
  baseUrl: 'http://127.0.0.1:8642',
  apiKey: 'your-key',
});

const resp = await client.createCompletion(
  [{ role: 'user', content: 'Hello' }],
  { model: 'anthropic/claude-sonnet-4-20250514' },
);
console.log(resp.choices[0].message.content);
```

## CLI

```bash
edgecrab chat "What is the meaning of life?"
edgecrab chat --model gpt-4 --system "Be concise" "Explain monads"
edgecrab chat --stream "Tell me a story"
edgecrab models
edgecrab health
```

### Environment Variables

| Variable | Description |
|---|---|
| `EDGECRAB_BASE_URL` | API server URL (default: `http://127.0.0.1:8642`) |
| `EDGECRAB_API_KEY` | Bearer token for authentication |

## API Reference

### `Agent`

| Method | Description |
|---|---|
| `chat(message)` | Send a message, return reply string. Maintains history. |
| `run(message)` | Full conversation run — returns `AgentResult` |
| `stream(message)` | Async generator yielding tokens |
| `addMessage(role, content)` | Inject a message into history |
| `reset()` | Clear history and start a new session |
| `getMessages()` | Get conversation history |
| `getTurnCount()` | Number of completed turns |
| `getUsage()` | Accumulated token usage |
| `listModels()` | List available models |
| `health()` | Check server health |

### `EdgeCrabClient`

Lower-level HTTP client with `chat()`, `createCompletion()`, `streamCompletion()`, `listModels()`, `health()`.

## Links

- [GitHub](https://github.com/raphaelmansuy/edgecrab)
- [Python SDK](https://github.com/raphaelmansuy/edgecrab/tree/main/sdks/python)
- [EdgeCrab Documentation](https://github.com/raphaelmansuy/edgecrab/tree/main/docs)
