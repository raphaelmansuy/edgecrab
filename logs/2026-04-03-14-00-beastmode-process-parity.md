# Task Log — 2026-04-03 background/backend parity audit

## Actions
- Audited hermes-agent process_registry.py, gateway/run.py, gateway/stream_consumer.py, cron/scheduler.py vs edgecrab equivalents
- Identified 6 confirmed gaps in ProcessTable / process.rs
- Implemented all fixes in process_table.rs + process.rs; committed

## Decisions
- Use `libc::kill(pid, SIGKILL)` (already a workspace dep) instead of nix; simpler, no new dep
- `ProcessStatus` already derives `Clone`; add `pid: Option<u32>` — copyable, works with Clone
- Made `prune_if_full()` pub so `RunProcessTool::execute()` can call it before register
- Shell noise filtering: first-lines prefix scan in drain_reader (matches hermes logic)
- `spawn_gc_task()` added to ProcessTable for optional auto-GC; called with CancellationToken

## Gap closure summary
| Gap | Fixed |
|-----|-------|
| kill_process only marks record; never sends SIGKILL | ✅ libc::kill(pid, SIGKILL) |
| No get_process_output tool | ✅ GetProcessOutputTool |
| No wait_for_process tool | ✅ WaitForProcessTool |
| No MAX_PROCESSES LRU cap | ✅ MAX_PROCESSES=64, prune_if_full() |
| No auto-GC timer | ✅ spawn_gc_task() every 5 min, TTL=30 min |
| Shell noise polluting output | ✅ SHELL_NOISE filter in drain_reader |
| PID not stored in ProcessRecord | ✅ pid: Option<u32> + set_pid() |

## Features confirmed at parity or exceeding hermes-agent
- Gateway lifecycle (session management, platform restart, pending queue, commands)
- Stream consumer (edit mode + batch mode, overflow split, cursor, dedup)
- Event processor (typing keepalive, hook emission, tool progress)
- Cron (4 schedule formats, LRU, atomic writes, prompt injection scan, silent marker)
- Model router (smart routing, fallback)

## Next steps
- Wire `spawn_gc_task` into `Agent::new()` with the agent cancel token
- Consider adding `has_active_for_session(session_key)` to ProcessTable for gateway reset protection

## Lessons
- `tokio::process::Child::id()` returns `Option<u32>` — safe to call before `wait()`
- `libc` was already a workspace dep; no cargo.toml changes needed
