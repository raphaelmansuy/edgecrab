# 001 — First Principles: What Is an Agent Runtime?

Before feature matrices, define the **invariants** both Hermes and EdgeCrab claim to satisfy. A runtime passes or fails on these — not on slash-command count.

---

## 1. The agent loop (non-negotiable)

An agent runtime is a **closed loop**:

```text
observe (messages + tools + world state)
  → plan (LLM)
  → act (tool dispatch)
  → observe (tool results)
  → … until stop condition
```

Both implement this as **ReAct** with OpenAI-shaped messages:

| Invariant | Hermes | EdgeCrab |
|-----------|--------|----------|
| Single orchestrator shared CLI/gateway/cron/ACP | `run_agent.py::AIAgent` | `edgecrab-core::Agent` |
| Tool dispatch centralised | `model_tools.py` | `edgecrab-tools::ToolRegistry` |
| Max iteration cap | `agent.max_turns` | `max_iterations` (default 90) |
| Streaming to UI | Yes | Yes (`StreamEvent`) |
| Hard interrupt | Yes | Yes (`Agent::interrupt()`) |

**Verdict:** **Parity** on core loop semantics. EdgeCrab runs hotter (90 vs Hermes' typical lower caps) but same model.

---

## 2. Context is a budget, not a log

Long conversations exceed model windows. Both must:

1. **Never break the system prompt cache mid-turn** (Anthropic economics + correctness).
2. **Compress** when over threshold.
3. **Inject dynamic context** (time, goals, steers) into *messages*, not cached system blocks.

| Mechanism | Hermes | EdgeCrab |
|-----------|--------|----------|
| Stable vs volatile prompt zones | `agent/prompt_builder.py`, `prompt_caching.py` | `prompt_builder.rs` stable/dynamic split |
| LLM summarization + structural fallback | `agent/context_compressor.py` | `compression.rs` |
| Prune old tool outputs | Yes | Yes |
| Protect last N messages | `protect_last_n` | Same (default 20) |
| Gateway hygiene compress at 85% | Yes (safety net) | Pressure warning at 85% |

**Verdict:** **Parity** — EdgeCrab shipped explicit cache-zone engineering (spec 004); Hermes had it first in production.

**Brutal note:** Both can still lose task fidelity after aggressive compression. Neither has provably lossless long-horizon memory without external stores (Honcho, session search, skills).

---

## 3. Tools are the real product

The LLM is interchangeable; **tools define capability**. First-principles tool taxonomy:

| Class | Purpose | Both? |
|-------|---------|-------|
| **Filesystem** | read/write/patch/search | Yes |
| **Shell** | local + remote execution | Yes (6 backends each) |
| **Web** | search + extract (+ crawl on EC) | Yes (EC adds `web_crawl`) |
| **Browser** | CDP automation | Yes |
| **Delegation** | sub-agents | Yes |
| **Memory** | cross-session persistence | Yes (different shapes) |
| **Scheduling** | cron | Yes |
| **MCP** | external tool servers | Yes |
| **Platform** | send_message on gateway | Yes |

**Verdict:** **Parity on core coding agent**. Hermes leads on **ecosystem tools** (kanban, video, Spotify, blueprints). EdgeCrab leads on **LSP depth** (25 semantic tools + write gate).

---

## 4. Trust boundaries

An agent with shell + network is a **remote code execution surface**. Minimum bar:

| Boundary | Required behavior |
|----------|-------------------|
| Path safety | Jail reads/writes |
| SSRF | Block private URLs on fetch |
| Command approval | Dangerous ops gated |
| Prompt injection | Scan injected context files |
| Secret hygiene | Redact before display/log |

Both implement all five. Hermes adds **smart approval** (aux LLM risk scorer) and **write approval gates** for memory/skills. EdgeCrab adds **shadow judge** (post-completion LLM oracle) — a different trust bet.

**Verdict:** **Different tradeoffs**. Hermes = more human-in-the-loop UX. EdgeCrab = more automated verification (shadow judge, mutation footer, LSP gate).

---

## 5. Multi-surface delivery

Same agent must run in:

- Interactive terminal
- Messaging platforms (Telegram, Slack, …)
- IDE (ACP)
- Scheduled jobs (cron)
- Optional API server

Both achieve this with **one agent core + platform adapters**. Hermes adds Electron desktop + React TUI sidecar; EdgeCrab uses **single-process ratatui**.

**Verdict:** **Hermes leads surfaces** (desktop, web dashboard, classic+modern TUI). **EdgeCrab leads ops simplicity** (one binary, no Node sidecar).

---

## 6. Extensibility without forking

Users will add: providers, platforms, web backends, memory backends, skills.

| Approach | Hermes | EdgeCrab |
|----------|--------|----------|
| Primary extension | Python plugins (`plugins/`, pip entry points) | Rust compile-time tools + subprocess plugin ADR |
| Plugin count (repo) | 100+ plugin directories | `edgecrab-plugins` (Hermes compat hooks) |
| Runtime plugin install | Yes (`hermes plugins install`) | Partial (skills hub; WASM deferred) |

**Verdict:** **Hermes leads extensibility today**. EdgeCrab trades plugin velocity for **type safety + single binary**.

---

## 7. The five questions to ask before picking

1. **Do I need kanban / curator / video / Spotify / Teams?** → Hermes (today).
2. **Do I need one static binary on edge/Termux/low-RAM?** → EdgeCrab.
3. **Do I need 30+ OAuth providers out of the box?** → Hermes.
4. **Do I need LSP-integrated coding with post-write type errors?** → EdgeCrab (deeper).
5. **Am I migrating from Hermes?** → EdgeCrab (`edgecrab migrate`); expect gaps on 007/012/021/025 per [001-gap-analysis](../001-gap-analysis-v14/999-roadmap.md).

---

## 8. Assessment rubric (used in all docs)

| Grade | Meaning |
|-------|---------|
| **A** | Production-grade, tested, documented, no known critical gap |
| **B** | Works; minor gaps or debt |
| **C** | Partial / behind parity / high debt |
| **D** | Missing or stub |
| **≠** | Different design — not comparable on same axis |

Scores are **per dimension**, not per product.
