# Plugin Types — Skill, ToolServer, Script

**Status:** PROPOSED  
**Version:** 0.1.0  
**Date:** 2026-04-09  
**Cross-refs:** [001_adr_architecture], [002_adr_transport], [003_manifest], [007_registry], [008_host_api]

---

## 1. Overview

Three plugin kinds are supported in Phase 1. All implement the `Plugin` trait.

```
 Plugin (trait)
   │
   ├── SkillPlugin
   │     • reads SKILL.md + frontmatter
   │     • injects text into system prompt via PromptBuilder
   │     • NO subprocess, NO tool execution
   │     • ZERO runtime overhead at tool-call time
   │
   ├── ToolServerPlugin
   │     • spawns subprocess, speaks JSON-RPC 2.0 over stdio
   │     • exposes N tools to ToolRegistry at runtime
   │     • crash-isolated: subprocess death ≠ agent death
   │     • MCP-compatible: any MCP server is a valid ToolServerPlugin
   │
   └── ScriptPlugin
         • embeds Rhai interpreter (in-process, synchronous)
         • no subprocess overhead
         • sandboxed: disabled unsafe operations by default
         • good for simple transformations / formatting tools
```

---

## 2. SkillPlugin

### 2.1 Purpose

Injects curated knowledge into the agent's system prompt. This is the simplest
and most secure plugin kind — no code runs, no subprocess is spawned.

### 2.2 Directory Structure

```
~/.edgecrab/plugins/<name>/
    plugin.toml
    SKILL.md         ← REQUIRED: main prompt content
    references/      ← OPTIONAL: extra .md files listed in read_files
    templates/       ← OPTIONAL: template files
```

### 2.3 SKILL.md Frontmatter

```yaml
---
name:        Rust Patterns
description: Common Rust design patterns for idiomatic code.
category:    coding
platforms:   [cli, telegram, discord]
read_files:  [references/error-handling.md, references/traits.md]
---

# Main skill content here

Use these patterns when writing Rust code...
```

| Field | Type | Required | Description |
|---|---|---|---|
| `name` | string | YES | Display name (overrides plugin.toml name in prompt) |
| `description` | string | YES | Short description |
| `category` | string | NO | Category for grouping in `/plugins list` |
| `platforms` | string[] | NO | Restrict injection to named platforms only |
| `read_files` | string[] | NO | Additional files appended after main content |

### 2.4 Prompt Injection

SkillPlugin participates in `PromptBuilder` through a new `add_plugin_skills()` step:

```
PromptBuilder::build()
  1. identity
  2. platform hint
  3. datetime
  4. context files (SOUL.md, AGENTS.md)
  5. memory (MEMORY.md, USER.md)
  6. skills summary       ← existing SKILL.md (edgecrab-tools)
  7. [NEW] plugin skills  ← SkillPlugin content, appended here
  8. guidance constants
```

Plugin skills are appended AFTER the core skill summary. This preserves prompt
caching validity — the system prompt is stable across calls.

### 2.5 Enabling / Disabling

```yaml
# ~/.edgecrab/config.yaml
plugins:
  disabled:
    - "rust-patterns"  # SkillPlugin: simply omitted from PromptBuilder
```

When disabled, a SkillPlugin contributes zero bytes to the system prompt.
No error is returned; the agent runs without that skill injected.

### 2.6 Platform Filtering

If `platforms` is set in SKILL.md frontmatter, the skill is only injected
when the current session platform matches one of the listed values.

```
platforms: [cli, telegram]
→ skill injected for   CLI sessions and Telegram sessions
→ skill NOT injected for Discord, Slack, API, gateway sessions
```

---

## 3. ToolServerPlugin

### 3.1 Purpose

Exposes new tools to the agent by spawning a subprocess and communicating
via JSON-RPC 2.0 over stdio. The plugin process is separate from the agent —
it can crash, hang, or be killed without affecting the agent process.

### 3.2 Subprocess Lifecycle

