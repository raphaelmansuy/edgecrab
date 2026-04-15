# ADR-0604: Background Process Monitoring — `watch_patterns`

| Field       | Value                                                  |
|-------------|--------------------------------------------------------|
| Status      | Proposed                                               |
| Date        | 2026-04-14                                             |
| Implements  | hermes-agent PR #7635                                  |
| Crate       | `edgecrab-tools`                                       |
| Files       | `crates/edgecrab-tools/src/tools/process.rs` (modify)  |
|             | `crates/edgecrab-tools/src/process_table.rs` (modify)  |

---

## 1. Context

EdgeCrab's `process.rs` already manages background processes with a ring
buffer for stdout/stderr. hermes-agent v0.9.0 adds `watch_patterns` — the
ability to set regex/substring patterns to watch for in background process
output and deliver real-time notifications when they match (errors, "listening
on port", build completion, etc.).

Current EdgeCrab background process flow:
```
run_process("cargo watch") -> ProcessTable.register() -> proc-1
  tokio::spawn drains stdout/stderr into ring buffer
  Agent polls via get_process_output("proc-1")
```

Missing: **push-based** notification when output matches user-specified patterns.

---

## 2. First Principles

| Principle       | Application                                               |
|-----------------|-----------------------------------------------------------|
| **SRP**         | Pattern matching logic isolated from I/O drain loop       |
| **OCP**         | Existing process tools unchanged; watch is additive       |
| **DRY**         | Reuse existing ring buffer drain; add match check inline  |
| **Code is Law** | Rate limiting constants from `process_registry.py:65-68`  |

---

## 3. Architecture

```
+-------------------------------------------------------------------+
|                     ProcessTable (existing)                        |
|                                                                    |
|  +----------------------------+                                    |
|  | ProcessRecord              |                                    |
|  | - pid, command, status     |                                    |
|  | - ring_buffer (output)     |                                    |
|  | + watch_patterns: Vec<String>  <-- NEW                          |
|  | + watch_state: WatchState      <-- NEW                          |
|  +----------------------------+                                    |
|              |                                                     |
|  +-----------v-----------------+     +-------------------------+   |
|  | drain_loop (existing task)  |     | WatchNotificationSink   |   |
|  | reads stdout/stderr lines   +---->| mpsc::Sender<WatchEvent>|   |
|  | appends to ring buffer      |     | (to gateway/CLI)        |   |
|  | + check_watch_patterns() NEW|     +-------------------------+   |
|  +-----------------------------+                                   |
+-------------------------------------------------------------------+
```

---

## 4. Data Model

### 4.1 Watch State

```rust
/// Per-process watch pattern state.
/// Rate-limiting prevents notification floods from chatty processes.
struct WatchState {
    patterns: Vec<String>,                // substring patterns to match
    hits: u64,                            // total matches delivered
    suppressed: u64,                      // matches dropped by rate limit
    disabled: bool,                       // permanently killed by overload
    window_hits: u32,                     // hits in current rate window
    window_start: Instant,               // when current window began
    overload_since: Option<Instant>,      // when sustained overload started
}
```

### 4.2 Rate Limiting Constants

```rust
// Source: hermes-agent process_registry.py:65-68
const WATCH_MAX_PER_WINDOW: u32 = 8;          // max notifications per window
const WATCH_WINDOW_SECONDS: u64 = 10;         // rolling window length
const WATCH_OVERLOAD_KILL_SECONDS: u64 = 45;  // sustained overload -> disable
```

### 4.3 Watch Event (notification payload)

```rust
pub struct WatchEvent {
    pub process_id: String,
    pub pattern: String,             // which pattern matched
    pub matched_output: String,      // trimmed: max 20 lines, max 2000 chars
    pub suppressed_count: u64,       // how many were suppressed since last delivery
    pub event_type: WatchEventType,
}

pub enum WatchEventType {
    Match,           // pattern matched
    Disabled,        // overload protection permanently disabled watching
}
```

### 4.4 Tool Schema Extension

```json
{
  "name": "run_process",
  "parameters": {
    "properties": {
      "command": { "type": "string" },
      "watch_patterns": {
        "type": "array",
        "items": { "type": "string" },
        "description": "Substring patterns to watch for in process output. Notifications are delivered in real-time when matched."
      }
    }
  }
}
```

---

## 5. Algorithm: Pattern Check on Each Output Line

```
check_watch_patterns(line: &str, state: &mut WatchState, sink: &Sender<WatchEvent>)
  |
  +-- if state.disabled: return
  |
  +-- for pattern in state.patterns:
  |     if line.contains(pattern):
  |       +-- state.hits += 1
  |       +-- if window expired (now - window_start > 10s):
  |       |     reset window_hits = 0, window_start = now
  |       +-- state.window_hits += 1
  |       +-- if window_hits > WATCH_MAX_PER_WINDOW:
  |       |     state.suppressed += 1
  |       |     if overload_since.is_none():
  |       |       overload_since = Some(now)
  |       |     elif now - overload_since > 45s:
  |       |       state.disabled = true
  |       |       sink.send(WatchEvent { type: Disabled, ... })
  |       |     return  // suppress this notification
  |       +-- else:
  |       |     overload_since = None  // rate within bounds, clear overload
  |       +-- sink.send(WatchEvent {
  |       |     process_id, pattern, matched_output (trimmed),
  |       |     suppressed_count: state.suppressed (then reset to 0),
  |       |     type: Match
  |       |   })
  |       +-- break  // one notification per line, even if multiple patterns match
```

---

## 6. Integration Points

### 6.1 Drain Loop (process.rs)

The existing `drain_loop` spawned for each background process reads lines
from stdout/stderr. Add pattern checking after ring buffer append:

```rust
// In the drain task (existing code path):
while let Some(line) = reader.next_line().await? {
    if !is_shell_noise(&line) {
        record.ring_buffer.push(line.clone());
        // NEW: check watch patterns
        if let Some(ref mut watch) = record.watch_state {
            check_watch_patterns(&line, watch, &notification_sink);
        }
    }
}
```

### 6.2 CLI Notification Display

```
WatchEvent received in CLI event loop:
  -> Display as tool-output style notification:
     "🔔 Process proc-1 matched 'error': <matched output>"

If suppressed_count > 0:
  -> "(3 earlier matches were rate-limited)"
```

### 6.3 Gateway Notification Delivery

```
WatchEvent received in gateway event loop:
  -> Send as platform message to the user's chat
  -> Respect display.background_process_notifications config:
     all    -> deliver all watch events
     result -> only final completion (no watch events)
     error  -> only if matched pattern suggests error
     off    -> suppress all
```

---

## 7. Edge Cases & Roadblocks

| #  | Edge Case                               | Remediation                                      | Source                          |
|----|-----------------------------------------|--------------------------------------------------|---------------------------------|
| 1  | Chatty process floods matches           | Rate limit: 8/10s window, kill after 45s         | `process_registry.py:65-68`     |
| 2  | Pattern matches every line              | Same rate limiting + permanent disable            | `_check_watch_patterns()`       |
| 3  | Watch disabled is permanent             | No recovery — user must restart process           | hermes-agent design decision    |
| 4  | Binary output (non-UTF8)               | `String::from_utf8_lossy()` on each line          | Existing ring buffer behavior   |
| 5  | Process exits before pattern match      | No notification — process_complete event instead  | Existing completion flow        |
| 6  | Multiple patterns match same line       | Only first match fires notification (break after) | `_check_watch_patterns()`       |
| 7  | `execute_code` tool leakage             | Block `watch_patterns` param in execute_code      | `code_execution_tool.py`        |
| 8  | Matched output very long                | Trim: max 20 lines, max 2000 chars               | `process_registry.py`           |
| 9  | Thread safety on WatchState             | Protect with per-record Mutex (existing pattern)  | `ProcessRecord` already locked  |
| 10 | Gateway dedup of rapid notifications    | Dedup by (process_id, pattern, 5s window)         | New — prevents spam             |

---

## 8. Implementation Plan

### 8.1 Files to Modify

| File                                          | Change                                     |
|-----------------------------------------------|---------------------------------------------|
| `crates/edgecrab-tools/src/process_table.rs`  | Add `WatchState`, `WatchEvent` structs      |
| `crates/edgecrab-tools/src/tools/process.rs`  | Add `watch_patterns` param to `run_process` |
| `crates/edgecrab-tools/src/tools/process.rs`  | Add `check_watch_patterns()` in drain loop  |
| `crates/edgecrab-tools/src/registry.rs`       | Add `watch_notification_sink` to ToolContext|
| `crates/edgecrab-cli/src/app.rs`              | Drain watch events in event loop            |
| `crates/edgecrab-gateway/src/run.rs`          | Drain watch events in gateway dispatch      |

### 8.2 Test Matrix

| Test                              | Validates                                    |
|-----------------------------------|----------------------------------------------|
| `test_watch_single_match`         | Pattern matches and fires notification       |
| `test_watch_no_match`             | No notification when output doesn't match    |
| `test_watch_rate_limit`           | >8 hits in 10s → suppression                |
| `test_watch_overload_disable`     | 45s sustained overload → permanent disable   |
| `test_watch_suppressed_count`     | Suppressed count reported on next delivery   |
| `test_watch_output_trimmed`       | Matched output capped at 20 lines / 2000 ch  |
| `test_watch_multiple_patterns`    | Only first matching pattern fires per line   |
| `test_watch_window_reset`         | Window resets after 10s                      |
| `test_execute_code_blocks_watch`  | `execute_code` rejects `watch_patterns`      |

---

## 9. Acceptance Criteria

- [ ] `run_process` accepts optional `watch_patterns: Vec<String>` parameter
- [ ] Pattern matching runs on each stdout/stderr line in drain loop
- [ ] Rate limiting: 8 per 10s window, permanent disable after 45s overload
- [ ] `WatchEvent` delivered to CLI via event channel
- [ ] `WatchEvent` delivered to gateway via notification channel
- [ ] Matched output trimmed to 20 lines / 2000 chars max
- [ ] Suppressed count bundled into next successful delivery
- [ ] `execute_code` tool blocks `watch_patterns` parameter
- [ ] All tests pass: `cargo test -p edgecrab-tools -- watch`
