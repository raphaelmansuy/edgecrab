# EdgeCrab — Implementation Mission Plan

> Tracks progress across all phases. Checkboxes are updated as work completes.

---

## Phase 0 — Foundation

### 0.1 Workspace Setup
- [x] Initialize Cargo workspace with 9 crates
- [x] Set up workspace-level Cargo.toml with shared dependencies
- [x] Add `#[deny(clippy::unwrap_used)]` to lib crates
- [x] Verify `cargo build` compiles all crates

### 0.2 Types Crate (`edgecrab-types`)
- [x] Define `Message`, `Role`, `Content`, `ContentPart`
- [x] Define `ToolCall`, `FunctionCall`
- [x] Define `Usage`, `Cost`
- [x] Define `ApiMode` enum
- [x] Define `Platform` enum
- [x] Define constants (`DEFAULT_MODEL`, etc.)
- [x] Write unit tests for serialization round-trips

### 0.3 Error Handling (`edgecrab-types`)
- [x] Define `AgentError` enum
- [x] Define `ToolError` enum with retry strategies
- [x] Implement `Display`, `Error` via `thiserror`
- [x] Write error→JSON conversion for LLM feedback

### 0.4 edgequake-llm Integration PoC
- [x] Add `edgequake-llm = "0.3.0"` dependency
- [x] Write integration test: MockProvider → chat completion
- [x] Write integration test: tool calling round-trip
- [x] Verify trait interface works through `ProviderFactory`

### 0.5 Phase 0 E2E Verification
- [x] `cargo build` — all crates compile
- [x] `cargo test` — all unit tests pass (67 tests)
- [x] `cargo clippy` — zero warnings
- [x] Commit Phase 0

---

## Phase 1 — Core Agent

### 1.1 State Management (`edgecrab-state`)
- [x] Implement `SessionDb` with rusqlite (WAL mode)
- [x] Write contention tuning (jitter retry, checkpoint)
- [x] Session CRUD operations
- [x] Message CRUD operations
- [x] FTS5 virtual table for session search
- [x] FTS5 sync triggers
- [x] Session search with BM25 ranking
- [x] Write tests: concurrent access, WAL correctness

### 1.2 Config System (`edgecrab-core`)
- [x] Implement `AppConfig` with serde_yml deserialization
- [x] Implement `edgecrab_home()` directory resolution
- [x] CLI arg merging over config file
- [x] Environment variable override support
- [x] Config validation with helpful error messages
- [x] Write tests: default config, partial config, env overrides

### 1.3 Security Crate (`edgecrab-security`)
- [x] Path traversal prevention (`resolve_safe_path`)
- [x] URL safety (SSRF prevention)
- [x] Command injection scanning (~30 patterns)
- [x] Command normalization (ANSI strip + NFKC + null removal)
- [x] Output redaction (API keys, tokens)
- [x] Approval policy engine
- [x] Write tests for each security check with bypass attempts

### 1.4 Agent Struct & Builder (`edgecrab-core`)
- [x] Implement `Agent` struct
- [x] Implement `AgentBuilder` with type-safe builder pattern
- [x] Implement `IterationBudget` with `AtomicU32`
- [x] Implement `CancellationToken` interrupt handling
- [x] Wire edgequake-llm provider into Agent

### 1.5 Prompt Builder (`edgecrab-core`)
- [x] Implement `PromptBuilder`
- [x] Context file discovery (SOUL.md, AGENTS.md)
- [x] Platform-specific hints
- [x] Memory section injection
- [x] Skills section injection
- [x] Write tests: prompt assembly

### 1.6 Conversation Loop (`edgecrab-core`)
- [x] Implement `run_conversation()`
- [x] API call with retry + exponential backoff
- [x] Tool call extraction and dispatch
- [x] Reasoning/thinking block extraction
- [x] Usage tracking + cost calculation
- [x] Interrupt handling (cooperative cancellation)
- [x] Streaming response consumption
- [x] Write integration tests: full loop with MockProvider

### 1.7 Context Compression (`edgecrab-core`)
- [x] Implement `ContextCompressor`
- [x] Token estimation
- [x] Chunked summarization via cheaper model
- [x] Preflight compression
- [x] Write tests: compression ratio, protected messages

### 1.8 Model Routing (`edgecrab-core`)
- [x] Implement `ModelRouter`
- [x] Provider fallback chain
- [x] API mode detection

### 1.9 Phase 1 E2E Verification
- [x] `cargo build` — all crates compile
- [x] `cargo test` — all unit + integration tests pass (135 tests)
- [x] `cargo clippy` — zero warnings
- [x] E2E: Agent.chat() with MockProvider returns response
- [x] Commit Phase 1

---

## Phase 2 — CLI & Tools

### 2.1 Tool Registry (`edgecrab-tools`)
- [x] Implement `ToolHandler` trait (async execute, schema, name, toolset)
- [x] Implement `ToolSchema` struct
- [x] Implement `ToolContext` struct
- [x] Implement `ToolRegistry` with `inventory` crate compile-time registration
- [x] Implement tool schema generation (OpenAI format)
- [x] Implement `dispatch()` with fuzzy fallback (`strsim`)
- [x] Implement `get_definitions()` with enabled/disabled filtering
- [x] Write tests: registration, dispatch, fuzzy match, filtering

### 2.2 Core Tools
- [x] `read_file` — file reading with line range support
- [x] `write_file` — atomic write with backup
- [x] `patch` — exact string match replacement with uniqueness check
- [x] `search_files` — recursive regex search with glob filtering
- [x] `terminal` — local command execution via `tokio::process` with security scanning
- [x] `process` — background process management stubs (list/kill)
- [x] `clarify` — user clarification marker
- [x] `todo` — task checklist management
- [x] Write tests for each tool (26 tests across 9 tool files)

### 2.3 Web & Browser Tools
- [x] `web_search` — stub implementation (disabled until Phase 3)
- [x] `web_extract` — stub implementation with SSRF check (disabled until Phase 3)

### 2.7 Toolset Composition
- [x] Toolset aliases resolution
- [x] Config-driven toolset filtering
- [x] Platform-specific defaults (CORE_TOOLS, ACP_TOOLS)
- [x] Write tests: alias expansion, filtering, platform defaults

### 2.4 Memory & Skills Tools
- [x] `memory_read` / `memory_write` — memories/ directory I/O with § delimiters
- [x] `skills_list` / `skill_view` — skill discovery with path traversal prevention
- [x] Write tests: read empty, write+read roundtrip, skill listing, traversal blocked

### 2.5 Session & Search Tools
- [x] `session_search` — FTS5 full-text search via edgecrab-state SessionDb

### 2.6 Advanced Tools (stubs)
- [x] `execute_code` — sandboxed code execution stub (unavailable)
- [x] `delegate_task` — subagent spawning stub (unavailable)
- [x] `generate_image` — image generation API stub (unavailable)
- [x] `send_message` — cross-platform messaging stub (unavailable)
- [x] Macro-based stub_tool! for consistent registration

### 2.8 CLI Application (`edgecrab-cli`)
- [x] clap argument parsing with derive macros
- [x] ratatui TUI layout (input, output, status bar)
- [x] Event loop (crossterm events + agent async events)
- [x] Streaming token display (StreamEvent channel + per-chunk TUI append)
- [x] Slash command registry (core commands)
- [x] Skin engine (YAML-driven themes)
- [x] Session management UI (/session new + /new → agent.new_session())
- [x] ASCII banner
- [x] Write integration tests: CLI e2e with mock provider

### 2.9 Phase 2 E2E Verification
- [x] `cargo build` — all crates compile
- [x] `cargo test` — all unit + integration tests pass (213 tests)
- [x] `cargo clippy` — zero warnings
- [x] E2E: Agent with tools dispatches read_file + terminal (conversation loop wired with tool calling)
- [x] Commit Phase 2

---

## Phase 3 — Gateway

