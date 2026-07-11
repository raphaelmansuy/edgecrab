# 项目概述 🦀

> **已验证来源：** `Cargo.toml` · `crates/edgecrab-core/src/agent.rs` ·
> `crates/edgecrab-core/src/conversation.rs` · `crates/edgecrab-tools/src/toolsets.rs` ·
> `crates/edgecrab-gateway/src/lib.rs` · `crates/edgecrab-cli/src/cli_args.rs`

---

## 起源故事 🦀

EdgeCrab 诞生于一场假想的三方对决：

```
  ┌─────────────────────┐   ┌──────────────────────┐   ┌─────────────────┐
  │     NousHermes      │   │    OpenClaw 🦞       │   │  EdgeCrab 🦀    │
  │  (Nous Research     │   │  (open-source self-  │   │                 │
  │   model fine-tune)  │   │   hosted assistant)  │   │                 │
  │                     │   │                      │   │                 │
  │  ■ enhanced reason  │   │  ■ tool use          │   │  ■ deep reason  │
  │  ■ model weights    │   │  ■ TypeScript/Node.js│   │  ■ fast tools   │
  │  ■ stateless infer  │   │  ■ no built-in       │   │  ■ 91 tools     │
  │  ■ Python inference │   │    security layer    │   │  ■ security     │
  │    stack            │   │  ■ single-user       │   │  ■ Rust, ~49 MB │
  │  ■ not an agent FW  │   │    desktop focus     │   │  ■ 15 gateways  │
  └─────────────────────┘   └──────────────────────┘   └─────────────────┘
         Round 1                    Round 2                WINNER 🏆
    (model, not a runtime)     (TypeScript, not Rust)    (all of the above)
```

