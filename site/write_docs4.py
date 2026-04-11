#!/usr/bin/env python3
"""Write docs batch 4: developer/architecture.md, developer/contributing.md, reference pages"""
import os

BASE = "src/content/docs"
os.makedirs(f"{BASE}/developer", exist_ok=True)
os.makedirs(f"{BASE}/reference", exist_ok=True)

# ─── developer/architecture.md ────────────────────────────────────────
architecture_md = r"""---
title: Architecture
description: Workspace layout, crate dependency chain, key design decisions, and runtime data-flow for EdgeCrab. Grounded in the Cargo workspace at edgecrab/.
sidebar:
  order: 1
---

EdgeCrab is a Rust 2024 edition workspace. Every crate has a single responsibility and the dependency graph is a strict DAG — no circular dependencies, no feature flags that reverse the graph.

---

## Workspace Layout

```
edgecrab/
├── Cargo.toml              # workspace manifest
└── crates/
    ├── edgecrab-types/     # shared types (no deps on other crates)
    ├── edgecrab-security/  # SSRF, path-safety, injection scanning
    ├── edgecrab-state/     # SQLite WAL session storage
    ├── edgecrab-cron/      # cron scheduler engine
    ├── edgecrab-tools/     # tool registry, toolsets, all tool implementations
    ├── edgecrab-core/      # agent loop, config, context, compression, LLM client
    ├── edgecrab-cli/       # ratatui TUI, CLI arg parsing, commands, theming
    ├── edgecrab-gateway/   # messaging gateway (Telegram, Discord, Slack, Signal, WhatsApp)
    ├── edgecrab-acp/       # ACP (Agent Communication Protocol) adapter
    └── edgecrab-migrate/   # database migrations + one-shot migration CLI
```

---

## Crate Dependency Graph

Dependency arrows point from dependent → dependency:

```
edgecrab-cli ──────┐
edgecrab-gateway ──┤
edgecrab-acp ──────┤──→ edgecrab-core
edgecrab-migrate ──┘         │
                              ├──→ edgecrab-tools
                              │         │
                              │         ├──→ edgecrab-security
                              │         ├──→ edgecrab-state
                              │         ├──→ edgecrab-cron
                              │         └──→ edgecrab-types
                              │
                              ├──→ edgecrab-security
                              ├──→ edgecrab-state
                              └──→ edgecrab-types
```

**Key constraint:** `edgecrab-types` has no edgecrab dependencies. `edgecrab-security` depends only on `types`. This ensures the security layer can never accidentally bypass compile-time type checks.

---

## Crate Responsibilities

### `edgecrab-types`
Shared data structures: `Message`, `ToolCall`, `ToolResult`, `ModelResponse`, `Provider`, `Session`, `ContentBlock`, etc. No business logic — pure `serde`-serializable types.

### `edgecrab-security`
- URL safety checker (SSRF guard, private IP blocking, metadata endpoint blocking)
- Path safety checker (path traversal, outside-of-allowed-roots)
- Prompt injection scanner (heuristic + pattern-matching on tool results)
- Command allowlist/blocklist evaluation

### `edgecrab-state`
SQLite WAL-mode session database:
- Session CRUD (create, read, update, delete)
- Message storage and retrieval
- FTS5 full-text search index over message content
- Checkpoint metadata storage
- Schema migrations (applied by `edgecrab-migrate`)

### `edgecrab-cron`
- Cron expression parser and scheduler
- Job storage in `~/.edgecrab/cron/*.json`
- Spawns agent loop invocations for scheduled tasks
- Timezone-aware scheduling via `chrono-tz`

### `edgecrab-tools`
- **Tool registry:** `ToolRegistry` with `get_definitions()` for LLM schema, `dispatch()` for invocation
- **Toolsets:** `toolsets.rs` — alias resolution, `CORE_TOOLS`, `ACP_TOOLS`, expansion logic
- **All tool implementations:** file, terminal, web, browser, memory, skills, MCP, delegation, code_execution, session, checkpoint, cron, Honcho, TTS/STT/vision, Home Assistant
- **MCP client:** JSON-RPC 2.0 over stdio or HTTP with SSE

### `edgecrab-core`
- `AppConfig` — config loading, env override resolution, profile isolation
- `ContextBuilder` — assembles system prompt from SOUL.md, AGENTS.md, memories, Honcho context
- `AgentLoop` — ReAct iteration: call LLM → parse tool calls → dispatch → inject results → repeat
- `LlmClient` — unified OpenAI-compatible HTTP client with streaming, prompt caching, retry logic
- `CompressionEngine` — context window compression (summarization when threshold reached)
- Provider setup (`setup.rs`): detect API keys, build `ModelConfig` for 13 providers

### `edgecrab-cli`
- `CliArgs` (clap) — all CLI subcommands and flags
- `App` — ratatui TUI main loop
- `CommandHandler` — 42 slash commands
- `Theme` + `SkinConfig` — TUI colors, symbols, kaomoji, personality presets
- Background session manager
- Worktree creation and cleanup

### `edgecrab-gateway`
- HTTP API server (Axum) with session routing
- Platform adapters: Telegram (teloxide), Discord (twilight), Slack (Socket Mode), Signal (signal-cli HTTP), WhatsApp (bridge HTTP)
- Approval workflow (inline buttons on Telegram/Discord)
- Proactive home-channel messaging

### `edgecrab-acp`
- ACP (Agent Communication Protocol) JSON-RPC 2.0 adapter over stdio
- Exposes the EdgeCrab agent to editor extensions (VS Code, Neovim, JetBrains)
- ACP_TOOLS subset (no `clarify`, `send_message`, `generate_image`, `text_to_speech`)

### `edgecrab-migrate`
- SQLite schema migration engine
- `edgecrab migrate` CLI subcommand
- Idempotent — safe to run on every startup

---

## Agent Loop Data Flow

```
User input
    │
    ▼
ContextBuilder.build()
  ├── SOUL.md
  ├── AGENTS.md (global + project)
  ├── Memories (files)
  └── Honcho context
    │
    ▼
AgentLoop.run()
  ┌─────────────────────────────────────────┐
  │  1. LlmClient.complete(messages)         │
  │         streaming → TUI token feed       │
  │                                          │
  │  2. Parse tool_calls from response       │
  │                                          │
  │  3. SecurityChecker.check(each call)     │
  │         ├── URL safety                   │
  │         ├── Path safety                  │
  │         └── Command allowlist            │
  │                                          │
  │  4. Build checkpoint (if destructive)    │
  │                                          │
  │  5. ToolRegistry.dispatch(tool_call)     │
  │         (parallel if allowed)            │
  │                                          │
  │  6. InjectionScanner.scan(result)        │
  │                                          │
  │  7. Append ToolResult to messages        │
  │                                          │
  │  8. Check max_iterations; loop → step 1  │
  └─────────────────────────────────────────┘
    │
    ▼
Auto-flush memory (if enabled)
    │
    ▼
honcho_conclude (if enabled)
    │
    ▼
Session saved to SQLite
```

---

## Key Design Decisions

**1. Single binary.** Everything compiles into one statically-linked executable. No runtime dependencies except the OS and optional Chrome for browser tools.

**2. Security at the type level.** `SanitizedPath` and `SafeUrl` are distinct Rust types in `edgecrab-types`. Functions that require filesystem access accept `SanitizedPath`, not `String` — making unsafe paths a compile error.

**3. SQLite over file-based state.** WAL mode provides concurrent reads with atomic writes. FTS5 enables instant full-text search across all session history without an external search service.

**4. Toolset as policy, not registry.** The tool registry owns what tools exist. Toolsets own which tools are active per session. This separation means changing policy never requires touching tool implementations.

**5. No Python.** The original Hermes Agent is Python. EdgeCrab is a ground-up Rust rewrite. The result: 38 ms cold start vs 1.2 s, 14 MB vs 87 MB resident memory, no GC pauses.
"""

