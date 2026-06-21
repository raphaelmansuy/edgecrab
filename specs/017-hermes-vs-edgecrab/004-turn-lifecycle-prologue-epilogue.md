# 004 — Turn Lifecycle (Prologue / Epilogue)

**Cross-ref:** [002 architecture](./002-architecture-module-map.md) · [003 loop](./003-main-loop-physics.md)

A turn = **prologue** (once) + **loop** (N iterations) + **epilogue** (once). Hermes extracted prologue/epilogue; EdgeCrab still embeds most of this in `execute_loop`.

---

## Side-by-side lifecycle

```text
  HERMES                              EDGECRAB
  ══════                              ════════

  build_turn_context()                [inline in execute_loop]
       │                                   │
       ├─ stdio guard                       ├─ conversation_lock acquire
       ├─ MCP between-turn refresh         ├─ cancel token reset
       ├─ retry counter reset               ├─ config/provider snapshot
       ├─ guardrails.reset_for_turn()      ├─ TurnDispatchTrackers::with_harness
       ├─ IterationBudget reset             ├─ IterationBudget reset
       ├─ todo/nudge hydrate                ├─ goal block inject (messages)
       ├─ system prompt restore/build       ├─ cached_system_prompt restore
       ├─ preflight compression (×3)        ├─ (compression in loop body)
       ├─ pre_llm_call plugin hook          ├─ plugin hooks via provider_call
       └─ memory prefetch                   └─ memory via prompt_builder (session start)

  run_conversation while loop           'conversation_loop while
       │                                   │
       └─ ...                              └─ ...

  finalize_turn()                       [inline + turn_completion.rs]
       │                                   │
       ├─ budget exhaustion summary         ├─ final assess_completion
       ├─ _handle_max_iterations            ├─ gate footer inject
       ├─ drop empty scaffolding            ├─ format_turn_completion_explanation
       ├─ persist session                   ├─ persist session (state DB)
       ├─ file-mutation verifier footer     ├─ StreamEvent::RunFinished
       ├─ turn completion explainer         └─ observability export
       ├─ plugin post hooks
       └─ spawn background_review
```

---

## Prologue deep-dive

### Hermes `TurnContext` (`agent/turn_context.py`)

```python
@dataclass
class TurnContext:
    user_message: str
    original_user_message: Any
    messages: List[Dict[str, Any]]
    conversation_history: Optional[List[Dict[str, Any]]]
    active_system_prompt: Optional[str]
    effective_task_id: str
    turn_id: str
    current_turn_user_idx: int
    should_review_memory: bool = False
    plugin_user_context: str = ""
    ext_prefetch_cache: str = ""
```

**Key behaviors (code-is-law):**

| Behavior | Hermes | EdgeCrab equivalent |
|----------|--------|---------------------|
| MCP late-connect refresh | `refresh_agent_mcp_tools` in prologue | MCP reload on `/reload-mcp`; no between-turn auto-refresh |
| Surrogate sanitize | `sanitize_surrogates(user_message)` | Unicode handling in message path |
| Guardrail reset | `agent._tool_guardrails.reset_for_turn()` | `ToolLoopGuardrailController::reset_for_turn` per tool turn |
| Preflight compress | Up to 3 passes; defers if real usage fits | Single estimate gate per loop iter |
| Plugin injection | `pre_llm_call` → append to user msg | `pre_api_request` hook in `provider_call.rs` |
| External memory prefetch | `memory_manager.prefetch_all` → cache | Honcho tools; no equivalent prefetch block |

### EdgeCrab prologue gap

EdgeCrab performs turn setup **inside** `execute_loop` (~first 200 lines) without a named `TurnContext` struct. Side effects (lock, budget, trackers) are correct but **hard to test in isolation**.

**Borrow target:** extract `turn_prologue.rs` returning a `TurnContext`-like struct (spec 016 item P2.1).

---

## Epilogue deep-dive

### Hermes `finalize_turn` (`agent/turn_finalizer.py`)

| Step | Purpose |
|------|---------|
| Budget exhausted | If no `final_response` → `_handle_max_iterations` (toolless summary API call) |
| Kanban worker | Records `timed_out` failure via `_record_task_failure` |
| Trajectory save | `_save_trajectory` if enabled |
| Task cleanup | `_cleanup_task_resources` (VM, browser) |
| Persist hygiene | `_drop_trailing_empty_response_scaffolding` before `_persist_session` |
| Diagnostic log | WARNING when last msg is `role=tool` ("just stops" scenario) |
| File-mutation footer | Verifier appended to response when writes occurred |
| Turn explainer | `_format_turn_completion_explanation` for empty/partial |
| Background review | `_spawn_background_review` daemon thread |

### EdgeCrab epilogue

| Step | Owner |
|------|-------|
| Final `build_harness_snapshot` | `conversation.rs` end |
| `assess_completion` | `completion_assessor.rs` |
| Gate footer | Injected into messages if mutation debt |
| `format_turn_completion_explanation` | `turn_completion.rs` (UX only — no persist side effects) |
| `StreamEvent::RunFinished` | Emitted with `RunOutcome` |
| Background review | **Not implemented** |

### Divergence — budget exhaustion

```text
  HERMES                                    EDGECRAB
  ─────────────────────────────────────     ─────────────────────────────────────
  max_iterations hit                        max_iterations hit
       │                                         │
       ▼                                         ▼
  _handle_max_iterations()                  Loop exits; assess_completion
  (extra toolless API call for summary)     may return Incomplete/BudgetExhausted
       │                                         │
       ▼                                         ▼
  User gets model-written summary           User gets harness explainer string
```

**Code:** Hermes `turn_finalizer.py` L53–70; EdgeCrab `completion_assessor.rs` budget branch.

### Divergence — "stopped mid-tool" honesty

Hermes logs at **WARNING** when `messages[-1].role == "tool"` — the operator-visible "agent just stopped" bug.

EdgeCrab has `count_unanswered_tool_calls` in `turn_completion.rs` but does **not** hard-gate exit on orphaned tool_calls.

**Borrow:** surface unanswered tool_calls as `CompletionDecision::Incomplete` (Hermes diagnostic → EdgeCrab gate).

---

## Background review (Hermes-only epilogue extension)

```text
  Parent turn completes
       │
       ▼
  finalize_turn spawns daemon thread
       │
       ▼
  Forked AIAgent (max_iterations=16, compression_enabled=False)
       │
       ├─ tool whitelist: memory + skill_manage only
       ├─ stdout → /dev/null
       └─ run_conversation(review_prompt, inherited snapshot)
```

**Code:** `agent/background_review.py::spawn_background_review_thread`

EdgeCrab has no post-turn forked reviewer. Memory/skill curation is inline or manual via slash commands.

---

## First-principle verdict

| Aspect | Hermes | EdgeCrab |
|--------|--------|----------|
| Prologue testability | High (pure dataclass output) | Low (inline) |
| Epilogue testability | High (single function) | Medium (split loop end + turn_completion) |
| Operator honesty | Strong diagnostics + summary on budget hit | Strong typed outcome; weaker mid-tool stop |
| Post-turn automation | Background memory/skill review | None |

**EdgeCrab should borrow:** prologue/epilogue module split + budget-exhaustion summary call + unanswered tool_call gate.  
**EdgeCrab should keep:** typed `RunOutcome` instead of string-only `_turn_exit_reason`.
