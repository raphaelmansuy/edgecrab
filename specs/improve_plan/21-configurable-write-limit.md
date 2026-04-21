# 21 — Configurable Write Limit + Compression Dedup Reset (FP16+FP17)

> **STATUS: FP16 IMPLEMENTED** ✅ — Configurable write limit with env override, clamping, backward compat.
> **STATUS: FP17 NOT YET IMPLEMENTED** — Compression dedup reset pending.

> G15: Make 32 KiB write limit configurable.
> G16: Reset read tracker after compression.
> Cross-ref: Hermes `reset_file_dedup()`, edgecrab `edit_contract.rs`

---

## Part 1: Configurable Write Limit (FP16)

### WHY

```
"Defaults protect, overrides empower"
```

The 32 KiB limit is correct for most providers (Anthropic, OpenAI, Google).
But:
- **Local LLMs** (Ollama, vLLM): No payload limit, streaming is local
- **High-bandwidth providers**: Can handle 64-128 KiB reliably
- **Power users**: Know their provider's limits, want to push

**Current state**: Hard-coded constant. No override path.

```
+----------------------------------------------------------------------+
|  CURRENT (Rigid):                                                    |
|                                                                      |
|    edit_contract.rs:  const MAX = 32 * 1024;  // Can't change        |
|    config.yaml:       (no option)                                    |
|                                                                      |
|  PROPOSED (Flexible):                                                |
|                                                                      |
|    config.yaml:       max_write_payload_kib: 32  // Default          |
|    edit_contract.rs:  reads from ToolContext.config                   |
|    env override:      EDGECRAB_MAX_WRITE_PAYLOAD_KIB=64              |
|                                                                      |
|  SAFETY: Minimum 8 KiB (prevent accidental 0), maximum 256 KiB      |
|  (prevent obviously broken values).                                  |
|                                                                      |
+----------------------------------------------------------------------+
```

### Implementation

| File | Change |
|------|--------|
| `edgecrab-core/src/config.rs` | Add `max_write_payload_kib: u32` to `AppConfig` with default 32 |
| `edgecrab-tools/src/edit_contract.rs` | Change functions to accept `max_bytes: usize` parameter |
| `edgecrab-tools/src/tools/file_write.rs` | Pass `ctx.config.max_write_payload_bytes()` to enforce fn |
| `edgecrab-tools/src/tools/file_patch.rs` | Pass `ctx.config.max_write_payload_bytes()` to enforce fn |
| `edgecrab-core/src/prompt_builder.rs` | Read limit from config instead of constant |

### API Change

```rust
// BEFORE (edit_contract.rs):
pub const MAX_MUTATION_PAYLOAD_BYTES: usize = 32 * 1024;

pub fn enforce_write_payload_limit(
    tool_name: &str, path: &str, resolved: &Path, content: &str,
) -> Result<(), ToolError>;

// AFTER:
pub const DEFAULT_MAX_MUTATION_PAYLOAD_BYTES: usize = 32 * 1024;
pub const MIN_MUTATION_PAYLOAD_BYTES: usize = 8 * 1024;
pub const MAX_MUTATION_PAYLOAD_BYTES_CAP: usize = 256 * 1024;

pub fn enforce_write_payload_limit(
    tool_name: &str, path: &str, resolved: &Path, content: &str,
    max_bytes: usize,
) -> Result<(), ToolError>;
```

### Edge Cases

| Case | Handling |
|------|----------|
| Config value < 8 KiB | Clamp to 8 KiB minimum |
| Config value > 256 KiB | Clamp to 256 KiB maximum |
| Config not set | Use 32 KiB default |
| Env override conflicts with config | Env wins (standard precedence) |
| Prompt builder references limit | Reads from config, not constant |

### Tests

```
test_configurable_limit_allows_larger_writes
test_configurable_limit_clamps_minimum
test_configurable_limit_clamps_maximum
test_default_limit_unchanged_without_config
test_env_override_takes_precedence
```

---

## Part 2: Compression Dedup Reset (FP17)

### WHY

```
"Compression must not poison read cache"
```

After context compression, old messages are replaced with a summary.
But the read tracker still has snapshots from those old messages.
If the model re-reads a file after compression, the tracker says
"already read, unchanged" — but the content is no longer in context.

```
+----------------------------------------------------------------------+
|  BEFORE COMPRESSION:                                                 |
|    Context: [..., read_file(main.rs) -> content, ...]                |
|    Read cache: {main.rs: mtime_1}                                    |
|                                                                      |
|  AFTER COMPRESSION:                                                  |
|    Context: [SUMMARY: "read main.rs...", ...recent...]               |
|    Read cache: {main.rs: mtime_1}  <-- STALE!                       |
|                                                                      |
|  Model asks to re-read main.rs:                                      |
|    Dedup says "unchanged" -> returns stub                            |
|    But content is NOT in context anymore!                             |
|                                                                      |
|  FIX: Reset read cache after compression.                            |
|                                                                      |
+----------------------------------------------------------------------+
```

### Implementation

| File | Change |
|------|--------|
| `read_tracker.rs` | Add `pub fn reset_session_snapshots(session_id: &str)` |
| `conversation.rs` | Call `reset_session_snapshots()` after successful compression |

### Tests

```
test_dedup_reset_after_compression_allows_reread
test_dedup_reset_only_affects_target_session
```

---

## Combined Estimated Impact

| Metric | Before | After |
|--------|--------|-------|
| Write limit flexibility | None | Config + env override |
| Post-compression stale reads | Possible | Prevented |
| Power user satisfaction | Frustrated by 32 KiB | Can override |
