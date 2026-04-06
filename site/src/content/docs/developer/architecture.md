---
title: Architecture
description: Workspace layout, crate dependency chain, key design decisions, and runtime data-flow for EdgeCrab. Grounded in the Cargo workspace at edgecrab/.
sidebar:
  order: 1
---

EdgeCrab is a Rust 2024 edition workspace. Every crate has a single
responsibility and the dependency graph is a strict DAG — no circular
dependencies, no feature flags that reverse the graph.

---

## Workspace Layout

```
edgecrab/
  Cargo.toml              -- workspace manifest
  crates/
    edgecrab-types/       -- shared types (no deps on other crates)
    edgecrab-security/    -- SSRF, path-safety, injection scanning
    edgecrab-state/       -- SQLite WAL session storage + FTS5
    edgecrab-cron/        -- cron scheduler engine
    edgecrab-tools/       -- tool registry, toolsets, all tool impls
    edgecrab-core/        -- agent loop, config, context, LLM client
    edgecrab-cli/         -- ratatui TUI, CLI args, commands, themes
    edgecrab-gateway/     -- messaging adapters (15 platforms)
    edgecrab-acp/         -- ACP JSON-RPC 2.0 stdio adapter
    edgecrab-migrate/     -- schema migrations + one-shot migrate CLI
```

---

## Crate Dependency Graph

Arrows point from dependent to dependency. The graph is a strict DAG.

```
edgecrab-cli ------+
edgecrab-gateway --+---> edgecrab-core
edgecrab-acp ------+          |
edgecrab-migrate --+          +--> edgecrab-tools
                              |         |
                              |         +--> edgecrab-security
                              |         +--> edgecrab-state
                              |         +--> edgecrab-cron
                              |         +--> edgecrab-types
                              |
                              +--> edgecrab-security
                              +--> edgecrab-state
                              +--> edgecrab-types
```

**Key constraint:** `edgecrab-types` has zero edgecrab dependencies.
`edgecrab-security` depends only on `types`. This ensures the security
layer can never accidentally bypass compile-time type safety.

---

## Crate Responsibilities

### `edgecrab-types`

Shared data structures:
`Message`, `ToolCall`, `ToolResult`, `ModelResponse`, `Provider`,
`Session`, `ContentBlock`, `SanitizedPath`, `SafeUrl`.

No business logic — pure `serde`-serializable types.
`SanitizedPath` and `SafeUrl` are _distinct Rust types_, not aliases
for `String`. Functions that accept file paths require `SanitizedPath`;
functions that accept URLs require `SafeUrl`. Passing an unsanitized value
is a *compile error*.

### `edgecrab-security`

Six modules:

```
edgecrab-security/
  path_jail.rs      -- path traversal prevention (SanitizedPath)
  url_safety.rs     -- SSRF guard (SafeUrl, private IP + metadata blocklist)
  command_scan.rs   -- dangerous command scanning (Aho-Corasick + regex)
  injection.rs      -- prompt injection heuristics in tool results
  redact.rs         -- secret / token redaction in output
  approval.rs       -- manual / smart approval policy engine
  normalize.rs      -- ANSI strip + NFKC normalization before scanning
```

The command scanner runs two passes:
1. Aho-Corasick O(n) literal scan across 8 danger categories
2. Regex scan for non-contiguous patterns (e.g. DELETE FROM without WHERE)

### `edgecrab-state`

SQLite WAL-mode session database:
- Session CRUD (create, read, update, delete)
- Message storage and retrieval
- FTS5 full-text search index over message content
- Checkpoint metadata storage
- Schema managed by `edgecrab-migrate`

### `edgecrab-cron`

- Cron expression parser and scheduler (`cron` crate)
- Job storage in `~/.edgecrab/cron/*.json`
- Spawns `AgentLoop` invocations for scheduled tasks
- Timezone-aware scheduling

### `edgecrab-tools`

- **Tool registry:** `ToolRegistry` with `get_definitions()` for LLM
  JSON schema and `dispatch()` for invocation
- **Toolsets:** `toolsets.rs` — alias resolution, `CORE_TOOLS`,
  `ACP_TOOLS`, expansion logic