with open(f"{BASE}/developer/architecture.md", "w") as f:
    f.write(architecture_md)
print("developer/architecture.md written")

# ─── developer/contributing.md ────────────────────────────────────────
contributing_md = r"""---
title: Contributing
description: How to build EdgeCrab from source, run tests, add a tool, and submit a pull request.
sidebar:
  order: 2
---

## Prerequisites

- Rust 1.86.0+ (`rustup update stable`)
- `cargo` (ships with Rust)
- Optional: Chrome/Chromium for browser tool tests
- Optional: `nextest` for parallel test execution: `cargo install cargo-nextest`

---

## Building

```bash
# Clone
git clone https://github.com/NousResearch/edgecrab
cd edgecrab

# Debug build (fast compile)
cargo build

# Release build (optimized)
cargo build --release

# The binary is at:
./target/release/edgecrab
```

---

## Running Tests

```bash
# All tests (unit + integration)
cargo test --workspace

# Parallel (faster)
cargo nextest run --workspace

# Single crate
cargo test -p edgecrab-tools

# With output
cargo test --workspace -- --nocapture

# E2E tests (requires --release)
cargo test --workspace --release --test '*'
```

---

## Code Structure

Before working on a feature, read the crate that owns it:

| Feature | Crate |
|---------|-------|
| New tool | `edgecrab-tools/src/tools/` |
| Config option | `edgecrab-core/src/config.rs` |
| Slash command | `edgecrab-cli/src/commands.rs` |
| CLI flag | `edgecrab-cli/src/cli_args.rs` |
| Platform adapter | `edgecrab-gateway/src/` |
| Security rule | `edgecrab-security/src/` |
| Database schema | `edgecrab-state/src/` + `edgecrab-migrate/` |

---

## Adding a Tool

1. Create `crates/edgecrab-tools/src/tools/your_tool.rs`
2. Implement the `Tool` trait:
   ```rust
   pub struct YourTool;

   impl Tool for YourTool {
       fn name(&self) -> &'static str { "your_tool" }
       fn description(&self) -> &'static str { "..." }
       fn parameters(&self) -> serde_json::Value { /* JSON schema */ }
       async fn call(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult { ... }
   }
   ```
3. Register in `crates/edgecrab-tools/src/registry.rs`:
   ```rust
   registry.register(Arc::new(YourTool));
   ```
4. Add to the appropriate toolset in `toolsets.rs`
5. Add tests in `crates/edgecrab-tools/tests/`

---

## Adding a Config Option

1. Add the field to the appropriate struct in `edgecrab-core/src/config.rs`
2. Set a sensible `Default` value
3. Add env var override in `apply_env_overrides()` if needed
4. Document in [Configuration Reference](/reference/configuration/)

---

## Code Style

- `cargo fmt --all` before committing
- `cargo clippy --workspace -- -D warnings` must pass
- No `unwrap()` in tool implementations — use `?` and `ToolError`
- All public APIs must have doc comments (`///`)
- Integration tests in `tests/` subdirectory, unit tests in `#[cfg(test)]` modules

---

## Pull Request Process

1. Fork the repository
2. Create a feature branch: `git checkout -b feat/my-feature`
3. Make changes with tests
4. Run the full test suite: `cargo test --workspace`
5. Run clippy: `cargo clippy --workspace -- -D warnings`
6. Format: `cargo fmt --all`
7. Open a PR against `main` with a clear description

---

## Crate Versioning

All crates in the workspace share the same version number (set in the workspace `Cargo.toml`). Version bumps are done via a single PR that updates the workspace version.

---

## License

Apache-2.0. By contributing, you agree to license your contribution under the Apache 2.0 license.
"""

