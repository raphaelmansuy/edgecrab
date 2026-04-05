# Concurrency Model

Verified against:
- `crates/edgecrab-core/src/agent.rs`
- `crates/edgecrab-core/src/conversation.rs`
- `crates/edgecrab-tools/src/process_table.rs`
- `crates/edgecrab-tools/src/tools/terminal.rs`
- `crates/edgecrab-gateway/src/session.rs`
- `crates/edgecrab-state/src/session_db.rs`

EdgeCrab uses Tokio, but the interesting part is not "it is async." The interesting part is that each part of the runtime uses a different concurrency primitive for a specific reason, and those choices show up directly in the code shape.

## Main primitives

- `tokio::sync::RwLock` for hot-swappable agent state such as config, provider, gateway sender, and sessions.
- `tokio_util::sync::CancellationToken` for cooperative cancellation of turns and long-running tools.
- `DashMap` for high-contention registries such as process tables and gateway session maps.
- `tokio::sync::mpsc` and `oneshot` for streaming events, clarify requests, and approvals.
- `std::sync::Mutex` around SQLite and a few startup caches where async locking is unnecessary.

## Where concurrency is used

In plain terms, concurrency shows up in three places: multiple frontends, multiple sessions, and long-running tool work.

```text
frontends run concurrently
  -> each session owns an Agent
  -> each Agent may spawn tool work
  -> gateway runs one adapter task per platform
  -> process GC and stream consumers run in background tasks
```

## Important boundaries

These boundaries are easy to break by accident during refactors.

- `Agent` snapshots config and provider at the start of a turn so `/model` changes do not mutate an in-flight conversation.
- `ProcessTable` is session-scoped. Background processes are shared within one agent session, not globally across all agents.
- `SessionDb` uses WAL plus jittered retry to tolerate concurrent readers and occasional writer contention.
- Blocking filesystem-heavy work is explicitly moved to `spawn_blocking` where needed, such as recursive search and image shrinking.

## What to preserve when changing this code

- Keep cancellation explicit. The code depends on cooperative checks, not forced task abortion.
- Do not hold `DashMap` or mutex guards across `.await`.
- Keep tool and gateway channels bounded or intentionally unbounded on purpose; the current code uses both depending on the interaction pattern.
