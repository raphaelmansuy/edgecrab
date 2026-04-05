# Task Log — 2026-04-02-00-03 — edgecrab/docs Full Update

## Actions
- Full rewrite of `docs/INDEX.md` — added Crate Workspace box, fixed all cross-reference paths
- Full rewrite of `docs/003_agent_core/001_agent_struct.md` — real Agent struct (RwLock fields, Mutex<CancellationToken>), AgentConfig 20+ fields
- Full rewrite of `docs/003_agent_core/003_prompt_builder.md` — 12-source pipeline, SOUL.md walk-up, skills cache
- Full rewrite of `docs/003_agent_core/004_context_compression.md` — SUMMARY_PREFIX constant, CompressionParams defaults
- Full rewrite of `docs/003_agent_core/005_smart_model_routing.md` — COMPLEX_KEYWORDS array, provider alias map
- Full rewrite of `docs/004_tools_system/001_tool_registry.md` — ToolHandler trait, inventory registration, SubAgentRunner
- Full rewrite of `docs/004_tools_system/002_tool_catalogue.md` — per-toolset tables, ToolContext struct, ~50+ tools
- Full rewrite of `docs/005_cli/001_cli_architecture.md` — skin engine (7 skins), profiles (ProfileManager), ACP integration, 42+ slash commands
- Full rewrite of `docs/006_gateway/001_gateway_architecture.md` — API server (port 8642), 14-platform adapter table, session management
- Update of `docs/009_config_state/001_config_state.md` — profiles/ dir layout, AppConfig schema, env overrides, cron jobs; removed 170 lines of duplicate content; fixed section numbering
- Update of `docs/002_architecture/002_crate_dependency_graph.md` — added `edgecrab-cron` to DAG, fixed build order from 8 to 10 steps
- Fixed broken cross-references: `001_rust_library_selection.md` → `001_library_selection.md`, `001_python_to_rust_roadblocks.md` → `001_roadblocks.md`

## Decisions
- "Code is law": verified all doc content against actual Rust source files before writing
- Kept existing comprehensive docs as-is where they were already accurate (002_architecture/001, 007_memory_skills, 008_environments, 010-015)
- Removed duplicate content in config_state.md rather than keeping both versions

## Next Steps
- Consider adding real implementation verification of `edgecrab-cron/src/` internals if that crate has significant logic
- The `docs/003_agent_core/002_conversation_loop.md` was already comprehensive and not changed

## Lessons/Insights
- When doing a replace_string_in_file on a section that doesn't replace the whole file, always check if the old content below the replacement target is still present (it was, causing duplicates in config_state.md)
- Section numbering fixes require bottom-up substitution (high numbers first) to avoid double-substitution