- **All tool implementations:** file, terminal, web, browser, memory,
  skills, MCP, delegation, code execution, session, checkpoint, cron,
  Honcho, TTS/STT/vision, Home Assistant

### `edgecrab-core`

- `AppConfig` — layered config (defaults → YAML → env → CLI)
- `ContextBuilder` — assembles system prompt from SOUL.md, AGENTS.md,
  memories, and Honcho context
- `AgentLoop` — ReAct iteration: LLM → parse tool calls → security
  check → checkpoint → dispatch → injection scan → inject results → loop
- `LlmClient` — unified OpenAI-compatible HTTP client with streaming,
  prompt caching, and retry logic
- `CompressionEngine` — context window summarisation when threshold hit
- Provider setup: detect API keys, build `ModelConfig` for 14 providers (12 cloud + 2 local)

### `edgecrab-cli`

- `CliArgs` (clap) — all CLI subcommands and flags
- `App` — ratatui TUI main loop
- `CommandRegistry` — slash command dispatch (O(1) HashMap lookup)
- `CommandResult` enum — 58+ variants for typed command outcomes
- `Theme` + `SkinConfig` — TUI colors, symbols, personalities
- Background session manager; worktree creation and cleanup

### `edgecrab-gateway`

HTTP API server (Axum) plus platform adapters:

```
Platform        Source file          Env vars needed
------------------------------------------------------
Telegram        telegram.rs          TELEGRAM_BOT_TOKEN
Discord         discord.rs           DISCORD_BOT_TOKEN
Slack           slack.rs             SLACK_BOT_TOKEN + SLACK_APP_TOKEN
Signal          signal.rs            SIGNAL_HTTP_URL + SIGNAL_ACCOUNT
WhatsApp        whatsapp.rs          QR pairing wizard (edgecrab whatsapp)
Matrix          matrix.rs            MATRIX_HOMESERVER + MATRIX_ACCESS_TOKEN
Mattermost      mattermost.rs        MATTERMOST_URL + MATTERMOST_TOKEN
DingTalk        dingtalk.rs          DINGTALK_APP_KEY + DINGTALK_APP_SECRET
SMS             sms.rs               TWILIO_ACCOUNT_SID + TWILIO_AUTH_TOKEN
Email           email.rs             EMAIL_PROVIDER + EMAIL_FROM + EMAIL_API_KEY
Home Assistant  homeassistant.rs     HASS_URL + HASS_TOKEN
Webhook         webhook.rs           (any HTTP caller)
API Server      api_server.rs        API_SERVER_PORT (optional)
Feishu/Lark     feishu.rs            FEISHU_APP_ID + FEISHU_APP_SECRET
WeCom           wecom.rs             WECOM_CORP_ID + WECOM_SECRET
```

Platform adapters:
- Approval workflow (inline buttons on Telegram / Discord)
- Proactive home-channel messaging
- Per-platform `allowed_users` allowlists
- Delivery confirmation and retry

### `edgecrab-acp`

ACP (Agent Communication Protocol) JSON-RPC 2.0 adapter over stdio.
Exposes EdgeCrab to VS Code, Zed, and JetBrains via `edgecrab acp`.

Uses `ACP_TOOLS` subset — no `clarify`, `send_message`,
`generate_image`, or `text_to_speech` (interactive-only tools that
do not make sense in an editor integration).

### `edgecrab-migrate`

- SQLite schema migration engine (idempotent, append-only)
- `edgecrab migrate` CLI subcommand
- One-shot hermes-agent → EdgeCrab state importer

---

## Agent Loop Data Flow

