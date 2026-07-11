# 🦀 配置和路径

> **为什么：** 一个能在笔记本电脑、树莓派和云服务器上运行的单一二进制文件需要一个确定性的分层配置系统。EdgeCrab 使用四层合并栈，因此每个默认值都可以在正确的范围内被覆盖 — 而无需触碰不相关的设置。

**来源：** `crates/edgecrab-core/src/config.rs`, `crates/edgecrab-cli/src/profile.rs`

---

## 四层加载顺序

```
┌─────────────────────────────────────────┐
│  Tier 1 — 编译时内置默认值            │  always present, never missing
└──────────────────────┬──────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────┐
│  Tier 2 — config.yaml                  │  $EDGECRAB_HOME/config.yaml
│            (或 ~/.edgecrab/config.yaml) │  或 profile home/config.yaml
└──────────────────────┬──────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────┐
│  Tier 3 — EDGECRAB_* 环境变量          │  EDGECRAB_MODEL, EDGECRAB_MAX_ITERATIONS…
└──────────────────────┬──────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────┐
│  Tier 4 — CLI 标志                      │  --model, --iterations, --no-memory…
└─────────────────────────────────────────┘
                       │
                       ▼
                  AppConfig
             (single merged view)
```

**规则：** 后面的层级总是获胜。CLI 标志胜过环境变量；环境变量胜过 config.yaml；config.yaml 胜过编译时默认值。

---

## `AppConfig` 顶层章节

| 章节 | 用途 |
|---|---|
| `model` | 默认模型、温度、上下文窗口 |
| `agent` | 最大迭代次数、反思阈值 |
| `tools` | 工具集允许列表、MCP 服务器列表 |
| `gateway` | 平台适配器设置（Telegram token、Slack credentials...） |
| `mcp_servers` | MCP 服务器定义（名称、命令、参数、环境变量） |
| `memory` | 基于文件的记忆启用/禁用、最大 token 数 |
| `skills` | 技能目录、自动发现 |
| `security` | 批准模式、命令扫描策略、路径监狱根 |
| `terminal` | Shell、PTY 设置 |
| `delegation` | 子智能体并发、预算 |
| `compression` | 触发阈值、目标比例、摘要模型 |
| `display` | 颜色、TUI、流式传输 |
| `privacy` | 脱敏模式、遥测退出 |
| `browser` | Playwright/CDP 设置 |
| `checkpoints` | 频率、存储路径 |
| `tts` / `stt` / `voice` | 音频 I/O 设置 |
| `image_generation` | 默认图像生成后端和设置 |
| `honcho` | Honcho 用户模型记忆服务 |
| `auxiliary` | 辅助模型设置，如视觉覆盖 |
| `moa` | 默认混合智能体聚合器和参考名单 |

顶层运行时标志（不在章节内嵌套）：

| 标志 | 默认值 | 含义 |
|---|---|---|
| `save_trajectories` | `false` | 每次会话后写入 JSONL 重放文件 |
| `worktree` | `false` | 默认在隔离的 git worktree 中启动智能体会话 |
| `logging.level` | `"info"` | 默认集中式日志详细程度 |
| `skip_context_files` | `false` | 跳过 `CLAUDE.md` / `AGENT.md` 注入 |
| `skip_memory` | `false` | 跳过基于文件的记忆注入 |
| `timezone` | system TZ | 覆盖 cron 和时间戳的时区 |
| `reasoning_effort` | `"medium"` | 传递给支持它的模型 |

---

## 关键环境变量

```bash
# 模型覆盖 — 尝试新模型而无需编辑 config.yaml 的最快方法
EDGECRAB_MODEL="anthropic/claude-opus-4-6-20260219"

# 安全上限 — 拒绝每个会话超过 N 次循环
EDGECRAB_MAX_ITERATIONS=40

# 强制使用 UTC，无论本地机器时区如何
EDGECRAB_TIMEZONE="UTC"

# 为每个会话写入 JSONL 轨迹文件
EDGECRAB_SAVE_TRAJECTORIES=true

# 默认在隔离的 git worktree 中启动智能体会话
EDGECRAB_WORKTREE=true

# 覆盖默认集中式日志级别
EDGECRAB_LOG_LEVEL=debug

# 完全跳过注入 CLAUDE.md / AGENT.md 文件
EDGECRAB_SKIP_CONTEXT_FILES=true

# 禁用基于文件的记忆注入
EDGECRAB_SKIP_MEMORY=true

# 控制过大工具结果的溢出到制品
EDGECRAB_TOOL_RESULT_SPILL=true
EDGECRAB_TOOL_RESULT_SPILL_THRESHOLD=16384
EDGECRAB_TOOL_RESULT_SPILL_PREVIEW_LINES=80
```

网关特定和终端特定的变量遵循相同的 `EDGECRAB_` 前缀约定；完整的列表请参阅网关和安全文档。

---

## 主目录布局

```
~/.edgecrab/              ← $EDGECRAB_HOME (默认)
├── config.yaml           ← 主配置 (Tier 2)
├── auth.json             ← 结构化提供商认证元数据和活动提供商
├── .env                  ← 提供商 API 密钥和其他本地秘密
├── models.yaml           ← 带成本元数据的模型目录
├── SOUL.md               ← 持久个性/系统提示附录
├── state.db              ← SQLite 会话存储（schema v6）
├── memories/             ← 基于文件的记忆 Markdown 文件
├── skills/               ← SKILL.md 技能定义
├── hooks/                ← 脚本钩子目录
│   └── my-hook/
│       ├── HOOK.yaml
│       └── handler.py
└── profiles/             ← 命名配置文件目录
    ├── work/
    │   ├── config.yaml
    │   ├── .env
    │   ├── SOUL.md
    │   └── state.db
    └── personal/
        └── …
```

