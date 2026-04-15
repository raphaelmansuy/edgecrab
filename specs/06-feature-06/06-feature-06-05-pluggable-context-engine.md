# ADR-0605: Pluggable Context Engine

| Field       | Value                                                   |
|-------------|---------------------------------------------------------|
| Status      | Implemented                                             |
| Date        | 2026-04-14                                              |
| Implements  | hermes-agent PR #7464                                   |
| Crate       | `edgecrab-core`, `edgecrab-plugins`                     |
| Files       | `crates/edgecrab-core/src/context_engine.rs`            |
|             | `crates/edgecrab-plugins/src/context.rs`                |

---

## 1. Context

EdgeCrab's context management is currently hardcoded: `compression.rs` handles
context compression when token usage exceeds a threshold. hermes-agent v0.9.0
introduces a **pluggable context engine** slot — an abstraction that lets
users swap in custom context engines via the plugin system, controlling what
the agent sees each turn (filtering, summarization, domain-specific injection).

EdgeCrab already has `edgecrab-plugins` with plugin discovery, manifest
parsing, and security scanning. This ADR extends it with context engine support.

---

## 2. First Principles

| Principle       | Application                                                   |
|-----------------|---------------------------------------------------------------|
| **SRP**         | Context engine trait owns context shaping; agent loop agnostic|
| **OCP**         | New engines via plugins, zero changes to `conversation.rs`    |
| **DIP**         | Agent depends on `ContextEngine` trait, not concrete impl     |
| **DRY**         | Built-in compressor becomes the default engine impl           |
| **ISP**         | Minimal trait surface: 4 lifecycle + 1 tool schema method     |
| **Code is Law** | hermes-agent `run_agent.py:L1271-1351` as reference           |

---

## 3. Architecture

```
+-------------------------------------------------------------------+
|                     edgecrab-core                                  |
|                                                                    |
|  +-------------------+       +-------------------------------+     |
|  | Agent             |       | dyn ContextEngine             |     |
|  | .context_engine   +------>|                               |     |
|  | (Arc<dyn ...>)    |       | on_session_start()            |     |
|  +-------------------+       | on_session_end()              |     |
|           |                  | on_session_reset()            |     |
|           v                  | get_tool_schemas() -> Vec     |     |
|  +-------------------+       | is_available() -> bool        |     |
|  | conversation.rs   |       +------+------+-----------------+     |
|  | execute_loop()    |              |      |                       |
|  | - injects engine  |              |      |                       |
|  |   tools into      |    +---------+      +----------+            |
|  |   tool_schemas    |    |                           |            |
|  +-------------------+    v                           v            |
|               +--------------------+   +-------------------+      |
|               | BuiltinCompressor  |   | PluginEngine      |      |
|               | (existing impl)    |   | (loaded from      |      |
|               | compression.rs     |   |  edgecrab-plugins)|      |
|               +--------------------+   +-------------------+      |
+-------------------------------------------------------------------+
```

---

## 4. Data Model

### 4.1 Context Engine Trait

```rust
// crates/edgecrab-core/src/context_engine.rs

#[async_trait]
pub trait ContextEngine: Send + Sync + 'static {
    /// Human-readable engine name (e.g. "compressor", "lcm", "custom")
    fn name(&self) -> &str;

    /// Max context window this engine supports (tokens)
    fn context_length(&self) -> usize;

    /// Token threshold at which compression/shaping triggers
    fn threshold_tokens(&self) -> usize;

    /// Additional tool schemas this engine injects into the agent's toolset.
    /// E.g. lcm_grep, lcm_describe, lcm_expand for a retrieval engine.
    fn get_tool_schemas(&self) -> Vec<ToolSchema> {
        vec![]
    }

    /// Called once at session start. Engine may initialize state.
    async fn on_session_start(&self, ctx: ContextEngineSessionCtx) -> anyhow::Result<()>;

    /// Called when session ends (CLI exit, /reset, gateway timeout).
    async fn on_session_end(&self, session_id: &str, messages: &[Message]) -> anyhow::Result<()>;

    /// Called when session is reset without ending.
    async fn on_session_reset(&self) -> anyhow::Result<()>;

    /// Whether this engine is available (e.g. required API keys present).
    fn is_available(&self) -> bool;
}

pub struct ContextEngineSessionCtx {
    pub session_id: String,
    pub edgecrab_home: PathBuf,
    pub platform: Platform,
    pub model: String,
    pub context_length: usize,
}
```

