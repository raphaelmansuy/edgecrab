# 009 — Spill & Turn Budget (Tool Results)

**Cross-ref:** [005 dispatch pipeline](./005-tool-dispatch-and-parallelism.md) · [007 compression prune](./007-compression-context-budget.md)

Large tool outputs are a **context bomb**. Both agents implement a **three-layer defense**.

---

## Three-layer stack (identical shape)

```text
  Layer 1 — Tool-internal cap
  ─────────────────────────────
  Each tool truncates its own output before returning
  (terminal tail, read_file limits, web extract caps)

           │
           ▼
  Layer 2 — Per-result spill (threshold)
  ─────────────────────────────────────
  Single tool result > threshold → persist to disk, return stub

           │
           ▼
  Layer 3 — Turn aggregate budget
  ───────────────────────────────
  Sum of all tool results in turn > turn_budget → spill largest first
```

---

## Side-by-side implementation

| Layer | Hermes | EdgeCrab |
|-------|--------|----------|
| **L1** | Per-tool (e.g. terminal tail) | Per-tool (same pattern) |
| **L2** | `maybe_persist_tool_result` | `maybe_spill` |
| **L3** | `enforce_turn_budget` | `enforce_turn_budget` |
| **Config** | `tools/budget_config.py` | `SpillConfig` + `result_turn_budget_chars` |
| **Storage path** | `{env_temp}/hermes-results/{tool_use_id}.txt` | `.edgecrab-artifacts/{session}/` |
| **Stub tag** | `<persisted-output>` | `[tool_result_spill]` |
| **Write method** | stdin pipe to sandbox (avoids 128KB argv) | Direct filesystem write |

---

## Default thresholds (code)

| Constant | Hermes | EdgeCrab |
|----------|--------|----------|
| Per-result threshold | 100,000 chars (`DEFAULT_RESULT_SIZE_CHARS`) | Config `spill.threshold` |
| Turn budget | 200,000 chars (`DEFAULT_TURN_BUDGET_CHARS`) | `result_turn_budget_chars` (config) |
| Preview size | 1,500 chars (`DEFAULT_PREVIEW_SIZE_CHARS`) | `preview_lines` in SpillConfig |
| read_file pin | **∞** (`PINNED_THRESHOLDS`) — anti loop | Equivalent pin intent |

**Hermes code:** `tools/budget_config.py`  
**EdgeCrab code:** `artifact_spill.rs`, `budget_config.rs`

---

## When spill runs

| Event | Hermes | EdgeCrab |
|-------|--------|----------|
| After each tool | `maybe_persist_tool_result` in executor | `maybe_spill` in dispatch path |
| After tool batch | `enforce_turn_budget` at batch end | `finalize_tool_turn` → `enforce_turn_budget` |
| During compression prune | Via compressor prune | `prune_tool_outputs` may spill |
| Re-read spill artifact | Inline (not re-spilled) | Inline (not re-spilled) |
| computer_use | Spill rules apply | **Never spilled** |

---

## Stub message shape

### Hermes

```text
<persisted-output>
Preview: first ~1500 chars at newline boundary...
Full output: /tmp/hermes-results/call_abc123.txt
</persisted-output>
```

### EdgeCrab

```text
[tool_result_spill]
Preview (N lines):
...
Artifact: .edgecrab-artifacts/{session}/{seq}.txt
Read with read_file using offset/limit — do not rewrite from memory.
```

**First principle:** stub must include **actionable path + read recipe** — both do.

---

## Spill blindness (shared anti-pattern)

```text
  ANTI-PATTERN AP3 (both agents):
  ┌─────────────────────────────────────────────────────────────┐
  │  Tool spills → model writes NEW file from memory instead    │
  │  of read_file(artifact_path)                                │
  └─────────────────────────────────────────────────────────────┘

  HERMES:  no offline detector
  EDGECRAB: harness_analyzer.spill_without_read metric
```

**Detection:** EdgeCrab `harness_analyzer.rs` counts spill events without subsequent `read_file` on artifact path.

---

## Turn budget algorithm (L3)

Both sort non-persisted tool results by size descending, spill largest until under budget:

```python
# Hermes tool_result_storage.enforce_turn_budget — conceptual
for result in sorted_by_size_desc(tool_results):
    if total_chars <= config.turn_budget: break
    maybe_persist(result)
```

```rust
// EdgeCrab artifact_spill.enforce_turn_budget
pub fn enforce_turn_budget(messages, turn_budget_chars, spill_config, ...)
```

---

## Explicit non-borrows

| Pattern | Why |
|---------|-----|
| Auto-inject full spill into context post-compress | Turn budget explosion |
| Hermes global spill path in argv | EdgeCrab direct write is simpler on native OS |
| Lower read_file pin below ∞ | Causes read_file spill loops |

---

## First-principle verdict

| | Hermes | EdgeCrab |
|---|--------|----------|
| Maturity | Battle-tested; stdin pipe for sandbox | Parity shipped; clearer artifact dir |
| Transparency | `<persisted-output>` tag | `[tool_result_spill]` + session subdir |
| Offline analysis | None | spill_without_read in doctor |
| computer_use handling | Standard spill | Explicit never-spill rule |

**Leader:** Tie on mechanism; EdgeCrab ahead on observability.