> **提示：** `EDGECRAB_HOME` 是将整个主目录移动到不同路径的唯一环境变量 — 对容器有用（`EDGECRAB_HOME=/data/.edgecrab`）。

---

## 配置文件

每个配置文件都是一个隔离的运行时上下文。配置文件切换更改所有后续命令的有效主目录。

```
~/.edgecrab/profiles/<name>/
├── config.yaml     ← 配置文件特定的覆盖
├── auth.json       ← 配置文件范围的提供商认证元数据
├── .env            ← 配置文件特定的秘密（在环境变量之前加载）
├── SOUL.md         ← 配置文件特定的个性
├── memories/       ← 配置文件特定的持久记忆
├── skills/         ← 配置文件特定的技能
├── plugins/        ← 配置文件特定的插件
├── hooks/          ← 配置文件特定的钩子
└── state.db        ← 配置文件特定的会话存储
```

EdgeCrab 在正常启动和配置文件命令时播种捆绑的入门配置文件：`work`、`research` 和 `homelab`。这些在 `~/.edgecrab/profiles/` 下创建一次，从不覆盖现有用户编辑的配置文件。

**配置文件共享的内容：** `edgecrab` 二进制文件、全局粘性配置文件标记 `~/.edgecrab/.active_profile` 以及仓库本地上下文文件如 `AGENTS.md`。

**配置文件隔离的内容：** 对话历史、秘密、模型选择、记忆、技能、插件、钩子、MCP token 和个性。

```bash
# 为本会话切换到 "work" 配置文件
edgecrab --profile work

# 在 "personal" 配置文件下运行一次性命令
edgecrab --profile personal "summarise my notes"
```

---

## OpenAI 兼容提供商配置

对于支持 OpenAI 兼容 API 的提供商（如讯飞 MaaS API、Groq、DeepSeek），使用 `openai-compatible` 提供商类型：

```yaml
provider: openai-compatible
model:
  default: xopkimik26
  base_url: https://maas-coding-api.cn-huabei-1.xf-yun.com/v2
  api_key_env: xfyun_API_KEY
  streaming: false
```

- **`provider`**: 设置为 `openai-compatible` 以使用 OpenAI 兼容 API
- **`model.default`**: 模型名称（不带提供商前缀）
- **`model.base_url`**: API 端点 URL
- **`model.api_key_env`**: 包含 API 密钥的环境变量名称
- **`model.streaming`**: 如果提供商不支持流式传输，设置为 `false`

`provider` 和 `model.default` 值会在运行时自动合并为 `openai-compatible/model_name` 格式。

---

## 最小 `config.yaml` 示例

```yaml
model:
  default: "anthropic/claude-sonnet-4-20250514"
  temperature: 0.3
  smart_routing:
    enabled: true
    cheap_model: "anthropic/claude-haiku-4-5-20251001"

agent:
  max_iterations: 30

security:
  approval_mode: "on_risk"   # never | on_risk | always

compression:
  trigger_ratio: 0.80        # 压缩当上下文达到 80% 时
  target_ratio: 0.40         # 缩小到窗口的 40%

tools:
  result_spill: true
  result_spill_threshold: 16384
  result_spill_preview_lines: 80

memory:
  enabled: true
  max_inject_tokens: 4000

moa:
  enabled: true
  aggregator_model: "anthropic/claude-opus-4.6"
  reference_models:
    - "anthropic/claude-opus-4.6"
    - "openai/gpt-4.1"
```

---

## 提示

- **不要在 `config.yaml` 中存储秘密 —** 使用配置文件 `.env` 文件或真实的环境变量；秘密通过 `edgecrab-security/src/redact.rs` 从日志中脱敏，但仅当它们包含已知模式时。
- **提供商认证现在有两个本地层 —** `auth.json` 跟踪活动提供商和元数据，而 `.env` 仍然携带运行时使用的实际提供商 API 密钥材料。
- **`SOUL.md` 文件是给 EdgeCrab 持久个性的最快方法，而无需修改代码。** 它附加到每轮的系统提示。
- **`models.yaml` 控制成本跟踪 —** 如果添加新模型，添加成本条目以便 `/cost` 和轨迹文件准确报告。
- **溢出的工具制品是工作区本地的，不是主目录本地的 —** 大型成功的工具结果写在活动的 cwd 下的 `.edgecrab-artifacts/<session_id>/` 中，这样智能体可以通过正常的文件工具读取它们。

---

## 常见问题

**问：我可以有每个项目的配置吗？**
答：可以。在项目目录中放置 `config.yaml` 并使用 `EDGECRAB_HOME=$(pwd)` 启动 EdgeCrab — Tier 2 会拾取它。

**问：当 `EDGECRAB_MODEL` 已设置且 `config.yaml` 有模型且 CLI 传递 `--model` 时，哪个配置值获胜？**
答：CLI 标志（`--model`）获胜。Tier 4 > Tier 3 > Tier 2。

**问：更改 `config.yaml` 是否需要重启 EdgeCrab？**
答：对于 CLI 会话，是的。对于网关长时间运行进程，优雅重启是最安全的路径。

---

## 交叉引用

- 记忆注入详情 → [`007_memory_skills/001_memory_skills.md`](../007_memory_skills/001_memory_skills.md)
- 安全门控设置 → [`011_security/001_security.md`](../011_security/001_security.md)
- 会话存储（`state.db`）→ [`009_config_state/002_session_storage.md`](002_session_storage.md)
- 模型路由配置 → [`003_agent_core/005_smart_model_routing.md`](../003_agent_core/005_smart_model_routing.md)
- 钩子发现路径 → [`hooks.md`](../hooks.md)
