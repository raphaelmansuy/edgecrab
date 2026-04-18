# EdgeCrab SDK — Custom Tools & Extensibility Deep-Dive

> **Cross-references:** [SPEC](02-SPEC.md) | [TUTORIAL](04-TUTORIAL.md) | [ADR](05-ADR.md) | [DEVELOPER-DOCS](08-DEVELOPER-DOCS.md)

---

## WHY This Document Exists

The spec says "custom tools in 5 minutes." That's marketing. This document answers the hard engineering questions:

1. How do Python/Node.js tool functions actually become Rust `ToolHandler` implementations?
2. What are the 4 extensibility layers and when should you use each?
3. What are the real performance and safety trade-offs?
4. What code actually runs when a custom tool is called across the FFI boundary?

Without answers to these questions, the SDK is vaporware with pretty diagrams.

---

## The 4 Extensibility Layers

EdgeCrab has **four distinct ways** to extend tools. Each layer serves a different use case. Understanding which layer to use — and why — is the single most important architectural decision for SDK users.

```
+------------------------------------------------------------------------+
|                   EdgeCrab Tool Extensibility Stack                     |
+------------------------------------------------------------------------+
|                                                                        |
|  Layer 4: MCP Servers (External Process, JSON-RPC 2.0)                |
|  +------------------------------------------------------------------+ |
|  | ANY language. ANY process. Standardized protocol.                 | |
|  | Use when: Integrating existing services, cross-language tools,    | |
|  | enterprise environments, team-shared tool servers.                | |
|  | Latency: ~5-50ms/call (process spawn + JSON-RPC + pipe I/O)      | |
|  +------------------------------------------------------------------+ |
|                                                                        |
|  Layer 3: Tool Server Plugins (External Process, EdgeCrab Protocol)   |
|  +------------------------------------------------------------------+ |
|  | JSON-RPC 2.0 subprocess. Managed lifecycle (restart, idle kill).  | |
|  | Use when: Writing plugins in any language, need sandboxing,       | |
|  | untrusted plugin code, complex dependency requirements.           | |
|  | Latency: ~2-10ms/call (warm subprocess, JSON-RPC)                | |
|  +------------------------------------------------------------------+ |
|                                                                        |
|  Layer 2: Rhai Scripts (Embedded, Sandboxed)                          |
|  +------------------------------------------------------------------+ |
|  | Lightweight scripting language. Sandboxed (op limits, no I/O).    | |
|  | Use when: Simple transformations, config-driven logic, safe       | |
|  | user-contributed scripts, rapid prototyping.                      | |
|  | Latency: ~0.01-0.1ms/call (interpreted in-process)               | |
|  +------------------------------------------------------------------+ |
|                                                                        |
|  Layer 1: Native (In-Process, Compiled)                               |
|  +------------------------------------------------------------------+ |
|  | Rust ToolHandler trait. Zero-overhead. Full access to ToolContext. | |
|  | Use when: Performance-critical tools, tools that need full agent  | |
|  | context (session, provider, process table), first-party tools.    | |
|  | Latency: ~0.001ms/call (function call, no serialization)          | |
|  +------------------------------------------------------------------+ |
|                                                                        |
+------------------------------------------------------------------------+
```

**Critical correction from previous specs:** The specs claim "WASM + Lua" plugin support. This is **incorrect**. The actual plugin system uses **Rhai** (a Rust-native scripting language) for embedded scripts and **Tool Servers** (JSON-RPC 2.0 subprocesses) for external tools. There is no WASM runtime and no Lua interpreter in the codebase.

---

## Layer 1: Native Rust Tools (ToolHandler Trait)

This is how EdgeCrab's 100+ built-in tools are implemented. It's also the path for SDK users writing Rust tools.

### How It Works Today

```
+---------------------------------------------------------------+
|                    Native Tool Lifecycle                       |
+---------------------------------------------------------------+
|                                                               |
|  COMPILE TIME:                                                |
|  1. Tool implements ToolHandler trait                         |
|  2. inventory::submit!() registers a static reference         |
|  3. inventory::collect!() gathers all registered tools        |
|                                                               |
|  RUNTIME:                                                     |
|  4. ToolRegistry::new() iterates inventory, indexes by name   |
|  5. Agent ReAct loop calls registry.dispatch(name, args)      |
|  6. dispatch() finds handler, calls handler.execute(args, ctx)|
|  7. Handler returns Result<String, ToolError>                 |
|                                                               |
|  ZERO OVERHEAD: No serialization, no IPC, no allocation       |
|  beyond the tool's own work.                                  |
+---------------------------------------------------------------+
```

### The ToolHandler Trait (Actual API)

```rust
// From crates/edgecrab-tools/src/registry.rs
#[async_trait]
pub trait ToolHandler: Send + Sync {
    /// Unique tool name (used in LLM function calling)
    fn name(&self) -> &'static str;

    /// Alternative names the model might use
    fn aliases(&self) -> &[&'static str] { &[] }

    /// Toolset grouping (e.g., "file", "web", "terminal")
    fn toolset(&self) -> &'static str;

    /// JSON Schema for LLM function calling
    fn schema(&self) -> ToolSchema;

    /// Execute the tool with parsed arguments
    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError>;

    /// Check if tool requirements are met (API keys, binaries, etc.)
    fn is_available(&self) -> bool { true }

    /// Availability check function (alternative to is_available)
    fn check_fn(&self) -> Option<fn() -> bool> { None }

    /// Whether tool can safely run concurrently
    fn parallel_safe(&self) -> bool { true }

    /// Display emoji for TUI
    fn emoji(&self) -> &'static str { "🔧" }
}
```

### ToolContext (All 27 Fields)

The `ToolContext` is the tool's window into the agent runtime. Custom tools get full access:

```rust
pub struct ToolContext {
    // Identity & Session
    pub task_id: String,              // Unique run identifier
    pub session_id: String,           // Session for persistence
    pub session_key: String,          // Platform-scoped session key
    pub user_task: String,            // Original user prompt
    pub platform: Platform,           // CLI, Telegram, Discord, etc.

    // Working Environment
    pub cwd: PathBuf,                 // Working directory for file ops
    pub config: Arc<AppConfig>,       // Full agent configuration
    pub cancel: CancellationToken,    // Check for interruption

    // Runtime Services
    pub state_db: Option<Arc<SessionDb>>,              // Session persistence
    pub provider: Option<Arc<dyn LLMProvider>>,        // LLM access (for sub-calls)
    pub tool_registry: Option<Arc<ToolRegistry>>,      // Access other tools
    pub process_table: Arc<ProcessTable>,              // Background processes
    pub sub_agent_runner: Option<Arc<dyn SubAgentRunner>>,  // Delegation

    // Communication Channels
    pub delegate_depth: u8,                            // Nesting depth limit
    pub delegation_event_tx: Option<DelegationEventSender>,
    pub clarify_tx: Option<ClarifyTx>,                 // Ask user questions
    pub approval_tx: Option<ApprovalTx>,               // Request command approval
    pub gateway_sender: Option<Arc<dyn GatewaySender>>, // Send to platform
    pub tool_progress_tx: Option<ToolProgressTx>,      // Progress updates
    pub watch_notification_tx: Option<WatchNotifTx>,   // File watch notifications

    // Tool-Specific State
    pub origin_chat: Option<OriginChat>,               // Platform origin info
    pub on_skills_changed: Option<SkillsChangedCallback>,
    pub todo_store: Option<Arc<TodoStore>>,             // Shared todo list
    pub current_tool_call_id: String,                  // LLM's call ID
    pub current_tool_name: String,                     // This tool's name
    pub injected_messages: Option<InjectedMessagesSender>, // Inject messages
}
```

### Adding a Rust Tool (Step-by-Step)

```rust
// crates/edgecrab-tools/src/tools/my_tool.rs
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use edgecrab_types::{ToolError, ToolSchema};
use crate::registry::{ToolContext, ToolHandler};

pub struct MyTool;

#[derive(Deserialize)]
struct MyArgs {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
}
fn default_limit() -> usize { 10 }

#[async_trait]
impl ToolHandler for MyTool {
    fn name(&self) -> &'static str { "my_tool" }
    fn toolset(&self) -> &'static str { "custom" }
    fn emoji(&self) -> &'static str { "🔍" }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "my_tool".into(),
            description: "Search internal knowledge base".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results (default: 10)"
                    }
                },
                "required": ["query"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        std::env::var("MY_TOOL_API_KEY").is_ok()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: MyArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::InvalidArgs {
                tool: "my_tool".into(),
                message: e.to_string(),
            })?;

        // Check cancellation
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        // Use ToolContext services
        tracing::info!(
            session = %ctx.session_id,
            cwd = %ctx.cwd.display(),
            "Searching for: {}",
            args.query
        );

        // Your implementation here
        let results = vec![
            json!({"title": "Result 1", "score": 0.95}),
            json!({"title": "Result 2", "score": 0.87}),
        ];

        Ok(json!({
            "results": results,
            "total": results.len(),
            "query": args.query
        }).to_string())
    }
}

// Auto-register at compile time via inventory
inventory::submit!(crate::registry::RegisteredTool {
    handler: &MyTool,
});
```

---

## Layer 2: Rhai Scripts (Embedded Scripting)

Rhai is a lightweight, safe scripting language designed for embedding in Rust applications. EdgeCrab uses it for user-contributed scripts that need sandboxing.

### How It Works

```
+---------------------------------------------------------------+
|                    Rhai Script Lifecycle                       |
+---------------------------------------------------------------+
|                                                               |
|  1. Plugin discovered in ~/.edgecrab/plugins/<name>/          |
|  2. manifest.yaml declares kind: script, entrypoint: main.rhai|
|  3. ScriptRuntime::load() compiles AST with op limits         |
|  4. On tool call: engine.call_fn("tool_call", [name, args])   |
|  5. Script returns JSON string result                         |
|                                                               |
|  SANDBOXING:                                                  |
|  - max_operations: 100_000 (prevents infinite loops)          |
|  - max_call_depth: 64 (prevents stack overflow)               |
|  - NO filesystem access (only get_env, log, emit_message)     |
|  - NO network access                                          |
|  - NO process spawning                                        |
+---------------------------------------------------------------+
```

### Example Rhai Script Plugin

```
~/.edgecrab/plugins/text-stats/
  manifest.yaml
  main.rhai
```

**manifest.yaml:**
```yaml
name: text-stats
version: "0.1.0"
kind: script
description: "Text statistics tools"
entrypoint: main.rhai
tools:
  - name: text_word_count
    description: "Count words in text"
    parameters:
      type: object
      properties:
        text:
          type: string
          description: "Text to analyze"
      required: [text]
  - name: text_char_freq
    description: "Character frequency analysis"
    parameters:
      type: object
      properties:
        text:
          type: string
          description: "Text to analyze"
      required: [text]
```

**main.rhai:**
```rhai
fn tool_call(name, args_json) {
    let args = parse_json(args_json);

    if name == "text_word_count" {
        let text = args.text;
        let words = text.split(' ');
        let count = words.len();
        return `{"word_count": ${count}}`;
    }

    if name == "text_char_freq" {
        let text = args.text;
        let len = text.len();
        return `{"total_chars": ${len}}`;
    }

    `{"error": "Unknown tool: ${name}"}`
}
```

### When to Use Rhai

| Use Case | Rhai? | Why |
|----------|-------|-----|
| Simple text transformation | **Yes** | Fast, safe, no deps |
| Math/logic operations | **Yes** | Good at numeric computation |
| Needs HTTP requests | **No** | No network access |
| Needs file I/O | **No** | No filesystem access |
| Needs database access | **No** | No external libraries |
| User-contributed scripts | **Yes** | Sandboxed by design |
| Performance-critical | **No** | Use native Rust instead |

---

## Layer 3: Tool Server Plugins (External Process)

Tool Servers are long-lived subprocesses that communicate with EdgeCrab over JSON-RPC 2.0 via stdin/stdout. They can be written in **any language**.

### How It Works

```
+---------------------------------------------------------------+
|                  Tool Server Lifecycle                         |
+---------------------------------------------------------------+
|                                                               |
|  EdgeCrab Process              Tool Server Process            |
|  +---------------------+      +-------------------------+    |
|  | ToolServerClient    |      | Your code (any lang)    |    |
|  |                     |stdin | - Reads JSON-RPC        |    |
|  | rpc("tools/list")  ------>| - Returns tool schemas   |    |
|  |                     |stdout| - Handles tool calls     |    |
|  | rpc("tools/call")  ------>| - Returns results        |    |
|  |                     |<-----| - Can call host API      |    |
|  +---------------------+      +-------------------------+    |
|                                                               |
|  LIFECYCLE MANAGEMENT:                                        |
|  - Lazy start: spawned on first tool call                     |
|  - Idle timeout: killed after N seconds of inactivity         |
|  - Restart policy: on_failure / always / never                |
|  - Max restarts: configurable per plugin                      |
+---------------------------------------------------------------+
```