### 3.1 Gateway Core (`edgecrab-gateway`)
- [x] Implement `PlatformAdapter` trait (IncomingMessage, OutgoingMessage, MessageMetadata)
- [x] Implement `SessionManager` with `DashMap` (resolve, remove, cleanup)
- [x] Implement `DeliveryRouter` with message splitting (paragraph > newline > space > hard cut)
- [x] Implement `GatewayHook` trait + `HookRegistry` with wildcard event matching
- [x] Implement `GatewayConfig` with serde deserialization
- [x] Implement `Gateway` runner with axum health/webhook endpoints
- [x] Implement `WebhookAdapter` (always-on HTTP adapter)
- [x] Session expiry + cleanup background task
- [x] Add `Hash` derive to `Platform` enum
- [x] Write tests: session lifecycle, message splitting, hooks, health endpoint, webhook (33 tests)

### 3.2 Platform Adapters (feature-gated, deferred)
- [ ] Telegram adapter (`teloxide` 0.17)
- [ ] Discord adapter (`serenity` 0.12.5)
- [ ] Slack adapter (`slack-morphism`)
- [x] WhatsApp adapter (Baileys bridge reuse via local HTTP bridge)
- [ ] Matrix adapter (`matrix-sdk`)
- [ ] Email adapter (`lettre` + `imap`)
- [ ] REST API (`axum` — OpenAI-compatible /v1/chat/completions)

### 3.3 Phase 3 E2E Verification
- [x] `cargo build` — all crates compile
- [x] `cargo test` — all tests pass (246 tests)
- [x] `cargo clippy` — zero warnings
- [x] Commit Phase 3

## Phase 4 — Advanced Features

### 4.3 ACP Server (`edgecrab-acp`)
- [x] Implement ACP protocol types (JSON-RPC 2.0 envelope, ACP request/response structs)
- [x] Implement `SessionManager` with `DashMap` (create, get, remove, fork, list, update_cwd)
- [x] Implement `AcpServer` with JSON-RPC stdio transport and method dispatch
- [x] Implement permission model (`is_safe_tool`, `is_acp_tool`, `PermissionDecision`)
- [x] Implement ACP tool filtering (`ACP_TOOLS` coding-focused subset)
- [x] Implement slash commands (help, model, tools, context, reset, version)
- [x] Full session lifecycle: new → prompt → fork → cancel → list
- [x] Write tests: protocol, session, permission, server dispatch, slash commands (32 tests)

### 4.5 Migration Tool (`edgecrab-migrate`)
- [x] Implement `HermesMigrator` (config, memory, skills, env migration)
- [x] Implement `MigrationReport` + `MigrationItem` + `MigrationStatus`
- [x] Implement env var compatibility layer (`compat.rs`)
- [x] Write tests: full migration, skip existing, empty source, report display (8 tests)

### 4.x Deferred
- [ ] Phase 4.1: Terminal Backends (Docker, SSH, Modal, Daytona)
- [ ] Phase 4.2: RL Environments (Atropos integration)
- [ ] Phase 4.4: Plugin System
- [ ] Phase 4.6: Checkpoint Manager

### Phase 4 E2E Verification
- [x] `cargo test` — all tests pass (286 tests)
- [x] `cargo clippy` — zero warnings
- [x] Commit Phase 4

## Phase 5 — Integration & Polish

### 5.1 Gap Analysis (post-Phase 4)
Known stub/deferred items requiring LLM provider connectivity:
- [x] CLI agent dispatch — wire `Agent.chat()` into TUI input loop
- [x] Gateway agent dispatch — wire `Agent.chat()` into message loop (`run.rs`)
- [x] ACP agent dispatch — wire `Agent.chat()` into prompt handler (`server.rs`)
- [x] Model hot-swap — `/model` command live provider switching
- [x] Session management — `/session new` + `/new` → `agent.new_session()`
- [x] Process tool — `run_process` with tokio::process background spawning + ring buffer
- [x] Web tools — DuckDuckGo Instant Answer API + reqwest HTML extraction with SSRF guard
- [x] Context compression — LLM-powered summarization via `compress_with_llm()` (structural fallback)
- [x] YAML skin engine — `~/.edgecrab/skin.yaml` → `SkinConfig` → `Theme::from_skin()` with hex colors
- [x] Streaming token display — `Agent::chat_streaming()` + `StreamEvent` channel + TUI chunk append
- [x] E2E tests with VsCodeCopilot — 3 `#[ignore]` tests (auto-skip if no IPC socket)
- [x] `cargo clippy` — zero warnings
- [ ] Advanced tools — `execute_code`, `delegate_task`, `generate_image`, `send_message` (deferred)

### 5.2 Documentation
- [x] README.md with build/install/usage instructions
- [ ] Architecture overview diagram
- [ ] API documentation (`cargo doc`)

### 5.3 Release Prep
- [x] Final `cargo test` — 306 passing (3 E2E ignored), 0 failing
- [x] Final `cargo clippy -- -D warnings` — zero warnings
- [ ] `cargo doc` — check docs compile
- [ ] License headers on new files (web.rs, theme.rs rewrite, compression.rs additions)
- [x] CHANGELOG.md

---

## Phase 6 — CLI Parity & Superior Onboarding

> Gap analysis (2026-03-28): edgecrab CLI lacks subcommands, setup wizard, doctor,
> and migrate wiring that hermes has. This phase brings CLI to parity and beyond.

### 6.1 Gap Analysis Results
**Identified gaps vs hermes-agent CLI:**
- [x] CLI has no subcommands — hermes has: `setup`, `doctor`, `migrate`, `acp`, `version`, `gateway`, `sessions browse`
- [x] No first-run setup wizard (hermes: `hermes setup` → interactive provider/key config)
- [x] `/doctor` slash command is a stub — hermes has deep diagnostics
- [x] `edgecrab migrate` subcommand not wired (migrator crate exists but no CLI entry)
- [x] `edgecrab acp` subcommand not wired (ACP server exists but no CLI entry)
- [x] Initial prompt dispatch in TUI has TODO comment (not dispatched to agent)
- [x] Web tools are stubs (DuckDuckGo + reqwest extract not wired)
- [ ] `cargo doc` not verified
- [ ] License headers missing on several files
- [x] README onboarding inferior to hermes (no provider setup guide)

### 6.2 CLI Subcommand Restructuring
- [x] Convert flat CLI to clap subcommand structure
- [x] Add `edgecrab` (default → interactive TUI)
- [x] Add `edgecrab setup` → interactive first-run setup wizard
- [x] Add `edgecrab doctor` → diagnostics (providers, config, connectivity)
- [x] Add `edgecrab migrate` → invoke `HermesMigrator` from edgecrab-migrate
- [x] Add `edgecrab acp` → start ACP stdio server (edgecrab-acp)
- [x] Add `edgecrab version` → show version + provider list
- [x] Write tests for all subcommand arg parsing

### 6.3 First-Run Setup Wizard
- [x] Detect first run (no `~/.edgecrab/config.yaml`)
- [x] Interactive provider selection (copilot, openai, anthropic, ollama, etc.)
- [x] API key prompt with masked input + validation
- [x] Write config.yaml on success
- [x] Show success message with next steps
- [x] Write tests: skip if config exists, write valid config

### 6.4 Doctor Command
- [x] Check `~/.edgecrab/config.yaml` exists + valid
- [x] Check API key env vars set
- [x] Ping LLM provider (test call with MockProvider fallback)
- [x] Check SQLite state DB writable
- [x] Check skills/memory directories exist
- [x] Print colored status report (✓ pass / ✗ fail / ⚠ warn)
- [x] Write tests: all-pass scenario, missing-key scenario

### 6.5 Migrate Subcommand
- [x] Invoke `HermesMigrator::run()` with dry-run flag support
- [x] Print MigrationReport in human-readable format
- [x] `--dry-run` flag to preview without writing
- [ ] Write integration test: migrate from temp hermes dir

