# EdgeCrab SDK — Specification Suite

> **The most loved AI Agent SDK, built for developers who ship.**

This directory contains the canonical specification, design, and implementation plan for the current EdgeCrab SDK — a quad-language (Rust, Python, Node.js, WASM) SDK that surfaces the full power of the EdgeCrab agent framework to developers. The directory name is retained for repository stability only.

---

## Document Map

```
+-------------------------------------------------------------------+
|                     EdgeCrab SDK — Spec Suite                      |
+-------------------------------------------------------------------+
|                                                                   |
|  01-WHY.md ──────────────> Philosophy & Vision                    |
|       |                    Why this SDK exists.                    |
|       |                    What makes developers love an SDK.      |
|       v                                                           |
|  02-SPEC.md ─────────────> Full SDK Specification                 |
|       |                    API surfaces, types, protocols.         |
|       |                                                           |
|       +──> 03-COMPARISON.md  Competitor Analysis                  |
|       |                      Claude SDK vs ADK vs OpenAI           |
|       |                      vs Pydantic AI vs EdgeCrab            |
|       |                                                           |
|       +──> 04-TUTORIAL.md    Developer Tutorial                   |
|       |                      Simple → Complex, real use cases      |
|       |                                                           |
|       +──> 05-ADR.md         Architecture Decision Records        |
|       |                      Key design choices & rationale        |
|       |                                                           |
|       v                                                           |
|  06-IMPLEMENTATION.md ───> Implementation Plan                    |
|       |                    Phases, milestones, deliverables        |
|       |                                                           |
|       +──> 07-ROADBLOCKS.md  Edge Cases & Risks                  |
|       |                      What can go wrong & mitigations       |
|       |                                                           |
|       v                                                           |
|  08-DEVELOPER-DOCS.md ──> Full Developer Documentation            |
|       |                   API reference, guides, recipes          |
|       |                                                           |
|       +──> 09-CUSTOM-TOOLS.md  Custom Tools Deep-Dive             |
|       |                        4 extensibility layers, FFI bridge  |
|       |                        WASM SDK tools & browser integration |
|       |                                                           |
|       v                                                           |
|  10-EXAMPLES.md ─────────> Real-World Agent Examples              |
|       |                    11 examples: Python, Node.js, Rust,    |
|       |                    WASM (browser agent)                   |
|       |                                                           |
|       v                                                           |
|  11-EVENT-SYSTEM.md ────> Event System & Observability            |
|                           21 typed events, transport layers,      |
|                           interactive UI, time-travel debugging   |
|       |                                                           |
|       v                                                           |
|  12-CLI-SDK-PARITY.md ──> CLI/SDK Parity Analysis                |
|                           Feature gap matrix, design decisions,   |
|                           phased implementation plan              |
+-------------------------------------------------------------------+
```

## Cross-Reference Key

| Code  | Document | Purpose |
|-------|----------|---------|
| `WHY` | [01-WHY.md](01-WHY.md) | Philosophy, vision, developer love thesis |
| `SPEC` | [02-SPEC.md](02-SPEC.md) | Complete API specification |
| `COMP` | [03-COMPARISON.md](03-COMPARISON.md) | Competitive analysis |
| `TUT` | [04-TUTORIAL.md](04-TUTORIAL.md) | Hands-on tutorial |
| `ADR` | [05-ADR.md](05-ADR.md) | Architecture decisions |
| `IMPL` | [06-IMPLEMENTATION.md](06-IMPLEMENTATION.md) | Implementation plan |
| `RISK` | [07-ROADBLOCKS.md](07-ROADBLOCKS.md) | Risks & edge cases |
| `DOCS` | [08-DEVELOPER-DOCS.md](08-DEVELOPER-DOCS.md) | Developer documentation |
| `TOOLS` | [09-CUSTOM-TOOLS.md](09-CUSTOM-TOOLS.md) | Custom tools deep-dive + WASM SDK |
| `EXAM` | [10-EXAMPLES.md](10-EXAMPLES.md) | 11 real-world agent examples |
| `EVENT` | [11-EVENT-SYSTEM.md](11-EVENT-SYSTEM.md) | Event system & observability spec |
| `PARITY` | [12-CLI-SDK-PARITY.md](12-CLI-SDK-PARITY.md) | CLI/SDK feature parity analysis |

## Design Principles

1. **Code is Law** — The SDK surfaces EdgeCrab's real capabilities, not a subset
2. **Four Languages, One Experience** — Rust, Python, Node.js, WASM with identical semantics (WASM is a lite subset)
3. **Zero Boilerplate** — Simple things must be simple; complex things must be possible
4. **Type-Safe by Default** — Catch errors at compile/write time, not runtime
5. **Observable** — Every agent run is traceable, debuggable, cost-trackable
6. **Run Anywhere** — Server, CLI, browser, edge compute — same agent core