with open(f"{BASE}/developer/contributing.md", "w") as f:
    f.write(contributing_md)
print("developer/contributing.md written")

# ─── reference/environment-variables.md ──────────────────────────────
env_vars_md = r"""---
title: Environment Variables
description: Complete reference for all EDGECRAB_* environment variables and platform-specific env vars. Grounded in crates/edgecrab-core/src/config.rs apply_env_overrides().
sidebar:
  order: 4
---

All `EDGECRAB_*` variables are applied via `apply_env_overrides()` in `config.rs`. They override the corresponding `config.yaml` values at runtime.

---

## Core Agent Variables

| Variable | Type | Default | Description |
|----------|------|---------|-------------|
| `EDGECRAB_HOME` | path | `~/.edgecrab` | Override the EdgeCrab home directory |
| `EDGECRAB_MODEL` | string | `anthropic/claude-sonnet-4-20250514` | Default LLM model |
| `EDGECRAB_MAX_ITERATIONS` | integer | `90` | Max tool call iterations per session |
| `EDGECRAB_TIMEZONE` | string | (system) | IANA timezone (e.g. `America/New_York`) |
| `EDGECRAB_REASONING_EFFORT` | string | `""` | Reasoning budget: `low`, `medium`, `high`, `xhigh` |
| `EDGECRAB_SAVE_TRAJECTORIES` | bool | `false` | Log full trajectory to file |
| `EDGECRAB_SKIP_CONTEXT_FILES` | bool | `false` | Skip SOUL.md and AGENTS.md loading |
| `EDGECRAB_SKIP_MEMORY` | bool | `false` | Disable memory for this session |
| `EDGECRAB_MANAGED` | bool | `false` | Block all config writes (`1` to enable) |

---

## Gateway Variables

| Variable | Type | Description |
|----------|------|-------------|
| `EDGECRAB_GATEWAY_HOST` | string | Gateway bind host (default: `127.0.0.1`) |
| `EDGECRAB_GATEWAY_PORT` | integer | Gateway bind port (default: `8080`) |
| `EDGECRAB_GATEWAY_WEBHOOK` | bool | Enable/disable webhook endpoint |

---

## Telegram Variables

| Variable | Type | Description |
|----------|------|-------------|
| `TELEGRAM_BOT_TOKEN` | string | **Required.** Auto-enables Telegram platform |
| `TELEGRAM_ALLOWED_USERS` | csv | Comma-separated allowed Telegram usernames |
| `TELEGRAM_HOME_CHANNEL` | string | Chat ID for proactive messages |

---

## Discord Variables

| Variable | Type | Description |
|----------|------|-------------|
| `DISCORD_BOT_TOKEN` | string | **Required.** Auto-enables Discord platform |
| `DISCORD_ALLOWED_USERS` | csv | Comma-separated Discord user IDs |
| `DISCORD_HOME_CHANNEL` | string | Channel ID for proactive messages |

---

## Slack Variables

| Variable | Type | Description |
|----------|------|-------------|
| `SLACK_BOT_TOKEN` | string | **Required** (with `SLACK_APP_TOKEN`). Auto-enables Slack |
| `SLACK_APP_TOKEN` | string | **Required** (with `SLACK_BOT_TOKEN`). Socket Mode app token |
| `SLACK_ALLOWED_USERS` | csv | Comma-separated Slack user IDs |

---

## Signal Variables

| Variable | Type | Description |
|----------|------|-------------|
| `SIGNAL_HTTP_URL` | string | **Required** (with `SIGNAL_ACCOUNT`). signal-cli HTTP daemon URL |
| `SIGNAL_ACCOUNT` | string | **Required** (with `SIGNAL_HTTP_URL`). Registered phone number |

---

## WhatsApp Variables

| Variable | Type | Description |
|----------|------|-------------|
| `WHATSAPP_ENABLED` | bool | Enable WhatsApp (`1`, `true`, or `yes`) |
| `WHATSAPP_MODE` | string | Bridge mode: `self-chat` or `any-sender` |
| `WHATSAPP_ALLOWED_USERS` | csv | Comma-separated phone numbers |
| `WHATSAPP_BRIDGE_PORT` | integer | Bridge port (default: `3000`) |
| `WHATSAPP_BRIDGE_URL` | string | Override bridge URL |
| `WHATSAPP_SESSION_PATH` | path | Bridge session storage path |
| `WHATSAPP_REPLY_PREFIX` | string | Text prepended to all replies |

---

## TTS / STT Variables

| Variable | Type | Description |
|----------|------|-------------|
| `EDGECRAB_TTS_PROVIDER` | string | TTS provider: `edge-tts`, `openai`, `elevenlabs` |
| `EDGECRAB_TTS_VOICE` | string | TTS voice name |
| `ELEVENLABS_API_KEY` | string | ElevenLabs API key |

---

## Provider API Keys

These are not `EDGECRAB_*` variables — they are standard API key env vars detected by `setup.rs`:

| Variable | Provider |
|----------|----------|
| `OPENROUTER_API_KEY` | OpenRouter (default endpoint) |
| `ANTHROPIC_API_KEY` | Anthropic |
| `OPENAI_API_KEY` | OpenAI |
| `GOOGLE_API_KEY` | Google Gemini |
| `XAI_API_KEY` | xAI Grok |
| `DEEPSEEK_API_KEY` | DeepSeek |
| `HUGGING_FACE_HUB_TOKEN` | HuggingFace |
| `ZAI_API_KEY` | ZAI |
| `GITHUB_TOKEN` | GitHub Copilot |

Ollama and LM Studio require no API key.

---

## Other Service Variables

| Variable | Service | Description |
|----------|---------|-------------|
| `HONCHO_API_KEY` | Honcho | Enables cloud sync (auto-sets `honcho.cloud_sync: true`) |
| `HA_URL` | Home Assistant | Base URL (enables Home Assistant tools) |
| `HA_TOKEN` | Home Assistant | Long-lived access token |
| `CDP_URL` | Browser | Chrome DevTools Protocol endpoint (instead of local Chrome) |

---

## Boolean Parsing

Variables marked as `bool` accept: `1`, `true`, `yes`, `on` (case-insensitive) to enable. Any other value (including empty) is treated as disabled.

## CSV Parsing

Variables marked as `csv` accept comma-separated values with optional whitespace: `"user1, user2, user3"`.
"""

