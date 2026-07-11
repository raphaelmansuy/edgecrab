# 工具集组成 🦀

> **已验证来源：** `crates/edgecrab-tools/src/toolsets.rs`

---

## 为什么需要工具集

不是每个会话都需要所有 91 个工具。一个只读文档任务不需要 `terminal`，一个纯 Python 项目不需要 `browser_*`。工具集允许你按工作负载裁剪活动工具集，减少模型看到的工具数量并降低成本。

---

## 默认工具集

| 工具集 | 包含的工具 | 用途 |
|---|---|---|
| `core` | `checkpoint`, `session_search` | 核心功能（始终启用） |
| `file` | `read_file`, `write_file`, `patch`, `search_files` | 文件操作 |
| `web` | `web_search`, `web_extract`, `web_crawl` | 网络搜索和提取 |
| `terminal` | `terminal`, `run_process`, `list_processes`, `kill_process`, `get_process_output`, `wait_for_process`, `write_stdin` | 终端和进程控制 |
| `browser` | `browser_navigate`, `browser_snapshot`, ... (14 个) | 浏览器自动化 |
| `media` | `text_to_speech`, `vision_analyze`, `transcribe_audio` | 媒体处理 |
| `messaging` | `send_message`, `generate_image` | 消息传递 |
| `memory` | `memory_read`, `memory_write` | 内存管理 |
| `skills` | `skills_list`, `skills_categories`, `skill_view`, `skill_manage`, `skills_hub` | 技能管理 |
| `scheduling` | `manage_cron_jobs` | Cron 作业调度 |
| `code_execution` | `execute_code` | 代码执行沙箱 |
| `delegation` | `delegate_task` | 子 agent 委托 |
| `meta` | `manage_todo_list`, `clarify` | 元数据和澄清 |
| `mcp` | `mcp_list_tools`, `mcp_call_tool`, ... (6 个) | MCP 服务器集成 |

---

## 工具集别名

某些工具集有预定义的别名，用于更简洁的配置：

| 别名 | 展开为 |
|---|---|
| `safe` | `core`, `file`, `web`, `memory`, `skills`, `scheduling` |
| `coding` | `safe`, `terminal`, `browser`, `media`, `code_execution`, `delegation`, `meta` |
| `full` | 所有工具集 |
| `minimal` | `core`, `file`, `web`, `memory`, `skills` |

使用别名在配置中更简洁：
```yaml
# ~/.edgecrab/config.yaml
tools:
  enabled_toolsets: [coding]  # 而不是列出 13 个工具集
```

---

## 动态工具集

除了静态工具集，EdgeCrab 还支持通过插件加载的动态工具集。这些在运行时注册并通过与静态工具集相同的机制暴露给模型。

---

## 工具集继承

当启用一个工具集时，它隐式启用其依赖的工具集：

```
启用 code_execution → 自动启用 terminal (因为 execute_code 需要 shell)
启用 browser → 自动启用 web (因为浏览器需要网络)
启用 delegation → 自动启用 core (因为委托需要会话状态)
```

这防止了无效的组合（例如，没有 `terminal` 的 `code_execution`）。

---

## 自定义工具集

你可以创建自己的工具集组合，通过编辑 `~/.edgecrab/config.yaml`：

```yaml
tools:
  enabled_toolsets:
    - core
    - file
    - web
    - memory
    - skills
```

或通过 CLI：
```sh
edgecrab --toolset safe "refactor the auth module"
```

---

## 提示

> **Tip: 从 `safe` 开始，然后根据需要添加工具集。**
> `safe` 工具集提供了大多数日常任务所需的最小安全集合。只有在需要时才添加 `terminal`、`browser` 或 `code_execution`。

> **Tip: 使用别名进行快速原型设计。**
> 在测试新配置时，使用 `coding` 或 `full` 别名而不是手动列出工具集。确认工作后再缩小范围。

> **Tip: 定期审查你的工具集。**
> 随着项目变化，你可能不再需要某些工具。保持精简可以减少模型的认知负荷和 API 成本。

---

## 交叉引用

- 所有工具及其描述 → [工具目录](./002_tool_catalogue.md)
- 工具如何分发 → [工具注册表](./001_tool_registry.md)
- 工具执行后端 → [工具运行时](./004_tools_runtime.md)
