# 插件系统 🦀

EdgeCrab 现在在 `crates/edgecrab-plugins/` 中包含一个共享的插件运行时。

## 插件 vs 技能

第一性原理区分：

- `skill` 是可重用的提示指导。
- `plugin` 是 EdgeCrab 发现和管理的运行时扩展包。

含义：

- 技能是指令、清单、示例和可重复工作流的正确原语。
- 插件是代码、工具、钩子、子进程、就绪检查、信任元数据和安装/更新生命周期的正确原语。
- 插件可以捆绑 `SKILL.md`，但该技能内容仍然是插件管理包的一部分。
- 独立技能仍然可以捆绑辅助文件和脚本。这并不使它们成为插件；这意味着技能可以通过正常的工具表面指向这些文件。
- 捆绑 Python 或 shell 辅助脚本的 Claude Code 风格独立技能仍然是技能，不是插件，除非它们还需要运行时注册、钩子或安装/审计生命周期。

示例：

- `~/.edgecrab/skills/release/SKILL.md` 是一个独立技能。
- `~/.edgecrab/plugins/release-helper/plugin.toml` 是一个插件。
- `~/.edgecrab/plugins/calculator/plugin.yaml` 带有 `__init__.py` 是一个 Hermes 插件。

## 支持的插件类型

- `skill`: 与 Hermes 兼容的 `SKILL.md` 包，注入到系统提示中
- `tool-server`: 通过 stdio JSON-RPC 暴露工具的子进程插件
- `script`: 基于 Rhai 的轻量级工具处理程序本地插件
- `hermes`: 带有 `plugin.yaml` + `__init__.py register(ctx)` 兼容性的 Python 目录插件

## 配置

插件策略位于 `~/.edgecrab/config.yaml`：

```yaml
plugins:
  enabled: true
  auto_enable: true
  disabled: []
  platform_disabled: {}
  install_dir: ~/.edgecrab/plugins
  quarantine_dir: ~/.edgecrab/plugins/.quarantine
```

插件配置控制插件生命周期。它不替代用于 `~/.edgecrab/skills/` 中独立技能的单独 `skills:` 配置块。

## CLI

```bash
edgecrab plugins list
edgecrab plugins info <name>
edgecrab plugins status
edgecrab plugins enable <name>
edgecrab plugins disable <name>
edgecrab plugins toggle [name]
edgecrab plugins install github:owner/repo/path
edgecrab plugins install hub:community/github-tools
edgecrab plugins install https://example.com/plugin.zip
edgecrab plugins install ./local-plugin
edgecrab plugins audit --lines 20
edgecrab plugins search github
edgecrab plugins search --source hermes weather
edgecrab plugins browse
edgecrab plugins refresh
edgecrab plugins remove <name>
```

插件安装现在通过隔离、静态安全扫描、信任分配、`plugin.toml` 中的校验和标记以及 `~/.edgecrab/plugins/.hub/audit.log` 处的审计日志进行。

远程搜索现在位于主要插件命令表面。`edgecrab plugins search` 支持 `--source hermes` 并打印安装就绪的 `hub:<source>/<plugin>` 目标，因此无需记住 hub 内部细节即可发现与 Hermes 兼容的注册表。

精选的面向 Hermes 的源现在包括：

- `edgecrab-official` 用于官方 EdgeCrab 仓库插件示例（`plugins/` 下）
- `hermes-plugins` 用于 `NousResearch/hermes-agent`
- `hermes-evey` 用于 `42-evey/hermes-plugins`

与 Hermes 兼容的插件也从传统根目录发现：

- `~/.hermes/plugins/`
- `HERMES_ENABLE_PROJECT_PLUGINS=true` 时的 `./.hermes/plugins/`

Hermes `requires_env` 声明在发现期间受到尊重。缺失的变量会将插件移至 `setup-needed`，不可用的插件不会作为运行时工具暴露。

工具暴露在控制台中是实时的：

- 已启用的插件工具出现在 `plugins` 工具集下的 `/tools` 中
- 禁用插件会立即从活动注册表中移除这些工具
- 重新启用插件无需重新启动会话即可恢复它们

最小验证流程：

```text
/plugins
/tools
/plugins disable calculator
/tools
/plugins enable calculator
/tools
```

工具服务器插件现在使用与 MCP 兼容的换行符分隔的 JSON-RPC：

- 主机 → 插件: `initialize`, `notifications/initialized`, `tools/list`, `tools/call`
- 插件 → 主机: `host:platform_info`, `host:log`, `host:memory_read`, `host:memory_write`, `host:session_search`, `host:secret_get`, `host:inject_message`, `host:tool_call`

Hermes 兼容钩子对等目前包括：