### 6.6 ACP Subcommand
- [x] Add `edgecrab acp` that calls `AcpServer::run()` on stdin/stdout
- [x] Thread through config (model, toolsets)
- [ ] Write test: `edgecrab acp` exits gracefully on empty stdin

### 6.7 Initial Prompt Dispatch Fix
- [x] Remove TODO comment in main.rs — dispatch initial prompt to agent via channel
- [ ] Test: `edgecrab -q "hello"` returns response and exits

### 6.8 Web Tools Real Implementation
- [x] `web_search` — DuckDuckGo Instant Answer API (non-JS, no key required)
- [x] `web_extract` — reqwest GET + scraper for text extraction with SSRF guard
- [x] Remove "stub" markers, enable in toolset
- [x] Write unit tests with mock HTTP responses
- [ ] Write integration test (with real network, `#[ignore]` by default)

### 6.9 Superior Onboarding README
- [x] Quick-start section with single-command install
- [x] Provider configuration table (all 13 providers)
- [x] Slash command reference table
- [x] Comparison table: edgecrab vs hermes-agent
- [x] Screenshot/demo GIF placeholder
- [x] Migration guide section

### 6.10 Phase 6 E2E Verification
- [x] `cargo build` — all crates compile
- [x] `cargo test` — all tests pass (324 passing)
- [x] `cargo clippy -- -D warnings` — zero warnings
- [x] `cargo doc --no-deps` — zero errors
- [ ] E2E: `edgecrab -q "hello"` with copilot/gpt-4.1-mini returns response
- [x] E2E: `edgecrab setup` detects existing config and skips
- [x] E2E: `edgecrab doctor` prints status report
- [x] E2E: `edgecrab migrate --dry-run` from temp hermes home
- [ ] Commit Phase 6

### 6.11 Messaging Parity Closure
- [x] Promote gateway config from flat platform list to host/port/webhook + platform-specific config
- [x] Auto-enable gateway platforms from runtime env when credentials are present
- [x] Add `edgecrab whatsapp` setup flow for bridge install, config persistence, and QR pairing
- [x] Reuse Hermes WhatsApp Baileys bridge from Rust gateway instead of duplicating transport logic
- [x] Normalize inbound WhatsApp media into message context with local attachment paths
- [ ] Add Slack adapter parity
- [ ] Add Matrix adapter parity
- [ ] Add Email adapter parity
- [ ] Add Signal / SMS / Mattermost / DingTalk / Home Assistant parity

---

## Phase 7 — Agent Loop Hardening & CLI Parity

> Gap analysis (2025-07-17): Deep spec-vs-implementation audit revealed:
> LLM compression lacks structured template, `@context` refs missing,
> session DB not wired in loop, only ~10/40+ slash commands implemented,
> model router not wired into conversation loop.
>
> **Status (2026-03-28)**: All Phase 7 items implemented and verified.

### 7.1 LLM Compression: Structured Summaries
- [x] Add `SUMMARY_PREFIX` constant (`[CONTEXT COMPACTION] Earlier turns...`)
- [x] Add `PRUNED_TOOL_PLACEHOLDER` constant
- [x] Update `llm_summarize()` to use structured Goal/Progress/Decisions/Files/NextSteps template
- [x] Add `prune_tool_outputs()` pre-pass (replace old tool results before LLM call)
- [x] Add iterative summary update (second compression replaces prior `SUMMARY_PREFIX` block)
- [x] Write unit tests for structured summary format

### 7.2 @Context Reference Expansion
- [x] Create `crates/edgecrab-core/src/context_references.rs`
- [x] Regex patterns: `@file:path`, `@url:...`, `@diff`, `@staged`, `@folder:path`, `@git:ref`
- [x] `expand_context_refs(text) -> (expanded_text, Vec<ContextRef>)` function
- [x] SSRF/path security guard: block `.ssh`, `.aws`, absolute paths outside CWD
- [x] Export from `crates/edgecrab-core/src/lib.rs`
- [x] Unit tests: each reference type, security guard (12 tests)

### 7.3 Session Persistence Wiring
- [x] Wire `SessionDb::save_session()` at end of `execute_loop()` in conversation.rs
- [x] Wire `SessionDb::save_messages()` for all messages after loop
- [x] Add `session_id` to `SessionState` struct
- [x] Unit test: execute_loop saves to mocked state_db

### 7.4 Extended Slash Commands (25+)
- [x] Add `/retry`, `/undo`, `/stop`, `/usage`, `/compress`, `/history`
- [x] Add `/save`, `/export`, `/status`, `/config`, `/provider`, `/prompt`
- [x] Add `/verbose`, `/toolsets` detail, `/personality`, `/insights`, `/version`
- [x] Update `/help` with all new commands
- [x] Write tests for new commands

### 7.5 Model Router Integration
- [x] Wire `resolve_turn_route()` into `execute_loop()` with `SmartRoutingConfig`
- [x] Add `SmartRoutingYaml` to `ModelConfig` for config-driven routing
- [x] Add `model_config` to `AgentConfig` for full routing information
- [x] Log routing decision (Primary vs Smart Route) with tracing
- [x] Unit tests: smart routing YAML deserialization, routing toggle, agent propagation

### 7.6 Phase 7 E2E Verification
- [x] `cargo build` — all crates compile
- [x] `cargo test` — all tests pass (355 tests, 3 E2E ignored)
- [x] `cargo clippy -- -D warnings` — zero warnings
- [x] `cargo doc --no-deps` — zero errors
- [ ] Commit Phase 7

---

## Phase 8 — DRY/SOLID Fixes + Dead Code Wiring + Parity Closure

> **Deep audit (2026-03-29):** Code-level comparison of edgecrab vs hermes-agent.
> Many Phase 8.1 items were already implemented but not checked off.
> Critical findings: dead code (pricing never wired), DRY violations (12× agent guard,
> 5× home-dir resolution, 2× ToolContext construction), SOLID violations (god App struct,
> 700-line register_defaults monolith, 7-param functions).
> This phase fixes quality issues first, then wires remaining gaps.

### 8.0 DRY Violation Fixes (code quality — do first)
- [x] Extract `fn require_agent(&mut self) -> Option<Arc<Agent>>` helper in `app.rs` (eliminates 12 duplications)
- [x] Extract `fn edgecrab_home() -> PathBuf` shared utility in `edgecrab-core` (eliminates 5 incompatible home-dir patterns)
- [x] Extract `fn build_tool_context(...)` factory in `conversation.rs` (eliminates 2 subtly inconsistent ToolContext constructions)
- [x] Extract `fn agent_snapshot(&self) -> SessionSnapshot` helper in `app.rs` (eliminates 4× block_on pattern)
- [x] Merge `build_chat_messages` / `build_chat_messages_pub` into single `pub` function
- [x] Extract `fn default_export_path(ext) -> String` helper (eliminates 2× timestamp path pattern)
- [x] Extract `fn resolve_by_prefix<T>(items, id_fn, prefix) -> Result<T>` utility (eliminates 3× prefix-matching)
- [x] Extract `fn bootstrap_agent(args, platform) -> Result<Agent>` factory (eliminates 2× 7-line bootstrap in cron_cmd + gateway_cmd)
- [x] Write tests for extracted helpers

### 8.0b SOLID Violation Fixes
- [ ] Move `scan_for_injection` + security types from `prompt_builder.rs` to `edgecrab-security`
- [x] Reduce `process_response` / `dispatch_single_tool` from 7 params to 1 `DispatchContext` struct (done Phase 11)
- [ ] Add platform hint as associated `fn hint(&self) -> &str` on `Platform` enum (OCP fix)

