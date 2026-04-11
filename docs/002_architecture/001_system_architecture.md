# System Architecture

> **Verified against:** `Cargo.toml` · `crates/edgecrab-core/src/lib.rs` ·
> `crates/edgecrab-tools/src/lib.rs` · `crates/edgecrab-cli/src/main.rs` ·
> `crates/edgecrab-gateway/src/lib.rs` · `crates/edgecrab-acp/src/lib.rs`

---

## Why this architecture

The obvious alternative — each frontend embeds its own agent loop — produces
N copies of the same prompt-assembly code, N approaches to tool dispatch, and
N databases. When you fix a security gap or tune context compression it must be
applied N times.

EdgeCrab inverts this by making `edgecrab-core::Agent` the single source of truth
for all agent behaviour. Every frontend becomes a thin adapter that serialises
user input into `Agent::chat()` calls and deserialises `ConversationResult` back
into platform-native output.

---

## Layer diagram

```
  ╔══════════════════════════════════════════════════════════════════╗
  ║  FRONTEND LAYER                                                  ║
  ║                                                                  ║
  ║  ┌─────────────────┐  ┌──────────────────┐  ┌───────────────┐  ║
  ║  │  edgecrab-cli   │  │ edgecrab-gateway  │  │ edgecrab-acp  │  ║
  ║  │                 │  │                  │  │               │  ║
  ║  │ clap subcommands│  │ 15 gateway        │  │ JSON-RPC 2.0  │  ║
  ║  │ ratatui TUI     │  │ adapters          │  │ stdio server  │  ║
  ║  │ slash commands  │  │ delivery router   │  │ VS Code / Zed │  ║
  ║  │ setup wizard    │  │ hook registry     │  │ JetBrains     │  ║
  ║  │ doctor          │  │ session fan-out   │  │               │  ║
  ║  └────────┬────────┘  └────────┬─────────┘  └───────┬───────┘  ║
  ╚═══════════╪════════════════════╪═══════════════════════╪════════╝
              │                   │                       │
              └───────────────────┼───────────────────────┘
                                  │
                    Agent::chat() / chat_streaming()
                    Agent::run_conversation()
                                  │
  ╔═══════════════════════════════▼══════════════════════════════════╗
  ║  CORE RUNTIME LAYER                                              ║
  ║                                                                  ║
  ║  ┌──────────────────────────────────────────────────────────┐   ║
  ║  │  edgecrab-core                                           │   ║
  ║  │                                                          │   ║
  ║  │  Agent          AgentBuilder     PromptBuilder           │   ║
  ║  │  execute_loop   compression      SmartRouter             │   ║
  ║  │  IterationBudget AppConfig        ModelCatalog           │   ║
  ║  └───────────────────┬──────────────────────────────────────┘   ║
  ║                      │ uses                                      ║
  ║  ┌───────────────────▼──────────┐  ┌────────────────────────┐   ║
  ║  │  edgecrab-tools               │  │  edgecrab-state        │   ║
  ║  │  ToolRegistry (91 tools)      │  │  SessionDb             │   ║
  ║  │  ToolHandler trait            │  │  SQLite WAL + FTS5     │   ║
  ║  │  ToolContext                  │  │  schema v6             │   ║
  ║  │  ProcessTable                 │  └────────────────────────┘   ║
  ║  │  toolset resolution           │                               ║
  ║  └───────────────────┬──────────┘                               ║
  ║                      │ uses                                      ║
  ║  ┌───────────────────▼──────────┐  ┌────────────────────────┐   ║
  ║  │  edgecrab-security            │  │  edgecrab-cron         │   ║
  ║  │  CommandScanner               │  │  schedule parsing      │   ║
  ║  │  path_jail                    │  │  CronStore             │   ║
  ║  │  injection check              │  │  TickLock              │   ║
  ║  │  ApprovalPolicy               │  └────────────────────────┘   ║
  ║  └──────────────────────────────┘                               ║
  ╚══════════════════════════════════════════════════════════════════╝
              │
  ╔═══════════▼══════════════════════════════════════════════════════╗
  ║  TYPE FOUNDATION                                                 ║
  ║                                                                  ║
  ║  edgecrab-types                                                  ║
  ║  Message · Role · Content · ToolCall · ToolSchema                ║
  ║  AgentError · ToolError · Usage · Cost · Trajectory              ║
  ║  Platform · ApiMode · DEFAULT_MODEL                              ║
  ╚══════════════════════════════════════════════════════════════════╝
```

