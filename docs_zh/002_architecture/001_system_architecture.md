# 系统架构

> **已验证来源：** `Cargo.toml` · `crates/edgecrab-core/src/lib.rs` ·
> `crates/edgecrab-tools/src/lib.rs` · `crates/edgecrab-cli/src/main.rs` ·
> `crates/edgecrab-gateway/src/lib.rs` · `crates/edgecrab-acp/src/lib.rs`

---

## 为什么采用此架构

明显的替代方案——每个前端嵌入自己的代理循环——会产生相同提示词组装代码的 N 个副本、N 种工具分发方法和 N 个数据库。当您修复安全漏洞或调优上下文压缩时，必须应用 N 次。

EdgeCrab 通过使 `edgecrab-core::Agent` 成为所有代理行为的单一事实来源来反转这种模式。每个前端都变成一个薄薄的适配器，将用户输入序列化为 `Agent::chat()` 调用，并将 `ConversationResult` 反序列化为平台原生输出。

---

## 层次结构图

```
  ╔══════════════════════════════════════════════════════════════════╗
  ║  FRONTEND LAYER                                                  ║
  ║                                                                  ║
  ║  ┌─────────────────┐  ┌──────────────────┐  ┌───────────────┐  ║
  ║  │  edgecrab-cli   │  │ edgecrab-gateway  │  │ edgecrab-acp  │  ║
  ║  │                 │  │                  │  │               │  ║
  ║  │ clap subcommands│  │ 15 gateway        │  │ JSON-RPC 2.0  │  ║
  ║  │ ratatui TUI     │  │ adapters          │  │ stdio server  │  ║
  ║  │ slash commands  │  │ delivery router   │  │ VS Code / Zed │  ║
  ║  │ setup wizard    │  │ hook registry     │  │ JetBrains     │  ║
  ║  │ doctor          │  │ session fan-out   │  │               │  ║
  ║  └────────┬────────┘  └────────┬─────────┘  └───────┬───────┘  ║
  ╚═══════════╪════════════════════╪═══════════════════════╪════════╝
              │                   │                       │
              └───────────────────┼───────────────────────┘
                                  │
                    Agent::chat() / chat_streaming()
                    Agent::run_conversation()
                                  │
  ╔═══════════════════════════════▼══════════════════════════════════╗
  ║  CORE RUNTIME LAYER                                              ║
  ║                                                                  ║
  ║  ┌──────────────────────────────────────────────────────────┐   ║
  ║  │  edgecrab-core                                           │   ║
  ║  │                                                          │   ║
  ║  │  Agent          AgentBuilder     PromptBuilder           │   ║
  ║  │  execute_loop   compression      SmartRouter             │   ║
  ║  │  IterationBudget AppConfig        ModelCatalog           │   ║
  ║  └───────────────────┬──────────────────────────────────────┘   ║
  ║                      │ uses                                      ║
  ║  ┌───────────────────▼──────────┐  ┌────────────────────────┐   ║
  ║  │  edgecrab-tools               │  │  edgecrab-state        │   ║
  ║  │  ToolRegistry (91 tools)      │  │  SessionDb             │   ║
  ║  │  ToolHandler trait            │  │  SQLite WAL + FTS5     │   ║
  ║  │  ToolContext                  │  │  schema v6             │   ║
  ║  │  ProcessTable                 │  └────────────────────────┘   ║
  ║  │  toolset resolution           │                               ║
  ║  └───────────────────┬──────────┘                               ║
  ║                      │ uses                                      ║
  ║  ┌───────────────────▼──────────┐  ┌────────────────────────┐   ║
  ║  │  edgecrab-security            │  │  edgecrab-cron         │   ║
  ║  │  CommandScanner               │  │  schedule parsing      │   ║
  ║  │  path_jail                    │  │  CronStore             │   ║
  ║  │  injection check              │  │  TickLock              │   ║
  ║  │  ApprovalPolicy               │  └────────────────────────┘   ║
  ║  └──────────────────────────────┘                               ║
  ╚══════════════════════════════════════════════════════════════════╝
              │
  ╔═══════════▼══════════════════════════════════════════════════════╗
  ║  TYPE FOUNDATION                                                 ║
  ║                                                                  ║
  ║  edgecrab-types                                                  ║
  ║  Message · Role · Content · ToolCall · ToolSchema                ║
  ║  AgentError · ToolError · Usage · Cost · Trajectory              ║
  ║  Platform · ApiMode · DEFAULT_MODEL                              ║
  ╚══════════════════════════════════════════════════════════════════╝
```

