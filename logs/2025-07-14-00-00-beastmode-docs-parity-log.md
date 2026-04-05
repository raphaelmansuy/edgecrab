# Task Log — 2025-07-14 — Docs & Code Parity (Session 2)

## Actions
- Created `docs/009_config_state/002_session_storage.md` — full SQLite WAL/FTS5 session storage dev guide
- Added hook lifecycle section (§10) to `docs/006_gateway/001_gateway_architecture.md` — event catalogue, delivery path, HOOK.yaml format, Python + Rust hook examples, discovery algorithm
- Created `docs/007_memory_skills/002_creating_skills.md` — full SKILL.md format guide, frontmatter reference, conditional activation, progressive disclosure tiers, skill_manage actions, discovery algorithm, preloaded skills, security considerations
- Created `docs/004_tools_system/004_tools_runtime.md` — compile-time inventory registration, dispatch flow, ToolContext fields, two-layer gating, async/spawn_blocking pattern, fuzzy matching, error handling, security boundaries, new-tool tutorial
- Fixed `StreamEvent::ContextPressure` match exhaustiveness in `e2e_copilot.rs`, `event_processor.rs`, and `app.rs`

## Decisions
- Gateway event_processor logs ContextPressure as `tracing::warn!` (no user-visible message — agent compresses automatically)
- CLI app.rs does the same (tracing::warn! only)
- All doc sources verified against real Rust source before writing

## Next steps
- Optional: add `ContextPressure` TUI indicator (yellow bar showing estimated vs threshold)
- Optional: `session_storage` doc could add migration path for schema v7 when ready

## Lessons/Insights
- Adding a new enum variant to `StreamEvent` requires updating all match sites across the workspace (core tests, gateway event_processor, cli app) — Rust catches these at compile time
- `cargo check` (not just `cargo check -p <crate>`) is needed to catch cross-crate match exhaustiveness
