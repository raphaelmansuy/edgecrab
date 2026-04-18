# EdgeCrab Node.js SDK

## WHY

Use this SDK when you want native Node.js ergonomics with the Rust runtime underneath.

It is meant for teams that want:

- Promise-based APIs
- fast local verification with Ollama
- persistent sessions and memory
- safe migration from prototype to production workflow

## WHAT

Core capabilities:

- embedded Rust runtime via napi-rs
- `Agent`, `Config`, `Session`, and `ModelCatalog`
- chat, streaming, batching, export, and session search
- profile-aware configuration via `Config.loadProfile()`

### Architecture

```text
Node app
   |
   v
NAPI bindings
   |
   v
EdgeCrab Rust runtime
   |
   +--> sessions
   +--> memory
   +--> tools
   |
   v
LLM provider or local Ollama
```

## HOW

### Quick start

```bash
npm install
npm run build
```

```js
const { Agent, Config } = require('edgecrab')

const cfg = Config.loadProfile('work')
const agent = Agent.fromConfig(cfg)
agent.chat('Hello').then(console.log)
```

### Run examples

See the full examples guide in `examples/README.md`.

```bash
cd sdks/nodejs-native
make e2e
```

## SDK Tutorials

All tutorials ship with working Node.js examples in `examples/`.

| Tutorial | Example | Model |
| --- | --- | --- |
| [1. Cost-Aware Code Review](../../site/src/content/docs/tutorials/01-cost-aware-review.md) | `examples/cost_aware_review.mjs` | openai/gpt-5 + copilot/gpt-5-mini |
| [2. Parallel Research Pipeline](../../site/src/content/docs/tutorials/02-parallel-research.md) | `examples/parallel_research.mjs` | copilot/gpt-5-mini |
| [3. Multi-Agent Pipeline](../../site/src/content/docs/tutorials/03-multi-agent-pipeline.md) | `examples/multi_agent_pipeline.mjs` | openai/gpt-4o + copilot/gpt-5-mini |
| [4. Session-Aware Support Bot](../../site/src/content/docs/tutorials/04-session-aware-support.md) | `examples/session_aware_support.mjs` | copilot/gpt-5-mini |
| [5. Safe SQL Agent](../../site/src/content/docs/tutorials/05-custom-tool-safe-sql.md) | `examples/safe_sql_agent.mjs` | copilot/gpt-5-mini |
