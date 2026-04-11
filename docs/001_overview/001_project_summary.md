# Project Summary 🦀

> **Verified against:** `Cargo.toml` · `crates/edgecrab-core/src/agent.rs` ·
> `crates/edgecrab-core/src/conversation.rs` · `crates/edgecrab-tools/src/toolsets.rs` ·
> `crates/edgecrab-gateway/src/lib.rs` · `crates/edgecrab-cli/src/cli_args.rs`

---

## The origin story 🦀

EdgeCrab was forged in the heat of a hypothetical three-way battle:

```
  ┌─────────────────────┐   ┌──────────────────────┐   ┌─────────────────┐
  │     NousHermes      │   │    OpenClaw 🦞         │   │  EdgeCrab 🦀    │
  │  (Nous Research     │   │  (open-source self-   │   │                 │
  │   model fine-tune)  │   │   hosted assistant)   │   │                 │
  │                     │   │                       │   │                 │
  │  ■ enhanced reason  │   │  ■ tool use           │   │  ■ deep reason  │
  │  ■ model weights    │   │  ■ TypeScript/Node.js  │   │  ■ fast tools   │
  │  ■ stateless infer  │   │  ■ no built-in        │   │  ■ 91 tools     │
  │  ■ Python inference │   │    security layer     │   │  ■ security     │
  │    stack            │   │  ■ single-user        │   │  ■ Rust, ~49 MB │
  │  ■ not an agent FW  │   │    desktop focus      │   │  ■ 15 gateways  │
  └─────────────────────┘   └──────────────────────┘   └─────────────────┘
         Round 1                    Round 2                WINNER 🏆
    (model, not a runtime)     (TypeScript, not Rust)    (all of the above)
```

