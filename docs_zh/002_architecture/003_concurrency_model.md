# 并发模型 🦀

> **已验证来源：** `crates/edgecrab-core/src/agent.rs` ·
> `crates/edgecrab-core/src/conversation.rs` ·
> `crates/edgecrab-tools/src/process_table.rs` ·
> `crates/edgecrab-gateway/src/session.rs` ·
> `crates/edgecrab-state/src/session_db.rs`

---

## 为什么并发模型是显式的

`hermes-agent` — EdgeCrab 的 Python 前身 — 运行在 asyncio 上；CPython GIL 在线程间序列化所有 CPU 密集型工作，因此无论有多少网关会话处于活动状态，工具执行和提示词组装都在单个核心上竞争。启动成本：约 1–3 秒，驻留内存：约 80–150 MB。

OpenClaw ([github.com/openclaw](https://github.com/openclaw)) 运行在 Node.js 的单线程 V8 事件循环上——出色的 I/O 并发，但仍然无法在核心之间分配 CPU 密集型提示词组装或并行工具执行。

EdgeCrab 为每个用例选择 *合适的原语*。下面的每个选择都是对特定竞争模式的深思熟虑的回答，这些模式在多个用户、网关适配器和后台工具同时写入共享状态时出现。

---

## 运行时：Tokio 多线程

所有异步代码都运行在带有工作窃取的 Tokio 多线程运行时上。

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

`Cargo.toml` 中的 `tokio = { version = "1", features = ["full"] }`。

**参考：** [Tokio tutorial](https://tokio.rs/tokio/tutorial)

---

## `Agent` 字段级同步

`Agent` 中跨越 `.await` 边界的每个字段都被显式保护。以下是完整映射：

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

## `IterationBudget` — 无锁计数器

每轮迭代限制使用 `AtomicU32` 以避免任何锁：

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

🦀 *当 18 个网关会话同时竞争时，每次互斥锁往返都是潜在的延迟。预算检查在每次迭代中运行——原子上的 CAS 比互斥锁便宜约 5–10 倍。*

**参考：** [Rust Atomics and Locks](https://marabos.nl/atomics/)

---

## `CancellationToken` — 协作式中断

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

每个 `Agent` 有两个 token：
- `cancel` — 每轮，在 `new_session()` 时重置
- `gc_cancel` — 后台 GC 任务生命周期，在 `Agent::drop()` 时取消

**参考：** [`tokio_util::sync::CancellationToken`](https://docs.rs/tokio-util/latest/tokio_util/sync/struct.CancellationToken.html)

---

## SQLite：单连接 + 抖动重试

`SessionDb` 使用 `Arc<Mutex<Connection>>` — SQLite 在 WAL 模式下无论如何都会在文件级别序列化写入者，因此连接池没有任何好处。

多个进程（CLI + 网关守护进程）可以共享一个 `state.db`，因为 WAL 提供文件级序列化。

**写车队问题：** 如果五个并发任务在相同的固定延迟（例如 100 毫秒）后都重试繁忙的写入，它们会立即重新碰撞。解决方案：

```rust
const WRITE_MAX_RETRIES: u32 = 15;
const WRITE_RETRY_MIN_MS: u64 = 20;
const WRITE_RETRY_MAX_MS: u64 = 150;

// On SQLITE_BUSY:
let delay = rng.gen_range(WRITE_RETRY_MIN_MS..WRITE_RETRY_MAX_MS);
tokio::time::sleep(Duration::from_millis(delay)).await;
```

来自 `[20, 150)` 毫秒的随机抖动打破了重试同步。

每 `CHECKPOINT_EVERY_N_WRITES = 50` 次写入会执行一次显式 WAL 检查点，以防止 WAL 文件无限制增长。

**参考：** [SQLite WAL mode](https://www.sqlite.org/wal.html) ·
[The convoy effect](https://en.wikipedia.org/wiki/Convoy_effect)

---

## 网关会话：`DashMap`

网关 `SessionManager` 服务来自多个用户的并发消息：

```rust
pub struct SessionManager {
    sessions: DashMap<SessionKey, Arc<RwLock<GatewaySession>>>,
    idle_timeout: Duration,
}
```

[`DashMap`](https://docs.rs/dashmap) 将键空间分片到 `2× CPU thread` 段。不同会话的查找是完全并行的。每个会话另外受 `RwLock<GatewaySession>` 保护，因此并发状态读取不会相互阻塞。

```
  User A (Telegram)──►  shard 0 lock  ─►  session A (RwLock)  ─►  Agent A
  User B (Discord) ──►  shard 3 lock  ─►  session B (RwLock)  ─►  Agent B
  User C (Telegram)──►  shard 0 lock  ─►  session C (RwLock)  ─►  Agent C
                           (only A and C contend; B is independent)
```

---

## 并行工具分发

声明 `parallel_safe() → true` 的工具在单个 LLM 响应中并发运行：

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

## 流事件：无界 MPSC

`chat_streaming()` 路径通过 `UnboundedSender<StreamEvent>` 向 TUI 发送事件：

```
  execute_loop task                        TUI render task
       │                                         │
       ├── StreamEvent::Token("Hello") ─────────►│ renders token
       ├── StreamEvent::ToolExec { .. } ─────────►│ shows spinner
       ├── StreamEvent::ToolDone { .. } ─────────►│ shows result
       └── StreamEvent::Done            ─────────►│ clears spinner
```

`UnboundedSender` 永远不会阻塞生产者。如果 TUI 落后，事件会在通道中排队。

---

## 不要做的事

| 反模式 | 为什么会破坏系统 |
|---|---|
| 在 `.await` 期间持有 `RwLock` guard | 任务让出时阻塞其他写入者；导致饥饿 |
| 在异步代码中使用 `std::sync::Mutex` | 阻塞 OS 线程；导致其他 Tokio 任务饥饿 |
| 为代理状态使用 `thread_local!` | 工作窃取可能在不同线程上恢复任务 |
| `SQLITE_BUSY` 时使用固定重试延迟 | 创建写车队；参见上面的抖动部分 |
| 在库 crate 中使用 `unwrap()` | 崩溃进程；`edgecrab-types` 中强制启用 `#![deny(clippy::unwrap_used)]` |

---

## 提示

> **提示：Clippy lint `clippy::await_holding_lock` 捕获 guard-across-await 问题。**
> 在 CI 中运行 `cargo clippy --all-targets -- -W clippy::await_holding_lock`。

> **提示：长期运行的工具必须轮询 `ctx.cancel.is_cancelled()`。**
> 任何循环的工具（文件监视器、进程轮询器、浏览器爬虫）都必须在 token 触发时返回 `Err(ToolError::ExecutionFailed { .. })`。

> **提示：使用 `spawn_blocking` 处理重型文件系统工作。**
> 递归目录搜索和图像缩小会阻塞 CPU 数十毫秒。将它们包装在 `tokio::task::spawn_blocking` 中以避免饥饿其他任务。

---

## 常见问题

**问：为什么使用 `tokio::sync::RwLock` 而不是 `parking_lot::RwLock`？**
`parking_lot` 阻塞 OS 线程。在 Tokio 运行时中，阻塞线程会导致该 worker 上的其他任务饥饿。`tokio::sync::RwLock` 会让出给调度器。

**问：来自同一 Telegram 用户的两条消息可以并发运行吗？**
不可以。它们都映射到相同的 `SessionKey` → 相同的 `Arc<RwLock<GatewaySession>>`。写锁将它们序列化。不同用户完全并行运行。

**问：有任何全局可变状态吗？**
`ModelCatalog` 使用 `OnceLock<RwLock<CatalogData>>`（进程全局，但初始化后很少写入）。其他所有状态都限定在 `Agent` 或 `SessionManager` 范围内。

---

## 交叉引用

- `Agent` 字段详情 → [Agent 结构](../003_agent_core/001_agent_struct.md)
- SQLite 架构 → [会话存储](../009_config_state/002_session_storage.md)
- 网关会话扇出 → [网关架构](../006_gateway/001_gateway_architecture.md)