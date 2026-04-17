---
title: 3. Multi-Agent Documentation Pipeline
description: Build a supervisor/specialist pattern using fork() — one coordinator orchestrates three specialists with different models, in Rust, Python, and Node.js.
sidebar:
  order: 3
---

# Multi-Agent Documentation Pipeline

> **Problem:** You ask one agent to "generate API docs." It writes endpoints OR examples OR tests — never all three at high quality. Bigger context ≠ better output.

> **Outcome:** One **coordinator** agent sets shared context, then **forks three specialists**, each tuned for one task, runs them in parallel, and merges the results. Total wall-clock is the slowest specialist, not the sum.

## Architecture

```
               ┌────────────────────────────┐
               │   Coordinator              │
               │   openai/gpt-4o            │
               │   Sets shared context:     │
               │   "We're documenting a     │
               │    bookmarks REST API."    │
               └──────────┬─────────────────┘
                          │ fork() × 3
          ┌───────────────┼───────────────┐
          ▼               ▼               ▼
   ┌───────────┐   ┌─────────────┐   ┌────────────┐
   │ API       │   │  Example    │   │  Test      │
   │ Designer  │   │  Writer     │   │  Writer    │
   │           │   │             │   │            │
   │ gpt-4o    │   │ gpt-5-mini  │   │ gpt-4o     │
   │ (precise) │   │ (fast/cheap)│   │ (careful)  │
   └─────┬─────┘   └──────┬──────┘   └─────┬──────┘
         │                │                 │
         └────────────────┴─────────────────┘
                          │ gather()
                          ▼
                ┌───────────────────┐
                │   Coordinator     │
                │   synthesizes     │
                │   → executive     │
                │     summary       │
                └───────────────────┘
```

**Why fork?** Each specialist inherits the coordinator's conversation history — so it "knows" what the project is — but their reasoning doesn't pollute each other's context. The coordinator has the whole picture at merge time.

## Rust Implementation

```rust
//! examples/multi_agent.rs
use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    // ── Coordinator ─────────────────────────────────────────────────
    let coordinator = SdkAgent::builder("openai/gpt-4o")?
        .max_iterations(5)
        .quiet_mode(true)
        .instructions("You are a technical writing coordinator.")
        .build()?;

    coordinator
        .chat("We're documenting a REST API that manages bookmarks. \
               Resources: bookmarks, tags, users.")
        .await?;

    // ── Fork specialists ────────────────────────────────────────────
    let api_designer   = coordinator.fork().await?;
    let example_writer = coordinator.fork().await?;
    let test_writer    = coordinator.fork().await?;

    // Cost optimization: example generation doesn't need a flagship model.
    example_writer.set_model("copilot/gpt-5-mini").await.ok();

    // ── Run all three in parallel ───────────────────────────────────
    let (endpoints, examples, tests) = tokio::join!(
        api_designer.chat(
            "Design 5 REST endpoints. Output a markdown table with \
             Method | Path | Description."
        ),
        example_writer.chat(
            "Write 3 runnable curl examples: create, list, delete a bookmark."
        ),
        test_writer.chat(
            "Write 3 pytest test cases (happy + 404 + auth error)."
        ),
    );

    // ── Merge back in the coordinator ───────────────────────────────
    let combined = format!(
        "# API\n{}\n\n# Examples\n{}\n\n# Tests\n{}",
        endpoints?, examples?, tests?
    );

    let summary = coordinator
        .chat(&format!(
            "Given this documentation, write a one-paragraph executive \
             summary for a product manager:\n\n{combined}"
        ))
        .await?;

    println!("=== FINAL DOCS ===\n{combined}\n\n=== SUMMARY ===\n{summary}");
    Ok(())
}
```

## Python Implementation

