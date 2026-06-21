# 001 — First Principles Rubric

**Cross-ref:** [003 loop](./003-main-loop-physics.md) · [010 completion](./010-completion-truth-verify.md)

Every agent harness must answer five **orthogonal** operator questions. Collapsing them produces "busy shelf, unverified fork" failure modes.

---

## Q1–Q5 operator questions

```text
  Q1  What is happening now?        → Progress events, tool shelf, subagent tree
  Q2  Is forward progress real?     → Stall detect, heartbeats, guardrails
  Q3  What work remains?            → Goals, todos, subgoals, report_task_status
  Q4  Why did the run stop?         → ExitReason + operator explainer
  Q5  Was the task actually done?   → Perception evidence, oracles, completion gates
```

| Question | Jun 2026 best practice | Hermes (code) | EdgeCrab (code) |
|----------|------------------------|---------------|-----------------|
| **Q1** | Sub-second tool state; parallel rows | `agent._emit_status`, `stream_callback`, TUI gateway | `StreamEvent` bus, `TurnActivityState`, activity shelf |
| **Q2** | Circuit breakers on loops | `ToolCallGuardrailController` — **warn default**, halt opt-in | `ToolLoopGuardrailController` + `HarnessTurnAdvisory` — **hard-stop default ON** |
| **Q3** | Plan survives compression | `TodoStore.format_for_injection()` after compress | Todo snapshot in `compression.rs` (ported intent) |
| **Q4** | Never ambiguous "done" | `_turn_exit_reason` string + `_format_turn_completion_explanation` | `RunOutcome` + `ExitReason` + `format_turn_completion_explanation` |
| **Q5** | Perception before claim | Weak — no typed gates; footer in finalizer | `CompletionPolicy` + `HarnessSnapshot` — **strong types, weak mid-loop enforce** |

### Q1 — Liveness surface

| Mechanism | Hermes anchor | EdgeCrab anchor |
|-----------|---------------|-----------------|
| Tool start/complete | `tool_executor._emit_terminal_post_tool_call` | `StreamEvent::ToolStart` / `ToolComplete` |
| Status line | `agent._emit_status` | `status_bar.rs`, shelf in CLI |
| Subagent tree | delegate_task + gateway | `sub_agent_runner.rs`, `/agents` overlay |

### Q2 — Progress truth

| Anti-pattern | Hermes detection | EdgeCrab detection |
|--------------|------------------|-------------------|
| Exact-arg failure loop | `before_call` block when `hard_stop_enabled` | Same controller; **default armed** |
| Idempotent no-progress | `ToolCallSignature` hash tracking | Ported in `tool_loop_guardrails.rs` |
| Act-without-perceive (visual) | Implicit (no dedicated module) | `HarnessTurnAdvisory` + `visual_storm_block_result` |
| Terminal storm | Guardrail only | `harness_analyzer::count_terminal_without_perception` (offline) |

### Q5 — VERIFY invariant (Jun 2026 consensus)

```text
  REQUIRED (visual/coding tasks):
  ┌─────────────────────────────────────────────────────────────┐
  │  No "completed" without evidence artifact OR explicit       │
  │  NeedsVerification surfaced to operator                     │
  └─────────────────────────────────────────────────────────────┘

  HERMES:  no CompletionPolicy type — relies on model + footer hints
  EDGECRAB: assess_completion() + HarnessSnapshot — but mid-loop uses
            provisional snapshot; loop body advisories do not block tools
```

---

## J1–J7 harness jobs

From production harness literature + EdgeCrab spec 015:

| Job | Definition | Hermes owner | EdgeCrab owner | Leader |
|-----|------------|--------------|----------------|--------|
| **J1** Schema / code-is-law | Strict tool JSON; wire set discipline | `model_tools` + tool registry | `registry.rs`, `tool_schema_index.rs` | Tie |
| **J2** Dispatch | Parallel-safe execution, dedup | `tool_executor.py` | `conversation.rs`, `turn_dispatch.rs` | Tie |
| **J3** Pre-dispatch validation | Budget gate, arg repair | guardrails + `budget_config` | `mutation_turn_policy`, `tool_argument_pipeline` | EdgeCrab (typed recovery) |
| **J4** Effect mediation | Path jail, SSRF, command scan | `file_safety.py`, `url_safety` | `edgecrab-security` | EdgeCrab |
| **J5** Result shaping | Spill, summarize, turn budget | `tool_result_storage.py` | `artifact_spill.rs`, `turn_dispatch::finalize_tool_turn` | Tie (Hermes 3-layer mature) |
| **J6** Failure recovery | Structured errors → next action | `error_classifier.py` | `recovery_catalog.rs`, `provider_call.rs` | Hermes (unified classifier) |
| **J7** Cost / liveness | Compression, ctx budget, streaming | `context_compressor.py` | `compression.rs`, `provider_call.rs` | Tie |

---

## Scoring matrix (honest, code-derived)

```text
  Area                    Hermes   EdgeCrab   Notes
  ──────────────────────  ───────  ────────   ─────────────────────────────
  Loop modularity         ████░    ██░░░      Hermes split prologue/epilogue
  Security defaults       ███░░    █████      EdgeCrab port allowlist > global private
  Dev preview UX          ████░    ██░░░      Hermes allow_private_urls broader
  Completion types        ███░░    ████░      EdgeCrab RunOutcome + CompletionPolicy
  VERIFY enforcement      ██░░░    ██░░░      Both weak in production
  Guardrail armament      ██░░░    ████░      EdgeCrab hard-stop default ON
  Observability           ███░░    ████░      EdgeCrab harness.jsonl + doctor
  Provider breadth        █████    ████░      Hermes adapters more battle-tested
  Post-turn memory review ████░    ░░░░░      Hermes background_review only
```
