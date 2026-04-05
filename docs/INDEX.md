# EdgeCrab — Architecture & Implementation Reference

> **Version**: EdgeCrab v0.1.0 · verified against real source (code-is-law)
> **Language**: Rust 2024 edition · MSRV 1.85.0
> **LLM Backend**: [edgequake-llm](https://github.com/raphaelmansuy/edgequake-llm) v0.3.0+
> **Inspiration**: [OpenClaw](https://github.com/openai/openai-claw) and [Nous Hermes](https://nousresearch.com) agent design
> **License**: Apache-2.0
> **Last updated**: 2026-04-02

This document set is a **complete, cross-referenced specification** sufficient to build EdgeCrab
from scratch. Every module, trait, algorithm, data structure, and integration point is documented.
The goal is to build the best personal AI agent, combining lessons from OpenClaw's tool-centric
design and Nous Hermes's self-improving agent architecture — all expressed in idiomatic Rust.

---

## Design Thesis

EdgeCrab is purpose-built as the **best personal AI agent** — a Rust-native implementation that:

1. **Eliminates entire classes of bugs** via Rust's ownership model (no GIL, no async-bridge hacks, no thread-safety surprises)
2. **Achieves 10-50x lower memory footprint** via zero-copy parsing, static dispatch, and no runtime VM
3. **Provides true parallelism** for tool execution (Tokio multi-thread, rayon for CPU-bound work)
4. **Ships as a single static binary** (~15-25MB) — no Python, no pip, no virtualenv, no Node.js
5. **Maintains migration compatibility** with OpenClaw and Nous Hermes configs, memories, and skills

---

## Crate Workspace at a Glance

```
+------------------------------------------------------------------+
|  edgecrab-types        (no deps — base types for everyone)       |
|  edgecrab-security     (types: path jail, SSRF, command scan)    |
|  edgecrab-state        (SQLite WAL + FTS5 — session store)       |
|  edgecrab-tools        (50+ tools, ToolRegistry, ProcessTable)   |
|  edgecrab-core         (Agent, conversation loop, compression)   |
|  edgecrab-cli          (ratatui TUI, clap CLI, skin engine)      |
|  edgecrab-gateway      (14 platform adapters + API server)       |
|  edgecrab-acp          (ACP JSON-RPC 2.0 stdio adapter)          |
|  edgecrab-cron         (tokio-cron-scheduler wrapper)            |
|  edgecrab-migrate      (hermes-agent/OpenClaw import)            |
+------------------------------------------------------------------+
```

## Document Map

```
+=========================================================================+
|                    EDGECRAB ARCHITECTURE REFERENCE                       |
|              Code-is-law  ·  Verified against real source                |
+=========================================================================+
|                                                                         |
|  001_overview/                                                          |
|    001_project_summary.md ........... Scope, Rust advantages, parity   |
|                                                                         |
|  002_architecture/                                                      |
|    001_system_architecture.md ....... Layered crate architecture        |
|    002_crate_dependency_graph.md .... Workspace crate dependency DAG    |
|    003_concurrency_model.md ......... Tokio runtime, Send+Sync, tasks   |
|    004_error_handling.md ............ thiserror + anyhow strategy       |
|                                                                         |
|  003_agent_core/                                                        |
|    001_agent_struct.md .............. Agent/AgentConfig/SessionState    |
|    002_conversation_loop.md ......... execute_loop() — full algorithm   |
|    003_prompt_builder.md ............ 12-source system prompt pipeline  |
|    004_context_compression.md ....... Tool pruning + LLM summarization  |
|    005_smart_model_routing.md ....... Complexity routing + fallbacks    |
|                                                                         |
|  004_tools_system/                                                      |
|    001_tool_registry.md ............. inventory + trait-based registry  |
|    002_tool_catalogue.md ............ All 30+ tool schemas by toolset   |
|    003_toolset_composition.md ....... Toolset groups + resolution       |
|                                                                         |
|  005_cli/                                                               |
|    001_cli_architecture.md .......... ratatui TUI, clap, slash commands |
|    ── skin_engine ................... 7 skins, 15-key color palette     |
|    ── profile_management ............ Isolated per-profile home dirs    |
|    ── acp_integration ............... JSON-RPC 2.0 editor integration   |
|                                                                         |
|  006_gateway/                                                           |
|    001_gateway_architecture.md ...... 14 platform adapters + API server |
|    ── api_server .................... OpenAI-compat HTTP (port 8642)    |
|    ── session_management ........... Per-user sessions, idle cleanup    |
|                                                                         |
|  007_memory_skills/                                                     |
|    001_memory_skills.md ............. MEMORY.md, skills, FTS5 search   |
|                                                                         |
|  008_environments/                                                      |
|    001_environments.md .............. Terminal backends + RL envs       |
|                                                                         |
|  009_config_state/                                                      |
|    001_config_state.md .............. config.yaml schema, SQLite, cron  |
|                                                                         |
|  010_data_models/                                                       |
|    001_data_models.md ............... Messages, ToolCall, Usage, Cost   |
|                                                                         |
|  011_security/                                                          |
|    001_security.md .................. Threat model + all 6 modules      |
|                                                                         |
|  012_migration/                                                         |
|    001_migration_guide.md ........... hermes/OpenClaw → EdgeCrab path   |
|                                                                         |
|  013_library_selection/                                                 |
|    001_library_selection.md ......... Every crate + selection rationale |
|                                                                         |
|  014_implementation_plan/                                               |
|    001_implementation_plan.md ....... Phases, sub-phases, tasks         |
|                                                                         |
|  015_roadblocks/                                                        |
|    001_roadblocks.md ................ Anticipated migration issues       |
|                                                                         |
+=========================================================================+
```

## Cross-Reference Convention

- `[→ SEC.DOC#anchor]` links to another spec section (e.g., `[→ 003.002#loop-step-4]`)
- `[edgecrab: crate::module]` references an EdgeCrab Rust module
- `[cfg: key.subkey]` references a config.yaml path
- `[env: VAR_NAME]` references an environment variable
- `[crate: name]` references an external Rust crate
- `[trait: Name]` references a Rust trait definition
- `[schema: tool_name]` references a tool schema definition

## Navigation

| # | Section | Key Topics |
|---|---------|------------|
| [001](001_overview/001_project_summary.md) | Project Overview | Scope, goals, Rust advantages, feature parity matrix |
| [002](002_architecture/) | Architecture | Crate workspace, dependency DAG, concurrency, errors |
| [003](003_agent_core/) | Agent Core | Agent struct, conversation loop, prompts, compression |
| [004](004_tools_system/) | Tools System | Registry, schemas, toolsets, trait-based dispatch |
| [005](005_cli/001_cli_architecture.md) | CLI | ratatui TUI, clap CLI, commands, skin engine |
| [006](006_gateway/001_gateway_architecture.md) | Gateway | Tokio async gateway, 14 platform adapters |
| [007](007_memory_skills/001_memory_skills.md) | Memory & Skills | Persistent memory, skills hub, FTS5 search |
| [008](008_environments/001_environments.md) | Environments | Terminal backends, RL training, Atropos |
| [009](009_config_state/001_config_state.md) | Config & State | config.yaml, SQLite state, cron |
| [010](010_data_models/001_data_models.md) | Data Models | Messages, API modes, pricing |
| [011](011_security/001_security.md) | Security | Threat model, injection defense, approval |
| [012](012_migration/001_migration_guide.md) | Migration | OpenClaw/Nous Hermes → EdgeCrab path |
| [013](013_library_selection/001_library_selection.md) | Library Selection | Every crate choice + rationale |
| [014](014_implementation_plan/001_implementation_plan.md) | Implementation Plan | Phases, sub-phases, detailed tasks |
| [015](015_roadblocks/001_roadblocks.md) | Implementation Roadblocks | Anticipated challenges + mitigations |

## Design Decisions from Reference Agent Implementations

EdgeCrab draws design inspiration from OpenClaw and Nous Hermes. The following table maps
key capabilities to their EdgeCrab location:

| Capability | Design Decision | EdgeCrab Location |
|------------|----------------|-------------------|
| Plugin/skill system | SKILL.md format with YAML frontmatter | [→ 007.001#skills] |
| Checkpoint manager | Filesystem snapshots for rollback | [→ 009.001#checkpoints] |
| Context references | @file, @url, @diff, @staged, @git | [→ 003.003#context-references] |
| Smart model routing | Complexity-based provider selection | [→ 003.005#smart-routing-algorithm] |
| Browser automation | CDP headless Chrome via trait adapter | [→ 004.002#browser-tools] |
| Security scanner | Command deny-list + SSRF + path jail | [→ 011.001] |
| Skills hub | Remote registry + installation | [→ 007.001#skills-hub] |
| MCP integration | JSON-RPC 2.0 stdio + HTTP OAuth 2.1 | [→ 004.001#mcp-oauth] |
| Voice mode | Push-to-talk STT + TTS playback | [→ 004.002#voice-mode] |
| Gateway hooks | Async event hook scripts | [→ 006.001#hooks] |
| Gateway mirror | Cross-platform session mirroring | [→ 006.001#mirror] |
| Skin engine | 7 built-in skins, 15-key color palette | [→ 005.001#skin-engine-complete] |
| Profile isolation | Separate home dirs per profile | [→ 005.001#profile-management] |