---

## 各层职责

决定新代码应该放在哪里时，将这些所有权规则视为约束：

### 基础层 (`edgecrab-types`)
稳定的共享类型。每个其他 crate 都导入这个。**不要在此添加运行时逻辑。** 只包含结构体、枚举及其 `impl` 块。强制启用 `#![deny(clippy::unwrap_used)]`。

### 安全 (`edgecrab-security`)
可重用的无状态策略检查。回答"这安全吗？"问题的函数和结构。不包含代理逻辑、LLM 调用或异步运行时。被 `edgecrab-tools` 和 `edgecrab-core` 共同使用。

### 持久化 (`edgecrab-state`)
拥有 SQLite 架构和所有 SQL。此 crate 之外没有任何代码执行原始 SQL。会话记录、消息、FTS5 索引、分析查询和架构迁移都在这里。

### 调度 (`edgecrab-cron`)
由 cron CLI 命令和 `manage_cron_jobs` 工具共享的调度解析和作业存储。隔离设计，避免 CLI 和工具互相拉入对方的调度依赖。

### 工具 (`edgecrab-tools`)
定义 `ToolHandler`、`ToolRegistry`、`ToolContext` 和 `ProcessTable`。所有 65 个工具实现都在这里。**不**拥有代理循环——子代理委托通过 `SubAgentRunner` trait 表达，这样 `edgecrab-core` 可以实现它而不会产生循环依赖。

### 核心运行时 (`edgecrab-core`)
拥有 `Agent`、`AgentBuilder`、对话循环 (`execute_loop`)、上下文压缩、智能模型路由和提示词组装。这是唯一调用 LLM 提供商的 crate。为 `edgecrab-tools` 实现 `SubAgentRunner`。

### 前端 (`edgecrab-cli`, `edgecrab-gateway`, `edgecrab-acp`)
薄薄的适配器。它们通过 `AgentBuilder` 构建 `Agent`，将用户输入传递给 `Agent::chat()` 或 `Agent::chat_streaming()`，并渲染结果。它们不实现自己的工具分发或提示词组装。

---

## 端到端请求路径（带注释）

```
  Terminal / Telegram / VS Code
          │
          │ raw string "find all TODO comments"
          ▼
  ┌─────────────────────────────────────────┐
  │  Frontend                               │
  │  ■ resolves session key                 │
  │  ■ looks up or creates GatewaySession   │
  │  ■ invokes Agent::chat_streaming()      │
  └───────────────────┬─────────────────────┘
                      │
                      ▼
  ┌─────────────────────────────────────────┐
  │  Agent::execute_loop()                  │
  │                                         │
  │  [expansion]                            │
  │    expand_context_refs("@./src/")        │
  │                                         │
  │  [routing]                              │
  │    classify_message() → TurnRoute       │
  │    resolve_turn_route() → swap model    │
  │                                         │
  │  [prompt]                               │
  │    PromptBuilder::build()               │
  │    → load memory files                  │
  │    → load skill summaries              │
  │    → inject context files              │
  │                                         │
  │  [budget check]                         │
  │    IterationBudget::try_consume()       │
  │                                         │
  │  [compression]                          │
  │    check_compression_status()           │
  │    maybe: compress_with_llm()           │
  └───────────────────┬─────────────────────┘
                      │
                      ▼
  ┌─────────────────────────────────────────┐
  │  edgequake-llm provider call            │
  │  (up to 3× retry with exponential       │
  │   backoff: 500 ms base)                 │
  └───────────────────┬─────────────────────┘
                      │
          ┌───────────┴──────────┐
          │                      │
          ▼ tool_calls           ▼ assistant text
  ┌────────────────┐    ┌────────────────────┐
  │  security gate │    │  emit StreamEvent  │
  │  approval gate │    │  ::Done            │
  │  ToolHandler   │    │                    │
  │  ::execute()   │    │  persist session   │
  │  emit events   │    │  to SQLite         │
  │  loop ◄────────┘    └────────────────────┘
  └────────────────┘
          │ ConversationResult
          ▼
  ┌─────────────────────────────────────────┐
  │  Frontend renders / delivers response    │
  └─────────────────────────────────────────┘
```

