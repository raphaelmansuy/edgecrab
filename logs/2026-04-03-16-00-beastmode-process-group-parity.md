# Beastmode Session Log — 2026-04-03-16-00

## Actions
- Deep audit: hermes-agent vs edgecrab across all backends (local, docker, ssh, modal, process registry)
- Fixed CRITICAL: process group kill — `.process_group(0)` + `kill(-pgid, SIGKILL)` so sh children are killed
- Added `kill_all(session_key)` to ProcessTable for gateway session reset protection
- Added `get_output_page(offset, limit)` to ProcessTable for output pagination parity with hermes `process(action="log")`
- Updated `GetProcessOutputTool` schema to expose `offset` param wired to `get_output_page`
- `list_processes` now shows `pid=<PID>` per entry
- 6 new unit tests; parity table updated in module doc

## Decisions
- Kept text output format for `get_process_output` (vs hermes's JSON) — LLM-friendly, avoids parsing overhead
- Used `.process_group(0)` not `.setsid()` (Rust/tokio only provides process_group; semantics equivalent for kill)
- Sent both `kill(-pgid)` and `kill(pid)` for belt-and-suspenders
- PTY support and crash-recovery checkpoints remain documented as known gaps (complex, gateway-specific)

## Next Steps
- Test `kill_all` in live gateway reset flow
- Consider exposing `kill_all` as a tool action or internal agent hook
- `spawn_via_env()` backend routing for bg procs (medium-priority future)

## Lessons
- Process group isolation is essential for sh -c wrappers: without `.process_group(0)`, killing the wrapper leaves children running
- offset=0 returns last N lines (tail) to match hermes convention; offset>0 paginates from oldest
- 1,117 tests pass (was 1,111)
