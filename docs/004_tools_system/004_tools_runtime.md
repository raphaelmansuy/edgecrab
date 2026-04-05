# 004.004 — Tools Runtime

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 004.001 Tool Registry](./001_tool_registry.md) | [→ 004.002 Tool Catalogue](./002_tool_catalogue.md) | [→ 004.003 Toolset Composition](./003_toolset_composition.md) | [→ 003.001 Agent Struct](../003_agent_core/001_agent_struct.md)
> **Source**: `edgecrab-tools/src/registry.rs`, `edgecrab-core/src/conversation.rs`, `edgecrab-tools/src/tools/*.rs`
> **Parity**: mirrors hermes-agent tool execution lifecycle

---

## 1. Compile-Time Registration

EdgeCrab uses the `inventory` crate for zero-cost compile-time tool registration, unlike hermes-agent where tools are registered at startup via Python decorators.

```rust
// Every tool file ends with:
inventory::submit!(&MyTool as &dyn ToolHandler);
```

The linker collects all `submit!` calls into a static slice. `ToolRegistry::new()` iterates this slice at startup — no manual registration, no forgotten imports.

```rust
inventory::collect!(&'static dyn ToolHandler);

impl ToolRegistry {
    pub fn new() -> Self {
        let mut tools = HashMap::new();
        let mut toolset_index = HashMap::new();

        for handler in inventory::iter::<&'static dyn ToolHandler> {
            let name = handler.name();
            let toolset = handler.toolset();
            tools.insert(name, *handler);
            toolset_index.entry(toolset).or_default().push(name);
        }

        Self { tools, toolset_index, dynamic_tools: HashMap::new() }
    }
}
```

**Invariant**: If a tool module is in `edgecrab-tools` but its `inventory::submit!` is removed, `cargo build` succeeds but the tool silently disappears. Add a build test if you need a guard (see §8).

---

## 2. Full Dispatch Flow

```
LLM response (tool_calls array)
  │
  └─▶ conversation.rs: for each tool_call in tool_calls:
        │
        ├─ 1. Emit StreamEvent::ToolStart { name, args }
        ├─ 2. ToolRegistry::dispatch(name, args, &ctx)
        │       │
        │       ├─ look up handler by exact name
        │       ├─ check_fn(ctx) → Unavailable error if false
        │       └─ handler.execute(args, ctx).await
        │
        ├─ 3. map Ok(output) → Message::tool_result(id, name, output)
        │    map Err(e)      → Message::tool_result(id, name, format_error(e))
        │
        ├─ 4. Emit StreamEvent::ToolEnd { name, result }
        └─ 5. session.messages.push(tool_result_message)
```

Tool errors are never fatal — they return a `tool_result` message that the LLM can read and decide how to recover.

---

## 3. `ToolContext` — Shared State

Every tool receives a `&ToolContext` with all shared runtime state:

| Field | Type | Purpose |
|-------|------|---------|
| `task_id` | `String` | Unique identifier for the current agent turn |
| `session_id` | `String` | Current session ID for state persistence |
| `cwd` | `PathBuf` | Working directory for file operations (path-jailed) |
| `user_task` | `Option<String>` | Original task description for sub-tool context |
| `cancel` | `CancellationToken` | Cooperative shutdown — check `cancel.is_cancelled()` in long loops |
| `config` | `AppConfigRef` | Full application config (security, tools, toolsets, paths) |
| `state_db` | `Option<Arc<SessionDb>>` | Session database (session_search, history tools) |
| `platform` | `Platform` | `Cli` / `Gateway` / `Acp` — affects tool behavior |
| `process_table` | `Option<Arc<ProcessTable>>` | Background processes (terminal tools) |
| `provider` | `Option<Arc<dyn LLMProvider>>` | LLM for sub-agent delegation |
| `tool_registry` | `Option<Arc<ToolRegistry>>` | Tool list for delegate_task |
| `delegate_depth` | `u32` | Delegation depth; blocked when ≥ 2 (prevents recursion) |
| `sub_agent_runner` | `Option<Arc<dyn SubAgentRunner>>` | Full execute_loop runner (breaks circular dep) |
| `clarify_tx` | `Option<mpsc::Sender<ClarifyRequest>>` | Interactive clarification channel (CLI only) |
| `on_skills_changed` | `Option<Arc<dyn Fn()>>` | Skills prompt cache invalidation callback |
| `gateway_sender` | `Option<Arc<dyn GatewaySender>>` | Outbound gateway message channel |
| `origin_chat` | `Option<(String, String)>` | `(platform, chat_id)` for cron job routing |