### 4.2 Built-in Compressor as Default Engine

```rust
// crates/edgecrab-core/src/builtin_compressor_engine.rs

pub struct BuiltinCompressorEngine {
    context_length: usize,
    threshold: f64,
    protect_last_n: usize,
}

impl ContextEngine for BuiltinCompressorEngine {
    fn name(&self) -> &str { "compressor" }
    fn context_length(&self) -> usize { self.context_length }
    fn threshold_tokens(&self) -> usize {
        (self.context_length as f64 * self.threshold) as usize
    }
    fn get_tool_schemas(&self) -> Vec<ToolSchema> { vec![] }
    async fn on_session_start(&self, _ctx: ContextEngineSessionCtx) -> Result<()> { Ok(()) }
    async fn on_session_end(&self, _id: &str, _msgs: &[Message]) -> Result<()> { Ok(()) }
    async fn on_session_reset(&self) -> Result<()> { Ok(()) }
    fn is_available(&self) -> bool { true }
}
```

### 4.3 Configuration

```yaml
# ~/.edgecrab/config.yaml
context:
  engine: "compressor"    # default; or name of plugin in plugins/
```

### 4.4 Engine Loading Cascade

```
AgentBuilder::build()
  |
  +-- 1. Read config.context.engine (default: "compressor")
  +-- 2. If "compressor": use BuiltinCompressorEngine
  +-- 3. Else: try edgecrab_plugins::load_context_engine(name)
  +-- 4. If plugin not found: warn, fallback to BuiltinCompressorEngine
  +-- 5. Inject engine.get_tool_schemas() into agent's tool list
  +-- 6. Call engine.on_session_start()
```

---

## 5. Plugin Engine Loading

### 5.1 Plugin Discovery

```rust
// crates/edgecrab-plugins/src/context.rs

pub fn discover_context_engines() -> Vec<(String, String, bool)> {
    // Scan ~/.edgecrab/plugins/context_engine/<name>/
    // Each must have a manifest.yaml with:
    //   name, description, command, args
    // Returns (name, description, available)
}

pub fn load_context_engine(name: &str) -> anyhow::Result<Arc<dyn ContextEngine>> {
    // Load plugin manifest from plugins/context_engine/<name>/manifest.yaml
    // Spawn subprocess, communicate via JSON-RPC stdio
    // Wrap in PluginContextEngine adapter
}
```

### 5.2 Plugin Protocol (JSON-RPC over stdio)

```
Host -> Plugin:
  {"method": "on_session_start", "params": {"session_id": "...", ...}}
  {"method": "on_session_end", "params": {"session_id": "...", "messages": [...]}}
  {"method": "on_session_reset", "params": {}}
  {"method": "get_tool_schemas", "params": {}}
  {"method": "handle_tool_call", "params": {"name": "lcm_grep", "args": {...}}}

Plugin -> Host:
  {"result": {...}}
  {"error": {"code": -1, "message": "..."}}
```

---

## 6. Tool Injection

When a context engine provides tool schemas, they are injected into the
agent's tool list alongside core tools:

```
Agent tool list at session start:
  [core_tools...] + [mcp_tools...] + [context_engine_tools...]

Tool dispatch:
  if tool_name in context_engine_tool_names:
    -> route to context_engine.handle_tool_call(name, args)
  else:
    -> route to ToolRegistry.dispatch(name, args)
```

This allows engines to add domain-specific tools (e.g., `lcm_grep`,
`lcm_describe`, `lcm_expand` for a retrieval-augmented context engine).

---

## 7. Edge Cases & Roadblocks

