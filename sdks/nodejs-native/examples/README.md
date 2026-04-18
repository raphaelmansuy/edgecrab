# EdgeCrab Node.js Examples

## WHY

If you want to open a file, run one command, and immediately feel **"yes, I can build with this"**, this folder is for you.

These examples are written for real Node.js developers who want:

- a fast first success
- clear copy-paste patterns
- local Ollama testing before production rollout
- session, memory, and orchestration examples that feel useful

## WHAT

### Good first choices

- New to the SDK? Start with `basic_usage.mjs`
- Need something you can demo to an operator or buyer? Run `business_case_showcase.mjs`
- Building something more production-like? Open `session_aware_support.mjs`
- Want the confidence check? Run `e2e_smoke.mjs`

### Prefer a guided path?

- [getting-started/README.md](getting-started/README.md) — the fastest first Node.js success
- [business/README.md](business/README.md) — examples that feel like actual product workflows
- [e2e/README.md](e2e/README.md) — proof-style checks and expected validation output

### Example map

| Example | Best for | What you’ll see |
| --- | --- | --- |
| `basic_usage.mjs` | First win | Minimal agent construction and chat |
| `business_case_showcase.mjs` | Business workflows | Support triage, executive updates, and sales prep |
| `cost_aware_review.mjs` | Budget control | Cost-aware model selection |
| `parallel_research.mjs` | Speed | Parallel research and aggregation |
| `multi_agent_pipeline.mjs` | Team-style flows | Staged multi-agent orchestration |
| `session_aware_support.mjs` | Context-aware apps | Memory and session reuse |
| `safe_sql_agent.mjs` | Safer tool usage | Guarded tool-driven workflow patterns |
| `e2e_smoke.mjs` | Proof | Core Node SDK verification against local Ollama |

## HOW

### The happy path

```text
Your app
   |
   v
new Agent({...})
   |
   +--> chat when you want simplicity
   +--> run when you want metadata
   +--> batch / fork when you want speed
   |
   v
A useful answer from local Ollama or your provider
```

### Setup once

```bash
cd sdks/nodejs-native
npm install
ollama pull gemma4:latest
```

### First run in under a minute

```bash
node examples/basic_usage.mjs
```

### Then try a more realistic workflow

```bash
node examples/session_aware_support.mjs
node examples/parallel_research.mjs
```

### Want the proof-oriented check?

```bash
cd sdks/nodejs-native
make e2e
```

### Suggested learning path

1. `basic_usage.mjs`
2. `session_aware_support.mjs`
3. `multi_agent_pipeline.mjs`
4. `e2e_smoke.mjs`
