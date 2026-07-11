# Hermes 命令兼容性

此审计的真实来源：`/Users/raphaelmansuy/Github/03-working/hermes-agent/hermes_cli/commands.py`

本文档从第一原理跟踪命令表面的兼容性：

- Hermes slash 名称和别名来自上游注册表
- EdgeCrab TUI 兼容性意味着 slash 命令存在于 `CommandRegistry` 中
- EdgeCrab CLI 兼容性意味着该命令可以通过专用的 clap 子命令或通过 `edgecrab slash <command...>` 访问

## 当前兼容性矩阵

| Hermes 命令 | EdgeCrab TUI slash | EdgeCrab CLI argv | 说明 |
|---|---|---|---|
| `new`, `reset` | yes | `edgecrab slash new` | 相同的实时会话重置路径 |
| `clear` | yes | `edgecrab slash clear` | 匹配 Hermes 新会话行为 |
| `history` | yes | `edgecrab slash history` | 实时会话历史查看 |
| `save` | yes | `edgecrab slash save` | TUI slash 加上已保存会话导出 |
| `retry` | yes | `edgecrab slash retry` | 相同的撤销并重新发送流程 |
| `undo` | yes | `edgecrab slash undo` | 相同的实时会话变异路径 |
| `title` | yes | `edgecrab slash title <name>` | 设置持久化会话标题 |
| `branch`, `fork` | yes | `edgecrab slash branch [name]` | 别名保留 |
| `compress` | yes | `edgecrab slash compress` | 相同的实时压缩流程 |
| `rollback` | yes | `edgecrab slash rollback [name]` | 检查点工具桥接 |
| `stop` | yes | `edgecrab slash stop` | 停止当前回合 |
| `approve` | yes | `edgecrab slash approve [session\|always]` | 网关/运行时审批表面 |
| `deny` | yes | `edgecrab slash deny` | 网关/运行时审批表面 |
| `background`, `bg` | yes | `edgecrab slash background <prompt>` | 隔离的后台会话 |
| `btw` | yes | `edgecrab slash btw <question>` | 短暂的侧问题路径 |
| `queue`, `q` | yes | `edgecrab slash queue <prompt>` | 排队的下一回合提示 |
| `status` | yes | `edgecrab status` 或 `edgecrab slash status` | 专用 CLI 加上 slash |
| `profile` | yes | `edgecrab profile ...` 或 `edgecrab slash profile` | 专用树加上 slash 桥接 |
| `sethome`, `set-home` | yes | `edgecrab slash sethome [channel]` | 网关主频道控制 |
| `resume` | yes | `edgecrab --resume <id>` 或 `edgecrab slash resume [id]` | 两者都是运行时和 slash 入口点 |
| `config` | yes | `edgecrab config ...` 或 `edgecrab slash config` | 专用树加上 TUI 中心 |
| `model` | yes | `edgecrab model` 或 `edgecrab slash model [name]` | 专用 TUI 打开器加上 slash |
| `provider` | yes | `edgecrab slash provider` | slash 驱动的信息表面 |
| `prompt` | yes | `edgecrab slash prompt [text]` | 持久化覆盖行为 |
| `personality` | yes | `edgecrab slash personality [name]` | 会话叠加层 |
| `statusbar`, `sb` | yes | `edgecrab slash statusbar [mode]` | 持久化的可见性切换 |
| `verbose` | yes | `edgecrab slash verbose [mode]` | 相同的工具进度策略 |
| `yolo` | yes | `edgecrab --yolo` 或 `edgecrab slash yolo [mode]` | 启动标志加上运行时切换 |
| `reasoning` | yes | `edgecrab slash reasoning [mode]` | 相同的推理控制表面 |
| `skin` | yes | `edgecrab slash skin [name]` | `/theme` 别名保留 |
| `voice` | yes | `edgecrab slash voice [mode]` | 语音/TTS 控制路径 |
| `tools` | yes | `edgecrab tools ...` 或 `edgecrab slash tools` | 专用树加上叠加层 |
| `toolsets` | yes | `edgecrab tools list` 或 `edgecrab slash toolsets` | 专用和 slash 表面 |
| `skills` | yes | `edgecrab skills ...` 或 `edgecrab slash skills` | 专用树加上叠加层 |
| `cron` | yes | `edgecrab cron ...` 或 `edgecrab slash cron` | 专用树加上 slash |
| `reload-mcp`, `reload_mcp` | yes | `edgecrab slash reload-mcp` | 实时 MCP 重连 |
| `browser` | yes | `edgecrab slash browser [sub]` | CDP 控制路径 |
| `plugins` | yes | `edgecrab plugins ...` 或 `edgecrab slash plugins` | 专用树加上叠加层 |
| `commands` | yes | `edgecrab slash commands [page]` | 网关命令目录 |
| `help` | yes | `edgecrab slash help` | 相同的注册表帮助 |
| `usage` | yes | `edgecrab slash usage` | 实时令牌/成本使用情况 |
| `insights` | yes | `edgecrab insights [--days N]` 或 `edgecrab slash insights [days]` | 现在匹配 Hermes 可选的天数窗口 |
| `platforms`, `gateway` | yes | `edgecrab gateway ...` 或 `edgecrab slash platforms` | 专用网关 CLI 加上 slash 信息/控制 |
| `paste` | yes | `edgecrab slash paste` | 剪贴板辅助输入流程 |
| `update` | yes | `edgecrab update` 或 `edgecrab slash update` | 专用更新器加上 TUI/网关触发 |
| `quit`, `exit`, `q` | yes | `edgecrab slash quit` | 设计为仅交互式 |

## 诚实说明

- 兼容性在 slash 表面本身上最强。Hermes 和 EdgeCrab 现在在 TUI 中暴露相同的操作员词汇表。
- EdgeCrab 有意用 `/worktree` 和 `/w` 扩展 Hermes。这不是 Hermes 命令，因此应视为 EdgeCrab 原生控制表面，而不是计入 Hermes 兼容性。
- CLI argv 兼容性有意用一个通用桥接而不是几十个薄的 clap 包装器实现。这是更好的工程实践，但与 Hermes 确切的顶级命令布局不完全相同。
- 仍然在提供实际价值的专用顶级 CLI 入口点存在：`chat`、`model`、`insights`、`status`、`profile`、`config`、`tools`、`skills`、`cron`、`gateway`、`auth`、`webhook` 及相关系列。
