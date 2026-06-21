# 002 — First Principles Rubric (Jun 2026)

**Cross-ref:** [015/002](../015-improve-harness-and-agent/002-first-principles.md) · [006-verdict](./006-brutal-verdict.md)

---

## The five operator questions (Q1–Q5)

Every agent harness must answer these **orthogonally**. Collapsing them produces games003: busy shelf, unverified fork.

```text
  Q1  What is happening now?        → Progress events, tool shelf, subagent tree
  Q2  Is forward progress real?     → Stall detect, heartbeats, guardrails
  Q3  What work remains?            → Goals, todos, subgoals, report_task_status
  Q4  Why did the run stop?         → ExitReason + operator explainer
  Q5  Was the task actually done?   → Perception evidence, oracles, completion gates
```

| Question | Jun 2026 best practice | EdgeCrab today | Hermes today |
|----------|------------------------|----------------|--------------|
| Q1 | Sub-second tool state; parallel rows | **Strong** — `TurnActivityState`, `StreamEvent` bus | Strong — `status_callback`, TUI gateway |
| Q2 | Circuit breakers on loops | **Weak** — guardrails warn; hard-stop **off** | Medium — guardrails + optional halt |
| Q3 | Plan survives compression | Partial — todo snapshot shipped; no verify coupling | Strong — `TodoStore.format_for_injection` |
| Q4 | Never ambiguous "done" | **Medium** — `RunOutcome` exists; mid-loop bypass | Medium — `_turn_exit_reason` + explainer |
| Q5 | Perception before claim | **Weak** — advisory + late policy | Weak on strict — but preview easier |

---

## Seven harness jobs (J1–J7)

From production agent harness literature + EdgeCrab spec 015:

| Job | Definition | EdgeCrab owner | Grade |
|-----|------------|----------------|-------|
| **J1** Schema / code-is-law | Strict tool JSON; wire set discipline | `registry.rs`, `tool_schema_index.rs` | ████░ |
| **J2** Dispatch | Parallel safe execution, dedup | `conversation.rs`, `turn_dispatch.rs` | █████ |
| **J3** Pre-dispatch validation | Budget gate, arg repair | `mutation_turn_policy`, `tool_argument_pipeline` | ████░ |
| **J4** Effect mediation | Path jail, SSRF, command scan | `edgecrab-security` | █████ |
| **J5** Result shaping | Spill, summarize, turn budget | `artifact_spill.rs`, `turn_dispatch.rs` | ███░░ |
| **J6** Failure recovery | Structured errors → next action | `recovery_catalog.rs`, `harness_advisory.rs` | ███░░ |
| **J7** Cost / liveness | Compression, ctx budget, streaming | `compression.rs`, `provider_call.rs` | ███░░ |

---

## VERIFY loop physics (the gap)

Jun 2026 consensus: **verification is not a post-hoc assessor** — it is **loop physics**.

```text
  REQUIRED INVARIANT (visual/coding tasks):
  ┌─────────────────────────────────────────────────────────────┐
  │  No CompletionDecision::Completed without evidence artifact   │
  │  OR explicit NeedsVerification surfaced to operator           │
  └─────────────────────────────────────────────────────────────┘

  EdgeCrab IMPLEMENTATION:
  ┌─────────────────────────────────────────────────────────────┐
  │  Mid-loop: assess_completion(HarnessSnapshot::default())   │  ← gates skipped
  │  End-loop: assess_completion(full snapshot)                  │  ← strict VisualUx
  │  Loop body: advisories only (no tool block)                  │  ← storm continues
  └─────────────────────────────────────────────────────────────┘
```

Code anchor: `conversation.rs` ~L2256 uses `HarnessSnapshot::default()` for provisional completion.

---

## Jun 2026 anti-patterns (scored in forensics)

| ID | Anti-pattern | Detection | Harness should |
|----|--------------|-----------|----------------|
| AP1 | Markdown verification theater | ≥3 `*VERIFY*.md` without browser success | Block `Completed` or cap writes |
| AP2 | Terminal storm | ≥5 `terminal` in 60s, 0 perception | Block or force browser |
| AP3 | Spill blindness | spill → write new file, no artifact read | Steer or gate patch path |
| AP4 | Config dead-end | `read_file ~/.edgecrab/config.yaml` after SSRF block | `/config` steer only |
| AP5 | Profile config drift | global ON, profile OFF | Merge or migrate on load |
| AP6 | False completed | DB `model_returned_final_text`, 0 perception | `NeedsVerification` |

Evidence: [003-session-forensics](./003-session-forensics.md)

---

## What "done" means in Jun 2026

| Task class | Minimum evidence | EdgeCrab gate |
|------------|------------------|---------------|
| VisualUx | `browser_snapshot` or `vision_*` on served URL | `effective_verification_strict` + `completion_assessor` |
| CodeChange | test/lint oracle or explicit test run | `harness_gates.rs` (JS syntax only today) |
| Research | `web_extract` success or cited URLs | `has_recent_critical_tool_failure` |
| Ops / shell | exit_code 0 + output sanity | tool result only |

**Gap:** gates exist in types; **enforcement is end-of-turn and bypassable mid-loop**.
