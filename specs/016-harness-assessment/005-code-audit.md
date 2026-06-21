# 005 — Code Architecture Audit

**Cross-ref:** [004-hermes](./004-hermes-comparator.md) · [006-verdict](./006-brutal-verdict.md) · [015/004](../015-improve-harness-and-agent/004-code-anchors.md)

---

## Control-plane map

```text
execute_loop (conversation.rs ~L391)
│
├─ SETUP
│    resolve_tool_policy, wire tools, TurnDispatchTrackers::new
│    apply_visual_ux_session_preview (task_class.rs L107)  ← runtime preview ON
│    task_class_advisory footer
│
├─ LOOP 'conversation_loop (~L1250)
│    sanitize history, compress_with_llm
│    api_call_with_retry (provider_call.rs)
│    process_response → dispatch tools
│    finalize_tool_turn (turn_dispatch.rs)
│         guardrail_before_dispatch / apply_guardrail_result
│         apply_harness_advisories (harness_advisory.rs)
│         enforce_turn_budget
│    assess_completion w/ HarnessSnapshot::default() (~L2256)  ← BUG CLASS
│
└─ TEARDOWN
     build_harness_snapshot (harness_gates.rs)
     assess_completion (completion_assessor.rs)
     format_turn_completion_explanation (turn_completion.rs)
     progress_sink::emit_run_finished
```

---

## Module grades

### `conversation.rs` — **C+ (critical path, unsustainable)**

| Strength | Weakness |
|----------|----------|
| Full ReAct loop production-hardened | **~7,603 lines** — owns compression, provider, shadow judge, completion |
| Parallel JoinSet + path claiming | Mid-loop completion uses empty harness |
| Stream recovery paths | Learning reflection fire-and-forget outside observability |

**Verdict:** Extraction to `turn_dispatch.rs` / `provider_call.rs` started; **must finish** before next harness feature.

### `turn_dispatch.rs` — **A-**

| Strength | Weakness |
|----------|----------|
| Clean FP11 dedup, failure tracker | `halt_decision` from guardrails **never read** by loop |
| `finalize_tool_turn` single owner | Storm advisories injected but not blocking |
| Good unit test coverage | — |

### `completion_assessor.rs` — **B**

| Strength | Weakness |
|----------|----------|
| Pluggable `CompletionPolicy` | Only runs with full context at end |
| `markdown_theater_without_perception` (HA-43) | `report_task_status` evidence is prose-trust |
| VisualUx strict via `effective_verification_strict` | e22c0a28 still completed in logs |

### `harness_advisory.rs` — **B- (advisory only)**

| Strength | Weakness |
|----------|----------|
| HA-16 preview recovery → `/config` steer | Does not block tools |
| HA-20e iteration storm message | 120s rate limit; model ignores |
| Sliding window tool tracking | Same-turn only |

### `harness_analyzer.rs` — **D+ (broken on JSONL)**

Parses **plain text** patterns (`harness: tool start`) but production `harness.jsonl` is **tracing JSON**:

```json
{"fields":{"message":"harness: tool start","tool_name":"terminal",...}}
```

`extract_field` looks for `tool_name=` in flat string — **never matches JSON**. Doctor reports `tool_starts: 0` on 973-line log.

**Fix:** parse JSON lines first; fall back to plain text. Gate: **HA-44**.

### `tool_loop_guardrails.rs` — **B (unarmed)**

```rust
// Default — hard_stop_enabled: false
```

Block/Halt paths exist; production gets **Warn prose only**. `halt_decision` stored, not consumed.

### `harness_gates.rs` — **B+**

Deterministic `HarnessSnapshot::blocks_completion()` for mutation debt + JS syntax oracle.  
**Gap:** no cargo test / playwright / visual diff.

### `provider_call.rs` — **A-**

Stream assembly, stall detect, retry-after, Copilot heartbeat.  
**Gap:** no Hermes-grade `FailoverReason` taxonomy.

### `task_class.rs` — **B+**

`VisualUx` classification, session preview fallback, strict verify hook.  
**Gap:** classification is heuristic on first user message; no plan-graph coupling.

### CLI pipeline — **A-**

`turn_activity.rs` + `stream_forward.rs` + `response_dispatch.rs` — Hermes `turnController` parity.  
**Gap:** `TurnStreamHarness` ignores `RunFinished` in tests.

---

## Critical code defects (harness-blocking)

| ID | Defect | Location | Impact |
|----|--------|----------|--------|
| **C1** | Mid-loop `HarnessSnapshot::default()` | `conversation.rs` ~L2256 | Gates skipped on early text exit |
| **C2** | Guardrail halt not wired | `turn_dispatch.rs` → `conversation.rs` | Infinite terminal storms |
| **C3** | `harness_analyzer` JSONL blind | `harness_analyzer.rs` | Doctor lies to operator |
| **C4** | HA-41 merge not visible to doctor | `config.rs` + `doctor.rs` | Preview OFF despite global ON |
| **C5** | `hard_stop_enabled: false` default | `tool_loop_guardrails.rs` L73 | Loops never hard-break |

---

## What's genuinely good (do not refactor away)

1. **Strict tool schemas** + indexed wire set — ahead of Hermes advisory JSON.
2. **Artifact spill to `.edgecrab-artifacts/`** — operator-auditable.
3. **`recovery_catalog` structured JSON** — right steer pattern.
4. **`RunOutcome` + `CompletionDecision`** — correct long-term contract.
5. **Security mediation** — path jail + SSRF port allowlist is the right default.
6. **`harness_games003_replay.rs`** — law in CI; expand timelines.
7. **Streaming bus** — rich `StreamEvent` taxonomy; shelf is best-in-class for Rust agents.

---

## Monolith extraction status (P2.1)

| Extracted | Still in `conversation.rs` |
|-----------|---------------------------|
| `provider_call.rs` | compression notices, shadow judge |
| `turn_dispatch.rs` | `process_response` bulk, steering |
| `progress_sink.rs` | `assess_completion` mid-loop |
| `turn_completion.rs` (UX only) | harness advisory triggers, spill inline |

**Target owners per [015/007](../015-improve-harness-and-agent/007-architecture-target.md):** one module per harness job J1–J7.