---

## What each layer owns

When deciding where new code should live, treat these ownership rules as constraints:

### Foundation (`edgecrab-types`)
Stable shared types. Every other crate imports this. **Never add runtime logic
here.** Structs, enums, and their `impl` blocks only. `#![deny(clippy::unwrap_used)]`
is enforced.

### Security (`edgecrab-security`)
Reusable, stateless policy checks. Functions and structures that answer
"is this safe?" questions. Contains no agent logic, no LLM calls, no
async runtime. Consumed by both `edgecrab-tools` and `edgecrab-core`.

### Persistence (`edgecrab-state`)
Owns the SQLite schema and all SQL. Nothing outside this crate executes
raw SQL. Session records, messages, FTS5 index, analytics queries, and
schema migrations all live here.

### Schedule (`edgecrab-cron`)
Schedule parsing and job storage shared by the cron CLI commands and
the `manage_cron_jobs` tool. Isolated so neither CLI nor tools pulls in
the other's dependencies for scheduling.

### Tools (`edgecrab-tools`)
Defines `ToolHandler`, `ToolRegistry`, `ToolContext`, and `ProcessTable`.
All 65 tool implementations live here. Does **not** own the agent loop —
sub-agent delegation is expressed through the `SubAgentRunner` trait so
that `edgecrab-core` can implement it without creating a circular dependency.

### Core runtime (`edgecrab-core`)
Owns `Agent`, `AgentBuilder`, the conversation loop (`execute_loop`), context
compression, smart model routing, and prompt assembly. This is the only crate
that calls the LLM provider. Implements `SubAgentRunner` for `edgecrab-tools`.

### Frontends (`edgecrab-cli`, `edgecrab-gateway`, `edgecrab-acp`)
Thin adapters. They build an `Agent` via `AgentBuilder`, pipe user input to
`Agent::chat()` or `Agent::chat_streaming()`, and render the result. They do
not implement their own tool dispatch or prompt assembly.

---

## End-to-end request path (annotated)

```
  Terminal / Telegram / VS Code
          │
          │ raw string "find all TODO comments"
          ▼
  ┌─────────────────────────────────────────┐
  │  Frontend                               │
  │  ■ resolves session key                 │
  │  ■ looks up or creates GatewaySession   │
  │  ■ invokes Agent::chat_streaming()      │
  └───────────────────┬─────────────────────┘
                      │
                      ▼
  ┌─────────────────────────────────────────┐
  │  Agent::execute_loop()                  │
  │                                         │
  │  [expansion]                            │
  │    expand_context_refs("@./src/")        │
  │                                         │
  │  [routing]                              │
  │    classify_message() → TurnRoute       │
  │    resolve_turn_route() → swap model    │
  │                                         │
  │  [prompt]                               │
  │    PromptBuilder::build()               │
  │    → load memory files                  │
  │    → load skill summaries              │
  │    → inject context files              │
  │                                         │
  │  [budget check]                         │
  │    IterationBudget::try_consume()       │
  │                                         │
  │  [compression]                          │
  │    check_compression_status()           │
  │    maybe: compress_with_llm()           │
  └───────────────────┬─────────────────────┘
                      │
                      ▼
  ┌─────────────────────────────────────────┐
  │  edgequake-llm provider call            │
  │  (up to 3× retry with exponential       │
  │   backoff: 500 ms base)                 │
  └───────────────────┬─────────────────────┘
                      │
          ┌───────────┴──────────┐
          │                      │
          ▼ tool_calls           ▼ assistant text
  ┌────────────────┐    ┌────────────────────┐
  │  security gate │    │  emit StreamEvent  │
  │  approval gate │    │  ::Done            │
  │  ToolHandler   │    │                    │
  │  ::execute()   │    │  persist session   │
  │  emit events   │    │  to SQLite         │
  │  loop ◄────────┘    └────────────────────┘
  └────────────────┘
          │ ConversationResult
          ▼
  ┌─────────────────────────────────────────┐
  │  Frontend renders / delivers response    │
  └─────────────────────────────────────────┘
```

