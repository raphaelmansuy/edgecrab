# 提示构建器 🦀

> **已验证来源：** `crates/edgecrab-core/src/prompt_builder.rs`

---

## 为什么需要集中式提示构建器

手工编写的系统提示在多前端 agent 中是一种反模式。如果 CLI、网关和 ACP 服务器各自组装自己的提示字符串，你会得到：三份内存注入逻辑的副本，三个在添加新指导块时需要更新的位置，以及三种需要调试的不同提示格式。

`PromptBuilder` 是唯一的组装点。所有前端在会话开始时调用它一次。结果缓存在 `SessionState::cached_system_prompt` 中，并在后续每次 API 调用时复用 — 提示构造不是免费的。

---

## 扫描的上下文文件 (按顺序)

```
  全局身份：
    ~/.edgecrab/SOUL.md          ← 身份槽位（从不作为通用上下文注入）

  项目指令（扫描 cwd）：
    .edgecrab.md                 ← 主要项目文件
    EDGECRAB.md
    .hermes.md                   ← 遗留兼容性
    HERMES.md
    AGENTS.md                    ← OpenAI Agents SDK 标准
    CLAUDE.md                    ← Anthropic Claude Code 标准
    .cursorrules                 ← Cursor 兼容性
    .cursor/rules/*.mdc          ← Cursor 规则文件

  所有项目上下文文件在包含前经过注入检查。
```

`SOUL.md` 被特殊处理为 persona/身份槽位 — 它设置 agent 的角色。项目文件（`AGENTS.md`、`CLAUDE.md`、`.edgecrab.md`）在 persona 之上添加项目特定指令。

---

## 提示组装顺序

```
  ┌─────────────────────────────────────────────────────────────────┐
  │  最终系统提示                                                 │
  │                                                                  │
  │  [1] SOUL.md 内容（如果存在）                                   │
  │       └── "你是 EdgeCrab，一个强大的编程助手..."               │
  │                                                                  │
  │  [2] 平台提示                                                 │
  │       └── "你在 platform: telegram 上运行"                    │
  │                                                                  │
  │  [3] 时间戳                                                   │
  │       └── "当前时间是 2026-04-05 14:32 UTC"                   │
  │                                                                  │
  │  [4] 上下文文件 (AGENTS.md, CLAUDE.md, .edgecrab.md, ...)     │
  │      每个在包含前经过注入检查                                     │
  │                                                                  │
  │  [5] 内存指导块（如果启用内存工具）                             │
  │       └── "当你学到一些东西时，写入内存..."                       │
  │                                                                  │
  │  [6] 内存内容（来自 ~/.edgecrab/memories/）                    │
  │       └── 每个内存部分文件的内容                                 │
  │                                                                  │
  │  [7] 会话搜索指导（如果启用会话工具）                           │
  │                                                                  │
  │  [8] 技能指导（如果启用技能工具）                               │
  │       └── 如何调用、安装、创建技能                               │
  │                                                                  │
  │  [9] 技能摘要（来自 ~/.edgecrab/skills/ 扫描）                 │
  │       └── 每个已安装技能的简要描述                               │
  │                                                                  │
  │  [10] 工具特定指导块                                           │
  │        cron 指导（如果启用 manage_cron_jobs）                    │
  │        messaging 指导（如果启用 send_message）                   │
  │        image analysis 指导（如果启用 vision_analyze）            │
  └─────────────────────────────────────────────────────────────────┘
```

---

## 技能摘要缓存

技能目录（`~/.edgecrab/skills/`）可能包含数千个文件。在每次会话开始时扫描它会明显减慢启动速度。构建器使用基于进程的 `OnceLock` 缓存：

```
  第一次会话：
    扫描 ~/.edgecrab/skills/，读取 frontmatter
    提取每个技能的 name + description
    缓存摘要字符串
    ↓
  本进程的所有后续会话：
    立即返回缓存字符串（无磁盘 I/O）

  明确失效：
    Agent::invalidate_system_prompt()
    → 下次 build() 重新扫描技能目录
```

---

## 注入检查

在任何外部内容（上下文文件、内存文件、技能文件）进入系统提示之前，它通过以下检查：
```sh
edgecrab_security::injection::check_injection(content)
```

这扫描：
- 提示注入模式：`"ignore previous instructions"`、`"you are now"`、`"system prompt:"`、HTML 注释 `<!--` 等
- 不可见 Unicode 字符（零宽空格、方向覆盖）
- 内存中的泄露模式：带有秘密环境变量的 `curl`/`wget`、`cat ~/.ssh/` 等

失败的文件被**替换**为占位符而不是静默丢弃或原样使用。

🦀 *包含注入指令的恶意 `AGENTS.md` 是对任何在未扫描的情况下加载上下文文件的 agent 的风险。EdgeCrab 的提示构建器在门口阻止它。*

