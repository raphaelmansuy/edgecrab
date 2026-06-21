# 003 — Main Loop Physics

**Cross-ref:** [004 turn lifecycle](./004-turn-lifecycle-prologue-epilogue.md) · [010 completion](./010-completion-truth-verify.md)

The harness **loop** is the contract between operator intent and model behavior. Everything else is plumbing.

---

## Loop condition (dual budget)

Both agents gate on **iteration count** AND an internal budget object:

| | Hermes | EdgeCrab |
|---|--------|----------|
| **Condition** | `while (api_call_count < max_iterations and iteration_budget.remaining > 0) or _budget_grace_call` | `while budget.try_consume()` inside `'conversation_loop` |
| **Default cap** | `max_iterations = 90` (`IterationBudget`) | `max_iterations = 90` (`IterationBudget`) |
| **Refund** | `iteration_budget.refund()` after `execute_code`-only batches | No equivalent refund path documented |
| **Grace call** | `_budget_grace_call` — one toolless summary attempt | Shadow judge / length recovery injects extra user msgs |
| **Owner** | `agent/conversation_loop.py` ~L589 | `conversation.rs::execute_loop` |

```text
  Per iteration:
  ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
  │ Budget gate  │ ──► │ Compress?    │ ──► │ API call     │
  └──────────────┘     └──────────────┘     └──────┬───────┘
                                                    │
                     ┌──────────────────────────────┴──────────────────────────────┐
                     │                                                             │
                     ▼                                                             ▼
              tool_calls present?                                          text only
                     │                                                             │
                     ▼                                                             ▼
              dispatch tools                                              assess completion
                     │                                                             │
                     └──────────────────────────► Continue ◄───────────────────────┘
                                                    (steer inject here)
```

---

## Per-iteration pipeline comparison

| Step | Hermes | EdgeCrab |
|------|--------|----------|
| 1. Cancel check | `_set_interrupt` polled in API + tools | `CancellationToken` raced in `api_call_with_retry` |
| 2. Message sanitize | `_sanitize_messages_*`, surrogate strip | Tool message sanitize in loop |
| 3. Compression trigger | `context_compressor.should_compress(last_prompt_tokens)` OR preflight in prologue | `check_compression_status_for_estimate` → `compress_with_llm` |
| 4. Provider call | `_interruptible_api_call` | `api_call_with_retry` |
| 5. Tool path | `_execute_tool_calls` → parallel/sequential | `process_response` → batch dispatch |
| 6. Text path | Append assistant msg; if no tools → exit loop region | `LoopAction::Done` → **provisional** `assess_completion` |
| 7. Steering | `_apply_pending_steer_to_tool_results` | `drain_pending_steers` at Continue boundary |
| 8. Loop continue | `api_call_count += 1`; `iteration_budget.consume()` | Implicit via `budget.try_consume()` |

---

## Exit paths (Q4: why did the run stop?)

### Hermes `_turn_exit_reason` taxonomy

Set throughout `run_conversation` and finalized in `finalize_turn`:

| Value | Trigger |
|-------|---------|
| `interrupted_by_user` | Cancel during wait or tools |
| `guardrail_halt` | `ToolGuardrailDecision.should_halt` |
| `partial_stream_recovery` | Dropped tool JSON mid-stream |
| `empty_response_exhausted` | Retries exhausted on empty assistant |
| `max_iterations_reached(N/M)` | Budget exhausted (may call `_handle_max_iterations`) |
| `text_response(finish_reason=…)` | Normal text completion |
| `all_retries_exhausted_no_response` | Provider failure |
| `context_overflow` recovery | Via classifier → compress branch |

**Code:** `agent/conversation_loop.py`, `agent/turn_finalizer.py`

### EdgeCrab `RunOutcome` + `ExitReason`

Typed enum consumed by TUI/gateway:

| `CompletionDecision` | When |
|---------------------|------|
| `Completed` | Gates pass + verification satisfied |
| `Incomplete` | Open todos, harness block, critical tool failure |
| `NeedsVerification` | VisualUx without browser/vision evidence |
| `NeedsInput` | Pending clarify/approval |
| `Failed` | Hard failure class |
| `Interrupted` | Cancel token |

**Code:** `completion_assessor.rs::DefaultCompletionPolicy::assess`, `edgecrab-types::RunOutcome`

### Critical divergence — mid-loop vs end-loop completion

```text
  HERMES                          EDGECRAB
  ─────────────────────────────   ─────────────────────────────────────────
  Model returns text              Model returns text
       │                               │
       ▼                               ▼
  Exit while loop                 LoopAction::Done
  (no re-open)                         │
                                       ▼
                                  assess_completion(provisional snapshot)
                                       │
                         ┌─────────────┴─────────────┐
                         │ Incomplete / NeedsVerif?  │
                         └─────────────┬─────────────┘
                              yes │         │ no
                                  ▼         ▼
                            inject follow-up  break
                            continue loop
```

**EdgeCrab advantage:** can re-open loop when assessor rejects premature "done".  
**EdgeCrab risk:** provisional snapshot may use `HarnessSnapshot::default()` — gates partially skipped mid-loop.

**Code anchor:** `conversation.rs` — `should_continue_after_model_text`, `build_completion_follow_up_message`

---

## Compression insertion points

| Trigger | Hermes | EdgeCrab |
|---------|--------|----------|
| Preflight (before 1st API) | `build_turn_context` — up to 3 passes on rough estimate | Inline at loop start (estimate-based) |
| Post-tool batch | `last_prompt_tokens` from provider usage | Token estimate after tools |
| On API error | `ClassifiedError.should_compress` | Partial — provider_call retries |
| Circuit breaker | Anti-thrashing (<10% savings × 2) | 3 LLM failures → structural-only |

---

## Iteration budget refund (Hermes-only nuance)

```python
# agent/iteration_budget.py
def refund(self) -> None:
    """Give back one iteration (e.g. for execute_code turns)."""
```

Hermes treats programmatic tool-calling (`execute_code`) as **not consuming** parent budget when the inner loop completes cleanly. EdgeCrab has no documented equivalent — subagent budgets are separate via delegation config.

---

## LoopAction (EdgeCrab-only control enum)

```rust
enum LoopAction {
    Continue,           // after tool batch
    Done(String),       // model text — triggers provisional assess
    PartialAbort { reason: String },  // invalid-tool budget exhaustion
}
```

Hermes has no equivalent typed enum — control flow is implicit `break`/`continue` with string `_turn_exit_reason`.

**First principle:** EdgeCrab's `LoopAction::Done` + provisional assess is **strictly more correct** than Hermes's immediate text exit — when the assessor is fully armed.