---

## Design constraints visible in code

These constraints are enforced today and changing them has ripple effects:

| Constraint | Location | Implication |
|---|---|---|
| Prompt assembly is centralised | `PromptBuilder` in `edgecrab-core` | Callers may not hand-roll system prompts |
| Tool dispatch is centralised | `ToolRegistry::dispatch()` | No inline tool execution in agent loop |
| Session persistence is optional but standardised | `SessionDb` opt-in via `AgentBuilder::state_db()` | Tests can skip DB; gateway always enables it |
| Frontends share config shape | `AgentConfig` and `AppConfig` | Config merging (`merge_cli`) works identically everywhere |
| Long-running side effects use explicit handles | `ProcessTable`, `SessionManager`, `DeliveryRouter` | Simplifies graceful shutdown and test isolation |
| Circular dep between tools↔core broken by trait | `SubAgentRunner` and `GatewaySender` traits in tools | Core implements; tools defines |

---

## Deployment topologies

### Single process (most common)

```
  ┌───────────────────────────────────┐
  │  edgecrab (single binary)         │
  │                                   │
  │  CLI frontend + Agent + Gateway   │
  │  all in-process                   │
  └───────────────────────────────────┘
          │
          ▼
  ~/.edgecrab/state.db   (SQLite, WAL)
  ~/.edgecrab/config.yaml
```

### Managed / headless (EDGECRAB_MANAGED=1)

```
  ┌─────────────────┐   JSON events   ┌──────────────────┐
  │  Supervisor /   │ ──────────────► │  edgecrab process │
  │  orchestrator   │ ◄────────────── │  (headless)       │
  └─────────────────┘   stdout        └──────────────────┘
```

### ACP (editor integration)

```
  ┌─────────────────┐  JSON-RPC 2.0  ┌──────────────────┐
  │  VS Code /      │ ──────────────► │  edgecrab acp     │
  │  Zed / JetBrains│ ◄────────────── │  (stdio server)  │
  └─────────────────┘                └──────────────────┘
```

---

## Tips

> **Tip: Trace a feature by starting at the frontend.**
> Find `Agent::chat` or `Agent::run_conversation` in the frontend crate, then follow
> the call into `execute_loop` in `conversation.rs`. That single function is where
> all non-trivial behaviour lives.

> **Tip: Tests should use `AgentBuilder` without `state_db()`.**
> Omitting `state_db()` skips SQLite and makes unit tests fast. Use
> `ToolContext::test_context()` for tool-level tests.

> **Tip: Never import `edgecrab-core` from `edgecrab-tools`.**
> The dependency graph is strictly `tools → types/security/state`. Breaking this
> invariant creates a circular dependency.

---

## FAQ

**Q: Why is there a separate `edgecrab-cron` crate?**
Both `edgecrab-cli` (the `cron` subcommand) and `edgecrab-tools` (the
`manage_cron_jobs` tool) need schedule parsing and job storage. Factoring it out
avoids pulling CLI dependencies into the tools crate.

**Q: How does the gateway share an `Agent` with the CLI?**
It doesn't. Each `edgecrab gateway start` spawns its own process. Session state
is shared through the SQLite database on disk, not through a shared in-memory object.

**Q: Can I embed `edgecrab-core` in my own Rust application?**
Yes. `AgentBuilder::new(model)` is the entry point. You provide an `LLMProvider`
and optionally a `ToolRegistry` and `SessionDb`. See
[Agent Struct](../003_agent_core/001_agent_struct.md) for the full builder API.

---

## Cross-references

- Dependency graph details → [Crate Dependency Graph](./002_crate_dependency_graph.md)
- Concurrency model → [Concurrency Model](./003_concurrency_model.md)
- Error propagation → [Error Handling](./004_error_handling.md)
- The central `Agent` type → [Agent Struct](../003_agent_core/001_agent_struct.md)
- Tool dispatch internals → [Tool Registry](../004_tools_system/001_tool_registry.md)
