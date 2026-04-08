# Tool Registry 🦀

> **Verified against:** `crates/edgecrab-tools/src/registry.rs` ·
> `crates/edgecrab-tools/src/tools/mod.rs`

---

## Why the registry exists

Without a registry, the agent loop needs to know about every tool: import it,
call it, handle its errors. Adding a tool means editing the loop.

The registry inverts this: tools declare themselves at compile time via
`inventory::submit!`. The loop calls `ToolRegistry::dispatch(name, args, ctx)`
and gets a result back — it has no idea which tool ran, how the tool works,
or what crate the tool lives in.

🦀 *`hermes-agent` (Python) dispatched tools through a central handler dict —
adding a tool meant editing the dispatch map. EdgeCrab's registry means a new
tool is literally a new struct in a new file. The crab grows new claws without surgery.*

---

## Registration: compile-time via `inventory`

```rust
// Any file in edgecrab-tools/src/tools/

struct ReadFileTool;

#[async_trait]
impl ToolHandler for ReadFileTool {
    fn name(&self)    -> &'static str { "read_file" }
    fn toolset(&self) -> &'static str { "file" }
    fn schema(&self)  -> ToolSchema   { /* JSON schema */ }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext)
        -> Result<String, ToolError>
    {
        let path = args["path"].as_str()
            .ok_or_else(|| ToolError::InvalidArgs { .. })?;
        // ... read the file ...
        Ok(content)
    }
}

// This line registers ReadFileTool at binary startup — no list to maintain
inventory::submit! { &ReadFileTool as &dyn ToolHandler }
```

`ToolRegistry::new()` iterates `inventory::iter::<&dyn ToolHandler>` and
builds the internal `HashMap<name, handler>` automatically.

