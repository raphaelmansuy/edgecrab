# EdgeCrab 文档 🦀

> **代码即法律。** 此树中的每个声明都经过验证，与 `crates/` 中的源代码一致。如果与代码冲突，代码获胜。

> 🦀 *"`hermes-agent` 拥有历史。OpenClaw 拥有爪子。EdgeCrab 两者兼有 —
> 加上安全扫描器、91 个核心工具，以及一个精简的发布二进制文件，在 macOS arm64 上目前约 49 MB。这场战斗很短暂。"*

> *(注意：`hermes-agent` 是 EdgeCrab 的 **Python** 前身 — `~/.hermes/`，`prompt_toolkit` TUI，约 80–150 MB。OpenClaw 是一个 TypeScript/Node.js 个人助手 — [github.com/openclaw](https://github.com/openclaw)。)*

EdgeCrab 是一个 Rust 原生 AI agent：一个单一的静态二进制文件，运行 ReAct 工具循环，与所有主要 LLM 提供商通信，并从一个共享运行时服务三个前端（终端 TUI、18 平台网关、编辑器 ACP）。

---

## 为什么存在这份文档

大多数 AI agent 项目会生长出一系列雄心勃勃的 readme 文件，但在几周内就与代码分道扬镳。此树采取相反的方法：它是一个简短、可导航的地图，描述工作区 **今天实际做什么**，源自源代码。阅读它应该给你足够的方向感，让你在第一天就能自信地做出更改。

---

## 工作空间概览

```
  ┌─────────────────────────────────────────────────────────────┐
  │                  用户界面                                     │
  │  edgecrab-cli (TUI)  │  edgecrab-gateway  │  edgecrab-acp   │
  └──────────────────────┴────────────────────┴─────────────────┘
                                  │
                    ┌─────────────▼──────────────┐
                    │      edgecrab-core          │
                    │  Agent · Loop · Prompt ·    │
                    │  Compression · Routing      │
                    └─────────────┬──────────────┘
          ┌──────────────────────┬┴───────────────────────┐
          │                      │                        │
  ┌───────▼───────┐   ┌──────────▼──────┐   ┌────────────▼────┐
  │edgecrab-tools │   │ edgecrab-state  │   │edgecrab-security│
  │ 91 tools,     │   │ SQLite WAL/FTS5 │   │path·cmd·inject  │
  │ registry,     │   │ sessions, FTS   │   │redact·url·policy│
  │ toolsets      │   └─────────────────┘   └─────────────────┘
  └───────────────┘
          │
  ┌───────▼───────────────────────┐
  │       edgecrab-types          │
  │  Message · Tool · Error ·     │
  │  Usage · Cost · Trajectory    │
  └───────────────────────────────┘

  edgecrab-cron ─── schedule parsing + job store (shared by cli + tools)
  edgecrab-migrate ─ hermes/openclaw → edgecrab import helper
```

---

## Crate 快速参考

| Crate | 它拥有什么 | 关键公共类型 |
|---|---|---|
| `edgecrab-types` | 共享消息/工具/错误/成本类型；叶依赖 | `Message`, `AgentError`, `ToolError` |
| `edgecrab-security` | Path jail, command scan, injection check, redaction | `CommandScanner`, `ApprovalPolicy` |
| `edgecrab-state` | SQLite session store, FTS5 search, analytics | `SessionDb` |
| `edgecrab-cron` | Cron schedule parsing, job store, delivery | `CronJob`, `CronStore` |
| `edgecrab-tools` | Tool registry, 91 tools, toolsets, backends | `ToolRegistry`, `ToolHandler` |
| `edgecrab-core` | Agent, conversation loop, prompt builder, routing | `Agent`, `AgentBuilder` |
| `edgecrab-cli` | TUI, clap commands, setup wizard, doctor | `CliArgs`, all subcommands |
| `edgecrab-gateway` | 15 gateway adapters, delivery, hooks, pairing | `PlatformAdapter`, `HookRegistry` |
| `edgecrab-acp` | JSON-RPC 2.0 stdio server for VS Code / Zed | `AcpServer` |
| `edgecrab-migrate` | One-time Hermes/OpenClaw migration helper | `MigrationReport` |

---

## 阅读顺序

选择符合你目标的路径。

### 新贡献者 — 从上到下阅读

1. [项目概述](./001_overview/001_project_summary.md) — EdgeCrab 是什么以及为什么存在
2. [系统架构](./002_architecture/001_system_architecture.md) — 分层和请求路径
3. [Crate 依赖图](./002_architecture/002_crate_dependency_graph.md) — 谁导入了什么
4. [Agent 结构](./003_agent_core/001_agent_struct.md) — 中心运行时对象
5. [对话循环](./003_agent_core/002_conversation_loop.md) — ReAct 核心
6. [工具注册表](./004_tools_system/001_tool_registry.md) — 工具如何分发
7. [安全](./011_security/001_security.md) — 护栏

### 添加工具

1. [工具注册表](./004_tools_system/001_tool_registry.md) — `ToolHandler` trait
2. [工具目录](./004_tools_system/002_tool_catalogue.md) — 现有工具以避免重复
3. [工具集组成](./004_tools_system/003_toolset_composition.md) — 加入哪个工具集
4. [工具运行时](./004_tools_system/004_tools_runtime.md) — `ToolContext` 和后端

### 添加网关平台

1. [网关架构](./006_gateway/001_gateway_architecture.md) — `PlatformAdapter` trait

### 处理 TUI / CLI

1. [CLI 架构](./005_cli/001_cli_architecture.md)

### 调试 agent 行为

1. [对话循环](./003_agent_core/002_conversation_loop.md)
2. [上下文压缩](./003_agent_core/004_context_compression.md)
3. [智能模型路由](./003_agent_core/005_smart_model_routing.md)
4. [配置和状态](./009_config_state/001_config_state.md)

### 理解持久化

1. [会话存储](./009_config_state/002_session_storage.md)
2. [数据模型](./010_data_models/001_data_models.md)

---

## 所有页面

| # | 页面 | 一句话 |
|---|---|---|
| 1 | [项目概述](./001_overview/001_project_summary.md) | EdgeCrab 是什么 |
| 2 | [系统架构](./002_architecture/001_system_architecture.md) | 分层和请求路径 |
| 3 | [Crate 依赖图](./002_architecture/002_crate_dependency_graph.md) | 谁导入了什么 |
| 4 | [并发模型](./002_architecture/003_concurrency_model.md) | Tokio, shared state, locking |
| 5 | [错误处理](./002_architecture/004_error_handling.md) | `AgentError`, `ToolError`, propagation |
| 6 | [Agent 结构](./003_agent_core/001_agent_struct.md) | Fields, builder, lifecycle |
| 7 | [对话循环](./003_agent_core/002_conversation_loop.md) | ReAct loop from source |
| 8 | [提示构建器](./003_agent_core/003_prompt_builder.md) | System prompt assembly |
| 9 | [上下文压缩](./003_agent_core/004_context_compression.md) | 5-pass compression pipeline |
| 10 | [智能模型路由](./003_agent_core/005_smart_model_routing.md) | Cheap / Primary / Fallback |
| 11 | [工具注册表](./004_tools_system/001_tool_registry.md) | `ToolHandler` trait and dispatch |
| 12 | [工具目录](./004_tools_system/002_tool_catalogue.md) | All 91 core tools |
| 13 | [工具集组成](./004_tools_system/003_toolset_composition.md) | Named sets and aliases |
| 14 | [工具运行时](./004_tools_system/004_tools_runtime.md) | `ToolContext`, execution backends |
| 15 | [CLI 架构](./005_cli/001_cli_architecture.md) | Clap, ratatui, slash commands |
| 16 | [Hermes 命令兼容性](./005_cli/002_hermes_command_parity.md) | Hermes command surface vs EdgeCrab |
| 17 | [网关架构](./006_gateway/001_gateway_architecture.md) | 15 adapters, hooks, delivery |
| 18 | [记忆和技能](./007_memory_skills/001_memory_skills.md) | `~/.edgecrab/memories/`, skill files |
| 19 | [创建技能](./007_memory_skills/002_creating_skills.md) | Writing and testing skill files |
| 20 | [执行后端](./008_environments/001_environments.md) | Local, Docker, SSH, Modal, Daytona |
| 21 | [配置和状态](./009_config_state/001_config_state.md) | `AppConfig`, resolution order |
| 22 | [会话存储](./009_config_state/002_session_storage.md) | SQLite schema, WAL, FTS5 |
| 23 | [数据模型](./010_data_models/001_data_models.md) | All public types |
| 24 | [安全](./011_security/001_security.md) | All security primitives |
| 25 | [库选择](./013_library_selection/001_library_selection.md) | Why each dependency |
| 26 | [CI/CD Secrets](./016_cicd/001_secrets_setup.md) | GitHub Actions secrets |
| 27 | [GitHub Pages DNS](./016_cicd/002_github_pages_dns.md) | DNS setup |
| 27 | [钩子](./hooks.md) | Native and script hooks |

---

## 编辑规则

- 每个声明必须可追溯到源代码。
- 图表显示存在的，不是计划的。
- 删除过时的部分，而不是留下"TODO"注释。
- 如果有疑问，查看代码。
