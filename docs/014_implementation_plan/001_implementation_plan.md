# 014.001 — Implementation Plan

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 015 Roadblocks](../015_roadblocks/001_roadblocks.md) | [→ 013 Library Selection](../013_library_selection/001_library_selection.md)

---

## Phase Overview

```
Phase 0  Foundation         ████░░░░░░░░░░░░░░░░  (Weeks 1-3)
Phase 1  Core Agent         ████████░░░░░░░░░░░░  (Weeks 4-8)
Phase 2  CLI & Tools        ████████████░░░░░░░░  (Weeks 9-14)
Phase 3  Gateway            ████████████████░░░░  (Weeks 15-19)
Phase 4  Advanced Features  ████████████████████  (Weeks 20-24)
Phase 5  Polish & Release   ████████████████████  (Weeks 25-28)
```

---

## Phase 0 — Foundation (Weeks 1-3)

### 0.1 Workspace Setup
- [ ] Initialize Cargo workspace with 9 crates (see [002.002](../002_architecture/002_crate_dependency_graph.md))
- [ ] Set up CI pipeline: `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt`
- [ ] Configure `cargo-deny` for license/advisory checks
- [ ] Configure `cargo-audit` in CI
- [ ] Add `#[deny(clippy::unwrap_used)]` to lib crates
- [ ] Set up feature gate matrix in CI (test all combinations)
- [ ] Create workspace-level Cargo.toml with shared dependencies

### 0.2 Types Crate (`edgecrab-types`)
- [ ] Define `Message`, `Role`, `Content`, `ContentPart` (see [010](../010_data_models/001_data_models.md))
- [ ] Define `ToolCall`, `FunctionCall`
- [ ] Define `Usage`, `Cost`
- [ ] Define `ApiMode` enum
- [ ] Define `Platform` enum
- [ ] Define constants (`DEFAULT_MODEL`, `OPENROUTER_BASE_URL`, etc.)
- [ ] Write unit tests for serialization round-trips
- [ ] Write proptest for Message fuzzing

### 0.3 Error Handling (`edgecrab-types`)
- [ ] Define `AgentError` enum (see [002.004](../002_architecture/004_error_handling.md))
- [ ] Define `ToolError` enum with retry strategies
- [ ] Define `GatewayError`, `ConfigError`, `SecurityError`
- [ ] Implement `Display`, `Error` via `thiserror`
- [ ] Write error→JSON conversion for LLM feedback

### 0.4 edgequake-llm Integration Proof-of-Concept
- [ ] Add `edgequake-llm` dependency
- [ ] Write integration test: `ProviderFactory::from_env()` → chat completion
- [ ] Write integration test: streaming response consumption
- [ ] Write integration test: tool calling round-trip
- [ ] Verify all 13 native providers work through the trait interface
- [ ] Document any API gaps or issues to upstream

---

## Phase 1 — Core Agent (Weeks 4-8)

### 1.1 State Management (`edgecrab-state`)
- [ ] Implement `SessionDb` with rusqlite (see [009](../009_config_state/001_config_state.md))
- [ ] Implement WAL mode, `PRAGMA synchronous=NORMAL`
- [ ] Write contention tuning: `BEGIN IMMEDIATE`, random jitter retry (20-150ms), `WRITE_MAX_RETRIES=15`
- [ ] `PRAGMA wal_checkpoint(PASSIVE)` every 50 writes
- [ ] Session CRUD operations
- [ ] Message CRUD operations
- [ ] FTS5 virtual table for session search (see [007](../007_memory_skills/001_memory_skills.md))
- [ ] FTS5 sync triggers (INSERT/DELETE/UPDATE cascade to FTS index)
- [ ] Session search queries with BM25 ranking
- [ ] Write tests: concurrent access, WAL correctness, write contention under load
- [ ] Migration support (schema versioning, `state.db` as canonical name)

### 1.2 Config System (`edgecrab-core`)
- [ ] Implement `AppConfig` with serde_yml deserialization
- [ ] Implement `edgecrab_home()` directory resolution
- [ ] CLI arg merging over config file
- [ ] Environment variable override support (`${VAR}` and `${VAR:-default}`)
- [ ] Config validation with helpful error messages
- [ ] Real-time config reload via `notify` file watcher
- [ ] Custom models registry (`custom_models.yaml`)
- [ ] Project-local config (`.edgecrab.md` / `.hermes.md`)
- [ ] Write tests: default config, partial config, env overrides, reload