- `pre_tool_call`
- `post_tool_call`
- `on_session_start`
- `pre_llm_call` 带临时用户消息上下文注入
- `post_llm_call`
- `pre_api_request`
- `post_api_request`
- `on_session_end`
- `on_session_finalize`
- `on_session_reset`

EdgeCrab 的 Hermes Python 桥现在提供真实上游 Hermes 插件期望的最小运行时垫片：

- `agent.memory_provider.MemoryProvider`
- `tools.registry.tool_error`
- `hermes_constants.get_hermes_home()` / `display_hermes_home()`
- 命名空间包布线，用于从 Hermes 仓库树导入 `plugins.*`

Claude 风格的独立技能包也独立于插件运行时支持：

- `skill_view` 和预加载技能中的 `Base directory for this skill: ...` 渲染
- `${CLAUDE_SKILL_DIR}` 和 `${CLAUDE_SESSION_ID}` 替换
- 从 `references/`、`templates/`、`scripts/` 和 `assets/` 发现辅助文件
- 解析 `when_to_use`、`arguments`、`argument-hint`、`allowed-tools`、`user-invocable`、`disable-model-invocation`、`context` 和 `shell` 的元数据

当前非对等边界：

- EdgeCrab 不会自动执行技能文本中的 Claude 提示 shell 块。
- EdgeCrab 不会自动派生 Claude 风格的专用技能子代理。

Hermes 技能兼容性现在在加载期间保留额外的元数据字段：

- 顶级 `compatibility`
- `metadata.hermes.related_skills`
- `metadata.hermes.category`

Hermes 本地安装现在接受原始上游包布局，无需作者添加 `plugin.toml`：

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

示例：

```bash
edgecrab plugins install ./calculator
edgecrab plugins info calculator

edgecrab plugins search --source edgecrab calculator
edgecrab plugins search --source edgecrab json
edgecrab plugins install ./plugins/productivity/calculator
edgecrab plugins install ./plugins/developer/json-toolbox
edgecrab plugins info json-toolbox

edgecrab plugins install ~/src/hermes-agent/plugins/memory/holographic
edgecrab plugins info holographic

EDGECRAB_PLUGIN_PYTHON=~/.venvs/hermes/bin/python \
  edgecrab plugins list
EDGECRAB_PLUGIN_PYTHON=~/.venvs/hermes/bin/python \
  edgecrab entry-demo status

edgecrab plugins search --source hermes-evey telemetry
edgecrab plugins install hub:hermes-evey/evey-telemetry
edgecrab plugins install hub:hermes-evey/evey-status
```

远程插件搜索现在仅限于插件。像 `1password` 这样的 Hermes 独立技能属于远程技能浏览器：

```bash
edgecrab skills search 1password
edgecrab skills install hermes-agent:security/1password
```

Hermes 插件根目录内的捆绑 `SKILL.md` 文件现在作为插件技能加载，因此它们的 `compatibility`、`related_skills` 和就绪状态通过正常的插件发现和 `/plugins info` 显示。

有关分步创作教程，请参阅 `docs/007_memory_skills/005_building_hermes_style_plugins.md`。

禁用插件会将其从提示注入或工具暴露中隐藏，而不删除其文件。

已验证的兼容性覆盖范围现在包括：

- 官方仓库 Hermes 示例 `calculator` 和 `json-toolbox`，包括官方搜索可见性以及本地安装/运行时证明
- 通过 CLI E2E 的指南风格 Hermes 插件安装 + 工具执行 + `post_tool_call` 钩子
- 来自 `NousResearch/hermes-agent` 的真实 Hermes 插件 (`honcho`, `holographic`)
- 来自 `NousResearch/hermes-agent` 的真实 Hermes 可选技能包兼容性 (`github-issues`, `1password`)
- 来自 `42-evey/hermes-plugins` 的真实 Hermes 插件 (`evey-telemetry`, `evey-status`)
- 通过 E2E 的 pip 入口点插件发现 + CLI 命令调度
- Hermes memory-provider `cli.py register_cli(subparser)` 桥接，包括真实的 `honcho` CLI 调用
- 插件浏览器中上游 `plugins/...` 目录和仓库根目录 Hermes 插件目录的 Hermes hub 索引
- 网关每聊天会话隔离加上 `on_session_start`、`on_session_end`、`on_session_finalize` 和 `on_session_reset` 在网关测试中的证明

验证：

```bash
cargo test -p edgecrab-plugins hermes_plugin_loads_bundled_skill_metadata -- --nocapture
cargo test -p edgecrab-plugins cached_hermes_repo_index_includes_python_plugin_directories -- --nocapture
cargo test -p edgecrab-core api_call_with_retry_invokes_hermes_api_hooks -- --nocapture
cargo test -p edgecrab-core session_boundary_hooks_fire_on_new_and_finalize -- --nocapture
cargo test -p edgecrab-cli --test plugins_e2e -- --nocapture
```