| #  | Edge Case                              | Remediation                                      | Source                          |
|----|----------------------------------------|--------------------------------------------------|---------------------------------|
| 1  | Plugin process crashes mid-session     | Fallback to BuiltinCompressorEngine + warn       | `run_agent.py:L1312`            |
| 2  | Engine tool name conflicts with core   | Prefix plugin tools with engine name `<eng>_`    | New — namespace isolation       |
| 3  | Engine returns invalid tool schemas    | Validate at load time, reject malformed schemas  | New — defensive loading         |
| 4  | Config engine name typo                | Warn + fallback to compressor                    | `run_agent.py:L1312`            |
| 5  | Engine modifies context mid-session    | Engine MUST NOT rebuild system prompt            | Prompt caching constraint       |
| 6  | Engine adds too many tools             | Cap at 20 engine-provided tools                  | New — prevents schema bloat     |
| 7  | Plugin subprocess timeout              | 30s timeout on all JSON-RPC calls                | New — prevents hangs            |
| 8  | Engine unavailable (missing API key)   | `is_available()` returns false → skip + warn     | `run_agent.py:L1329`            |
| 9  | Multiple engines configured            | Only one active at a time (config is singular)   | hermes-agent design decision    |
| 10 | on_session_end called after crash      | Best-effort only — engine may have already exited| New — fire-and-forget cleanup   |

---

## 8. Implementation Plan

### 8.1 Files to Create

| File                                               | Purpose                              |
|----------------------------------------------------|--------------------------------------|
| `crates/edgecrab-core/src/context_engine.rs`       | `ContextEngine` trait definition     |
| `crates/edgecrab-core/src/builtin_compressor_engine.rs` | Default engine wrapping compression.rs |
| `crates/edgecrab-plugins/src/context.rs`           | Plugin engine discovery + loading    |

### 8.2 Files to Modify

| File                                               | Change                                         |
|----------------------------------------------------|-------------------------------------------------|
| `crates/edgecrab-core/src/agent.rs`                | Add `context_engine` field to Agent/AgentBuilder|
| `crates/edgecrab-core/src/conversation.rs`         | Inject engine tools into tool schemas           |
| `crates/edgecrab-core/src/config.rs`               | Add `context.engine` config key                 |
| `crates/edgecrab-core/src/lib.rs`                  | Export new modules                              |
| `crates/edgecrab-plugins/src/lib.rs`               | Export context module                           |

### 8.3 Test Matrix

| Test                                  | Validates                                   |
|---------------------------------------|---------------------------------------------|
| `test_builtin_engine_default`         | Compressor used when no config              |
| `test_engine_tool_injection`          | Engine tools appear in agent tool list      |
| `test_engine_fallback_on_missing`     | Unknown engine name falls back + warns      |
| `test_engine_lifecycle`               | start -> reset -> end lifecycle             |
| `test_engine_tool_dispatch`           | Engine tool calls routed correctly          |
| `test_engine_tool_name_prefix`        | No conflicts with core tools                |
| `test_engine_tool_cap`               | Rejects engine with >20 tools               |
| `test_plugin_engine_json_rpc`         | JSON-RPC roundtrip with subprocess          |

---

## 9. Prompt Caching Constraint

**CRITICAL**: The context engine MUST NOT modify the system prompt after
session start. The only thing engines can do is:
1. Inject additional tools at session start (one-time)
2. Handle tool calls during the session
3. Perform cleanup at session end

This preserves Anthropic prompt cache validity. The system prompt is assembled
once by `prompt_builder.rs` and cached in `SessionState.cached_system_prompt`.

---

## 10. Acceptance Criteria