### 1.3 Security Crate (`edgecrab-security`)
- [ ] Path traversal prevention (`resolve_safe_path`)
- [ ] URL safety (SSRF prevention)
- [ ] Command injection scanning (~30 destructive patterns across 8 categories)
- [ ] Command normalization: ANSI strip + Unicode NFKC (`unicode-normalization`) + null byte removal
- [ ] Environment variable passthrough allowlist
- [ ] Output redaction (API keys, tokens)
- [ ] Approval policy engine
- [ ] Memory content scanning (injection/exfiltration detection)
- [ ] Provider-scoped credential isolation (`secrecy` crate)
- [ ] @ context reference security (blocked sensitive paths)
- [ ] MCP server sandboxing/permissions
- [ ] Skills guard (trust levels, scan verdict, install policy)
- [ ] Write tests for each security check with bypass attempts

### 1.4 Agent Struct & Builder (`edgecrab-core`)
- [ ] Implement `Agent` struct (see [003.001](../003_agent_core/001_agent_struct.md))
- [ ] Implement `AgentBuilder` with type-safe builder pattern
- [ ] Implement `IterationBudget` with `AtomicU32`
- [ ] Implement `SessionState` with `RwLock`
- [ ] Implement `CancellationToken` interrupt handling
- [ ] Wire edgequake-llm provider into Agent

### 1.5 Prompt Builder (`edgecrab-core`)
- [ ] Implement `PromptBuilder` (see [003.003](../003_agent_core/003_prompt_builder.md))
- [ ] Context file discovery (SOUL.md, AGENTS.md, .cursorrules)
- [ ] Platform-specific hints
- [ ] Memory section injection
- [ ] Skills section injection
- [ ] Anthropic prompt caching support
- [ ] Write tests: prompt assembly, cache stability

### 1.6 Conversation Loop (`edgecrab-core`)
- [ ] Implement `run_conversation()` (see [003.002](../003_agent_core/002_conversation_loop.md))
- [ ] API call with retry + exponential backoff
- [ ] Fallback model activation
- [ ] Tool call extraction and response processing
- [ ] Reasoning/thinking block extraction
- [ ] @ context reference expansion (parse, security check, inject)
- [ ] Usage tracking + cost calculation (billing route resolution)
- [ ] Interrupt handling (cooperative cancellation)
- [ ] Streaming response consumption
- [ ] Length truncation + continuation logic
- [ ] Background memory/skill review nudge
- [ ] Queue processing (`/queue` command)
- [ ] Anthropic prompt caching header injection
- [ ] Write integration tests: full conversation loop with mock provider

### 1.7 Context Compression (`edgecrab-core`)
- [ ] Implement `ContextCompressor` (see [003.004](../003_agent_core/004_context_compression.md))
- [ ] Token estimation (rough + actual from API)
- [ ] Chunked summarization via cheaper model
- [ ] Preflight compression before main loop
- [ ] Context probing (model context length discovery)
- [ ] Write tests: compression ratio, protected message preservation

### 1.8 Model Routing (`edgecrab-core`)
- [ ] Implement `ModelRouter` (see [003.005](../003_agent_core/005_smart_model_routing.md))
- [ ] Model metadata fetching/caching
- [ ] Provider fallback chain
- [ ] API mode detection (unified via edgequake-llm)

---

## Phase 2 — CLI & Tools (Weeks 9-14)

### 2.1 Tool Registry (`edgecrab-tools`)
- [ ] Implement `ToolHandler` trait (see [004.001](../004_tools_system/001_tool_registry.md))
- [ ] Implement `ToolRegistry` with `inventory` crate
- [ ] Implement `ToolContext`
- [ ] Parallel vs sequential dispatch logic
- [ ] Tool schema generation (OpenAI format)
- [ ] Fuzzy tool name matching for typos
- [ ] Write tests: registration, dispatch, parallel safety

### 2.2 Core Tools
- [ ] `read_file` — with line range support
- [ ] `write_file` — atomic write with backup
- [ ] `patch` — unified diff application (using `similar` crate)
- [ ] `search_files` — ripgrep-style recursive search
- [ ] `terminal` — local command execution via `tokio::process`
- [ ] `process` — long-running process management (list/poll/log/wait/kill/write/submit actions)
- [ ] `vision_analyze` — multimodal image analysis
- [ ] `clarify` — user clarification (callback-based)
- [ ] `todo` — task checklist management
- [ ] Write tests for each tool (happy path + error cases)

### 2.3 Web & Browser Tools
- [ ] `web_search` — search engine API integration
- [ ] `web_extract` — page content extraction (readability)
- [ ] Browser automation (feature-gated, `chromiumoxide` 0.9 — async, tokio-native)