### Context in tests

```rust
let ctx = ToolContext::test_context();
// All optional fields are None, platform = Cli, cwd = tempdir
```

---

## 4. Two-Layer Gating

Tools have two independent availability checks:

```
┌────────────────────────────────────────────────────────┐
│ Layer 1: is_available() — called ONCE at registry init │
│                                                        │
│   • Binary present: `which ffmpeg`                     │
│   • Required env var: std::env::var("OPENAI_API_KEY")  │
│   • Tool absent from registry if this returns false    │
└────────────────────────────────────────────────────────┘
                        │
                        ▼ (if true, tool enters registry)
┌────────────────────────────────────────────────────────┐
│ Layer 2: check_fn(&ctx) — called on EVERY dispatch     │
│                                                        │
│   • Gateway running: ctx.gateway_sender.is_some()      │
│   • Honcho active: honcho client connected             │
│   • Per-session gate based on runtime context          │
└────────────────────────────────────────────────────────┘
                        │
                        ▼ (if false → ToolError::Unavailable)
                tool.execute(args, ctx)
```

Both layers use the same error path — `ToolError::Unavailable` becomes a `tool_result` message the LLM sees and can work around.

---

## 5. Async Dispatch and `spawn_blocking`

All tool execution is `async`. Blocking operations (file I/O, SQLite queries, image processing) use `tokio::task::spawn_blocking` to avoid starving the Tokio runtime:

```rust
// session_search.rs — SQLite is blocking
let results = tokio::task::spawn_blocking(move || {
    db.search(&query_str, limit)
})
.await
.map_err(|e| ToolError::Other(e.to_string()))??;

// vision.rs — image compression is CPU-bound
let result = tokio::task::spawn_blocking(move || {
    shrink_to_jpeg(bytes, mime)
})
.await
.map_err(|e| ToolError::Other(format!("spawn_blocking panic: {e}")))?;
```

**Rule**: If a tool operation takes more than ~1ms of CPU or blocks on I/O, wrap it in `spawn_blocking`.

---

## 6. Fuzzy Name Matching

When the LLM calls a tool with a typo, the registry uses Levenshtein distance to suggest the correct tool name:

```rust
// registry.rs
fn fuzzy_match(&self, name: &str) -> Option<&str> {
    let threshold = 3;
    // ... strsim::levenshtein(name, tool_name) ≤ threshold → return tool_name
}

// dispatch result:
// ToolError::NotFound("Unknown tool 'read_fil'. Did you mean 'read_file'?")
```

The LLM receives this error as a `tool_result` message and immediately retries with the corrected name in the next turn.

---

## 7. Error Handling

All tool errors are wrapped in `ToolError` and converted to `tool_result` messages — never bubbled up as agent failures:

| `ToolError` variant | When used |
|--------------------|-----------|
| `InvalidArgs { tool, message }` | JSON argument validation failed |
| `NotFound(String)` | Tool name unknown (with fuzzy suggestion) |
| `Unavailable { tool, reason }` | `check_fn` returned false |
| `PermissionDenied(String)` | Security check failed (path traversal, injection) |
| `Timeout` | Tool exceeded execution time limit |
| `Other(String)` | Catch-all for unexpected errors |

```rust
// conversation.rs — error → tool_result
let result_msg = match registry.dispatch(name, args, &ctx).await {
    Ok(output) => Message::tool_result(call_id, tool_name, &output),
    Err(e)     => Message::tool_result(call_id, tool_name, &format!("Error: {e}")),
};
session.messages.push(result_msg);
```

---

## 8. Security at Tool Boundaries

### 8.1 Path-jailing

File tools receive `ctx.cwd` as their root. They validate all resolved paths stay within this working directory:

```rust
// read_file.rs pattern
let path = ctx.cwd.join(args.path);
let canonical = path.canonicalize()?;
if !canonical.starts_with(&ctx.cwd) {
    return Err(ToolError::PermissionDenied("Path escapes working directory".into()));
}
```

