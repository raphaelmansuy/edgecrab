# edgecrab-acp

> **Why this crate?** VS Code (and other IDEs) speak [Agent Communication Protocol](https://github.com/i-am-bee/acp)  
> — a JSON-RPC 2.0 stdio protocol that lets editors embed agents without shipping their own  
> LLM infrastructure. `edgecrab-acp` wraps the full EdgeCrab agent as an ACP server, so  
> you get EdgeCrab's 30+ tools, persistent memory, and multi-provider LLM support directly  
> inside your editor sidebar — no browser, no separate terminal.

Part of [EdgeCrab](https://www.edgecrab.com) — the Rust SuperAgent.

---

## Start the ACP server

```bash
edgecrab acp
```

The process communicates over `stdin`/`stdout` using JSON-RPC 2.0. Your IDE connects to  
the process directly — nothing to configure beyond pointing the IDE at the binary.

## VS Code quick setup

1. Install the EdgeCrab VS Code extension (or add a manual ACP entry):
   ```json
   // .vscode/settings.json
   {
     "acp.agents": [
       {
         "id": "edgecrab",
         "name": "EdgeCrab",
         "command": "edgecrab",
         "args": ["acp"]
       }
     ]
   }
   ```
2. Open the Agent panel → select **EdgeCrab** → start chatting.

## Protocol flow

```
IDE            edgecrab-acp              edgecrab-core
 │─ agent/run ──────────────────────────────▶│
 │              │─ Agent::run_conversation() ─▶│
 │              │◀── tool calls ──────────────│
 │◀ agent/run/token (streaming tokens) ───────│
 │◀ agent/run (final response) ───────────────│
```

## Supported ACP methods

| Method | Description |
|--------|-------------|
| `agent/run` | Start a conversation turn (streaming via `agent/run/token` notifications) |
| `agent/list` | Return agent metadata (name, description, capabilities) |

## Embed in your own binary

```toml
[dependencies]
edgecrab-acp = { path = "../edgecrab-acp" }
```

```rust
use edgecrab_acp::AcpServer;
use edgecrab_core::Agent;

let agent = Agent::default_builder()?.build()?;
AcpServer::new(agent).run_stdio().await?;
```

---

> Full docs, guides, and release notes → [edgecrab.com](https://www.edgecrab.com)
