# edgecrab-tools

> **Why this crate?** LLM reasoning is only useful when it connects to the real world.  
> `edgecrab-tools` is the action layer: a central `ToolRegistry`, a `ToolHandler` trait,  
> and 30+ ready-to-use tool implementations that let EdgeCrab read files, run commands,  
> browse the web, execute code, talk to MCP servers, and more — all with security checks  
> baked in at every boundary.

Part of [EdgeCrab](https://www.edgecrab.com) — the Rust SuperAgent.

---

## Built-in tools (30+)

| Toolset | Tools |
|---------|-------|
| `file` | `file_read`, `file_write`, `file_patch`, `file_search` |
| `terminal` | `terminal` (local/SSH/Docker/Modal backends), `process` (background) |
| `web` | `web` (search + extract + crawl), `browser` (headless Chrome CDP) |
| `memory` | `memory` (MEMORY.md / USER.md read-write) |
| `skills` | `skills` (list/view/manage), `skills_hub` (remote registry), `skills_guard` (security scan) |
| `session` | `session_search` (FTS5 across history) |
| `delegation` | `delegate_task` (sub-agent), `mixture_of_agents` (MoA) |
| `code_execution` | `execute_code` (sandboxed runner) |
| `mcp` | `mcp_client` (stdio + HTTP Bearer, JSON-RPC 2.0) |
| `vision` | `vision` (multimodal image analysis) |
| `tts` | `tts` (text-to-speech) |
| `transcribe` | `transcribe` (audio → text) |
| `misc` | `todo`, `cron`, `clarify`, `checkpoint`, `advanced`, `pdf_to_markdown`, `homeassistant`, `honcho` |

## Add to your crate

```toml
[dependencies]
edgecrab-tools = { path = "../edgecrab-tools" }
```

## Implement a new tool

```rust
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use edgecrab_types::{ToolError, ToolSchema};
use edgecrab_tools::registry::{ToolContext, ToolHandler};

pub struct EchoTool;

#[derive(Deserialize)]
struct EchoArgs { message: String }

#[async_trait]
impl ToolHandler for EchoTool {
    fn name(&self) -> &'static str { "echo" }
    fn toolset(&self) -> &'static str { "demo" }
    fn emoji(&self) -> &'static str { "📢" }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "echo".into(),
            description: "Returns the message unchanged.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "Text to echo" }
                },
                "required": ["message"]
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: serde_json::Value, _ctx: &ToolContext) -> Result<String, ToolError> {
        let a: EchoArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::InvalidArgs { tool: "echo".into(), message: e.to_string() })?;
        Ok(json!({"echo": a.message}).to_string())
    }
}

// Auto-register at startup
inventory::submit!(edgecrab_tools::registry::RegisteredTool { handler: &EchoTool });
```

Add the module to `tools/mod.rs` and (optionally) to `CORE_TOOLS` in `toolsets.rs`.

## Security rules (enforced in every built-in tool)

- File paths → `edgecrab_security::path_safety::validate_path()`
- Web URLs → `edgecrab_security::ssrf::is_safe_url()`
- Shell args → `edgecrab_security::command_scan::scan_command()`
- Skill installs → `skills_guard::scan_skill()` (23 threat patterns)

---

> Full docs, guides, and release notes → [edgecrab.com](https://www.edgecrab.com)