### Example: Python Tool Server

```
~/.edgecrab/plugins/stock-prices/
  manifest.yaml
  server.py
  requirements.txt
```

**manifest.yaml:**
```yaml
name: stock-prices
version: "0.1.0"
kind: tool-server
description: "Real-time stock price lookup"
exec:
  command: python3
  args: ["server.py"]
  cwd: "."             # Relative to plugin directory
  env:
    ALPHA_VANTAGE_KEY: "${ALPHA_VANTAGE_KEY}"
  restart_policy: on_failure
  max_restarts: 3
  idle_timeout_secs: 300
```

**server.py:**
```python
#!/usr/bin/env python3
"""EdgeCrab Tool Server — Stock price lookup via JSON-RPC 2.0 over stdio."""
import json
import sys
import os

import requests

API_KEY = os.environ.get("ALPHA_VANTAGE_KEY", "")

TOOLS = [
    {
        "name": "get_stock_price",
        "description": "Get current stock price for a ticker symbol",
        "inputSchema": {
            "type": "object",
            "properties": {
                "symbol": {"type": "string", "description": "Stock ticker (e.g., AAPL)"},
            },
            "required": ["symbol"],
        },
    },
    {
        "name": "compare_stocks",
        "description": "Compare prices of multiple stocks",
        "inputSchema": {
            "type": "object",
            "properties": {
                "symbols": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "List of ticker symbols",
                },
            },
            "required": ["symbols"],
        },
    },
]

def get_price(symbol: str) -> dict:
    url = f"https://www.alphavantage.co/query"
    params = {"function": "GLOBAL_QUOTE", "symbol": symbol, "apikey": API_KEY}
    resp = requests.get(url, params=params, timeout=10)
    data = resp.json().get("Global Quote", {})
    return {
        "symbol": symbol,
        "price": data.get("05. price", "N/A"),
        "change": data.get("09. change", "N/A"),
        "change_pct": data.get("10. change percent", "N/A"),
    }

def handle_request(request: dict) -> dict:
    method = request.get("method", "")
    req_id = request.get("id")

    if method == "tools/list":
        return {"jsonrpc": "2.0", "id": req_id, "result": {"tools": TOOLS}}

    if method == "tools/call":
        name = request["params"]["name"]
        args = request["params"]["arguments"]

        if name == "get_stock_price":
            result = get_price(args["symbol"])
        elif name == "compare_stocks":
            result = [get_price(s) for s in args["symbols"]]
        else:
            return {
                "jsonrpc": "2.0", "id": req_id,
                "error": {"code": -32601, "message": f"Unknown tool: {name}"},
            }

        return {
            "jsonrpc": "2.0", "id": req_id,
            "result": {"content": [{"type": "text", "text": json.dumps(result)}]},
        }

    if method == "shutdown":
        sys.exit(0)

    return {
        "jsonrpc": "2.0", "id": req_id,
        "error": {"code": -32601, "message": f"Unknown method: {method}"},
    }

def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
            response = handle_request(request)
            sys.stdout.write(json.dumps(response) + "\n")
            sys.stdout.flush()
        except Exception as e:
            error_resp = {
                "jsonrpc": "2.0",
                "id": None,
                "error": {"code": -32603, "message": str(e)},
            }
            sys.stdout.write(json.dumps(error_resp) + "\n")
            sys.stdout.flush()

if __name__ == "__main__":
    main()
```

### Example: Node.js Tool Server

**server.js:**
```javascript
#!/usr/bin/env node
const readline = require("readline");

const TOOLS = [
  {
    name: "analyze_sentiment",
    description: "Analyze text sentiment using local NLP",
    inputSchema: {
      type: "object",
      properties: {
        text: { type: "string", description: "Text to analyze" },
      },
      required: ["text"],
    },
  },
];

function analyzeSentiment(text) {
  // Simplified — real impl would use a proper NLP library
  const positiveWords = ["good", "great", "excellent", "happy", "love"];
  const negativeWords = ["bad", "terrible", "awful", "hate", "sad"];
  const words = text.toLowerCase().split(/\s+/);
  const pos = words.filter((w) => positiveWords.includes(w)).length;
  const neg = words.filter((w) => negativeWords.includes(w)).length;
  const total = pos + neg || 1;
  return {
    sentiment: pos > neg ? "positive" : neg > pos ? "negative" : "neutral",
    confidence: Math.max(pos, neg) / total,
    positive_count: pos,
    negative_count: neg,
  };
}

const rl = readline.createInterface({ input: process.stdin });
rl.on("line", (line) => {
  try {
    const req = JSON.parse(line);
    let response;

    if (req.method === "tools/list") {
      response = { jsonrpc: "2.0", id: req.id, result: { tools: TOOLS } };
    } else if (req.method === "tools/call") {
      const { name, arguments: args } = req.params;
      if (name === "analyze_sentiment") {
        const result = analyzeSentiment(args.text);
        response = {
          jsonrpc: "2.0", id: req.id,
          result: { content: [{ type: "text", text: JSON.stringify(result) }] },
        };
      }
    } else if (req.method === "shutdown") {
      process.exit(0);
    }

    process.stdout.write(JSON.stringify(response) + "\n");
  } catch (e) {
    const err = {
      jsonrpc: "2.0", id: null,
      error: { code: -32603, message: e.message },
    };
    process.stdout.write(JSON.stringify(err) + "\n");
  }
});
```

### Tool Server Host API

Tool Servers can call back into EdgeCrab using the host API:

| Method | Description |
|--------|-------------|
| `host/get_env` | Read environment variable (filtered) |
| `host/get_config` | Read agent configuration value |
| `host/emit_message` | Inject a message into the conversation |
| `host/log` | Write to EdgeCrab's structured log |

---

## Layer 4: MCP Servers (Model Context Protocol)

MCP is the industry-standard protocol for LLM tool integration. EdgeCrab supports both stdio and HTTP MCP servers.

### How It Works

```
+---------------------------------------------------------------+
|                    MCP Client Lifecycle                        |
+---------------------------------------------------------------+
|                                                               |
|  EdgeCrab Agent                   MCP Server                  |
|  +---------------------+         +-------------------+       |
|  | MCP Client          |         | Any MCP Server    |       |
|  |                     |         | (npx, Docker,     |       |
|  | initialize()  ----------->    |  cloud, etc.)     |       |
|  | tools/list()  ----------->    |                   |       |
|  | tools/call()  ----------->    |                   |       |
|  | prompts/list() ---------->    |                   |       |
|  | resources/read() -------->    |                   |       |
|  +---------------------+         +-------------------+       |
|                                                               |
|  TRANSPORT:                                                   |
|  - stdio: subprocess JSON-RPC (like Tool Servers)             |
|  - HTTP: POST to server URL with Bearer token auth            |
|                                                               |
|  DISCOVERY:                                                   |
|  - Tools are auto-discovered via tools/list                   |
|  - Tool names prefixed: mcp__<server>__<tool>                 |
|  - Available to agent alongside built-in tools                |
+---------------------------------------------------------------+
```

