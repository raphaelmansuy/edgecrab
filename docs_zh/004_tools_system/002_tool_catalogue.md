# 工具目录 🦀

> **已验证来源：** `crates/edgecrab-tools/src/toolsets.rs` ·
> `crates/edgecrab-tools/src/tools/`

91 个核心工具通过 `toolsets.rs` 中的 `CORE_TOOLS` 暴露，处理器通过 `inventory::submit!` 注册。此页面按功能对它们进行分组并记录每个工具属于哪个工具集。

🦀 *`hermes-agent` (EdgeCrab 的 Python 前身) 在相同类别中发送了广泛的工具集。OpenClaw ([TypeScript/Node.js](https://github.com/openclaw)) 专注于浏览器自动化、相机和生产力集成。EdgeCrab 在每个执行领域部署 91 个核心工具 — 并且每个都通过安全门运行。*

---

## Web

| 工具 | 工具集 | 作用 |
|---|---|---|
| `web_search` | `web` | 搜索网络；返回排名片段 |
| `web_extract` | `web` | 获取并从 URL 提取文本 |
| `web_crawl` | `web` | 最多 N 深度跟随链接，聚合内容 |

---

## 终端和进程控制

| 工具 | 工具集 | 作用 |
|---|---|---|
| `terminal` | `terminal` | 运行 shell 命令；完整 stdin/stdout/stderr |
| `run_process` | `terminal` | 启动长时间运行的后台进程 |
| `list_processes` | `terminal` | 列出此会话中的活动后台进程 |
| `kill_process` | `terminal` | 按 ID 终止后台进程 |
| `get_process_output` | `terminal` | 读取后台进程的缓冲 stdout/stderr |
| `wait_for_process` | `terminal` | 阻塞直到后台进程退出（带超时） |
| `write_stdin` | `terminal` | 向后台进程的 stdin 发送数据 |

> **安全 note:** `terminal` 在执行前受 `CommandScanner` 和 `ApprovalPolicy` 门控。见[安全](../011_security/001_security.md)。

---

## 文件

| 工具 | 工具集 | 作用 |
|---|---|---|
| `read_file` | `file` | 读取文件；遵守路径监狱 |
| `write_file` | `file` | 写入或覆盖文件 |
| `patch` | `file` | 对文件应用统一 diff patch |
| `search_files` | `file` | 在工作树中进行 Ripgrep 风格的内容搜索 |

---

## 技能

| 工具 | 工具集 | 作用 |
|---|---|---|
| `skills_list` | `skills` | 列出所有已安装的技能及其描述 |
| `skills_categories` | `skills` | 按类别分组技能 |
| `skill_view` | `skills` | 显示技能的完整内容 |
| `skill_manage` | `skills` | 安装、更新或删除技能 |
| `skills_hub` | `skills` | 浏览远程技能中心 |

---

## 浏览器（完整无头控制）

| 工具 | 工具集 | 作用 |
|---|---|---|
| `browser_navigate` | `browser` | 导航到 URL |
| `browser_snapshot` | `browser` | 捕获可访问性树（DOM 快照） |
| `browser_screenshot` | `browser` | 捕获视觉截图 |
| `browser_click` | `browser` | 通过选择器点击元素 |
| `browser_type` | `browser` | 在聚焦元素中输入文本 |
| `browser_scroll` | `browser` | 按像素偏移或滚动到元素 |
| `browser_console` | `browser` | 读取浏览器控制台输出 |
| `browser_back` | `browser` | 在历史中后退 |
| `browser_press` | `browser` | 按下键盘键（Enter、Tab、F5, …） |
| `browser_close` | `browser` | 关闭浏览器会话 |
| `browser_get_images` | `browser` | 列出当前页面的图片 |
| `browser_vision` | `browser` | 通过视觉模型分析截图 |
| `browser_wait_for` | `browser` | 等待选择器或文本出现 |
| `browser_select` | `browser` | 选择下拉选项 |
| `browser_hover` | `browser` | 悬停在元素上 |

---

## Media

| 工具 | 工具集 | 作用 |
|---|---|---|
| `text_to_speech` | `media` | 通过 TTS 提供者将文本转换为音频 |
| `vision_analyze` | `media` | 使用视觉模型分析图像 |
| `transcribe_audio` | `media` | 使用 STT 提供者转录音频 |
| `generate_image` | `messaging` | 使用图像生成模型生成图像 |

---

## Planning, memory, and session history

| 工具 | 工具集 | 作用 |
|---|---|---|
| `manage_todo_list` | `meta` | 创建、更新和完成结构化待办事项 |
| `memory_read` | `memory` | 从 `~/.edgecrab/memories/` 读取命名内存文件 |
| `memory_write` | `memory` | 追加或覆盖内存文件 |
| `session_search` | `session` | 对所有过去会话进行 FTS5 全文搜索 |
| `checkpoint` | `core` | 保存当前会话状态的命名检查点 |
| `clarify` | `meta` | 向用户询问带有 1–4 个选项的澄清问题 |

---

## Honcho (user memory and profile)

[Honcho](https://honcho.dev) 是用户级内存和个性化层。这些工具仅在配置了 `HONCHO_*` 环境变量时工作。

| 工具 | 工具集 | 作用 |
|---|---|---|
| `honcho_conclude` | `memory` | 从当前会话推导见解 |
| `honcho_search` | `memory` | 在用户内存中进行语义搜索 |
| `honcho_list` | `memory` | 列出内存条目 |
| `honcho_remove` | `memory` | 删除内存条目 |
| `honcho_profile` | `memory` | 查看从内存派生的用户档案 |
| `honcho_context` | `memory` | 从用户内存注入相关上下文 |

---

## 智能家居

当配置了 `HA_URL` 和 `HA_TOKEN` 时，这些工具工作。

| 工具 | 工具集 | 作用 |
|---|---|---|
| `ha_list_entities` | `file` | 列出所有智能家居实体 |
| `ha_get_state` | `file` | 获取实体状态和属性 |
| `ha_list_services` | `file` | 列出可用的智能家居服务 |
| `ha_call_service` | `file` | 调用智能家居服务 |

---

## Execution and delegation

| 工具 | 工具集 | 作用 |
|---|---|---|
| `execute_code` | `code_execution` | 在隔离沙箱中执行代码（Docker 或本地） |
| `delegate_task` | `delegation` | 用特定目标和工具集生成子 agent |
| `moa` | `moa` | 并行运行 N 个参考模型，然后用配置或覆盖的聚合器合成；当 `moa.enabled` 为 `false` 或活动工具集策略排除 `moa` 时隐藏。遗留别名：`mixture_of_agents` |

> `ToolContext` 中的 `delegate_depth` 上限为 2。子 agent 不能生成另一个生成第三个的子 agent — 递归保护。

---

## Scheduling

| 工具 | 工具集 | 作用 |
|---|---|---|
| `manage_cron_jobs` | `scheduling` | 创建、列出、暂停、恢复、运行或删除 cron 作业 |

---

## MCP（模型上下文协议）

这些工具代理到配置的 MCP 服务器的操作。
见 [MCP docs at modelcontextprotocol.io](https://modelcontextprotocol.io)。

| 工具 | 工具集 | 作用 |
|---|---|---|
| `mcp_list_tools` | `mcp` | 列出所有配置的 MCP 服务器暴露的工具 |
| `mcp_call_tool` | `mcp` | 调用 MCP 服务器上的工具 |
| `mcp_list_resources` | `mcp` | 列出 MCP 服务器的资源 |
| `mcp_read_resource` | `mcp` | 读取 MCP 服务器的资源 |
| `mcp_list_prompts` | `mcp` | 列出 MCP 服务器的提示 |
| `mcp_get_prompt` | `mcp` | 从 MCP 服务器获取提示 |

---

## Messaging

| 工具 | 工具集 | 作用 |
|---|---|---|
| `send_message` | `messaging` | 向配置的网关平台目标发送消息 |

---

## Runtime gating tools

一些工具被编译在内，但当所需能力缺失时在运行时对模型不可见：

| 工具 | 可见性条件 |
|---|---|
| `execute_code` | Docker 存在，或 `sandbox_code_execution.enabled=true` |
| `browser_*` | Playwright / Chromium 已安装且可访问 |
| `ha_*` | `HA_URL` 和 `HA_TOKEN` 已配置 |
| `honcho_*` | `HONCHO_APP_ID` 已配置 |
| `text_to_speech` | TTS 提供者已配置 |
| `transcribe_audio` | STT 提供者已配置 |
| `generate_image` | 图像生成提供者已配置 |
| `send_message` | 至少一个网关平台正在运行 |

---

## ACP 工具子集 (54 tools)

ACP 服务器 (`edgecrab acp`) 移除不适合 IDE 集成的交互式和交付特定工具：

**从 ACP 排除：**
`clarify`, `send_message`, `generate_image`, `text_to_speech`,
`transcribe_audio`, `honcho_*`, `ha_*`, `manage_cron_jobs` (部分),
以及少数其他需要用户交互的工具。

---

## 提示

> **Tip: 对于大型代码库，在 `read_file` 之前使用 `search_files`。**
> `search_files` 在底层使用 ripgrep — 它可以在毫秒内找到确切位置而无需读取每个文件。如果先搜索，模型会更快到达正确答案。

> **Tip: 对于可并行化的子问题使用 `delegate_task`。**
> 对范围良好的子任务运行 `delegate_task` 并设置 `max_iterations=30` 比让主 agent 在 90 轮迭代中做所有事情更可靠。

> **Tip: 在危险操作之前使用 `checkpoint`。**
> 在多文件重构之前存储命名检查点。如果出问题，`session_search` + `rollback` 可以恢复重构前的状态。

---

## 交叉引用

- 工具如何分发 → [工具注册表](./001_tool_registry.md)
- 每个工具属于哪个工具集 → [工具集组成](./003_toolset_composition.md)
- `terminal` 和 `execute_code` 的执行后端 → [工具运行时](./004_tools_runtime.md)
- 所有工具的安全门控 → [安全](../011_security/001_security.md)