with open(f"{BASE}/reference/environment-variables.md", "w") as f:
    f.write(env_vars_md)
print("reference/environment-variables.md written")

# ─── reference/slash-commands.md ──────────────────────────────────────
slash_commands_md = r"""---
title: Slash Commands
description: All 42 EdgeCrab TUI slash commands with descriptions and arguments. Grounded in crates/edgecrab-cli/src/commands.rs CommandResult enum.
sidebar:
  order: 3
---

Type any slash command at the `❯` prompt. All commands start with `/`.

---

## Navigation & Display

| Command | Description |
|---------|-------------|
| `/help` | Show the help overlay with all available commands |
| `/quit` | Exit EdgeCrab (saves session first) |
| `/clear` | Clear the screen and session display (does not reset session) |
| `/new` | Start a new session (clears current context) |
| `/status` | Show session status: model, provider, iteration count, cost |
| `/version` | Print EdgeCrab version and build info |

---

## Session Management

| Command | Description |
|---------|-------------|
| `/session [id]` | List recent sessions or load a specific session by ID |
| `/retry` | Retry the last user message (re-sends to the model) |
| `/undo` | Undo the last message exchange (removes from context) |
| `/stop` | Stop the currently running agent task |
| `/history` | Show session message history in a scrollable view |
| `/save [file]` | Save the current session context to a file |
| `/export [format]` | Export session as Markdown, JSON, or plain text |
| `/title [text]` | Set a human-readable title for the current session |
| `/resume [id]` | Resume a previous session by its ID |

---

## Model & Intelligence

| Command | Description |
|---------|-------------|
| `/model [name]` | Switch model (e.g. `/model anthropic/claude-opus-4`) |
| `/provider [name]` | Switch provider |
| `/reasoning [level]` | Set reasoning effort: `low`, `medium`, `high`, `xhigh`, `off`, `show` |
| `/stream [on\|off]` | Toggle streaming output |

---

## Configuration

| Command | Description |
|---------|-------------|
| `/config [key] [value]` | Get or set a config value |
| `/prompt [text]` | Prepend text to the next message |
| `/verbose` | Toggle verbose mode (show raw tool call JSON) |
| `/personality [preset]` | Switch personality preset |
| `/statusbar [on\|off]` | Toggle the status bar |

---

## Tools & Plugins

| Command | Description |
|---------|-------------|
| `/tools` | List all active tools |
| `/toolsets [name]` | List available toolsets or activate a toolset |
| `/reload-mcp` | Reconnect all MCP servers (hot reload) |
| `/plugins` | List installed plugins |

---

## Memory

| Command | Description |
|---------|-------------|
| `/memory` | Show all memory files with sizes and last-modified times |

---

## Analysis & Cost

| Command | Description |
|---------|-------------|
| `/cost` | Show token usage and cost for the current session |
| `/usage` | Show per-tool usage counts for the current session |
| `/compress` | Manually trigger context compression |
| `/insights` | Show Honcho-derived insights about the current session |

---

## Advanced Workflow

| Command | Description |
|---------|-------------|
| `/queue [message]` | Queue a message to send after the current task completes |
| `/background` | Move current session to a background process |
| `/rollback` | Open the interactive checkpoint browser |

---

## Appearance

| Command | Description |
|---------|-------------|
| `/theme [name]` | Switch to a named skin (from `~/.edgecrab/skins/`) |
| `/paste` | Enter multi-line paste mode |

---

## Gateway & Automation

| Command | Description |
|---------|-------------|
| `/platforms` | Show status of all configured messaging platforms |
| `/approve` | Approve a pending security confirmation |
| `/deny` | Deny a pending security confirmation |
| `/sethome` | Set the current messaging channel as the home channel |
| `/update` | Check for and install EdgeCrab updates |
| `/cron` | Open the cron job manager UI |
| `/voice [on\|off\|tts]` | Toggle voice input/output |
| `/doctor` | Run diagnostics: check all providers, tools, and platform connections |

---

## Personality Presets

Use `/personality <preset>` to switch tone:

| Preset | Description |
|--------|-------------|
| `helpful` | Default — clear and professional |
| `concise` | Ultra-terse responses |
| `technical` | Deep technical detail, no hand-holding |
| `kawaii` | K-pop inspired, enthusiastic |
| `pirate` | Talks like a pirate (arr!) |
| `philosopher` | Every answer is a philosophical meditation |
| `hype` | Maximum hype energy |
| `shakespeare` | Shakespearean English |
| `noir` | Hard-boiled detective noir |
| `catgirl` | Anime catgirl personality |
| `creative` | Creative writing focus |
| `teacher` | Patient, step-by-step explanations |
| `surfer` | Chill surfer vibes |
| `uwu` | Internet uwu speech |
"""

