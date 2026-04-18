---
title: SDK Tutorials
description: Production-grade, business-value tutorials for the EdgeCrab SDK. Each tutorial solves a real problem with measurable outcomes in Rust, Python, Node.js, and WASM.
sidebar:
  order: 0
---

## SDK Tutorials — Real Problems, Real Solutions

These tutorials don't waste your time with "hello world" demos. Each one solves a problem that costs real money, real engineer-hours, or both. They're short, opinionated, and brutally focused on outcomes.

## Need something demoable today?

Start with the runnable SDK example tracks below before you go deep into the full tutorials.

| Business need | Best example track |
| --- | --- |
| Customer support triage | `business_case_showcase` + `session_aware_support` |
| Executive update drafting | `business_case_showcase` |
| Budget-sensitive engineering help | `cost_aware_review` |
| Multi-step analyst work | `parallel_research` + `multi_agent_pipeline` |

## Verified SDK proof matrix

These examples are not aspirational — they are backed by fresh local end-to-end verification.

| Target | Coverage proof |
| --- | --- |
| Rust | 34/34 passed |
| Node.js | 30/30 passed |
| Python | 28/28 passed |
| WASM | 17/17 passed |

## The Five Tutorials

| # | Tutorial | Problem Solved | Measured Win |
| --- | --- | --- | --- |
| [1](./01-cost-aware-review/) | **Cost-Aware Code Review** | LLM bills from over-using flagship models on trivial work | **60–90% cost cut** vs. always-Opus baseline |
| [2](./02-parallel-research/) | **Parallel Research Pipeline** | Sequential LLM calls that take 30s when they could take 6s | **5× throughput** on independent queries |
| [3](./03-multi-agent-pipeline/) | **Multi-Agent Documentation** | One model trying to do 5 jobs poorly | **Specialist fork pattern** with shared context |
| [4](./04-session-aware-support/) | **Session-Aware Support Bot** | Bots that forget who the user is and re-ask everything | **FTS5 search** across every prior conversation |
| [5](./05-custom-tool-safe-sql/) | **Safe SQL Agent (Custom Tool)** | Agents that can read anything — including your prod DB | **Allow-list + approval** gate for destructive ops |

## Who These Are For

If any of these apply, read on:

```text
┌────────────────────────────────────────────────────────────────┐
│  You are building an LLM product and…                         │
│                                                                 │
│   ✓  Your monthly LLM bill just doubled                        │
│   ✓  Users complain the bot is "slow"                          │
│   ✓  You need to mix models (cheap + expensive) in one flow    │
│   ✓  You need tool-use with human-in-the-loop approval         │
│   ✓  You need to embed an agent in an existing app (not CLI)   │
└────────────────────────────────────────────────────────────────┘
```

## Architecture at a Glance

The three EdgeCrab SDKs are **thin language wrappers around one Rust engine** — the same engine that powers the CLI. No REST, no IPC, no Python-subprocess-spawns-Rust hacks.

```text
                ┌─────────────────────────────────┐
                │    edgecrab-sdk-core (Rust)     │
                │  SdkAgent │ ToolRegistry │ DB   │
                │     ReAct loop │ streaming      │
                └──┬────────────┬────────────┬────┘
                   │            │            │
         ┌─────────┘            │            └─────────┐
         │                      │                      │
    ┌────┴─────┐         ┌──────┴─────┐         ┌──────┴─────┐
    │   Rust   │         │   Python   │         │  Node.js   │
    │ edgecrab │         │  edgecrab  │         │  edgecrab  │
    │   -sdk   │         │  (PyO3)    │         │  (napi-rs) │
    └──────────┘         └────────────┘         └────────────┘
         │                      │                      │
    in-process             in-process             in-process
    zero-copy              zero-copy              zero-copy
```

This matters because:

- **No network hop** → p50 latency matches CLI.
- **Shared session DB** → a Python script and a Node.js dashboard can read the same conversation history.
- **Same toolset** → any tool that works in the CLI works in every SDK.

## Conventions Used

- All code samples are **tested and known-working** on EdgeCrab SDK v0.6+.
- Every tutorial has a **verification block** you can paste into a terminal.
- Every tutorial has a **cost/latency table** showing the win.
- We recommend starting with [Cost-Aware Code Review](./01-cost-aware-review/) — it sets up patterns used throughout.

## Prerequisites

```bash
# 1. Install EdgeCrab (any SDK)
# Rust:
cargo add edgecrab-sdk tokio --features tokio/full

# Python:
pip install edgecrab

# Node.js:
npm install edgecrab

# 2. Set one of these API keys in your shell
export OPENAI_API_KEY=sk-...           # OpenAI
export GITHUB_TOKEN=ghp_...            # GitHub Copilot (gpt-5-mini and related catalog access)
export OPENAI_BASE_URL=http://localhost:11434/v1  # Local Ollama / compatible endpoint
```

## Running the Tutorials

Every tutorial works with **any model** in the catalog. We recommend `copilot/gpt-5-mini` for the cost-sensitive examples and `openai/gpt-4o` or `openai/gpt-5` for the reasoning-heavy ones.

To swap models in any example:

```rust
// Rust
let agent = SdkAgent::new("copilot/gpt-5-mini")?;
```

```python
# Python
agent = Agent("copilot/gpt-5-mini")
```

```typescript
// Node.js
const agent = new Agent({ model: 'copilot/gpt-5-mini' });
```

---

Ready? Start with [Tutorial 1 — Cost-Aware Code Review →](./01-cost-aware-review/)
