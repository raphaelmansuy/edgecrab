# 010 — Completion Truth & VERIFY

**Cross-ref:** [001 rubric Q5](./001-first-principles-rubric.md) · [003 mid-loop assess](./003-main-loop-physics.md)

**Q5: Was the task actually done?** This is where the harnesses diverge most in **type system** — and converge in **weak enforcement**.

---

## Philosophy comparison

```text
  HERMES                              EDGECRAB
  ───────────────────────────────     ───────────────────────────────────────
  String exit reason                  Typed CompletionDecision + ExitReason
  Heuristic finalizer footers         Pluggable CompletionPolicy trait
  Model text = loop exit              LoopAction::Done → provisional assess
  No HarnessSnapshot type             HarnessSnapshot deterministic gates
```

---

## Hermes completion path

```text
  Model returns text (no tool_calls)
       │
       ▼
  Break while loop
       │
       ▼
  finalize_turn()
       ├─ completed = (final_response is not None
       │               AND api_call_count < max_iterations
       │               AND not failed)
       ├─ _turn_exit_reason string
       ├─ file-mutation verifier footer (if writes occurred)
       └─ _format_turn_completion_explanation()
```

**No mid-loop re-open** when model claims done prematurely.

**Code:** `agent/turn_finalizer.py`, `run_agent._format_turn_completion_explanation`

### Hermes `_turn_exit_reason` → operator

The explainer maps string reasons to human text — not a decision enum:

| Scenario | Typical reason string |
|----------|----------------------|
| Normal text | `text_response(finish_reason=stop)` |
| User cancel | `interrupted_by_user` |
| Guardrail | `guardrail_halt` |
| Budget | `max_iterations_reached(N/M)` |
| Stream recovery | `partial_stream_recovery` |

---

## EdgeCrab completion path

```text
  Model returns text
       │
       ▼
  LoopAction::Done(text)
       │
       ▼
  build_harness_snapshot(...)     ← may be provisional / default gates
       │
       ▼
  assess_completion(CompletionContext)
       │
       ├─ Incomplete / NeedsVerification / Failed
       │       └─► inject follow-up user msg → continue loop
       │
       └─ Completed (+ optional shadow judge veto)
               └─► break loop
       │
       ▼
  Final assess_completion(full snapshot)
       │
       ▼
  format_turn_completion_explanation → StreamEvent::RunFinished
```

**Code:** `completion_assessor.rs`, `conversation.rs`, `turn_completion.rs`

---

## CompletionDecision priority tree (EdgeCrab)

`DefaultCompletionPolicy::assess` — ordered checks:

| Order | Condition | Decision |
|-------|-----------|----------|
| 1 | Cancel / interrupt | `Interrupted` |
| 2 | Pending clarify/approval | `NeedsInput` |
| 3 | Harness block | `Incomplete` |
| 4 | Budget exhausted | `Incomplete` |
| 5 | Open todos | `Incomplete` |
| 6 | **HarnessSnapshot.blocks_completion()** | `Incomplete` |
| 7 | Critical tool failure | `Incomplete` |
| 8 | Empty response | `Failed` |
| 9 | Verification debt (strict) | `NeedsVerification` |
| 10 | Else | `Completed` |

**Code:** `completion_assessor.rs` L94–102 harness block branch

---

## HarnessSnapshot gates (EdgeCrab-only)

**Code:** `edgecrab-tools/src/harness_gates.rs`

Deterministic gates **without LLM judgment**:

| Gate | Blocks completion when |
|------|------------------------|
| Mutation debt | Unresolved write/patch failures in turn |
| Oracle failure | Post-mutation syntax/test oracle failed |
| Terminal mutation error | Last mutation tool returned error |

Consumed via `CompletionContext.harness: HarnessSnapshot`.

Hermes equivalent: **file-mutation footer** in finalizer — advisory, not typed gate.

---

## VERIFY by task class

| Task class | Minimum evidence | Hermes | EdgeCrab |
|------------|------------------|--------|----------|
| VisualUx | browser_snapshot / vision on served URL | Footer hints only | `verification_strict` + `NeedsVerification` |
| CodeChange | test/lint oracle | Footer hints | `harness_gates` (JS syntax today) |
| Research | web_extract success | None | `has_recent_critical_tool_failure` |
| Ops/shell | exit_code 0 | Tool result only | Tool result only |

### VisualUx anti-patterns (EdgeCrab)

| Check | Function |
|-------|----------|
| Browser nav exhausted | `visual_browser_navigate_exhausted` |
| Markdown theater | `markdown_theater_without_perception` |
| Terminal storm | `HarnessTurnAdvisory` + analyzer |

Hermes has no named equivalents — relies on operator + model.

---

## Critical gap (both weak): VERIFY loop physics

```text
  Jun 2026 REQUIRED INVARIANT:
  ┌─────────────────────────────────────────────────────────────┐
  │  No CompletionDecision::Completed without evidence artifact │
  │  OR explicit NeedsVerification surfaced to operator           │
  └─────────────────────────────────────────────────────────────┘

  HERMES:   no CompletionPolicy — model prose ends turn
  EDGECRAB: types exist — but:
            • mid-loop provisional snapshot may skip gates
            • loop body advisories warn but do not block tools
            • end-loop assess is authoritative (when reached)
```

**Code anchor (EdgeCrab risk):** provisional `assess_completion` in `LoopAction::Done` path — snapshot completeness varies.

---

## Shadow judge (EdgeCrab-only)

Optional cheap model veto after assessor returns `Completed`:

- Config: `auxiliary.shadow_judge`
- Can downgrade `Completed` → `Incomplete`

Hermes has no equivalent — background_review is post-turn memory, not completion veto.

---

## report_task_status integration (EdgeCrab)

EdgeCrab parses `report_task_status` tool results for:
- Remaining steps
- Evidence claims

Hermes uses todo store + kanban tools — different shape, similar Q3 intent.

---

## turn_completion.rs (UX layer)

Formats operator-facing explanation **after** assessment:

```rust
pub fn format_turn_completion_explanation(
    outcome: &RunOutcome,
    ctx: &TurnCompletionContext,
) -> String
```

Adds:
- Harness block reason from snapshot
- Visual verification hints on `NeedsVerification`
- Warning on unanswered tool_calls (`count_unanswered_tool_calls`)

Hermes `_format_turn_completion_explanation` — same UX intent, string-based input.

---

## Side-by-side scoring

| Capability | Hermes | EdgeCrab |
|------------|--------|----------|
| Typed completion | ░░░░░ | ████░ |
| Mid-loop re-open on premature done | ░░░░░ | ████░ |
| Deterministic gates | ██░░░ | ████░ |
| Strict visual verify enforce | ██░░░ | ██░░░ |
| Operator explainer | ████░ | ████░ |
| Post-turn completion veto | ░░░░░ | ██░░░ (shadow judge) |

**Verdict:** EdgeCrab has the **better type system**; both fail Jun 2026 VERIFY bar in production unless operator enables strict modes and gates are fully armed end-to-end.

---

## Convergence target

Both should implement:

```text
  VERIFY as loop physics (not post-hoc markdown):
  1. Block terminal/write tools on VisualUx until perception success
  2. Block Completed without artifact path in snapshot
  3. Surface NeedsVerification to TUI/gateway as first-class state
```

EdgeCrab is closer structurally; Hermes is closer on operator footers and budget-exhaustion summary.
