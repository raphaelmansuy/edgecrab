# Task Logs — 2025-07-16 Phase 4 Completion

## Actions
- Updated `edgecrab-migrate/src/lib.rs` to declare `hermes`, `report`, `compat` modules
- Added `dirs` workspace dependency to `edgecrab-migrate/Cargo.toml`
- Created `edgecrab-migrate/src/compat.rs` — env var compatibility layer (2 tests)
- Built + tested migration crate: 8 tests passing
- Created `edgecrab-acp/src/protocol.rs` — JSON-RPC 2.0 wire types + ACP structs (5 tests)
- Created `edgecrab-acp/src/session.rs` — DashMap-backed session manager (7 tests)
- Created `edgecrab-acp/src/permission.rs` — tool approval bridge + ACP tool filtering (6 tests)
- Created `edgecrab-acp/src/server.rs` — JSON-RPC stdio server with full dispatch (14 tests)
- Updated `edgecrab-acp/src/lib.rs` to declare all 4 modules
- Fixed clippy `filter_map` → `map` in server.rs
- Committed Phase 4.3+4.5: 13 files, 1829 insertions
- Updated plan.md with Phase 4 checkboxes + Phase 5 gap analysis
- Cleaned up stale "Phase 3" references → "Phase 5" in CLI crate
- Committed gap analysis: 4 files, 32 insertions

## Decisions
- Used DashMap for ACP session manager (consistent with gateway pattern)
- ACP server uses JSON-RPC 2.0 over stdio (matching hermes-agent pattern)
- ACP tool filtering is static (`ACP_TOOLS` const) per spec in 004_tools_system/003_toolset_composition.md
- Deferred actual LLM dispatch in ACP prompt handler to Phase 5

## Next Steps
- Phase 5 integration: wire Agent.chat() into CLI, Gateway, and ACP dispatch loops
- Implement web tool HTTP backends
- Implement process management shared table
- README.md and architecture documentation

## Lessons/Insights
- The crate dependency chain (types → security → state → tools → core → {cli,gateway,acp}) keeps compilation fast and parallel
- 286 tests across 9 crates with zero clippy warnings
- 9 git commits total on master branch
