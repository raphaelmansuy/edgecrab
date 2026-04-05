# Error Handling 🦀

> **Verified against:** `crates/edgecrab-types/src/error.rs` ·
> `crates/edgecrab-core/src/conversation.rs` ·
> `crates/edgecrab-tools/src/registry.rs`

---

## Why the error model is what it is

`hermes-agent` — EdgeCrab's Python predecessor — surfaced failures through Python
exceptions and string messages: easy to raise, impossible to branch on specific
failure modes without `isinstance` checks or string parsing. Tool errors were
formatted as plain text and passed back to the model, losing all structure.
OpenClaw ([TypeScript/Node.js](https://github.com/openclaw)) surfaces tool
failures as untyped JavaScript `Error` objects — the same limitation in a
different runtime.

EdgeCrab uses typed errors (`thiserror` enums) for two concrete reasons:

1. **Callers can branch on the variant** — the agent loop treats `RateLimited`
   differently from `BudgetExhausted` differently from `ToolExecution`. You
   cannot do that with a stringly-typed error.

2. **Tool failures become structured LLM input** — when a tool returns
   `Err(ToolError)`, the loop does not propagate a Rust error. It serialises
   the error into a JSON `ToolErrorResponse` and appends it to the
   conversation history. The model reads it, understands what went wrong, and
   adapts — rather than looping blindly.

🦀 *This is EdgeCrab's decisive advantage in the tool-use bout:
instead of surfacing an opaque error string when a claw misses, the crab tells
itself exactly which variant failed and what angle to try next.*

---

## The two error enums

### `AgentError` — agent / provider layer

```rust
// edgecrab-types/src/error.rs
pub enum AgentError {
    Llm(String),
    ToolExecution { tool: String, message: String },
    ContextLimit { used: usize, limit: usize },
    BudgetExhausted { used: u32, max: u32 },
    Interrupted,
    Config(String),
    Database(String),
    Io(#[from] std::io::Error),
    Serde(#[from] serde_json::Error),
    RateLimited { provider: String, retry_after_ms: u64 },
    CompressionFailed(String),
    ApiRefusal(String),
    MalformedToolCall(String),
    Plugin { plugin: String, message: String },
    GatewayDelivery { platform: String, message: String },
    Migration(String),
    Security(String),
    Validation(String),
}

pub type Result<T> = std::result::Result<T, AgentError>;
```

### `ToolError` — tool execution layer

```rust
pub enum ToolError {
    NotFound(String),
    InvalidArgs { tool: String, message: String },
    Unavailable { tool: String, reason: String },
    Timeout { tool: String, seconds: u64 },
    PermissionDenied(String),
    ExecutionFailed { tool: String, message: String },
    CapabilityDenied {
        tool: String,
        code: String,
        message: String,
        suppression_key: Option<String>,  // prevents infinite retry loops
        suggested_tool: Option<String>,   // guides the model to a fallback
        suggested_action: Option<String>, // human-readable next step
    },
    Other(String),
}
```

---

## `ToolError` → JSON payload → model input

When `ToolHandler::execute()` returns `Err(ToolError)`, the execution path:

```
  tool returns  Err(ToolError::ExecutionFailed { tool: "write_file",
                                                  message: "Read-only filesystem" })
        │
        ▼
  ToolError::to_llm_payload()
        │
        ▼
  ToolErrorResponse {
      response_type:     "error",
      category:          "execution",
      code:              "execution_failed",
      error:             "Read-only filesystem",
      retryable:         true,
      suppress_retry:    false,
      suppression_key:   None,
      tool:              Some("write_file"),
      suggested_tool:    None,
      suggested_action:  Some("Use a writable path under /tmp or the project root"),
  }
        │
        ▼
  serde_json::to_string(&response) → JSON string
        │
        ▼
  Message::tool_result(tool_call_id, "write_file", json_string)
        │
        ▼
  Appended to conversation history
        │
        ▼
  LLM reads it on next iteration, adjusts its approach
```

The model sees a structured JSON object — not a stack trace, not a panic,
not silence.

---

## Retry and suppression logic

`ToolError` has three classification methods the dispatcher uses before
deciding what to do after a failure:

```
  ToolError::is_retryable()
  ─────────────────────────────────────────────────────────────────────
  ExecutionFailed, Timeout  → true   (transient; try again)
  NotFound, PermissionDenied → false  (structural; retrying won't help)

  ToolError::should_suppress_retry()
  ─────────────────────────────────────────────────────────────────────
  CapabilityDenied with suppression_key  → true
  (don't feed back into the model loop — it clearly can't do this)

  ToolError::suppression_key()
  ─────────────────────────────────────────────────────────────────────
  Stable string key used to deduplicate retry loops:
    "execute_code:no_docker"  prevents the model from requesting
    Docker-based code execution 5 times in a row when Docker is absent
```

---

## `AgentError` recovery in the loop

The conversation loop (`execute_loop`) handles each `AgentError` variant
differently:

```
  execute_loop
        │
        ├── AgentError::RateLimited { retry_after_ms }
        │       └── sleep(retry_after_ms) + exponential backoff
        │           base=500ms, max retries=3
        │
        ├── AgentError::ContextLimit { used, limit }
        │       └── trigger compression pipeline → retry API call
        │
        ├── AgentError::BudgetExhausted { used, max }
        │       └── break loop
        │           ConversationResult::budget_exhausted = true
        │
        ├── AgentError::Interrupted
        │       └── break loop
        │           ConversationResult::interrupted = true
        │
        ├── AgentError::MalformedToolCall
        │       └── log warning + continue loop
        │           (model issued bad JSON; give it another chance)
        │
        └── AgentError::Llm / AgentError::Serde
                └── propagate to caller (unrecoverable turn failure)
```

---

## Fuzzy match on `ToolError::NotFound`

When the registry cannot find a tool by exact name, it applies Levenshtein
distance ≤ 3 before giving up:

```
  model requests "write_fiel"  (typo)
        │
        ▼
  registry.dispatch("write_fiel", ...)
        │
        ▼
  exact match? No
        │
        ▼
  fuzzy_match("write_fiel")
        │
        ▼
  Levenshtein("write_fiel", "write_file") = 1  ≤ 3
        │
        ▼
  ToolError::NotFound("write_fiel. Did you mean: write_file?")
```

The "Did you mean" hint is included in the `ToolErrorResponse` fed back to
the model — it typically self-corrects in one additional step.

**Reference:** [Levenshtein distance](https://en.wikipedia.org/wiki/Levenshtein_distance)

---

## `#[from]` implicit conversions

`AgentError` derives `#[from]` for stdlib error types, enabling `?` syntax:

```rust
fn read_config(path: &Path) -> crate::Result<AppConfig> {
    let text = std::fs::read_to_string(path)?;  // io::Error → AgentError::Io
    let cfg = serde_yaml::from_str(&text)
        .map_err(|e| AgentError::Config(e.to_string()))?;
    Ok(cfg)
}
```

---

## `#![deny(clippy::unwrap_used)]`

Enforced in `edgecrab-types` (the leaf all other crates import). No `.unwrap()`
or `.expect()` outside of `#[cfg(test)]`. Compile fails if violated.

---

## Practical rule

> **If the failure should be visible to the model as part of the conversation → `ToolError`.**
> **If it should abort or short-circuit the conversation machinery → `AgentError`.**

Do not return `AgentError` from a `ToolHandler`. Map it:

```rust
// Wrong:
async fn execute(...) -> Result<String, ToolError> {
    do_something()?  // AgentError leaks through
}

// Right:
async fn execute(...) -> Result<String, ToolError> {
    do_something()
        .map_err(|e| ToolError::ExecutionFailed {
            tool: self.name().to_string(),
            message: e.to_string(),
        })
}
```

---

## Tips

> **Tip: Use `ToolError::capability_denied()` for soft "can't do this" situations.**
> Set `.with_suggested_action()` and `.with_suppression_key()` to guide the
> model away from infinite retry loops.

> **Tip: `ToolErrorRecord` is stored in `ConversationResult::tool_errors`.**
> After a session, you can inspect every tool failure including the full arguments
> and the exact response sent back to the model. Useful for debugging agent behaviour.

> **Tip: `AgentError::Security(String)` is used by `edgecrab-security` checks.**
> If a path escapes the jail or a command matches a dangerous pattern, the check
> returns `Err(AgentError::Security(...))` — the loop converts this into a
> `ToolError::PermissionDenied` response visible to the model.

---

## FAQ

**Q: What happens if a tool panics?**
Tokio task panics do not crash the process. The conversation loop catches the
failed join and synthesises a `ToolError::ExecutionFailed` response.

**Q: Why is `Database(String)` a string, not `rusqlite::Error`?**
Exposing `rusqlite::Error` in `edgecrab-types` would force all 10 crates to
depend on `rusqlite`. The `edgecrab-state` crate converts the error to a string
before crossing the crate boundary.

**Q: Can the LLM see `AgentError` details?**
No. Only `ToolError` serialised into `ToolErrorResponse` enters the conversation
history. `AgentError` propagates to the frontend, which decides how to present it.

---

## Cross-references

- Where errors are handled in the loop → [Conversation Loop](../003_agent_core/002_conversation_loop.md)
- Tool dispatch producing `ToolError` → [Tool Registry](../004_tools_system/001_tool_registry.md)
- Security errors (`AgentError::Security`) → [Security](../011_security/001_security.md)