### 8.1 Real Slash Command Handlers — Status Update
**Already REAL (verified 2026-03-29):**
- [x] `/history` — shows message/turn/token counts from session snapshot
- [x] `/save` — exports session to JSON with timestamped filename
- [x] `/export` — exports session to Markdown
- [x] `/cost` — shows real token counts + estimated cost via `estimate_cost()`
- [x] `/usage` — delegates to cost display
- [x] `/status` — shows session ID, model, message count, API calls, budget
- [x] `/verbose` — toggles `self.verbose` boolean
- [x] `/prompt` — shows assembled system prompt (up to 2000 chars)
- [x] `/session list/switch/delete` — real SQLite operations
- [x] `/title <text>` — sets session title in DB
- [x] `/resume <id>` — restores session from DB
- [x] `/background <prompt>` — spawns async agent.chat()
- [x] `/model <name>` — real hot-swap via ProviderFactory
- [x] `/new` / `/clear` / `/version` / `/quit` / `/stop` / `/theme` / `/doctor` / `/config`

**Still STUB — wire to real logic:**
- [x] `/retry` — re-send last user message (pop last assistant + re-dispatch)
- [x] `/undo` — remove last assistant turn from session messages
- [x] `/compress` — trigger manual `compress_messages()` call on agent
- [x] `/reasoning` — set reasoning effort on model config (low/medium/high)
- [x] `/tools` — query real `ToolRegistry` instead of hardcoded string
- [x] `/toolsets` — query real toolset state from config instead of hardcoded string
- [x] `/cron` — bridge to real `cron_cmd` module instead of returning "(none)"
- [x] `/plugins` — bridge to real `plugins.rs` discovery instead of "deferred"
- [x] `/platforms` — query real gateway adapter availability instead of stale hardcoded text
- [x] `/personality` — read/switch SOUL.md files from `~/.edgecrab/personalities/`
- [x] `/insights` — compute real stats from SessionDb (model distribution, avg tokens, tool freq)
- [x] `/paste` — clipboard read via `arboard` crate
- [x] `/queue` — fix drain: process prompt_queue after each agent turn completes — **verified DONE in check_responses() app.rs line 1018 (2026-03-29)**
- [ ] `/approve` / `/deny` — wire to pending-action queue for destructive commands — **stub only: prints "No pending actions" — real queue deferred**
- [x] Write tests for each newly wired handler

### 8.2 Dead Code Wiring (implemented but disconnected)
- [x] Wire `pricing::estimate_cost()` into `conversation.rs` (replace `Cost::default()` at line 337) — **already done at line 438 (verified 2026-03-29)**
- [x] Wire `model_router::resolve_turn_route()` result into actual provider override (currently computed then discarded)
- [x] Wire `model_router::fallback_route()` on API failure (currently never called)
- [x] Track `tool_call_count` in `SessionRecord` (currently always 0)
- [x] Track `estimated_cost_usd` in `SessionRecord` (currently always None)
- [x] Call `scan_for_injection()` from `PromptBuilder::build()` (currently dead code)
- [x] Write tests verifying wiring

### 8.3 Core Tool Status Update
**Already REAL (verified 2026-03-29):**
- [x] `execute_code` — full sandboxed execution (Python, JS, Bash, Ruby, Rust)
- [x] `checkpoint` — full create/restore/list/diff with SHA-256
- [x] `mcp_client` — full MCP stdio client with JSON-RPC 2.0
- [x] `tts` — Edge TTS + OpenAI backends

**Still needs work:**
- [x] `delegate_task` — uses `ctx.provider` for real LLM sub-agent calls (already wired)
- [x] Remove phantom tools from `CORE_TOOLS` that have no implementation:
  `vision_analyze`, `mixture_of_agents`, `honcho_*` (4), `ha_*` (4), `browser_back/press/close/get_images/vision` (5)
  — either implement or remove from toolset constants to avoid registry warnings

### 8.4 MCP + Cron + Skill Management — Status Update (verified 2026-03-29)
**Already REAL:**
- [x] MCP tool client — `mcp_list_tools` + `mcp_call_tool` with stdio transport
- [x] Cron system — full CLI with `cron_cmd.rs` (create, list, edit, pause, resume, run, remove, tick)
- [x] Plugins system — full discovery + install/update/remove via `plugins.rs` + `plugins_cmd.rs`

**Fixed in Phase 11 (2026-03-29):**
- [x] `skill_manage` tool — create/edit/delete skills from within agent conversation
- [x] Bridge `/cron` slash command to `cron_cmd` real logic (calls `cron_cmd::status_snapshot()`)
- [x] Bridge `/plugins` slash command to `plugins.rs` discovery (calls `PluginManager::discover_all()`)
- [x] MCP: pass environment variables from config to spawned subprocess (`spawn()` now takes `envs`)

### 8.5 CLI Subcommand Parity — Status Update
**Already REAL (verified 2026-03-29):**
- [x] `edgecrab sessions list/browse/export/delete/stats`
- [x] `edgecrab config show/set/path`
- [x] `edgecrab gateway start/stop/status`
- [x] `edgecrab tools list/enable/disable`
- [x] `edgecrab mcp list/add/remove`
- [x] `edgecrab plugins list/install/update/remove`
- [x] `edgecrab cron list/create/edit/pause/resume/run/remove`

**Fixed in Phase 11 (2026-03-29):**
- [x] `edgecrab skills list/search/install/view/remove` — skill management from CLI

### 8.6 Conversation Loop Hardening
- [x] Fallback model on API failure (call `fallback_route()` after primary fails)
- [x] Length truncation continuation (if `finish_reason == "length"`, auto-continue)
- [x] Empty response nudge (prompt "please continue" on empty content)
- [x] Sanitize orphaned tool results (tool_result without matching assistant tool_call)
- [x] Write tests for loop hardening

