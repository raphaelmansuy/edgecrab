# 001 — Project Summary

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 002.001 Architecture](../002_architecture/001_system_architecture.md) | [→ 013.001 Library Selection](../013_library_selection/001_library_selection.md)
> **Version**: EdgeCrab v0.1.0 · Source verified (code-is-law)
> **Inspiration**: [OpenClaw](https://github.com/openai/openai-claw) agent design + [Nous Hermes](https://nousresearch.com) self-improving agent architecture

## 1. What Is EdgeCrab

EdgeCrab is the **best personal AI agent** — a Rust-native, self-improving agent built from the ground up,
inspired by the best ideas from OpenClaw's tool-centric architecture and Nous Hermes's skill-learning
design, then realised in idiomatic Rust for uncompromising performance, safety, and portability.

It ships as a **single static binary** (~15-25MB) with zero runtime dependencies — no Python, no
pip, no virtualenv, no Node.js, no npm. Download, run, done.

**Language**: Rust (2024 edition, MSRV 1.85.0)
**LLM Backend**: `edgequake-llm` v0.3.0+ (13 native providers + embedding, streaming, tool calling, caching, rate limiting, cost tracking, OTel observability, BM25/RRF reranking)
**Python Bridge**: `edgequake-litellm` — drop-in LiteLLM replacement backed by edgequake-llm Rust core (abi3-py39 wheels for all platforms)
**License**: Apache-2.0

## 2. Why Rust Over Python

| Dimension | hermes-agent (Python) | EdgeCrab (Rust) |
|-----------|-----------------------|-----------------|
| **Startup time** | ~2-4s (import chain, venv) | ~50ms (static binary) |
| **Memory idle** | ~80-150MB (Python VM + deps) | ~8-15MB (static alloc) |
| **Concurrency** | GIL-limited, async-bridge hacks | True parallelism (Tokio, rayon) |
| **Tool parallelism** | ThreadPoolExecutor + _run_async() | Native async tasks, zero-cost |
| **Event loop bugs** | "Event loop is closed" (5+ workarounds in code) | No event-loop lifecycle issues |
| **Type safety** | Runtime AttributeError, KeyError | Compile-time guarantees |
| **Distribution** | pip install + venv + python + node | Single binary (curl + chmod) |
| **Memory safety** | GC pauses, reference cycles | Ownership model, zero GC |
| **Binary size** | ~500MB+ (venv with all deps) | ~15-25MB (static, stripped) |
| **Cross-compilation** | Complex (manylinux, wheels) | `cross` for any target |
| **Dependency supply chain** | 30+ PyPI packages | ~15-20 crates, auditable |

### Rust Advantages Not Possible in Python

1. **Zero-cost abstractions**: Trait-based tool dispatch compiles to direct function calls (no vtable when monomorphized)
2. **Send + Sync compile-time enforcement**: Thread safety proven at compile time, not at runtime
3. **Algebraic types**: `enum ToolResult { Success(String), Error(ToolError), Timeout }` — exhaustive match
4. **Lifetime-checked memory reuse**: Arena allocators for message history, zero-copy JSON parsing
5. **Static binary embedding**: Skills, default skins, and prompts compiled into binary via `include_str!()`
6. **WASM target**: Same codebase can compile to WebAssembly for browser-based agents
7. **`#[cfg(feature = "...")]`**: Compile-time feature gating (telegram, discord, mcp — only what you need)
8. **Deterministic resource cleanup**: `Drop` trait guarantees sandbox cleanup — no `atexit` fragility

## 3. Feature Parity Matrix

> Every row verified against hermes-agent v0.4.0 source code. No feature omitted.

### 3.1 Core Agent

| Feature | hermes-agent | OpenClaw | EdgeCrab | Notes |
|---------|:------------:|:--------:|:--------:|-------|
| Interactive TUI | ✅ | ✅ | ✅ | ratatui (superior rendering) |
| Streaming by default | ✅ | ✅ | ✅ | Spinner + tool progress during stream |
| 50+ built-in tools | ✅ | ✅ | ✅ | Trait-based, compile-time checked |
| Self-improving skills | ✅ | ❌ | ✅ | Same SKILL.md format, Skills Hub sync |
| Persistent memory | ✅ | ✅ | ✅ | MEMORY.md + USER.md + file locking |
| SOUL.md identity | ✅ | ❌ | ✅ | Primary agent persona (replaces hardcoded) |
| FTS5 session search | ✅ | ❌ | ✅ | rusqlite with bundled SQLite |
| Honcho dialectic memory | ✅ | ❌ | ✅ | 4 tools: context, profile, search, conclude |
| Cron scheduling | ✅ | ❌ | ✅ | cron crate + tokio::time |
| Subagent delegation | ✅ | ❌ | ✅ | tokio::spawn (true parallelism) |
| Code execution (PTC) | ✅ | ❌ | ✅ | Sandboxed subprocess |
| 6 terminal backends | ✅ | ✅ | ✅ | local, docker, ssh, daytona, modal, singularity |
| MCP integration + OAuth 2.1 | ✅ | ❌ | ✅ | mcp-rust-sdk + PKCE flow |
| 17+ LLM providers | ✅ | ✅ | ✅ | Via edgequake-llm [→ 3.4] |
| Smart model routing | ✅ | ❌ | ✅ | Eager fallback on rate-limit |
| Context compression | ✅ | ❌ | ✅ | Structured summaries, iterative updates |
| Prompt caching | ✅ | ❌ | ✅ | Anthropic cache control, gateway session caching |
| @ context references | ✅ | ❌ | ✅ | @file, @url, @diff, @staged, @folder, @git |
| Background memory review | ✅ | ❌ | ✅ | Replaces inline nudges |
| Context pressure warnings | ✅ | ❌ | ✅ | CLI + gateway budget alerts |
| Auto session titles | ✅ | ❌ | ✅ | LLM-generated, `.hermes.md` project config |
| models.dev integration | ✅ | ❌ | ✅ | Provider-aware context length resolution |
| Checkpoint manager | ✅ | ❌ | ✅ | [→ 009.001#checkpoints] |
| Real-time config reload | ✅ | ❌ | ✅ | Hot-reload without restart |
| ${ENV_VAR} config substitution | ✅ | ❌ | ✅ | In config.yaml values |
| custom_models.yaml | ✅ | ❌ | ✅ | User-managed model additions |
| 12 tool-call parsers | ✅ | ❌ | ✅ | DeepSeek V3/V3.1, GLM 4.5/4.7, Hermes, Kimi K2, Llama, LongCat, Mistral, Qwen/Qwen3 Coder |
| Copilot ACP client | ✅ | ❌ | ✅ | OpenAI-shim over `copilot --acp` JSONRPC |
| Honcho integration module | ✅ | ❌ | ✅ | Dedicated cli.py, client.py, session.py |
| Trajectory compression | ✅ | ❌ | ✅ | RL trajectory compressor for data gen |

### 3.2 Tools & Capabilities

| Feature | hermes-agent | OpenClaw | EdgeCrab | Notes |
|---------|:------------:|:--------:|:--------:|-------|
| Mixture of Agents (MoA) | ✅ | ❌ | ✅ | Multi-LLM collaboration (4 reference models + aggregator) |
| Image generation | ✅ | ❌ | ✅ | `image_generate` tool |
| Vision / image analysis | ✅ | ❌ | ✅ | `vision_analyze` tool |
| Text-to-speech (TTS) | ✅ | ❌ | ✅ | `text_to_speech` + NeuTTS local provider |
| Speech-to-text (STT) | ✅ | ❌ | ✅ | Transcription tools (Whisper) |
| Voice mode (push-to-talk) | ✅ | ❌ | ✅ | sounddevice capture + STT + TTS playback |
| Browser automation | ✅ | ❌ | ✅ | navigate, snapshot, click, type, scroll, vision, console |
| Browser providers | ✅ | ❌ | ✅ | Browserbase + Browser Use (pluggable) |
| Todo / planning tool | ✅ | ❌ | ✅ | Structured task tracking for agent |
| Send message (cross-platform) | ✅ | ❌ | ✅ | Gateway-gated cross-platform messaging |
| Home Assistant | ✅ | ❌ | ✅ | 4 tools: list_entities, get_state, list_services, call_service |
| Clarify tool | ✅ | ❌ | ✅ | Ask user clarifying questions mid-task |
| Fuzzy tool dispatch | ✅ | ❌ | ✅ | Fuzzy-match tool names on mismatch |
| URL safety | ✅ | ❌ | ✅ | url_safety.py + website_policy.py |
| Tirith static analysis | ✅ | ❌ | ✅ | Security scanning for generated code |
| Skills guard | ✅ | ❌ | ✅ | Security gate for skill execution |
| ANSI strip | ✅ | ❌ | ✅ | Clean terminal output before LLM ingestion |
| Env passthrough | ✅ | ❌ | ✅ | Selective env var forwarding to sandboxes |
| RL training tool | ✅ | ❌ | ✅ | Atropos RL environment integration |
| Patch parser | ✅ | ❌ | ✅ | Unified diff / patch application |
| 8 RL environments | ✅ | ❌ | ✅ | hermes_swe_env, web_research_env, agentic_opd_env, terminal_test_env, tblite_env, terminalbench2_env, yc_bench_env + base |
| FRAMES web research | ✅ | ❌ | ✅ | Multi-hop factual QA training (Google FRAMES benchmark) |
| TBLite benchmark | ✅ | ❌ | ✅ | Lightweight terminal benchmark environment |
| TerminalBench 2 | ✅ | ❌ | ✅ | Next-gen terminal benchmark suite |
| YC Bench | ✅ | ❌ | ✅ | YC-class benchmark environment |
| Persistent shell | ✅ | ❌ | ✅ | Long-lived shell sessions across tool calls |
| OpenRouter HTTP client | ✅ | ❌ | ✅ | Absorbed into edgequake-llm |

### 3.3 Gateway & Platforms

| Feature | hermes-agent | OpenClaw | EdgeCrab | Notes |
|---------|:------------:|:--------:|:--------:|-------|
| Multi-platform gateway | ✅ 14 | ❌ | ✅ 14+ | Tokio async, native performance |
| Telegram | ✅ | ❌ | ✅ | MarkdownV2, groups, threads, topics, auto-reconnect |
| Discord | ✅ | ❌ | ✅ | DM vision, typing indicator, thread persistence |
| Slack | ✅ | ❌ | ✅ | Full adapter |
| WhatsApp | ✅ | ❌ | ✅ | Bridge-based, image support, LID format |
| Signal | ✅ | ❌ | ✅ | Attachments, group filtering, Note to Self protection |
| Email (IMAP) | ✅ | ❌ | ✅ | Keyring integration |
| SMS (Twilio) | ✅ | ❌ | ✅ | Twilio adapter |
| DingTalk | ✅ | ❌ | ✅ | Full adapter with setup docs |
| Matrix | ✅ | ❌ | ✅ | Vision support, image caching |
| Mattermost | ✅ | ❌ | ✅ | @-mention-only channel filter, MIME types |
| Webhook | ✅ | ❌ | ✅ | External event triggers |
| Home Assistant | ✅ | ❌ | ✅ | Smart home control adapter |
| OpenAI-compatible API | ✅ | ❌ | ✅ | /v1/chat/completions + /api/jobs REST |
| Telegram network relay | ✅ | ❌ | ✅ | Multi-bot Telegram routing |
| Gateway hooks | ✅ | ❌ | ✅ | Pre/post message lifecycle hooks |
| Gateway mirror | ✅ | ❌ | ✅ | Cross-platform message mirroring |
| Gateway pairing | ✅ | ❌ | ✅ | DM security pairing flow |
| Sticker cache | ✅ | ❌ | ✅ | Platform sticker/emoji caching |
| Stream consumer | ✅ | ❌ | ✅ | Streaming response delivery to platforms |
| Channel directory | ✅ | ❌ | ✅ | Platform channel routing |
| Gateway auto-reconnect | ✅ | ❌ | ✅ | Exponential backoff on failure |
| Gateway prompt caching | ✅ | ❌ | ✅ | Cache AIAgent per session, preserve prompt cache |

### 3.4 LLM Providers (17+ supported)

> **Architecture decision**: EdgeCrab delegates all LLM communication to `edgequake-llm`.
> Hermes-agent providers that map to edgequake-llm's 13 native providers get **zero new code**.
> Custom/niche providers (Nous Portal, z.ai, Kimi, MiniMax, DeepSeek, DashScope, Kilo Code,
> OpenCode) are implemented as `OpenAICompatible` provider configs in edgequake-llm or thin
> adapter modules in `edgecrab-core`.
>
> **edgequake-llm native providers** (v0.3.0): OpenAI, Azure OpenAI, Anthropic, Gemini/Vertex AI,
> xAI (Grok), Mistral AI, OpenRouter, Ollama, LMStudio, HuggingFace, VSCode Copilot,
> AWS Bedrock, OpenAI Compatible.
>
> **edgequake-llm built-in capabilities** leveraged by EdgeCrab (no reimplementation):
> - Response caching (memory + persistent, configurable TTL)
> - Rate limiting (per-minute requests + tokens, exponential backoff)
> - Session-level cost tracking (per-model pricing tables)
> - OpenTelemetry integration (GenAI semantic conventions)
> - BM25 / RRF / hybrid reranking
> - Embeddings (native for 12+ providers)
> - Mock provider (testing)

| Provider | hermes-agent | EdgeCrab | Notes |
|----------|:------------:|:--------:|-------|
| Nous Portal | ✅ | ✅ | Primary provider |
| OpenRouter | ✅ | ✅ | 200+ models |
| OpenAI | ✅ | ✅ | GPT-5.4 series |
| Anthropic | ✅ | ✅ | Claude 4.6 (1M context), prompt caching |
| Google (Gemini) | ✅ | ✅ | Gemini 3 Pro/Flash |
| z.ai / GLM | ✅ | ✅ | |
| Kimi / Moonshot | ✅ | ✅ | |
| MiniMax | ✅ | ✅ | M2.7 |
| DeepSeek | ✅ | ✅ | V3.2 |
| GitHub Copilot | ✅ | ✅ | OAuth + 400k context |
| Alibaba / DashScope | ✅ | ✅ | DashScope v1 runtime |
| Kilo Code | ✅ | ✅ | |
| OpenCode Zen | ✅ | ✅ | |
| OpenCode Go | ✅ | ✅ | |
| Ollama | ✅ | ✅ | model:tag colon preservation |
| vLLM / llama.cpp | ✅ | ✅ | /v1/props context detection |
| Custom endpoint | ✅ | ✅ | model.base_url in config, no-key support |

### 3.5 CLI Features (hermes v0.4.0)

| Feature | hermes-agent | EdgeCrab | Notes |
|----------|:------------:|:--------:|-------|
| /model switch | ✅ | ✅ | Provider-aware model catalog |
| /personality | ✅ | ✅ | Named persona switching |
| /queue | ✅ | ✅ | Queue prompts without interrupting |
| /statusbar | ✅ | ✅ | Persistent config bar (model + provider) |
| /permission | ✅ | ✅ | Dynamic approval mode switch |
| /browser | ✅ | ✅ | Interactive browser from CLI |
| /cost | ✅ | ✅ | Live pricing + usage |
| /compress | ✅ | ✅ | Manual context compression |
| /usage | ✅ | ✅ | Token usage display |
| /insights | ✅ | ✅ | Usage analytics by day |
| /skills | ✅ | ✅ | Skills hub browse/install |
| /approve, /deny | ✅ | ✅ | Explicit gateway approval commands |
| /retry, /undo | ✅ | ✅ | Conversation step management |
| /new, /reset | ✅ | ✅ | Fresh conversation |
| /stop | ✅ | ✅ | Interrupt current run |
| Tab completions | ✅ | ✅ | Slash commands + @ context |
| Multiline editing | ✅ | ✅ | Shift+Enter line mode |
| Ctrl+C interrupt-and-redirect | ✅ | ✅ | Send new message mid-task |
| Skin engine | ✅ | ✅ | YAML themes, full color customization |
| Doctor diagnostics | ✅ | ✅ | `edgecrab doctor` |
| Setup wizard | ✅ | ✅ | Interactive first-run configuration |
| Uninstall command | ✅ | ✅ | Clean removal |
| hermes mcp (MCP mgmt) | ✅ | ✅ | Install, configure, OAuth |
| hermes tools (enable/disable) | ✅ | ✅ | Per-platform tool configuration |
| hermes skills (enable/disable) | ✅ | ✅ | Per-platform skill configuration |

### 3.6 Plugin System

| Feature | hermes-agent | OpenClaw | EdgeCrab | Notes |
|---------|:------------:|:--------:|:--------:|-------|
| Plugin system | ✅ | ❌ | ✅ | 3 sources: user, project, pip entry-point → WASM plugins |
| Plugin hooks | ✅ | ❌ | ✅ | pre/post_tool_call, pre/post_llm_call, session start/end |
| Plugin tool registration | ✅ | ❌ | ✅ | Plugins register tools via PluginContext |
| Plugin manifest | ✅ | ❌ | ✅ | plugin.yaml + __init__.py register(ctx) |
| **WASM plugin hot-reload** | ❌ | ❌ | ✅ | **New**: wasmtime-based dynamic loading |

### 3.7 EdgeCrab-Only Features (New)

| Feature | hermes-agent | OpenClaw | EdgeCrab | Notes |
|---------|:------------:|:--------:|:--------:|-------|
| **Single static binary** | ❌ | ❌ | ✅ | ~15-25MB, curl + chmod |
| **True parallelism** | ❌ | ❌ | ✅ | No GIL, Tokio + rayon |
| **WASM support** | ❌ | ❌ | ✅ | Phase 3: browser-based agents |
| **WASM plugin hot-reload** | ❌ | ❌ | ✅ | Phase 2: wasmtime isolation |
| **Embedded web UI** | ❌ | ❌ | ✅ | Phase 3: axum-served SPA |
| **Compile-time tool checks** | ❌ | ❌ | ✅ | Trait bounds catch errors pre-runtime |
| **Feature-gated binary** | ❌ | ❌ | ✅ | `--features telegram,discord` |
| **Deterministic Drop cleanup** | ❌ | ❌ | ✅ | No atexit fragility |
| hermes config migration | N/A | ❌ | ✅ | `edgecrab migrate` |
| OpenClaw migration | ✅ | N/A | ✅ | `edgecrab migrate --from openclaw` |

### 3.8 Skills Ecosystem (Claude Skills Compatible)

EdgeCrab's skills system is **fully compatible with the [Agent Skills](https://agentskills.io/) open standard**
used by Claude Code. Skills can be written in
**Python, Node.js, or Shell** — EdgeCrab executes them as subprocesses, not compiled Rust.

| Feature | hermes-agent | Claude Code | EdgeCrab | Notes |
|---------|:------------:|:-----------:|:--------:|-------|
| SKILL.md format | ✅ | ✅ | ✅ | YAML frontmatter + markdown instructions |
| `$ARGUMENTS` substitution | ✅ | ✅ | ✅ | `$ARGUMENTS`, `$ARGUMENTS[N]`, `$N` shorthand |
| `$EDGECRAB_SKILL_DIR` | ❌ | ✅ (`$CLAUDE_SKILL_DIR`) | ✅ | Skill directory path for bundled scripts |
| `$EDGECRAB_SESSION_ID` | ❌ | ✅ (`$CLAUDE_SESSION_ID`) | ✅ | Current session ID for logging |
| Shell injection `!`cmd`` | ❌ | ✅ | ✅ | Dynamic context via shell commands |
| `context: fork` subagents | ❌ | ✅ | ✅ | Run skill in isolated subagent |
| `disable-model-invocation` | ❌ | ✅ | ✅ | Manual-only invocation |
| `user-invocable: false` | ❌ | ✅ | ✅ | Background knowledge, hidden from menu |
| `allowed-tools` restriction | ❌ | ✅ | ✅ | Limit tool access per skill |
| `paths` glob activation | ❌ | ✅ | ✅ | Auto-activate on matching files |
| Supporting files | ✅ | ✅ | ✅ | templates, examples, scripts in skill dir |
| Python skill scripts | ✅ | ✅ | ✅ | `scripts/*.py` executed via subprocess |
| Node.js skill scripts | ✅ | ✅ | ✅ | `scripts/*.js` executed via subprocess |
| Shell skill scripts | ✅ | ✅ | ✅ | `scripts/*.sh` executed via subprocess |
| Skills Hub sync | ✅ | ❌ | ✅ | Central skills registry + install |
| 30+ built-in skill categories | ✅ | ~6 bundled | ✅ | apple, gaming, music, data-science, github, mcp, etc. |
| Skill enable/disable per platform | ✅ | ❌ | ✅ | `edgecrab skills` CLI |
| Skills guard (security gate) | ✅ | ✅ (permissions) | ✅ | Prevent unauthorized skill execution |

**Hermes-agent ships 30+ skill categories**: apple, autonomous-ai-agents, creative, data-science, diagramming, dogfood, domain, email, feeds, gaming, gifs, github, inference-sh, leisure, mcp, media, mlops, music-creation, note-taking, productivity, red-teaming, research, smart-home, social-media, software-development. Optional: autonomous-ai-agents, blockchain, creative, devops, email, health, mcp, migration, productivity, research, security.

**SKILL.md format** (compatible with both hermes-agent and Claude Code):
```yaml
---
name: my-skill
description: What this skill does and when to use it
disable-model-invocation: false    # true = manual /slash only
user-invocable: true               # false = background knowledge only
allowed-tools: Read, Grep, Bash    # restrict tool access
context: fork                      # run in isolated subagent
paths: "src/**/*.rs, tests/**"     # auto-activate on matching files
---

Skill instructions in markdown...

## Dynamic context
- Current branch: !`git branch --show-current`
- Changed files: !`git diff --name-only`

## Usage
Run the helper script:
```bash
python ${EDGECRAB_SKILL_DIR}/scripts/helper.py $ARGUMENTS
```
```

## 4. Entry Points

```
+--------------------+---------------------------+------------------------------------------+
| Command            | Rust Module               | Purpose                                  |
+--------------------+---------------------------+------------------------------------------+
| edgecrab           | edgecrab_cli::main        | Interactive CLI + all subcommands        |
+--------------------+---------------------------+------------------------------------------+
| edgecrab agent     | edgecrab_core::main       | Standalone headless agent runner         |
+--------------------+---------------------------+------------------------------------------+
| edgecrab gateway   | edgecrab_gateway::main    | Multi-platform messaging gateway         |
+--------------------+---------------------------+------------------------------------------+
| edgecrab acp       | edgecrab_acp::main        | ACP server (VS Code/Zed/JetBrains)      |
+--------------------+---------------------------+------------------------------------------+
| edgecrab batch     | edgecrab_core::batch      | Parallel batch processing runner         |
+--------------------+---------------------------+------------------------------------------+
| edgecrab migrate   | edgecrab_migrate::main    | hermes/OpenClaw migration utility        |
+--------------------+---------------------------+------------------------------------------+
| edgecrab doctor    | edgecrab_cli::doctor      | Diagnostics and troubleshooting          |
+--------------------+---------------------------+------------------------------------------+
| edgecrab setup     | edgecrab_cli::setup       | Interactive first-run setup wizard       |
+--------------------+---------------------------+------------------------------------------+
| edgecrab rl        | edgecrab_core::rl         | RL training environment runner           |
+--------------------+---------------------------+------------------------------------------+
```

All entry points compile into a **single binary** with subcommand dispatch via `clap`.

**Design analogues from OpenClaw/Nous Hermes:**
- `edgecrab` — interactive TUI with full slash-command set
- `edgecrab agent` — headless agent runner (batch, CI, scripts)
- `edgecrab gateway` — multi-platform messaging bridge
- `edgecrab acp` — editor integration (VS Code, Zed, JetBrains)
- `edgecrab batch` — parallel batch processing runner
- `edgecrab rl` — RL training environment runner

## 5. Workspace Crate Layout

```
edgecrab/
├── Cargo.toml                    # Workspace root
├── Cargo.lock
├── edgecrab-cli/                 # Binary crate: TUI + subcommands
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs               # Entry point, clap dispatch
│       ├── commands.rs           # Slash commands + autocomplete
│       ├── callbacks.rs          # Terminal callbacks (clarify, sudo, approval)
│       ├── setup.rs              # Interactive setup wizard
│       ├── skin_engine.rs        # Skin/theme engine (YAML themes)
│       ├── skills_config.rs      # hermes skills enable/disable
│       ├── tools_config.rs       # hermes tools enable/disable
│       ├── skills_hub.rs         # /skills slash command (search, browse, install)
│       ├── model_switch.rs       # /model switch pipeline
│       ├── models.rs             # Model catalog, provider lists
│       ├── auth.rs               # Provider credential resolution
│       ├── doctor.rs             # Diagnostics (hermes doctor)
│       ├── banner.rs             # Startup banner
│       ├── clipboard.rs          # Clipboard integration
│       ├── pairing.rs            # DM pairing setup
│       ├── cron.rs               # hermes cron subcommands
│       ├── mcp_config.rs         # hermes mcp install/config/auth
│       ├── plugins_cmd.rs        # hermes plugins management
│       ├── uninstall.rs          # Clean removal
│       ├── claw.rs               # OpenClaw migration subcommands
│       ├── codex_models.rs       # Codex model catalog
│       ├── runtime_provider.rs   # Runtime provider resolution
│       ├── env_loader.rs         # .env file loading chain
│       ├── curses_ui.rs          # Alternative curses-based UI
│       └── colors.rs             # Color palette definitions
├── edgecrab-core/                # Library crate: Agent, conversation loop
│   ├── Cargo.toml
│   └── src/
│       ├── agent.rs              # Agent struct + builder pattern
│       ├── conversation.rs       # async run_conversation() loop
│       ├── prompt_builder.rs     # System prompt assembly pipeline
│       ├── context_compressor.rs # Structured summary compression
│       ├── context_references.rs # @ reference expansion (@file, @url, @diff, etc.)
│       ├── prompt_caching.rs     # Anthropic prompt cache control
│       ├── smart_routing.rs      # Provider routing + fallbacks
│       ├── model_metadata.rs     # Context lengths, token estimation, models.dev
│       ├── auxiliary_client.rs   # Auxiliary LLM (vision, summarization)
│       ├── trajectory.rs         # Trajectory saving for RL
│       ├── usage_pricing.rs      # Cost estimation + normalization
│       ├── insights.rs           # Usage analytics
│       ├── title_generator.rs    # Auto session title generation
│       ├── display.rs            # Spinner, tool preview formatting
│       ├── redact.rs             # Sensitive data redaction
│       ├── skill_commands.rs     # Skill slash commands (shared CLI/gateway)
│       ├── skill_utils.rs        # Skill utility functions
│       ├── batch.rs              # Parallel batch runner
│       ├── rl.rs                 # RL training runner
│       └── anthropic_adapter.rs  # Anthropic message format conversion
├── edgecrab-tools/               # Library crate: Tool registry + implementations
│   ├── Cargo.toml
│   └── src/
│       ├── registry.rs           # Central tool registry (schemas, handlers)
│       ├── approval.rs           # Dangerous command detection
│       ├── terminal.rs           # Terminal orchestration
│       ├── process_registry.rs   # Background process management
│       ├── file_tools.rs         # File read/write/search/patch
│       ├── file_operations.rs    # Higher-level file ops
│       ├── web_tools.rs          # Web search/extract (parallel + Firecrawl)
│       ├── browser.rs            # Browser automation (11 sub-tools)
│       ├── browser_providers/    # Browserbase, Browser Use backends
│       ├── code_execution.rs     # execute_code sandbox
│       ├── delegate.rs           # Subagent delegation
│       ├── mcp.rs                # MCP client (~1050 lines equiv)
│       ├── mcp_oauth.rs          # MCP OAuth 2.1 flow
│       ├── mixture_of_agents.rs  # MoA multi-LLM tool
│       ├── image_generation.rs   # Image generation tool
│       ├── vision.rs             # Vision analysis tool
│       ├── tts.rs                # Text-to-speech tool
│       ├── transcription.rs      # Speech-to-text tool
│       ├── voice_mode.rs         # Push-to-talk audio I/O
│       ├── todo.rs               # Planning / task tracking tool
│       ├── memory.rs             # Memory read/write tool
│       ├── session_search.rs     # FTS5 session search tool
│       ├── clarify.rs            # Clarification questions tool
│       ├── cronjob.rs            # Cron job management tool
│       ├── send_message.rs       # Cross-platform send_message
│       ├── homeassistant.rs      # Home Assistant 4-tool suite
│       ├── honcho.rs             # Honcho memory 4-tool suite
│       ├── skill_manager.rs      # Skill CRUD operations
│       ├── skills_hub.rs         # Skills Hub sync
│       ├── skills_guard.rs       # Skill execution security gate
│       ├── skills_sync.rs        # Hub synchronization logic
│       ├── skills_tool.rs        # Skills list/view/manage tools
│       ├── rl_training.rs        # RL training integration
│       ├── checkpoint_manager.rs # Checkpoint save/restore
│       ├── interrupt.rs          # Interrupt signal handling
│       ├── fuzzy_match.rs        # Fuzzy tool name dispatch
│       ├── url_safety.rs         # URL validation + blocking
│       ├── website_policy.rs     # Website access policy
│       ├── tirith_security.rs    # Static analysis scanning
│       ├── ansi_strip.rs         # ANSI escape cleanup
│       ├── env_passthrough.rs    # Env var forwarding
│       ├── patch_parser.rs       # Unified diff application
│       ├── debug_helpers.rs      # Debug utilities
│       ├── neutts_synth.rs       # NeuTTS local TTS
│       └── environments/         # Terminal backends
│           ├── base.rs           # TerminalBackend trait
│           ├── local.rs
│           ├── docker.rs
│           ├── ssh.rs
│           ├── daytona.rs
│           ├── modal.rs
│           ├── singularity.rs
│           └── persistent_shell.rs
├── edgecrab-gateway/             # Library crate: Multi-platform gateway
│   ├── Cargo.toml
│   └── src/
│       ├── run.rs                # Main loop, slash commands, message dispatch
│       ├── session.rs            # Session persistence
│       ├── config.rs             # Gateway configuration
│       ├── delivery.rs           # Message delivery routing
│       ├── hooks.rs              # Pre/post message lifecycle hooks
│       ├── mirror.rs             # Cross-platform message mirroring
│       ├── pairing.rs            # DM security pairing
│       ├── sticker_cache.rs      # Platform sticker/emoji cache
│       ├── stream_consumer.rs    # Streaming response consumer
│       ├── channel_directory.rs  # Platform channel routing
│       ├── status.rs             # Gateway health status
│       └── platforms/            # 14 platform adapters
│           ├── base.rs           # PlatformAdapter trait
│           ├── telegram.rs
│           ├── telegram_network.rs
│           ├── discord.rs
│           ├── slack.rs
│           ├── whatsapp.rs
│           ├── signal.rs
│           ├── email.rs
│           ├── sms.rs
│           ├── dingtalk.rs
│           ├── matrix.rs
│           ├── mattermost.rs
│           ├── webhook.rs
│           ├── homeassistant.rs
│           └── api_server.rs
├── edgecrab-state/               # Library crate: SQLite state + config
│   ├── Cargo.toml
│   └── src/
├── edgecrab-security/            # Library crate: Security scanning
│   ├── Cargo.toml
│   └── src/
├── edgecrab-types/               # Library crate: Shared types (Message, ToolDef)
│   ├── Cargo.toml
│   └── src/
├── edgecrab-acp/                 # Library crate: ACP server (VS Code/Zed/JetBrains)
│   ├── Cargo.toml
│   └── src/
├── edgecrab-migrate/             # Library crate: hermes/OpenClaw migration
│   ├── Cargo.toml
│   └── src/
├── tests/                        # Integration tests
├── benches/                      # Criterion benchmarks
└── docs/                         # This specification
```

[→ 002.002 Crate Dependency Graph](../002_architecture/002_crate_dependency_graph.md)

## 6. Configuration Layout (Backward Compatible)

```
~/.edgecrab/                       [env: EDGECRAB_HOME, default ~/.edgecrab]
├── config.yaml                    # Main configuration (hermes-compatible keys + ${ENV_VAR} substitution)
├── custom_models.yaml             # User-managed model additions (hermes v0.4.0+)
├── .env                           # API keys and secrets
├── state.db                       # SQLite session database (FTS5)
├── auth.json                      # OAuth/provider credentials (Copilot, MCP)
├── SOUL.md                        # Primary agent identity/persona (replaces DEFAULT_AGENT_IDENTITY)
├── .hermes.md                     # Project-level config (auto session titles, context)
├── memories/
│   ├── MEMORY.md                  # Agent personal notes (same format as hermes)
│   └── USER.md                    # User profile observations
├── skills/                        # Installed skills (same SKILL.md format)
│   └── .hub/                      # Skills Hub sync metadata
├── skins/                         # Custom UI themes (YAML, hermes-compatible)
├── cron/
│   └── crontab.yaml               # Scheduled jobs
├── plugins/                       # WASM plugin directory
│   └── <name>/
│       ├── plugin.yaml            # Plugin manifest
│       └── __init__.py → lib.wasm # Plugin code (Python→WASM in EdgeCrab)
├── mcp/                           # MCP server configurations
│   └── servers.yaml               # MCP server registry + OAuth tokens
├── image_cache/                   # Downloaded platform images
├── audio_cache/                   # Gateway audio + TTS cache
├── checkpoints/                   # Agent checkpoint snapshots
└── logs/                          # Structured logs (JSON + text)
```

**Migration symlink**: `edgecrab migrate` can create `~/.edgecrab` as a symlink to `~/.hermes`
for zero-friction transition, or copy selectively.

**Config reload**: Changes to `config.yaml` apply without restart (hermes v0.4.0+, EdgeCrab native).

## 7. Hermes-Agent Source File Audit (Code-Is-Law)

> Complete mapping of every hermes-agent source file to its EdgeCrab module.
> This ensures **zero feature loss** during the Rust rewrite.

### 7.1 Files NOT Previously Documented (discovered in v0.4.0 audit)

| hermes-agent File | EdgeCrab Module | Status |
|-------------------|-----------------|--------|
| `hermes_cli/plugins.py` | edgecrab-cli/plugins_cmd.rs | Plugin system (3 sources, 6 hooks) |
| `hermes_cli/plugins_cmd.py` | edgecrab-cli/plugins_cmd.rs | Plugin CLI management |
| `hermes_cli/copilot_auth.py` | edgecrab-cli/auth.rs | GitHub Copilot OAuth flow |
| `hermes_cli/clipboard.py` | edgecrab-cli/clipboard.rs | System clipboard integration |
| `hermes_cli/banner.py` | edgecrab-cli/banner.rs | Startup banner rendering |
| `hermes_cli/colors.py` | edgecrab-cli/colors.rs | Color palette definitions |
| `hermes_cli/checklist.py` | edgecrab-cli/checklist.rs | Interactive checklist UI |
| `hermes_cli/curses_ui.py` | edgecrab-cli/curses_ui.rs | Alternative curses-based UI |
| `hermes_cli/default_soul.py` | edgecrab-core (include_str!) | Default SOUL.md content |
| `hermes_cli/doctor.py` | edgecrab-cli/doctor.rs | System diagnostics |
| `hermes_cli/env_loader.py` | edgecrab-cli/env_loader.rs | .env loading chain |
| `hermes_cli/mcp_config.py` | edgecrab-cli/mcp_config.rs | MCP server management CLI |
| `hermes_cli/pairing.py` | edgecrab-cli/pairing.rs | DM pairing setup |
| `hermes_cli/status.py` | edgecrab-cli/status.rs | Gateway status display |
| `hermes_cli/uninstall.py` | edgecrab-cli/uninstall.rs | Clean removal |
| `hermes_cli/codex_models.py` | edgecrab-cli/codex_models.rs | Codex model catalog |
| `hermes_cli/runtime_provider.py` | edgecrab-cli/runtime_provider.rs | Runtime provider resolution |
| `hermes_cli/cron.py` | edgecrab-cli/cron.rs | Cron CLI subcommands |
| `hermes_cli/gateway.py` | edgecrab-cli/gateway.rs | Gateway CLI subcommands |
| `hermes_cli/claw.py` | edgecrab-migrate | OpenClaw migration internals |
| `agent/context_references.py` | edgecrab-core/context_references.rs | @file, @url, @diff, @staged, @folder, @git |
| `agent/anthropic_adapter.py` | edgecrab-core/anthropic_adapter.rs | Anthropic message format conversion |
| `agent/copilot_acp_client.py` | edgecrab-acp/copilot_client.rs | Copilot ACP client |
| `agent/models_dev.py` | edgecrab-core/model_metadata.rs | models.dev registry integration |
| `tools/mixture_of_agents_tool.py` | edgecrab-tools/mixture_of_agents.rs | MoA multi-LLM tool |
| `tools/todo_tool.py` | edgecrab-tools/todo.rs | Planning/task tracking |
| `tools/image_generation_tool.py` | edgecrab-tools/image_generation.rs | Image generation |
| `tools/tts_tool.py` | edgecrab-tools/tts.rs | Text-to-speech |
| `tools/neutts_synth.py` | edgecrab-tools/neutts_synth.rs | NeuTTS local TTS backend |
| `tools/transcription_tools.py` | edgecrab-tools/transcription.rs | Speech-to-text (Whisper) |
| `tools/voice_mode.py` | edgecrab-tools/voice_mode.rs | Push-to-talk audio I/O |
| `tools/homeassistant_tool.py` | edgecrab-tools/homeassistant.rs | Home Assistant 4-tool suite |
| `tools/send_message_tool.py` | edgecrab-tools/send_message.rs | Cross-platform messaging |
| `tools/rl_training_tool.py` | edgecrab-tools/rl_training.rs | RL training integration |
| `tools/debug_helpers.py` | edgecrab-tools/debug_helpers.rs | Debug utilities |
| `tools/openrouter_client.py` | edgequake-llm (absorbed) | OpenRouter HTTP client |
| `tools/checkpoint_manager.py` | edgecrab-tools/checkpoint_manager.rs | Checkpoint save/restore |
| `tools/browser_providers/` | edgecrab-tools/browser_providers/ | Browserbase + Browser Use |
| `tools/skills_tool.py` | edgecrab-tools/skills_tool.rs | Skills list/view/manage |
| `tools/skills_hub.py` | edgecrab-tools/skills_hub.rs | Skills Hub operations |
| `tools/skills_sync.py` | edgecrab-tools/skills_sync.rs | Hub sync logic |
| `tools/honcho_tools.py` | edgecrab-tools/honcho.rs | 4 Honcho memory tools |
| `gateway/delivery.py` | edgecrab-gateway/delivery.rs | Message delivery routing |
| `gateway/config.py` | edgecrab-gateway/config.rs | Gateway configuration |
| `gateway/status.py` | edgecrab-gateway/status.rs | Gateway health status |
| `gateway/platforms/api_server.py` | edgecrab-gateway/platforms/api_server.rs | OpenAI-compatible API |
| `gateway/platforms/telegram_network.py` | edgecrab-gateway/platforms/telegram_network.rs | Multi-bot Telegram relay |
| `gateway/platforms/sms.py` | edgecrab-gateway/platforms/sms.rs | Twilio SMS adapter |
| `gateway/platforms/dingtalk.py` | edgecrab-gateway/platforms/dingtalk.rs | DingTalk adapter |
| `gateway/platforms/matrix.py` | edgecrab-gateway/platforms/matrix.rs | Matrix adapter |
| `gateway/platforms/mattermost.py` | edgecrab-gateway/platforms/mattermost.rs | Mattermost adapter |
| `gateway/platforms/webhook.py` | edgecrab-gateway/platforms/webhook.rs | Webhook adapter |
| `toolset_distributions.py` | edgecrab-tools/registry.rs | Toolset distribution logic |
| `trajectory_compressor.py` | edgecrab-core/trajectory.rs | Trajectory compression for RL |
| `hermes_time.py` | std::time (Rust stdlib) | Time utilities |
| `hermes_constants.py` | edgecrab-types/constants.rs | Shared constants |
| `hermes_state.py` | edgecrab-state | SessionDB (SQLite + FTS5) |
| `utils.py` | edgecrab-types/utils.rs | atomic_json_write, etc. |
| `environments/tool_call_parsers/` | edgecrab-core/tool_call_parsers/ | 12 model-specific parsers (DeepSeek V3/V3.1, GLM 4.5/4.7, Hermes, Kimi K2, Llama, LongCat, Mistral, Qwen/Qwen3 Coder) |
| `environments/web_research_env.py` | edgecrab-core/rl/web_research.rs | FRAMES benchmark RL environment |
| `environments/agentic_opd_env.py` | edgecrab-core/rl/agentic_opd.rs | Agentic OPD RL environment |
| `environments/hermes_swe_env/` | edgecrab-core/rl/swe.rs | SWE-bench RL environment |
| `environments/terminal_test_env/` | edgecrab-core/rl/terminal_test.rs | Terminal tool testing environment |
| `environments/hermes_base_env.py` | edgecrab-core/rl/base.rs | Base RL environment trait |
| `environments/patches.py` | edgecrab-core/rl/patches.rs | RL environment patches/fixes |
| `environments/agent_loop.py` | edgecrab-core/rl/agent_loop.rs | RL agent loop integration |
| `environments/tool_context.py` | edgecrab-core/rl/tool_context.rs | RL tool execution context |
| `environments/benchmarks/tblite/` | edgecrab-core/rl/benchmarks/tblite.rs | TBLite terminal benchmark environment |
| `environments/benchmarks/terminalbench_2/` | edgecrab-core/rl/benchmarks/terminalbench2.rs | TerminalBench 2 benchmark environment |
| `environments/benchmarks/yc_bench/` | edgecrab-core/rl/benchmarks/yc_bench.rs | YC Bench benchmark environment |
| `honcho_integration/client.py` | edgecrab-tools/honcho.rs | Honcho HTTP client |
| `honcho_integration/session.py` | edgecrab-tools/honcho.rs | Honcho session management |
| `honcho_integration/cli.py` | edgecrab-cli/honcho.rs | Honcho CLI subcommands |
| `agent/redact.py` | edgecrab-core/redact.rs | Sensitive data redaction |
| `agent/skill_utils.py` | edgecrab-core/skill_utils.rs | Skill utility functions |
| `agent/title_generator.py` | edgecrab-core/title_generator.rs | Auto session title generation |