### Config (in ~/.edgecrab/config.yaml)

```yaml
mcp_servers:
  # stdio MCP server
  postgres:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-postgres"]
    env:
      DATABASE_URL: "postgresql://localhost/mydb"
    enabled: true

  # HTTP MCP server
  remote-tools:
    url: "https://mcp.example.com/rpc"
    bearer_token: "sk-..."
    enabled: true

  # Docker-based MCP server
  browser:
    command: docker
    args: ["run", "-i", "--rm", "mcp/browser"]
    enabled: true
```

---

## The SDK Custom Tool Challenge: Bridging FFI

This is the **hardest engineering problem** in the SDK. When a Python or Node.js user writes:

```python
@Tool("get_weather", description="Get weather")
async def get_weather(city: str) -> dict:
    return {"temp": 22, "city": city}

agent = Agent("claude-sonnet-4", tools=[get_weather])
```

What actually needs to happen is:

```
+-----------------------------------------------------------------------+
|                  Custom Tool FFI Bridge (Python)                      |
+-----------------------------------------------------------------------+
|                                                                       |
|  Python Side                          Rust Side                       |
|  +---------------------------+        +---------------------------+   |
|  | @Tool("get_weather")      |        | ForeignToolHandler        |   |
|  | async def get_weather():  |        | impl ToolHandler          |   |
|  |   return {"temp": 22}     |        |                           |   |
|  +---------------------------+        +---------------------------+   |
|           |                                      |                    |
|  REGISTRATION (at Agent construction):           |                    |
|  1. @Tool decorator captures:                    |                    |
|     - function reference                         |                    |
|     - name, description                          |                    |
|     - schema inferred from type hints            |                    |
|                                                  |                    |
|  2. PyO3 receives tool definitions               |                    |
|     via Agent constructor                        |                    |
|                                                  |                    |
|  3. Rust creates ForeignToolHandler              |                    |
|     wrapping a PyObject callback                 |                    |
|                                                  |                    |
|  EXECUTION (during ReAct loop):                  |                    |
|  4. LLM decides to call "get_weather"            |                    |
|  5. ToolRegistry.dispatch("get_weather", args)   |                    |
|  6. ForeignToolHandler.execute() called          |                    |
|  7. Rust acquires GIL, calls Python function:    |                    |
|     py_func.call(args) -> awaitable              |                    |
|  8. Rust awaits Python coroutine via pyo3-async  |                    |
|  9. Python function executes, returns dict       |                    |
|  10. Result serialized to JSON string            |                    |
|  11. JSON returned to ReAct loop                 |                    |
|                                                  |                    |
|  LATENCY: ~0.05-0.2ms overhead per tool call     |                    |
|  (GIL acquire + Python call + serialize)         |                    |
+-----------------------------------------------------------------------+
```

### The ForeignToolHandler Implementation (SDK-Core)

This is the key Rust struct that bridges foreign-language tools into the native registry:

```rust
// In edgecrab-sdk-core (proposed)
use edgecrab_tools::registry::{ToolHandler, ToolContext};
use edgecrab_types::{ToolSchema, ToolError};

/// A ToolHandler that delegates execution to a foreign function
/// (Python callable via PyO3, or JS function via napi-rs).
pub struct ForeignToolHandler {
    name: String,
    toolset: String,
    schema: ToolSchema,
    emoji: String,
    /// Opaque callback — PyO3 uses Py<PyAny>, napi uses JsFunction
    /// Both are wrapped in a trait object:
    executor: Arc<dyn ForeignExecutor>,
}

#[async_trait]
pub trait ForeignExecutor: Send + Sync {
    async fn call(
        &self,
        args: serde_json::Value,
        ctx_snapshot: ToolContextSnapshot,
    ) -> Result<String, ToolError>;
}

/// Subset of ToolContext that's safe to send across FFI
/// (no Arc<dyn Provider>, no channels — just data)
pub struct ToolContextSnapshot {
    pub task_id: String,
    pub session_id: String,
    pub cwd: String,
    pub platform: String,
    pub cancel_flag: Arc<AtomicBool>,
}

#[async_trait]
impl ToolHandler for ForeignToolHandler {
    fn name(&self) -> &'static str {
        // Leak is acceptable here — tools are registered once
        Box::leak(self.name.clone().into_boxed_str())
    }
    fn toolset(&self) -> &'static str {
        Box::leak(self.toolset.clone().into_boxed_str())
    }
    fn schema(&self) -> ToolSchema { self.schema.clone() }
    fn emoji(&self) -> &'static str {
        Box::leak(self.emoji.clone().into_boxed_str())
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let snapshot = ToolContextSnapshot {
            task_id: ctx.task_id.clone(),
            session_id: ctx.session_id.clone(),
            cwd: ctx.cwd.display().to_string(),
            platform: ctx.platform.to_string(),
            cancel_flag: Arc::new(AtomicBool::new(ctx.cancel.is_cancelled())),
        };
        self.executor.call(args, snapshot).await
    }
}
```

### PyO3 Implementation

```rust
// In sdks/python/src/tools.rs (proposed)
use pyo3::prelude::*;
use pyo3::types::PyDict;

pub struct PyExecutor {
    py_func: Py<PyAny>,
    is_async: bool,
}

#[async_trait]
impl ForeignExecutor for PyExecutor {
    async fn call(
        &self,
        args: serde_json::Value,
        ctx: ToolContextSnapshot,
    ) -> Result<String, ToolError> {
        Python::with_gil(|py| {
            // Convert args to Python dict
            let py_args = pythonize::pythonize(py, &args)
                .map_err(|e| ToolError::InvalidArgs {
                    tool: "foreign".into(),
                    message: e.to_string(),
                })?;

            let result = if self.is_async {
                // Call async function — returns a coroutine
                let coro = self.py_func.call1(py, (py_args,))?;
                // Drive the coroutine via pyo3-asyncio
                pyo3_asyncio::tokio::into_future(coro.bind(py))?
            } else {
                // Call sync function directly
                let result = self.py_func.call1(py, (py_args,))?;
                // Wrap in ready future
                Box::pin(std::future::ready(Ok(result)))
            };

            // Serialize result to JSON
            let py_result = result.await?;
            let json_str = py_result
                .call_method0(py, "__str__")?
                .extract::<String>(py)?;
            Ok(json_str)
        })
    }
}
```