```
 Agent Start
     │
     ▼
 [PluginRegistry::load_all()] iterates enabled tool-server plugins
     │
     ▼
 ToolServerPlugin::start()
     │
     ├── spawn subprocess (Command::new + stdin piped + stdout piped)
     │         │
     │         ▼
     │    [initialize handshake] ← waits startup_timeout_secs
     │         │
     │         ├── success → tools/list → populate tool table
     │         │
     │         └── timeout / error → PluginState::Failed(StartupTimeout)
     │
     └── store ChildProcess handle in ToolServerPlugin::process

 Agent runs (dispatching tool calls)
     │
     ▼
 Tool called → ToolServerPlugin::call_tool()
     │
     ├── send {"jsonrpc":"2.0","id":N,"method":"tools/call","params":{...}}
     ├── wait call_timeout_secs for response or BrokenPipe
     │
     │   [BrokenPipe / EOF] → plugin crashed
     │         │
     │         ├── restart_policy = "never" → ToolError::PluginCrashed
     │         ├── restart_policy = "once"  → restart once, retry call
     │         └── restart_policy = "always" → restart up to N times
     │
     └── success → parse result → return Ok(String)

 Agent Exit
     │
     ▼
 ToolServerPlugin::shutdown()
     │
     ├── send {"jsonrpc":"2.0","method":"notifications/shutdown"}
     ├── wait 5s for graceful exit
     └── if still alive → SIGKILL
```

### 3.3 Process Isolation Architecture

```
 ┌─────────────────────────────────────────────────────────────┐
 │  edgecrab agent process                                     │
 │                                                             │
 │  ToolServerPlugin {                                         │
 │      stdin_writer:  Arc<Mutex<ChildStdin>>,                 │
 │      stdout_reader: Arc<Mutex<BufReader<ChildStdout>>>,     │
 │      process:       Arc<Mutex<Child>>,                      │
 │      pending: HashMap<u64, oneshot::Sender<JsonValue>>,     │
 │  }                                              │           │
 │                                                 │ pipes     │
 └─────────────────────────────────────────────────┼───────────┘
                                                   │
                              ┌────────────────────┴───────────┐
                              │  plugin subprocess              │
                              │                                 │
                              │  read JSON-RPC from stdin       │
                              │  write JSON-RPC to stdout       │
                              │                                 │
                              │  (any language: Python, Node,   │
                              │   Go, Rust, Bash + jq, ...)     │
                              └────────────────────────────────┘
```

### 3.4 Reader Task

A dedicated `tokio::task` drives the stdout reader:

```rust
// Pseudocode — full spec in [007_registry.md]
tokio::spawn(async move {
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await? {
        let msg: JsonRpcMessage = serde_json::from_str(&line)?;
        match msg {
            JsonRpcMessage::Response { id, result } => {
                if let Some(tx) = pending.remove(&id) {
                    let _ = tx.send(result);
                }
            }
            // Host-API reverse calls: plugin sends a request TO us
            JsonRpcMessage::Request { id, method, params } if method.starts_with("host/") => {
                handle_host_api_call(id, method, params, &host_api_tx).await;
            }
            JsonRpcMessage::Notification { .. } => { /* log and ignore */ }
            _ => { /* malformed: log and continue */ }
        }
    }
    // Loop ended = subprocess closed stdout = subprocess exited
    // Notify all pending callers that plugin died
    flush_pending_with_error(&pending, PluginError::ProcessDied);
});
```

### 3.5 Tool Naming Convention

Plugin tools are exposed to the agent under their bare name (e.g., `create_github_issue`).
The agent sees no indication of whether a tool comes from `inventory!` or a plugin.

```
Compile-time tools:  read_file, write_file, terminal, ...
Plugin tools:        create_github_issue, list_github_issues, ...

SAME interface from agent's perspective.
```

Collisions between plugin tool names and compile-time tool names are caught at install time
(INV-4: compile-time tools win). See [006_security.md] §4.

### 3.6 MCP Compatibility

Any server that speaks MCP (Model Context Protocol 2024-11-05) is compatible as a
ToolServerPlugin — simply point `exec.command` at the MCP server binary.

Edgecrab-native ToolServer plugins gain one optional extension: [008_host_api.md]
reverse-call methods (`host/*`). Pure MCP servers that don't use these work without
any changes.

---

## 4. ScriptPlugin

### 4.1 Purpose

Lightweight, in-process plugin kind using the `rhai` interpreter. Good for:
- Simple string/data transformations
- Format converters (date, currency, unit)
- Agent-generated plugins (the `plugin_manage` tool writes Rhai scripts)

