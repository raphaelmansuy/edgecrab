# 插件规范完成清单 🦀

仓库可见源：

- `specs/spec_plugins/spec/015_hermes_compatibility.md`
- `specs/spec_plugins/spec/016_implementation_plan.md`

Hermes 兼容性文档现在是精确的契约。较旧的研究文档作为示例仍然有用，但不作为实现基准。

## 当前运行时

- [x] `plugin.toml` 解析和验证，支持 `skill`、`tool-server` 和 `script`
- [x] `config.yaml` 中 `plugins:` 下的插件配置表面
- [x] 在用户、项目和系统插件根目录中发现插件
- [x] 工具服务器和脚本插件工具的运行时注册
- [x] 启用的技能插件的提示注入
- [x] 发现 Hermes 风格的 `plugin.yaml` + `__init__.py` 目录插件
- [x] 传统 Hermes 用户/项目插件根目录发现
- [x] Hermes `requires_env` 就绪门控到 `setup-needed`
- [x] 不可用的插件从运行时工具暴露中排除

## 传输和主机 API

- [x] MCP 风格的 stdio 握手：`initialize`、`notifications/initialized`
- [x] `tools/list` 和 `tools/call` 的工具服务器调度
- [x] 插件调用期间的反向 `host:*` 请求处理
- [x] 主机 API：`platform_info`、`log`、`memory_read`、`memory_write`、`session_search`、`secret_get`、`inject_message` 和委托的 `tool_call`
- [x] 与 Hermes 兼容的 `pre_llm_call` 钩子执行，带用户消息上下文注入
- [x] 与 Hermes 兼容的 `on_session_start` 钩子执行
- [x] 与 Hermes 兼容的 `on_session_end` 钩子执行路径
- [x] 与 Hermes 兼容的 `pre_tool_call` 和 `post_tool_call`
- [x] 与 Hermes 兼容的 `post_llm_call`
- [x] 与 Hermes 兼容的 `pre_api_request` 和 `post_api_request`
- [x] 与 Hermes 兼容的 `on_session_finalize` 和 `on_session_reset`（在 CLI 会话中）

## 安装、Hub 和安全

- [x] 隔离安装流程
- [x] 激活前的静态保护扫描
- [x] 安装/删除审计日志
- [x] 精选和配置的 hub 索引搜索
- [x] `hub:<source>/<plugin>` 源解析
- [x] 直接 `https://...zip` 存档安装
- [x] 使用 `GITHUB_TOKEN`/`GH_TOKEN` 和 `gh auth token` 回退的 GitHub Contents API 下载
- [x] 安装时的清单信任/源/校验和标记
- [x] 当 hub 元数据提供预期摘要时的校验和验证
- [x] 一流的 `plugins search|browse|refresh` UX，带向后兼容的 `hub-*` 别名
- [x] 源感知的插件搜索输出，带安装就绪的 `hub:<source>/<plugin>` 目标
- [x] 上游 `plugins/...` 目录的 Hermes hub 索引
- [x] 仓库根目录 Hermes 插件目录的 Hermes hub 索引（`42-evey/hermes-plugins`）
- [x] 精选 GitHub Hermes 源的确定性共享支持文件解析，带仓库根目录辅助模块

## Hermes 兼容性证明

- [x] 指南风格的 Hermes 插件 E2E 覆盖（上游构建指南契约中的 `calculator`）
- [x] 真实上游 Hermes 插件 E2E 覆盖（`holographic`）
- [x] 真实上游 Hermes 包导入兼容性覆盖（`honcho`）
- [x] 真实上游 Hermes 技能安装覆盖（`1password`）
- [x] 真实 `42-evey/hermes-plugins` E2E 覆盖（`evey-telemetry`、`evey-status`）
- [x] 无需手写 `plugin.toml` 的原始本地 Hermes 包安装
- [x] Hermes 插件根目录内捆绑的 `SKILL.md` 加载
- [x] 在真实上游技能内容上验证的 Hermes 路径转换
- [x] `metadata.hermes.related_skills` 在插件信息渲染中显示
- [x] pip 入口点插件加载对等
- [x] Hermes CLI 子命令注册对等
- [x] Hermes memory-provider `cli.py register_cli(subparser)` 约定
- [x] 网关特定会话边界对等证明

## 旧研究笔记中的残留非目标

- [ ] 旧研究文档中的字面 WASM/TypeScript 插件 SDK 流程

已验证的非差距：

- 上游 `hermes-agent` 和精选社区仓库使用的 Python Hermes 插件系统通过真实安装/运行时测试覆盖。
- `cargo test -- --include-ignored` 仍未检查，因为忽略的套件包括外部网络或凭证场景，不是 Hermes 插件的稳定兼容性证明。

## 文档和验证

- [x] README 更新
- [x] 插件文档更新
- [x] 站点文档更新
- [x] Hermes 风格插件创作教程添加到仓库文档
- [x] Hermes 风格插件创作教程添加到站点文档
- [x] 变更日志更新
- [x] `cargo test -p edgecrab-plugins hermes_plugin_loads_bundled_skill_metadata -- --nocapture`
- [x] `cargo test -p edgecrab-plugins cached_hermes_repo_index_includes_python_plugin_directories -- --nocapture`
- [x] `cargo test -p edgecrab-core api_call_with_retry_invokes_hermes_api_hooks -- --nocapture`
- [x] `cargo test -p edgecrab-core session_boundary_hooks_fire_on_new_and_finalize -- --nocapture`
- [x] `cargo test -p edgecrab-gateway --lib run::tests::gateway_keeps_agent_history_isolated_per_chat_session -- --nocapture`
- [x] `cargo test -p edgecrab-gateway --lib run::tests::gateway_session_hooks_fire_across_chat_reset_and_shutdown -- --nocapture`
- [x] `cargo test -p edgecrab-cli --test plugins_e2e real_hermes_honcho_memory_cli_is_invocable_end_to_end -- --nocapture`
- [x] `cargo test -p edgecrab-cli --test plugins_e2e -- --nocapture`
- [x] `cargo test -p edgecrab-plugins live_official_hermes_search_returns_real_plugins -- --ignored --nocapture`
- [x] `cargo test -p edgecrab-plugins --lib`
- [x] `cargo test -p edgecrab-core --lib`
- [x] `cargo clippy -p edgecrab-plugins -p edgecrab-core -p edgecrab-cli -p edgecrab-gateway --tests -- -D warnings`
- [x] `cargo test`
- [ ] `cargo test -- --include-ignored`
- [x] `pnpm build` in `site/`