# EdgeCrab Documentation 🦀

> **Code is law.** Every claim in this tree is verified against the source at
> `crates/`. If it conflicts with the code, the code wins.

> 🦀 *"`hermes-agent` had the history. OpenClaw had the claws. EdgeCrab had both —
> plus a security scanner, 65 tools, and a 15 MB binary that starts in 50 ms.
> The bout was brief."*
>
> *(Note: `hermes-agent` is EdgeCrab's **Python** predecessor — `~/.hermes/`, `prompt_toolkit` TUI, ~80–150 MB. OpenClaw is a TypeScript/Node.js personal assistant — [github.com/openclaw](https://github.com/openclaw).)*

EdgeCrab is a Rust-native AI agent: a single static binary that runs a
ReAct tool loop, speaks to every major LLM provider, and serves three
frontends (terminal TUI, 18-platform gateway, editor ACP) from one shared
runtime.

---

## Why this documentation exists

Most AI-agent projects grow a stack of aspirational readme files that
diverge from the code within weeks. This tree takes the opposite approach:
it is a short, navigable map of what the workspace **actually does today**,
derived from source. Reading it should give you enough orientation to make
a change with confidence on day one.

---

## Workspace at a glance

```
  ┌─────────────────────────────────────────────────────────────┐
  │                   USER SURFACES                             │
  │  edgecrab-cli (TUI)  │  edgecrab-gateway  │  edgecrab-acp   │
  └──────────────────────┴────────────────────┴─────────────────┘
                                  │
                    ┌─────────────▼──────────────┐
                    │      edgecrab-core          │
                    │  Agent · Loop · Prompt ·    │
                    │  Compression · Routing      │
                    └─────────────┬──────────────┘
          ┌──────────────────────┬┴───────────────────────┐
          │                      │                        │
  ┌───────▼───────┐   ┌──────────▼──────┐   ┌────────────▼────┐
  │edgecrab-tools │   │ edgecrab-state  │   │edgecrab-security│
  │ 65 tools,     │   │ SQLite WAL/FTS5 │   │path·cmd·inject  │
  │ registry,     │   │ sessions, FTS   │   │redact·url·policy│
  │ toolsets      │   └─────────────────┘   └─────────────────┘
  └───────────────┘
          │
  ┌───────▼───────────────────────┐
  │       edgecrab-types          │
  │  Message · Tool · Error ·     │
  │  Usage · Cost · Trajectory    │
  └───────────────────────────────┘

  edgecrab-cron ─── schedule parsing + job store (shared by cli + tools)
  edgecrab-migrate ─ legacy hermes→edgecrab import helper
```

---

## Crate quick-reference

| Crate | What it owns | Key public type |
|---|---|---|
| `edgecrab-types` | Shared message/tool/error/cost types; leaf dep | `Message`, `AgentError`, `ToolError` |
| `edgecrab-security` | Path jail, command scan, injection check, redaction | `CommandScanner`, `ApprovalPolicy` |
| `edgecrab-state` | SQLite session store, FTS5 search, analytics | `SessionDb` |
| `edgecrab-cron` | Cron schedule parsing, job store, delivery | `CronJob`, `CronStore` |
| `edgecrab-tools` | Tool registry, 65 tools, toolsets, backends | `ToolRegistry`, `ToolHandler` |
| `edgecrab-core` | Agent, conversation loop, prompt builder, routing | `Agent`, `AgentBuilder` |
| `edgecrab-cli` | TUI, clap commands, setup wizard, doctor | `CliArgs`, all subcommands |
| `edgecrab-gateway` | 18-platform adapters, delivery, hooks, pairing | `PlatformAdapter`, `HookRegistry` |
| `edgecrab-acp` | JSON-RPC 2.0 stdio server for VS Code / Zed | `AcpServer` |
| `edgecrab-migrate` | One-time hermes migration helper | `MigrateReport` |

---

## Reading order

Pick the path that matches your goal.

### New contributor — read top to bottom

1. [Project Summary](./001_overview/001_project_summary.md) — what EdgeCrab is and why it exists
2. [System Architecture](./002_architecture/001_system_architecture.md) — layering and request path
3. [Crate Dependency Graph](./002_architecture/002_crate_dependency_graph.md) — what imports what
4. [Agent Struct](./003_agent_core/001_agent_struct.md) — the central runtime object
5. [Conversation Loop](./003_agent_core/002_conversation_loop.md) — the ReAct core
6. [Tool Registry](./004_tools_system/001_tool_registry.md) — how tools are dispatched
7. [Security](./011_security/001_security.md) — the guard rails

### Adding a tool

1. [Tool Registry](./004_tools_system/001_tool_registry.md) — `ToolHandler` trait
2. [Tool Catalogue](./004_tools_system/002_tool_catalogue.md) — existing tools to avoid duplication
3. [Toolset Composition](./004_tools_system/003_toolset_composition.md) — which toolset to join
4. [Tools Runtime](./004_tools_system/004_tools_runtime.md) — `ToolContext` and backends

### Adding a gateway platform

1. [Gateway Architecture](./006_gateway/001_gateway_architecture.md) — `PlatformAdapter` trait

### Working on the TUI / CLI

1. [CLI Architecture](./005_cli/001_cli_architecture.md)

### Debugging agent behaviour

1. [Conversation Loop](./003_agent_core/002_conversation_loop.md)
2. [Context Compression](./003_agent_core/004_context_compression.md)
3. [Smart Model Routing](./003_agent_core/005_smart_model_routing.md)
4. [Config and State](./009_config_state/001_config_state.md)

### Understanding persistence

1. [Session Storage](./009_config_state/002_session_storage.md)
2. [Data Models](./010_data_models/001_data_models.md)

---

## All pages

| # | Page | One-liner |
|---|---|---|
| 1 | [Project Summary](./001_overview/001_project_summary.md) | What EdgeCrab is |
| 2 | [System Architecture](./002_architecture/001_system_architecture.md) | Layers and request path |
| 3 | [Crate Dependency Graph](./002_architecture/002_crate_dependency_graph.md) | Who imports what |
| 4 | [Concurrency Model](./002_architecture/003_concurrency_model.md) | Tokio, shared state, locking |
| 5 | [Error Handling](./002_architecture/004_error_handling.md) | `AgentError`, `ToolError`, propagation |
| 6 | [Agent Struct](./003_agent_core/001_agent_struct.md) | Fields, builder, lifecycle |
| 7 | [Conversation Loop](./003_agent_core/002_conversation_loop.md) | ReAct loop from source |
| 8 | [Prompt Builder](./003_agent_core/003_prompt_builder.md) | System prompt assembly |
| 9 | [Context Compression](./003_agent_core/004_context_compression.md) | 5-pass compression pipeline |
| 10 | [Smart Model Routing](./003_agent_core/005_smart_model_routing.md) | Cheap / Primary / Fallback |
| 11 | [Tool Registry](./004_tools_system/001_tool_registry.md) | `ToolHandler` trait and dispatch |
| 12 | [Tool Catalogue](./004_tools_system/002_tool_catalogue.md) | All 65 tools |
| 13 | [Toolset Composition](./004_tools_system/003_toolset_composition.md) | Named sets and aliases |
| 14 | [Tools Runtime](./004_tools_system/004_tools_runtime.md) | `ToolContext`, execution backends |
| 15 | [CLI Architecture](./005_cli/001_cli_architecture.md) | Clap, ratatui, slash commands |
| 16 | [Gateway Architecture](./006_gateway/001_gateway_architecture.md) | 18 adapters, hooks, delivery |
| 17 | [Memory and Skills](./007_memory_skills/001_memory_skills.md) | `~/.edgecrab/memories/`, skill files |
| 18 | [Creating Skills](./007_memory_skills/002_creating_skills.md) | Writing and testing skill files |
| 19 | [Execution Backends](./008_environments/001_environments.md) | Local, Docker, SSH, Modal, Daytona |
| 20 | [Config and State](./009_config_state/001_config_state.md) | `AppConfig`, resolution order |
| 21 | [Session Storage](./009_config_state/002_session_storage.md) | SQLite schema, WAL, FTS5 |
| 22 | [Data Models](./010_data_models/001_data_models.md) | All public types |
| 23 | [Security](./011_security/001_security.md) | All security primitives |
| 24 | [Library Selection](./013_library_selection/001_library_selection.md) | Why each dependency |
| 25 | [CI/CD Secrets](./016_cicd/001_secrets_setup.md) | GitHub Actions secrets |
| 26 | [GitHub Pages DNS](./016_cicd/002_github_pages_dns.md) | DNS setup |
| 27 | [Hooks](./hooks.md) | Native and script hooks |

---

## Editorial rules

- Every claim must be traceable to source code.
- Diagrams show what exists, not what is planned.
- Delete stale sections rather than leaving them with a "TODO" note.
- If in doubt, look at the code.