---

## 代码中可见的设计约束

这些约束目前已强制执行，更改它们会产生连锁反应：

| 约束 | 位置 | 含义 |
|---|---|---|
| 提示词组装集中化 | `edgecrab-core` 中的 `PromptBuilder` | 调用者不得手动构建系统提示词 |
| 工具分发集中化 | `ToolRegistry::dispatch()` | 代理循环中不允许内联工具执行 |
| 会话持久化可选但标准化 | `SessionDb` 通过 `AgentBuilder::state_db()` 可选启用 | 测试可跳过数据库；网关始终启用 |
| 前端共享配置结构 | `AgentConfig` 和 `AppConfig` | 配置合并 (`merge_cli`) 在所有地方工作方式相同 |
| 长期运行的副作用使用显式句柄 | `ProcessTable`、`SessionManager`、`DeliveryRouter` | 简化优雅关闭和测试隔离 |
| 工具↔核心之间的循环依赖通过 trait 打破 | `SubAgentRunner` 和 `GatewaySender` trait 在 tools 中定义 | 核心实现；工具定义 |

---

## 部署拓扑

### 单进程（最常见）

```
  ┌───────────────────────────────────┐
  │  edgecrab (single binary)         │
  │                                   │
  │  CLI frontend + Agent + Gateway   │
  │  all in-process                   │
  └───────────────────────────────────┘
          │
          ▼
  ~/.edgecrab/state.db   (SQLite, WAL)
  ~/.edgecrab/config.yaml
```

### 托管/无头模式 (EDGECRAB_MANAGED=1)

```
  ┌─────────────────┐   JSON events   ┌──────────────────┐
  │  Supervisor /   │ ──────────────► │  edgecrab process │
  │  orchestrator   │ ◄────────────── │  (headless)       │
  └─────────────────┘   stdout        └──────────────────┘
```

### ACP（编辑器集成）

```
  ┌─────────────────┐  JSON-RPC 2.0  ┌──────────────────┐
  │  VS Code /      │ ──────────────► │  edgecrab acp     │
  │  Zed / JetBrains│ ◄────────────── │  (stdio server)  │
  └─────────────────┘                └──────────────────┘
```

---

## 提示

> **提示：从前端开始追踪功能。**
> 在前端 crate 中找到 `Agent::chat` 或 `Agent::run_conversation`，然后跟随调用进入 `conversation.rs` 中的 `execute_loop`。所有非平凡行为都在这个单一函数中。

> **提示：测试应使用不带 `state_db()` 的 `AgentBuilder`。**
> 省略 `state_db()` 会跳过 SQLite，使单元测试更快。工具级测试使用 `ToolContext::test_context()`。

> **提示：永远不要从 `edgecrab-tools` 导入 `edgecrab-core`。**
> 依赖图严格为 `tools → types/security/state`。违反此不变量会产生循环依赖。

---

## 常见问题

**问：为什么有单独的 `edgecrab-cron` crate？**
`edgecrab-cli`（`cron` 子命令）和 `edgecrab-tools`（`manage_cron_jobs` 工具）都需要调度解析和作业存储。将其分离出来避免将 CLI 依赖拉入工具 crate。

**问：网关如何与 CLI 共享 `Agent`？**
实际上并不共享。每个 `edgecrab gateway start` 都会生成自己的进程。会话状态通过磁盘上的 SQLite 数据库共享，而不是通过共享的内存对象。

**问：我可以在自己的 Rust 应用中嵌入 `edgecrab-core` 吗？**
可以。`AgentBuilder::new(model)` 是入口点。您提供一个 `LLMProvider`，可选地提供 `ToolRegistry` 和 `SessionDb`。完整的 builder API 参见
[Agent 结构](../003_agent_core/001_agent_struct.md)。

---

## 交叉引用

- 依赖图详情 → [Crate 依赖图](./002_crate_dependency_graph.md)
- 并发模型 → [并发模型](./003_concurrency_model.md)
- 错误传播 → [错误处理](./004_error_handling.md)
- 核心 `Agent` 类型 → [Agent 结构](../003_agent_core/001_agent_struct.md)
- 工具分发内部实现 → [工具注册表](../004_tools_system/001_tool_registry.md)