- [x] `ContextEngine` trait defined with lifecycle + tool methods
- [x] `BuiltinCompressorEngine` wraps existing compression.rs logic
- [x] `AgentBuilder` accepts custom context engine via `.context_engine()`
- [x] Engine tool schemas injected into agent tool list (capped at 20)
- [x] Engine tool calls dispatched to engine (not ToolRegistry)
- [x] `PluginContextEngine` (subprocess JSON-RPC stdio) in `edgecrab-core`
- [x] `find_context_engine_manifest()` in `edgecrab-plugins` (no trait dep)
- [x] `load_context_engine()` cascade: builtin > plugin > warn+fallback
- [x] `build_agent()` in CLI wires context engine from `config.context.engine`
- [x] 30s timeout on all plugin subprocess JSON-RPC calls
- [x] Config key `context.engine` with default (None → compressor)
- [x] System prompt NOT modified by engine (cache preservation)
- [x] 8 unit tests in `context_engine.rs`, 6 in `context.rs`

---

## 11. Python Plugin Example

This section shows how to write a Python context engine plugin.

### 11.1 Plugin Directory Structure

```
~/.edgecrab/plugins/context_engine/my-engine/
├── manifest.yaml
└── engine.py
```

### 11.2 manifest.yaml

```yaml
name: my-engine
description: "Custom context engine that filters messages by relevance"
command: python3
args: ["-m", "engine"]
```

### 11.3 engine.py (JSON-RPC 2.0 over stdio)

```python
#!/usr/bin/env python3
"""
EdgeCrab context engine plugin — JSON-RPC 2.0 over stdio.

Protocol:
  Host → Plugin: {"jsonrpc": "2.0", "id": N, "method": "...", "params": {...}}
  Plugin → Host: {"jsonrpc": "2.0", "id": N, "result": ...}
               | {"jsonrpc": "2.0", "id": N, "error": {"code": -1, "message": "..."}}
"""
import json
import sys


def get_tool_schemas() -> list:
    """Return tool schemas this engine injects into the agent."""
    return [
        {
            "name": "my_engine_search",
            "description": "Search the context engine knowledge base.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query"}
                },
                "required": ["query"],
            },
        }
    ]


def handle_tool_call(name: str, args: dict) -> str:
    """Handle a tool call routed to this engine. Return JSON string."""
    if name == "my_engine_search":
        query = args.get("query", "")
        # Your custom logic here
        return json.dumps({"results": [f"Result for: {query}"]})
    return json.dumps({"error": f"Unknown tool: {name}"})


def on_session_start(params: dict) -> None:
    """Called at session start. Initialize state as needed."""
    pass  # e.g. load vector index, open DB connection


def on_session_end(params: dict) -> None:
    """Called at session end. Flush caches, close connections."""
    pass


def on_session_reset() -> None:
    """Called on /reset. Clear per-session state."""
    pass


# ── JSON-RPC 2.0 dispatcher ─────────────────────────────────────────

DISPATCH = {
    "get_tool_schemas": lambda p: get_tool_schemas(),
    "handle_tool_call": lambda p: handle_tool_call(p["name"], p["args"]),
    "on_session_start": lambda p: (on_session_start(p), None)[1],
    "on_session_end": lambda p: (on_session_end(p), None)[1],
    "on_session_reset": lambda p: (on_session_reset(), None)[1],
}


def main():
    for raw_line in sys.stdin:
        line = raw_line.strip()
        if not line:
            continue
        try:
            req = json.loads(line)
            method = req.get("method", "")
            params = req.get("params", {})
            req_id = req.get("id")

            if method in DISPATCH:
                result = DISPATCH[method](params)
                resp = {"jsonrpc": "2.0", "id": req_id, "result": result}
            else:
                resp = {
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "error": {"code": -32601, "message": f"Method not found: {method}"},
                }
        except Exception as exc:
            resp = {
                "jsonrpc": "2.0",
                "id": req.get("id") if "req" in dir() else None,
                "error": {"code": -1, "message": str(exc)},
            }
        print(json.dumps(resp), flush=True)


if __name__ == "__main__":
    main()
```

### 11.4 Activation

```yaml
# ~/.edgecrab/config.yaml
context:
  engine: "my-engine"
```

The engine subprocess is spawned once per session. All JSON-RPC calls have
a 30-second timeout. If the subprocess exits or times out, EdgeCrab falls
back to the built-in compressor engine with a warning.