### Schema Inference from Python Type Hints

```python
# SDK Python side — schema inference engine
import inspect
import typing
from typing import get_type_hints

def infer_schema(func) -> dict:
    """Infer JSON Schema from Python function signature."""
    hints = get_type_hints(func)
    sig = inspect.signature(func)
    properties = {}
    required = []

    for name, param in sig.parameters.items():
        if name == "ctx":  # Skip ToolContext injection
            continue
        if name in ("self", "cls"):
            continue

        hint = hints.get(name, str)
        json_type = _python_type_to_json(hint)
        prop = {"type": json_type}

        # Extract description from docstring
        doc_desc = _extract_param_doc(func, name)
        if doc_desc:
            prop["description"] = doc_desc

        properties[name] = prop

        if param.default is inspect.Parameter.empty:
            required.append(name)
        elif param.default is not None:
            prop["default"] = param.default

    return {
        "type": "object",
        "properties": properties,
        "required": required,
    }

TYPE_MAP = {
    str: "string",
    int: "integer",
    float: "number",
    bool: "boolean",
}

def _python_type_to_json(hint) -> str:
    # Simple types
    if hint in TYPE_MAP:
        return TYPE_MAP[hint]

    # Optional[T] → same type, not required
    origin = getattr(hint, "__origin__", None)
    if origin is typing.Union:
        args = hint.__args__
        non_none = [a for a in args if a is not type(None)]
        if len(non_none) == 1:
            return _python_type_to_json(non_none[0])

    # list[T]
    if origin is list:
        return "array"

    # dict[str, Any]
    if origin is dict:
        return "object"

    # Fallback
    return "string"
```

### What Works and What Doesn't (Honest Assessment)

| Type Hint | Inferred Schema | Works? |
|-----------|----------------|--------|
| `str` | `{"type": "string"}` | Yes |
| `int` | `{"type": "integer"}` | Yes |
| `float` | `{"type": "number"}` | Yes |
| `bool` | `{"type": "boolean"}` | Yes |
| `list[str]` | `{"type": "array", "items": {"type": "string"}}` | Yes |
| `Optional[str]` | `{"type": "string"}`, not required | Yes |
| `str = "default"` | `{"type": "string", "default": "default"}` | Yes |
| `list[dict[str, Any]]` | `{"type": "array"}` (no item schema) | Partial |
| `Union[str, int]` | **Error** — ambiguous | No |
| `MyPydanticModel` | **Error** — needs explicit schema | No |
| `TypedDict` | **Could work** but not implemented | No |

**Recommendation:** For complex types, require explicit schema:
```python
@Tool("complex_tool", schema={
    "items": {"type": "array", "items": {"type": "object", "properties": {...}}}
})
async def complex_tool(items: list) -> dict: ...
```

---

## How Each SDK Language Maps to the Extensibility Layers

```
+------------------------------------------------------------------------+
|            SDK Language → Extensibility Layer Mapping                   |
+------------------------------------------------------------------------+
|                                                                        |
|  Language    | Layer 1 (Native) | Layer 2 (Rhai) | Layer 3 (Tool Srv) |
|  -----------+------------------+----------------+---------------------|
|  Rust SDK   | Direct impl      | Via plugins/   | Via plugins/        |
|             | ToolHandler      |                |                     |
|  -----------+------------------+----------------+---------------------|
|  Python SDK | Via PyO3 +       | Via plugins/   | Via plugins/ OR     |
|             | ForeignTool-     |                | write server.py     |
|             | Handler bridge   |                | directly            |
|  -----------+------------------+----------------+---------------------|
|  Node.js SDK| Via napi-rs +    | Via plugins/   | Via plugins/ OR     |
|             | ForeignTool-     |                | write server.js     |
|             | Handler bridge   |                | directly            |
|  -----------+------------------+----------------+---------------------|
|                                                                        |
|  ALSO: All SDKs support MCP servers via config (Layer 4)               |
+------------------------------------------------------------------------+
```

### Decision Tree: Which Layer Should I Use?

```
Need a custom tool?
  |
  +-- Is it simple (no I/O, no deps)? ──> Rhai Script (Layer 2)
  |
  +-- Is it in Python/Node.js?
  |     |
  |     +-- Using the SDK? ──> @Tool decorator (Layer 1 via FFI)
  |     |
  |     +-- Standalone process? ──> Tool Server (Layer 3)
  |
  +-- Is it in Rust?
  |     |
  |     +-- Part of your crate? ──> ToolHandler trait (Layer 1)
  |     |
  |     +-- Separate binary? ──> Tool Server (Layer 3)
  |
  +-- Is it an existing MCP server? ──> MCP (Layer 4)
  |
  +-- Need to share across teams? ──> MCP (Layer 4) or Tool Server (Layer 3)
  |
  +-- Need maximum isolation? ──> Tool Server (Layer 3) or MCP (Layer 4)
```

---

## Security Considerations for Custom Tools

### What EdgeCrab Protects Automatically

| Protection | Native Tools | FFI Tools | Tool Servers | MCP |
|------------|-------------|-----------|-------------|-----|
| Path traversal (path_jail) | Yes | Yes (via ctx.cwd) | No* | No* |
| SSRF guard | Yes | Yes (via runtime) | No* | No* |
| Command injection scan | Yes | Yes (via runtime) | No* | No* |
| Secret redaction | Yes | Yes | Yes | Yes |
| Tool timeout | Yes | Yes | Yes (idle_timeout) | Yes |
| Approval workflow | Yes | Yes | Yes | Yes |

\* Tool Servers and MCP Servers run in separate processes. EdgeCrab protects the *boundary* (approval, timeout, result redaction) but cannot protect what happens *inside* the external process.

### Guidelines for Secure Custom Tools

1. **Never return raw secrets.** The redaction pipeline catches patterns like `sk-`, `API_KEY=`, but don't rely on it.
2. **Validate inputs yourself.** The LLM can send arbitrary strings as arguments.
3. **Use ctx.cwd for file paths.** Don't construct paths from user input without validation.
4. **Set timeouts.** Default is 120s. For network tools, use 30s: `@Tool("api_call", timeout=30)`
5. **Check cancellation.** Long-running tools should check `ctx.cancel.is_cancelled()` periodically.

---

## WASM SDK: Running EdgeCrab in Browser & Edge Compute

