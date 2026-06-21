# 006 — Guardrails & Stall Breakers

**Cross-ref:** [005 dispatch](./005-tool-dispatch-and-parallelism.md) · [001 rubric Q2](./001-first-principles-rubric.md)

Guardrails answer **Q2: is forward progress real?** Both agents ported the same controller shape; **defaults diverge**.

---

## Shared controller model

Both implement `ToolCallGuardrailController` with identical action enum semantics:

| Action | Model sees | Loop effect |
|--------|------------|-------------|
| `allow` | Normal result | Continue |
| `warn` | Result + guidance suffix | Continue |
| `block` | Synthetic error JSON | Skip execution (before_call) |
| `halt` | Guidance + steer | Break loop / inject user msg |

**Hermes:** `agent/tool_guardrails.py`  
**EdgeCrab:** `crates/edgecrab-tools/src/tool_loop_guardrails.rs`

---

## Default configuration (CRITICAL DIVERGENCE)

| Setting | Hermes default | EdgeCrab default |
|---------|----------------|------------------|
| `hard_stop_enabled` | **`false`** (`hermes_cli/config.py`) | **`true`** via `HarnessConfig.guardrails_hard_stop: true` |
| `exact_failure_block_after` | 4 (when hard stop on) | 4 |
| `same_tool_failure_halt_after` | 6 (when hard stop on) | 6 |
| `no_progress_block_after` | 3 idempotent repeats | 3 idempotent repeats |

```text
  OPERATOR IMPACT
  ───────────────────────────────────────────────────────────────────
  Hermes out-of-box:  warnings appended, loops CAN continue indefinitely
  EdgeCrab out-of-box: hard blocks + halts fire on failure storms
  ───────────────────────────────────────────────────────────────────
```

**Code anchors:**
- Hermes: `agent/tool_guardrails.py` L73 `hard_stop_enabled: bool = False`
- EdgeCrab: `config.rs` `guardrails_hard_stop: true` → `harness_loop_policy.rs::resolve_guardrail_config`

---

## Tracking dimensions

Both track three independent stall signals:

```text
  ┌─────────────────────────────────────────────────────────────┐
  │  1. Exact repeat failure   (tool_name + args_hash)          │
  │  2. Same-tool failure streak (varying args)                 │
  │  3. Idempotent no-progress   (same tool + same result hash) │
  └─────────────────────────────────────────────────────────────┘
         │                    │                    │
         ▼                    ▼                    ▼
    before_call: block   after_call: warn/halt   before_call: block
    (if hard_stop)       (if hard_stop)          (if hard_stop)
```

**Idempotent tool list (both):** `read_file`, `web_search`, `browser_navigate`, MCP read tools, etc.

---

## EdgeCrab-only: HarnessTurnAdvisory

EdgeCrab adds a **60-second sliding window** advisory layer Hermes lacks:

| Advisory | Trigger | Effect |
|----------|---------|--------|
| HA-16 preview recovery | CDP/SSRF block on localhost | Inject `[harness]` steer with `/config` hint |
| HA-20e iteration storm | ≥N act tools, 0 perception | Warn only |
| Repeated browser nav block | Failed navigations ≥ threshold | **Hard block** further nav |
| Verification theater block | Markdown verify without browser | **Hard block** write/patch |

**Code:** `harness_advisory.rs`

### Visual storm hard block

```rust
// harness_loop_policy.rs
const STORM_BLOCK_TOOLS: &[&str] = &["terminal", "run_process", "execute_code"];

pub fn visual_storm_block_result(...) -> Option<String>
```

Blocks shell/code on `TaskClass::VisualUx` when act tools fire without browser/vision in window.

Hermes has **no equivalent** task-class-aware pre-dispatch block.

---

## Halt → loop integration

| | Hermes | EdgeCrab |
|---|--------|----------|
| Halt detection | `ToolGuardrailDecision.should_halt` in loop | `take_halt_decision()` in `finalize_tool_turn` |
| Loop exit | Sets `_turn_exit_reason = "guardrail_halt"` | Injects `[harness] Tool loop halted…` user msg |
| Synthetic response | `_guardrail_block_result` | `guardrail_block_result` JSON |

```text
  Hermes halt path:
  after_call → halt decision → append observation → break while loop

  EdgeCrab halt path:
  after_call → store halt_decision → finalize_tool_turn → user message → Continue
               (may allow one more API iteration with steer)
```

Subtle behavioral difference: EdgeCrab gives the model **one steered iteration** after halt; Hermes may break immediately.

---

## Empty tool name dampening (Hermes)

Hermes has dedicated loop dampening for empty tool names from streaming providers:

**Code:** `tests/agent/test_empty_tool_name_loop_dampening.py`, logic in `conversation_loop.py`

EdgeCrab handles via stream assembly in `provider_call.rs` — partial tool call finalization.

---

## Offline stall analysis (EdgeCrab-only)

`harness_analyzer.rs` parses `harness.jsonl` for post-mortem:

| Metric | Meaning |
|--------|---------|
| `terminal_without_perception` | Act storm without browser/vision |
| `spill_without_read` | Model blind to spilled artifacts |
| `last_exit_reason` / `last_decision` | Terminal state |

Hermes has no equivalent in-core doctor command.

---

## First-principle verdict

| | Hermes | EdgeCrab |
|---|--------|----------|
| Philosophy | Permissive default — trust operator to enable hard stops | Aggressive default — stop useless loops on cheap models |
| Task-class awareness | None in guardrails | VisualUx storm blocks |
| Extensibility | Config-only | Config + advisory module |
| Production armament | Requires `hard_stop_enabled: true` in config | Armed by default |

**Neither wins outright:** Hermes better for exploratory dev; EdgeCrab better for autonomous long runs on local models.