```
User types message
       |
       v
ContextBuilder.build()
  -- SOUL.md + AGENTS.md
  -- ~/.edgecrab/memories/*.md
  -- Honcho context (cross-session user model)
       |
       v
AgentLoop.run()
  +----- Step 1: LlmClient.complete(messages) ----+
  |      streaming tokens --> TUI token feed       |
  |                                                |
  |  Step 2: Parse tool_calls from response        |
  |                                                |
  |  Step 3: SecurityChecker per tool call         |
  |    -- url_safety check (SSRF guard)            |
  |    -- path_jail check (traversal guard)        |
  |    -- command_scan check (injection guard)     |
  |                                                |
  |  Step 4: Build checkpoint if op is destructive |
  |    (write_file / patch / dangerous terminal)   |
  |                                                |
  |  Step 5: ToolRegistry.dispatch(tool_call)      |
  |    (parallel dispatch where allowed)           |
  |                                                |
  |  Step 6: injection.check(result)               |
  |    (scan tool results for prompt injection)    |
  |                                                |
  |  Step 7: Append ToolResult to messages         |
  |                                                |
  |  Step 8: Check max_iterations; loop -> Step 1  |
  +------------------------------------------------+
       |
       v
auto_flush memory (if enabled)
       |
       v
honcho_conclude (if enabled)
       |
       v
Session saved to SQLite (WAL commit)
```

---

## Key Design Decisions

**1. Single binary.** Everything compiles into one statically-linked
executable. No Python venv, no Node.js, no shared libraries except
the OS. Browser tools require Chrome — that is the only soft
dependency. Cold start: < 50 ms.

**2. Security at the type level.** `SanitizedPath` and `SafeUrl` are
distinct Rust types in `edgecrab-types`. Functions that touch the
filesystem accept only `SanitizedPath`. Bypassing the sanitizer is
a *compile error*, not a runtime risk.

**3. SQLite over file-based state.** WAL mode gives concurrent reads
with atomic writes. FTS5 enables instant full-text search across all
session history without an external search service. The database file
is `~/.edgecrab/state.db`.

**4. Toolset as policy, not registry.** The registry owns what tools
exist. Toolsets own which tools are active per session. Policy changes
never require touching tool implementations.

**5. No Python.** EdgeCrab is a ground-up Rust rewrite of hermes-agent.
Result: < 50 ms cold start (vs 1–3 s), ~15 MB resident (vs ~80–150 MB), no GC
pauses, no venv to manage.

**6. Config resolution order.** `AppConfig::default()` → `config.yaml`
→ `EDGECRAB_*` env vars → CLI flags. Later layers always win.
This makes container and CI deployments predictable: set env vars,
never edit files.

---

## Pro Tips

- **Read `edgecrab-types` first**: All shared data structures live there. Understanding `Message`, `SanitizedPath`, and `SafeUrl` before touching any other crate saves a lot of friction.
- **Check the DAG before adding a dep**: If crate A already transitively depends on crate B, adding an explicit dep from B to A creates a cycle and won't compile. Use `cargo tree -p <crate>` to verify.
- **`edgecrab-security` is your first read for security work**: It owns path jail, SSRF guard, command scanning, injection detection, and the approval engine — all in one place.
- **The compression threshold is 0.50 by default**: If sessions feel slow on large contexts, check `compression.threshold` in config — lowering it triggers earlier summarisation.
- **`cargo doc --workspace --no-deps --open`**: Generates and opens the full local doc tree. Faster than reading individual files when exploring a new crate.

---

## FAQ

**Why 10 separate crates instead of one?**
Each crate maps to a distinct responsibility with a clear interface. The benefit is clean security boundaries: `edgecrab-security` can never depend on `edgecrab-core`, so security logic can't be accidentally bypassed by core code.

**Why SQLite instead of a file-based session store?**
FTS5 full-text search, WAL-mode concurrent access, and atomic transactions. Session search across thousands of messages is instant without an external search service.

**Why is the binary ~15 MB instead of smaller?**
Static linking embeds all dependencies (TLS, SQLite, the Aho-Corasick scanner, etc.) into a single executable. No dynamic library dependencies means the binary runs on any Linux distro without managing shared libraries.

**How do I find which crate owns a given feature?**
See the Crate Responsibilities table above, or search: `grep -rn "fn my_function" crates/` to find the implementation.

---

## See Also

- [Contributing](/developer/contributing/) — build, test, and PR process
- [Configuration Reference](/reference/configuration/) — `AppConfig` fields and defaults
- [Security Model](/features/security/) — runtime security layers in detail
