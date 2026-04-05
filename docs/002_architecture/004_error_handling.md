# 002.004 — Error Handling Strategy

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 002.001 Architecture](001_system_architecture.md)

## 1. Strategy: thiserror for Libraries, anyhow for Binaries

| Crate Type | Error Library | Rationale |
|-----------|--------------|-----------|
| `edgecrab-types` | `thiserror` | Precise error variants, downstream can match |
| `edgecrab-core` | `thiserror` | Library consumers need structured errors |
| `edgecrab-tools` | `thiserror` | Tool errors carry retry strategy |
| `edgecrab-state` | `thiserror` | Database errors distinguish transient vs permanent |
| `edgecrab-security` | `thiserror` | Scan results are structured data, not errors |
| `edgecrab-cli` | `anyhow` | Top-level binary, context-rich reporting |
| `edgecrab-gateway` | `anyhow` | Top-level binary, context-rich reporting |

## 2. Core Error Types

```rust
// edgecrab-types/src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("LLM API error: {source}")]
    Llm {
        #[from]
        source: edgequake_llm::LlmError,
    },

    #[error("Tool execution failed: {tool} — {message}")]
    ToolExecution { tool: String, message: String },

    #[error("Context limit exceeded: {used}/{limit} tokens")]
    ContextLimit { used: usize, limit: usize },

    #[error("Budget exhausted: {used}/{max} iterations")]
    BudgetExhausted { used: u32, max: u32 },

    #[error("Interrupted by user")]
    Interrupted,

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Provider rate limited: retry after {retry_after_ms}ms")]
    RateLimited {
        provider: String,
        retry_after_ms: u64,
    },

    #[error("Context compression failed: {0}")]
    CompressionFailed(String),

    #[error("API refusal: {0}")]
    ApiRefusal(String),

    #[error("Malformed tool call from LLM: {0}")]
    MalformedToolCall(String),

    #[error("Plugin error in {plugin}: {message}")]
    Plugin { plugin: String, message: String },

    #[error("Gateway delivery failed to {platform}: {message}")]
    GatewayDelivery { platform: String, message: String },

    #[error("OAuth flow failed: {0}")]
    OAuth(String),

    #[error("Migration error: {0}")]
    Migration(String),
}

// Tool errors carry retry strategy (mirrors edgequake-llm pattern)
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Unknown tool: {0}")]
    UnknownTool(String),

    #[error("Invalid arguments for {tool}: {message}")]
    InvalidArgs { tool: String, message: String },

    #[error("Tool {tool} unavailable: {reason}")]
    Unavailable { tool: String, reason: String },

    #[error("Execution timeout after {seconds}s: {tool}")]
    Timeout { tool: String, seconds: u64 },

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("{0}")]
    Other(String),
}
```

## 3. Result Type Aliases

```rust
// Crate-level Result aliases
pub type Result<T> = std::result::Result<T, AgentError>;
pub type ToolResult = std::result::Result<String, ToolError>;
```

## 4. Error Conversion to LLM

Tool errors must be converted to JSON strings for the LLM to understand:

```rust
impl ToolError {
    pub fn to_llm_response(&self) -> String {
        serde_json::json!({
            "error": self.to_string(),
            "retryable": self.is_retryable(),
        }).to_string()
    }

    pub fn is_retryable(&self) -> bool {
        matches!(self, ToolError::Timeout { .. } | ToolError::Unavailable { .. })
    }
}
```

## 5. Panic Policy

- **No panics in library crates.** All fallible operations return `Result`.
- Binary crates may `unwrap()` only during startup (config load, argument parse).
- Tool handlers catch panics via `std::panic::catch_unwind` at the dispatch boundary.
- `#[deny(clippy::unwrap_used)]` enforced in library crates via clippy config.

## 6. Edge-Case Error Recovery

These error scenarios arise in production LLM agent loops and **must** be handled:

| Scenario | Common failure mode | EdgeCrab handling |
|----------|---------------------|-------------------|
| Empty/null function args in tool calls | Return JSON parse error to model | `MalformedToolCall` variant, return error string to LLM |
| API refusal responses (content policy) | Graceful handling | `ApiRefusal` variant, surface to user |
| Stuck loop on malformed tool calls | Detection + skip | Timeout + error message to LLM |
| Consecutive assistant message merge | Content type mismatch | Type-safe `MessageContent` enum |
| compression_attempts unlimited resets | Counter per conversation | `AtomicU32` in `ConversationState` |
| length_continue_retries stale state | Reset per truncation event | Scoped retry counter |
| Compressor summary role violation | Consecutive-role constraint | Type-level role alternation enforcement |
| Silent tool result loss in compression | Preserve tool_result markers | Compression preserves `ToolResult` messages |
| None entry in tool_calls list | Filter before dispatch | `Option<ToolCall>` → filter_map |
| Event loop already running (parallel tools) | Per-thread event loops | No event loops — native async |
| Broken pipe in stdout (headless) | `_SafeWriter` wrapper | `SafeWriter<W: Write>` [→ 002.001#4.1] |
| Stale memory overwrites by flush agent | File locking | `tokio::sync::Mutex` on memory file |
| Provider token leaking to wrong endpoint | Provider-scoped credentials | Per-provider `ProviderCredential` type |
| OAuth flag stale after refresh | Refresh token invalidation | Token lifecycle state machine |
| Concurrent memory writes drop entries | File locking | `RwLock` on `MemoryStore` |
| FTS5 hyphenated query breakage | Quote preservation | Proper SQLite FTS5 query escaping |
| Session key case duplicates | Normalize keys | `SessionKey` newtype with forced lowercase |
| Model-specific tool-call format | 12 custom parsers in `tool_call_parsers/` | `ToolCallParser` trait with `parse(raw: &str) -> Vec<ToolCall>` per model |
| Codex Responses API tool format | Codex adapter in hermes-agent | `ResponsesApiAdapter` that normalizes to standard ToolCall |
| Anthropic thinking/reasoning blocks | `scratchpad_to_think` conversion | `ThinkingBlock` enum in message types, transparent to tool dispatch |

## 7. Tracing & Observability

```rust
// All errors are traced with structured context
use tracing::{error, warn, instrument};

#[instrument(skip(self, args), fields(tool = %name))]
async fn dispatch(&self, name: &str, args: &Value, ctx: &ToolContext) -> ToolResult {
    match self.registry.get(name) {
        Some(handler) => {
            match tokio::time::timeout(ctx.timeout, handler.execute(args, ctx)).await {
                Ok(Ok(result)) => Ok(result),
                Ok(Err(e)) => {
                    warn!(tool = %name, error = %e, "Tool execution failed");
                    Err(e)
                }
                Err(_) => {
                    error!(tool = %name, timeout = ?ctx.timeout, "Tool execution timed out");
                    Err(ToolError::Timeout { tool: name.to_string(), seconds: ctx.timeout.as_secs() })
                }
            }
        }
        None => Err(ToolError::UnknownTool(name.to_string())),
    }
}
```