**Reference:** [`inventory` crate](https://docs.rs/inventory/latest/inventory/)

---

## `ToolHandler` trait

```rust
#[async_trait]
pub trait ToolHandler: Send + Sync + 'static {
    // Required
    fn name(&self)    -> &'static str;
    fn toolset(&self) -> &'static str;
    fn schema(&self)  -> ToolSchema;
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext)
        -> Result<String, ToolError>;

    // Optional — defaults shown
    fn is_available(&self) -> bool { true }        // startup: docker present? API configured?
    fn check_fn(&self, _ctx: &ToolContext) -> bool { true }  // per-request: platform allowed?
    fn parallel_safe(&self) -> bool { false }       // can run concurrently with peer tools?
    fn emoji(&self) -> &'static str { "⚡" }         // TUI display
}
```

`is_available()` is called once at registry build time. Tools that fail the
check are still registered but excluded from schema lists sent to the LLM.

`check_fn()` is called on every dispatch. Use for per-request conditions
(e.g., `ha_*` tools check that `HA_URL` is configured at call time).

---

## Dispatch path

```
  ToolRegistry::dispatch(name, args, ctx)
        │
        ├─ exact match in static tools?
        │       │
        │       ├─ toolset in active_toolsets? (No → CapabilityDenied)
        │       ├─ check_fn(&ctx)?             (No → CapabilityDenied)
        │       └─ handler.execute(args, ctx)
        │
        ├─ exact match in dynamic tools? (MCP, plugins)
        │       └─ same gates as static
        │
        └─ no exact match
                │
                ▼
          fuzzy_match(name)   [Levenshtein distance ≤ 3]
                │
                ├─ found:  ToolError::NotFound("Did you mean: <suggestion>?")
                └─ not found: ToolError::NotFound(name)
```

**Reference:** [Levenshtein distance](https://en.wikipedia.org/wiki/Levenshtein_distance)

---

## `ToolContext` — the execution environment

Every tool receives a `ToolContext` reference. This is the complete picture of
what a tool can access:

```rust
pub struct ToolContext {
    pub task_id:          String,
    pub cwd:              PathBuf,          // current working directory
    pub session_id:       String,
    pub user_task:        Option<String>,   // original user request (for delegation)
    pub cancel:           CancellationToken,
    pub config:           AppConfigRef,     // read-only config snapshot
    pub state_db:         Option<Arc<SessionDb>>,
    pub platform:         Platform,
    pub process_table:    Option<Arc<ProcessTable>>,
    pub provider:         Option<Arc<dyn LLMProvider>>,  // for generate_image, etc.
    pub tool_registry:    Option<Arc<ToolRegistry>>,     // for moa
    pub delegate_depth:   u32,              // max=2; prevents runaway recursion
    pub sub_agent_runner: Option<Arc<dyn SubAgentRunner>>,
    pub clarify_tx:       Option<UnboundedSender<ClarifyRequest>>,   // ask user
    pub approval_tx:      Option<UnboundedSender<ApprovalRequest>>,  // gate danger
    pub on_skills_changed: Option<Arc<dyn Fn() + Send + Sync>>,
    pub gateway_sender:   Option<Arc<dyn GatewaySender>>,
    pub origin_chat:      Option<(String, String)>,  // (platform, chat_id)
    pub session_key:      Option<String>,
    pub todo_store:       Option<Arc<TodoStore>>,
}
```

Tests use `ToolContext::test_context()` (compiled only with `#[cfg(test)]`).

---

## Dynamic tools (MCP + plugins)

Static tools use `inventory` and are compiled in. Dynamic tools are registered
at runtime:

```rust
impl ToolRegistry {
    pub fn register_dynamic(&mut self, handler: Box<dyn ToolHandler>)
}
```

This is used by:
- **MCP servers** — `mcp_list_tools` proxies remote tools as dynamic `ToolHandler` instances
- **Plugins** — loaded from `~/.edgecrab/plugins/` at startup

Dynamic tools participate in all the same dispatch logic (toolset filtering,
approval gating, fuzzy matching) as static tools.

---

## `GatewaySender` and `SubAgentRunner` traits

These two traits break the circular dependency (see
[Crate Dependency Graph](../002_architecture/002_crate_dependency_graph.md)):

```rust
// Defined in edgecrab-tools/src/registry.rs
// Implemented in edgecrab-gateway and edgecrab-core respectively

#[async_trait]
pub trait GatewaySender: Send + Sync + 'static {
    async fn send_message(&self, platform, recipient, message) -> Result<(), String>;
    async fn list_targets(&self) -> Result<Vec<String>, String>;
}

#[async_trait]
pub trait SubAgentRunner: Send + Sync {
    async fn run_task(
        &self,
        goal: String,
        system_prompt: Option<String>,
        enabled_toolsets: Vec<String>,
        max_iterations: u32,
        model_override: Option<String>,
        parent_cancel: CancellationToken,
    ) -> Result<SubAgentResult, String>;
}
```

---

## Writing a new tool — step by step

```sh
# 1. Create the file
touch crates/edgecrab-tools/src/tools/my_tool.rs

# 2. Implement ToolHandler (see template below)

# 3. Add module declaration
echo 'pub mod my_tool;' >> crates/edgecrab-tools/src/tools/mod.rs

# 4. Add to a toolset in toolsets.rs (or create a new toolset entry)

# 5. Add tool name to CORE_TOOLS or ACP_TOOLS in toolsets.rs if applicable

# 6. cargo build -- verify it compiles and appears in tool list
edgecrab tools list | grep my_tool
```

Minimal tool template:

```rust
use async_trait::async_trait;
use serde_json::Value;
use edgecrab_types::{ToolSchema, ToolError};
use crate::registry::{ToolContext, ToolHandler};

pub struct MyTool;

#[async_trait]
impl ToolHandler for MyTool {
    fn name(&self)    -> &'static str { "my_tool" }
    fn toolset(&self) -> &'static str { "file" }       // or a new toolset name

    fn schema(&self)  -> ToolSchema {
        ToolSchema {
            name: "my_tool".into(),
            description: "What this tool does and when to use it.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path" }
                },
                "required": ["path"]
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: Value, ctx: &ToolContext)
        -> Result<String, ToolError>
    {
        let path = args["path"].as_str()
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: self.name().into(),
                message: "path is required".into(),
            })?;

        // Check cancellation regularly if this might be slow
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::ExecutionFailed {
                tool: self.name().into(),
                message: "cancelled".into(),
            });
        }

        Ok(format!("processed {path}"))
    }
}

inventory::submit! { &MyTool as &dyn ToolHandler }
```

---

## Tips

> **Tip: Write tool descriptions for the model, not for humans.**
> The `description` field in `ToolSchema` is what the LLM reads to decide whether
> to call your tool. Be explicit about *when* to use it and what it returns.

> **Tip: Include the full input schema with `"required"` fields.**
> Tools that accept optional parameters should handle missing keys gracefully
> with defaults. The model may omit optional fields.

> **Tip: Use `check_fn()` for environment-dependent availability.**
> If your tool needs an API key or a running service, check it in `check_fn()`.
> This returns a capability-denied error with a helpful message rather than
> silently producing a cryptic execution failure.

---

## FAQ

**Q: How does the model know which tools are available?**
`ToolRegistry::get_definitions(enabled, disabled, ctx)` returns a `Vec<ToolSchema>`
filtered by active toolsets and `check_fn()`. This list is passed to the LLM
provider as the `tools` parameter on every API call.

**Q: Can a tool call another tool?**
Not directly — tools should not import each other or call `ToolRegistry::dispatch`
themselves. For sub-tasks, use `ctx.sub_agent_runner.run_task(...)` (the
`delegate_task` tool wraps this) or spawn an isolated agent via `fork_isolated`.

**Q: What is `MAX_CLARIFY_CHOICES = 4`?**
The `clarify` tool sends a clarification request with up to 4 options to the user.
The `Clarify` stream event carries a `oneshot::Sender<String>`; the frontend
renders the options and sends the user's choice back.

---

## Cross-references

- Tool catalogue (all 65 names) → [Tool Catalogue](./002_tool_catalogue.md)
- Toolset composition and aliases → [Toolset Composition](./003_toolset_composition.md)
- `ToolContext` and backends → [Tools Runtime](./004_tools_runtime.md)
- Error payloads sent back to model → [Error Handling](../002_architecture/004_error_handling.md)