```python
# examples/multi_agent_pipeline.py
import asyncio
from edgecrab import AsyncAgent

async def main():
    coordinator = AsyncAgent(
        "openai/gpt-4o",
        max_iterations=5,
        quiet_mode=True,
        instructions="You are a technical writing coordinator.",
    )

    await coordinator.chat(
        "We're documenting a REST API that manages bookmarks. "
        "Resources: bookmarks, tags, users."
    )

    api_designer   = await coordinator.fork()
    example_writer = await coordinator.fork()
    test_writer    = await coordinator.fork()

    # Cost optimization
    await example_writer.set_model("copilot/gpt-5-mini")

    # Run 3 specialists in parallel
    endpoints, examples, tests = await asyncio.gather(
        api_designer.chat(
            "Design 5 REST endpoints. Output a markdown table with "
            "Method | Path | Description."
        ),
        example_writer.chat(
            "Write 3 runnable curl examples: create, list, delete a bookmark."
        ),
        test_writer.chat(
            "Write 3 pytest test cases (happy + 404 + auth error)."
        ),
    )

    combined = f"# API\n{endpoints}\n\n# Examples\n{examples}\n\n# Tests\n{tests}"

    summary = await coordinator.chat(
        f"Given this documentation, write a one-paragraph executive summary "
        f"for a product manager:\n\n{combined}"
    )

    print(f"=== FINAL DOCS ===\n{combined}\n\n=== SUMMARY ===\n{summary}")

if __name__ == "__main__":
    asyncio.run(main())
```

## Node.js Implementation

```typescript
// examples/multi_agent_pipeline.mjs
import { Agent } from 'edgecrab';

const coordinator = new Agent({
  model: 'openai/gpt-4o',
  maxIterations: 5,
  quietMode: true,
  instructions: 'You are a technical writing coordinator.',
});

await coordinator.chat(
  "We're documenting a REST API that manages bookmarks. " +
  'Resources: bookmarks, tags, users.'
);

const apiDesigner   = await coordinator.fork();
const exampleWriter = await coordinator.fork();
const testWriter    = await coordinator.fork();

await exampleWriter.setModel('copilot/gpt-5-mini');

const [endpoints, examples, tests] = await Promise.all([
  apiDesigner.chat(
    'Design 5 REST endpoints. Output a markdown table with Method | Path | Description.'
  ),
  exampleWriter.chat('Write 3 runnable curl examples: create, list, delete a bookmark.'),
  testWriter.chat('Write 3 jest test cases (happy + 404 + auth error).'),
]);

const combined = `# API\n${endpoints}\n\n# Examples\n${examples}\n\n# Tests\n${tests}`;

const summary = await coordinator.chat(
  `Given this documentation, write a one-paragraph executive summary ` +
  `for a product manager:\n\n${combined}`
);

console.log(`=== FINAL DOCS ===\n${combined}\n\n=== SUMMARY ===\n${summary}`);
```

## Measured Results

On the bookmarks API spec above:

| Approach | Output quality* | Wall-clock | Total cost |
|----------|------------------|------------|------------|
| Single agent, one big prompt | 6.5 / 10 | 22s | $0.047 |
| 3 specialists, sequential `fork()` | 8.2 / 10 | 31s | $0.056 |
| **3 specialists, parallel (this)** | **8.4 / 10** | **12s** | $0.056 |

> \* Manual rubric: completeness of endpoints, correctness of curl flags, test coverage of auth path. 3 raters.

**~60% faster at the same cost, with better output.**

## Key Takeaways

1. **Shared context is cheap — `fork()` uses the same in-memory messages.**
2. **Specialists outperform generalists** on structured output tasks (tables, code, tests).
3. **Mix models per specialist.** Examples don't need Opus. Tests might.
4. **The coordinator is the integration layer.** It's where you enforce the final voice/format.

## Verification

Expected tail output:

```
=== SUMMARY ===
The Bookmarks API provides a CRUD interface for managing...
```

## Next

- [Tutorial 4 — Session-Aware Support Bot](../04-session-aware-support/) — persist and search prior conversations
