# 015 — Improve Harness & Agent Experience

**Status:** Draft · **Date:** 2026-06-19  
**Owner:** architecture · **Evidence anchor:** homelab `games003` sessions + harness JSONL

First-principles spec for making EdgeCrab a **reliable long-horizon agent harness** — not merely a chat UI with tools bolted on.

---

## Read order

| # | Document | Purpose |
|---|----------|---------|
| [001-five-whys.md](./001-five-whys.md) | Root-cause chain from real failure (games003) |
| [002-first-principles.md](./002-first-principles.md) | Four harness questions · seven jobs · concept separation |
| [003-official-thesis.md](./003-official-thesis.md) | External grounding (providers, security, agent safety literature) |
| [004-code-anchors.md](./004-code-anchors.md) | **Code is law** — current implementation map |
| [005-evidence-games003.md](./005-evidence-games003.md) | Battle-tested case study (`927f4d85`, `e22c0a28`, **`2e720f47`**) |
| [006-comparator-hermes-claude-pi.md](./006-comparator-hermes-claude-pi.md) | Hermes · Claude Code · PI-style verify loops |
| [007-architecture-target.md](./007-architecture-target.md) | Target shape (ProgressSink · CompletionPolicy · Perception) |
| [008-improvement-plan.md](./008-improvement-plan.md) | Phased plan (P0–P3) · DRY · SOLID module boundaries |
| [009-acceptance-criteria.md](./009-acceptance-criteria.md) | CI gates **HA-01..HA-40** |
| [010-battle-test-matrix.md](./010-battle-test-matrix.md) | Scenario matrix · expected harness behavior |
| [011-hermes-parity-map.md](./011-hermes-parity-map.md) | Hermes-agent borrow list · module mapping |
| [012-brutal-assessment-jun2026.md](./012-brutal-assessment-jun2026.md) | **Honest scorecard** · R1–R8 · 4-session forensics |
| [013-impact-ranked-backlog.md](./013-impact-ranked-backlog.md) | **Rank 1–13** backlog · HA-41..43 · 90-day metrics |
| [014-post-implementation-reassessment.md](./014-post-implementation-reassessment.md) | **Post-fix scorecard** · remaining gaps · homelab expectation |

---

## Cross-references (existing specs)

| Spec | Relationship |
|------|----------------|
| [agent_harness/000_overview.md](../agent_harness/000_overview.md) | Progress vs completion gap study |
| [agent_harness/001_adr_unified_agent_harness.md](../agent_harness/001_adr_unified_agent_harness.md) | ADR: Unified harness (proposed) |
| [014-improve-local-harness/README.md](../014-improve-local-harness/README.md) | Local LM Studio / Ollama geometry |
| [002-terminal-ux-ui/006-stuck-scenarios-playbook.md](../002-terminal-ux-ui/006-stuck-scenarios-playbook.md) | User-visible “stuck” taxonomy |
| [007-minimum-context/007-implementation-assessment.md](../007-minimum-context/007-implementation-assessment.md) | Indexed tools · `tool_search` |
| [improve_plan/31-harness-deep-comparison.md](../improve_plan/31-harness-deep-comparison.md) | J1–J7 harness jobs vs Hermes/Claude |
| [001-gap-analysis-v14/000-methodology.md](../001-gap-analysis-v14/000-methodology.md) | Scoring rubric · DRY/SOLID rules |

---

## Executive summary (one screen)

```text
  TODAY                         TARGET
  ─────                         ──────
  Strong StreamEvent bus        + Unified RunOutcome contract (all surfaces)
  Weak perception loop        + Task-class verification (preview / screenshot)
  Spill hides source truth    + Spill with mandatory artifact path + range hints
  Completion ≈ "had text"     + CompletionPolicy + harness gates authoritative
  107 tools / 55 on wire      + Operator-visible wire/deferred partition
  OTEL on but collector off   + Fail-soft observability (no log spam)
  Copilot non-streaming 30s+  + Provider-accurate liveness + streaming recovery
  memory_write cap blind fail + Structured limit recovery (Hermes parity)
  Todo without verify step    + Task-class verify targets + todo compress snapshot

  4 VISUAL SESSIONS (Jun 18–19): 0/4 browser preview success
  ┌────────────────────────────────────────────────────────────┐
  │ games003 ×3 + race_gamey (0aeef965): 59 API iters each class │
  │ ROOT: profile config preview OFF while global ON (no merge)  │
  │ → config read jail → terminal storm → markdown theater       │
  └────────────────────────────────────────────────────────────┘
  ┌────────────────────────────────────────────────────────────┐
  │ The harness must close: INTENT → PLAN → ACT → VERIFY → DONE │
  │ EdgeCrab is strong on ACT and weak on VERIFY and DONE.      │
  └────────────────────────────────────────────────────────────┘
```

**Non-goal:** This spec does not replace model quality. It makes the **cheap path correct** so Haiku-class models fail less often and operators see truth.

---

## Glossary

| Term | Definition |
|------|------------|
| **Harness** | Runtime that owns the ReAct loop, tool dispatch, result shaping, and stop semantics |
| **Wire set** | Tool schemas sent to the LLM on this iteration (`indexed` mode: hot + materialized) |
| **Spill** | Tool result moved to `.edgecrab-artifacts/`; stub returned to model |
| **RunOutcome** | Terminal contract: `CompletionDecision` + `ExitReason` + user message |
| **Perception turn** | Tool-backed observation the harness treats as verification evidence |
| **Code is law** | Behavior not in schema, validator, or deterministic gate does not exist for the model |
