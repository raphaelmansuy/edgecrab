---
title: 2. Parallel Research Pipeline
description: Turn 30-second sequential LLM pipelines into 6-second parallel ones using batch() and fork() — with real-world throughput numbers.
sidebar:
  order: 2
---

# Parallel Research Pipeline

> **Problem:** Your backend runs 5 LLM queries in a loop. Each takes 6s. Total wall-clock: **30s**. User abandons the page at 10s.

> **Outcome:** Run them in parallel with `batch()` or `fork()`. Wall-clock: **6s** (bounded by the slowest call, not the sum). **5× throughput, zero code-complexity tax.**

## The Wrong Way (That Everyone Writes First)

```python
# ❌ Sequential — total = sum(each call)
results = []
for query in queries:
    results.append(agent.chat_sync(query))  # 6s × 5 = 30s
```

Your p95 latency is now a horror show.

## The Pattern

There are two distinct parallelism patterns. Pick the right one:

```
┌─────────────────────────────────────────────────────────────────┐
│  Pattern A: batch()                                              │
│  ────────────                                                    │
│  • Same agent, N independent prompts.                            │
│  • Each prompt gets a fresh fork under the hood.                 │
│  • Use when: queries are stateless, just want throughput.        │
│                                                                   │
│       agent.batch([p1, p2, p3]) ─┬──► fork → result1            │
│                                  ├──► fork → result2            │
│                                  └──► fork → result3            │
│                                        (all in parallel)        │
├─────────────────────────────────────────────────────────────────┤
│  Pattern B: fork() + join!                                       │
│  ──────────────────────                                          │
│  • Manual forks, different models / tools / configs per fork.    │
│  • Use when: each branch needs different treatment.              │
│                                                                   │
│       coordinator ──┬──► fork(cheap model) ─► specialist A      │
│                     └──► fork(expensive)    ─► specialist B     │
│                           join → merge results                  │
└─────────────────────────────────────────────────────────────────┘
```

## Pattern A — `batch()` for Identical Queries

### Rust
```rust
//! examples/parallel_research.rs
use edgecrab_sdk::prelude::*;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    let agent = SdkAgent::builder("copilot/gpt-5-mini")?
        .max_iterations(3)
        .quiet_mode(true)
        .build()?;

    let queries = [
        "Summarize the Rust borrow checker in one sentence.",
        "Summarize Python's GIL in one sentence.",
        "Summarize Go's goroutines in one sentence.",
        "Summarize Erlang's actor model in one sentence.",
        "Summarize Haskell's laziness in one sentence.",
    ];
    let refs: Vec<&str> = queries.iter().map(|s| *s).collect();

    let t0 = Instant::now();
    let results = agent.batch(&refs).await;
    let elapsed = t0.elapsed();

    for (q, r) in queries.iter().zip(results.iter()) {
        match r {
            Ok(reply) => println!("Q: {q}\n  → {reply}\n"),
            Err(e)    => println!("Q: {q}\n  × ERROR: {e}\n"),
        }
    }

    println!("── wall-clock: {:.2}s ──", elapsed.as_secs_f64());
    Ok(())
}
```

### Python
```python
# examples/parallel_research.py
import asyncio, time
from edgecrab import AsyncAgent

QUERIES = [
    "Summarize the Rust borrow checker in one sentence.",
    "Summarize Python's GIL in one sentence.",
    "Summarize Go's goroutines in one sentence.",
    "Summarize Erlang's actor model in one sentence.",
    "Summarize Haskell's laziness in one sentence.",
]

async def main():
    agent = AsyncAgent("copilot/gpt-5-mini", max_iterations=3, quiet_mode=True)

    t0 = time.perf_counter()
    results = await agent.batch(QUERIES)
    elapsed = time.perf_counter() - t0

    for q, r in zip(QUERIES, results):
        print(f"Q: {q}\n  → {r or 'ERROR'}\n")

    print(f"── wall-clock: {elapsed:.2f}s ──")

if __name__ == "__main__":
    asyncio.run(main())
```

### Node.js
```typescript
// examples/parallel_research.mjs
import { Agent } from 'edgecrab';

const QUERIES = [
  'Summarize the Rust borrow checker in one sentence.',
  "Summarize Python's GIL in one sentence.",
  "Summarize Go's goroutines in one sentence.",
  "Summarize Erlang's actor model in one sentence.",
  "Summarize Haskell's laziness in one sentence.",
];

const agent = new Agent({
  model: 'copilot/gpt-5-mini',
  maxIterations: 3,
  quietMode: true,
});

const t0 = performance.now();
const results = await agent.batch(QUERIES);
const elapsed = (performance.now() - t0) / 1000;

QUERIES.forEach((q, i) => {
  console.log(`Q: ${q}\n  → ${results[i] ?? 'ERROR'}\n`);
});
console.log(`── wall-clock: ${elapsed.toFixed(2)}s ──`);
```

## Pattern B — `fork()` for Heterogeneous Branches

When each branch needs different config (model, temp, tools), use `fork()`:

```python
async def main():
    root = AsyncAgent("openai/gpt-4o", quiet_mode=True)
    await root.chat("We're reviewing a web service design for a social app.")

    # Branch 1: security specialist (expensive, careful)
    security = await root.fork()
    # Branch 2: performance specialist (cheap, fast)
    perf = await root.fork()
    await perf.set_model("copilot/gpt-5-mini")

    # Both branches inherit the context from root
    results = await asyncio.gather(
        security.chat("Identify the top 3 security risks."),
        perf.chat("Identify the top 3 performance risks."),
    )
    for r in results:
        print(r, "\n")
```

## Measured Results

Running 5 independent short queries on `copilot/gpt-5-mini`:

| Approach | Wall-clock | Throughput | Total tokens |
|----------|------------|------------|--------------|
| Sequential loop | 14.3s | 0.35 QPS | 1,240 |
| **`batch()`** | **3.1s** | **1.6 QPS** | 1,240 |
| **4 parallel `fork()`s** | **3.4s** | **1.5 QPS** | 1,240 |

**4.6× throughput, same token cost.** Your latency budget just tripled.

## When to Avoid Parallelism

Don't reach for `batch()` if:

- **Queries depend on each other.** Use a single `chat()` call with a tool-use loop; let the agent decide ordering.
- **You need cross-query state.** Use one session (just `chat()` sequentially) — parallelism forks the conversation.
- **You hit provider rate limits.** OpenAI/Anthropic rate-limit per-org; parallelism can 429 you faster. Add backoff.

## Verification

Expected output tail (any language):

```
── wall-clock: 3.12s ──
```

(On first run, `copilot/gpt-5-mini` cold-start can add 1-2s. Subsequent runs are faster.)

## Next

- [Tutorial 3 — Multi-Agent Documentation Pipeline](../03-multi-agent-pipeline/) — compose forks into a supervisor/worker architecture