### The Vision

EdgeCrab's core is Rust. Rust compiles to WebAssembly. This means the agent loop, custom tool dispatch, streaming, and provider routing can run **natively in the browser** or on **edge compute platforms** (Cloudflare Workers, Deno Deploy, Vercel Edge Functions) — with no server needed for the agent logic itself.

```
+------------------------------------------------------------------------+
|                    WASM SDK Architecture                               |
+------------------------------------------------------------------------+
|                                                                        |
|  Browser / Edge Runtime                                               |
|  +------------------------------------------------------------------+ |
|  | JavaScript / TypeScript application                              | |
|  |                                                                  | |
|  |  import { Agent, Tool } from "@edgecrab/wasm";                   | |
|  |                                                                  | |
|  |  const agent = new Agent("openai/gpt-4o");                       | |
|  |  const reply = await agent.chat("Hello!");                       | |
|  |                                                                  | |
|  +------------------------------------------------------------------+ |
|       |                                                                |
|       v                                                                |
|  +------------------------------------------------------------------+ |
|  | edgecrab-sdk-wasm (Rust compiled to WASM)                        | |
|  |                                                                  | |
|  |  +-------------------+  +------------------+  +--------------+   | |
|  |  | Agent Loop        |  | ForeignToolHandler|  | Streaming   |   | |
|  |  | (ReAct, compress) |  | (JS callbacks)   |  | (JS callback)|   | |
|  |  +-------------------+  +------------------+  +--------------+   | |
|  |  +-------------------+  +------------------+  +--------------+   | |
|  |  | Provider Router   |  | Model Catalog    |  | Cost Tracker |   | |
|  |  | (via fetch API)   |  | (compiled-in)    |  | (built-in)   |   | |
|  |  +-------------------+  +------------------+  +--------------+   | |
|  +------------------------------------------------------------------+ |
|       |                                                                |
|       v  (HTTP via browser fetch / edge runtime fetch)                 |
|  +------------------------------------------------------------------+ |
|  | LLM Provider APIs (OpenAI, Anthropic, Google, etc.)              | |
|  +------------------------------------------------------------------+ |
|                                                                        |
+------------------------------------------------------------------------+
```

### Compilation Targets

| Target | Platform | Async Runtime | Network | Storage |
|--------|----------|--------------|---------|---------|
| `wasm32-unknown-unknown` | Browser | `wasm-bindgen-futures` / `spawn_local` | `fetch` API | IndexedDB, localStorage |
| `wasm32-wasi` | Cloudflare Workers, Deno, Vercel Edge | WASI Preview 2 | `fetch` binding | KV stores (platform-specific) |

### What's Included in the WASM SDK (Subset)

| Capability | Included? | Notes |
|-----------|-----------|-------|
| Agent core (chat, run) | **Yes** | Full ReAct loop compiled to WASM |
| Custom tools (JS callbacks) | **Yes** | Tools registered in JS, called from WASM agent loop |
| Streaming | **Yes** | Via JS callback, Rust `async fn` → JS `Promise` |
| Model catalog | **Yes** | 15 providers, 200+ models, compiled-in YAML |
| Cost tracking | **Yes** | Zero-overhead token counting |
| Context compression | **Yes** | Structural + LLM-based compression |
| Provider routing | **Yes** | Provider selection via `fetch` API |
| Multi-agent delegation | **Yes** | Sub-agents run in same WASM instance |
| Session persistence | **Partial** | Via JS adapter (IndexedDB, KV store, etc.) |
| Memory | **Partial** | Via JS adapter (no filesystem) |

### What's NOT Included

| Capability | Why Not |
|-----------|---------|
| File I/O tools (read, write, patch, search) | No filesystem in browser WASM |
| Terminal tool | No subprocess spawning in WASM |
| Browser automation (CDP) | Cannot spawn headless Chrome from WASM |
| MCP (stdio) | No subprocess pipes in WASM |
| Web crawl (full) | CORS restrictions in browser |
| Background processes | No process management |
| TTS / Transcribe | Platform-specific, use Web Audio API directly |
| Cron scheduler | No persistent background execution |

### WASM SDK API (TypeScript / JavaScript)

```typescript
// @edgecrab/wasm — compiled from Rust via wasm-bindgen + wasm-pack

import init, { Agent, Tool, StreamEvent } from "@edgecrab/wasm";

// 1. Initialize the WASM module (required once)
await init();

// 2. Define custom tools in JavaScript
const searchTool = Tool.create({
  name: "search_docs",
  description: "Search documentation database",
  parameters: {
    query: { type: "string", description: "Search query" },
    limit: { type: "integer", description: "Max results", default: 10 },
  },
  handler: async (args: { query: string; limit: number }) => {
    // This runs in JS — full browser API access
    const response = await fetch(`/api/search?q=${encodeURIComponent(args.query)}&limit=${args.limit}`);
    const data = await response.json();
    return JSON.stringify(data);
  },
});

const saveTool = Tool.create({
  name: "save_to_db",
  description: "Save a record to IndexedDB",
  parameters: {
    key: { type: "string" },
    value: { type: "string" },
  },
  handler: async (args) => {
    // IndexedDB, localStorage, etc. — all available in browser JS
    localStorage.setItem(args.key, args.value);
    return JSON.stringify({ saved: true });
  },
});

// 3. Create agent with custom tools
const agent = new Agent("openai/gpt-4o", {
  tools: [searchTool, saveTool],
  instructions: "You are a documentation assistant.",
  maxIterations: 20,
  apiKey: "sk-...",  // Or use agent.setApiKey() after construction
});

// 4. Simple chat
const reply = await agent.chat("Find docs about authentication");
console.log(reply);

// 5. Streaming
const stream = agent.stream("Explain OAuth2 flow");
for await (const event of stream) {
  switch (event.type) {
    case "token":
      document.getElementById("output").textContent += event.text;
      break;
    case "tool_exec":
      console.log(`Calling tool: ${event.name}`);
      break;
    case "done":
      console.log(`Cost: $${event.cost.toFixed(4)}`);
      break;
  }
}

// 6. Session persistence via adapter
import { IndexedDBSessionStore } from "@edgecrab/wasm/adapters";

const agent2 = new Agent("anthropic/claude-sonnet-4", {
  sessionStore: new IndexedDBSessionStore("my-app"),
  sessionId: "user-session-123",
});

// Conversations persist across page reloads
await agent2.chat("Remember: my name is Alice");
// ... later, after page reload:
const reply2 = await agent2.chat("What's my name?"); // "Your name is Alice"
```