### 2.4 Memory & Skills Tools
- [ ] `memory_read` / `memory_write` — `memories/` directory (not legacy MEMORY.md)
- [ ] `skill_manage` — CRUD for skills
- [ ] `skill_view` / `skills_list` — read-only skill access
- [ ] Memory nudge tracker
- [ ] Skill nudge tracker
- [ ] Skills discovery from filesystem
- [ ] Claude Skills compatibility: YAML frontmatter parser (agentskills.io + Claude Skills fields)
- [ ] `$ARGUMENTS` / `$ARGUMENTS[N]` substitution engine
- [ ] `!\`command\`` shell injection with trust-level gating
- [ ] Script skill runtimes: Python, Node.js, Shell (sandboxed subprocess execution)
- [ ] Skill execution modes: prompt injection, fork context, script execution

### 2.5 Session & Search Tools
- [ ] `session_search` — FTS5 full-text search
- [ ] Session database integration

### 2.6 Advanced Tools
- [ ] `execute_code` — sandboxed code execution
- [ ] `delegate_task` — subagent spawning
- [ ] `generate_image` — image generation API
- [ ] `tts_speak` — text-to-speech (feature-gated)
- [ ] `send_message` — cross-platform messaging
- [ ] `mixture_of_agents` — multi-model query
- [ ] `cronjob_*` — scheduled tasks
- [ ] Honcho tools (`honcho_context`, `honcho_profile`, `honcho_search`, `honcho_conclude`)
- [ ] Home Assistant tools (feature-gated)
- [ ] MCP tool proxy (dynamic registration)

### 2.7 Toolset Composition
- [ ] Toolset aliases resolution
- [ ] Config-driven toolset filtering
- [ ] Platform-specific defaults
- [ ] Toolset distributions for RL/batch

### 2.8 CLI Application (`edgecrab-cli`)
- [ ] clap argument parsing with derive macros
- [ ] ratatui TUI layout (input, output, status, tool progress)
- [ ] Event loop (crossterm events + agent async events)
- [ ] Slash command registry (30+ commands)
- [ ] Skin engine (YAML-driven themes)
- [ ] Streaming token display
- [ ] Session management UI
- [ ] Model selection UI
- [ ] ASCII banner
- [ ] Clipboard integration (`arboard`)
- [ ] Auth flow (API key setup)
- [ ] Doctor command (dependency check)
- [ ] Write integration tests: CLI e2e with mock provider

---

## Phase 3 — Gateway (Weeks 15-19)

### 3.1 Gateway Core (`edgecrab-gateway`)
- [ ] Implement `PlatformAdapter` trait (see [006](../006_gateway/001_gateway_architecture.md))
- [ ] Implement `SessionManager` with `DashMap`
- [ ] Implement `DeliveryRouter` with message splitting
- [ ] Implement `GatewayHook` trait for lifecycle events
- [ ] Implement health check endpoint
- [ ] Implement gateway agent cache (preserve prompt cache per session)
- [ ] Implement auto-reconnect with exponential backoff
- [ ] Session expiry + cleanup task
- [ ] Session worktree isolation
- [ ] Write tests: session lifecycle, message delivery, reconnect

### 3.2 Platform Adapters (feature-gated)
- [ ] Telegram adapter (`teloxide` 0.17)
- [ ] Discord adapter (`serenity` 0.12.5)
- [ ] Slack adapter (`slack-morphism`)
- [ ] WhatsApp adapter (Cloud API, custom HTTP)
- [ ] Matrix adapter (`matrix-sdk`)
- [ ] Email adapter (`lettre` + `imap`)
- [ ] Webhook inbound/outbound (`axum`)
- [ ] REST API (`axum`)
- [ ] Write per-platform integration tests

### 3.3 Gateway Extensions
- [ ] Mirror hook (message duplication)
- [ ] Channel directory
- [ ] Sticker cache (Telegram)
- [ ] Stream consumer (SSE/WebSocket)
- [ ] Status endpoint

---

## Phase 4 — Advanced Features (Weeks 20-24)

### 4.1 Terminal Backends
- [ ] Docker backend (`bollard` 0.20.2)
- [ ] SSH backend (`russh`)
- [ ] Modal/Daytona/Singularity backends (HTTP)
- [ ] Backend trait unification
- [ ] Write tests per backend

