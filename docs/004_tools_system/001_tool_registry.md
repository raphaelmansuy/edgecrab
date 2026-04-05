# 004.001 — Tool Registry

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 002.001 Architecture](../002_architecture/001_system_architecture.md) | [→ 004.002 Tool Catalogue](002_tool_catalogue.md)
> **Source**: `edgecrab-tools/src/registry.rs` — verified against real implementation

## 1. Registry Design

hermes-agent uses a **runtime** Python decorator registry. EdgeCrab uses a **compile-time** registry via the `inventory` crate — inspired by [OpenClaw](https://github.com/openai/openai-claw) and [Nous Hermes](https://nousresearch.com):

```
┌──────────────────────────────────────────────────────────┐
│                     ToolRegistry                         │
│                                                          │
│  inventory::iter ──→ HashMap<name, &dyn ToolHandler>     │
│                                                          │
│  dispatch("read_file", args, ctx)                        │
│      │                                                   │
│      ├── exact match? → handler.execute(args, ctx)       │
│      │                                                   │
│      └── no match? → fuzzy_match (strsim) → suggestion   │
│                                                          │
│  get_definitions(enabled, disabled, ctx)                  │
│      → filter by toolset + availability + check_fn       │
│      → Vec<ToolSchema> for LLM API call                  │
└──────────────────────────────────────────────────────────┘
```

**Key advantage over Python**: The linker guarantees all registered tools are present. No forgotten imports, no typos in registration calls. `cargo build` fails if a tool file is missing.

## 2. ToolHandler Trait

Every tool implements this trait. 8 methods — 4 required, 4 with defaults:

```rust
// edgecrab-tools/src/registry.rs

#[async_trait]
pub trait ToolHandler: Send + Sync + 'static {
    /// Unique tool name identifier (e.g., "read_file", "terminal")
    fn name(&self) -> &'static str;

    /// Toolset membership — for enable/disable filtering (e.g., "file", "web")
    fn toolset(&self) -> &'static str;

    /// OpenAI-format function schema sent to the LLM
    fn schema(&self) -> ToolSchema;

    /// Execute the tool with parsed JSON arguments
    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError>;

    /// Startup availability check (env vars present, binary exists, etc.)
    /// Called once at registry build time.
    fn is_available(&self) -> bool { true }

    /// Per-request gate (gateway running, specific token present, etc.)
    /// Distinct from is_available: runs on EVERY dispatch.
    fn check_fn(&self, _ctx: &ToolContext) -> bool { true }

    /// Whether this tool is safe for parallel execution
    fn parallel_safe(&self) -> bool { false }

    /// Emoji displayed in TUI tool progress
    fn emoji(&self) -> &'static str { "⚡" }
}
```

### Two-Layer Gating

```
is_available()          ← called ONCE at startup (binary exists? env var set?)
     │
     └──→ tool included in registry
              │
check_fn(ctx) ← called on EVERY dispatch (gateway running? Honcho active?)
     │
     └──→ tool actually executes
```

Example: `ha_list_entities` uses `is_available()` to check `HASS_TOKEN` env var. `send_message` uses `check_fn()` to verify gateway is running at call time.

## 3. Compile-Time Registration

```rust
// In each tool file (e.g. file_read.rs):
inventory::submit! { &ReadFileTool as &dyn ToolHandler }

// In registry.rs — collect all submitted handlers:
inventory::collect!(&'static dyn ToolHandler);

// At startup — build HashMap once:
pub fn new() -> Self {
    let mut tools = HashMap::new();
    let mut toolset_index = HashMap::new();
    for handler in inventory::iter::<&dyn ToolHandler> {
        tools.insert(handler.name(), *handler);
        toolset_index.entry(handler.toolset())
            .or_default()
            .push(handler.name());
    }
    Self { tools, toolset_index, dynamic_tools: HashMap::new() }
}
```

## 4. ToolRegistry Struct & API

```rust
pub struct ToolRegistry {
    /// name → handler lookup (static, compile-time registered)
    tools: HashMap<&'static str, &'static dyn ToolHandler>,
    /// toolset → [tool_names] for group operations
    toolset_index: HashMap<&'static str, Vec<&'static str>>,
    /// Dynamic tools registered at runtime (plugins, MCP)
    dynamic_tools: HashMap<String, Box<dyn ToolHandler>>,
}
```

### Key Methods

| Method | Signature | Purpose |
|--------|-----------|---------|
| `new()` | `→ Self` | Build from inventory (once at startup) |
| `get_definitions()` | `(enabled, disabled, &ToolContext) → Vec<ToolSchema>` | Filtered schemas for LLM API call |
| `dispatch()` | `(name, args, &ToolContext) → Result<String, ToolError>` | Execute tool by name (fuzzy fallback) |
| `register_dynamic()` | `(Box<dyn ToolHandler>)` | Add runtime tool (MCP, plugins) |
| `tool_names()` | `→ Vec<&str>` | All registered names (static + dynamic) |
| `toolset_names()` | `→ Vec<&str>` | All toolset names |
| `tools_in_toolset()` | `(toolset) → Vec<&str>` | Tools in a specific toolset |
| `toolset_summary()` | `→ Vec<(String, usize)>` | Toolset → count pairs |
| `is_parallel_safe()` | `(name) → bool` | Check parallel safety flag |

### `get_definitions()` — Three-Context Filter

```
get_definitions(enabled, disabled, ctx)
    ├── 1. is_available() ← startup check
    ├── 2. check_fn(ctx)  ← runtime gate
    ├── 3. enabled filter (whitelist — None means include all)
    ├── 4. disabled filter (blacklist — None means exclude none)
    └── chain static + dynamic → Vec<ToolSchema>
```

Note: Takes `&ToolContext` parameter — this is how runtime gating works. The doc's old version omitted `ctx` from the signature.

### `dispatch()` — Fuzzy Fallback

On name mismatch, uses Levenshtein distance (strsim crate, threshold ≤ 3) to suggest the closest tool — helps the LLM self-correct typos:

```rust
Err(ToolError::NotFound(format!(
    "Unknown tool '{}'. Did you mean '{}'?", name, suggestion
)))
```

Dispatch checks both `tools` (static) and `dynamic_tools` maps. Also verifies `check_fn()` before execution — returns `ToolError::Unavailable` if gating fails.

## 5. ToolContext — Shared Execution Context

Passed to every tool's `execute()` method. **18 fields** — verified against source:

```rust
pub struct ToolContext {
    pub task_id: String,
    pub cwd: PathBuf,
    pub session_id: String,
    pub user_task: Option<String>,
    pub cancel: CancellationToken,
    pub config: AppConfigRef,                   // Arc<RwLock<AppConfig>>
    pub state_db: Option<Arc<SessionDb>>,
    pub platform: Platform,
    pub process_table: Option<Arc<ProcessTable>>,
    pub provider: Option<Arc<dyn LLMProvider>>,
    pub tool_registry: Option<Arc<ToolRegistry>>,
    pub delegate_depth: u32,
    pub sub_agent_runner: Option<Arc<dyn SubAgentRunner>>,
    pub clarify_tx: Option<UnboundedSender<ClarifyRequest>>,
    pub on_skills_changed: Option<Arc<dyn Fn() + Send + Sync>>,
    pub gateway_sender: Option<Arc<dyn GatewaySender>>,
    pub origin_chat: Option<(String, String)>,
}
```

### Field Purpose Guide

| Field | Type | Why |
|-------|------|-----|
| `task_id` | `String` | Unique per tool invocation |
| `cwd` | `PathBuf` | Working directory — path jail root for file tools |
| `session_id` | `String` | Stable per-conversation identifier |
| `user_task` | `Option<String>` | Original task description (for sub-agent context) |
| `cancel` | `CancellationToken` | Propagates Ctrl+C into tool execution |
| `config` | `AppConfigRef` | Application config — `Arc<RwLock<AppConfig>>` (NOT `Arc<AppConfig>`) |
| `state_db` | `Option<Arc<SessionDb>>` | Session database — None in tests |
| `platform` | `Platform` | CLI vs gateway vs ACP — affects tool behavior |
| `process_table` | `Option<Arc<ProcessTable>>` | Background process management — None in tests/ACP |
| `provider` | `Option<Arc<dyn LLMProvider>>` | LLM for tools that call LLMs (vision, delegate) |
| `tool_registry` | `Option<Arc<ToolRegistry>>` | For sub-agent delegation (delegate_task needs tools) |
| `delegate_depth` | `u32` | 0=root, 1=child, 2+=blocked — prevents infinite recursion |
| `sub_agent_runner` | `Option<Arc<dyn SubAgentRunner>>` | Breaks circular dep: tools crate → core crate |
| `clarify_tx` | `Option<UnboundedSender<ClarifyRequest>>` | One-shot channel to TUI for user clarification |
| `on_skills_changed` | `Option<Arc<dyn Fn()>>` | Callback to invalidate skills prompt cache |
| `gateway_sender` | `Option<Arc<dyn GatewaySender>>` | Send messages to external platforms |
| `origin_chat` | `Option<(String, String)>` | (platform, chat_id) for cron delivery routing |

> **Note**: There is NO `agent_ref: Weak<Agent>` field. The previous doc was wrong. Circular dependency is broken via `SubAgentRunner` trait + `sub_agent_runner` field.

## 6. SubAgentRunner Trait

Breaks circular dependency between edgecrab-tools (defines) and edgecrab-core (implements):

```rust
#[async_trait]
pub trait SubAgentRunner: Send + Sync {
    async fn run_task(
        &self,
        goal: &str,
        system_prompt: &str,
        enabled_toolsets: Vec<String>,
        max_iterations: u32,
        model_override: Option<String>,
    ) -> Result<SubAgentResult, String>;
}

pub struct SubAgentResult {
    pub summary: String,
    pub api_calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
}
```

## 7. ClarifyRequest Channel

The `clarify` tool communicates with the TUI via an `mpsc` channel:

```rust
pub struct ClarifyRequest {
    pub question: String,
    /// One-shot channel — TUI sends user's answer back here
    pub response_tx: tokio::sync::oneshot::Sender<String>,
}
```

```
┌──────────┐   ClarifyRequest    ┌──────────┐
│  clarify │ ───(mpsc::tx)────→  │   TUI    │
│   tool   │                     │  layer   │
│          │ ←─(oneshot::rx)───  │          │
└──────────┘    user answer      └──────────┘
```

Only available in CLI/interactive mode (`clarify_tx` is `Some`). Gateway and ACP modes fall back to returning a `[CLARIFY]` marker in the tool result.

## 8. GatewaySender Trait

Allows tools to send messages to external platforms:

```rust
#[async_trait]
pub trait GatewaySender: Send + Sync + 'static {
    async fn send_message(
        &self,
        platform: &str,
        recipient: &str,
        message: &str,
    ) -> Result<(), String>;

    async fn list_targets(&self) -> Result<Vec<String>, String>;
}
```

## 9. edgequake-llm Bridge

Converts EdgeCrab's `ToolSchema` to edgequake-llm's `ToolDefinition` for API calls:

```rust
pub fn to_llm_definitions(schemas: &[ToolSchema]) -> Vec<edgequake_llm::ToolDefinition> {
    schemas.iter().map(|s| {
        edgequake_llm::ToolDefinition::function(&s.name, &s.description, s.parameters.clone())
    }).collect()
}
```

## 10. Adding a New Tool

**Step 1:** Create `crates/edgecrab-tools/src/tools/my_tool.rs`:

```rust
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use edgecrab_types::{ToolError, ToolSchema};
use crate::registry::{ToolContext, ToolHandler};

pub struct MyTool;

#[derive(Deserialize)]
struct MyArgs {
    param: String,
}

#[async_trait]
impl ToolHandler for MyTool {
    fn name(&self)    -> &'static str { "my_tool" }
    fn toolset(&self) -> &'static str { "my_toolset" }
    fn emoji(&self)   -> &'static str { "🔧" }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "my_tool".into(),
            description: "Does X given Y.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "param": { "type": "string", "description": "..." }
                },
                "required": ["param"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: MyArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::InvalidArgs {
                tool: "my_tool".into(),
                message: e.to_string(),
            })?;
        Ok(json!({"result": args.param}).to_string())
    }
}

// Compile-time registration — linker guarantees this runs
inventory::submit! { &MyTool as &dyn ToolHandler }
```

**Step 2:** Add `pub mod my_tool;` to `crates/edgecrab-tools/src/tools/mod.rs`.

**Step 3:** (Optional) Add tool name to `CORE_TOOLS` in `toolsets.rs` and/or a toolset alias in `resolve_alias()`.

No other files need changes. `cargo build` verifies registration.

## 11. Rust Advantage — Compile-Time Tool Registration

| Aspect | hermes-agent (Python) | EdgeCrab (Rust) |
|--------|----------------------|-----------------|
| Registration | Runtime import, can fail silently | Compile-time via `inventory`, guaranteed |
| Type safety | `dict` args, runtime typecheck | `serde_json::Value` + `Deserialize` |
| Dispatch | `json.loads` + `handler(args)` | Zero-copy deserialization, typed dispatch |
| Parallel safety | Runtime frozenset check | Trait method, compile-time knowable |
| Discovery cost | O(n) imports at startup (~200ms) | Zero — linker resolves at build time |
| Dead tool | Silently registered, never called | Compiler warns unused |
| Dynamic tools | Python decorator | `register_dynamic()` for MCP/plugins |