> **事实说明**：NousHermes ([NousResearch/Hermes-3-Llama-3.1-8B](https://huggingface.co/NousResearch/Hermes-3-Llama-3.1-8B)) 是一个 LLM 微调系列，而非代理框架。其推理工具基于 Python（HuggingFace Transformers / vLLM）。OpenClaw ([github.com/openclaw](https://github.com/openclaw)) 是一个真实的 TypeScript/Node.js 个人 AI 助手——并非基于 Python。上述对决图是设计理念的示意性比较，而非详尽的功能审计。🦀

设计目标：融合微调函数调用模型（如 Hermes）的增强推理能力与个人助手平台（如 OpenClaw）的工具使用模式，将它们统一在单个 Rust 二进制文件中，并提供这两类项目都未曾实现的安全加固和多平台交付能力。

## 前身：`hermes-agent`

EdgeCrab 是 `hermes-agent` 的 Rust 重写版本——一个由同一作者维护的 Python 代理（Python venv + Node.js，`prompt_toolkit` TUI，约 80–150 MB 驻留内存，1–3 秒启动）。EdgeCrab 保留了相同的配置结构、内存格式和技能格式，因此迁移只需一条命令：

```bash
edgecrab migrate   # imports ~/.hermes/ → ~/.edgecrab/
```

有关导入内容，请参阅 [README 迁移表](../../README.md#migrating-from-hermes-agent)。`edgecrab-migrate` crate 处理配置、会话、内存、技能和环境变量。

## 为什么存在 EdgeCrab

在生产环境中跨多个渠道（终端、Telegram、Discord、VS Code、定时任务）运行 AI 代理，通常意味着每个渠道都要维护独立的代理运行时，在每个集成中重复实现提示词组装、工具分发、安全检查和会话持久化。

EdgeCrab 通过提供一个**单一的 Rust 二进制文件**解决了这个问题，该文件包含一个共享的代理运行时（`edgecrab-core`），所有前端都委托给它。您只需在一个地方处理工具执行逻辑，在一个地方调优系统提示词，在一个地方加固安全，无论会话来自哪里，都统一存储在一个 SQLite 数据库中。

这种设计带来的具体好处：

| 关注点 | 没有 EdgeCrab | 有 EdgeCrab |
|---|---|---|
| 工具分发 | 每个前端重新实现 | `ToolRegistry` 包含 91 个已注册的核心工具 |
| 会话历史 | 按渠道隔离 | 统一的 SQLite 数据库，支持 FTS5 搜索 |
| 安全 | 每个集成自行决定 | `CommandScanner`、`PathJail`、`InjectionCheck` 在注册表级别强制执行 |
| 提示组装 | 手工编写字符串 | `PromptBuilder` 支持内存、技能和上下文文件注入 |
| 上下文溢出 | OOM 或截断 | 5 遍压缩管道，使用 LLM 总结历史 |
| 多平台交付 | 每个渠道自定义 | 18 个适配器的网关，配有统一的交付路由器 |

---

## EdgeCrab 是什么

在运行时，EdgeCrab 有三种形态，共享同一个核心：

### 1. 终端 TUI (`edgecrab`)

交互式 ratatui UI，支持流式 token、语法高亮的 Markdown、斜杠命令、会话历史浏览器和代理的完整工具库。是开发者的默认入口。

### 2. 消息传递网关 (`edgecrab gateway start`)

为 Telegram、Discord、Slack、WhatsApp、Signal、Email、Matrix、Mattermost、钉钉、飞书、企业微信、短信、Webhook、HomeAssistant 等平台提供并发适配器进程。每条消息到达后，被传递给共享代理，响应再被分发回去。会话状态按 `(platform, user_id)` 对存储。

### 3. 编辑器协议服务器 (`edgecrab acp`)

实现 Agent Communication Protocol 的 JSON-RPC 2.0 stdio 服务器，使 VS Code、Zed 和 JetBrains Copilot 集成能够直接访问与 CLI 相同的代理运行时。

---

## 核心对象

所有重要的功能都追溯到 `crates/edgecrab-core/src/agent.rs` 中的单个 `Agent` 值：

```rust
pub struct Agent {
    config:          RwLock<AgentConfig>,
    provider:        RwLock<Arc<dyn LLMProvider>>,
    state_db:        Option<Arc<SessionDb>>,
    tool_registry:   Option<Arc<ToolRegistry>>,
    gateway_sender:  RwLock<Option<Arc<dyn GatewaySender>>>,
    process_table:   Arc<ProcessTable>,
    session:         RwLock<SessionState>,
    budget:          Arc<IterationBudget>,
    cancel:          Mutex<CancellationToken>,
    gc_cancel:       CancellationToken,
    todo_store:      Arc<TodoStore>,
}
```

公开接口很简洁：`chat(message)`、`chat_streaming(message, chunk_tx)` 和 `run_conversation(user_message, system_message, history)`。所有复杂性都隐藏在内部。

---

## 请求生命周期

来自任何前端的消息都会沿着以下路径穿过运行时：

```
  Input
    │
    ▼
  ┌─────────────────────────────────────────────────┐
  │  Frontend (CLI / Gateway / ACP)                 │
  │  Normalises input, resolves session key         │
  └────────────────────┬────────────────────────────┘
                       │  Agent::chat() or
                       │  Agent::run_conversation()
                       ▼
  ┌─────────────────────────────────────────────────┐
  │  Agent::execute_loop()    [edgecrab-core]        │
  │                                                  │
  │  1. Expand @context refs                         │
  │  2. Build / reuse cached system prompt           │
  │  3. Classify message → route to model            │
  │  4. Check iteration budget                       │
  │  5. Compress context if threshold exceeded       │
  │  6. Call LLM provider (up to 3× retry)           │
  │                                                  │
  │     ┌── tool_calls? ──────────────────────┐      │
  │     │  ToolRegistry::dispatch()            │      │
  │     │  → security checks                   │      │
  │     │  → approval gate                     │      │
  │     │  → ToolHandler::execute()            │      │
  │     │  → append results → loop              │      │
  │     └────────────────────────────────────── ┘     │
  │                                                  │
  │     └── text response? → break                   │
  │                                                  │
  │  7. Optional learning reflection (≥5 tool calls) │
  │  8. Persist session to SQLite                    │
  └────────────────────┬────────────────────────────┘
                       │ ConversationResult
                       ▼
  ┌─────────────────────────────────────────────────┐
  │  Frontend delivers formatted response            │
  └─────────────────────────────────────────────────┘
```

---

## 工作区结构

```
edgecrab/
├── crates/
│   ├── edgecrab-types/        ← leaf: Message, AgentError, ToolSchema, Usage
│   ├── edgecrab-security/     ← path jail, cmd scan, injection, redaction
│   ├── edgecrab-state/        ← SQLite WAL + FTS5 session persistence
│   ├── edgecrab-cron/         ← schedule parsing, job store, delivery metadata
│   ├── edgecrab-tools/        ← registry, 91 tools, toolsets, process table
│   ├── edgecrab-core/         ← Agent, loop, prompt builder, compression, routing
│   ├── edgecrab-cli/          ← clap, ratatui, setup wizard, doctor, profiles
│   ├── edgecrab-gateway/      ← 15 adapters, delivery, hooks, pairing, mirroring
│   ├── edgecrab-acp/          ← JSON-RPC 2.0 stdio ACP server
│   └── edgecrab-migrate/      ← hermes→edgecrab migration helper
├── docs/                      ← this documentation tree
├── skills/                    ← bundled Claude Code-compatible skill files
├── memories/                  ← project-level memory files loaded at startup
└── Cargo.toml                 ← workspace manifest
```

---

## 源码中的实际数据

| 事实 | 值 | 来源 |
|---|---|---|
| Rust edition | 2024 | `Cargo.toml` |
| MSRV | 1.95.0 | `workspace.package.rust-version` |
| 默认模型 | `ollama/gemma4:latest` | `edgecrab-core/src/config.rs` |
| 默认最大迭代次数 | 90 | `AgentConfig` 默认实现 |
| 已注册核心工具 | 91 | `edgecrab-tools/src/toolsets.rs` `CORE_TOOLS` |
| CLI 斜杠命令 | 53 | `edgecrab-cli/src/commands.rs` |
| 网关适配器 | 15 | `edgecrab-gateway/src/lib.rs` |
| SQLite 架构版本 | 6 | `edgecrab-state/src/session_db.rs` |
| 命令扫描模式 | ~40 字面量 + 正则二次检查 | `edgecrab-security/src/command_scan.rs` |
| 最大压缩重试次数 | 3 | `conversation.rs: MAX_RETRIES` |
| 技能反思阈值 | 5 次工具调用 | `conversation.rs: SKILL_REFLECTION_THRESHOLD` |

---

## 关键设计决策

**1. 单二进制，零运行时依赖。**
Release profile 使用 `lto = true`、`codegen-units = 1`、`strip = true`。
当前剥离后的 macOS arm64 发行版构建大小约为 49 MB。具体大小因目标三元组和启用的特性而异。

**2. 特征对象前端，而非泛型。**
`LLMProvider`、`ToolHandler`、`GatewaySender`、`SubAgentRunner` 和
`PlatformAdapter` 都是 `dyn Trait` 对象。这避免了工作区中的单态化爆炸，并允许网关在启动时插入适配器。

**3. `edgecrab-types` 中启用 `#![deny(clippy::unwrap_used)]`。**
每个其他 crate 都导入的叶子 crate 强制禁止使用 `unwrap`。错误作为 `AgentError` 变体显式传播。

**4. 基于 inventory 的编译时工具注册。**
工具在 crate 加载时使用 `inventory::submit!`。`ToolRegistry::new()` 遍历
`inventory::iter` ——无需更新 `match` 分支，无需手动维护列表。

**5. 工具层的特征对象解耦。**
`edgecrab-tools` 将 `SubAgentRunner` 和 `GatewaySender` 定义为 trait；`edgecrab-core` 实现它们。这打破了工具（需要运行子代理）和核心（拥有代理）之间明显的循环依赖。

---

## 快速开始清单

```sh
# Install
cargo install edgecrab-cli   # or: npm i -g edgecrab-cli / pip install edgecrab-cli

# First-run setup wizard (provider keys, model, gateway)
edgecrab setup

# Verify health
edgecrab doctor

# Start interactive session
edgecrab

# Ask a non-interactive question
edgecrab "summarise the last 10 git commits"

# Non-interactive with a specific toolset
edgecrab --toolset coding "refactor src/lib.rs to use thiserror"

# Start the multi-platform gateway
edgecrab gateway start
```

---

## 常见问题

**问：我可以使用 OpenAI 或 Gemini 而不是 Anthropic 来运行 EdgeCrab 吗？**
可以。LLM 抽象层是 `edgequake-llm`，它支持 OpenRouter 作为通用代理。设置 `EDGECRAB_MODEL=openai/gpt-4o` 或在
`~/.edgecrab/config.yaml` 中配置 `model.name`。

**问：会话历史存储在哪里？**
`~/.edgecrab/state.db` ——一个启用 WAL 模式和 FTS5 全文搜索的 SQLite 数据库。
详见 [会话存储](../009_config_state/002_session_storage.md)。

**问：如何添加自己的工具？**
实现 `ToolHandler`，调用 `inventory::submit!`，然后重新编译。详见
[工具注册表](../004_tools_system/001_tool_registry.md)。

**问：启用 shell 访问运行 EdgeCrab 是否安全？**
`CommandScanner` 在每个终端命令执行前，使用 Aho-Corasick 算法扫描约 40 个字面量模式，再加上正则二次检查。详见 [安全](../011_security/001_security.md)。

**问：EdgeCrab 可以无头运行吗？**
可以。`Agent::chat(message)` 和 `Agent::run_conversation(...)` 没有 UI
依赖。网关和 ACP 服务器都可以无头运行。

---

## 交叉引用

- 架构层 → [系统架构](../002_architecture/001_system_architecture.md)
- 循环工作原理 → [对话循环](../003_agent_core/002_conversation_loop.md)
- 工具分发详情 → [工具注册表](../004_tools_system/001_tool_registry.md)
- 安全模型 → [安全](../011_security/001_security.md)
- 配置解析 → [配置和状态](../009_config_state/001_config_state.md)