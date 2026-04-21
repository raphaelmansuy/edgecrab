# 18 — Read Dedup + Consecutive Loop Detection (FP13)

> G11 + G12: Prevent wasted context tokens from redundant re-reads.
> Cross-ref: Hermes `_read_tracker` + `_consecutive_read_tracker`

---

## WHY (First Principle FP13)

```
"Don't re-read what hasn't changed"
```

Every `read_file` call injects file content into the conversation context.
If the file hasn't changed since the last read, those tokens are wasted —
the model already has the content. Worse, if the model is stuck in a loop
calling `read_file` with identical arguments, it burns budget indefinitely.

**Two distinct sub-problems:**

```
+----------------------------------------------------------------------+
|  SUB-PROBLEM 1: Redundant Re-Read (Dedup)                           |
+----------------------------------------------------------------------+
|                                                                      |
|  Turn 5:  read_file("src/main.rs", 1, 50)  -> 2000 tokens           |
|  Turn 8:  read_file("src/main.rs", 1, 50)  -> 2000 tokens (SAME!)   |
|                                                                      |
|  File hasn't changed (same mtime). Second read = wasted context.     |
|                                                                      |
|  FIX: Track (path, start, end, mtime). If match, return stub:        |
|       "File unchanged since last read. Content already in context."   |
|                                                                      |
+----------------------------------------------------------------------+
|  SUB-PROBLEM 2: Stuck Loop (Consecutive Detection)                   |
+----------------------------------------------------------------------+
|                                                                      |
|  Turn 10: read_file("config.yaml")                                   |
|  Turn 11: read_file("config.yaml")  <- same args, consecutive        |
|  Turn 12: read_file("config.yaml")  <- 3rd time -> WARN              |
|  Turn 13: read_file("config.yaml")  <- 4th time -> BLOCK             |
|                                                                      |
|  Model is stuck. Each re-read returns same content, model retries.   |
|                                                                      |
|  FIX: Track last (name, args) tuple. Count consecutive identical.    |
|       3x = warn + return content + prepend warning.                  |
|       4x = block + return error with guidance.                       |
|                                                                      |
+----------------------------------------------------------------------+
```

---

## Hermes Agent Pattern (Cross-Reference)

**File:** `tools/file_tools.py` lines 199-450

```python
_read_tracker = {
    task_id: {
        "last_key": ("read", path, offset, limit),
        "consecutive": 3,
        "dedup": {(path, offset, limit): mtime},
    }
}
```

Hermes uses a per-task dict with:
1. **Dedup dict**: Maps `(path, offset, limit)` → `mtime` at read time
2. **Consecutive counter**: Increments when `last_key == current_key`
3. **Thresholds**: 3 = warn, 4 = hard block

---

## EdgeCrab Implementation

### Location: `crates/edgecrab-tools/src/read_tracker.rs`

EdgeCrab already has `read_tracker.rs` with `record_file_snapshot()` and
`has_file_snapshot()`. We extend it with:

1. **Dedup cache**: `HashMap<(PathBuf, Option<u64>, Option<u64>), SystemTime>`
2. **Consecutive counter**: `HashMap<(String, u64), u32>` keyed by `(tool_name, args_hash)`
3. **Public API**:

```rust
/// Check if a read is redundant (file unchanged since last read of same range).
/// Returns Some(stub_message) if dedup should short-circuit.
pub fn check_read_dedup(
    session_id: &str,
    path: &Path,
    start_line: Option<u64>,
    end_line: Option<u64>,
) -> Option<String>;

/// Track consecutive identical read calls.
/// Returns ReadLoopStatus { warn: bool, block: bool, count: u32 }.
pub fn check_consecutive_read(
    session_id: &str,
    tool_name: &str,
    args_hash: u64,
) -> ReadLoopStatus;

/// Reset dedup cache for a session (called after compression).
pub fn reset_read_dedup(session_id: &str);
```

### Integration Points

| File | Change | Why |
|------|--------|-----|
| `read_tracker.rs` | Add dedup + consecutive structs | New capability |
| `file_read.rs` | Call `check_read_dedup()` before reading | FP13 dedup |
| `file_read.rs` | Call `check_consecutive_read()` before reading | FP13 loop detect |
| `conversation.rs` | Call `reset_read_dedup()` after compression | FP17 cache reset |

### Edge Cases

| Case | Handling |
|------|----------|
| File modified externally between reads | mtime differs → dedup cache miss → full read |
| User explicitly asks "re-read this file" | Dedup returns stub, but model can use `patch` or different range |
| Concurrent sessions reading same file | Per-session tracker → no cross-contamination |
| File deleted between reads | Path no longer exists → normal error path |
| Compression clears context | `reset_read_dedup()` clears cache → next read is fresh |

### Tests

```
test_read_dedup_returns_stub_on_unchanged_file
test_read_dedup_allows_read_after_file_modified
test_consecutive_read_warns_at_3
test_consecutive_read_blocks_at_4
test_consecutive_read_resets_on_different_args
test_reset_read_dedup_clears_cache
```

---

## Estimated Impact

| Metric | Before | After |
|--------|--------|-------|
| Redundant read tokens per session | ~5-15K | ~0 (dedup) |
| Stuck loop iterations | Unbounded | Max 4 |
| Compression cache poisoning | Possible | Prevented |