NOT suitable for: HTTP calls, subprocess spawning, long I/O operations.

### 4.2 Rhai Engine Configuration

```rust
let engine = rhai::Engine::new();
// Security: disable all unsafe operations by default
engine.disable_symbol("eval");          // no meta-eval
engine.set_max_operations(max_ops);     // INF loop guard
engine.set_max_call_levels(max_depth);  // stack overflow guard
engine.set_max_string_size(1_000_000); // 1MB string limit
engine.set_max_array_size(100_000);     // 100K array limit
engine.set_max_map_size(10_000);        // 10K map limit

// Only expose host functions listed in [capabilities].host
// "host:memory_read" → register_fn("memory_read", ...)
// "host:secret_get"  → register_fn("secret_get", ...)
register_declared_capabilities(&engine, &manifest.capabilities.host);
```

### 4.3 Script Interface Convention

Each Rhai script MUST expose a `call_tool(name, args)` function:

```javascript
// main.rhai
fn call_tool(name, args) {
    if name == "format_date" {
        let date = args["date"];
        let locale = args["locale"] ?? "en-US";
        // ... format logic ...
        return #{ success: true, output: formatted };
    }
    return #{ success: false, error: `Unknown tool: ${name}` };
}

// Optional: list available tools (called once at load time)
fn list_tools() {
    return [
        #{ name: "format_date",
           description: "Format a date string in a requested locale." }
    ];
}
```

### 4.4 Execution Model

```
ToolRegistry::dispatch("format_date", { "date": "2026-04-09" })
     │
     └── looks up tool → ScriptPlugin
           │
           ├── acquire engine lock (Mutex<rhai::Engine>)
           ├── set scope variables from args map
           ├── call call_tool("format_date", args) with timeout guard
           ├── parse return value as JSON string
           └── return Ok(result) or Err(ToolError::ScriptError)
```

⚠️ **Blocking note**: Rhai is synchronous. The engine is called via
`tokio::task::spawn_blocking()` to avoid blocking the async runtime:

```rust
let result = tokio::task::spawn_blocking(move || {
    engine.call_fn::<Dynamic>(&mut scope, &ast, "call_tool", (name, args))
}).await??;
```

### 4.5 Agent-Generated ScriptPlugins

The `plugin_manage` tool allows the agent to create new ScriptPlugins at runtime:

```
Agent: "Create a plugin that converts temperatures"
→ plugin_manage::create({
    name: "temp-converter",
    kind: "script",
    rhai_code: "fn call_tool(name, args) { ... }",
    tools: [{ name: "convert_temperature", description: "..." }],
    capabilities: { host: [] }
})
→ writes plugin.toml + main.rhai to ~/.edgecrab/plugins/temp-converter/
→ security scans the rhai code
→ registers with PluginRegistry (no restart needed)
```

---

## 5. Type System (Rust)

```rust
/// The unified plugin trait. All three kinds implement this.
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Plugin name (from manifest).
    fn name(&self) -> &str;

    /// Plugin kind.
    fn kind(&self) -> PluginKind;

    /// The parsed, validated manifest.
    fn manifest(&self) -> &PluginManifest;

    /// Current operational state.
    fn state(&self) -> PluginState;

    /// Tools this plugin exposes (empty for SkillPlugin).
    async fn list_tools(&self) -> Vec<ToolSchema>;

    /// Call a tool by name. Only valid for ToolServer and Script kinds.
    async fn call_tool(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, PluginError>;

    /// For SkillPlugin: return the content to inject into the system prompt.
    /// For other kinds: returns None.
    fn prompt_injection(&self) -> Option<Cow<'_, str>>;

    /// Start the plugin (spawn subprocess, compile script).
    /// MUST be idempotent — calling twice on a running plugin is a no-op.
    async fn start(&self) -> Result<(), PluginError>;

    /// Gracefully shut down the plugin.
    async fn shutdown(&self) -> Result<(), PluginError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    Skill,
    ToolServer,
    Script,
    #[serde(other)]
    Wasm, // Reserved, not implemented
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginState {
    /// Discovered but not yet started.
    Idle,
    /// Subprocess starting / Rhai compiling.
    Starting,
    /// Fully operational.
    Running,
    /// Intentionally stopped by user.
    Disabled,
    /// Plugin suffered a fatal error.
    Failed(String),
}
```
