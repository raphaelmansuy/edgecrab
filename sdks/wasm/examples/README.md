# EdgeCrab WASM SDK Examples

## WHY

These examples show you how to use the EdgeCrab agent directly from JavaScript — through a compiled WASM package. Whether you are targeting a browser, an edge runtime, or Node.js, the same API applies.

Start with the `basic_usage.mjs` to get your first working chat, then look at the E2E smoke test when you want proof that every API surface works.

## WHAT

### Start here

| Example | Best for | What you'll see |
| --- | --- | --- |
| `basic_usage.mjs` | First win | Chat, streaming, tools, memory, and fork |
| `business_case_showcase.mjs` | Business workflows | Quick customer-support and executive-response patterns |
| `e2e_smoke.mjs` | Proof | Core WASM SDK verification against local Ollama |

### Prefer a guided path?

- [getting-started/README.md](getting-started/README.md) — first success with the WASM agent
- [business/README.md](business/README.md) — customer and operations demo flows
- [e2e/README.md](e2e/README.md) — proof coverage, commands, and expected outcomes

### API surface covered

```text
Your JS/TS code
      |
      v
  WASM Agent
      |
      +--> chat / stream
      +--> addTool (JS handler)
      +--> memory (read/write/entries)
      +--> fork / newSession / setModel
      |
      v
   Local Ollama or any OpenAI-compatible endpoint
```

## HOW

### Prerequisites

1. Install [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/).
2. Start Ollama locally and pull the example model:

```bash
ollama pull gemma4:latest
```

### Build the WASM package

```bash
cd sdks/wasm
wasm-pack build --target nodejs --out-dir pkg-node
```

### Run your first example

```bash
OPENAI_BASE_URL=http://localhost:11434/v1 OPENAI_API_KEY=ollama \
  node examples/basic_usage.mjs
```

### Run the proof-oriented E2E check

```bash
cd sdks/wasm
make e2e
```

Or manually:

```bash
wasm-pack build --target nodejs --out-dir pkg-node
OPENAI_BASE_URL=http://localhost:11434/v1 OPENAI_API_KEY=ollama \
  node examples/e2e_smoke.mjs
```

### Suggested learning path

1. `basic_usage.mjs` — get a chat reply, stream tokens, register a tool
2. `e2e_smoke.mjs` — see every API method exercised with assertions
