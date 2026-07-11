# Agent 结构 🦀

> **已验证来源：** `crates/edgecrab-core/src/agent.rs`

---

## 为什么 `Agent` 是这样的结构

`Agent` 结构体中的每个设计决策都在回答一个问题：*"在多轮对话中什么能够存活下来？"*

- Config 和 provider 放在 `RwLock` 后面，因为 `/model` 命令可以在会话中途交换它们而不会结束对话。
- `ProcessTable` 和 `TodoStore` 由 agent 拥有，这样在第 3 轮启动的后台进程在第 10 轮仍然可追踪。
- `state_db` 是可选的，因为测试和 cron 运行不应该需要 SQLite 数据库才能工作。
- `budget` 使用原子操作，因为它在每次迭代时都会被检查，不能成为竞争瓶颈。

---

## 结构体字段

```rust
// crates/edgecrab-core/src/agent.rs
pub struct Agent {
    // 热切换：/model 命令在锁下写入这些字段
    pub(crate) config:          RwLock<AgentConfig>,
    pub(crate) provider:        RwLock<Arc<dyn LLMProvider>>,
    pub(crate) gateway_sender:  RwLock<Option<Arc<dyn GatewaySender>>>,

    // 对话历史、令牌计数器、缓存的系统提示
    pub(crate) session:         RwLock<SessionState>,

    // 可选 — 测试和 cron 可以跳过 SQLite
    pub(crate) state_db:        Option<Arc<SessionDb>>,

    // 构建后只读；无需锁即可安全访问
    pub(crate) tool_registry:   Option<Arc<ToolRegistry>>,

    // 后台进程在多次工具调用之间保持存活
    pub(crate) process_table:   Arc<ProcessTable>,

    // 无锁迭代预算（内部使用 AtomicU32）
    pub(crate) budget:          Arc<IterationBudget>,

    // 每轮取消（在新会话时重置）
    pub(crate) cancel:          Mutex<CancellationToken>,

    // 后台 GC 任务生命周期 — Drop 时取消
    pub(crate) gc_cancel:       CancellationToken,

    // 与工具共享的会话范围待办列表
    pub(crate) todo_store:      Arc<TodoStore>,
}
```

---

## `AgentConfig` 关键字段

```
  ┌──────────────────────────────────────────────────────────┐
  │  AgentConfig (默认值来自代码)                            │
  │                                                          │
  │  model                  "anthropic/claude-opus-4.6"      │
  │  max_iterations         90                               │
  │  streaming              true                             │
  │  platform               Platform::Cli                    │
  │  delegation_enabled     true                             │
  │  delegation_max_subagents  3                             │
  │  delegation_max_iterations 50                            │
  │  checkpoints_enabled    true                             │
  │  checkpoints_max_snapshots 50                            │
  │  terminal_backend       BackendKind::Local               │
  │                                                          │
  │  enabled_toolsets       Vec<String>  (空 = 全部)         │
  │  disabled_toolsets      Vec<String>                      │
  │  file_allowed_roots     Vec<PathBuf>                     │
  │  path_restrictions      Vec<PathBuf>                     │
  └──────────────────────────────────────────────────────────┘
```

---

## `SessionState` — 可变的对话状态

```rust
pub struct SessionState {
    pub session_id:                Option<String>,
    pub messages:                  Vec<Message>,
    pub cached_system_prompt:      Option<String>,
    pub user_turn_count:           u32,
    pub api_call_count:            u32,
    pub session_input_tokens:      u64,
    pub session_output_tokens:     u64,
    pub session_cache_read_tokens: u64,
    pub session_cache_write_tokens: u64,
    pub session_reasoning_tokens:  u64,
    pub session_tool_call_count:   u32,
}
```

`cached_system_prompt` 是性能关键字段。`PromptBuilder` 每个会话调用一次（或在明确调用 `invalidate_system_prompt()` 时）。两次调用之间，系统提示被逐字复用。

---

## `AgentBuilder` — 构造 `Agent`

```rust
// 最小可用构建器：
let agent = AgentBuilder::new("anthropic/claude-sonnet-4-20250514")
    .provider(Arc::new(my_provider))
    .build()?;

// 生产网关使用的完整构建器：
let agent = AgentBuilder::new(config.model.name.as_str())
    .from_config(&app_config)
    .provider(Arc::clone(&provider))
    .state_db(Arc::clone(&session_db))
    .tools(Arc::clone(&tool_registry))
    .platform(Platform::Telegram)
    .session_id(session_id.clone())
    .origin_chat(platform_str, chat_id)
    .streaming(true)
    .build()?;
```

如果从未调用 `.provider()`，`build()` 返回 `Err(AgentError::Config("no provider set"))` — 这是唯一必填字段。

---

## `IterationBudget`

```
  AgentConfig::max_iterations = 90  (默认)
        │
        ▼
  IterationBudget::new(90)
    remaining = AtomicU32(90)
        │
  每次迭代：
        ▼
  budget.try_consume()  → CAS 递减
    ├── true  → 继续循环
    └── false → AgentError::BudgetExhausted { used: 90, max: 90 }
                ConversationResult::budget_exhausted = true
```

