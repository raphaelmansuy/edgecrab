# EdgeCrab Rust SDK Examples

## WHY

Welcome — this folder is here so you can go from **"I want to try the SDK"** to **"I have a working Rust agent"** without reading half the codebase first.

These examples are meant to feel practical, copyable, and fun to explore.

Use them when you want to:

- get a first success in a few minutes
- understand which pattern fits your app
- learn by running real code instead of reading abstract API docs
- validate everything locally with Ollama before you commit to a provider

## WHAT

### Start here if you are new

- Want the quickest first win? Start with `basic_usage.rs`
- Want something business-oriented right away? Run `business_case_showcase.rs`
- Want a more realistic app flow? Try `full_conversation.rs`
- Want proof that the SDK is solid? Run `e2e_smoke.rs`

### Prefer a guided path?

- [getting-started/README.md](getting-started/README.md) — first success in a few minutes
- [business/README.md](business/README.md) — demoable support, sales, and operations scenarios
- [e2e/README.md](e2e/README.md) — proof-oriented validation and what the smoke run actually covers

### Example map

| Example | Best for | What you’ll learn |
| --- | --- | --- |
| `basic_usage.rs` | First run | The smallest useful agent |
| `business_case_showcase.rs` | Business demos | Support, sales, and executive-style workflows |
| `full_conversation.rs` | Real apps | Response metadata, usage, and cost |
| `batch_processing.rs` | Throughput | Parallel prompt execution |
| `config_profiles.rs` | Reuse | Profile-aware config and defaults |
| `memory_usage.rs` | Persistence | Memory read and write patterns |
| `model_catalog.rs` | Model choice | Provider, model, and pricing lookup |
| `session_management.rs` | Continuity | Session listing, reuse, and export |
| `session_aware_support.rs` | Support flows | Context-aware assistant patterns |
| `multi_agent.rs` | Orchestration | Coordinating multiple agents |
| `parallel_research.rs` | Research flows | Concurrent investigation patterns |
| `cost_aware_review.rs` | Budget-sensitive work | Choosing the right model for the task |
| `safe_sql_agent.rs` | Safe tools | Guarded tool patterns |
| `e2e_smoke.rs` | Proof | Core SDK verification against local Ollama |

## HOW

### The big picture

```text
You ask a question
        |
        v
  SdkAgent::builder(...)
        |
        +--> model choice
        +--> optional tools
        +--> optional memory/session state
        |
        v
   EdgeCrab runtime does the work
        |
        v
  You get a useful answer back
```

### Prerequisites

1. Start Ollama locally.
2. Pull the local model used by the examples:

```bash
ollama pull gemma4:latest
```

### Try your first example

From the repository root:

```bash
cargo run --example basic_usage -p edgecrab-sdk
```

Then move on to a richer example:

```bash
cargo run --example full_conversation -p edgecrab-sdk
```

### Want the proof run?

The E2E example is intentionally proof-oriented:

```bash
cd crates/edgecrab-sdk
make e2e
```

### A simple learning path

1. Run `basic_usage.rs`
2. Read `full_conversation.rs`
3. Explore `memory_usage.rs` or `session_management.rs`
4. Finish with `e2e_smoke.rs` for confidence
