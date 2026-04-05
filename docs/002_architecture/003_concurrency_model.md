# 002.003 — Concurrency Model

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 002.001 Architecture](001_system_architecture.md) | [→ 015.001 Roadblocks](../015_roadblocks/001_roadblocks.md)

## 1. Why Native Async

Python-based agent frameworks accumulate async/threading workarounds over time:
- `_run_async()` bridges to call async from sync contexts
- Persistent event loops to prevent "Event loop is closed" errors
- Per-worker-thread loop storage for parallel delegation
- `ThreadPoolExecutor` for CPU-bound tasks
- Pipe-write error guards for subprocess output

EdgeCrab **has none of these** because Rust's ownership model and Tokio's
work-stealing async runtime handle all concurrency correctly by default.

## 2. Runtime: Tokio Multi-Threaded

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
```

```rust
#[tokio::main]
async fn main() {
    // Multi-threaded runtime with work-stealing scheduler
    // Default: num_cpus threads
    edgecrab_cli::run().await;
}
```

### Why Tokio over async-std

| Criterion | Tokio | async-std |
|-----------|-------|-----------|
| Ecosystem | Dominant (axum, reqwest, tonic, teloxide) | Smaller |
| Performance | Best-in-class work-stealing | Good |
| edgequake-llm | Uses tokio internally | Would require compat layer |
| Features | io-uring, tracing integration | Basic |

## 3. Send + Sync Guarantees

All core types enforce thread safety at compile time:

```rust
// Agent is Send + Sync — can be shared across threads
pub struct Agent {
    provider: Arc<dyn LLMProvider>,      // Arc<dyn Trait> is Send+Sync
    tools: Arc<ToolRegistry>,             // Shared read-only tool registry
    state: Arc<RwLock<AgentState>>,       // RwLock for mutable state
    callbacks: Arc<dyn AgentCallbacks>,   // Callbacks must be Send+Sync
    budget: Arc<IterationBudget>,         // Atomic iteration counter
}

// IterationBudget uses atomics — no mutex needed
pub struct IterationBudget {
    max_total: u32,
    used: AtomicU32,
}

impl IterationBudget {
    pub fn consume(&self) -> bool {
        // CAS loop — lock-free
        loop {
            let current = self.used.load(Ordering::Acquire);
            if current >= self.max_total { return false; }
            if self.used.compare_exchange(
                current, current + 1,
                Ordering::Release, Ordering::Relaxed
            ).is_ok() {
                return true;
            }
        }
    }
}
```

## 4. Tool Execution Parallelism

```
hermes-agent (Python):                EdgeCrab (Rust):
───────────────────────                ──────────────────
ThreadPoolExecutor(1)                  tokio::spawn per tool
  → asyncio.run() per call               → native async execution
  → GIL prevents true parallel            → true parallel I/O
  → _run_async() bridge                   → no bridge needed
  → _worker_thread_local loops            → no loop management
```

### Parallel Tool Dispatch

```rust
// When LLM returns multiple tool calls, execute in parallel
let results: Vec<ToolResult> = if tool_calls.len() > 1 && can_parallelize(&tool_calls) {
    let futs = tool_calls.iter().map(|tc| {
        let registry = self.tools.clone();
        let ctx = tool_ctx.clone();
        tokio::spawn(async move {
            registry.dispatch(&tc.name, &tc.args, &ctx).await
        })
    });
    futures::future::join_all(futs).await
        .into_iter()
        .map(|r| r.expect("tool task panicked"))
        .collect()
} else {
    // Sequential execution for tools with side effects
    let mut results = Vec::new();
    for tc in &tool_calls {
        results.push(self.tools.dispatch(&tc.name, &tc.args, &tool_ctx).await);
    }
    results
};
```

## 5. Cancellation via CancellationToken

EdgeCrab uses `tokio_util::sync::CancellationToken` for cooperative interrupt handling:

```rust
use tokio_util::sync::CancellationToken;

pub struct Agent {
    cancel: CancellationToken,
    // ...
}

impl Agent {
    pub async fn run_conversation(&self, msg: &str) -> Result<ConversationResult> {
        loop {
            tokio::select! {
                response = self.call_llm(&messages) => {
                    // Process response...
                }
                _ = self.cancel.cancelled() => {
                    return Ok(ConversationResult::interrupted());
                }
            }
        }
    }

    pub fn interrupt(&self) {
        self.cancel.cancel();
    }
}
```

## 6. Structured Concurrency

All spawned tasks are tracked via `JoinSet` or `TaskTracker`:

```rust
use tokio::task::JoinSet;

// Gateway: track all platform adapter tasks
let mut tasks = JoinSet::new();
for adapter in adapters {
    tasks.spawn(adapter.run(cancel.child_token()));
}

// Graceful shutdown: cancel all, then await completion
cancel.cancel();
while let Some(result) = tasks.join_next().await {
    if let Err(e) = result {
        tracing::error!("Adapter task panicked: {e}");
    }
}
```

## 7. No Unsafe Required

The entire concurrency model uses safe Rust:
- `Arc<T>` for shared ownership
- `RwLock<T>` for mutable shared state (tokio::sync version for async)
- `AtomicU32` / `AtomicBool` for lock-free counters
- `mpsc` channels for message passing
- `CancellationToken` for cooperative cancellation
- `JoinSet` for structured task management

Zero `unsafe` blocks in the concurrency layer.
