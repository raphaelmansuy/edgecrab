# EdgeCrab 🦀

> **"你的超级代理 — 用 Rust 构建。"**

[![License](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.86%2B-orange.svg)](https://www.rust-lang.org/)
[![crates.io](https://img.shields.io/crates/v/edgecrab-cli.svg)](https://crates.io/crates/edgecrab-cli)
[![PyPI](https://img.shields.io/pypi/v/edgecrab-cli.svg)](https://pypi.org/project/edgecrab-cli/)
[![npm](https://img.shields.io/npm/v/edgecrab-cli.svg)](https://www.npmjs.com/package/edgecrab-cli)
[![CI](https://github.com/raphaelmansuy/edgecrab/actions/workflows/ci.yml/badge.svg)](https://github.com/raphaelmansuy/edgecrab/actions/workflows/ci.yml)
[![Website](https://img.shields.io/badge/Website-edgecrab.com-orange.svg)](https://www.edgecrab.com)

[![Changelog](https://img.shields.io/badge/Changelog-CHANGELOG.md-blue.svg)](CHANGELOG.md)

EdgeCrab 是一个**超级代理** — 一个用 Rust 打造的个人助手和编码代理。它承载着 **Nous Hermes Agent** 的灵魂（自主推理、持久化记忆、用户优先对齐）和 **OpenClaw** 的始终在线存在感（17 个消息网关、智能家居集成），打包为一个精简的原生发布二进制文件，在当前 macOS arm64 构建上约 **49 MB**，零 Python 或 Node.js 运行时依赖。可在 Linux、macOS 和 Android（Termux）上运行。

> **最新版本：v0.10.0** — Hermes 兼容的**终端用户体验**：带有工具进度尾部的实时活动架、`/agents` 委派仪表板（kill · replay · Gantt · spawn pause）、队列消息编辑器、`/model` 即时热交换，以及模块化 TUI 覆盖层堆栈。加上 Ralph 循环目标、LSP 诊断、原生网络搜索、OpenAI 代理和订阅 OAuth。


## 架构

![Architecture](./assets/edgecrab-archi.jpg)

```
hermes-agent soul  +  OpenClaw vision  =  EdgeCrab
   (推理)             (存在)             (Rust)
```

| 指标 | EdgeCrab 🦀 | hermes-agent ☤ |
|------|-------------|---------------|
| 二进制 | ~49 MB 精简发布构建 | Python venv + uv |
| 运行时引导 | 无 | Python + uv |
| 内存 | 工作负载相关的原生进程 | ~80–150 MB |
| LLM 提供商 | 16 个内置 | 可变 |
| 消息平台 | 17 个网关 | 7 个平台 |
| 测试 | 1629 个通过（Rust） | — |
| 从 hermes 迁移 | `edgecrab migrate` | N/A |

![EdgeCrab — The Clash of the Crustaceans](assets/edgecrab-hero.jpeg)

---

## 目录

- [EdgeCrab 🦀](#edgecrab-)
  - [架构](#架构)
  - [目录](#目录)
  - [为什么选择 EdgeCrab？](#为什么选择-edgecrab)
  - [快速开始（90秒）](#快速开始-90秒)
    - [选项 A — npm（无需 Rust）](#选项-a-npm无需-rust)
    - [选项 B — pip（无需 Rust）](#选项-b-pip无需-rust)
    - [选项 C — cargo](#选项-c-cargo)
    - [选项 D — 从源码构建](#选项-d-从源码构建)
    - [引导设置输出](#引导设置输出)
    - [首次提示](#首次提示)
  - [EdgeCrab 能做什么](#edgecrab-能做什么)
    - [ReAct 工具循环](#react-工具循环)
    - [内置工具](#内置工具)
    - [语义代码智能（LSP）](#语义代码智能-lsp)
      - [文件工具（`file` 工具集）](#文件工具-file-工具集)
      - [终端工具（`terminal` 工具集）](#终端工具-terminal-工具集)
      - [网络工具（`web` 工具集）](#网络工具-web-工具集)
      - [浏览器工具（`browser` 工具集）](#浏览器工具-browser-工具集)
      - [内存与 Honcho 工具（`memory` + `honcho` 工具集）](#内存与-honcho-工具-memory--honcho-工具集)
      - [技能工具（`skills` 工具集）](#技能工具-skills-工具集)
      - [会话与搜索（`session` 工具集）](#会话与搜索-session-工具集)
      - [委派与 MoA（`delegation` + `moa` 工具集）](#委派与-moa-delegation--moa-工具集)
      - [代码执行（`code_execution` 工具集）](#代码执行-code_execution-工具集)
      - [MCP 工具（`mcp` 工具集）](#mcp-工具-mcp-工具集)
      - [媒体工具（`vision` / `tts` / `transcribe` 工具集）](#媒体工具-vision--tts--transcribe-工具集)
      - [自动化工具](#自动化工具)
    - [子代理委派](#子代理委派)
    - [沙箱代码执行](#沙箱代码执行)
    - [浏览器自动化](#浏览器自动化)
    - [17 个消息网关](#17-个消息网关)
    - [持久化记忆与学习](#持久化记忆与学习)
    - [技能库](#技能库)
    - [技能与插件](#技能与插件)
    - [插件系统](#插件系统)
    - [Cron 调度](#cron-调度)
    - [检查点与回滚](#检查点与回滚)
    - [配置文件与工作树](#配置文件与工作树)
    - [视觉、TTS 与转录](#视觉-tts-与转录)
  - [16 个 LLM 提供商](#16-个-llm-提供商)
  - [6 个终端后端](#6-个终端后端)
  - [MCP 服务器集成](#mcp-服务器集成)
  - [ACP / VS Code Copilot 集成](#acp--vs-code-copilot-集成)
  - [ratatui TUI](#ratatui-tui)
  - [所有 CLI 命令](#所有-cli-命令)
  - [所有斜杠命令](#所有斜杠命令)
  - [安全模型](#安全模型)
  - [架构](#架构-1)
  - [配置](#配置)
  - [SDKs](#sdks-one-edgecrab-experience)
    - [Python SDK（`edgecrab`）](#python-sdk-edgecrab)
    - [Node.js SDK（`edgecrab`）](#nodejs-sdk-edgecrab)
  - [Docker](#docker)
  - [从 hermes-agent 迁移](#从-hermes-agent-迁移)
  - [测试](#测试)
  - [项目结构](#项目结构)
  - [要求与构建](#要求与构建)
  - [贡献](#贡献)
  - [发布渠道](#发布渠道)
  - [许可证](#许可证)

---

## 为什么选择 EdgeCrab？

大多数 AI 代理要么过于受限（会话结束后就忘记你的编码代理），要么过于沉重（Python 运行时、Node 守护进程、GB 级内存）。EdgeCrab 与众不同。

**它会学习。** 像 Nous Hermes Agent 一样，EdgeCrab 在会话之间保持持久化记忆，自动生成可复用技能，并构建一个跨会话的 Honcho 用户模型，随着时间推移变得更智能。

**它无处不在。** 像 OpenClaw 一样，EdgeCrab 生活在你的频道中 — Telegram、Discord、Slack、WhatsApp、Signal、Matrix、Mattermost、钉钉、SMS、Email、Home Assistant 等等。在 WhatsApp 上发送语音备忘录，得到一个 PR 回报。

**它快速且精简。** 与 Python 代理不同，EdgeCrab 作为原生 Rust 二进制文件交付，而不是 Python 或 Node.js 运行时堆栈。当前精简的 macOS arm64 发布构建约 49 MB，安全性是编译在内的 — 路径监狱、SSRF 防护、命令扫描器 — 不是运行时补丁。

**它可扩展。** MCP 服务器、自定义 Rust 工具、Python/JS 沙箱、子代理、混合代理共识 — 全套重型自动化工具包。

**它现在是插件原生的。** 技能插件注入提示专业知识，工具服务器插件暴露外部 JSON-RPC 工具，脚本插件运行安全的 Rhai 逻辑，所有这些都来自 `~/.edgecrab/plugins/`，具有持久化的启用/禁用策略。

---

## 快速开始（90秒）

### 选项 A — npm（无需 Rust）

```bash
npm install -g edgecrab-cli
edgecrab update              # 通道感知更新器
edgecrab setup               # 交互式向导 — 检测 API 密钥，写入配置
edgecrab doctor              # 验证健康状态
edgecrab                     # 启动 TUI
```

### 选项 B — pip（无需 Rust）

```bash
pip install edgecrab-cli
# 或：pipx install edgecrab-cli （隔离安装）
edgecrab update
edgecrab setup && edgecrab doctor && edgecrab
```

### 选项 C — cargo

```bash
cargo install edgecrab-cli
edgecrab update --check
edgecrab setup && edgecrab doctor && edgecrab
```

### 选项 D — 从源码构建

```bash
git clone https://github.com/raphaelmansuy/edgecrab
cd edgecrab
cargo build --workspace --release         # 首次构建约 30 秒
./target/release/edgecrab setup
```

### 引导设置输出

```
EdgeCrab Setup Wizard
──────────────────────────────────────────────────────────────
✓ Detected GitHub Copilot (GITHUB_TOKEN)
✓ Detected OpenAI (OPENAI_API_KEY)

Choose LLM provider:
  [1] copilot      (GitHub Copilot — GPT-5 / Claude / Gemini catalog)  ← auto-detected
  [2] openai       (OpenAI — GPT-4.1, GPT-5, o3/o4)
  [3] anthropic    (Anthropic — Claude Opus 4.6)
  [4] ollama       (local — llama3.3)
  ...
Provider [1]: 1

✓ Config written to ~/.edgecrab/config.yaml
✓ Created ~/.edgecrab/memories/
✓ Created ~/.edgecrab/skills/

Run `edgecrab` to start chatting!
```

### 首次提示

```bash
edgecrab "summarise the git log for today and open PRs"
edgecrab --model openai/gpt-5 "review this codebase for security issues"
edgecrab --model ollama/llama3.3 "explain this code offline"
edgecrab --quiet "count lines in src/**/*.rs"   # 管道安全，无横幅
edgecrab -C "continue-my-refactor"              # 恢复命名会话
edgecrab -w "explore that perf idea"            # 隔离的 git 工作树
```

### OpenAI 兼容代理（Aider、Cline、OpenAI SDK）

将 EdgeCrab 配置的 LLM 提供商暴露给第三方客户端 — **不是**完整的代理 API（与网关 `api_server` 平台不同）：

```bash
# Grok / xAI OAuth（推荐路径）
edgecrab proxy setup grok             # 写入配置 + 令牌 + 客户端代码片段
edgecrab proxy doctor
edgecrab proxy start --provider xai

# 或分步进行
edgecrab proxy enable grok
edgecrab proxy token set
edgecrab proxy client                 # 打印 OPENAI_API_BASE / Aider 变量
edgecrab proxy start --provider xai
```

将任何 OpenAI 客户端指向 `http://127.0.0.1:11434/v1`，使用 `Authorization: Bearer <proxy-token>`。在 `~/.edgecrab/config.yaml` 中映射友好名称：

```yaml
proxy:
  port: 11434
  model_aliases:
    claude-sonnet: anthropic/claude-sonnet-4-20250514
    gpt-4o: openai/gpt-4o
    nous-portal: forward:nous          # 模式 A — 凭证转发器
  forward_upstreams:
    nous:
      adapter: nous_portal          # OAuth 刷新 + 调用 JWT（Hermes NousPortalAdapter）
      auth_provider: nous
      base_url: https://inference-api.nousresearch.com/v1
      # 或只读：adapter: hermes_auth
      # 或静态 bearer: bearer_env: NOUS_API_KEY
    xai:
      adapter: xai_oauth
      auth_provider: xai-oauth
      base_url: https://api.x.ai/v1
  default_forward_upstream: nous   # 可选：GET /v1/models → 上游（Hermes 风格）
  cors_allow_origins: []           # 例如 ["http://localhost:3000"] 用于浏览器客户端
```

**Aider**（`~/.aider.conf.yml`）：

```yaml
openai-api-base: http://127.0.0.1:11434/v1
openai-api-key: <your-proxy-token>
```

默认绑定仅回环；仅在使用强令牌时使用 `--allow-public`。

---

## EdgeCrab 能做什么

EdgeCrab 是一个自主代理。用自然语言给它一个目标；它推理、调用工具、观察结果，并循环直到任务完成。以下是它实际能做的事情。

### ReAct 工具循环

EdgeCrab 使用 **Reason → Act → Observe** 循环（ReAct 模式），在 `crates/edgecrab-core/src/conversation.rs` 中实现。每轮：

1. **系统提示词每个会话构建一次**（SOUL.md、AGENTS.md、记忆、技能、日期/时间、cwd）— 缓存用于 Anthropic 提示词缓存命中
2. **LLM 决定**下一步做什么（包括并行工具调用）
3. **安全检查**在每次工具执行前运行（路径监狱、SSRF 防护、命令扫描）
4. **工具执行** — 文件 I/O、shell、网络、代码、子代理、浏览器等
5. **结果注入**回到上下文中
6. **循环**直到没有更多工具调用（任务完成）、`Ctrl-C` 或 90 次迭代预算耗尽
7. **上下文压缩**在上下文窗口的 50% 触发 — 修剪旧工具输出，然后 LLM 总结
8. **学习反思**在 ≥5 次工具调用后自动触发 — 代理可以保存新技能并更新记忆

预算默认为 **90 次迭代**（配置中的 `max_iterations`）。对于长自主任务，增加它。

### 内置工具

工具通过 `inventory` crate 在编译时注册 — 零启动成本。`ToolRegistry` 按确切名称调度，带有模糊（Levenshtein ≤3）回退建议。

### 语义代码智能（LSP）

EdgeCrab 现在通过 `lsp` 工具集公开专用的 LSP 子系统。当配置了语言服务器时，代理可以优先使用语义操作而不是 grep 风格的猜测：

- Claude 兼容导航：`lsp_goto_definition`、`lsp_find_references`、`lsp_hover`、`lsp_document_symbols`、`lsp_workspace_symbols`、`lsp_goto_implementation`、`lsp_call_hierarchy_prepare`、`lsp_incoming_calls`、`lsp_outgoing_calls`
- EdgeCrab 独有的语义编辑：`lsp_code_actions`、`lsp_apply_code_action`、`lsp_rename`、`lsp_format_document`、`lsp_format_range`
- 深度分析：`lsp_inlay_hints`、`lsp_semantic_tokens`、`lsp_signature_help`、`lsp_type_hierarchy_prepare`、`lsp_supertypes`、`lsp_subtypes`
- 诊断：`lsp_diagnostics_pull`、`lsp_linked_editing_range`、`lsp_enrich_diagnostics`、`lsp_select_and_apply_action`、`lsp_workspace_type_errors`

内置默认服务器定义现在覆盖 Rust、TypeScript、JavaScript、Python、Go、C、C++、Java、C#、PHP、Ruby、Bash、HTML、CSS 和 JSON。

#### 文件工具（`file` 工具集）
| 工具 | 功能 |
|------|------|
| `read_file` | 读取文件，可选 `start_line`/`end_line` — 路径监狱，规范化 |
| `write_file` | 写入或创建文件（父目录自动创建） |
| `patch_file` | 搜索替换补丁 — 精确字符串匹配，原子写入 |
| `search_files` | 目录树中的正则表达式 + glob 搜索 |

#### 终端工具（`terminal` 工具集）
| 工具 | 功能 |
|------|------|
| `terminal` | 执行 shell 命令 — 每个任务的持久化 shell，环境变量阻止列表 |
| `manage_process` | 启动/停止/列出/杀死/读取后台进程 |

#### 网络工具（`web` 工具集）
| 工具 | 功能 |
|------|------|
| `web_search` | 通过 Firecrawl → Tavily → Brave → DuckDuckGo 回退链进行网络搜索 |
| `web_extract` | 全页面提取 — HTML 剥离 + PDF 解析（EdgeParse）— SSRF 防护 |

#### 浏览器工具（`browser` 工具集）
| 工具 | 功能 |
|------|------|
| `browser_navigate` | 通过 CDP 导航 Chrome |
| `browser_snapshot` | 可访问性树快照（文本，非像素） |
| `browser_click` | 按快照中的 `@eN` ref ID 点击元素 |
| `browser_type` | 在聚焦输入中输入文本 |
| `browser_screenshot` | 带编号元素覆盖的注释截图 |
| `browser_console` | 捕获/清除浏览器控制台日志 |

#### 内存与 Honcho 工具（`memory` + `honcho` 工具集）
| 工具 | 功能 |
|------|------|
| `memory_read` | 从 `~/.edgecrab/memories/` 读取 `MEMORY.md` 和 `USER.md` |
| `memory_write` | 写入/追加到内存文件（提示注入扫描） |
| `honcho_profile` | 通过 Honcho 跨会话模型获取/设置用户配置文件事实 |
| `honcho_context` | 为当前任务检索上下文相关的 Honcho 记忆 |

#### 技能工具（`skills` 工具集）
| 工具 | 功能 |
|------|------|
| `skill_manage` | 创建、查看、补丁、删除、列出技能 |

#### 会话与搜索（`session` 工具集）
| 工具 | 功能 |
|------|------|
| `session_search` | SQLite FTS5 全文搜索所有过去的会话 |

#### 委派与 MoA（`delegation` + `moa` 工具集）
| 工具 | 功能 |
|------|------|
| `delegate_task` | Fork 子代理 — 单个任务或最多 3 个并行批处理 |
| `mixture_of_agents` | 通过 Claude Opus 4.6、Gemini 2.5 Pro、GPT-4.1、DeepSeek R1 并行运行任务；综合共识 |

#### 代码执行（`code_execution` 工具集）
| 工具 | 功能 |
|------|------|
| `execute_code` | 沙箱 Python / JS / Bash / Ruby / Perl / Rust 执行，带工具 RPC |

#### MCP 工具（`mcp` 工具集）
| 工具 | 功能 |
|------|------|
| `mcp_list_tools` | 列出所有连接的 MCP 服务器公开的工具 |
| `mcp_call_tool` | 调用任何连接的 MCP 服务器上的命名工具 |

#### 媒体工具（`vision` / `tts` / `transcribe` 工具集）
| 工具 | 功能 |
|------|------|
| `vision_analyze` | 通过多模态模型分析图像（URL 或本地路径） |
| `text_to_speech` | 从文本生成音频（OpenAI TTS 或配置的提供商） |
| `transcribe_audio` | 转录音频文件（Whisper 或 Groq/OpenAI） |

#### 自动化工具
| 工具 | 功能 |
|------|------|
| `manage_todo_list` | 结构化清单 — 创建、更新、完成、删除项目 |
| `manage_cron_jobs` | 调度定期和一次性 cron 任务 |
| `checkpoint` | 文件系统快照用于回滚（创建、列出、恢复、差异） |
| `clarify` | 向用户提出澄清问题（带可选选项） |
| `send_message` | 通过网关向任何连接的平台发送消息 |
| `ha_get_states` | 获取 Home Assistant 实体状态 |
| `ha_call_service` | 调用 HA 服务（例如 `light.turn_on`） |
| `ha_trigger_automation` | 触发 HA 自动化 |
| `ha_get_history` | 获取 HA 实体历史 |

**控制激活哪些工具集：**
```bash
edgecrab --toolset file,terminal "add tests"        # 最小开发
edgecrab --toolset all "go wild"                    # 完整功能
edgecrab --toolset coding "refactor this module"    # file+terminal+search+exec+lsp
edgecrab --toolset research "investigate this bug"  # web+browser+vision
```

---

### 子代理委派

EdgeCrab 可以生成运行完整 ReAct 循环的子代理，每个代理有自己的会话状态。这实现了复杂任务的并行处理。

```
# 示例：代理并行委派 3 个子任务
delegate_task([
  { task: "Review auth module for security issues" },
  { task: "Write unit tests for the payment service" },
  { task: "Update API documentation" }
])
# → 3 个子代理并发运行，结果聚合
```

**工作原理**（`crates/edgecrab-tools/src/tools/delegate_task.rs`）：
- 子代理共享 LLM 提供商 Arc + 工具注册表 Arc
- 每个子代理获得自己的 `SessionState`、`ProcessTable`、`TodoStore`、`IterationBudget`
- 最大并发：**3 个子代理并行**（可通过 `delegation.max_subagents` 配置）
- 最大深度：**2 级**（父 → 子 → 孙被阻止）
- 子代理不能使用 `delegation`、`clarify`、`memory`、`code_execution` 或 `messaging` 工具集

配置委派：
```yaml
delegation:
  enabled: true
  model: "openai/gpt-4o"   # 为子代理使用一个功能强大的共享模型
  max_subagents: 3
  max_iterations: 50
```

---

### 沙箱代码执行

`execute_code` 工具在具有严格资源限制的隔离子进程中运行代码：

- **语言**：Python、JavaScript、Bash、Ruby、Perl、Rust
- **工具 RPC**：脚本可以通过 Unix 域套接字调用 7 个工具 — `web_search`、`web_extract`、`read_file`、`write_file`、`search_files`、`terminal`、`session_search`
- **限制**：50 次工具调用限制、5 分钟超时、50 KB stdout 上限、10 KB stderr 上限
- **安全**：执行前从子环境中剥离 API 密钥/令牌

```python
# 示例：代理在沙箱中编写并执行此代码
import subprocess
result = subprocess.run(['cargo', 'test', '-p', 'edgecrab-core'], capture_output=True)
print(result.stdout.decode())
```

---

### 浏览器自动化

基于 Chrome DevTools Protocol 的浏览器自动化 — 无需 Selenium，无需 Playwright 依赖。ElementCrab 直接连接到 CDP 端点。

```
Requirements: Chrome/Chromium binary, or set CDP_URL to an existing instance
Check:         edgecrab doctor  (reports browser availability)
```

`browser_snapshot` 工具返回可访问性树 — 不是像素 — 因此 LLM 可以在没有视觉成本的情况下推理页面结构。`browser_screenshot` 添加编号元素覆盖用于精确点击。

---

### 17 个消息网关

启动网关服务器，EdgeCrab 同时在 17 个消息平台中成为始终在线的助手：

```bash
edgecrab gateway start           # 在后台运行
edgecrab gateway start --foreground   # 保持在前台
edgecrab gateway status          # 检查哪些平台在线
edgecrab gateway stop
```

| 平台 | 传输 | 认证 |
|------|------|------|
| **Telegram** | 长轮询 REST | `TELEGRAM_BOT_TOKEN` |
| **Discord** | WebSocket 网关 | `DISCORD_BOT_TOKEN` |
| **Slack** | Socket Mode WebSocket | `SLACK_BOT_TOKEN` + `SLACK_APP_TOKEN` |
| **WhatsApp** | Baileys 桥接（本地 Node 子进程） | `edgecrab whatsapp` QR 配对 |
| **Signal** | signal-cli HTTP + SSE | `SIGNAL_HTTP_URL` + `SIGNAL_ACCOUNT` |
| **Matrix** | 客户端-服务器 REST + 长轮询同步 | `MATRIX_HOMESERVER` + `MATRIX_ACCESS_TOKEN` |
| **Mattermost** | REST v4 + WebSocket | `MATTERMOST_URL` + `MATTERMOST_TOKEN` |
| **钉钉** | Stream SDK（无公开 webhook） | `DINGTALK_APP_KEY` + `DINGTALK_APP_SECRET` |
| **SMS** | Twilio REST v2010 | `TWILIO_ACCOUNT_SID` + `TWILIO_AUTH_TOKEN` |
| **Email** | SMTP（lettre, rustls）+ 入站 webhook | `EMAIL_PROVIDER` + `EMAIL_FROM` + `EMAIL_API_KEY` |
| **Home Assistant** | WebSocket + REST | `HASS_URL` + `HASS_TOKEN` |
| **Webhook** | axum HTTP POST | 任何 HTTP 调用者 |
| **API Server** | axum OpenAI 兼容 HTTP | `API_SERVER_PORT`（可选） |
| **飞书/Lark** | REST | `FEISHU_APP_ID` + `FEISHU_APP_SECRET` |
| **企业微信** | WebSocket + REST + 心跳 | `WECOM_BOT_ID` + `WECOM_SECRET` |
| **iMessage** | BlueBubbles REST + webhook + 附件 | `BLUEBUBBLES_SERVER_URL` + `BLUEBUBBLES_PASSWORD` |
| **微信** | iLink Bot API POST-poll + AES CDN 媒体 | `WEIXIN_TOKEN` + `WEIXIN_ACCOUNT_ID` |

**流式传输交付**：编辑模式平台（Telegram、Discord、Slack）接收实时 token 流，编辑间隔为 300ms。批处理模式平台（WhatsApp、Signal、SMS、Email）累积完整响应并一次发送。

**内置网关斜杠命令**（通过聊天发送）：
```
/help      /new       /reset     /stop      /retry
/status    /usage     /background  /approve   /deny
```

**设置 WhatsApp**（一次性 QR 配对）：
```bash
edgecrab whatsapp      # 启动 QR 码扫描向导
# 用手机扫描 — 会话跨重启持久化
edgecrab gateway start
```

**Cron 触发消息**：安排代理主动向您发送消息：
```yaml
# ~/.edgecrab/cron/daily-standup.json
schedule: "0 9 * * 1-5"     # 每个工作日上午 9 点
task: "Summarize open PRs and blockers for today's standup"
target: telegram             # 发送到您的 Telegram
```

---

### 持久化记忆与学习

EdgeCrab 有三层记忆系统：

**第一层 — MEMORY.md**（`~/.edgecrab/memories/MEMORY.md`）：自由格式笔记。代理在会话开始时读取此文件，并可以更新它。您也可以直接编辑它。

**第二层 — SQLite 会话历史**（`~/.edgecrab/state.db`）：每个会话都存储在 WAL 模式 SQLite 中，带 FTS5 全文搜索。浏览、搜索和导出会话：
```bash
edgecrab sessions list                           # 列出最近会话
edgecrab sessions search "auth bug from last week"  # FTS5 搜索
edgecrab sessions export <id> --format jsonl     # 导出会话
edgecrab sessions browse                         # 交互式浏览器
```

**第三层 — Honcho 跨会话用户模型**：EdgeCrab 通过 Honcho API 构建您的语义模型 — 您的偏好、项目、工作风格。此上下文在新会话开始时注入以提供连续性。

**自动学习**：在会话中进行 ≥5 次工具调用后，学习反思会自动触发。代理可以保存新技能、更新 MEMORY.md，并记录有用模式而无需被询问。

---

### 技能库

技能是可复用的代理程序 — Markdown 文件，定义了重复任务的提示词、步骤和最佳实践。把它想象成您代理的食谱卡。

```bash
# 创建技能
edgecrab skills list                    # 浏览已安装技能
edgecrab skills view git-workflow       # 读取技能
edgecrab skills install my-skill.md    # 从文件安装
edgecrab skills search "diagram"       # 搜索远程技能源
edgecrab skills install edgecrab:diagramming/ascii-diagram-master
edgecrab skills install hermes-agent:research/ml-paper-writing
edgecrab skills install raphaelmansuy/edgecrab/skills/research/ml-paper-writing
edgecrab skills update                 # 刷新所有远程安装的技能
edgecrab skills update ml-paper-writing

# 在会话中使用技能
edgecrab -S git-workflow "review this branch for prod readiness"
edgecrab -S security,refactor          # 加载多个技能
```

在 TUI 内部：`/skills` 打开已安装技能浏览器，`/skills search [query]` 打开远程技能浏览器，带实时搜索、源注释和安装/更新操作。

技能保存到 `~/.edgecrab/skills/` 并按需加载。代理也可以在学习反思期间在会话中创建新技能。

独立技能运行时支持带辅助脚本的 Claude 风格技能包：

- 在 `references/`、`templates/`、`scripts/` 和 `assets/` 下的捆绑辅助文件
- `${CLAUDE_SKILL_DIR}` 替换为具体技能目录
- `${CLAUDE_SESSION_ID}` 替换为活动的 EdgeCrab 会话 ID
- `skill_view` 和预加载的 `--skill` / `skills.preloaded` 流程使用相同的包渲染
- 解析和显示 `when_to_use`、`arguments`、`argument-hint`、`allowed-tools`、
  `user-invocable`、`disable-model-invocation`、`context` 和 `shell`

当前边界：EdgeCrab 不会自动执行 Claude 内联提示-shell
块，也不会仅从这些元数据字段自动 fork 专用技能子代理。

### 技能与插件

基本原则：

- `skill` 是模型的可复用指导。
- `plugin` 是 EdgeCrab 发现、启用、禁用、更新和审计的可安装运行时单元。

这导致了清晰的操作分离：

- 当扩展是以指令为先时使用 `skills`：程序、示例、清单、工作流脚手架，或代理通过普通工具使用的捆绑辅助文件/脚本。
- 当扩展需要可执行代码、工具注册、钩子、就绪检查、信任元数据或安装生命周期管理时使用 `plugins`。
- 普通技能改变提示行为。它可以捆绑辅助文件，如 `scripts/`、`references/`、`templates/` 和 `assets/`，但它仍然不注册新的运行时服务或插件生命周期本身。
- 插件可以捆绑 `SKILL.md`，但该捆绑技能仍然是插件管理的运行时包的一部分。

具体示例：

- `~/.edgecrab/skills/security-review/SKILL.md` 是独立技能。
- `~/.edgecrab/skills/security-review/scripts/check.py` 可以与该技能捆绑，并从 `SKILL.md` 引用。
- `~/.edgecrab/plugins/github-tools/plugin.toml` 是插件。
- `~/.edgecrab/plugins/calculator/plugin.yaml` 加上 `__init__.py` 是 Hermes 插件。
- 类型为 `skill` 的插件仍然通过 `edgecrab plugins ...` 管理，而不是 `edgecrab skills ...`。

---

### 插件系统

插件扩展 EdgeCrab 超出内置工具清单，无需分叉仓库。

```bash
edgecrab plugins list
edgecrab plugins info github-tools
edgecrab plugins status
edgecrab plugins install github:edgecrab/plugins/github-tools
edgecrab plugins install hub:community/github-tools
edgecrab plugins install https://example.com/github-tools.zip
edgecrab plugins install ./plugins/github-tools
edgecrab plugins enable github-tools
edgecrab plugins disable github-tools
edgecrab plugins toggle [github-tools]
edgecrab plugins audit --lines 20
edgecrab plugins search github
edgecrab plugins search --source hermes weather
edgecrab plugins search --source hermes-evey telemetry
edgecrab plugins browse
edgecrab plugins update
edgecrab plugins remove github-tools
```

在 TUI 内部，`/plugins search ...` 和 `/plugins browse` 现在打开与 EdgeCrab 已经用于技能和 MCP 的相同类型的异步远程浏览器：
模糊过滤、后台搜索、分割详情视图，以及一键安装或从官方注册表替换。

EdgeCrab 现在支持四种插件类型：

- `skill` 插件从 `~/.edgecrab/plugins/<name>/` 加载 `SKILL.md` 内容到会话提示词中，具有 Hermes 兼容的 frontmatter、就绪检查和平台过滤。
- `tool-server` 插件生成子进程并通过 stdio 代理 MCP 兼容的换行分隔 JSON-RPC，包括反向 `host:*` 调用以获取平台信息、内存/会话访问、秘密读取、安全会话消息注入、日志记录和委派工具执行。
- `script` 插件加载 Rhai 代码用于轻量级本地扩展点和工具处理程序，无需单独的守护进程。
- `hermes` 插件加载 Hermes 风格的 Python 目录插件，具有 `plugin.yaml` + `__init__.py register(ctx)` 兼容性，包括 `requires_env` 设置门控、捆绑的 `SKILL.md` 加载、`post_tool_call`、`on_session_start`、`pre_llm_call` 和 `on_session_end`。

EdgeCrab 还从 `~/.hermes/plugins/` 发现遗留 Hermes 插件根目录，以及当 `HERMES_ENABLE_PROJECT_PLUGINS=true` 时的 `./.hermes/plugins/`。插件安装现在在隔离区中暂存，运行静态安全扫描，从其源解析信任，并在激活前用目录校验和标记 `plugin.toml`。插件状态在 `config.yaml` 的 `plugins:` 下持久化。已禁用或需要设置的插件在不卸载的情况下从工具暴露或提示注入中排除。

运行时暴露是实时的：

- 启用的插件工具注册到 `plugins` 工具集中，并出现在 `/tools` 中
- 禁用插件会从活动注册表中移除其工具，无需重启 EdgeCrab
- 重新启用插件会在同一 TUI 会话中立即重新暴露这些工具

在 TUI 中您可以直接验证：

```text
/plugins                 # 打开已安装插件浏览器覆盖层
/tools                   # 显示活动的内置 + 插件工具
/plugins disable demo
/tools                   # demo 插件工具消失
/plugins enable demo
/tools                   # demo 插件工具在 plugins 工具集下回来
```

远程插件搜索按基本原则缓存：

- hub 索引和 repo 支持的源树缓存在 `~/.edgecrab/plugins/.hub/cache/` 下
- repo 支持的插件描述单独缓存，因此重复搜索不会重新获取 `plugin.yaml` 或 `SKILL.md`
- 过期缓存在可能时刷新，但刷新失败时仍使用过期缓存，因此插件搜索优雅降级而不是变为空

示例：安装带捆绑技能的 Hermes 指南风格本地插件：

```text
calculator/
├── plugin.yaml
├── __init__.py
├── schemas.py
├── tools.py
├── SKILL.md
└── data/
    └── units.json
```

```bash
edgecrab plugins install ./calculator
edgecrab plugins info calculator
edgecrab plugins status
```

该仓库还附带官方 Hermes 格式示例，由 `edgecrab-official` 搜索源索引：

```bash
edgecrab plugins search --source edgecrab calculator
edgecrab plugins search --source edgecrab json

edgecrab plugins install ./plugins/productivity/calculator
edgecrab plugins install ./plugins/developer/json-toolbox

edgecrab plugins info calculator
edgecrab plugins info json-toolbox
```

这些示例证明了两种不同的 Hermes 运行时表面：

- `plugins/productivity/calculator` 注册工具加上 `post_tool_call` 钩子
- `plugins/developer/json-toolbox` 注册工具加上顶级 CLI 命令

示例：从 `NousResearch/hermes-agent` 的本地克隆直接安装真实 Hermes 资产：

```bash
edgecrab plugins install ~/src/hermes-agent/plugins/memory/holographic
edgecrab plugins info holographic

# pip 入口点插件通过选定的 Python 运行时发现
EDGECRAB_PLUGIN_PYTHON=~/.venvs/hermes/bin/python \
  edgecrab plugins list
EDGECRAB_PLUGIN_PYTHON=~/.venvs/hermes/bin/python \
  edgecrab entry-demo status
```

独立 Hermes 技能从技能表面浏览，而不是插件浏览器：

```bash
edgecrab skills search 1password
edgecrab skills install hermes-agent:security/1password
```

示例：从 `42-evey/hermes-plugins` 搜索和安装精选社区 Hermes 插件：

```bash
edgecrab plugins search --source hermes-evey telemetry
edgecrab plugins install hub:hermes-evey/evey-telemetry
edgecrab plugins install hub:hermes-evey/evey-status
edgecrab plugins info evey-telemetry
```

有关分步创作教程，请参阅 `docs/007_memory_skills/005_building_hermes_style_plugins.md` 和站点指南 `site/src/content/docs/guides/build-hermes-plugin.md`。

兼容性证明目前涵盖：

- 官方仓库 Hermes 示例 `calculator` 和 `json-toolbox`，包括搜索可见性和本地端到端安装/运行时证明
- 指南风格的 Hermes 插件安装和上游"Build a Hermes Plugin"契约的端到端工具执行
- 真实上游 Hermes 插件安装和 `holographic` 的运行时执行
- 通过本地包安装的真实上游 Hermes 可选技能兼容性 `1password`
- 真实上游 Python 导入/运行时 shims 加上 `cli.py register_cli(subparser)` CLI 桥接 `honcho`
- 真实的 `42-evey/hermes-plugins` 运行时执行 `evey-telemetry` 和 `evey-status`
- 通过 `ctx.register_cli_command()` 的 pip 入口点发现和顶级 Hermes CLI 命令执行
- 上游 `plugins/...` 目录和插件浏览器中 `42-evey` repo-root Hermes 目录的 Hermes hub 索引
- CLI 运行时中的完整 Hermes `VALID_HOOKS` 表面：`pre_tool_call`、`post_tool_call`、`pre_llm_call`、`post_llm_call`、`pre_api_request`、`post_api_request`、`on_session_start`、`on_session_end`、`on_session_finalize`、`on_session_reset`
- 网关每聊天会话隔离和 `on_session_start`、`on_session_end`、`on_session_finalize` 和 `on_session_reset` 的会话边界兼容性证明

---

### Cron 调度

调度定期或一次性任务：

```bash
edgecrab cron list
edgecrab cron add "0 9 * * 1-5" "Summarize open PRs for standup"
edgecrab cron add "@daily" "Update MEMORY.md with project progress"
edgecrab cron pause <id>
edgecrab cron resume <id>
edgecrab cron remove <id>
edgecrab cron run <id>      # 手动触发
edgecrab cron tick          # 处理到期任务（由系统 cron 调用）
```

或在 TUI 会话内：
```
/cron list
/cron add "0 18 * * 5" "Generate weekly summary"
```

`manage_cron_jobs` 工具也允许代理自主调度自己的后续任务。

---

### 检查点与回滚

在破坏性操作之前，EdgeCrab 创建文件系统快照：

```bash
# 手动检查点
edgecrab sessions
# → 每次文件写入前自动创建检查点

# 在 TUI 内部
/rollback                    # 恢复最后检查点
/rollback checkpoint-abc123  # 恢复特定检查点
```

配置：
```yaml
checkpoints:
  enabled: true
  max_snapshots: 50    # 每个会话保留最后 50 个检查点
```

`checkpoint` 工具也可供代理本身使用 — 它可以在危险操作前快照，并在出现问题时提供回滚。

---

### 配置文件与工作树

**配置文件**为 EdgeCrab 提供隔离的运行时主目录，具有单独的 `config.yaml`、
`.env`、`SOUL.md`、记忆、技能、插件、钩子、MCP 令牌和
`state.db`。EdgeCrab 现在默认种子三个启动配置文件：
`work`、`research` 和 `homelab`。

```bash
edgecrab profile list                # 默认 + 捆绑启动配置
edgecrab profile show work
edgecrab profile use work            # 粘性默认配置文件
edgecrab -p research "compare SDKs"  # 一次性覆盖
edgecrab profile alias work --name w
edgecrab profile list
```

启动配置文件示例：

```yaml
# ~/.edgecrab/profiles/work/config.yaml
model:
  default: "openai/gpt-5"
  max_iterations: 90

display:
  personality: "technical"
  tool_progress: "verbose"
  show_cost: true

reasoning_effort: "high"
```

```yaml
# ~/.edgecrab/profiles/research/config.yaml
model:
  default: "openai/gpt-5"
  max_iterations: 120

display:
  personality: "teacher"

reasoning_effort: "high"
```

在 TUI 中，`/profile` 现在镜像 Hermes 并显示活动配置文件名称
加上其有效主目录。`/profiles` 打开交互式浏览器，
`/profile show <name>` 将该浏览器跳转到特定配置文件。在内部：`Enter` 切换，
`C` 配置，`S` SOUL，`M` 内存，`T` 工具，`A` 别名，`E` 导出，
`D` 删除，`N` 创建，`I` 导入，`O` 重命名，`Tab` 或 `Left`/`Right`
循环详情视图，`Home`/`End` 跳过结果。运行时
切换是实时的，不延迟到下次启动。

**工作树**在单独的 git 工作树中隔离每个代理会话：

```bash
edgecrab -w "explore that refactor idea safely"
# 在当前 git 仓库内创建 .worktrees/edgecrab-<id>/
# 更改保持隔离在临时分支上，直到您合并或丢弃它们
```

您也可以在配置中启用始终开启的工作树模式：

```yaml
# ~/.edgecrab/config.yaml
worktree: true
```

在 TUI 中，`/worktree` 打开当前检出和保存的启动策略的报告覆盖层，`/worktree on|off|toggle` 更新未来启动的默认值。

`/log` 打开 `~/.edgecrab/logs/` 的分割面板浏览器，`Enter` 深入到选定文件尾部的每条目检查器。覆盖层现在默认实时跟随，`F` 切换跟随模式，`1-5` 或 `/log level <error|warn|info|debug|trace>` 在 `config.yaml` 中持久化默认日志详细级别；当运行时日志重新加载可用时，当前进程立即重新加载其过滤器。

清理设计保守：EdgeCrab 在退出时移除干净的一次性工作树，但保留包含未推送提交的工作树，因此代理无法静默销毁分支本地工作。

---

### 视觉、TTS 与转录

```bash
# 视觉：分析图像
edgecrab "What's in this screenshot?" --attach screenshot.png

# TTS：朗读响应
edgecrab --quiet "Write a haiku about Rust" | say   # 管道到 macOS say
# 或代理可以通过 text_to_speech 工具直接生成音频

# 转录：通过 WhatsApp 网关发送语音笔记
# → EdgeCrab 用 Whisper 转录并响应
```

视觉提供商：任何多模态模型（Claude、GPT-4o、Gemini）。
TTS 提供商：OpenAI TTS、edge-tts（离线）。
转录：Whisper（本地）、Groq Whisper、OpenAI Whisper。

---

## 16 个 LLM 提供商

EdgeCrab 内置 16 个 LLM 提供商（14 个云端，2 个本地）。编译了 200+ 个模型，用户可通过 `~/.edgecrab/models.yaml` 覆盖。

| 提供商 | 环境变量 | 著名模型 |
|--------|----------|----------|
| `copilot` | `GITHUB_TOKEN` 或 VS Code 认证缓存 | `copilot/auto`、GPT-5 mini、GPT-4.1 — 由 GitHub Copilot 路由 |
| `openai` | `OPENAI_API_KEY` | GPT-4.1、GPT-5、o3、o4-mini |
| `anthropic` | `ANTHROPIC_API_KEY` | Claude Opus 4.6、Sonnet 4.6、Haiku 4.5 |
| `google` | `GOOGLE_API_KEY` | Gemini 2.5 Pro、Gemini 2.5 Flash |
| `vertexai` | `GOOGLE_APPLICATION_CREDENTIALS` | 通过 Google Cloud 的 Gemini |
| `nvidia` | `NVIDIA_API_KEY` | NVIDIA NIM（Nemotron、Llama、DeepSeek 系列） |
| `xai` | `XAI_API_KEY` | Grok 3、Grok 4 |
| `deepseek` | `DEEPSEEK_API_KEY` | DeepSeek V3、DeepSeek R1 |
| `mistral` | `MISTRAL_API_KEY` | Mistral Large、Mistral Small |
| `groq` | `GROQ_API_KEY` | Llama 3.3 70B、Gemma2 9B（极快推理） |
| `huggingface` | `HUGGING_FACE_HUB_TOKEN` | 任何 HF Inference API 模型 |
| `zai` | `ZAI_API_KEY` | Z.AI / GLM 系列 |
| `openrouter` | `OPENROUTER_API_KEY` | 600+ 模型通过一个端点 |
| `ollama` | *(无)* | 任何模型 — `ollama serve` 在端口 11434 |
| `lmstudio` | *(无)* | 任何模型 — LM Studio 在端口 1234 |

**随时切换提供商：**
```bash
edgecrab --model openai/gpt-5 "deep code review"
edgecrab --model ollama/llama3.3 "work offline"
edgecrab --model groq/llama-3.3-70b-versatile "quick task"
```

**在 TUI 中热交换：**
```
/model groq/llama-3.3-70b-versatile
/reasoning high                      # 启用扩展思考（Anthropic/OpenAI）
```

**为什么 `copilot/auto` 现在是最佳默认值：** GitHub Copilot 决定您的实时会话使用哪个支持聊天的模型和计费路径。跟随服务器选择避免了可避免的模型特定节流，并使 EdgeCrab 与真实的 VS Code 体验保持一致。

**智能路由**（实验性）：根据轮次复杂度自动选择廉价 vs 全模型：
```yaml
model:
  smart_routing:
    enabled: true
    cheap_model: "groq/llama-3.3-70b-versatile"
```

**混合代理**：通过 4 个前沿模型同时运行单个提示，并获得综合共识：
```
/model moa    # Claude Opus 4.6 + Gemini 2.5 Pro + GPT-4.1 + DeepSeek R1 → 聚合
```

---

## 6 个终端后端

`terminal` 工具是可插拔的。选择您的执行环境：

| 后端 | 激活方式 | 用例 |
|------|----------|------|
| **Local**（默认） | `EDGECRAB_TERMINAL_BACKEND=local` | 您机器上的持久化 shell |
| **Docker** | `backend: docker` | 每个任务的隔离容器 |
| **SSH** | `backend: ssh` | 通过 ControlMaster 的远程服务器 |
| **Modal** | `backend: modal` | 云沙箱（Modal.com） |
| **Daytona** | `backend: daytona` | 持久化云开发沙箱 |
| **Singularity** | `backend: singularity` | 带持久化覆盖的 HPC/Apptainer |

```yaml
terminal:
  backend: docker
  docker:
    image: "python:3.12-slim"
    container_name: "edgecrab-sandbox"
```

---

## MCP 服务器集成

EdgeCrab 是完整的 MCP（模型上下文协议）客户端。连接任何 MCP 服务器，其工具自动对代理可用。

```yaml
# ~/.edgecrab/config.yaml
mcp_servers:
  filesystem:
    command: "npx"
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp/workspace"]

  my-api-server:
    url: "https://my-server.example.com/mcp"
    bearer_token: "${MY_API_TOKEN}"   # 支持环境变量支持的 bearer token
    enabled: true
```

```bash
edgecrab mcp list                                 # 显示配置的 MCP 服务器
edgecrab mcp install filesystem --path "/tmp/ws" # 安装精选预设
edgecrab mcp doctor                              # 静态检查 + 实时探测
edgecrab mcp doctor filesystem                   # 诊断一个配置的服务器
edgecrab mcp remove server-name
/mcp                                             # 打开 TUI MCP 浏览器
/reload-mcp                                      # 在 TUI 中热重载，无需重启
```

代理使用 `mcp_list_tools` 和 `mcp_call_tool` 来发现和调用 MCP 服务器功能。
TUI MCP 浏览器支持安装、查看、测试、诊断和移除流程，引用的
`--path` / `name=` 值安全解析为 Unix 和 Windows 风格路径。
依赖 OAuth 风格 bearer 访问令牌的 HTTP MCP 服务器通过
`bearer_token`、`/mcp-token set <server> <token>` 或环境变量支持的配置值（如
`bearer_token: "${MY_API_TOKEN}"`）支持。

---

## ACP / VS Code Copilot 集成

EdgeCrab 实现了 [Agent Communication Protocol](https://github.com/i-am-bee/acp) — 通过 stdio 的 JSON-RPC 2.0 — 使其能够作为 VS Code Copilot 代理、在 Zed、JetBrains 和任何 ACP 兼容运行器中运行。

```bash
edgecrab acp           # 在 stdin/stdout 上启动 ACP 服务器
edgecrab acp init      # 为工作区搭建 agent.json 清单
```

`acp_registry/agent.json` 清单声明了扩展发现的能力。ACP 适配器使用受限的 `ACP_TOOLS` 子集，排除了仅交互式工具（`clarify`、`send_message`、`text_to_speech`）。

---

## ratatui TUI

支持 60fps，GPU 合成的全屏 TUI，使用 [ratatui](https://ratatui.rs/) 构建。

**布局：**
```
┌────────────────────────────────────────────────────────────┐
│  output area (markdown-rendered, mouse-scrollable)          │
│  ⚙  file_read  src/main.rs                                  │
│     → 342 lines read                                        │
│                                                             │
│  The `main` function initializes the agent loop and...      │
├────────────────────────────────────────────────────────────┤
│ ● openai/gpt-5              1,234t  $0.023  [/commands]  │
├────────────────────────────────────────────────────────────┤
│ ❯ Type your message…                                        │
└────────────────────────────────────────────────────────────┘
```

**功能：**
- **实时活动架** — 思考 / 工具 / 委派阶段，带旋转器、并行工具行和转录与状态栏之间的流式工具参数预览
- **`/agents` 覆盖层** — 监控子代理、杀死子树、生成暂停、轮次差异、甘特图时间线、磁盘 `/replay`
- **`/details` 公开选择器** — 每节隐藏/折叠/展开模式持久化到 YAML
- **`/indicator`** — 热交换状态栏动画样式（颜文字、表情符号、unicode、ascii）
- **队列消息** — 在代理运行时撰写；用 Esc / Ctrl+X / ↑↓ 编辑
- 带逐 token 渲染的流式输出
- Fish 风格幽灵文本（提前输入）完成
- 带模糊匹配覆盖层的 Tab 补全斜杠命令
- 多行输入（Shift+Enter 换行）
- 输出区域中的鼠标滚动
- 危险操作的批准对话框（内联，非阻塞）
- 澄清对话框 — 代理提出问题而不阻塞循环
- 秘密请求覆盖层 — 在会话中提示缺失的 API 密钥
- 状态栏中的会话旋转器 + 模型名称 + token 计数 + 成本

**主题自定义**（`~/.edgecrab/skin.yaml`）：
```yaml
user_fg:      "#89b4fa"   # Catppuccin blue
assistant_fg: "#a6e3a1"   # Catppuccin green
system_fg:    "#f9e2af"   # Catppuccin yellow
error_fg:     "#f38ba8"   # Catppuccin red
tool_fg:      "#cba6f7"   # Catppuccin mauve
status_bg:    "#313244"
status_fg:    "#cdd6f4"
border_fg:    "#6c7086"
prompt_symbol: "❯"
tool_prefix:   "⚙"
```

---

## 所有 CLI 命令

```bash
# 启动
edgecrab                          # 交互式 TUI
edgecrab "prompt here"            # TUI + 自动提交
edgecrab --quiet "prompt"         # 无横幅，管道安全输出
edgecrab --model p/m "prompt"     # 指定 LLM
edgecrab --toolset web,file "p"   # 限制工具集
edgecrab --session id "p"         # 使用特定会话
edgecrab --resume title "p"       # 按标题恢复
edgecrab -C "p"                   # 继续上次会话
edgecrab -w "p"                   # 隔离的 git 工作树
edgecrab -S skill1,skill2 "p"     # 预加载技能

# 设置与诊断
edgecrab setup [--section s] [--force]    # 交互式向导
edgecrab doctor                           # 完整健康检查
edgecrab version                          # 版本 + 提供商
edgecrab migrate [--dry-run]              # 导入 hermes-agent 状态

# 会话
edgecrab sessions list
edgecrab sessions browse
edgecrab sessions export <id> [--format jsonl]
edgecrab sessions delete <id>
edgecrab sessions rename <id> <title>
edgecrab sessions prune [--older-than 30d]
edgecrab sessions stats

# 配置
edgecrab config show
edgecrab config edit
edgecrab config path
edgecrab config set <key> <value>

# 工具
edgecrab tools list
edgecrab tools enable <toolset>
edgecrab tools disable <toolset>

# 提供商
edgecrab auth list
edgecrab auth status [copilot|provider/<name>|mcp/<server>]
edgecrab auth add copilot --token <github-token>
edgecrab auth add provider/openai --token <api-token>   # 写入 ~/.edgecrab/.env 和 ~/.edgecrab/auth.json
edgecrab auth add mcp/<server> --token <bearer-token>
edgecrab auth login [copilot|mcp/<server>]
edgecrab login [target]                                  # 默认 copilot
edgecrab logout [target]                                 # 清除本地认证缓存；提供商目标也清除 auth.json 元数据

# 如果 GitHub Copilot 需要重新登录，EdgeCrab 会打开一个专用的纯终端
# 认证屏幕，使一次性代码易于阅读和鼠标选择。
edgecrab mcp list
edgecrab mcp add <name>
edgecrab mcp remove <name>

# 插件
edgecrab plugins list
edgecrab plugins info <name>
edgecrab plugins status
edgecrab plugins install <source>
edgecrab plugins audit [--lines 20]
edgecrab plugins search <query>
edgecrab plugins search --source hermes <query>
edgecrab plugins browse
edgecrab plugins refresh
edgecrab plugins toggle [name]
edgecrab plugins update [name]
edgecrab plugins remove <name>

# Cron
edgecrab cron list
edgecrab cron add "<schedule>" "<task>"
edgecrab cron run <id>
edgecrab cron tick
edgecrab cron remove <id>
edgecrab cron pause <id>
edgecrab cron resume <id>

# 网关
edgecrab gateway start [--foreground]
edgecrab gateway stop
edgecrab gateway restart
edgecrab gateway status
edgecrab gateway configure [--platform <name>]
edgecrab webhook subscribe <name> [--events push,pull_request] [--skill code-review] [--deliver github_comment] [--deliver-extra repo=org/repo] [--deliver-extra pr_number=42] [--rate-limit 30] [--max-body-bytes 1048576]
edgecrab webhook list
edgecrab webhook test <name>
edgecrab webhook path
edgecrab whatsapp               # WhatsApp QR 配对向导
edgecrab status                 # 整体网关状态

# 清理
edgecrab uninstall --dry-run
edgecrab uninstall --purge-data --yes

# 技能
edgecrab skills list
edgecrab skills view <name>
edgecrab skills search <query>
edgecrab skills install <path|edgecrab:path|owner/repo/path>
edgecrab skills update [name]
edgecrab skills remove <name>

# 配置文件
edgecrab profile list
edgecrab profile use <name>
edgecrab profile create <name>
edgecrab profile delete <name>
edgecrab profile show [name]
edgecrab profile alias <name> [--name alias]
edgecrab profile rename <old> <new>
edgecrab profile export <name> [--output path]
edgecrab profile import <path> [--name name]

# ACP
edgecrab acp                    # 启动 ACP stdio 服务器
edgecrab acp init [--workspace] [--force]

# Shell 补全
edgecrab completion bash
edgecrab completion zsh
edgecrab completion fish
```

---

## 所有斜杠命令

在 TUI 中输入这些命令（在 `❯` 之后）：

每个内置斜杠命令也可以通过 argv 使用 `edgecrab slash <command...>` 访问。

| 命令 | 操作 |
|------|------|
| `/help` | 列出所有斜杠命令及其描述 |
| `/quit` / `/exit` | 退出 EdgeCrab |
| `/clear` | 清除屏幕并开始新会话 |
| `/new` | 开始新会话 |
| `/model [provider/model]` | 无需重启即可热交换 LLM |
| `/reasoning [effort]` | 设置推理努力程度（low/medium/high/auto） |
| `/retry` | 重试上一条消息 |
| `/undo` | 从历史记录中移除上一轮 |
| `/stop` | 中断当前工具执行和生成 |
| `/history` | 显示会话消息历史 |
| `/save [title]` | 用标题保存会话 |
| `/export [format]` | 导出会话（jsonl, markdown） |
| `/title <title>` | 重命名当前会话 |
| `/resume [id-or-title]` | 恢复过去的会话 |
| `/session [list/switch/delete]` | 管理会话 |
| `/config [show/set]` | 查看或更新配置 |
| `/prompt` | 显示、清除或设置自定义系统提示词 |
| `/verbose` | 循环工具进度或显式设置 |
| `/personality [preset]` | 切换代理个性（14 个预设） |
| `/statusbar` | 切换状态栏 |
| `/log [open\|level <level>]` | 浏览本地日志、实时跟随尾部、设置保存的日志级别 |
| `/worktree [status\|on\|off\|toggle]` | 显示当前 git 检出状态和保存的工作树启动策略 |
| `/tools` | 列出活动工具集和工具 |
| `/toolsets` | 显示工具集别名和扩展 |
| `/mcp [subcommand]` | 浏览、安装、测试、诊断或移除 MCP 服务器 |
| `/reload-mcp` | 热重载 MCP 服务器（无需重启） |
| `/mcp-token <server> <token>` | 在运行时设置 MCP bearer token |
| `/plugins [info/status/install/enable/disable/toggle/audit/hub]` | 浏览已安装插件并管理插件操作 |
| `/memory [show/edit]` | 查看或编辑代理内存 |
| `/cost` | 显示本次会话的 token 成本 |
| `/usage` | 详细使用情况分解 |
| `/compress` | 立即强制上下文压缩 |
| `/insights [days]` | 显示会话统计和 N 天历史分析 |
| `/skin [preset]` | 浏览或切换皮肤（`/theme` 别名） |
| `/paste` | 切换粘贴模式（多行剪贴板输入） |
| `/queue <message>` | 在代理运行时队列消息 |
| `/background` | 将当前任务 fork 到后台，释放 TUI |
| `/rollback [checkpoint]` | 将文件系统恢复到检查点 |
| `/platforms` | 显示连接的网关平台 |
| `/approve` | 批准待处理的代理操作 |
| `/deny` | 拒绝待处理的代理操作 |
| `/sethome` | 配置网关主频道 |
| `/update` | 检查 EdgeCrab 更新 |
| `/cron [list/add/remove]` | 在线管理 cron 任务 |
| `/voice <on/off/status>` | 切换语音输出 |
| `/skills [list/view/install/remove/hub]` | 管理技能 |
| `/doctor` | 运行在线健康诊断 |
| `/version` | 显示版本和提供商信息 |

键盘快捷键：

| 按键 | 操作 |
|------|------|
| `Enter` | 提交提示词 |
| `Shift+Enter` | 输入新行 |
| `Ctrl+C` | 中断运行中的代理 |
| `Ctrl+L` | 清除输出区域 |
| `Ctrl+U` | 清除输入行 |
| `Ctrl+B` / `Ctrl+F` | 当终端吞没 PgUp/PgDn 时的备用上下翻页 |
| `Alt+↑` / `Alt+↓` | 滚动输出 |
| `Ctrl+Home` / `Ctrl+End` | 跳转到输出顶部/底部 |
| `Tab` | 接受幽灵文本 / 循环斜杠命令补全 |

终端故障排除：

- 如果 `PgUp` / `PgDn` 无法到达 EdgeCrab，请使用 `Ctrl+B` / `Ctrl+F`。
- 在 macOS Terminal.app 上，EdgeCrab 现在以保守兼容性模式启动：默认关闭鼠标捕获，并自动启用备用翻页键。
- 您可以使用 `EDGECRAB_TUI_COMPAT=1 edgecrab` 在任何终端中强制该模式。

---

## 安全模型

安全性是编译在内的 — 不是事后补丁。EdgeCrab 在七个独立层应用纵深防御：

| 层 | 机制 | 位置 |
|------|------|------|
| **文件 I/O** | 所有路径规范化，检查 `allowed_roots`。`SanitizedPath` 是独特的 Rust 类型 — 绕过它是编译错误。 | `edgecrab-security::path_safety` |
| **Web 工具** | SSRF 防护在任何出站 HTTP 调用前阻止私有 IP 范围（10.x、192.168.x、172.16.x、127.x、::1）。`SafeUrl` 独特类型。 | `edgecrab-security::ssrf` |
| **终端** | 命令注入扫描（Aho-Corasick + 正则表达式）覆盖 8 个危险类别，拒绝 shell 元字符和禁止模式。 | `edgecrab-security::command_scan` |
| **上下文文件** | 在 SOUL.md、AGENTS.md、.cursor/rules 中扫描提示注入模式（正则表达式 + 不可见 Unicode + 同形字）。高严重性被 `[BLOCKED: ...]` 阻止。 | `prompt_builder.rs` |
| **代码执行沙箱** | API 密钥/令牌从子环境中剥离。仅通过 Unix 套接字 RPC 暴露 7 个白名单工具存根。超时采用 `SIGTERM→SIGKILL` 升级。 | `execute_code.rs` |
| **技能安装** | 外部技能在安装前通过 23 模式威胁扫描器（数据泄露、注入、破坏性操作、持久化、混淆）。 | `skills_guard` |
| **LLM 输出** | 编辑管道在显示或记录任何 LLM 响应前剥离秘密和令牌。 | `edgecrab-security::redact` |

路径安全和 SSRF 使用 Rust 的**类型系统**作为主要控制 — 不仅是运行时检查。如果您的代码没有 `SanitizedPath`，它就不能调用文件 I/O。就是这样。

---

## 架构

EdgeCrab 是一个 11 个 crate 的 Rust 工作区。依赖图是严格的 DAG — 无循环依赖，无反转图的功能标志。

```
edgecrab-types      (共享类型 — 无其他 crate 依赖)
       ↑
edgecrab-security   (路径安全，SSRF，命令扫描 — 仅类型)
edgecrab-cron       (独立 cron 存储 + 调度解析器)
       ↑
edgecrab-tools      (ToolRegistry + 内置工具实现)
edgecrab-lsp        (语言服务器客户端，文档同步，语义工具)
edgecrab-state      (SQLite WAL + FTS5 会话存储)
       ↑
edgecrab-core       (Agent，ReAct 循环，提示词构建器，压缩)
       ↑
edgecrab-cli    edgecrab-gateway    edgecrab-acp    edgecrab-migrate
```

| Crate | 职责 |
|-------|------|
| `edgecrab-types` | `Message`、`Role`、`ToolCall`、`ToolSchema`、`Usage`、`Cost`、`AgentError`、`Trajectory` — 所有共享且无业务逻辑 |
| `edgecrab-security` | 路径监狱，SSRF，命令扫描，编辑，批准引擎 |
| `edgecrab-state` | SQLite WAL + FTS5 会话存储（`~/.edgecrab/state.db`） |
| `edgecrab-cron` | Cron 表达式解析器，任务存储（`~/.edgecrab/cron/`） |
| `edgecrab-tools` | `ToolRegistry`、`ToolHandler` trait、`ToolContext`，包括浏览器、MCP、媒体和 LSP 的内置工具表面 |
| `edgecrab-lsp` | 语言服务器管理器，JSON-RPC 客户端，文档同步，诊断，编辑应用，以及 `lsp_*` 工具处理程序 |
| `edgecrab-core` | `Agent`、`AgentBuilder`、`execute_loop()`、`PromptBuilder`、压缩、路由、200+ 模型目录 |
| `edgecrab-cli` | ratatui TUI，42 个斜杠命令，所有 CLI 子命令，皮肤引擎，配置文件 |
| `edgecrab-gateway` | axum HTTP + 15 个平台适配器，流式传输交付，`MEDIA://` 协议 |
| `edgecrab-acp` | 用于 VS Code / Zed / JetBrains 的 ACP JSON-RPC 2.0 stdio 适配器 |
| `edgecrab-migrate` | hermes-agent → EdgeCrab 状态导入，模式迁移 |

**关键设计决策（来自代码）：**

1. **单一二进制** — 静态链接嵌入所有依赖（TLS、SQLite、Aho-Corasick）。除 OS 外无共享库。
2. **类型级安全** — `SanitizedPath` 和 `SafeUrl` 在 `edgecrab-types` 中是独特类型。绕过清理是编译错误。
3. **编译时工具注册表** — `inventory::submit!()` 在链接时注册工具。零启动成本。所有工具根据功能标志存在或不存在，而非运行时配置。
4. **每个会话单一系统提示词** — 构建一次，缓存在 `SessionState.cached_system_prompt` 中。压缩从不重建它（保留 Anthropic 提示词缓存命中）。
5. **可热交换模型** — `Agent` 中的 `RwLock<Arc<dyn LLMProvider>>`。进行中的对话保留其 Arc 克隆；交换仅影响新轮次。

---

## 配置

EdgeCrab 使用分层配置：`defaults → ~/.edgecrab/config.yaml → EDGECRAB_* 环境变量 → CLI 标志`。后出现的层优先。

```yaml
# ~/.edgecrab/config.yaml

model:
  default_model: "ollama/gemma4:latest"
  max_iterations: 90          # 每个会话的 ReAct 循环预算
  streaming: true
  smart_routing:
    enabled: false
    cheap_model: ""

display:
  skin: "catppuccin"
  show_reasoning: false

logging:
  level: "info"              # error | warn | info | debug | trace

worktree: false               # true = 默认在隔离的 git 工作树中启动代理会话

tools:
  enabled_toolsets: null       # null = 所有工具集激活
  disabled_toolsets: null
  file:
    allowed_roots: []          # 空 = 仅当前目录
  custom_groups:
    backend-dev:
      - read_file
      - write_file
      - terminal
      - session_search

lsp:
  enabled: true
  file_size_limit_bytes: 10000000
  servers:
    rust:
      command: "rust-analyzer"
      args: []
      file_extensions: ["rs"]
      language_id: "rust"
      root_markers: ["Cargo.toml", "rust-project.json"]

memory:
  enabled: true

skills:
  enabled: true
  preloaded: []

delegation:
  enabled: true
  model: null                  # null = 使用默认模型
  max_subagents: 3
  max_iterations: 50

terminal:
  backend: local               # local | docker | ssh | modal | daytona | singularity
  docker:
    image: "ubuntu:22.04"

browser:
  record_sessions: false

checkpoints:
  enabled: true
  max_snapshots: 50

gateway:
  host: "0.0.0.0"
  port: 8642
  enabled_platforms: []        # ["telegram", "discord", ...]
  whatsapp:
    enabled: false
    mode: "self-chat"          # self-chat | any-sender
    allowed_users: []

security:
  path_restrictions: []

mcp_servers:
  my-server:
    command: "npx"
    args: ["-y", "@modelcontextprotocol/server-example"]
    enabled: true
```

关键环境变量：
```bash
EDGECRAB_MODEL=openai/gpt-5
EDGECRAB_MAX_ITERATIONS=120
EDGECRAB_TERMINAL_BACKEND=docker
EDGECRAB_SKIP_MEMORY=false
EDGECRAB_SAVE_TRAJECTORIES=true
```

---

## SDKs：一个 EdgeCrab 体验

EdgeCrab 为 Rust、Python、Node.js 和 WASM 提供一流的 SDK 表面。发布的包名称保持简单 — `edgecrab`、`edgecrab-sdk` 和 `@edgecrab/wasm`。规范的 Python SDK 现在直接位于 `sdks/python` 下用于发布和分发。

### Python SDK（`edgecrab`）

**Python 3.10+ — 异步优先，流式传输，会话，以及 E2E 支持的示例。**

```bash
pip install edgecrab
```

```python
from edgecrab import Agent

# 简单聊天
agent = Agent(model="openai/gpt-4o")
reply = agent.chat("Explain Rust ownership in 3 sentences")
print(reply)

# 异步流式传输
import asyncio
from edgecrab import AsyncAgent

async def main():
    agent = AsyncAgent(model="copilot/gpt-5-mini")
    async for token in agent.stream("Write a Rust hello-world"):
        print(token, end="", flush=True)

asyncio.run(main())
```

内置 CLI：
```bash
edgecrab chat "Hello, EdgeCrab!"
edgecrab models
edgecrab health
```

完整文档：[Python SDK README](sdks/python/README.md)

### Node.js SDK（`edgecrab`）

**Node 18+ — TypeScript 优先，流式传输，以及原生运行时访问。**

```bash
npm install edgecrab
```

```typescript
import { Agent } from 'edgecrab';

// 简单聊天
const agent = new Agent({ model: 'openai/gpt-4o' });
const reply = await agent.chat('Explain Rust ownership');
console.log(reply);

// 流式传输
for await (const token of agent.stream('Write a README')) {
  process.stdout.write(token);
}
```

通过 npx 使用 CLI：
```bash
npx edgecrab chat "Hello!"
npx edgecrab models
```

完整文档：[Node.js SDK README](sdks/nodejs-native/README.md)

---

## Docker

在容器中运行 EdgeCrab 作为网关服务器：

```bash
# 拉取多架构 GHCR 镜像
docker pull ghcr.io/raphaelmansuy/edgecrab:latest

# 运行网关服务器
docker run -p 8642:8642 \
  -e ANTHROPIC_API_KEY="$ANTHROPIC_API_KEY" \
  -e TELEGRAM_BOT_TOKEN="$TELEGRAM_BOT_TOKEN" \
  -v "$HOME/.edgecrab:/root/.edgecrab" \
  ghcr.io/raphaelmansuy/edgecrab:latest

# 或使用 docker-compose
docker compose up -d
```

Docker 镜像是多阶段的，约 50 MB（无发行版最终阶段）。多架构：`linux/amd64` + `linux/arm64`。使用 `rustls-tls` — 无 OpenSSL 依赖，实现干净的交叉编译。

---

## 从 hermes-agent 迁移

EdgeCrab 通过一个命令导入您的整个 hermes-agent 状态：

```bash
# 先预览（不做任何更改）
edgecrab migrate --dry-run

# 实时迁移
edgecrab migrate

# OpenClaw 导入
edgecrab claw migrate --dry-run
edgecrab claw migrate
```

| 内容 | 来源 | 目标 |
|------|------|------|
| 配置 | `~/.hermes/config.yaml` | `~/.edgecrab/config.yaml` |
| 记忆 | `~/.hermes/memories/` | `~/.edgecrab/memories/` |
| 技能 | `~/.hermes/skills/` | `~/.edgecrab/skills/` |
| 环境 | `~/.hermes/.env` | `~/.edgecrab/.env` |

迁移器位于 `crates/edgecrab-migrate/`。它返回带有每项 `MigrationStatus`（Success/Skipped/Failed）的 `MigrationReport`。配置格式差异自动处理。

对于 OpenClaw，EdgeCrab 导入干净映射到 EdgeCrab 原生状态的部分（`SOUL.md`、记忆、技能、选定的 `.env` 键、选定的配置节），并将不支持的 OpenClaw 专用配置归档到 `~/.edgecrab/migration/openclaw/` 供手动审查。

---

## 测试

```bash
# 根便捷目标
cargo run

# 运行所有单元 + 集成测试
cargo test --workspace

# 仅运行特定 crate
cargo test -p edgecrab-core
cargo test -p edgecrab-tools
cargo test -p edgecrab-gateway

# 运行 E2E 测试（需要配置的 LLM 提供商）
cargo test --workspace -- --include-ignored

# 静态检查（零警告策略）
cargo clippy --workspace -- -D warnings

# 格式检查
cargo fmt --check

# 构建文档
cargo doc --no-deps --open
```

当前：**1629 个测试通过**（单元 + 集成）。代码库在 CI 中强制执行零 clippy 警告策略。

> **注意：** `edgecrab-cli` 中的 8 个差距审计测试需要 `../hermes-agent/` 处的 hermes-agent 源代码树。独立开发时跳过它们：`cargo test --workspace --exclude edgecrab-cli`

---

## 项目结构

```
edgecrab/
├── crates/
│   ├── edgecrab-types/         共享类型 — Message、Role、ToolCall、错误
│   ├── edgecrab-security/      路径监狱、SSRF、命令扫描器、注入、编辑
│   ├── edgecrab-state/         SQLite WAL + FTS5 会话存储
│   ├── edgecrab-cron/          Cron 解析器、任务存储、调度器
│   ├── edgecrab-tools/         ToolRegistry + 内置工具实现
│   │   └── tools/
│   │       ├── file.rs         read_file、write_file、patch_file、search_files
│   │       ├── terminal.rs     terminal、manage_process
│   │       ├── web.rs          web_search、web_extract
│   │       ├── browser.rs      CDP 浏览器自动化（6 个工具）
│   │       ├── memory.rs       memory_read、memory_write、Honcho 工具
│   │       ├── delegate_task.rs 子代理委派 + 批处理并行
│   │       ├── execute_code.rs 沙箱多语言代码执行
│   │       ├── vision.rs       vision_analyze、text_to_speech、transcribe_audio
│   │       └── ...             session、cron、checkpoint、skills、mcp、todo、HA
│   ├── edgecrab-lsp/           LSP 客户端、文档同步、诊断、语义编辑
│   ├── edgecrab-core/
│   │   └── src/
│   │       ├── agent.rs        AgentBuilder、Agent、StreamEvent、fork_isolated
│   │       ├── conversation.rs execute_loop() — ReAct 引擎
│   │       ├── compression.rs  上下文窗口压缩
│   │       ├── prompt_builder.rs 系统提示词从 9+ 源组装
│   │       ├── model_router.rs 智能路由（廉价 vs 全模型）
│   │       └── model_catalog.rs 200+ 模型，用户可覆盖的 YAML
│   ├── edgecrab-cli/           ratatui TUI、斜杠命令、皮肤引擎、配置文件
│   ├── edgecrab-gateway/       axum + 15 平台适配器、流式传输交付
│   ├── edgecrab-acp/           ACP JSON-RPC 2.0 stdio 适配器
│   └── edgecrab-migrate/       hermes-agent 导入 + 模式迁移
├── sdks/
│   ├── python/                 Python SDK（PyPI 上的 edgecrab）
│   └── node/                   Node.js SDK（npm 上的 edgecrab-sdk）
├── site/                       Astro 文档网站
├── docs/                       规范文档
├── acp_registry/
│   └── agent.json              VS Code Copilot 代理清单
├── .github/workflows/          CI + 4 个发布工作流（Rust/Python/Node/Docker）
├── Dockerfile                  多阶段、无发行版、多架构
└── docker-compose.yml          一键网关部署
```

---

## 要求与构建

| 工具 | 版本 |
|------|------|
| Rust | 1.86+ |
| Cargo | 随 Rust 捆绑 |
| OS | macOS、Linux、Windows |

```bash
# 调试构建（快速迭代）
cargo build --workspace

# 发布构建（优化，启动速度比调试快约 3 倍）
cargo build --workspace --release

# 在 macOS 上交叉编译 Linux
cargo build --release --target x86_64-unknown-linux-musl
```

发布二进制文件是静态链接的 — 无 OpenSSL，无 libc 版本问题。将其放到任何 Linux 机器上即可运行。

---

## 贡献

EdgeCrab 欢迎贡献。代码库有零 clippy 警告策略，并强制执行 `cargo fmt`。

```bash
git clone https://github.com/raphaelmansuy/edgecrab
cd edgecrab
cargo build --workspace                    # 验证编译
cargo test --workspace                     # 运行测试套件
cargo clippy --workspace -- -D warnings    # 必须零警告
```

**添加新工具：**
1. 创建 `crates/edgecrab-tools/src/tools/my_tool.rs`
2. 实现 `ToolHandler` trait（name、schema、execute、toolset、emoji）
3. 使用 `inventory::submit!(RegisteredTool { handler: &MyTool })` 注册
4. 在 `crates/edgecrab-tools/src/tools/mod.rs` 中声明

**添加新网关：**
1. 创建 `crates/edgecrab-gateway/src/my_platform.rs`
2. 实现 `PlatformAdapter` trait
3. 在 `crates/edgecrab-gateway/src/run.rs` 中注册

**安全报告：** `security@elitizon.com`

详见 [CONTRIBUTING.md](CONTRIBUTING.md)。

---

## 发布渠道

| 渠道 | 制品 | 安装方式 |
|------|------|----------|
| **npm** | `edgecrab-cli`（二进制包装器 — 无需 Rust） | `npm install -g edgecrab-cli` |
| **pip** | `edgecrab-cli`（二进制包装器 — 无需 Rust） | `pip install edgecrab-cli` |
| **cargo** | Rust crates（12 个 crates 发布） | `cargo install edgecrab-cli` |
| **Python SDK** | `edgecrab` | `pip install edgecrab` |
| **Node SDK** | `edgecrab-sdk` | `npm install edgecrab-sdk` |
| **Docker** | GHCR 多架构 | `docker pull ghcr.io/raphaelmansuy/edgecrab:latest` |
| **Binary** | GitHub Release 归档 | [Releases 页面](https://github.com/raphaelmansuy/edgecrab/releases) |

发布自动化：`.github/workflows/release-rust.yml`、`release-python.yml`、`release-node.yml`、`release-docker.yml`。

---

## 许可证

Apache-2.0 — 见 [LICENSE](LICENSE)。

由 [Elitizon](https://elitizon.com) 构建 · 受 [Nous Hermes Agent](https://github.com/NousResearch) 和 [OpenClaw](https://github.com/openclaw) 启发。