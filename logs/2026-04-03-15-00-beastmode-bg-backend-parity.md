# 2026-04-03 Session Task Log

## Actions
- Built env passthrough registry in `local.rs` with `RwLock<HashSet>`, `register_env_passthrough()`, `clear_env_passthrough()`, `is_env_passthrough()`, `_EDGECRAB_FORCE_<VAR>` mechanism; wired through config chain (TerminalConfig → AgentConfig → AppConfigRef → conversation.rs)
- Wired `register_env_passthrough()` in `skills.rs` from `required_environment_variables` frontmatter
- Fixed `clear_env_passthrough()` + re-register in `Agent::new_session()` to prevent cross-session stale registrations
- Changed `ProcessRecord.output_lines` from `Vec<String>` to `VecDeque<String>` for O(1) `pop_front()` eviction
- Applied `safe_env()` + `PYTHONUNBUFFERED=1` to `run_process` background process spawn (security gap)
- Added `cancel` token check in `wait_for_process` poll loop via `tokio::select!` (interrupt parity)
- Updated stale cron lib.rs parity table TBD entry (delivery routing was already implemented via GatewaySender)
- Added 4 unit tests for passthrough registry (register/clear, idempotent, safe_env passthrough, force-prefix)
- Documented `run_process` local-only limitation vs hermes's `spawn_via_env()` pattern

## Decisions
- Used global `RwLock<HashSet>` (not per-session) because subprocesses inherit env at session start; clearing on `new_session()` is the correct reset point
- Did NOT implement `run_process` backend routing (requires ToolContext + backend cache refactor); documented as known limitation
- Used `PASSTHROUGH_LOCK: std::sync::Mutex<()>` (same pattern as SUDO_ENV_LOCK) to serialize env-mutating tests

## Next Steps
- Backend routing for `run_process` would require adding `backend_cache` to `ToolContext`; medium-priority future work
- Final 1,111 tests pass across all crates

## Lessons/Insights
- `safe_env()` was applied to `PersistentShell` + `oneshot()` but NOT to `run_process` — a silent security gap where background processes inherited full process env including provider API keys
- VecDeque is the correct Rust type for bounded-size queues; Vec with remove(0) is O(n) and accumulates cost for chatty processes
- CancellationToken must be threaded into ALL blocking tool loops, not just the shell execution path
