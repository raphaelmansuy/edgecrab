# 016 — Harness Assessment (Jun 2026)

**Date:** 2026-06-19  
**Method:** first-principles rubric · live session forensics · SQLite · `harness.jsonl` · code audit · Hermes Agent comparator  
**Baseline:** [015-improve-harness-and-agent](../015-improve-harness-and-agent/README.md) (especially [012](../015-improve-harness-and-agent/012-brutal-assessment-jun2026.md), [014](../015-improve-harness-and-agent/014-post-implementation-reassessment.md))

---

## One-line verdict

EdgeCrab is a **production-grade ACT harness** (dispatch, security, spill, streaming) wearing a **CompletionPolicy** costume that **does not govern mid-loop physics**. Homelab visual sessions prove it: **158 terminal calls vs 0 successful CLI perception on visual tasks**, yet **2 sessions marked `completed`** in the DB. Hermes is messier in code layout but **more honest at turn finalization** and **more forgiving on localhost preview** for dev UX.

---

## Document map

| Doc | Purpose | Read when |
|-----|---------|-----------|
| [001-methodology](./001-methodology.md) | Sources, rubric, scope | You need audit provenance |
| [002-first-principles-rubric](./002-first-principles-rubric.md) | Jun 2026 harness law (Q1–Q5, J1–J7) | You need the standard |
| [003-session-forensics](./003-session-forensics.md) | Logs + `state.db` quantitative evidence | You need numbers |
| [004-hermes-comparator](./004-hermes-comparator.md) | Hermes mechanisms to borrow / reject | You need parity targets |
| [005-code-audit](./005-code-audit.md) | Module-by-module architecture truth | You need code anchors |
| [006-brutal-verdict](./006-brutal-verdict.md) | Scorecard + root-cause chain | You need the honest summary |
| [007-priority-backlog](./007-priority-backlog.md) | Ranked fixes with gates | You need what to ship next |

---

## Evidence anchors

```text
  Logs:     ~/.edgecrab/profiles/homelab/logs/harness.jsonl  (973 lines, 7 sessions)
            ~/.edgecrab/profiles/homelab/logs/agent.log
  DB:       ~/.edgecrab/profiles/homelab/state.db
  Config:   ~/.edgecrab/config.yaml          (preview ON)
            ~/.edgecrab/profiles/homelab/config.yaml  (preview key ABSENT)
  Hermes:   /Users/raphaelmansuy/Github/03-working/hermes-agent
  Code:     crates/edgecrab-core/src/conversation.rs (~7.6k lines)
            crates/edgecrab-core/src/turn_dispatch.rs
            crates/edgecrab-tools/src/tool_loop_guardrails.rs
```

---

## Relationship to spec 015

| 015 doc | 016 update |
|---------|------------|
| [012-brutal-assessment](../015-improve-harness-and-agent/012-brutal-assessment-jun2026.md) | Still valid; extended with `d4d6b6b4`, doctor JSONL bug |
| [014-post-reassessment](../015-improve-harness-and-agent/014-post-implementation-reassessment.md) | **Overstated** — profile YAML still lacks `security.preview`; doctor still warns |
| [013-backlog](../015-improve-harness-and-agent/013-impact-ranked-backlog.md) | Superseded ranks in [007](./007-priority-backlog.md) |

---

## Quick scorecard

```text
  Dimension                    Score   Δ vs 015-012
  ───────────────────────────  ─────   ─────────────────────────────
  J2 Dispatch                  █████   unchanged — best-in-class
  J4 Effect mediation          █████   unchanged — correct security defaults
  Q1 Progress / liveness       ████░   shelf strong; Copilot opacity persists
  Q5 Verification (DONE)       ██░░░   +1 if generous — strict mode exists but late
  Config ↔ runtime parity      ██░░░   HA-41 coded; ops path still broken
  Observability (doctor)       ██░░░   harness.jsonl rich; analyzer blind to JSONL
  Hermes parity (loop physics) ███░░   ahead on CompletionPolicy types; behind on finalize
```

Full analysis: [006-brutal-verdict](./006-brutal-verdict.md)
