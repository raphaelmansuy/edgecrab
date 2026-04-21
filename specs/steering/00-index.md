# Mission Steering — Specification Index

> **Status:** Design + Implementation  
> **Branch:** feat/agent-harness-next-release  
> **Scope:** edgecrab-core, edgecrab-cli

## Documents in this Series

| # | File | Purpose |
|---|------|---------|
| 01 | [01-problem-analysis.md](01-problem-analysis.md) | Problem framing, cross-ref analysis, prior art |
| 02 | [02-first-principles.md](02-first-principles.md) | First-principle decomposition, design invariants |
| 03 | [03-architecture.md](03-architecture.md) | Component diagrams, data-flow, message lifecycle |
| 04 | [04-edge-cases.md](04-edge-cases.md) | Edge cases, mitigations, failure modes |
| 05 | [05-ux-tui.md](05-ux-tui.md) | UX design, TUI wireframes, keybindings |
| 06 | [06-implementation-plan.md](06-implementation-plan.md) | Phased implementation checklist |

## One-Line Summary

**Steering** = injecting a user hint or redirect into a *running* agent loop at the
next safe tool-dispatch boundary, without destroying conversation coherence or
Anthropic prompt-cache validity.