### 4.2 Environments & RL
- [ ] Base environment trait (5 required methods: `setup`, `collect_trajectories`, `evaluate`, `scoring`, `cleanup`)
- [ ] Atropos integration: Phase 1 (OpenAI-compat server for SFT/eval), Phase 2 (VLLM ManagedServer for full RL)
- [ ] SWE-Bench environment
- [ ] RL training environment (HermesAgentEnvConfig with 20+ fields)
- [ ] Agentic OPD environment (distill_token_ids + distill_logprobs, per-token advantages)
- [ ] Web research environment (FRAMES benchmark + 4-component reward: LLM judge, keyword, length, heuristic)
- [ ] Terminal test environment
- [ ] Benchmark environments: tblite, terminalbench_2, yc_bench
- [ ] PersistentShellMixin (file-based IPC: stdin→file, stdout→file, session management, exponential polling)
- [ ] Tool context (thread-bridge for RL async→sync tool execution)
- [ ] Agent loop for episodes (AgentResult with tool_errors, reasoning_per_turn, tool pool resize)
- [ ] Reasoning extraction from 3 formats: `<think>`, `<scratchpad>`, `<|thinking|>`
- [ ] 11 tool call parsers: hermes, mistral, llama, qwen, qwen3_coder, deepseek_v3, deepseek_v3_1, glm45, glm47, kimi_k2, longcat
- [ ] Trajectory save/load (JSONL)
- [ ] Trajectory compression (batch)
- [ ] WandB logging integration

### 4.3 ACP Server (`edgecrab-acp`)
- [ ] Agent Communication Protocol adapter
- [ ] Permission model
- [ ] Tool exposure over ACP
- [ ] Session binding

### 4.4 Plugin System
- [ ] Dynamic library loading (`libloading`)
- [ ] Plugin discovery (project, user, global)
- [ ] Plugin sandboxing
- [ ] Plugin API versioning

### 4.5 Migration Tool (`edgecrab-migrate`)
- [ ] hermes-agent → EdgeCrab migration (see [012](../012_migration/001_migration_guide.md))
- [ ] OpenClaw → EdgeCrab migration
- [ ] Config format conversion
- [ ] Session DB schema migration
- [ ] API key compat layer
- [ ] Write migration tests

### 4.6 Checkpoint Manager
- [ ] Working directory snapshots
- [ ] Rollback support
- [ ] Snapshot pruning (max_snapshots)

---

## Phase 5 — Polish & Release (Weeks 25-28)

### 5.1 Testing & Quality
- [ ] Integration test suite: full agent conversation with real LLM
- [ ] Property-based testing (`proptest`) for message/config parsing
- [ ] Snapshot testing (`insta`) for prompt builder output
- [ ] Benchmark suite (`criterion`) for hot paths
- [ ] Fuzzing: JSON parsing, message deserialization
- [ ] Cross-compilation testing (Linux, macOS, Windows)
- [ ] `cargo-deny` audit: all licenses compatible
- [ ] `cargo-audit`: zero known vulnerabilities

### 5.2 Documentation
- [ ] README.md with quick start
- [ ] `--help` text for all CLI commands
- [ ] API docs (`cargo doc`)
- [ ] Migration guide (user-facing)
- [ ] Architecture decision records (ADR)

### 5.3 Packaging
- [ ] GitHub Actions: build + test + release
- [ ] Release binaries: Linux (x86_64, aarch64), macOS (universal), Windows
- [ ] Homebrew formula
- [ ] Nix flake
- [ ] Docker image (multi-arch)
- [ ] cargo install support (crates.io publish)

### 5.4 Performance Validation
- [ ] Startup time < 50ms
- [ ] Memory baseline < 10MB
- [ ] 10,000+ concurrent gateway sessions
- [ ] Tool dispatch < 1ms overhead
- [ ] Context compression 5x faster than Python

---

## Dependency Graph (Phase Ordering)

```
Phase 0 (types, errors, edgequake-llm PoC)
   │
   ▼
Phase 1 (state, config, security, agent core)
   │
   ├──────────────────┐
   ▼                  ▼
Phase 2 (tools, CLI)  Phase 3 (gateway)
   │                  │
   ├──────────────────┘
   ▼
Phase 4 (environments, ACP, plugins, migration)
   │
   ▼
Phase 5 (testing, docs, packaging, perf)
```

Notes:
- Phase 2 and Phase 3 can run **in parallel** by different team members
- Phase 4 depends on Phase 2 (tools) and Phase 3 (gateway) being at least partially complete
- Phase 5 runs continuously but is finalized last

---

## Milestone Checkpoints

| Milestone | Criteria | Phase |
|-----------|----------|-------|
| M0: Compiles | All 9 crates compile, CI green | 0 |
| M1: Chat works | `edgecrab "hello"` returns LLM response | 1 |
| M2: Tools work | File read/write + terminal execution | 2 |
| M3: TUI works | Full ratatui CLI with streaming | 2 |
| M4: Telegram works | Bot responds to messages | 3 |
| M5: Migration works | `edgecrab migrate --from hermes` | 4 |
| M6: Parity | All hermes-agent features functional | 4 |
| M7: Release | Binary published, docs complete | 5 |