with open(f"{BASE}/reference/slash-commands.md", "w") as f:
    f.write(slash_commands_md)
print("reference/slash-commands.md written")

# ─── reference/configuration.md ───────────────────────────────────────
reference_config_md = r"""---
title: Configuration Reference
description: Complete config.yaml reference for EdgeCrab — all sections, fields, types, and defaults. Grounded in crates/edgecrab-core/src/config.rs AppConfig struct.
sidebar:
  order: 2
---

This is the complete configuration reference. All fields are optional — unset fields use their compiled defaults. The file lives at `~/.edgecrab/config.yaml`.

:::tip
Run `edgecrab config show` to print your active resolved configuration (after env override merging).
:::

---

## Full Annotated config.yaml

```yaml
# ~/.edgecrab/config.yaml

# ── Model ──────────────────────────────────────────────────────────────
model:
  default: "anthropic/claude-sonnet-4-20250514"  # Default LLM model
  max_iterations: 90             # Max tool calls per session
  streaming: true                # Stream tokens to TUI
  prompt_caching: true           # Prompt caching (Anthropic / OpenAI)
  cache_ttl: 300                 # Cache TTL in seconds
  max_tokens: ~                  # null = model default
  temperature: ~                 # null = model default
  api_key_env: "OPENROUTER_API_KEY"  # API key env name
  base_url: ~                    # Custom OpenAI-compatible URL

  fallback:
    model: "copilot/gpt-4.1-mini"  # null = no fallback
    provider: "copilot"

  smart_routing:
    enabled: false               # Route simple messages to cheap_model
    cheap_model: ""

# ── Tools ──────────────────────────────────────────────────────────────
tools:
  enabled_toolsets: ~            # null = all; list = whitelist
  disabled_toolsets: ~           # toolsets to always remove
  custom_groups: {}              # custom alias → tool list
  tool_delay: 1.0                # seconds between tool calls
  parallel_execution: true       # allow concurrent tool calls
  max_parallel_workers: 8        # concurrency limit

# ── Memory ─────────────────────────────────────────────────────────────
memory:
  enabled: true
  auto_flush: true               # auto-save memory at session end

# ── Skills ─────────────────────────────────────────────────────────────
skills:
  enabled: true
  hub_url: ~                     # null = default hub
  disabled: []                   # globally disabled skill names
  platform_disabled: {}          # platform: [skill-name, ...]
  external_dirs: []              # extra skill directories
  preloaded: []                  # skills loaded every session

# ── Security ───────────────────────────────────────────────────────────
security:
  approval_required: []          # command patterns requiring approval
  blocked_commands: []           # patterns always blocked
  path_restrictions: []          # disallowed path prefixes
  injection_scanning: true       # scan tool results for prompt injection
  url_safety: true               # block private IPs and SSRF targets
  managed_mode: false            # block config writes

# ── Terminal ───────────────────────────────────────────────────────────
terminal:
  shell: ~                       # null = login shell
  timeout: 120                   # seconds per command
  env_passthrough: []            # env var names to forward

# ── Gateway ────────────────────────────────────────────────────────────
gateway:
  host: "127.0.0.1"
  port: 8080
  webhook_enabled: true
  session_timeout_minutes: 30
  enabled_platforms: []

  telegram:
    enabled: false
    token_env: "TELEGRAM_BOT_TOKEN"
    allowed_users: []
    home_channel: ~

  discord:
    enabled: false
    token_env: "DISCORD_BOT_TOKEN"
    allowed_users: []
    home_channel: ~

  slack:
    enabled: false
    bot_token_env: "SLACK_BOT_TOKEN"
    app_token_env: "SLACK_APP_TOKEN"
    allowed_users: []
    home_channel: ~

  signal:
    enabled: false
    http_url: ~
    account: ~
    allowed_users: []

  whatsapp:
    enabled: false
    bridge_port: 3000
    bridge_url: ~
    mode: "self-chat"
    allowed_users: []
    install_dependencies: true

# ── MCP Servers ────────────────────────────────────────────────────────
mcp_servers: {}
# Example:
# mcp_servers:
#   github:
#     command: npx
#     args: ["-y", "@modelcontextprotocol/server-github"]
#     env:
#       GITHUB_PERSONAL_ACCESS_TOKEN: "ghp_xxx"
#     enabled: true
#     timeout: 30
#     tools:
#       include: []
#       exclude: []

# ── Delegation ─────────────────────────────────────────────────────────
delegation:
  enabled: true
  model: ~
  provider: ~
  base_url: ~
  max_subagents: 3
  max_iterations: 50
  shared_budget: false

# ── Compression ────────────────────────────────────────────────────────
compression:
  enabled: true
  threshold: 0.50                # compress when context > 50% of window
  target_ratio: 0.20             # keep 20% uncompressed
  protect_last_n: 20             # always keep last N messages
  summary_model: ~               # null = use main model

# ── Display ────────────────────────────────────────────────────────────
display:
  compact: false
  personality: "helpful"
  show_reasoning: false
  streaming: true
  show_cost: true
  skin: "default"

# ── Privacy ────────────────────────────────────────────────────────────
privacy:
  redact_pii: false

# ── Browser ────────────────────────────────────────────────────────────
browser:
  command_timeout: 30
  record_sessions: false
  recording_max_age_hours: 72

# ── Checkpoints ────────────────────────────────────────────────────────
checkpoints:
  enabled: true
  max_snapshots: 50

# ── TTS ────────────────────────────────────────────────────────────────
tts:
  provider: "edge-tts"           # "edge-tts" | "openai" | "elevenlabs"
  voice: "en-US-AriaNeural"
  rate: ~
  model: ~
  auto_play: true
  elevenlabs_voice_id: ~
  elevenlabs_model_id: ~
  elevenlabs_api_key_env: "ELEVENLABS_API_KEY"

# ── STT ────────────────────────────────────────────────────────────────
stt:
  provider: "local"              # "local" | "groq" | "openai"
  whisper_model: "base"          # tiny|base|small|medium|large-v3
  silence_threshold: -40.0       # dB
  silence_duration_ms: 1500

# ── Voice ──────────────────────────────────────────────────────────────
voice:
  enabled: false
  push_to_talk_key: "ctrl+b"
  continuous: false
  hallucination_filter: true

# ── Honcho ─────────────────────────────────────────────────────────────
honcho:
  enabled: true
  cloud_sync: false
  api_key_env: "HONCHO_API_KEY"
  api_url: "https://api.honcho.dev/v1"
  max_context_entries: 10
  write_frequency: 0             # 0 = manual conclude only

# ── Auxiliary Model ────────────────────────────────────────────────────
auxiliary:
  model: ~
  provider: ~
  base_url: ~
  api_key_env: ~

# ── Reasoning ──────────────────────────────────────────────────────────
reasoning_effort: ""             # "" | "low" | "medium" | "high" | "xhigh"

# ── Timezone ───────────────────────────────────────────────────────────
timezone: ""                     # "" = system timezone; IANA format
```

---

## Field Index

| Config Key | Type | Default | Env Override |
|------------|------|---------|-------------|
| `model.default` | string | `anthropic/claude-sonnet-4-20250514` | `EDGECRAB_MODEL` |
| `model.max_iterations` | integer | `90` | `EDGECRAB_MAX_ITERATIONS` |
| `model.streaming` | bool | `true` | — |
| `model.prompt_caching` | bool | `true` | — |
| `model.cache_ttl` | integer | `300` | — |
| `tools.tool_delay` | float | `1.0` | — |
| `tools.parallel_execution` | bool | `true` | — |
| `tools.max_parallel_workers` | integer | `8` | — |
| `terminal.timeout` | integer | `120` | — |
| `delegation.max_subagents` | integer | `3` | — |
| `delegation.max_iterations` | integer | `50` | — |
| `compression.threshold` | float | `0.50` | — |
| `compression.protect_last_n` | integer | `20` | — |
| `checkpoints.max_snapshots` | integer | `50` | — |
| `tts.provider` | string | `edge-tts` | `EDGECRAB_TTS_PROVIDER` |
| `tts.voice` | string | `en-US-AriaNeural` | `EDGECRAB_TTS_VOICE` |
| `stt.whisper_model` | string | `base` | — |
| `voice.push_to_talk_key` | string | `ctrl+b` | — |
| `honcho.max_context_entries` | integer | `10` | — |
| `gateway.port` | integer | `8080` | `EDGECRAB_GATEWAY_PORT` |
| `gateway.host` | string | `127.0.0.1` | `EDGECRAB_GATEWAY_HOST` |
| `timezone` | string | `""` | `EDGECRAB_TIMEZONE` |
| `reasoning_effort` | string | `""` | `EDGECRAB_REASONING_EFFORT` |
"""

