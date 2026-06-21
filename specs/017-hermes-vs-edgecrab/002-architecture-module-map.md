# 002 — Architecture Module Map

**Cross-ref:** [003 loop](./003-main-loop-physics.md) · [011 borrow matrix](./011-borrow-reject-matrix.md)

---

## Stack shape (side by side)

```text
  HERMES (Python)                              EDGECRAB (Rust)
  ═══════════════════════════════════════      ═══════════════════════════════════════

  run_agent.py                                 crates/edgecrab-cli / gateway
       │                                            │
       ▼                                            ▼
  AIAgent (god-object facade)                  Agent + AgentBuilder
       │                                            │
       ├── conversation_loop.py (~4.5k)       ├── conversation.rs (~7.6k)  ← monolith
       ├── turn_context.py (~408)            │     (prologue inline)
       ├── turn_finalizer.py (~428)          ├── turn_completion.rs (~128) ← UX only
       ├── tool_executor.py (~1.4k)           ├── turn_dispatch.rs (~341)
       ├── tool_guardrails.py (~475)         │     └── edgecrab-tools guardrails
       ├── error_classifier.py (~1.4k)        ├── provider_call.rs (~1.5k)
       ├── context_compressor.py (~2.5k)      ├── compression.rs (~2.5k)
       ├── conversation_compression.py (~1k)  │     (no session lineage yet)
       └── background_review.py (~735)        └── (not ported)

  tools/tool_result_storage.py                 edgecrab-tools/artifact_spill.rs
  tools/budget_config.py                       config: result_turn_budget_chars
```

---

## Entry-point cross-ref

| Concern | Hermes | EdgeCrab |
|---------|--------|----------|
| Public turn API | `AIAgent.run_conversation` → `conversation_loop.run_conversation` | `Agent::run_conversation` → `execute_loop` |
| Single-shot chat | `AIAgent.chat` | `Agent::chat` |
| Streaming | `stream_callback` kwarg | `chat_streaming` + `StreamEvent` channel |
| Cancel | `agent.interrupt()` | `agent.interrupt()` → `CancellationToken` |
| Mid-turn steer | `agent.steer()` | `SteeringEvent` channel |
| Subagent | `delegate_task` + forked `AIAgent` | `sub_agent_runner.rs` |

---

## Module ownership table

| Harness job | Hermes file | EdgeCrab file | Parity |
|-------------|-------------|---------------|--------|
| Main loop | `agent/conversation_loop.py` | `conversation.rs::execute_loop` | Mechanism parity; structure differs |
| Turn prologue | `agent/turn_context.py::build_turn_context` | Inline top of `execute_loop` | **Gap** — extract target |
| Turn epilogue | `agent/turn_finalizer.py::finalize_turn` | End of `execute_loop` + `turn_completion.rs` | **Gap** — logic split |
| Tool dispatch | `agent/tool_executor.py` | `turn_dispatch.rs` + `process_response` in `conversation.rs` | Parity |
| Guardrails | `agent/tool_guardrails.py` | `tool_loop_guardrails.rs` + `harness_loop_policy.rs` | Ported; defaults differ |
| Harness advisories | (none dedicated) | `harness_advisory.rs` | EdgeCrab-only |
| Error taxonomy | `agent/error_classifier.py` | `provider_call.rs` (partial) | **Gap** |
| Compression algo | `agent/context_compressor.py` | `compression.rs` | Strong parity |
| Compression session | `agent/conversation_compression.py` | (messages only) | **Gap** — no parent_session_id |
| Spill L2 | `tools/tool_result_storage.py::maybe_persist_tool_result` | `artifact_spill.rs::maybe_spill` | Parity |
| Spill L3 | `tools/tool_result_storage.py::enforce_turn_budget` | `artifact_spill.rs::enforce_turn_budget` | Parity |
| Completion assess | `_turn_exit_reason` + finalizer heuristics | `completion_assessor.rs` | EdgeCrab stronger types |
| Deterministic gates | finalizer file-mutation footer | `harness_gates.rs` → `HarnessSnapshot` | EdgeCrab stronger |
| Post-mortem | none in core | `harness_analyzer.rs` | EdgeCrab-only |
| Background review | `agent/background_review.py` | none | Hermes-only |

---

## Dependency flow (ASCII)

### Hermes

```text
  run_conversation
       │
       ├─► build_turn_context ──► preflight compress, MCP refresh, plugin hook
       │
       ├─► while (api_call_count < max_iterations
       │         AND iteration_budget.remaining > 0)
       │       │
       │       ├─► _interruptible_api_call
       │       │        └─► classify_api_error → retry/failover/compress
       │       │
       │       └─► _execute_tool_calls
       │                └─► tool_executor (parallel|sequential)
       │                     ├─► maybe_persist_tool_result
       │                     ├─► enforce_turn_budget
       │                     └─► tool_guardrails before/after
       │
       └─► finalize_turn ──► persist, footer, background_review spawn
```

### EdgeCrab

```text
  execute_loop
       │
       ├─► [inline prologue] lock, budget reset, config snapshot
       │
       ├─► 'conversation_loop: while budget.try_consume()
       │       │
       │       ├─► compress_with_llm (if threshold)
       │       │
       │       ├─► api_call_with_retry
       │       │
       │       └─► process_response
       │                ├─► guardrail_before_dispatch_checked
       │                ├─► registry.dispatch (parallel batch)
       │                └─► finalize_tool_turn
       │                     ├─► apply_harness_advisories
       │                     └─► enforce_turn_budget
       │
       ├─► match LoopAction::Done → provisional assess_completion
       │       └─► should_continue_after_model_text? → re-open loop
       │
       └─► final assess_completion → format_turn_completion_explanation
```

---

## Line-count reality (maintainability signal)

| Module | Hermes LOC | EdgeCrab LOC | Note |
|--------|-----------|--------------|------|
| Main loop | ~4,561 | ~7,612 | EdgeCrab concentration risk |
| Turn prologue | ~408 (extracted) | ~0 (inline) | Hermes ahead |
| Turn epilogue | ~428 (extracted) | ~128 UX + inline | Hermes ahead |
| Tool executor | ~1,442 | ~341 dispatch + loop inline | Split differently |
| Error classifier | ~1,365 | ~1,549 provider_call | Different shape |
| Context compressor | ~2,508 | ~2,471 | Parity |

**First principle:** harness complexity is inevitable; **module boundaries** determine whether it stays operable.
