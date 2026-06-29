# 000 — EdgeCrab vs Hermes Agent: Assessment Index

**Status:** Draft (June 2026)  
**Scope:** Brutal, first-principles, feature-by-feature comparison of two agent runtimes  
**Codebases:**

| Agent | Path | Language | Role |
|-------|------|----------|------|
| **EdgeCrab (EC)** | `/Users/raphaelmansuy/Github/03-working/edgecrab` | Rust 2024 | Spiritual successor; Hermes-parity + Rust-native bets |
| **Hermes / NousHermes** | `/Users/raphaelmansuy/Github/03-working/hermes-agent` | Python 3.11+ | Reference implementation; broad ecosystem, plugin-first |

---

## Why this folder exists

`specs/001-gap-analysis-v14/` tracks **Hermes → EdgeCrab porting gaps** (30 numbered features).  
`specs/002-tui-hemes-vs-edgecrab/` tracks **TUI parity only**.

This folder (`003-ec-vs-hermes`) is the **neutral cross-ref**: what each agent *is*, what each *does better*, and where parity claims are honest vs marketing.

---

## Methodology

1. **First principles** — strip branding; compare on agent invariants (see [001-first-principles.md](001-first-principles.md)).
2. **Code is law** — file paths and module names cited; docs treated as aspirational until verified.
3. **Brutal scoring** — each row gets: **EC leads** | **Hermes leads** | **Parity** | **Both weak** | **Different tradeoffs**.
4. **No aggregate "winner"** — these are different products optimizing different constraints.

---

## Document map

| # | Document | Question answered |
|---|----------|-------------------|
| [001](001-first-principles.md) | First Principles | What must an agent runtime guarantee? |
| [002](002-architecture-runtime.md) | Architecture & Runtime | How is each system built and deployed? |
| [003](003-core-agent-harness.md) | Core Agent Harness | ReAct loop, goals, steering, compression, completion |
| [004](004-tools-toolsets.md) | Tools & Toolsets | Capability surface area |
| [005](005-gateway-messaging.md) | Gateway & Messaging | Multi-platform delivery |
| [006](006-models-providers-routing.md) | Models, Providers, Routing | Who can you talk to, how cheaply? |
| [007](007-memory-skills-sessions.md) | Memory, Skills, Sessions | Persistent knowledge & history |
| [008](008-security-trust.md) | Security & Trust | What can go wrong, what is guarded? |
| [009](009-cli-tui-ux.md) | CLI, TUI, UX | Operator experience |
| [010](010-extensibility-mcp-proxy-acp.md) | Extensibility | MCP, proxy, ACP, plugins |
| [011](011-engineering-quality.md) | Engineering Quality | Tests, CI, maintainability debt |
| [012](012-master-gap-matrix.md) | Master Gap Matrix | One table to rule them all |
| [013](013-verdict-and-strategy.md) | Verdict & Strategy | Pick one, migrate, coexist |

---

## Headline numbers (verified June 2026)

| Metric | EdgeCrab | Hermes |
|--------|----------|--------|
| Workspace crates / top-level modules | 20 Rust crates | Monolith + `hermes_cli/`, `tools/`, `gateway/`, `plugins/` |
| Built-in slash commands (catalog) | 84 (`edgecrab-command-catalog`) | 75 (`hermes_cli/commands.py`) |
| Core tool handlers (approx.) | ~75 core + 25 LSP + MCP ext | ~71 built-in + plugin-gated |
| Gateway platform adapters (built-in) | 17 (`edgecrab-gateway/src/*.rs`) | ~20 built-in + ~10 plugin platforms |
| Model providers (catalog/registry) | 19 YAML providers | 30+ registry + auto-extended plugins |
| Test suite (order of magnitude) | ~650+ Rust tests | ~25k pytest tests (~1.25k files) |
| Default agent home | `~/.edgecrab/` | `~/.hermes/` |
| Migration path | `edgecrab migrate` from Hermes | `hermes claw migrate` from OpenClaw |

---

## Related specs (do not duplicate)

- Gap backlog: [specs/001-gap-analysis-v14/999-roadmap.md](../001-gap-analysis-v14/999-roadmap.md)
- TUI deep-dive: [specs/002-tui-hemes-vs-edgecrab/](../002-tui-hemes-vs-edgecrab/)
- EC dev guide: [AGENTS.md](../../AGENTS.md)

---

## Reading order

**Executive (30 min):** 001 → 012 → 013  
**Engineering lead:** 002 → 003 → 004 → 011  
**Gateway operator:** 005 → 008  
**Model/provider integrator:** 006 → 010
