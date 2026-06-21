# 005 — Tool Dispatch & Parallelism

**Cross-ref:** [006 guardrails](./006-guardrails-stall-breakers.md) · [009 spill](./009-spill-turn-budget-results.md)

Tool dispatch is where harness **J2 (dispatch)** and **J3 (pre-validation)** meet **J4 (effect mediation)**.

---

## Dispatch routing

| | Hermes | EdgeCrab |
|---|--------|----------|
| **Router** | `AIAgent._execute_tool_calls` | `process_response` in `conversation.rs` |
| **Parallel gate** | `_should_parallelize_tool_batch` (path overlap / mutating tools) | Batch parallel dispatch when safe |
| **Max workers** | `_MAX_TOOL_WORKERS = 8` | Tokio concurrent futures (registry-dependent) |
| **Sequential path** | `execute_tool_calls_sequential` — inline routes for todo/memory/delegate | Sequential fallback on conflict |
| **Implementation** | `agent/tool_executor.py` | `turn_dispatch.rs` + inline in `conversation.rs` |

```text
  Assistant message with tool_calls[N]
           │
           ▼
  ┌────────────────────┐
  │ Pre-dispatch batch │  parse JSON, middleware, plugin block
  └─────────┬──────────┘
            │
     ┌──────┴──────┐
     │ parallel OK?│
     └──────┬──────┘
       yes  │  no
            ▼  ▼
     ThreadPool    Sequential loop
     (max 8)       (interrupt between tools)
            │  │
            └──┴──► per-tool pipeline (below)
```

---

## Per-tool pipeline (ordered)

| Stage | Hermes (`tool_executor.py`) | EdgeCrab (`turn_dispatch.rs` + tools) |
|-------|----------------------------|---------------------------------------|
| 1. Tool Search unwrap | Scope check via `_tool_search_scoped_names` | `tool_search` BM25 + schema index |
| 2. Request middleware | Plugin + approval gates | Clarify/approval in `RunProgressState` |
| 3. Pre-dispatch block | `tool_guardrails.before_call` | `guardrail_before_dispatch_checked` + advisories |
| 4. Arg budget | Implicit in tools | `check_tool_argument_budget` (`mutation_turn_policy`) |
| 5. Execute | `handle_function_call` / `_invoke_tool` | `ToolRegistry::dispatch` |
| 6. Spill L2 | `maybe_persist_tool_result` | `maybe_spill` |
| 7. Post-dispatch | `tool_guardrails.after_call` + guidance append | `apply_guardrail_result` |
| 8. Post-tool hook | `_emit_post_tool_call_hook` | Stream events + OTEL |
| 9. Steer inject | `_apply_pending_steer_to_tool_results` | Steering at Continue boundary |
| 10. Turn budget L3 | `enforce_turn_budget` (batch end) | `finalize_tool_turn` → `enforce_turn_budget` |

---

## Pre-dispatch blocks (EdgeCrab extensions)

EdgeCrab adds harness advisories **before** guardrails — Hermes has no dedicated module:

| Block | EdgeCrab function | Condition |
|-------|-------------------|-----------|
| Visual storm | `visual_storm_block_result` | VisualUx + act tools without perception |
| Repeated browser nav | `maybe_repeated_browser_nav_block` | N failed navigations in window |
| Verification theater | `maybe_verification_theater_block` | Markdown verify docs without browser |

**Code:** `harness_advisory.rs`, `harness_loop_policy.rs`

Hermes relies on guardrails + operator config only for these patterns.

---

## Parallel safety model

### Hermes `_should_parallelize_tool_batch`

Considers:
- Path overlap on file mutations
- Destructive terminal commands
- Tools that require sequential VM/browser state

Falls back to `execute_tool_calls_sequential` when any tool in batch is mutating or shares resources.

### EdgeCrab

Parallel dispatch when tool batch has no ordering dependency; file tools go through path safety independently. `DuplicateToolCallDetector` (FP11) blocks cross-turn identical calls.

---

## Special inline dispatch (Hermes sequential path)

Hermes sequential executor has **fast paths** for agent-runtime tools without full `handle_function_call` round-trip:

- `todo` / `memory` / `delegate_task`
- Context-engine tools

EdgeCrab routes all tools through `ToolRegistry::dispatch` uniformly — simpler, slightly higher overhead.

---

## Interrupt semantics

| Event | Hermes | EdgeCrab |
|-------|--------|----------|
| Cancel during tool | `_cancelled_tool_result` JSON | Cancel token → tool error |
| Cancel during parallel pool | Future cancel + partial results | Tokio task abort |
| STOP steer | Sets interrupt + tool cancel | `SteeringKind::Stop` → cancel token |

**Code:** Hermes `tool_executor._cancelled_tool_result`; EdgeCrab `steering.rs` + `CancellationToken`

---

## Turn finalization (`finalize_tool_turn`)

EdgeCrab centralizes post-batch work in one async function:

```rust
// turn_dispatch.rs
pub async fn finalize_tool_turn(
    trackers: &mut TurnDispatchTrackers,
    params: ToolTurnFinalizeParams<'_>,
)
```

Steps:
1. `apply_harness_advisories` — record tools, inject one-shot `[harness]` user msgs
2. `enforce_turn_budget` — spill largest results until under char cap
3. Invalid-args recovery messages
4. `consume_guardrail_halt_message` — inject halt steer if set

Hermes equivalent is inline at end of `execute_tool_calls_*` — same three layers, no named function.

---

## First-principle comparison

| Principle | Hermes | EdgeCrab |
|-----------|--------|----------|
| Uniform dispatch | No — inline fast paths | Yes — registry only |
| Pre-dispatch defense depth | Guardrails + plugins | Guardrails + advisories + arg budget + recovery catalog |
| Spill integration | Mature 3-layer in executor | Mature 3-layer in finalize_tool_turn |
| Parallel cap | Explicit 8 threads | Async pool (no hard cap in dispatch module) |

**Leader:** Tie on mechanism; EdgeCrab ahead on pre-dispatch typed recovery; Hermes ahead on sequential fast paths for hot tools.