### How Custom Tools Bridge WASM ↔ JS

```
+-----------------------------------------------------------------------+
|              Custom Tool FFI Bridge (WASM)                            |
+-----------------------------------------------------------------------+
|                                                                       |
|  JavaScript Side                      WASM (Rust) Side                |
|  +---------------------------+        +---------------------------+   |
|  | Tool.create({             |        | WasmToolHandler           |   |
|  |   name: "search_docs",   |        | impl ToolHandler          |   |
|  |   handler: async (args)  |        |                           |   |
|  |     => { ... }           |        |                           |   |
|  | })                       |        |                           |   |
|  +---------------------------+        +---------------------------+   |
|           |                                      |                    |
|  REGISTRATION:                                   |                    |
|  1. Tool.create() captures JS function           |                    |
|     as a wasm_bindgen::JsValue                   |                    |
|  2. Schema extracted from parameters object      |                    |
|  3. Rust creates WasmToolHandler wrapping         |                    |
|     the JsValue (a JS Function)                  |                    |
|                                                  |                    |
|  EXECUTION (during ReAct loop in WASM):          |                    |
|  4. LLM response parsed in Rust (WASM)           |                    |
|  5. ToolRegistry.dispatch("search_docs", args)   |                    |
|  6. WasmToolHandler.execute() called             |                    |
|  7. Rust calls JS function via wasm-bindgen:     |                    |
|     js_func.call1(&JsValue::NULL, &js_args)      |                    |
|  8. Returns a Promise → JsFuture.await           |                    |
|  9. JS handler runs (with full browser access)   |                    |
|  10. Result flows back as JsValue → String       |                    |
|  11. String returned to WASM ReAct loop          |                    |
|                                                  |                    |
|  LATENCY: ~0.01-0.05ms overhead per tool call    |                    |
|  (WASM↔JS boundary + Promise resolution)         |                    |
+-----------------------------------------------------------------------+
```

### The WasmToolHandler (Rust Implementation)

```rust
// In edgecrab-sdk-wasm/src/tools.rs (proposed)
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use js_sys::{Function, JSON, Promise};

/// A ToolHandler that delegates execution to a JavaScript function
/// via the wasm-bindgen FFI boundary.
pub struct WasmToolHandler {
    name: String,
    schema: ToolSchema,
    /// The JS handler function — stored as a JsValue
    js_handler: Function,
}

#[async_trait(?Send)]  // Note: ?Send required for WASM (single-threaded)
impl ToolHandler for WasmToolHandler {
    fn name(&self) -> &'static str {
        Box::leak(self.name.clone().into_boxed_str())
    }

    fn schema(&self) -> ToolSchema {
        self.schema.clone()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        // Serialize args to JsValue
        let args_str = serde_json::to_string(&args)
            .map_err(|e| ToolError::InvalidArgs {
                tool: self.name.clone(),
                message: e.to_string(),
            })?;
        let js_args = JSON::parse(&args_str)
            .map_err(|_| ToolError::ExecutionFailed {
                tool: self.name.clone(),
                message: "Failed to parse args as JS object".into(),
            })?;

        // Call the JS function — it returns a Promise
        let promise = self.js_handler
            .call1(&JsValue::NULL, &js_args)
            .map_err(|e| ToolError::ExecutionFailed {
                tool: self.name.clone(),
                message: format!("JS handler threw: {:?}", e),
            })?;

        // Await the Promise
        let result = JsFuture::from(Promise::from(promise))
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: self.name.clone(),
                message: format!("JS Promise rejected: {:?}", e),
            })?;

        // Convert result to string
        let result_str = result.as_string().unwrap_or_else(|| {
            JSON::stringify(&result)
                .map(|s| s.into())
                .unwrap_or_else(|_| "null".to_string())
        });

        Ok(result_str)
    }
}
```

### Key Technical Constraints for WASM

1. **Single-threaded.** Browser WASM is single-threaded. Rust's `Send + Sync` constraints must be relaxed with `?Send` on async trait impls. No `tokio::spawn` — use `wasm_bindgen_futures::spawn_local` instead.

2. **No `std::fs`.** All file operations must go through JS adapters. The WASM SDK cannot include any of EdgeCrab's file tools natively.

3. **No `std::process`.** No subprocess spawning. Terminal tool, MCP stdio, browser automation — all excluded.

4. **Network via `fetch`.** HTTP requests go through the browser's `fetch` API (or edge runtime's `fetch` binding), not `reqwest`. LLM API calls work fine. CORS restrictions apply for cross-origin requests.

5. **Binary size.** A stripped WASM binary for the agent core (no built-in tools) is estimated at ~2-4MB gzipped. With model catalog and compression logic, possibly ~5MB.

6. **No `tokio` runtime.** WASM uses `wasm-bindgen-futures` for async scheduling. The ReAct loop must be adapted to use `spawn_local` instead of `tokio::spawn`.

### Session Persistence Adapters

Since WASM has no filesystem, session persistence requires platform-specific adapters:

```typescript
// Browser: IndexedDB adapter
import { IndexedDBSessionStore } from "@edgecrab/wasm/adapters";

// Cloudflare Workers: KV adapter
import { CloudflareKVSessionStore } from "@edgecrab/wasm/adapters/cloudflare";

// Custom: implement the SessionStore interface
class MySessionStore implements SessionStore {
  async save(sessionId: string, messages: Message[]): Promise<void> { ... }
  async load(sessionId: string): Promise<Message[] | null> { ... }
  async list(): Promise<SessionInfo[]> { ... }
  async search(query: string): Promise<SearchResult[]> { ... }
}
```

### Example: Browser Chat Agent

```html
<!DOCTYPE html>
<html>
<head><title>EdgeCrab Browser Agent</title></head>
<body>
  <div id="chat"></div>
  <input id="input" placeholder="Ask anything..." />
  <button onclick="send()">Send</button>

  <script type="module">
    import init, { Agent, Tool } from "@edgecrab/wasm";

    await init();

    const agent = new Agent("openai/gpt-4o", {
      apiKey: prompt("Enter your OpenAI API key:"),
      instructions: "You are a helpful coding assistant.",
      maxIterations: 10,
    });

    // Add a custom tool that uses browser APIs
    agent.addTool(Tool.create({
      name: "get_clipboard",
      description: "Read the user's clipboard content",
      parameters: {},
      handler: async () => {
        const text = await navigator.clipboard.readText();
        return JSON.stringify({ text });
      },
    }));

    window.send = async function() {
      const input = document.getElementById("input");
      const chat = document.getElementById("chat");
      const message = input.value;
      input.value = "";

      chat.innerHTML += `<p><b>You:</b> ${message}</p>`;

      const output = document.createElement("p");
      output.innerHTML = "<b>Agent:</b> ";
      chat.appendChild(output);

      // Stream tokens directly into the DOM
      for await (const event of agent.stream(message)) {
        if (event.type === "token") {
          output.textContent += event.text;
        }
      }
    };
  </script>
</body>
</html>
```

