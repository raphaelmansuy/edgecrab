# EdgeCrab — Development Guide

Instructions for AI coding assistants and developers working on the edgecrab codebase.

## Development Environment

```bash
cargo build             # debug build
cargo build --release   # optimised binary
cargo test              # full test suite (~650+ tests)
cargo clippy -- -D warnings   # lint (zero warnings required)
```

**Binary location:** `target/release/edgecrab`  
**User config:** `~/.edgecrab/config.yaml` (settings)  
**State directory:** `~/.edgecrab/` (memories, skills, sessions)

---

## Project Structure

```
edgecrab/
├── crates/
│   ├── edgecrab-types/      Core types: Message, Role, ToolCall, ToolError, errors
│   ├── edgecrab-core/       Agent ReAct loop, conversation, compression, routing
│   │   ├── agent.rs         AgentBuilder + Agent — hot-swap, streaming, session
│   │   ├── conversation.rs  execute_loop() — the ReAct tool-call loop
│   │   ├── compression.rs   Context compression: structural + LLM-based
│   │   ├── prompt_builder.rs System prompt assembly (12 sources)
│   │   ├── config.rs        AppConfig, ModelConfig, DEFAULT_CONFIG
│   │   ├── model_catalog.rs ModelCatalog — single source of truth for models
│   │   ├── model_catalog_default.yaml  13 providers × N models (compiled-in)
│   │   ├── model_router.rs  Provider factory, smart routing
│   │   ├── pricing.rs       Token cost calculation
│   │   └── sub_agent_runner.rs  Subagent delegation runner
│   ├── edgecrab-tools/      Tool registry + 30+ tool implementations
│   │   ├── registry.rs      Central ToolRegistry (schemas, handlers, dispatch)
│   │   ├── toolsets.rs      CORE_TOOLS, ACP_TOOLS, toolset composition
│   │   └── tools/
│   │       ├── file_read.rs     Read files — path-safe
│   │       ├── file_write.rs    Write files — path-safe
│   │       ├── file_patch.rs    Patch files (search-replace)
│   │       ├── file_search.rs   Grep/ripgrep file search
│   │       ├── terminal.rs      Shell commands + background processes
│   │       ├── process.rs       Background process management
│   │       ├── web.rs           Web search + HTML extract + recursive crawl (SSRF-guarded)
│   │       ├── browser.rs       Headless Chrome automation (CDP)
│   │       ├── memory.rs        Persistent agent memory (MEMORY.md / USER.md)
│   │       ├── skills.rs        Skill library (list/view/manage)
│   │       ├── session_search.rs SQLite FTS5 session search
│   │       ├── execute_code.rs  Sandboxed code execution
│   │       ├── delegate_task.rs Subagent delegation
│   │       ├── mcp_client.rs    MCP client — stdio + HTTP with Bearer token OAuth
│   │       ├── vision.rs        Image analysis (multimodal LLM)
│   │       ├── tts.rs           Text-to-speech
│   │       ├── transcribe.rs    Audio transcription
│   │       ├── todo.rs          Structured todo list management
│   │       ├── cron.rs          Cron job management
│   │       ├── clarify.rs       Clarifying questions to user
│   │       ├── checkpoint.rs    Conversation checkpoints
│   │       ├── advanced.rs      Advanced utilities + send_message
│   │       ├── honcho.rs       Honcho profile + context tools
│   │       ├── homeassistant.rs Home Assistant tools (4 tools)
│   │       ├── skills_hub.rs   Remote skill registry + installation
│   │       ├── skills_guard.rs Security scanner for external skills
│   │       └── skills_sync.rs  Manifest-based skill sync/seeding
│   ├── edgecrab-state/      SQLite WAL + FTS5 session store
│   ├── edgecrab-security/   Path safety, SSRF guard, command scanner
│   ├── edgecrab-cli/        ratatui TUI, subcommands, skin engine
│   │   ├── main.rs          Entry point — CLI subcommand dispatch
│   │   ├── app.rs           TUI App event loop (ratatui)
│   │   ├── commands.rs      Slash command registry (42 commands + 50 aliases)
│   │   ├── setup.rs         Interactive setup wizard
│   │   ├── doctor.rs        Health diagnostics
│   │   ├── skin_engine.rs   YAML theme engine (skin.yaml)
│   │   ├── model_discovery.rs Model catalog integration for TUI selector
│   │   └── plugins.rs       Plugin/skill management
│   ├── edgecrab-gateway/    Messaging platform gateway
│   │   ├── run.rs           Gateway runner, slash commands, message dispatch
│   │   ├── session.rs       SessionManager — conversation persistence
│   │   ├── stream_consumer.rs  Progressive message editing with streamed tokens
│   │   ├── channel_directory.rs Cached map of reachable channels/contacts
│   │   ├── pairing.rs       Code-based DM approval for new gateway users
│   │   ├── mirror.rs        Cross-platform session mirroring
│   │   └── platforms/       Adapters: telegram, discord, slack, whatsapp, signal, webhook,
│   │                        sms, matrix, mattermost, dingtalk, homeassistant, api_server, email
│   ├── edgecrab-acp/        ACP JSON-RPC 2.0 stdio adapter (VS Code integration)
│   └── edgecrab-migrate/    Import hermes-agent config/memories/skills
└── docs/                    Architecture docs, guides, feature specs
```

