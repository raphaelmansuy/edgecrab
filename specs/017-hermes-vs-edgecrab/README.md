# 017 — Hermes Agent vs EdgeCrab Harness (Code-is-Law Cross-Ref)

Side-by-side first-principles comparison of the **agent harness** — the ReAct loop, tool dispatch, context budget, guardrails, and completion truth — not gateway, TUI, or provider adapters.

**Sources (authoritative):**

| Agent | Root | Harness entry |
|-------|------|---------------|
| Hermes | `/Users/raphaelmansuy/Github/03-working/hermes-agent` | `run_agent.py` → `agent/conversation_loop.py::run_conversation` |
| EdgeCrab | `/Users/raphaelmansuy/Github/03-working/edgecrab` | `crates/edgecrab-core/src/agent.rs` → `conversation.rs::execute_loop` |

**Method:** Read source. No marketing docs. When behavior differs, cite the owning function.

---

## Document index

| # | Doc | Question answered |
|---|-----|-------------------|
| 001 | [First-principles rubric](./001-first-principles-rubric.md) | Q1–Q5 operator questions + J1–J7 harness jobs |
| 002 | [Architecture module map](./002-architecture-module-map.md) | File-to-file ownership, dependency shape |
| 003 | [Main loop physics](./003-main-loop-physics.md) | `while` condition, budgets, exit paths |
| 004 | [Turn lifecycle](./004-turn-lifecycle-prologue-epilogue.md) | Prologue / epilogue split |
| 005 | [Tool dispatch](./005-tool-dispatch-and-parallelism.md) | Parallel vs sequential, pre/post pipeline |
| 006 | [Guardrails & stall breakers](./006-guardrails-stall-breakers.md) | Loop dampening, visual storms, halt semantics |
| 007 | [Compression & context](./007-compression-context-budget.md) | When/how context is reshaped |
| 008 | [Error classification & recovery](./008-error-classification-recovery.md) | API failures → next action |
| 009 | [Spill & turn budget](./009-spill-turn-budget-results.md) | Large tool result handling |
| 010 | [Completion truth & VERIFY](./010-completion-truth-verify.md) | When is a turn "done"? |
| 011 | [Borrow / reject matrix](./011-borrow-reject-matrix.md) | What EdgeCrab should port or refuse |
| **012** | **[Implementation plan: exceed Hermes](./012-implementation-plan-exceed-hermes.md)** | **SOLID/DRY owners, HA test matrix, sprint order** |

**Related prior work:** [016-harness-assessment](../016-harness-assessment/README.md) · [015/011 hermes-parity-map](../015-improve-harness-and-agent/011-hermes-parity-map.md)

---

## One-screen verdict (Jun 2026)

```text
  DIMENSION              HERMES                    EDGECRAB
  ─────────────────────  ────────────────────────  ────────────────────────
  Loop decomposition     Modular (4 modules)       Monolith (~7.6k lines)
  Error taxonomy         Single classifier         Scattered retries
  Completion types       String exit reason        Typed RunOutcome + gates
  Guardrail defaults     warn-only (hard opt-in)   hard-stop default ON
  Security mediation     Global private URLs       Port allowlist preview
  VERIFY enforcement     Weak (both)               Weak (types stronger)
  Offline harness doctor None                      harness_analyzer + replay CI
  Background memory      Forked post-turn agent    Not ported
```

Net: **complementary**. Hermes optimizes loop maintainability and operator UX paths; EdgeCrab optimizes typed completion, security defaults, and post-mortem tooling.