### Example: Cloudflare Worker Agent

```typescript
// src/index.ts — Cloudflare Worker with EdgeCrab WASM
import init, { Agent, Tool } from "@edgecrab/wasm";

export interface Env {
  OPENAI_API_KEY: string;
  AGENT_KV: KVNamespace;
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    await init();

    const agent = new Agent("openai/gpt-4o", {
      apiKey: env.OPENAI_API_KEY,
      maxIterations: 15,
    });

    // Add a tool that queries the Worker's KV store
    agent.addTool(Tool.create({
      name: "lookup_user",
      description: "Look up user data from KV store",
      parameters: {
        user_id: { type: "string", description: "User ID" },
      },
      handler: async (args: { user_id: string }) => {
        const data = await env.AGENT_KV.get(args.user_id, "json");
        return JSON.stringify(data ?? { error: "User not found" });
      },
    }));

    const body = await request.json() as { message: string };
    const reply = await agent.chat(body.message);

    return new Response(JSON.stringify({ reply }), {
      headers: { "Content-Type": "application/json" },
    });
  },
};
```

### WASM SDK Build Pipeline

```bash
# Build the WASM SDK
cd edgecrab-sdk-wasm/
wasm-pack build --target web --release          # Browser target
wasm-pack build --target nodejs --release       # Node.js (for SSR/testing)
wasm-pack build --target bundler --release      # Webpack/Vite bundler

# Output structure:
# pkg/
#   edgecrab_sdk_wasm_bg.wasm    (~3-5MB)
#   edgecrab_sdk_wasm.js         (JS glue code)
#   edgecrab_sdk_wasm.d.ts       (TypeScript definitions — auto-generated)
#   package.json

# Publish to npm
cd pkg/ && npm publish --access public
```

### WASM vs Node.js SDK: When to Use Which

| Criteria | WASM SDK | Node.js SDK (napi-rs) |
|----------|---------|----------------------|
| **Runtime** | Browser, Cloudflare Workers, Deno, Vercel Edge | Node.js, Bun |
| **Built-in tools** | None (custom only) | All 100+ tools |
| **File I/O** | No (use JS adapter) | Yes (native) |
| **Terminal** | No | Yes |
| **Performance** | Near-native (~1.2x overhead) | Native (zero overhead) |
| **Binary size** | ~3-5MB gzipped WASM | ~30MB native binary |
| **Install** | `npm install @edgecrab/wasm` | `npm install @edgecrab/node` |
| **Use case** | Browser apps, edge functions, serverless | Backend servers, CLI tools, full-power agents |

### Honest Assessment: WASM SDK

**What's strong:**
- Rust → WASM compilation is mature and well-tooled (`wasm-bindgen`, `wasm-pack`)
- `wasm-bindgen-futures` bridges async seamlessly — Rust `async fn` becomes JS `Promise`
- Auto-generated TypeScript definitions from `#[wasm_bindgen]` annotations
- Same agent loop code runs in browser, edge, and native — true "write once, run anywhere"
- Cloudflare Workers + EdgeCrab WASM = serverless AI agents with sub-millisecond cold starts

**What's challenging:**
- **No `tokio`.** The ReAct loop in `conversation.rs` uses `tokio::spawn` and `tokio::select!`. WASM requires refactoring to `spawn_local` and `wasm-bindgen-futures` scheduling.
- **`reqwest` needs WASM feature flag.** `reqwest` supports WASM via `reqwest/wasm` feature, but the SSRF guard (`edgecrab-security`) uses `std::net` which doesn't exist in WASM. Security must be re-implemented.
- **SQLite doesn't run in browser WASM.** Session persistence (`edgecrab-state`) uses rusqlite/SQLite which requires filesystem. Must be replaced with JS adapter pattern.
- **Binary size.** Including the full model catalog YAML and compression prompts inflates the WASM binary. Aggressive tree-shaking and lazy loading are needed.
- **Testing.** WASM tests run in headless browsers (`wasm-pack test --headless`), which is slower and more complex than native `cargo test`.

**What must be true for this to ship:**
1. `edgecrab-core` must compile with `#[cfg(target_arch = "wasm32")]` gates for platform-specific code
2. A `no-fs` feature flag must exist to strip filesystem-dependent tools at compile time
3. `reqwest` WASM feature must be validated for all 15 provider HTTP clients
4. Session persistence must use a trait-based adapter pattern (not hardcoded SQLite)
5. The WASM binary must be <5MB gzipped for acceptable browser load times

---

## Brutal Honest Assessment

### What's Strong
- 4 extensibility layers genuinely cover the full spectrum from "quick hack" to "enterprise integration"
- The Tool Server protocol is simple enough that a Python developer can write one in 30 minutes
- MCP support means the entire MCP ecosystem is automatically available
- The ForeignToolHandler bridge design is clean and type-safe

### What's Weak
- **The ForeignToolHandler doesn't exist yet.** This document describes what MUST be built, not what IS built.
- **Schema inference is inherently limited.** Complex Python types can't be losslessly converted to JSON Schema. We must be honest about this limitation.
- **GIL contention in PyO3.** During tool execution, Rust holds the GIL to call Python. If the Python function does CPU-heavy work, it blocks other Python coroutines.
- **Rhai is not well-known.** Users expecting Lua or Python scripting will be surprised. Documentation must explain why Rhai was chosen (safety, Rust-native, no FFI overhead).
- **Tool Servers have no type safety.** The JSON-RPC protocol means typos in tool names or wrong argument types are caught at runtime, not compile time.

### What's Missing from Previous Specs
- **No mention of the plugin manifest format.** The specs describe decorators and trait impls but never show how plugins are discovered, installed, or managed.
- **No mention of Rhai.** The specs say "WASM + Lua" which is factually wrong.
- **No mention of Tool Servers.** The second most important extensibility mechanism was completely absent from the spec.
- **ToolContext was documented with ~10 fields instead of the actual 27.** This matters because SDK users need to know what capabilities they can access.
- **The performance characteristics of each layer were never quantified.** Users can't make informed decisions without knowing the latency difference between native (~0.001ms), Rhai (~0.01ms), and Tool Server (~5ms) calls.
