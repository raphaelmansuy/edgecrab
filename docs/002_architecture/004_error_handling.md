# Error Handling

Verified against:
- `crates/edgecrab-types/src/error.rs`
- `crates/edgecrab-core/src/conversation.rs`
- `crates/edgecrab-tools/src/registry.rs`

The error strategy is intentionally boring, which is good. Library code returns structured errors. Binary entry points use `anyhow` when they need command-level context. Tool failures stay visible to the model instead of disappearing into logs.

- library crates expose structured errors with `thiserror`
- binary entry points use `anyhow` for top-level command failures
- tool failures are converted into machine-readable payloads and fed back to the model

## Main error types

If you only remember two names, remember `AgentError` and `ToolError`.

- `AgentError`: top-level runtime failures such as provider errors, config errors, interruption, rate limits, compression failures, and security violations.
- `ToolError`: tool-scoped failures such as bad arguments, unavailable capabilities, permission denial, timeout, and execution failure.
- `ToolErrorResponse`: normalized JSON payload returned to the model so it can retry or change course.
- `ToolErrorRecord`: session-level record used for reporting and post-run analysis.

## Runtime behavior

The most important behavior is that tool failures stay in-band:

```text
tool call fails
  -> tool returns ToolError
  -> core serializes ToolErrorResponse
  -> response is appended as a tool result message
  -> model gets one more chance to recover
```

## Recovery semantics encoded today

- `AgentError::RateLimited` participates in retry and backoff.
- `AgentError::Interrupted` and `AgentError::BudgetExhausted` terminate the loop cleanly.
- `ToolError::Unavailable` and `ToolError::Timeout` are marked retryable.
- capability-denied tool errors can carry suppression keys and suggested fallback actions.

## Practical rule

If a failure should be visible to the model as part of the conversation, it belongs in `ToolError`. If it should abort or short-circuit the conversation machinery itself, it belongs in `AgentError`.