> **Fact note**: NousHermes ([NousResearch/Hermes-3-Llama-3.1-8B](https://huggingface.co/NousResearch/Hermes-3-Llama-3.1-8B)) is an LLM fine-tune series, not an agent framework. Its inference tooling is Python (HuggingFace Transformers / vLLM). OpenClaw ([github.com/openclaw](https://github.com/openclaw)) is a real TypeScript/Node.js personal AI assistant — not Python-based. The combat diagram above is an illustrative comparison of design philosophies, not an exhaustive feature audit. 🦀

The design goal: take the enhanced reasoning capabilities of fine-tuned function-calling models
(like Hermes), the tool-use patterns of personal assistant platforms (like OpenClaw), unify
them in a single Rust binary, and ship security hardening and multi-platform delivery that
neither category bothered to implement.

## The predecessor: `hermes-agent`

EdgeCrab is a Rust rewrite of `hermes-agent` — a Python agent (Python venv + Node.js, `prompt_toolkit` TUI, ~80–150 MB resident, 1–3 s startup) maintained by the same authors. EdgeCrab preserves the same configuration structure, memory format, and skills format so migration is a one-command import:

```bash
edgecrab migrate   # imports ~/.hermes/ → ~/.edgecrab/
```

See the [README migration table](../../README.md#migrating-from-hermes-agent) for what gets imported. The `edgecrab-migrate` crate handles config, sessions, memories, skills, and env vars.

## Why EdgeCrab exists

Running an AI agent in production across multiple channels — terminal, Telegram,
Discord, VS Code, cron — typically means maintaining a separate agent runtime per
channel, duplicating prompt assembly, tool dispatch, security checks, and session
persistence across each integration.

EdgeCrab solves this by providing a **single Rust binary** with one shared agent
runtime (`edgecrab-core`) that all frontends delegate to. You get one place to
reason about tool execution, one place to tune the system prompt, one place to
harden security, and one SQLite database for every conversation no matter where it
originated.

The concrete benefits that flow from this design:

| Concern | Without EdgeCrab | With EdgeCrab |
|---|---|---|
| Tool dispatch | Re-implemented per frontend | `ToolRegistry` with 91 registered core tools |
| Session history | Siloed per channel | Unified SQLite with FTS5 search |
| Security | Each integration decides its own | `CommandScanner`, `PathJail`, `InjectionCheck` enforced at the registry level |
| Prompt assembly | Hand-rolled strings | `PromptBuilder` with memory, skills, and context-file injection |
| Context overflow | OOM or truncation | 5-pass compression pipeline with LLM-summarised history |
| Multi-platform delivery | Custom per channel | 18-adapter gateway with unified delivery router |

---

## What EdgeCrab is

At runtime EdgeCrab has three faces, all sharing the same core:

### 1. Terminal TUI (`edgecrab`)

Interactive ratatui UI with streaming tokens, syntax-highlighted Markdown, slash
commands, session history browser, and the agent's full tool belt. Default entry
point for developers.

### 2. Messaging gateway (`edgecrab gateway start`)

Concurrent adapter processes for Telegram, Discord, Slack, WhatsApp, Signal,
Email, Matrix, Mattermost, DingTalk, FeisHu, Wecom, SMS, Webhook, HomeAssistant,
and more. Each message arrives, is delivered to the shared agent, and the response
is dispatched back. Session state per `(platform, user_id)` pair.

### 3. Editor protocol server (`edgecrab acp`)

A JSON-RPC 2.0 stdio server that implements the Agent Communication Protocol, giving
VS Code, Zed, and JetBrains Copilot integration direct access to the same agent
runtime the CLI uses.

---

## The central object

Everything interesting traces back to a single `Agent` value in
`crates/edgecrab-core/src/agent.rs`:

```rust
pub struct Agent {
    config:          RwLock<AgentConfig>,
    provider:        RwLock<Arc<dyn LLMProvider>>,
    state_db:        Option<Arc<SessionDb>>,
    tool_registry:   Option<Arc<ToolRegistry>>,
    gateway_sender:  RwLock<Option<Arc<dyn GatewaySender>>>,
    process_table:   Arc<ProcessTable>,
    session:         RwLock<SessionState>,
    budget:          Arc<IterationBudget>,
    cancel:          Mutex<CancellationToken>,
    gc_cancel:       CancellationToken,
    todo_store:      Arc<TodoStore>,
}
```

The public surface is narrow: `chat(message)`, `chat_streaming(message, chunk_tx)`,
and `run_conversation(user_message, system_message, history)`. All complexity lives
inside.

---

## Request lifecycle

One message from any frontend follows this path through the runtime:

```
  Input
    │
    ▼
  ┌─────────────────────────────────────────────────┐
  │  Frontend (CLI / Gateway / ACP)                 │
  │  Normalises input, resolves session key         │
  └────────────────────┬────────────────────────────┘
                       │  Agent::chat() or
                       │  Agent::run_conversation()
                       ▼
  ┌─────────────────────────────────────────────────┐
  │  Agent::execute_loop()    [edgecrab-core]        │
  │                                                  │
  │  1. Expand @context refs                         │
  │  2. Build / reuse cached system prompt           │
  │  3. Classify message → route to model            │
  │  4. Check iteration budget                       │
  │  5. Compress context if threshold exceeded       │
  │  6. Call LLM provider (up to 3× retry)           │
  │                                                  │
  │     ┌── tool_calls? ──────────────────────┐      │
  │     │  ToolRegistry::dispatch()            │      │
  │     │  → security checks                   │      │
  │     │  → approval gate                     │      │
  │     │  → ToolHandler::execute()            │      │
  │     │  → append results → loop              │      │
  │     └────────────────────────────────────── ┘     │
  │                                                  │
  │     └── text response? → break                   │
  │                                                  │
  │  7. Optional learning reflection (≥5 tool calls) │
  │  8. Persist session to SQLite                    │
  └────────────────────┬────────────────────────────┘
                       │ ConversationResult
                       ▼
  ┌─────────────────────────────────────────────────┐
  │  Frontend delivers formatted response            │
  └─────────────────────────────────────────────────┘
```

---

## Workspace structure

```
edgecrab/
├── crates/
│   ├── edgecrab-types/        ← leaf: Message, AgentError, ToolSchema, Usage
│   ├── edgecrab-security/     ← path jail, cmd scan, injection, redaction
│   ├── edgecrab-state/        ← SQLite WAL + FTS5 session persistence
│   ├── edgecrab-cron/         ← schedule parsing, job store, delivery metadata
│   ├── edgecrab-tools/        ← registry, 91 tools, toolsets, process table
│   ├── edgecrab-core/         ← Agent, loop, prompt builder, compression, routing
│   ├── edgecrab-cli/          ← clap, ratatui, setup wizard, doctor, profiles
│   ├── edgecrab-gateway/      ← 15 adapters, delivery, hooks, pairing, mirroring
│   ├── edgecrab-acp/          ← JSON-RPC 2.0 stdio ACP server
│   └── edgecrab-migrate/      ← hermes→edgecrab migration helper
├── docs/                      ← this documentation tree
├── skills/                    ← bundled Claude Code-compatible skill files
├── memories/                  ← project-level memory files loaded at startup
└── Cargo.toml                 ← workspace manifest
```

---

## Hard numbers from the source

| Fact | Value | Source |
|---|---|---|
| Rust edition | 2024 | `Cargo.toml` |
| MSRV | 1.86.0 | `workspace.package.rust-version` |
| Default model | `ollama/gemma4:latest` | `edgecrab-core/src/config.rs` |
| Default max iterations | 90 | `AgentConfig` default impl |
| Registered core tools | 91 | `edgecrab-tools/src/toolsets.rs` `CORE_TOOLS` |
| CLI slash commands | 53 | `edgecrab-cli/src/commands.rs` |
| Gateway adapters | 15 | `edgecrab-gateway/src/lib.rs` |
| SQLite schema version | 6 | `edgecrab-state/src/session_db.rs` |
| Command scanner patterns | ~40 literal + regex secondary | `edgecrab-security/src/command_scan.rs` |
| Max compression retries | 3 | `conversation.rs: MAX_RETRIES` |
| Skill reflection threshold | 5 tool calls | `conversation.rs: SKILL_REFLECTION_THRESHOLD` |

---

## Key design decisions

**1. Single binary, zero runtime deps.**
Release profile uses `lto = true`, `codegen-units = 1`, `strip = true`.
Current stripped macOS arm64 release builds land around 49 MB. Exact size varies by target triple and enabled features.

**2. Trait-object frontends, not generics.**
`LLMProvider`, `ToolHandler`, `GatewaySender`, `SubAgentRunner`, and
`PlatformAdapter` are all `dyn Trait` objects. This avoids monomorphisation
explosion across the workspace and lets the gateway plug in adapters at startup.

**3. `#![deny(clippy::unwrap_used)]` in `edgecrab-types`.**
The leaf crate that every other crate imports enforces no `unwrap`. Errors
propagate explicitly as `AgentError` variants.

**4. Inventory-based compile-time tool registration.**
Tools use `inventory::submit!` at crate load time. `ToolRegistry::new()` iterates
`inventory::iter` — no `match` arm to update, no manual list to keep in sync.

**5. Trait-object decoupling at the tool layer.**
`edgecrab-tools` defines `SubAgentRunner` and `GatewaySender` as traits; `edgecrab-core` implements them. This breaks the obvious circular dependency between tools (which need to run sub-agents) and core (which owns the agent).

---

## Quick start checklist

```sh
# Install
cargo install edgecrab-cli   # or: npm i -g edgecrab-cli / pip install edgecrab-cli

# First-run setup wizard (provider keys, model, gateway)
edgecrab setup

# Verify health
edgecrab doctor

# Start interactive session
edgecrab

# Ask a non-interactive question
edgecrab "summarise the last 10 git commits"

# Non-interactive with a specific toolset
edgecrab --toolset coding "refactor src/lib.rs to use thiserror"

# Start the multi-platform gateway
edgecrab gateway start
```

---

## FAQ

**Q: Can I use EdgeCrab with OpenAI or Gemini instead of Anthropic?**
Yes. The LLM abstraction is `edgequake-llm`, which supports OpenRouter as the universal
proxy. Set `EDGECRAB_MODEL=openai/gpt-4o` or configure `model.name` in
`~/.edgecrab/config.yaml`.

**Q: Where is conversation history stored?**
`~/.edgecrab/state.db` — a SQLite database with WAL mode and FTS5 full-text search.
See [Session Storage](../009_config_state/002_session_storage.md).

**Q: How do I add my own tools?**
Implement `ToolHandler`, call `inventory::submit!`, and recompile. See
[Tool Registry](../004_tools_system/001_tool_registry.md).

**Q: Is it safe to run EdgeCrab with shell access enabled?**
`CommandScanner` runs Aho-Corasick over ~40 literal patterns plus regex secondary
checks on every terminal command before execution. See [Security](../011_security/001_security.md).

**Q: Can EdgeCrab run headless?**
Yes. `Agent::chat(message)` and `Agent::run_conversation(...)` have no UI
dependency. The gateway and ACP server both run headless.

---

## Cross-references

- Architecture layers → [System Architecture](../002_architecture/001_system_architecture.md)
- How the loop works → [Conversation Loop](../003_agent_core/002_conversation_loop.md)
- Tool dispatch details → [Tool Registry](../004_tools_system/001_tool_registry.md)
- Security model → [Security](../011_security/001_security.md)
- Config resolution → [Config and State](../009_config_state/001_config_state.md)
