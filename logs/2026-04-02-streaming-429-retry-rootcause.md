# Root Cause: 429 RESOURCE_EXHAUSTED Fails Immediately During Native Streaming

**Date**: 2026-04-02
**Severity**: High — every 429 from Vertex AI during streaming is unrecoverable
**Status**: Fixed (commit aa83b3c)

---

## Observed Symptom

```
LLM API error: API call failed after 0 retries: API error: Stream error: {
  "error": {
    "code": 429,
    "message": "Resource exhausted. Please try again later.",
    "status": "RESOURCE_EXHAUSTED"
  }
}
failed attempt=0 error=API error: Stream error: { ...429... }
```

Model: `vertexai/gemini-2.5-pro` in native-streaming mode (shown in status bar).

---

## First-Principles Analysis

### Two independent bugs

| # | File | Bug | Effect |
|---|------|-----|--------|
| A | `vendor/edgequake-llm/src/providers/gemini.rs` | HTTP 429 mapped to `LlmError::ApiError` | Error not recognisable as rate-limit |
| B | `crates/edgecrab-core/src/conversation.rs` | `retry_budget = 0` when streaming | All errors abort on first attempt |

### Bug A — Wrong error classification in gemini.rs

The Gemini streaming path checks `response.status().is_success()`. When the
server returns HTTP 429 before sending any SSE bytes:

```rust
// BEFORE (wrong)
if !response.status().is_success() {
    let text = response.text().await.unwrap_or_default();
    return Err(LlmError::ApiError(format!("Stream error: {}", text)));
    //         ^^^^^^^^^^^^^ generic — caller cannot distinguish 429 from 500
}
```

The error string contains `"RESOURCE_EXHAUSTED"` but the variant is `ApiError`,
not `RateLimited`. Any caller matching on error type for retry decisions treats it
as a generic, non-retryable failure.

### Bug B — Streaming disables all retries in conversation.rs

`api_call_with_retry` sets:

```rust
// BEFORE (wrong)
let retry_budget = if use_native_streaming { 0 } else { max_retries };
```

**Why this was added**: If a stream fails *mid-flight* (after tokens have been
pushed to `event_tx`), retrying would re-stream from the beginning and the TUI
would display duplicate content.

**Why this is wrong**: 429 errors are returned as an HTTP error response
**before** the streaming body is opened. The `chat_with_tools_stream()` call
itself returns `Err(...)` — the loop body (where `event_tx.send(Token(...))` is
called) is never entered. There is zero partial state to protect.

Setting `retry_budget = 0` conflated "pre-stream failure" with "mid-stream
failure" and disabled the only protection against transient quota exhaustion.

---

## Fix

### gemini.rs (Bug A)

Check the HTTP status code before discarding it:

```rust
// AFTER (correct)
if !response.status().is_success() {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if status.as_u16() == 429 || text.contains("RESOURCE_EXHAUSTED") {
        return Err(LlmError::RateLimited(format!("Stream error: {}", text)));
    }
    return Err(LlmError::ApiError(format!("Stream error: {}", text)));
}
```

Applied to both SSE streaming entry points in the file (lines ~1700 and ~1861).

### conversation.rs (Bug B)

Remove the streaming special-case entirely; instead abort in the `Err` arm for
errors that are NOT pre-stream-safe:

```rust
// AFTER (correct)
let retry_budget = max_retries;  // same for streaming and non-streaming

// In the Err(e) match arm:
if use_native_streaming {
    let is_prestream_retryable = matches!(
        e,
        edgequake_llm::LlmError::RateLimited(_)
            | edgequake_llm::LlmError::NetworkError(_)
            | edgequake_llm::LlmError::Timeout
            | edgequake_llm::LlmError::AuthError(_)
    );
    if !is_prestream_retryable {
        // Mid-stream or unknown — abort immediately (tokens may have been sent).
        return Err(AgentError::Llm(format!(
            "API call failed after {} retries: {}",
            attempt, e
        )));
    }
}
last_err = Some(e);
```

**Invariant maintained**:
- `RateLimited` → always pre-stream (server rejects before opening body) → retry ✅
- `NetworkError` / `Timeout` → connection-level, before stream data → retry ✅
- `ApiError` during streaming → could be mid-stream → abort immediately ✅

---

## Impact

| Scenario | Before | After |
|----------|--------|-------|
| 429 on first attempt, streaming | ❌ Immediate failure, "0 retries" | ✅ Retried up to `max_retries` times with exponential backoff |
| 429 on first attempt, non-streaming | ✅ Already retried | ✅ Unchanged |
| Mid-stream network drop | ✅ Aborted (0 retries) | ✅ Aborted (0 retries, correctly) |
| Non-429 API error during streaming | ✅ Aborted (0 retries) | ✅ Aborted (0 retries, correctly) |