---

## 条件指导块

指导仅在相关工具活动时注入。这防止系统提示在最小工具集配置下无限增长：

| 指导块 | 在以下情况时注入 |
|---|---|
| 内存指导 | `memory_read` 或 `memory_write` 在活动工具集中 |
| 会话搜索指导 | `session_search` 在活动工具集中 |
| 技能指导 | `skills_list` 或 `skill_manage` 在活动工具集中 |
| cron 指导 | `manage_cron_jobs` 在活动工具集中 |
| 消息传递指导 | `send_message` 在活动工具集中 |
| 图像分析指导 | `vision_analyze` 在活动工具集中 |

---

## 关键公共函数

```rust
// 为会话构建完整系统提示
pub async fn build(
    config: &AgentConfig,
    tool_registry: Option<&ToolRegistry>,
    cwd: &Path,
    platform: Platform,
) -> String

// 从技能文件的 YAML frontmatter 提取 `name:` 字段
pub fn extract_frontmatter_name(content: &str) -> Option<String>

// 从技能 frontmatter 提取 `description:` 字段
pub fn extract_skill_description(content: &str) -> Option<String>

// 加载 ~/.edgecrab/memories/ 的所有内存部分（或 profile 目录）
pub fn load_memory_sections(config: &AgentConfig) -> Vec<(String, String)>

// 返回带简要描述的预加载技能名称（缓存）
pub fn load_preloaded_skills(config: &AgentConfig) -> String

// 为提示注入汇总技能目录
pub fn load_skill_summary(config: &AgentConfig) -> String
```

---

## 缓存规则

```
  除非有意为之，否则不要在对话中途重建提示。

  调用 Agent::invalidate_system_prompt() 是触发重建的正确方式。它将 cached_system_prompt = None；下一次 execute_loop() 调用将重建。

  不必要的重建：
    - 驱逐 Claude 的系统提示缓存前缀（消耗 cache_write tokens）
    - 重新加载所有内存和技能文件（磁盘 I/O）
    - 如果文件更改，可能在会话中途改变指导
```

---

## 示例：最小和最大提示

**最小** (`edgecrab --toolset safe "what is 2+2"`):
```
  SOUL.md 内容
  Platform: cli
  Timestamp: ...
  (未找到上下文文件)
  (内存工具缺失 → 无内存块)
  (技能工具缺失 → 无技能块)
```

**最大** (完整网关会话，所有工具)：
```
  SOUL.md 内容
  Platform: telegram
  Timestamp: ...
  .edgecrab.md 项目指令
  AGENTS.md 额外上下文
  内存指导
  [内存文件 1]: 过去会话的关键事实
  [内存文件 2]: 代码模式
  会话搜索指导
  技能指导
  技能摘要: 12 个已安装技能
  Cron 调度指导
  消息传递指导
  图像分析指导
```

---

## 提示

> **Tip: 在仓库根目录的 `.edgecrab.md` 中放置项目特定指令。**
> 此文件保证在 `AGENTS.md` 或 `CLAUDE.md` 之前被拾取。它非常适合代码风格规则、测试命令和架构约束。

> **Tip: `~/.edgecrab/memories/` 中的内存文件跨所有会话存活。**
> 工具 `memory_write` 和 `memory_read` 操作这些文件。写入其中的任何内容都将出现在此机器上后续会话的系统提示中。

> **Tip: 用 `edgecrab doctor` 测试你的上下文文件。**
> Doctor 扫描上下文文件，检查注入模式，并报告哪些文件将被包含在当前目录的系统提示中。

---

## 常见问题

**Q: 我可以添加自己的指导块吗？**
可以。在 `PromptBuilder::build()` 中添加一个条件块，键为特定工具名称是否在 `tool_registry.tool_names()` 中。该块仅在工具活动时注入。

**Q: 更改 `SOUL.md` 需要重启 EdgeCrab 吗？**
不需要。调用 `agent.invalidate_system_prompt()`（或 TUI 中的 `/refresh`）。下一轮将从更新的文件重建。

**Q: 如果 `SOUL.md` 不存在会怎样？**
构建器静默跳过身份槽位。LLM 使用其默认 persona。这对于快速一次性调用是正常的。

**Q: 内存文件有大小限制吗？**
代码中没有强制执行，但大型内存文件会增加系统提示的令牌计数。如果上下文压缩开始频繁触发，考虑修剪旧内存条目。

---

## 交叉引用

- 内存文件位置和格式 → [记忆和技能](../007_memory_skills/001_memory_skills.md)
- 检查的注入模式 → [安全](../011_security/001_security.md)
- 何时重建提示 → [对话循环](./002_conversation_loop.md)
- 技能文件格式 → [创建技能](../007_memory_skills/002_creating_skills.md)