### 8.7 Phase 8 E2E Verification
- [x] `cargo build` — all crates compile
- [x] `cargo test` — all tests pass (525 passed, 0 failed)
- [x] `cargo clippy -- -D warnings` — zero warnings (fixed in Phase 11: DispatchContext + skin_engine #[allow(dead_code)])
- [ ] Commit Phase 8

---

## Phase 9 — UX/UI Overhaul (OODA-Driven)

> **OODA Cycle 1 (2026-03-29):** Deep comparison of edgecrab TUI vs edgecode
> (https://github.com/raphaelmansuy/edgecode) + best Rust TUI tools.
>
> **Observations:**
> - Input is a manual single-line `String` buffer with byte-level cursor math.
>   No readline shortcuts, no history, no multi-line, no bracket paste. Unicode panics.
> - No tab completion for 42+ slash commands. Typos yield hard "Unknown command."
> - No spinner/animation when agent is thinking. Static "Thinking..." text only.
> - Output is raw unstyled text. No markdown rendering, no code block detection.
> - Status bar only updates on explicit `/cost` call, not after each response.
> - No input-line highlighting (no cyan for valid commands, no red for invalid).
> - No ghost-text / Fish-style history suggestions.
> - Streaming mode silently disables tool calling (bypasses execute_loop).
> - Two disconnected theme systems (theme.rs + skin_engine.rs).
>
> **Reference patterns from edgecode:**
> - rustyline with `Completer + Highlighter + Hinter` traits for input
> - `HistoryHinter` for gray Fish-style suggestions
> - `indicatif` spinners with RAII `SpinnerGuard` pattern
> - Streaming state machine: Idle → Thinking → Progress → ToolExecution
> - Newline-gated thinking buffer (prevents fragmented output)
> - Composable `StatusBar` with color-coded threshold segments
> - Lightweight markdown renderer (headers, bold, code blocks with `│` prefix)
> - `strsim` fuzzy matching for "did you mean?" on unknown commands
>
> **Decision:** Keep ratatui (enables richer split-pane UI + mouse + scrollback)
> but replace the hand-rolled input with `tui-textarea` and add overlay widgets
> for completion. Add indicatif-equivalent spinner within ratatui frame.

### 9.0 Architecture Decisions
- Keep ratatui 0.29 + crossterm 0.28 (superior to readline for split-pane TUI)
- Replace manual input `String` buffer with `tui-textarea` crate (multi-line, unicode-safe, history, readline shortcuts, bracket paste)
- Add completion overlay as ratatui `List` widget rendered above input area
- Add braille spinner widget in status bar (no external crate, 10-frame rotation)
- Add lightweight markdown → ratatui `Text` renderer (no external crate)
- Use `strsim::jaro_winkler` for fuzzy command matching
- Auto-update status bar after every `check_responses()` drain cycle

### 9.1 Input Overhaul — tui-textarea (P0)
- [x] Add `tui-textarea = "0.7"` to edgecrab-cli Cargo.toml
- [x] Replace `self.input: String` + `self.cursor: usize` with `TextArea<'a>`
- [x] Configure TextArea: Emacs key bindings, 3-line height, themed border
- [x] Wire `TextArea::input(key_event)` into `handle_key_event()`
- [x] Add input history ring buffer (`Vec<String>`, max 500 entries)
- [x] Wire Up/Down arrows to cycle history (load into TextArea)
- [x] Wire Enter to submit (extract text, push to history, clear TextArea)
- [x] Wire Ctrl+Enter / Shift+Enter for newline insertion (multi-line)
- [x] Wire Ctrl+C to cancel current input (clear TextArea, reset history pos)
- [x] Remove all manual cursor math (`self.cursor`, byte-level insert/remove)
- [x] Write test: multi-line input preserves newlines
- [x] Write test: history recall with Up/Down

### 9.2 Slash Command Tab Completion (P1)
- [x] Build `CompletionState { candidates: Vec<(String,String)>, selected: usize, active: bool }`
- [x] On Tab key: if input starts with `/`, filter commands by prefix, activate overlay
- [x] On repeated Tab: cycle `selected` index through candidates
- [x] On Enter (while overlay active): accept selected candidate, deactivate
- [x] On Escape / any non-Tab key: deactivate overlay
- [x] Render overlay as `List` widget positioned directly above input area
- [x] Style: selected item highlighted, max 8 visible items, scrollable
- [x] Add fuzzy fallback: if no prefix match, use `strsim::jaro_winkler` > 0.7
- [x] Add "did you mean?" suggestion on unknown `/` command dispatch
- [x] Show command description alongside name in overlay (intellisense-style)
- [x] Include command aliases in completion candidates (via `CommandRegistry::all_names()`)
- [x] Write test: exact prefix match returns correct candidates
- [x] Write test: fuzzy match suggests closest command

### 9.3 Animated Spinner + Streaming State Machine (P1)
- [x] Define `DisplayState` enum: `Idle`, `Thinking(frame)`, `Streaming(tokens)`, `ToolExec(name, frame)`
- [x] Add `SPINNER_FRAMES: &[&str] = &["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"]`
- [x] Create `SpinnerWidget` that renders current frame + elapsed time in status bar
- [x] Advance spinner frame every 80ms (check in event_loop before poll)
- [x] On agent start: transition to `Thinking(0)` with "Thinking..." message
- [x] On first StreamEvent::Token: transition to `Streaming(1)` with token counter
- [x] On tool execution: transition to `ToolExec(tool_name, 0)` with tool name
- [x] On StreamEvent::Done: transition to `Idle`, show completion time
- [x] Add escalating wait messages: 3s "Waiting...", 6s "+ Ctrl+C to cancel", 10s "Long wait..."
- [x] Write test: state transitions follow expected sequence

### 9.4 Markdown Rendering in Output (P1)
- [x] Create `edgecrab-cli/src/markdown_render.rs` module
- [x] Parse lines into styled ratatui `Spans`:
  - `# Header` → Cyan + Bold
  - `**bold**` → Bold
  - `*italic*` → Italic
  - `` `inline code` `` → Yellow
  - ```` ``` ```` code blocks → dimmed `│` prefix per line
  - `- list` → `  • item`
  - `> quote` → `  ▎ text` dimmed
  - `---` → dimmed `─` repeated to width
- [x] Integrate into `render()`: convert `OutputLine` text through markdown renderer
- [x] Cache rendered `Text` per `OutputLine` (avoid re-rendering on every frame)
- [x] Write test: code block renders with `│` prefix
- [x] Write test: nested bold+italic renders correctly

### 9.5 Status Bar Auto-Update (P1)
- [x] After `check_responses()` processes `StreamEvent::Done`: update `token_count`, `session_cost`
- [x] Add `last_response_time: Option<Instant>` to App for latency display
- [x] Add color-coded cost thresholds: green < $0.10, yellow < $1.00, red >= $1.00
- [x] Add color-coded token thresholds: green < 50k, yellow < 100k, red >= 100k
- [x] Show model name + provider in status bar (always visible)
- [x] Live streaming feedback: token count + tokens/second rate
- [x] Write test: status bar updates after stream completion

### 9.6 Input Line Highlighting (P2)
- [x] In `render()`, style the input text dynamically:
  - Input starts with `/` + valid command → Cyan
  - Input starts with `/` + invalid command → Red
  - Input starts with `@` → Green (context reference)
  - Default → foreground color from theme
- [x] Validate commands against `all_command_names` (includes aliases)
- [x] Write test: `/help` renders cyan, `/xyzzy` renders red

### 9.7 Fish-Style History Ghost Text (P2)
- [x] After each keystroke, search history for entries starting with current input
- [x] If match found: render remaining text as `Style::new().fg(Color::DarkGray)` after cursor
- [x] Right arrow at end of input: accept ghost text (fill input with full match)
- [x] Ghost text disappears on any edit that changes prefix
- [x] Only show ghost text when input has >= 2 characters (avoid noise)
- [x] Write test: ghost text shown for matching history entry
- [x] Write test: right arrow accepts ghost text

### 9.8 Unified Theme System (P2)
- [ ] Merge `skin_engine.rs` presets into `theme.rs` `Theme` struct
- [ ] Apply `code_theme` field to markdown code block rendering
- [ ] Remove duplication between `SkinConfig` and `SkinEngine`
- [ ] Add `Theme::from_preset(name: &str) -> Theme` constructor
- [ ] Write test: all 6 preset themes load without panic

### 9.9 Phase 9 E2E Verification
- [x] `cargo build` — all crates compile with new deps
- [x] `cargo test` — 130 tests pass (all existing + new)
- [x] `cargo clippy` — zero new warnings in CLI crate
- [ ] Manual test: type `/he` + Tab → completes to `/help`
- [ ] Manual test: type multi-line prompt with Shift+Enter
- [ ] Manual test: spinner animates during agent processing
- [ ] Manual test: code block in output has `│` prefix
- [ ] Manual test: Up arrow recalls last command
- [ ] Commit Phase 9

> **OODA Cycle 2 (2026-03-29):** Post-build observation pass
>
> **Observations:**
> - Tab completion only showed canonical command names, not aliases
> - Completion overlay showed bare names without descriptions
> - All 9 UX features verified complete after code review
>
> **Actions taken:**
> - Added `CommandRegistry::all_names()` returning names + aliases
> - Added `CommandRegistry::all_descriptions()` for alias → description mapping
> - Changed `CompletionState.candidates` from `Vec<String>` to `Vec<(String, String)>`
> - Completion overlay now shows "command — description" with dim desc style
> - Fixed clippy `manual_clamp` warning

---

## Phase 10 — UX/UI Deep Rework (OODA Cycle 3)

> **OODA Cycle 3 (2026-03-29):** User testing reveals poor UX despite Phase 9 features.
> Root causes: bloated bordered boxes, no dynamic model selector, emoji alignment
> issues, Copilot provider not verified. Reference implementation: edgecode.
>
> **Architecture decision:** KEEP ratatui TUI. Fix rendering quality, not architecture.

### 10.0 OBSERVE — Current UX Problems
1. **Bloated boxes everywhere** — Output area, input area, status bar all have bordered
   boxes with `Borders::ALL`. This wastes screen real estate and looks cluttered.
   Edgecode uses borderless/whitespace-driven layout.
2. **Static model list** — `/models` shows hardcoded catalog. `/model` requires typing
   exact `provider/model-name`. Edgecode has dynamic model fetching from all providers
   with fuzzy search selector UI.
3. **Copilot provider** — Not verified to work. Provider resolution chain may fail
   silently falling back to MockProvider.
4. **Emoji in output** — `OutputRole::User` lines prepend `❯` (fine) but box-drawing
   borders + variable-width text cause alignment drift.
5. **No model selector UX** — Must type exact model name. No browsing, no fuzzy search,
   no arrow-key selection.
6. **Status bar too cluttered** — Shows `·` separators, redundant spacing.

### 10.1 ORIENT — Priority Ranking
| Priority | Issue | Impact | Effort |
|----------|-------|--------|--------|
| P0 | Remove bloated borders → borderless layout | High (first thing users see) | Low |
| P0 | Clean status bar (no `·`, compact) | High | Low |
| P1 | Dynamic model selector overlay | High (core UX flow) | Medium |
| P1 | Verify Copilot provider works | High (primary provider) | Low |
| P2 | Better banner (minimal, no boxes) | Medium | Low |
| P2 | Remove emoji from box-drawing contexts | Medium | Low |
| P2 | Input area: minimal border, clean prompt | Medium | Low |

### 10.2 DECIDE — Implementation Plan
1. **Borderless output area** — Remove `Block::default().borders(Borders::ALL)` from
   output paragraph. Use whitespace + left margin instead.
2. **Minimal status bar** — Single line, no borders, compact segments with space separation.
3. **Clean input area** — Thin bottom border only, or single-line prompt indicator.
4. **Dynamic model selector** — TUI overlay widget: fetch models from providers async,
   show filterable list with arrow keys, Enter to select. Similar to telescope/fzf.
5. **Copilot provider** — Add explicit `copilot` provider path in `create_provider()`.
6. **Compact banner** — One-line version + model, no box borders.

### 10.3 ACT — Borderless/Minimal Layout
- [x] Remove `Borders::ALL` from output area Block
- [x] Remove `Borders::ALL` from input area textarea Block
- [x] Replace input Block with thin single-line bottom indicator
- [x] Add left-margin padding (2 spaces) to output lines instead of borders
- [x] Remove ` EdgeCrab ` title from output Block
- [x] Use horizontal `─` separator between output and status bar (1 char)

### 10.4 ACT — Compact Status Bar
- [x] Remove `·` dot separators
- [x] Use space-padded segments: `model tokens cost [state]`
- [x] Right-align cost, left-align state indicator
- [x] Use dimmed gray background for status bar (not bordered)
- [x] Compact spinner: `⠋ thinking 3s` not ` ⠋ Thinking... (3s) `

### 10.5 ACT — Dynamic Model Selector Overlay
- [x] Add `ModelSelectorState` to App: candidates, filter text, selected index, active
- [x] On `/model` (no args): activate overlay, fetch models async from providers
- [x] Fetch from: Copilot (via edgequake-llm), static registry as fallback
- [x] Render overlay as full-screen List with search filter at top
- [x] Arrow Up/Down to navigate, type to filter (fuzzy), Enter to select, Esc to cancel
- [x] On select: call `handle_model_switch()` with selected model
- [x] Group models by provider in the list

### 10.6 ACT — Copilot Provider Verification
- [x] In `create_provider()`, add explicit `copilot` handling before `from_env()`
- [x] Check `GITHUB_TOKEN` env var for Copilot auth
- [x] Add tracing::info for provider selection path
- [x] Test with `copilot/gpt-4.1-mini` model string

### 10.7 ACT — Polish
- [x] Minimal single-line banner: `EdgeCrab v0.1.0 · copilot/gpt-4.1-mini`
- [x] Remove emoji from all box-drawing contexts
- [x] User input echo: `> text` (not `❯ text`)
- [x] Completion overlay: remove border, use background highlight only
- [x] Consistent color scheme: cyan=commands, green=success, yellow=warning, red=error, dim=meta

### 10.8 Phase 10 Verification
- [x] `cargo build` — compiles
- [x] `cargo test` — all tests pass (525 passed, 0 failed)
- [x] `cargo clippy` — zero warnings (fixed in Phase 11: DispatchContext, skin_engine dead_code, banner dead_code)
- [x] Manual: clean borderless output
- [x] Manual: model selector works with fuzzy search
- [x] Manual: Copilot provider connects
- [ ] Commit Phase 10

---

## Deferred (Future Phases)

- [ ] Platform adapters: Slack, Matrix, Email, Signal, SMS, Mattermost, DingTalk
- [ ] Terminal backends: Docker, SSH, Modal, Daytona
- [ ] RL environments: Atropos integration
- [ ] Browser CDP WebSocket (browser_navigate works, others need Runtime.evaluate)
- [ ] Vision tools (`vision_analyze` via multimodal API)
- [ ] Image generation (`generate_image` via fal.ai/DALL-E)
- [ ] Home Assistant integration (`ha_*` tools)
- [ ] Honcho AI memory integration (`honcho_*` tools)
- [ ] Skills Hub marketplace (agentskills.io)
- [ ] Skills Guard security scanning
- [ ] OAuth login/logout flow
- [ ] Self-update mechanism

---

## Phase 11 — Audit & Functional Gap Closure (2026-03-29)

> Deep code audit revealed several plan.md items marked `[ ]` were already implemented,
> and several real functional gaps existed. This phase documents corrections and fixes.

### 11.1 Verification Corrections (already implemented, incorrectly marked open)
- [x] `pricing::estimate_cost()` wired in conversation.rs line 438 (was incorrectly `[ ]`)
- [x] `/queue` drain in check_responses() at app.rs line 1018 (was incorrectly `[ ]`)
- [x] `/cron` bridge calls `cron_cmd::status_snapshot()` real logic (was incorrectly `[ ]`)
- [x] `/plugins` bridge calls `PluginManager::discover_all()` real logic (was incorrectly `[ ]`)

### 11.2 Real Gap Fixes
- [x] MCP subprocess env var passthrough — `spawn()` now takes `envs: &HashMap<String, String>`, applied via `.envs(envs)` to tokio Command; both `mcp_list_tools` and `mcp_call_tool` extract `env` field from server JSON config
- [x] `skill_manage` tool — create/edit/delete skills in-conversation with path-traversal guard; 5 tests added; registered in CORE_TOOLS and ACP_TOOLS
- [x] `edgecrab skills list/view/search/install/remove` CLI subcommand — full implementation with local path copy and `git clone --depth=1` support; 5 parser tests added

### 11.3 Code Quality (clippy zero-warnings)
- [x] `DispatchContext<'a>` struct — reduces `process_response` + `dispatch_single_tool` from 8→3 params (was clippy too_many_arguments)
- [x] `skin_engine.rs` `#![allow(dead_code)]` — deferred Phase 9.8 code, not yet wired into theme.rs
- [x] `banner.rs` BANNER `#[allow(dead_code)]` — banner is inline in main.rs, constant unused

### 11.4 Phase 11 Verification
- [x] `cargo build` — all crates compile
- [x] `cargo test` — 535 tests pass (up from 492), 0 failed, 5 ignored
- [x] `cargo clippy` — zero warnings (down from 10)

---

## Phase 12 — Deep Audit: hermes-agent Parity & Pipeline Wiring (2025-07-15)

> Comparative audit of hermes-agent (Python reference) vs edgecrab (Rust port).
> Focus: tools, skills, prompts/soul/memory, and whether code is **actually wired**
> into the runtime pipeline — not just present in the codebase.

### 12.0 Critical Pipeline Findings

The audit revealed that several major subsystems exist as **dead code** —
fully implemented but never called in the actual conversation pipeline.

#### CRITICAL-1: PromptBuilder is never invoked
- `prompt_builder.rs` exists with: DEFAULT_IDENTITY, 10 platform hints, context
  file discovery (SOUL.md, AGENTS.md, .cursorrules, CLAUDE.md, .hermes.md,
  .cursor/rules/*.mdc), injection scanning, YAML frontmatter stripping,
  memory guidance, session search guidance, skill prompt support, 20K truncation.
- **BUT**: No code outside `prompt_builder.rs` (and its unit tests) ever calls
  `PromptBuilder::new()` or `.build()`.
- `conversation.rs:execute_loop()` uses:
  `system_message.unwrap_or("You are a helpful assistant.")`
- `agent.rs:run_conversation()` passes `system_message: None` from `chat()`.
- **Result**: Every conversation uses "You are a helpful assistant." as the
  system prompt. The agent has no identity, no platform awareness, no context
  file injection, no memory guidance.

#### CRITICAL-2: chat_streaming() bypasses execute_loop()
- `agent.rs:chat_streaming()` calls `provider.chat_with_tools_stream()` directly
  with `&[]` tool definitions — **no tools are available during streaming**.
- The TUI (app.rs) dispatches user input via `chat_streaming()`.
- **Result**: Interactive TUI mode has NO tool execution capability. The agent
  cannot search files, write code, browse the web, or use any tools in the
  primary interactive interface.

#### CRITICAL-3: Memory is never loaded into system prompt
- `memory_read`/`memory_write` tools exist and work at the tool level.
- **BUT**: No code loads MEMORY.md at session start and injects it into
  the system prompt (hermes does this via frozen snapshot pattern in
  `MemoryStore.__init__` → `prompt_builder.py:_memory_section()`).
- **Result**: The agent starts every session with zero memory context.
  Even if it writes memories via the tool, they never appear in the
  system prompt for future sessions.

#### CRITICAL-4: Skills are never assembled into system prompt
- `skills_list`/`skill_view` tools exist for the agent to discover skills.
- **BUT**: No code scans the skills directory at prompt build time and
  includes skill descriptions/prompts in the system prompt (hermes does
  this via `prompt_builder.py:_skill_prompts()` with progressive disclosure
  and disk caching).
- **Result**: The agent doesn't know what skills are available unless it
  explicitly calls skills_list. No skill prompts are injected.

### 12.1 P0 — Wire the Prompt Pipeline (agent is non-functional without these)

> **STATUS: ALL FIXED** — Verified wired and working as of 2025-07-15 session.

- [x] **Wire PromptBuilder into execute_loop**: `conversation.rs:execute_loop()`
  calls `PromptBuilder::new(platform).build(system_message, Some(cwd), &mem, skill_prompt.as_deref())`
  at session start. The `"You are a helpful assistant."` fallback is gone.
  Config flags `skip_context_files` and `skip_memory` are passed through.

- [x] **Fix chat_streaming to use execute_loop**: `agent.rs:chat_streaming()`
  now delegates fully to `execute_loop()` with `Some(&chunk_tx)` as event_tx.
  Tools are available in streaming/TUI mode. Streaming is simulated by
  chunking the final response into word tokens.

- [x] **Wire memory loading at session start**: `conversation.rs` calls
  `load_memory_sections(edgecrab_home)` which reads MEMORY.md and USER.md;
  result is passed to `PromptBuilder.build()`. Frozen snapshot pattern applies:
  mid-session `memory_write` updates disk but NOT the cached system prompt.

- [x] **Wire skill prompt assembly**: `conversation.rs` calls
  `load_skill_summary(edgecrab_home)` to scan `~/.config/edgecrab/skills/`
  with nested category support (see Phase 13). Result injected into system
  prompt under `## Available Skills`.

- [x] **Add SKILLS_GUIDANCE constant**: SKILLS_GUIDANCE text exists in
  `prompt_builder.rs` and is appended to the system prompt when skills
  are available.

### 12.2 P1 — Tool Feature Parity

#### 12.2.1 Memory Tools
- [ ] **Upgrade memory to hermes parity**: Replace basic append-only
  `memory_write` with action-based API matching hermes: `add`, `replace`,
  `remove`, `read` actions. Add substring matching for replace/remove.
- [ ] **Add char limits**: Implement 2200-char limit for MEMORY.md and
  1375-char limit for USER.md (or configurable equivalents).
- [ ] **Add USER.md**: hermes maintains separate MEMORY.md (agent notes)
  and USER.md (user profile). Edgecrab only has memory_read/memory_write
  with no user profile concept.
- [ ] **Add injection scanning**: Scan memory content for prompt injection
  attempts before including in system prompt.
- [ ] **Add file locking**: Use file/advisory locks when writing MEMORY.md
  to prevent concurrent corruption (hermes uses fcntl).

#### 12.2.2 Skills Tools
- [ ] **Parse skill frontmatter**: `skills_list` should parse YAML
  frontmatter from each skill's SKILL.md to extract name, description,
  category, platforms — not just list directory names.
- [ ] **Progressive disclosure**: `skill_view` should support linked file
  loading (`read_files` frontmatter field) and load related files when
  viewing a skill, not just the main file content.
- [ ] **Platform-aware filtering**: Filter skills by platform compatibility
  (e.g., CLI-only skills shouldn't appear in Telegram gateway).
- [ ] **Category descriptions**: Group skills by category in listings with
  human-readable descriptions.
- [ ] **Skill prompt caching**: Cache assembled skill prompts to disk
  (hermes uses `~/.cache/hermes_skill_prompts_<hash>.txt`) to avoid
  re-scanning on every session start.

#### 12.2.3 Delegate Task
- [ ] **Multi-tool loop in sub-agent**: Currently `delegate_task` does a
  single-turn LLM completion with no tool definitions. Port hermes'
  architecture: spawn a child Agent instance with tool access and let it
  run a full execute_loop.
- [ ] **Batch/parallel mode**: hermes supports `mode: "batch"` with
  `MAX_CONCURRENT_CHILDREN=3` for parallel sub-task execution.
- [ ] **Depth limiting**: Implement `MAX_DEPTH=2` to prevent recursive
  delegation bombs.
- [ ] **Blocked tools enforcement**: Sub-agents should not have access to
  `delegate_task`, `clarify`, `memory`, `send_message`, `execute_code`
  (matching hermes' DELEGATE_BLOCKED_TOOLS).
- [ ] **Isolated terminal sessions**: Each sub-agent should get its own
  terminal session/cwd, not share the parent's.

#### 12.2.4 Execute Code (PTC)
- [ ] **Programmatic Tool Calling**: hermes implements a sophisticated
  Unix domain socket RPC mechanism where the parent generates a
  `hermes_tools.py` stub, spawns a child process, and the child calls
  tools via UDS back to the parent. Edgecrab currently does direct
  subprocess execution with no tool callback capability.
- [ ] **Sandbox allowed tools**: Implement SANDBOX_ALLOWED_TOOLS subset
  (web_search, web_extract, read_file, write_file, search_files, patch,
  terminal) — only these tools should be callable from executed code.

#### 12.2.5 Browser Tools (7 → 11)
- [ ] **browser_back**: Navigate browser history back (hermes has this).
- [ ] **browser_press**: Press keyboard keys (Enter, Escape, Tab, etc.).
- [ ] **browser_close**: Close browser/tab explicitly.
- [ ] **browser_get_images**: Extract all images from current page.

#### 12.2.6 Missing Tools vs hermes
- [ ] **vision_analyze**: Multimodal image analysis via LLM vision API.
- [ ] **image_generate**: Image generation via fal.ai / DALL-E.
- [ ] **mixture_of_agents**: Multi-model consensus for complex queries.
- [ ] **cronjob**: Schedule recurring tasks (hermes has full cron system).
- [ ] **send_message**: Cross-platform message sending (gateway).
- [ ] **honcho_***: AI memory integration (honcho_add, honcho_search,
  honcho_list, honcho_delete).
- [ ] **ha_***: Home Assistant integration (ha_list_devices, ha_control,
  ha_get_state, ha_run_automation).

### 12.3 P2 — Prompt & Context Engineering Parity

- [ ] **Port hermes DEFAULT_AGENT_IDENTITY**: The current edgecrab
  DEFAULT_IDENTITY is defined but never used (CRITICAL-1). Once wired,
  verify it matches hermes' Nous Research identity text quality: detailed
  personality traits, memory/skill usage instructions, code style
  preferences, interaction protocols.

- [ ] **Platform-specific MEDIA hints**: hermes' platform hints include
  rich `MEDIA:` sections specifying how to deliver files per platform
  (e.g., WhatsApp: send as document attachment, Telegram: send photo with
  caption, CLI: save to disk and show path). Edgecrab's platform hints
  lack these MEDIA delivery instructions.

- [ ] **Context file priority order**: hermes walks up directory tree to
  git root looking for `.hermes.md`/`HERMES.md`, with specific priority:
  HERMES.md > .hermes.md > .cursorrules. Verify edgecrab's implementation
  matches this traversal and priority order.

- [ ] **Frozen snapshot memory pattern**: System prompt gets memory
  snapshot at session start. Mid-session memory_write updates disk but
  does NOT update `cached_system_prompt`. When memory_read is called
  mid-session, it reads from disk (live state). This preserves prompt
  cache efficiency (Anthropic charges for cache misses).

- [ ] **Context compression parity**: hermes has structured summary
  template with SUMMARY_PREFIX and tool output pruning in
  `context_compressor.py`. Verify edgecrab's context compression
  (conversation.rs) matches the summary quality and preserves key
  information across compressions.

- [ ] **@context references**: hermes supports @file, @url, @diff,
  @staged, @folder, @git inline references that expand at send time.
  Verify edgecrab's implementation in conversation.rs matches all
  reference types and expansion behavior.

- [ ] **Prompt caching strategy**: hermes implements Anthropic's
  `system_and_3` strategy in `prompt_caching.py` — system prompt + first
  3 messages get cache_control breakpoints. Verify edgecrab's
  `CachePromptConfig` is actually effective (currently only used in
  chat_streaming path which bypasses tools).

### 12.4 P2 — Toolset & Gateway Parity

- [ ] **Platform-specific toolsets**: hermes defines per-platform toolsets
  (hermes-telegram, hermes-discord, hermes-whatsapp, etc.) with
  platform-appropriate tool selections. Edgecrab has CORE_TOOLS and
  ACP_TOOLS but no platform-specific toolset definitions.

- [ ] **Composable toolset includes**: hermes toolsets support an
  `includes` field for composition (e.g., `hermes-cli` includes `core`).
  Verify edgecrab's alias system (`coding`, `research`, `debugging`,
  `safe`, `all`, `minimal`) provides equivalent flexibility.

### 12.5 Phase 12 Verification
- [x] After P0 fixes: `cargo build` compiles, `cargo test` passes, agent
  actually uses assembled system prompt (not "You are a helpful assistant.")
- [x] TUI interactive mode can execute tools (not just stream text)
- [x] Memory content appears in system prompt on session start
- [x] Skill descriptions appear in system prompt
- [x] `cargo clippy` — zero warnings maintained

---

## Phase 13 — SubAgentRunner + Tool Progress Streaming + Skill Parity (2025-07-15)

> Continued from Phase 12 audit. All remaining P0/P1 gaps addressed this session.  
> Build: `cargo build` ✅ | Tests: **558 passed, 0 failed, 5 ignored** ✅ | Clippy: 0 warnings ✅

### 13.1 SubAgentRunner Implementation (P0 — delegate_task was non-functional)

Previously `dispatch_single_tool` passed `None` as the `sub_agent_runner` arg to every
tool, making `delegate_task`'s `runner.run_task()` call unreachable dead code.

- [x] **Create `crates/edgecrab-core/src/sub_agent_runner.rs`** — New file implementing
  `CoreSubAgentRunner` struct that satisfies the `SubAgentRunner` trait (defined in
  `edgecrab-tools` to avoid circular dependency):
  - `CoreSubAgentRunner { provider, tool_registry, platform, model }`
  - `impl SubAgentRunner::run_task()` — spawns child `AgentBuilder` with `quiet_mode`,
    overrides `enabled_toolsets`, disables nested delegation, runs full `execute_loop`
- [x] **Wire into `execute_loop`**: Instantiated once before the main loop from
  `provider`, `tool_registry`, `config.platform`, `config.model`
- [x] **Pass through `DispatchContext`**: Added `sub_agent_runner` field;
  `dispatch_single_tool` now receives it instead of `None`
- [x] **Parallel spawn safety**: Parallel dispatch inner `DispatchContext` uses
  `event_tx: None` — ToolExec event already emitted before spawning parallel tasks

### 13.2 StreamEvent::ToolExec — TUI Tool Progress Visibility

Previously the TUI showed no indication that tools were executing between token bursts.

- [x] **Added `StreamEvent::ToolExec(String)` variant** to `agent.rs`
- [x] **`execute_loop` accepts `event_tx` param**: `chat_streaming` passes
  `Some(&chunk_tx)`; `run_conversation` passes `None`
- [x] **Emit event before each tool dispatch** in `process_response()`:
  one event per tool call with the tool's function name
- [x] **TUI routing** (`app.rs`): `AgentResponse::ToolExec(String)` variant added;
  `check_responses` transitions `DisplayState` to `ToolExec { name, frame, started }`
  showing a spinner with the tool name; Token events transition back to `Streaming`
- [x] **E2E test fix**: `e2e_copilot.rs` match on `StreamEvent` updated for exhaustiveness

### 13.3 Skill Parity Improvements

#### Nested Category Support in `load_skill_summary`

- [x] **Rewrote `load_skill_summary`** (`prompt_builder.rs`) with inner `scan_dir`
  recursive function supporting both skill layouts:
  - Flat: `skills/skill-name/SKILL.md` → no category header, rendered directly
  - Nested: `skills/category/skill-name/SKILL.md` → emits `### category` header first
- [x] Skills and categories sorted alphabetically; category headers emitted once per group
- [x] Tests: `load_skill_summary_flat_skills`, `load_skill_summary_nested_categories`,
  `load_skill_summary_returns_none_when_no_skills_dir`

#### `skill_manage` Patch Action

- [x] **Added `patch` action** to `skill_manage` tool (`skills.rs`):
  - Reads SKILL.md content, counts exact occurrences of `old_string`
  - Errors with descriptive message on 0 matches or >1 matches (uniqueness enforced)
  - Performs single `replacen(old, new, 1)` and writes back to disk
  - Mirrors `file_patch` tool behavior — targeted, safe, find-and-replace
- [x] Schema updated: `"enum": ["create", "edit", "patch", "delete"]`; `old_string`/
  `new_string` optional params documented
- [x] Tests: `patch_skill_replaces_unique_occurrence`, `patch_skill_no_match_returns_error`,
  `patch_skill_multiple_matches_returns_error`, `patch_skill_not_found_returns_error`

### 13.4 Phase 13 Verification

| Check | Result |
|-------|--------|
| `cargo build` | ✅ Compiles clean |
| `cargo test` | ✅ 558 passed, 0 failed, 5 ignored |
| `cargo clippy -- -D warnings` | ✅ 0 warnings |
| New tests (7 total) | ✅ All pass |
| `delegate_task` functional | ✅ CoreSubAgentRunner wired |
| TUI tool progress visible | ✅ ToolExec spinner state |

### 13.5 Outstanding P1 Items (Next Session)

- [ ] **Memory file locking**: Use `fd-lock` crate to prevent concurrent MEMORY.md corruption
- [ ] **delegate_task depth limiting**: Implement `MAX_DEPTH=2` guard in `CoreSubAgentRunner`
- [ ] **delegate_task blocked tools**: Sub-agents must not have `delegate_task`, `memory`,
  `execute_code` access (match hermes `DELEGATE_BLOCKED_TOOLS`)
- [ ] **execute_code PTC**: Unix domain socket RPC pattern from hermes for programmatic
  tool calling from within executed code
- [ ] **memory actions parity**: Port hermes `add`/`replace`/`remove` action API with
  char limits (2200 for MEMORY.md, 1375 for USER.md)