---

## `StreamEvent` — 前端接收的内容

`Agent::chat_streaming()` 通过 `tokio::sync::mpsc::UnboundedSender<StreamEvent>` 发送这些事件：

```
  客户端 (TUI / gateway / ACP)               Agent 任务
        │                                         │
        │◄── StreamEvent::Token("Hello ")         │
        │◄── StreamEvent::Reasoning("let me ...")  │  (思考模型)
        │◄── StreamEvent::ToolExec { name, args } │
        │◄── StreamEvent::ToolDone { name, dur.. }│
        │◄── StreamEvent::ContextPressure { .. }  │  (压缩警告)
        │◄── StreamEvent::Clarify { question, tx }│  (agent 询问用户)
        │◄── StreamEvent::Approval { command, tx }│  (危险 shell 命令)
        │◄── StreamEvent::Done                    │
```

`Clarify` 和 `Approval` 携带一个 `oneshot::Sender<String>`（或 `oneshot::Sender<ApprovalChoice>`）— 前端通过通道将用户的响应发回，循环恢复。

---

## `ApprovalChoice`

```rust
pub enum ApprovalChoice {
    Once,     // 仅批准此次执行
    Session,  // 为本次会话批准所有相同的命令
    Always,   // 添加到永久白名单 (~/.edgecrab/approval.json)
    Deny,     // 阻止命令；模型看到 PermissionDenied 错误
}
```

---

## 公共 API 参考

`Agent` 最常用的方法：

| 方法 | 作用 |
|---|---|
| `chat(&str)` | 单轮对话，返回完整响应字符串 |
| `chat_in_cwd(&str, &Path)` | 指定工作目录的单轮对话 |
| `chat_streaming(&str, tx)` | 流式回合；向 `tx` 发送 `StreamEvent` |
| `run_conversation(user, sys, history)` | 提供自己的历史记录和系统提示 |
| `fork_isolated(opts)` | 克隆 agent，隔离会话用于子 agent 委托 |
| `interrupt()` | 信号协同取消 |
| `new_session()` | 清除历史记录和会话 ID，保留配置和 provider |
| `swap_model(model, provider)` | 热切换模型/provider 而不丢失历史 |
| `force_compress()` | 立即触发压缩 |
| `undo_last_turn()` | 从历史记录中移除最后一轮 assistant+user 对 |
| `restore_session(&str)` | 从 SQLite 加载会话到内存 |
| `session_snapshot()` | 复制当前会话用于检查点 |

---

## 生命周期图

```
  AgentBuilder::build()
        │
        ▼
  Agent 创建
   ├── gc 后台任务启动 (带 gc_cancel)
   │
   ├── chat() / chat_streaming()
   │       │
   │       ▼
   │   execute_loop()  [见对话循环文档]
   │       │
   │       ▼
   │   ConversationResult 返回
   │
   ├── new_session()  → 清除消息，重置 session_id
   │
   └── Agent::drop()  → 取消 gc_cancel → GC 任务停止
```

---

## 提示

> **Tip: `fork_isolated()` 创建一个子 agent，共享工具注册表和 state_db，但有自己的消息历史和取消令牌。**
> 用于 `delegate_task` — 子 agent 可以独立运行 50 轮迭代并返回 `SubAgentResult` 给父级。

> **Tip: 调用 `invalidate_system_prompt()` 强制下一轮从头重建。** 在 `/memory` 写入或技能安装后执行此操作，使新内容立即生效。

> **Tip: `session_snapshot()` 返回可克隆的结构，适合存储为检查点。** 与 `restore_session()` 配对实现类似撤销的回滚。

---

## 常见问题

**Q: 每个用户有一个 `Agent` 还是一个全局 `Agent`？**
每个逻辑会话一个。网关为每个 `(platform, user_id)` 对创建一个 `Agent`。CLI 为每个交互式会话创建一个。它们共享相同的 `ToolRegistry` 和 `SessionDb`（在 `Arc` 后面），但有独立的对话历史。

**Q: 为什么 `state_db` 是可选的？**
测试调用 `AgentBuilder::new(..).provider(..).build()` — 不需要数据库。Cron 运行通常也跳过持久化。只有需要会话历史的网关和 CLI 会话传递 `.state_db()`。

**Q: `Drop` 在 `Agent` 上做什么？**
它取消 `gc_cancel`，这向后台垃圾回收任务发出信号（修剪旧进程句柄和过期会话数据）以优雅停止。

---

## 交叉引用

- `execute_loop()` 实现的对话循环 → [对话循环](./002_conversation_loop.md)
- 系统提示组装 → [提示构建器](./003_prompt_builder.md)
- `RwLock` 使用的并发细节 → [并发模型](../002_architecture/003_concurrency_model.md)
- 传递给工具的 `ToolContext` → [工具运行时](../004_tools_system/004_tools_runtime.md)