### 8.2 Prompt injection detection

Before writing user-controlled content to memory or skills, tools call:

```rust
edgecrab_security::check_injection(content)?;
```

This checks for patterns that would cause the agent to execute unintended instructions (role-switching strings, system prompt overrides, etc.).

### 8.3 Cancellation cooperation

Long-running tools check the cancellation token in their processing loops:

```rust
// terminal.rs / execute_code.rs pattern
while reading_output {
    if ctx.cancel.is_cancelled() {
        process.kill().await?;
        return Ok("Cancelled.".into());
    }
    // ... read next chunk
}
```

---

## 9. Writing a New Tool

### Minimal implementation

```rust
// edgecrab-tools/src/tools/my_tool.rs
use async_trait::async_trait;
use serde_json::json;
use edgecrab_types::{ToolError, ToolSchema};
use crate::registry::{ToolContext, ToolHandler};

pub struct MyTool;

#[async_trait]
impl ToolHandler for MyTool {
    fn name(&self) -> &'static str { "my_tool" }

    fn toolset(&self) -> &'static str { "web" }    // or "file", "system", etc.

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "my_tool".into(),
            description: "Does something useful for the agent.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The thing to look up"
                    }
                },
                "required": ["query"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let query = args["query"].as_str()
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "my_tool".into(),
                message: "query must be a string".into(),
            })?;

        // Check cancellation for long operations
        if ctx.cancel.is_cancelled() {
            return Ok("Cancelled.".into());
        }

        // ... actual implementation ...
        Ok(format!("Result for: {query}"))
    }

    fn is_available(&self) -> bool {
        // Optional: check binary/env at startup
        std::env::var("MY_TOOL_API_KEY").is_ok()
    }
}

// Register with compile-time inventory
inventory::submit!(&MyTool as &dyn ToolHandler);
```

### Wire it in

Add the module to `edgecrab-tools/src/tools/mod.rs`:

```rust
pub mod my_tool;
```

Add a toolset mapping (if new toolset) to `edgecrab-tools/src/toolsets.rs`.

The tool is automatically available in `ToolRegistry::new()` — no other changes needed.

---

## 10. Dynamic Tools (MCP Proxies)

Tools from MCP servers are registered at runtime as dynamic tools:

```rust
// After discovering MCP tools:
registry.register_dynamic(Box::new(McpToolProxy {
    name: "mcp_server_my_action".to_string(),
    schema: tool_schema,
    client: mcp_client.clone(),
}));
```

Dynamic tools participate in normal dispatch but are not compile-time registered. They are rebuilt on every MCP server reconnect.

---

## 11. Parallel Tool Execution

Tools that are marked `parallel_safe: true` can be dispatched concurrently when the LLM requests multiple tool calls in one response:

```rust
// In conversation.rs — parallel dispatch
let parallel_calls: Vec<_> = tool_calls.iter()
    .filter(|c| registry.is_parallel_safe(c.name()))
    .collect();

let results = futures::future::join_all(
    parallel_calls.iter().map(|call| registry.dispatch(call.name(), call.args(), &ctx))
).await;
```

Most tools are **not** parallel-safe by default (`parallel_safe() → false`) because they may share state (file handles, session messages). Mark a tool parallel-safe only when it is provably stateless.

---

## 12. Testing Tools

```rust
#[tokio::test]
async fn test_my_tool_basic() {
    let ctx = ToolContext::test_context();
    let result = MyTool.execute(json!({"query": "test"}), &ctx).await;
    assert!(result.is_ok());
    assert!(result.unwrap().contains("test"));
}

#[tokio::test]
async fn test_my_tool_missing_arg() {
    let ctx = ToolContext::test_context();
    let result = MyTool.execute(json!({}), &ctx).await;
    assert!(matches!(result, Err(ToolError::InvalidArgs { .. })));
}
```

Use `tempfile::TempDir` for any file I/O operations and set `ctx.cwd = tempdir.path()`.

```bash
# Run all tool tests
cargo test -p edgecrab-tools

# Run a specific tool's tests
cargo test -p edgecrab-tools my_tool
```