with open(f"{BASE}/reference/configuration.md", "w") as f:
    f.write(reference_config_md)
print("reference/configuration.md written")

# ─── reference/cli-commands.md ────────────────────────────────────────
cli_commands_md = r"""---
title: CLI Commands
description: Complete reference for all edgecrab CLI subcommands and global flags. Grounded in crates/edgecrab-cli/src/cli_args.rs.
sidebar:
  order: 1
---

## Global Flags

These flags apply to the main `edgecrab` command and most subcommands:

| Flag | Short | Description |
|------|-------|-------------|
| `--model <model>` | `-m` | Override default model (e.g. `openai/gpt-4o`) |
| `--toolset <list>` | `-t` | Comma-separated toolset names or aliases |
| `--profile <name>` | `-p` | Run under a named profile |
| `--worktree` | `-w` | Create a git worktree for this session |
| `--skip-memory` | | Disable memory loading and saving |
| `--skip-context-files` | | Skip SOUL.md and AGENTS.md |
| `--quiet` | `-q` | Suppress TUI; print only final response |
| `--verbose` | `-v` | Show raw tool call JSON and debug info |
| `--session <id>` | `-S` | Resume a specific session by ID |
| `--checkpoints` | | Enable checkpoints (overrides config) |
| `--no-checkpoints` | | Disable checkpoints for this session |

---

## Running the Agent

```bash
edgecrab [FLAGS] [MESSAGE]
```

| Form | Behavior |
|------|----------|
| `edgecrab` | Start interactive TUI session |
| `edgecrab "message"` | Start with initial message |
| `edgecrab -q "message"` | One-shot quiet mode |
| `edgecrab -` | Read message from stdin |

---

## `edgecrab config`

Manage the `~/.edgecrab/config.yaml` file.

```bash
edgecrab config show                  # Print active config as YAML
edgecrab config edit                  # Open in $EDITOR
edgecrab config set <key> <value>     # Set any config key
edgecrab config path                  # Print path to config.yaml
edgecrab config env-path              # Print path to .env
edgecrab config edit-soul             # Open SOUL.md in $EDITOR
```

---

## `edgecrab session`

Manage session history.

```bash
edgecrab session list                  # List recent sessions
edgecrab session show <id>             # Show session messages
edgecrab session search <query>        # Full-text search (FTS5)
edgecrab session delete <id>           # Delete a session
edgecrab session export <id> [format]  # Export as markdown, json, or text
```

---

## `edgecrab profile`

Manage named profiles with isolated configurations.

```bash
edgecrab profile list                  # List all profiles
edgecrab profile create <name>         # Create a new profile
edgecrab profile create <name> --clone <from>   # Clone existing profile
edgecrab profile use <name>            # Switch active profile
edgecrab profile show [name]           # Show profile info
edgecrab profile path [name]           # Print profile home path
edgecrab profile delete <name>         # Delete a profile
```

---

## `edgecrab gateway`

Manage the messaging gateway.

```bash
edgecrab gateway start                 # Start gateway daemon
edgecrab gateway stop                  # Stop gateway daemon
edgecrab gateway status               # Show daemon + platform status
edgecrab gateway logs                  # Follow gateway logs
edgecrab gateway restart              # Restart daemon
```

Platform flags (override config):

```bash
edgecrab gateway start --telegram
edgecrab gateway start --discord
edgecrab gateway start --slack
edgecrab gateway start --whatsapp
```

---

## `edgecrab skills`

Manage the skills system.

```bash
edgecrab skills list                   # List installed skills
edgecrab skills install <url>          # Install a skill from URL or path
edgecrab skills install <name>         # Install from skills hub
edgecrab skills uninstall <name>       # Remove a skill
edgecrab skills update                 # Update all installed skills
edgecrab skills update <name>          # Update a specific skill
edgecrab skills show <name>            # Display a skill's SKILL.md
edgecrab skills search <query>         # Search the skills hub
```

---

## `edgecrab mcp`

Manage MCP server connections.

```bash
edgecrab mcp list                      # List configured MCP servers
edgecrab mcp add <name> <command>      # Add an MCP server
edgecrab mcp remove <name>             # Remove an MCP server
edgecrab mcp test <name>               # Test connection to MCP server
edgecrab mcp logs <name>               # Show MCP server logs
```

---

## `edgecrab cron`

Manage scheduled agent jobs.

```bash
edgecrab cron list                     # List all cron jobs
edgecrab cron add <schedule> <task>    # Add a new job
edgecrab cron add --name <n> <schedule> <task>
edgecrab cron enable <name>            # Enable a job
edgecrab cron disable <name>           # Disable a job
edgecrab cron delete <name>            # Delete a job
edgecrab cron run <name>               # Run a job immediately
edgecrab cron logs <name>              # Show last run output
```

---

## `edgecrab migrate`

Database migration tool.

```bash
edgecrab migrate                       # Run all pending migrations
edgecrab migrate status               # Show migration status
edgecrab migrate rollback             # Roll back the last migration
```

---

## `edgecrab doctor`

Run a full diagnostic check.

```bash
edgecrab doctor                       # Check all APIs, tools, platforms
```

Checks:
- Provider API key validity
- MCP server connectivity
- Browser availability (Chrome/CDP)
- Gateway platform status
- Disk space at `EDGECRAB_HOME`
- SQLite database integrity

---

## `edgecrab update`

Update to the latest EdgeCrab version.

```bash
edgecrab update                       # Check and install updates
edgecrab update --check               # Check without installing
```

---

## `edgecrab version`

Print version information.

```bash
edgecrab version                      # Print version, git hash, build date
edgecrab --version                    # Same
```
"""

with open(f"{BASE}/reference/cli-commands.md", "w") as f:
    f.write(cli_commands_md)
print("reference/cli-commands.md written")

print("\nBatch 4 (developer + reference) complete")
