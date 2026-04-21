# Spec 14: Stale Stream Detection

**Priority:** P1 — High
**Crate:** `edgecrab-core` (conversation.rs)
**Cross-ref:** [09-assessment-round2.md](09-assessment-round2.md) Gap 5

## Problem

EdgeCrab has `STREAM_FIRST_CHUNK_TIMEOUT = 20s` for the first chunk, but
**no timeout between subsequent chunks**. A hung connection after the first
chunk holds the loop forever.

Hermes Agent scales the inter-chunk timeout with context size. Claude Code
uses `getNonstreamingFallbackTimeoutMs()` bounded per-request.

```
+---------------------------------------------------------------+
|                   CURRENT (BROKEN)                            |
+---------------------------------------------------------------+
|                                                               |
|  Stream starts → chunk 1 arrives (within 20s) ✓              |
|  chunk 2 arrives ✓                                            |
|  ... 5 minutes pass, no more chunks ...                       |
|  ... still waiting ... (forever)                              |
|                                                               |
+---------------------------------------------------------------+

+---------------------------------------------------------------+
|                   FIXED (INTER-CHUNK TIMEOUT)                 |
+---------------------------------------------------------------+
|                                                               |
|  Stream starts → chunk 1 arrives (within 20s) ✓              |
|  chunk 2 arrives ✓                                            |
|  ... 60s pass, no more chunks ...                             |
|  STALE DETECTED → kill stream → retry non-streaming           |
|                                                               |
+---------------------------------------------------------------+
```

## Design

Add `STREAM_INTER_CHUNK_TIMEOUT` constant. In the streaming loop, reset a
deadline after every chunk. If the deadline expires, abort the stream and
return a retryable error so the existing retry loop handles it.

**Constants:**
```rust
#[cfg(not(test))]
const STREAM_INTER_CHUNK_TIMEOUT: Duration = Duration::from_secs(60);
#[cfg(test)]
const STREAM_INTER_CHUNK_TIMEOUT: Duration = Duration::from_millis(100);
```

## Implementation

**File:** `crates/edgecrab-core/src/conversation.rs`
**Location:** Inside the streaming chunk collection loop.

The streaming loop already uses `tokio::select!` with the first-chunk
timeout. We add the same pattern for inter-chunk: reset a
`tokio::time::sleep` future after each chunk. If it fires, the stream
is declared stale.

```rust
// In the streaming loop:
let mut inter_chunk_deadline = tokio::time::sleep(STREAM_INTER_CHUNK_TIMEOUT);
loop {
    tokio::select! {
        chunk = stream.next() => {
            match chunk {
                Some(Ok(c)) => {
                    // process chunk...
                    inter_chunk_deadline = tokio::time::sleep(STREAM_INTER_CHUNK_TIMEOUT);
                }
                Some(Err(e)) => break Err(e),
                None => break Ok(response),
            }
        }
        _ = &mut inter_chunk_deadline => {
            tracing::warn!("stale stream detected — no chunk for {}s",
                STREAM_INTER_CHUNK_TIMEOUT.as_secs());
            break Err(edgequake_llm::LlmError::Timeout);
        }
        _ = cancel.cancelled() => {
            break Err(edgequake_llm::LlmError::Cancelled);
        }
    }
}
```

## Edge Cases

1. **Reasoning models** with long think pauses: The 60s timeout is generous.
   Models like Claude opus-4.6 with extended thinking still send reasoning
   chunks within 60s. If needed, scale timeout with `reasoning_effort`.
2. **Very large outputs**: Even at 100 tokens/sec streaming, inter-chunk
   gaps never exceed a few hundred ms. 60s is very conservative.
3. **Test mode**: 100ms timeout ensures tests don't hang.

## Tests

1. Stream that stalls after 2 chunks → `LlmError::Timeout`
2. Normal stream → completes without timeout
3. Cancelled stream → `LlmError::Cancelled` (existing behavior preserved)

## SOLID

- **SRP:** Timeout logic is a `select!` branch, not a new abstraction.
- **OCP:** Timeout value is a constant — easy to make configurable later.