**Crate dependency graph:**
```
edgecrab-types   (no deps — imported by all)
     ↑
edgecrab-security  (types only)
     ↑
edgecrab-tools   (types + security)
     ↑
edgecrab-state   (types)
     ↑
edgecrab-core    (tools + state + security + types)
     ↑
edgecrab-cli, edgecrab-gateway, edgecrab-acp, edgecrab-migrate
```

---

## Agent Architecture (edgecrab-core)

### AgentBuilder + Agent (agent.rs)

```rust
AgentBuilder::new("anthropic/claude-opus-4.6")
    .provider(provider)          // Arc<dyn LLMProvider>
    .tools(registry)             // Arc<ToolRegistry>
    .state_db(db)                // Arc<SessionDb>
    .config(cfg)                 // AgentConfig
    .build()?  →  Agent

// Simple interface
agent.chat("explain this code").await?

// Streaming interface
agent.chat_streaming("explain this code", tx).await? // tokens via UnboundedSender<StreamEvent>

// Full interface — returns ConversationResult with usage/cost
agent.run_conversation(msg, system, history).await?
```

### ReAct Loop (conversation.rs `execute_loop`)

```text
while api_call_count < max_iterations && budget.try_consume() {
    if needs_compression(messages, params) {
        messages = compress_with_llm(messages, params, provider).await;
    }
    response = provider.chat(model, messages, tools).await?;
    if response.has_tool_calls() {
        for call in response.tool_calls {
            result = registry.dispatch(call.name, call.args).await;
            messages.push(tool_result(call.id, result));
        }
    } else {
        return ConversationResult { final_response, ... };
    }
}
```

Messages use the OpenAI format: `role` ∈ {system, user, assistant, tool}.  
Reasoning content is stored in `Message::reasoning`.

### Key Agent Config Defaults

| Key | Default | Notes |
|-----|---------|-------|
| `model` | `anthropic/claude-opus-4.6` | Override with `--model p/m` |
| `max_iterations` | 90 | Hard cap on ReAct loop turns |
| `streaming` | `true` | TUI gets tokens as they arrive |
| `platform` | `Platform::Cli` | Changes platform hints in system prompt |
| `save_trajectories` | `false` | Saves full turn transcripts to disk |
| `skip_context_files` | `false` | Skip SOUL.md/AGENTS.md injection |
| `skip_memory` | `false` | Skip memory file injection |

These agent flags are top-level `config.yaml` keys and also support env overrides:
- `EDGECRAB_SAVE_TRAJECTORIES`
- `EDGECRAB_SKIP_CONTEXT_FILES`
- `EDGECRAB_SKIP_MEMORY`

---

## System Prompt Assembly (prompt_builder.rs)

`PromptBuilder::build()` assembles ~12 sources in priority order:

1. **Identity** — `DEFAULT_IDENTITY` (or `override_identity` from SOUL.md/config)
2. **Platform hint** — concise formatting/behavior guidance per platform
3. **Date/time stamp** — current local time injected fresh each session
4. **Context files** — SOUL.md (walk up), AGENTS.md, .cursorrules, CLAUDE.md, .edgecrab.md, .hermes.md, .cursor/rules/*.mdc (all scanned for prompt injection)
5. **Memory guidance** — `MEMORY_GUIDANCE` constant
6. **Memory sections** — MEMORY.md + USER.md from `~/.edgecrab/memories/`
7. **Session search guidance** — `SESSION_SEARCH_GUIDANCE` constant
8. **Skills guidance** — `SKILLS_GUIDANCE` constant (encourage saving skills)
9. **Skills summary** — compact skill index from `~/.edgecrab/skills/`

**Prompt caching policy:** The system prompt is assembled once per session and cached in `SessionState.cached_system_prompt`. Do NOT rebuild or mutate it mid-conversation — this would invalidate Anthropic's prompt cache, dramatically increasing costs. The ONLY exception is manual `/compress` or automatic compression events.

**Injection scanning:** All context files (AGENTS.md, SOUL.md, .edgecrab.md, etc.) are scanned for prompt injection patterns before injection. Blocked files are replaced with a `[BLOCKED: ...]` placeholder.

---

## Tool Registry (edgecrab-tools)

### File Dependency Chain

```
edgecrab-tools/src/registry.rs   (no deps — ToolHandler trait + ToolRegistry)
       ↑
edgecrab-tools/src/tools/*.rs    (each implements ToolHandler + registered via inventory!)
       ↑
edgecrab-core/src/conversation.rs (imports ToolRegistry for dispatch)
```

### Adding a New Tool

**Step 1: Create `crates/edgecrab-tools/src/tools/my_tool.rs`:**

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
    fn name(&self) -> &'static str { "my_tool" }
    fn toolset(&self) -> &'static str { "my_toolset" }
    fn emoji(&self) -> &'static str { "🔧" }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "my_tool".into(),
            description: "Does X given Y.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "param": { "type": "string", "description": "The input" }
                },
                "required": ["param"]
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: MyArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::InvalidArgs { tool: "my_tool".into(), message: e.to_string() })?;
        // ... implementation ...
        Ok(serde_json::json!({"success": true}).to_string())
    }
}

// ─── Auto-register via inventory ─────────────────────────────────────
inventory::submit!(crate::registry::RegisteredTool {
    handler: &MyTool,
});
```

**Step 2: Add to `crates/edgecrab-tools/src/tools/mod.rs`:**
```rust
pub mod my_tool;
```

**Step 3: Add to `CORE_TOOLS` in `toolsets.rs` (if it's a core tool):**
```rust
pub const CORE_TOOLS: &[&str] = &[
    // ...existing tools...
    "my_tool",
];
```

**Step 4: Write tests in `crates/edgecrab-tools/src/tools/my_tool.rs`:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_my_tool_basic() { ... }
}
```

### Tool Security Rules

- **File I/O**: All paths must be validated through `edgecrab_security::path_safety::validate_path()` before access.
- **Web requests**: All URLs must pass `edgecrab_security::ssrf::is_safe_url()` before fetching.
- **Terminal commands**: All shell arguments must pass `edgecrab_security::command_scan::scan_command()`.
- **Memory writes**: Content must be scanned for prompt injection patterns before persisting.
- **Skills installation**: All external skills must pass `skills_guard::scan_skill()` before installation — 23 threat patterns covering exfiltration, injection, destructive, persistence, and obfuscation.

### Skills Hub & Guard (edgecrab-tools)

| Module | Purpose |
|--------|---------|
| `skills_hub.rs` | Remote skill registry, installation with quarantine→scan→install flow, tap management |
| `skills_guard.rs` | Security scanner for externally-sourced skills — 23 threat patterns, severity scoring |
| `skills_sync.rs` | Manifest-based bundled skills seeding and update (NEW/UNCHANGED/MODIFIED/DELETED) |

**GitHub skill install** (via CLI `/skills install`):  
`/skills install owner/repo/path/to/skill.md` fetches from GitHub raw API.  
Directories are downloaded file-by-file via GitHub Contents API.  
Set `GITHUB_TOKEN` env var for higher rate limits.

### MCP Client (mcp_client.rs) — stdio + HTTP

| Transport | Config | Notes |
|-----------|--------|-------|
| **stdio** (default) | `command`, `args`, `env`, `cwd` | Subprocess JSON-RPC 2.0 over stdin/stdout |
| **HTTP** | `url` field | JSON-RPC 2.0 POST; supports Bearer token auth |

**HTTP MCP server config example (`~/.edgecrab/config.yaml`):**
```yaml
mcp_servers:
  my-http-server:
    url: https://my-mcp-server.example.com/mcp
    bearer_token: "sk-..."   # static token (or store via /mcp-token)
    enabled: true
```

**Token store** (`~/.edgecrab/mcp-tokens/<server>.json`):  
Tokens are stored with `chmod 0o600`. Use `/mcp-token set <server> <token>` to add,  
`/mcp-token remove <server>` to delete, `/mcp-token list` to view.  
Token file overrides the config `bearer_token` field.

**Key public functions:**
```rust
pub fn reload_mcp_connections()             // drop all connections (called by /reload-mcp)
pub fn read_mcp_token(server_name)          // read stored Bearer token
pub fn write_mcp_token(server_name, token)  // persist Bearer token
pub fn remove_mcp_token(server_name)        // delete stored token
```

### Home Assistant Tools (edgecrab-tools)

| Tool | Description |
|------|-------------|
| `ha_get_states` | Fetch entity states from Home Assistant |
| `ha_call_service` | Call a Home Assistant service (light.turn_on, etc.) |
| `ha_trigger_automation` | Trigger an automation by entity_id |
| `ha_get_history` | Fetch entity history for a time range |

### Honcho Tools (edgecrab-tools)

| Tool | Description |
|------|-------------|
| `honcho_profile` | Get/set user profile facts via Honcho |
| `honcho_context` | Retrieve relevant context from Honcho memory |

### Send Message Tool (edgecrab-tools)

The `send_message` tool (in `advanced.rs`) uses the `GatewaySender` trait to send messages to any connected platform. `ToolContext.gateway_sender` is populated when running in gateway mode.

---

## CLI Architecture (edgecrab-cli)

- **ratatui** for the full-screen TUI (60fps capable, GPU-composited)
- **YAML skin engine** (`skin_engine.rs`) — reads `~/.edgecrab/skin.yaml` at startup; 7 semantic colors + `prompt_symbol` + `tool_prefix` are all overridable
- **Slash commands** registered as plain types in `commands.rs` — 42 commands, 50+ aliases
- `CommandResult` enum variants dispatched from command handlers to `App::event_loop()`

### Key Slash Commands

| Category | Commands |
|----------|----------|
| Navigation | `/help` `/quit` `/clear` `/doctor` `/version` |
| Model | `/model [p/m]` `/reasoning [effort]` |
| Session | `/new` `/session [list/switch/delete]` `/retry` `/undo` `/stop` `/history` `/save` `/export` `/title` `/resume` |
| Config | `/config` `/prompt` `/verbose` `/personality` `/statusbar` |
| Tools | `/tools` `/toolsets` `/reload-mcp` `/mcp-token` `/plugins` |
| Memory | `/memory` |
| Analysis | `/cost` `/usage` `/compress` `/insights` |
| Appearance | `/theme` `/paste` |
| Advanced | `/queue` `/background` `/rollback [checkpoint]` |
| Gateway | `/platforms` `/approve` `/deny` `/sethome` `/update` |
| Scheduling | `/cron` |
| Media | `/voice <on\|off\|status>` |
| Skills | `/skills [list\|view \<name\>\|install \<path-or-github\>\|remove \<name\>\|hub]` |

### MCP Command Details

| Command | Description |
|---------|-------------|
| `/reload-mcp` | Drop all active MCP connections; forces fresh reconnect on next tool call |
| `/mcp-token set <server> <token>` | Store a Bearer token for an HTTP MCP server |
| `/mcp-token remove <server>` | Delete stored token for the named server |
| `/mcp-token list` | List all servers with stored tokens |

### Skills Command Details

| Subcommand | Description |
|------------|-------------|
| `/skills` or `/skills list` | List installed skills from `~/.edgecrab/skills/` |
| `/skills view <name>` | Print skill content |
| `/skills install <local-path>` | Copy a local .md file or directory to the skills dir |
| `/skills install owner/repo/path` | Install a skill directly from a public GitHub repo |
| `/skills remove <name>` | Delete an installed skill |
| `/skills hub` | Show Skills Hub usage guidance |

### Voice Mode

`/voice on`  — Enable TTS readback; each agent response is read aloud after completion via `text_to_speech` tool  
`/voice off` — Disable TTS readback  
`/voice status` — Show current state  

Requires `TTS_PROVIDER` or `OPENAI_API_KEY` (used by the `text_to_speech` tool). No-op if TTS is unavailable.

### Rollback (Checkpoint Restore)

`/rollback` — Prompt the agent to list available checkpoints  
`/rollback <name>` — Prompt the agent to restore the named checkpoint  

This sends a natural-language message to the agent, which calls the `checkpoint` tool. Checkpoints are saved by the `checkpoint` tool during conversation.

### Adding a Slash Command

1. Add a handler variant to `CommandResult` enum in `commands.rs`
2. Add matching in `commands.rs` dispatch or `app.rs` event loop
3. If gateway-visible, add dispatch in `gateway/run.rs`

---

## Gateway Architecture (edgecrab-gateway)

Platform adapters implement `PlatformAdapter` trait. Available platforms:

| Platform | Adapter | Env Vars Required |
|----------|---------|-------------------|
| Telegram | `telegram.rs` | `TELEGRAM_BOT_TOKEN` |
| Discord | `discord.rs` | `DISCORD_BOT_TOKEN` |
| Slack | `slack.rs` | `SLACK_BOT_TOKEN`, `SLACK_APP_TOKEN` |
| WhatsApp | `whatsapp.rs` | `WHATSAPP_PHONE_NUMBER_ID`, `WHATSAPP_ACCESS_TOKEN` |
| Signal | `signal.rs` | `SIGNAL_CLI_PATH` |
| Webhook | `webhook.rs` | *(any HTTP server call)* |
| SMS | `sms.rs` | `TWILIO_ACCOUNT_SID`, `TWILIO_AUTH_TOKEN`, `TWILIO_PHONE_NUMBER` |
| Matrix | `matrix.rs` | `MATRIX_HOMESERVER`, `MATRIX_ACCESS_TOKEN` |
| Mattermost | `mattermost.rs` | `MATTERMOST_URL`, `MATTERMOST_TOKEN` |
| DingTalk | `dingtalk.rs` | `DINGTALK_APP_KEY`, `DINGTALK_APP_SECRET` |
| Home Assistant | `homeassistant.rs` | `HASS_URL`, `HASS_TOKEN` |
| API Server | `api_server.rs` | `API_SERVER_PORT` *(optional)* |
| Email | `email.rs` | `EMAIL_PROVIDER`, `EMAIL_FROM`, provider-specific SMTP/API credentials |

### Gateway Features

| Feature | Module | Description |
|---------|--------|-------------|
| Stream Consumer | `stream_consumer.rs` | Progressive message editing with streamed LLM tokens |
| Channel Directory | `channel_directory.rs` | Cached map of reachable channels/contacts per platform |
| DM Pairing | `pairing.rs` | Code-based DM approval flow for new gateway users |
| Session Mirroring | `mirror.rs` | Cross-platform message delivery records |

`DeliveryRouter` maps platform+user_id → send function for reply routing.
`HookRegistry` provides lifecycle hooks (gateway:startup, message:received, etc.).
`SessionManager` handles per-user conversation persistence and idle timeout.

### Media Delivery (MEDIA:// Protocol)

When the agent includes `MEDIA:/path/to/file` in its response, `DeliveryRouter` intercepts it before sending and uses the platform's native media upload API (photo for images, voice for audio, document for others).

---

## ACP Integration (edgecrab-acp)

EdgeCrab implements [Agent Communication Protocol](https://github.com/i-am-bee/acp) (JSON-RPC 2.0 over stdio):

```bash
edgecrab acp   # starts ACP server — VS Code Copilot agent
```

The ACP adapter translates VS Code `agent/run` requests into `agent.run_conversation()` calls and streams back `agent/run/token` notifications.

---

## Context Compression (compression.rs)

```text
compress_with_llm(messages, params, provider)
    ├── prune_tool_outputs(old_messages)      ← step 1: free, no LLM
    ├── find prior SUMMARY_PREFIX block?       ← iterative update
    │     yes → prepend prior summary as context
    ├── llm_summarize(pruned_old) → Ok(text) OR Err
    │     ↓ on Err
    │   build_summary() [structural fallback — stat-based]
    └── [Message::system_summary(SUMMARY_PREFIX + text), ...recent_messages]
```

| Parameter | Default | Notes |
|-----------|---------|-------|
| `context_window` | 128,000 | Estimated from model catalog |
| `threshold` | 0.50 | Compress when 50% of context used |
| `protect_last_n` | 20 | Always preserve last 20 messages |

**Caching note:** Compression rebuilds the message list. The system prompt is NOT regenerated on compression — only the conversation history is reshaped. This preserves Anthropic cache validity.

---

## Model Catalog (model_catalog.rs)

Single source of truth for all 13 providers. Compiled-in YAML:

```
~/.edgecrab/models.yaml   ← user overrides (merged on top)
model_catalog_default.yaml ← embedded default (13 providers, 200+ models)
```

Access via `ModelCatalog::get()` (thread-safe lazy OnceLock).

---

## State / Session (edgecrab-state)

SQLite WAL + FTS5 for fast full-text search across conversation history.

```rust
let db = SessionDb::open("~/.edgecrab/sessions.db")?;
db.save_session(&session)?;
db.list_sessions(limit)?;
db.search_sessions("query text")?;   // FTS5
db.get_messages(session_id)?;
```

---

## Security Model

| Layer | Protection | Crate |
|-------|-----------|-------|
| File I/O | Path traversal — canonicalize + check against allowed root | `edgecrab-security` |
| Web tools | SSRF — block private IPs (10.x, 192.168.x, 172.16.x, 127.x, ::1) | `edgecrab-security` |
| Terminal | Command injection scan — reject shell metacharacters in args | `edgecrab-security` |
| Context files | Prompt injection scan — regex + invisible unicode + homoglyphs | `edgecrab-core` |
| Memory writes | Injection patterns blocked before persisting | `edgecrab-tools` |
| LLM output | Redaction pipeline — strip secrets/tokens before display | `edgecrab-core` |
| State DB | WAL mode + integrity checks | `edgecrab-state` |

---

## Migration from hermes-agent (edgecrab-migrate)

```bash
edgecrab migrate --dry-run    # preview what will be imported
edgecrab migrate              # live migration
```

| Asset | Source | Destination |
|-------|--------|-------------|
| Config | `~/.hermes/config.yaml` | `~/.edgecrab/config.yaml` |
| Memories | `~/.hermes/memories/` | `~/.edgecrab/memories/` |
| Skills | `~/.hermes/skills/` | `~/.edgecrab/skills/` |
| Env vars | `~/.hermes/.env` | `~/.edgecrab/.env` |

---

## Known Pitfalls / DO NOT

- **DO NOT rebuild the system prompt mid-conversation.** Cache-breaking forces Anthropic to re-process the prompt on every turn. Only rebuild on explicit `/compress` or at session start.
- **DO NOT use `unwrap()` in tool handlers.** Return `ToolError` variants instead — the agent loop handles them gracefully and reports to the model.
- **DO NOT issue network requests without SSRF check.** Always call `is_safe_url()` before any `reqwest` call in tool code.
- **DO NOT slice Rust strings by raw byte offsets unless you have already proved the index is a char boundary.** Prefer `get(..)`, `char_indices()`, or helper functions like `safe_char_start()` for prefix scanning and truncation. Gateway/user text is frequently Unicode and byte slicing will panic in production.
- **DO NOT hardcode `~/.edgecrab/` in tests.** Use `TempDir` and set `EDGECRAB_HOME` to the temp dir path.
- **DO NOT store secrets (API keys, tokens) in the model output or logs.** The redaction pipeline catches most, but tool code should not log secret-bearing values.
- **DO NOT share ToolContext state across concurrent agent instances.** Each `Agent` has its own `ProcessTable` and `ToolContext` — do not share them.
- **Context file injection scanning is active:** High-severity threats in SOUL.md/AGENTS.md/etc. cause the file to be blocked rather than injected, logged with `tracing::warn!`. Test your AGENTS.md content if it contains adversarial-looking patterns.

---

## Testing

```bash
cargo test                          # full suite (~650+ tests)
cargo test -p edgecrab-core         # core crate only
cargo test -p edgecrab-tools        # tools crate only
cargo test -p edgecrab-tools --lib browser   # specific module
cargo test -- --include-ignored     # include E2E tests (need VS Code Copilot)
cargo clippy -- -D warnings         # lint (must be clean)
cargo doc --no-deps --open          # browse generated docs
```

**Tests must not write to `~/.edgecrab/`.** Use `tempfile::TempDir` for any test that touches the file system. Set the `EDGECRAB_HOME` env var to the temp dir path in your test fixture.

Always run the full `cargo test` suite before pushing changes.

---

## Editors / IDE Config

This project uses standard Rust toolchain (`rustup`, `cargo`). Recommended extensions:

- **rust-analyzer** — LSP server for Rust
- **CodeLLDB** — debug adapter
- **Even Better TOML** — Cargo.toml editing

No extra setup required beyond `cargo build`.
