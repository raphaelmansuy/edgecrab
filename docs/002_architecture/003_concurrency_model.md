# Concurrency Model 🦀

> **Verified against:** `crates/edgecrab-core/src/agent.rs` ·
> `crates/edgecrab-core/src/conversation.rs` ·
> `crates/edgecrab-tools/src/process_table.rs` ·
> `crates/edgecrab-gateway/src/session.rs` ·
> `crates/edgecrab-state/src/session_db.rs`

---

## Why the concurrency model is explicit

`hermes-agent` — EdgeCrab's Python predecessor — ran on asyncio; the CPython
GIL serialised all CPU-bound work across threads, so tool execution and
prompt assembly contended on a single core regardless of how many gateway
sessions were active. Startup cost: ~1–3 s, resident memory: ~80–150 MB.

OpenClaw ([github.com/openclaw](https://github.com/openclaw)) runs on Node.js's
single-threaded V8 event loop — excellent I/O concurrency but still no ability to
distribute CPU-bound prompt assembly or parallel tool execution across cores.

EdgeCrab picks the *right primitive for each use case*. Each choice below
is a deliberate answer to a specific contention pattern that emerges when
multiple users, gateway adapters, and background tools write to shared state
simultaneously.

---

## Runtime: Tokio multi-thread

All async code runs on a Tokio multi-threaded runtime with work-stealing.

```
  ┌─────────────────────────────────────────────────────────────────┐
  │  tokio::runtime::Builder::new_multi_thread()                    │
  │                                                                 │
  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐       │
  │  │ Worker 0 │  │ Worker 1 │  │ Worker 2 │  │ Worker N │       │
  │  │ ┌──────┐ │  │ ┌──────┐ │  │ ┌──────┐ │  │ ┌──────┐ │       │
  │  │ │tasks │ │  │ │tasks │ │  │ │tasks │ │  │ │tasks │ │       │
  │  │ └──────┘ │  │ └──────┘ │  │ └──────┘ │  │ └──────┘ │       │
  │  └──────────┘  └──────────┘  └──────────┘  └──────────┘       │
  │              work-stealing scheduler                            │
  └─────────────────────────────────────────────────────────────────┘
```

`tokio = { version = "1", features = ["full"] }` in `Cargo.toml`.

**Reference:** [Tokio tutorial](https://tokio.rs/tokio/tutorial)

---

## `Agent` field-by-field synchronisation

Every field in `Agent` that crosses an `.await` boundary is explicitly guarded.
Here is the complete map:

```
  ┌────────────────────────────────────────────────────────────────┐
  │  Agent fields                                                  │
  │                                                                │
  │  config          tokio::sync::RwLock<AgentConfig>              │
  │                  ↳ hot-swap model at runtime without restart   │
  │                                                                │
  │  provider        tokio::sync::RwLock<Arc<dyn LLMProvider>>     │
  │                  ↳ same: /model command swaps provider mid-    │
  │                    session without dropping the session        │
  │                                                                │
  │  gateway_sender  tokio::sync::RwLock<Option<Arc<dyn ...>>>     │
  │                  ↳ optional; attached after agent creation     │
  │                                                                │
  │  session         tokio::sync::RwLock<SessionState>             │
  │                  ↳ message history, token counters, session_id │
  │                                                                │
  │  budget          Arc<IterationBudget>                          │
  │                  ↳ AtomicU32 internally — lock-free           │
  │                                                                │
  │  cancel          std::sync::Mutex<CancellationToken>           │
  │                  ↳ held briefly for reset only; sync is fine   │
  │                                                                │
  │  state_db        Option<Arc<SessionDb>>                        │
  │                  ↳ Mutex<Connection> inside; WAL + jitter      │
  │                                                                │
  │  tool_registry   Option<Arc<ToolRegistry>>                     │
  │                  ↳ read-only after build(); no lock needed     │
  │                                                                │
  │  process_table   Arc<ProcessTable>                             │
  │                  ↳ DashMap<pid, ProcessHandle> inside          │
  │                                                                │
  │  todo_store      Arc<TodoStore>                                │
  │                  ↳ session-scoped todo list; Arc for tools     │
  └────────────────────────────────────────────────────────────────┘
```

---

## `IterationBudget` — lock-free counter

The per-turn iteration limit uses `AtomicU32` to avoid any lock:

```rust
pub struct IterationBudget {
    remaining: AtomicU32,
    max: u32,
}

impl IterationBudget {
    /// Compare-and-swap decrement. Returns false when exhausted.
    pub fn try_consume(&self) -> bool {
        let mut cur = self.remaining.load(Ordering::Relaxed);
        loop {
            if cur == 0 { return false; }
            match self.remaining.compare_exchange_weak(
                cur, cur - 1, Ordering::AcqRel, Ordering::Relaxed
            ) {
                Ok(_) => return true,
                Err(v) => cur = v,
            }
        }
    }
}
```

🦀 *When 18 gateway sessions fight concurrently, every mutex round-trip
is a potential delay. The budget check runs on every iteration — CAS on
an atomic is ~5–10× cheaper than a mutex lock.*

**Reference:** [Rust Atomics and Locks](https://marabos.nl/atomics/)

---

## `CancellationToken` — cooperative interrupt

```
  User presses Ctrl-C  or  gateway sends /stop
          │
          ▼
  Agent::interrupt()
    └── token.cancel()
          │
  ┌───────▼────────────────────────────────────────────┐
  │  execute_loop                                       │
  │  every iteration:                                   │
  │    if self.is_cancelled() { break }                 │
  └───────────────────────────────────────────────────-─┘
          │
  ┌───────▼────────────────────────────────────────────┐
  │  long-running tools (terminal, web_crawl, browser) │
  │  poll ctx.cancel.is_cancelled() in their inner     │
  │  loops and return early                            │
  └────────────────────────────────────────────────────┘
```

Two tokens per `Agent`:
- `cancel` — per-turn, reset on `new_session()`
- `gc_cancel` — background GC task lifetime, cancelled on `Agent::drop()`

**Reference:** [`tokio_util::sync::CancellationToken`](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html)

---

## SQLite: single connection + jitter retry

`SessionDb` uses `Arc<Mutex<Connection>>` — SQLite in WAL mode serialises
writers at the file level anyway, so a connection pool adds no benefit.

Multiple processes (CLI + gateway daemon) can share one `state.db` because WAL
provides file-level serialisation.

**The write-convoy problem:** if five concurrent tasks all retry a busy write
after the same fixed delay (e.g. 100 ms), they re-collide immediately. Solution:

```rust
const WRITE_MAX_RETRIES: u32 = 15;
const WRITE_RETRY_MIN_MS: u64 = 20;
const WRITE_RETRY_MAX_MS: u64 = 150;

// On SQLITE_BUSY:
let delay = rng.gen_range(WRITE_RETRY_MIN_MS..WRITE_RETRY_MAX_MS);
tokio::time::sleep(Duration::from_millis(delay)).await;
```

Random jitter from `[20, 150)` ms breaks the retry synchronisation.

Every `CHECKPOINT_EVERY_N_WRITES = 50` writes an explicit WAL checkpoint
to prevent unbounded WAL file growth.

**Reference:** [SQLite WAL mode](https://www.sqlite.org/wal.html) ·
[The convoy effect](https://en.wikipedia.org/wiki/Convoy_effect)

---

## Gateway sessions: `DashMap`

The gateway `SessionManager` serves concurrent messages from many users:

```rust
pub struct SessionManager {
    sessions: DashMap<SessionKey, Arc<RwLock<GatewaySession>>>,
    idle_timeout: Duration,
}
```

[`DashMap`](https://docs.rs/dashmap) shards the keyspace across `2× CPU thread`
segments. Lookups on different sessions are fully parallel. Each session is
additionally protected by `RwLock<GatewaySession>` so concurrent status reads
do not block each other.

```
  User A (Telegram)──►  shard 0 lock  ─►  session A (RwLock)  ─►  Agent A
  User B (Discord) ──►  shard 3 lock  ─►  session B (RwLock)  ─►  Agent B
  User C (Telegram)──►  shard 0 lock  ─►  session C (RwLock)  ─►  Agent C
                           (only A and C contend; B is independent)
```

---

## Parallel tool dispatch

Tools that declare `parallel_safe() → true` run concurrently within a single
LLM response:

```
  LLM response contains three tool calls:
  ───────────────────────────────────────
  web_search("rust async")  ─── parallel_safe=true  ────┐
  web_search("tokio docs")  ─── parallel_safe=true  ────┤ tokio::join!
  vision_analyze(image.png) ─── parallel_safe=true  ────┘

  write_file("a.rs")        ─── parallel_safe=false ─── sequential
  patch("b.rs", diff)       ─── parallel_safe=false ─── sequential
```

---

## Streaming events: unbounded MPSC

The `chat_streaming()` path sends events to the TUI via an
`UnboundedSender<StreamEvent>`:

```
  execute_loop task                        TUI render task
       │                                         │
       ├── StreamEvent::Token("Hello") ─────────►│ renders token
       ├── StreamEvent::ToolExec { .. } ─────────►│ shows spinner
       ├── StreamEvent::ToolDone { .. } ─────────►│ shows result
       └── StreamEvent::Done            ─────────►│ clears spinner
```

`UnboundedSender` never blocks the producer. If the TUI falls behind,
events queue in the channel.

---

## Don't-do list

| Anti-pattern | Why it breaks things |
|---|---|
| Hold `RwLock` guard across `.await` | Blocks other writers while the task yields; starvation |
| `std::sync::Mutex` in async code | Blocks the OS thread; starves other Tokio tasks |
| `thread_local!` for agent state | Work-stealing may resume the task on a different thread |
| Fixed retry delay on `SQLITE_BUSY` | Creates write convoy; see jitter section above |
| `unwrap()` in library crates | Crashes the process; `#![deny(clippy::unwrap_used)]` enforced in `edgecrab-types` |

---

## Tips

> **Tip: Clippy lint `clippy::await_holding_lock` catches guard-across-await.**
> Run `cargo clippy --all-targets -- -W clippy::await_holding_lock` in CI.

> **Tip: Long-running tools must poll `ctx.cancel.is_cancelled()`.**
> Any tool that loops (file watcher, process poller, browser crawler) must
> return `Err(ToolError::ExecutionFailed { .. })` when the token fires.

> **Tip: `spawn_blocking` for heavy filesystem work.**
> Recursive directory search and image shrinking block the CPU for tens of
> milliseconds. Wrap them in `tokio::task::spawn_blocking` to avoid
> starving other tasks.

---

## FAQ

**Q: Why `tokio::sync::RwLock` and not `parking_lot::RwLock`?**
`parking_lot` blocks OS threads. In a Tokio runtime, blocking a thread starves
other tasks on that worker. `tokio::sync::RwLock` yields back to the scheduler.

**Q: Can two messages from the same Telegram user run concurrently?**
No. They both map to the same `SessionKey` → same `Arc<RwLock<GatewaySession>>`.
The write lock serialises them. Different users run fully in parallel.

**Q: Is there any global mutable state?**
`ModelCatalog` uses a `OnceLock<RwLock<CatalogData>>` (process-global, but rarely
written after init). Everything else is scoped to `Agent` or `SessionManager`.

---

## Cross-references

- `Agent` fields detail → [Agent Struct](../003_agent_core/001_agent_struct.md)
- SQLite schema → [Session Storage](../009_config_state/002_session_storage.md)
- Gateway session fan-out → [Gateway Architecture](../006_gateway/001_gateway_architecture.md)
