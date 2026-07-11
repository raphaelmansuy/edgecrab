# EdgeCrab — 开发指南

针对 AI 编码助手和开发人员的 edgecrab 代码库工作指南。

## 开发环境

```bash
cargo run               # 从工作区根目录启动默认 CLI 目标
cargo build --workspace             # 构建完整工作区的调试版本
cargo build --workspace --release   # 构建完整工作区的优化版本
cargo test --workspace              # 完整测试套件（约 650+ 测试）
cargo clippy --workspace -- -D warnings   # lint（零警告要求）
```

**二进制文件位置：** `target/release/edgecrab`  
**用户配置：** `~/.edgecrab/config.yaml`（设置）  
**状态目录：** `~/.edgecrab/`（记忆、技能、会话）

---

## 项目结构

```
edgecrab/
├── crates/
│   ├── edgecrab-types/      核心类型：Message、Role、ToolCall、ToolError、错误
│   ├── edgecrab-core/       Agent ReAct 循环、会话、压缩、路由
│   │   ├── agent.rs         AgentBuilder + Agent — 热交换、流式传输、会话
│   │   ├── conversation.rs  execute_loop() — ReAct 工具调用循环
│   │   ├── compression.rs   上下文压缩：结构化 + 基于 LLM
│   │   ├── prompt_builder.rs 系统提示词组装（12 个来源）
│   │   ├── config.rs        AppConfig、ModelConfig、DEFAULT_CONFIG
│   │   ├── model_catalog.rs ModelCatalog — 模型的唯一真实来源
│   │   ├── model_catalog_default.yaml  13 个提供商 × N 个模型（编译内置）
│   │   ├── model_router.rs  提供商工厂、智能路由
│   │   ├── pricing.rs       Token 成本计算
│   │   └── sub_agent_runner.rs  子代理委派运行器
│   ├── edgecrab-tools/      工具注册表 + 30+ 工具实现
│   │   ├── registry.rs      中央 ToolRegistry（模式、处理器、调度）
│   │   ├── toolsets.rs      CORE_TOOLS、ACP_TOOLS、工具集组合
│   │   └── tools/
│   │       ├── file_read.rs     读取文件 — 路径安全
│   │       ├── file_write.rs    写入文件 — 路径安全
│   │       ├── file_patch.rs    补丁文件（搜索替换）
│   │       ├── file_search.rs   Grep/ripgrep 文件搜索
│   │       ├── terminal.rs      Shell 命令 + 后台进程
│   │       ├── process.rs       后台进程管理
│   │       ├── web.rs           网络搜索 + HTML 提取 + 递归爬取（SSRF 保护）
│   │       ├── browser.rs       无头 Chrome 自动化（CDP）
│   │       ├── memory.rs        持久化代理内存（MEMORY.md / USER.md）
│   │       ├── skills.rs        技能库（列表/查看/管理）
│   │       ├── session_search.rs SQLite FTS5 会话搜索
│   │       ├── execute_code.rs  沙箱代码执行
│   │       ├── delegate_task.rs 子代理委派
│   │       ├── mcp_client.rs    MCP 客户端 — stdio + HTTP 带 Bearer token OAuth
│   │       ├── vision.rs        图像分析（多模态 LLM）
│   │       ├── tts.rs           文本转语音
│   │       ├── transcribe.rs    音频转录
│   │       ├── todo.rs          结构化待办事项管理
│   │       ├── cron.rs          Cron 任务管理
│   │       ├── clarify.rs       向用户提出澄清问题
│   │       ├── checkpoint.rs    会话检查点
│   │       ├── advanced.rs      高级工具 + send_message
│   │       ├── honcho.rs       Honcho 配置文件 + 上下文工具
│   │       ├── homeassistant.rs Home Assistant 工具（4 个工具）
│   │       ├── skills_hub.rs   远程技能注册表 + 安装
│   │       ├── skills_guard.rs 外部技能安全扫描器
│   │       └── skills_sync.rs  基于清单的技能同步/播种
│   ├── edgecrab-state/      SQLite WAL + FTS5 会话存储
│   ├── edgecrab-security/   路径安全、SSRF 防护、命令扫描器、代理
│   ├── edgecrab-plugins/    插件系统（WASM + Lua）
│   ├── edgecrab-command-catalog/  斜杠命令定义目录
│   ├── edgecrab-lsp/        语言服务器协议集成
│   ├── edgecrab-cron/       Cron 任务调度引擎
│   ├── edgecrab-cli/        ratatui TUI、子命令、皮肤引擎
│   │   ├── main.rs          入口点 — CLI 子命令调度
│   │   ├── app.rs           TUI App 事件循环（ratatui）
│   │   ├── commands.rs      斜杠命令注册表（42 个命令 + 50 个别名）
│   │   ├── setup.rs         交互式设置向导
│   │   ├── doctor.rs        健康诊断
│   │   ├── skin_engine.rs   YAML 主题引擎（skin.yaml）
│   │   ├── model_discovery.rs 模型目录集成用于 TUI 选择器
│   │   └── plugins.rs       插件/技能管理
│   ├── edgecrab-gateway/    消息平台网关
│   │   ├── run.rs           网关运行器、斜杠命令、消息调度
│   │   ├── session.rs       SessionManager — 会话持久化
│   │   ├── stream_consumer.rs  使用流式 token 进行渐进式消息编辑
│   │   ├── channel_directory.rs 可访问频道/联系人的缓存映射
│   │   ├── pairing.rs       新网关用户的基于代码的 DM 批准
│   │   ├── mirror.rs        跨平台会话镜像
│   │   └── platforms/       适配器：telegram、discord、slack、whatsapp、signal、webhook、
│   │                        sms、matrix、mattermost、dingtalk、homeassistant、api_server、
│   │                        email、feishu、wecom、bluebubbles、weixin
│   ├── edgecrab-acp/        ACP JSON-RPC 2.0 stdio 适配器（VS Code 集成）
│   └── edgecrab-migrate/    导入 hermes-agent 配置/记忆/技能
└── docs/                    架构文档、指南、功能规格
```

**Crate 依赖图：**
```
edgecrab-types   (无依赖 — 被所有模块导入)
     ↑
edgecrab-security  (仅类型)
     ↑
edgecrab-tools   (类型 + 安全)
     ↑
edgecrab-state   (类型)
     ↑
edgecrab-core    (工具 + 状态 + 安全 + 类型)
     ↑
edgecrab-cli, edgecrab-gateway, edgecrab-acp, edgecrab-migrate, edgecrab-plugins
```

---

## Agent 架构（edgecrab-core）

### AgentBuilder + Agent（agent.rs）

```rust
AgentBuilder::new("anthropic/claude-opus-4.6")
    .provider(provider)          // Arc<dyn LLMProvider>
    .tools(registry)             // Arc<ToolRegistry>
    .state_db(db)                // Arc<SessionDb>
    .config(cfg)                 // AgentConfig
    .context_engine(engine)      // Optional Arc<dyn ContextEngine>
    .build()?  →  Agent

// 简单接口
agent.chat("explain this code").await?

// 流式接口
agent.chat_streaming("explain this code", tx).await? // 通过 UnboundedSender<StreamEvent> 传输 token

// 完整接口 — 返回带有使用量/成本的 ConversationResult
agent.run_conversation(msg, system, history).await?

// 取消 — 硬中断（单向闩锁；自动重置以便下次轮次）
agent.interrupt();

// 任务引导 — 在不停机的情况下向运行中的循环注入指导
let steer_tx = agent.steer_sender(); // 克隆并存储在 TUI / 网关中
steer_tx.send(SteeringEvent::new(SteeringKind::Hint, "focus on auth module")).ok();
steer_tx.send(SteeringEvent::new(SteeringKind::Redirect, "use async approach instead")).ok();
steer_tx.send(SteeringEvent::new(SteeringKind::Stop, "stop after this tool")).ok();
// 引导在下一个 LoopAction::Continue 边界（工具结果之后）注入

// TUI: 在代理运行时按 Ctrl+S 打开任务引导覆盖层。
// Tab 循环切换 HINT → REDIRECT → STOP，Enter 发送，Esc 取消。
// 状态栏显示 "⛵ N pending"（琥珀色）/ "⛵ applied"（绿色闪烁）。
//
// 网关 second_message_mode（config.yaml）：
//   gateway.second_message_mode: queue     # 默认 — 排队第二条消息
//   gateway.second_message_mode: steer     # 作为 Redirect 引导注入
//   gateway.second_message_mode: interrupt # 取消并重新启动
```

### 持久化目标（Ralph 循环）

持久化目标使代理在长会话、`/compress` 和重启之间保持任务聚焦。目标存储在 SQLite 中（`session_goals` / `session_subgoals` 表），通过 `session_id` 键索引，**位于**消息历史**之外**。

每个 ReAct 迭代在 `provider.chat(...)` 之前立即追加一个合成的**用户角色**目标块 — 从不修改 `cached_system_prompt`（Anthropic 缓存安全）。

| 命令 | 效果 |
|------|------|
| `/goal <text>` | 设置持续目标并启动 Ralph 循环（默认 20 轮预算） |
| `/goal status` | 显示目标状态、轮次预算、子目标数量 |
| `/goal pause` / `/goal resume` | 暂停或恢复自动继续循环 |
| `/goal clear` | 清除当前会话的目标 + 子目标 |
| `/subgoal` | 列出子目标 |
| `/subgoal <text>` | 添加一个标准 |
| `/subgoal remove <N>` | 删除子目标 N（从 1 开始） |
| `/subgoal clear` | 删除所有子目标，保留顶级目标 |
| `/done` | 将最近推送的未完成子目标标记为已完成 `[x]` |

配置：`goals.max_turns`（默认 20），`auxiliary.goal_judge.model` 用于廉价判断模型。

```rust
// 核心 API（也连接到 CLI + 网关斜杠命令）
agent.goal_set("Refactor payment service to async/await").await?;
agent.subgoal_push("migrate handlers").await?;
agent.subgoal_done().await?; // 将最新未完成的子目标标记为 [x]
let block = agent.goal_show().await?; // 每轮注入相同文本
```

任务引导（HINT/REDIRECT/STOP）用于**单次**飞行中的轻推。持久化目标用于**持续意图**，在压缩和会话恢复后仍然存在。

### ReAct 循环（conversation.rs `execute_loop`）

```text
while api_call_count < max_iterations && budget.try_consume() {
    if needs_compression(messages, params) {
        messages = compress_with_llm(messages, params, provider).await;
    }
    response = provider.chat(model, messages, tools).await?;
    if response.has_tool_calls() {
        for call in response.tool_calls {
            result = registry.dispatch(call.name, call.args).await;
            messages.push(tool_result(call.id, result));
        }
        // ← 引导注入点：在此处排空引导通道 ←
        // drain_pending_steers() 返回 [⛵ STEER] 文本 → 作为用户消息推送
        // STOP 引导也会发出取消令牌用于长运行工具中断
    } else {
        return ConversationResult { final_response, ... };
    }
}
```

消息使用 OpenAI 格式：`role` ∈ {system, user, assistant, tool}。  
推理内容存储在 `Message::reasoning` 中。

### 关键 Agent 配置默认值

| 键 | 默认值 | 说明 |
|----|--------|------|
| `model` | `anthropic/claude-opus-4.6` | 使用 `--model p/m` 覆盖 |
| `max_iterations` | 90 | ReAct 循环轮次硬上限 |
| `streaming` | `true` | TUI 在 token 到达时获取它们 |
| `platform` | `Platform::Cli` | 更改系统提示词中的平台提示 |
| `save_trajectories` | `false` | 将完整轮次转录保存到磁盘 |
| `skip_context_files` | `false` | 跳过 SOUL.md/AGENTS.md 注入 |
| `skip_memory` | `false` | 跳过内存文件注入 |

这些代理标志是顶级 `config.yaml` 键，也支持环境变量覆盖：
- `EDGECRAB_SAVE_TRAJECTORIES`
- `EDGECRAB_SKIP_CONTEXT_FILES`
- `EDGECRAB_SKIP_MEMORY`

---

## 系统提示词组装（prompt_builder.rs）

`PromptBuilder::build()` 按优先级顺序组装约 12 个来源：

1. **身份** — `DEFAULT_IDENTITY`（或来自 SOUL.md/config 的 `override_identity`）
2. **平台提示** — 每个平台的简洁格式/行为指导
3. **日期/时间戳** — 每个会话新鲜注入当前本地时间
4. **上下文文件** — SOUL.md（向上遍历）、AGENTS.md、.cursorrules、CLAUDE.md、.edgecrab.md、.hermes.md、.cursor/rules/*.mdc（全部扫描提示注入）
5. **内存指导** — `MEMORY_GUIDANCE` 常量
6. **内存部分** — 来自 `~/.edgecrab/memories/` 的 MEMORY.md + USER.md
7. **会话搜索指导** — `SESSION_SEARCH_GUIDANCE` 常量
8. **技能指导** — `SKILLS_GUIDANCE` 常量（鼓励保存技能）
9. **技能摘要** — 来自 `~/.edgecrab/skills/` 的紧凑技能索引

**提示词缓存策略：** 系统提示词通过 `PromptBuilder::build_blocks()` 每个会话组装一次，并缓存在 `SessionState.cached_system_prompt` + `cached_stable_prompt` 中。**稳定**区域（身份、工具指导、行为常量）绝不能包含时间戳、会话 ID、上下文文件、内存或技能 — 这些属于**动态**区域，以便 Anthropic 的跨会话前缀缓存可以命中。不要在会话中途重建或修改缓存的系统提示词（会使缓存断点失效）。唯一例外是手动 `/compress` 或自动压缩事件。配置：`cache.prompt_prefix.enabled` 和 `cache.prompt_prefix.ttl`（`"5m"` 或 `"1h"`；默认 `"1h"`）。目标、文件变异页脚和引导注入到 `messages` 中，从不注入到缓存的系统提示词中。

**注入扫描：** 所有上下文文件（AGENTS.md、SOUL.md、.edgecrab.md 等）在注入前都会扫描提示注入模式。被阻止的文件会被替换为 `[BLOCKED: ...]` 占位符。

---

## 工具注册表（edgecrab-tools）

### 文件依赖链

```
edgecrab-tools/src/registry.rs   (无依赖 — ToolHandler trait + ToolRegistry)
       ↑
edgecrab-tools/src/tools/*.rs    (每个实现 ToolHandler + 通过 inventory! 注册)
       ↑
edgecrab-core/src/conversation.rs (导入 ToolRegistry 用于调度)
```

### 添加新工具

**步骤 1：创建 `crates/edgecrab-tools/src/tools/my_tool.rs`：**

```rust
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use edgecrab_types::{ToolError, ToolSchema};
use crate::registry::{ToolContext, ToolHandler};

pub struct MyTool;

#[derive(Deserialize)]
struct MyArgs {
    param: String,
}

#[async_trait]
impl ToolHandler for MyTool {
    fn name(&self) -> &'static str { "my_tool" }
    fn toolset(&self) -> &'static str { "my_toolset" }
    fn emoji(&self) -> &'static str { "🔧" }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "my_tool".into(),
            description: "Does X given Y.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "param": { "type": "string", "description": "The input" }
                },
                "required": ["param"]
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> Result<String, ToolError> {
        let args: MyArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::InvalidArgs { tool: "my_tool".into(), message: e.to_string() })?;
        // ... 实现 ...
        Ok(serde_json::json!({"success": true}).to_string())
    }
}

// ─── 通过 inventory 自动注册 ─────────────────────────────────────
inventory::submit!(crate::registry::RegisteredTool {
    handler: &MyTool,
});
```

**步骤 2：添加到 `crates/edgecrab-tools/src/tools/mod.rs`：**
```rust
pub mod my_tool;
```

**步骤 3：添加到 `toolsets.rs` 中的 `CORE_TOOLS`（如果是核心工具）：**
```rust
pub const CORE_TOOLS: &[&str] = &[
    // ...现有工具...
    "my_tool",
];
```

**步骤 4：在 `crates/edgecrab-tools/src/tools/my_tool.rs` 中编写测试：**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_my_tool_basic() { ... }
}
```

### 工具安全规则

- **文件 I/O**：所有路径必须在访问前通过 `edgecrab_security::path_safety::validate_path()` 验证。
- **网络请求**：所有 URL 必须在获取前通过 `edgecrab_security::ssrf::is_safe_url()`。
- **终端命令**：所有 shell 参数必须通过 `edgecrab_security::command_scan::scan_command()`。
- **内存写入**：内容必须在持久化前扫描提示注入模式。
- **技能安装**：所有外部技能必须在安装前通过 `skills_guard::scan_skill()` — 23 种威胁模式，涵盖数据泄露、注入、破坏性、持久化和混淆。

### 技能中心与防护（edgecrab-tools）

| 模块 | 用途 |
|------|------|
| `skills_hub.rs` | 远程技能注册表、隔离→扫描→安装流程的安装、tap 管理 |
| `skills_guard.rs` | 外部来源技能的安全扫描器 — 23 种威胁模式、严重程度评分 |
| `skills_sync.rs` | 基于清单的捆绑技能播种和更新（NEW/UNCHANGED/MODIFIED/DELETED） |

**GitHub 技能安装**（通过 CLI `/skills install`）：  
`/skills install owner/repo/path/to/skill.md` 从 GitHub raw API 获取。  
目录通过 GitHub Contents API 逐文件下载。  
设置 `GITHUB_TOKEN` 环境变量以获得更高的速率限制。

### MCP 客户端（mcp_client.rs）— stdio + HTTP

| 传输 | 配置 | 说明 |
|------|------|------|
| **stdio**（默认） | `command`、`args`、`env`、`cwd` | 通过 stdin/stdout 的子进程 JSON-RPC 2.0 |
| **HTTP** | `url` 字段 | JSON-RPC 2.0 POST；支持 Bearer token 认证 |

**HTTP MCP 服务器配置示例（`~/.edgecrab/config.yaml`）：**
```yaml
mcp_servers:
  my-http-server:
    url: https://my-mcp-server.example.com/mcp
    bearer_token: "sk-..."   # 静态令牌（或通过 /mcp-token 存储）
    enabled: true
```

**令牌存储**（`~/.edgecrab/mcp-tokens/<server>.json`）：  
令牌以 `chmod 0o600` 存储。使用 `/mcp-token set <server> <token>` 添加，  
`/mcp-token remove <server>` 删除，`/mcp-token list` 查看。  
令牌文件覆盖配置中的 `bearer_token` 字段。

**关键公共函数：**
```rust
pub fn reload_mcp_connections()             // 删除所有连接（由 /reload-mcp 调用）
pub fn read_mcp_token(server_name)          // 读取存储的 Bearer 令牌
pub fn write_mcp_token(server_name, token)  // 持久化 Bearer 令牌
pub fn remove_mcp_token(server_name)        // 删除存储的令牌
```

### Home Assistant 工具（edgecrab-tools）

| 工具 | 描述 |
|------|------|
| `ha_get_states` | 从 Home Assistant 获取实体状态 |
| `ha_call_service` | 调用 Home Assistant 服务（light.turn_on 等） |
| `ha_trigger_automation` | 通过 entity_id 触发自动化 |
| `ha_get_history` | 获取实体在时间范围内的历史 |

### Honcho 工具（edgecrab-tools）

| 工具 | 描述 |
|------|------|
| `honcho_profile` | 通过 Honcho 获取/设置用户配置文件事实 |
| `honcho_context` | 从 Honcho 内存中检索相关上下文 |

### 发送消息工具（edgecrab-tools）

`send_message` 工具（在 `advanced.rs` 中）使用 `GatewaySender` trait 将消息发送到任何连接的平台。在网关模式下运行时，`ToolContext.gateway_sender` 会被填充。

---

## CLI 架构（edgecrab-cli）

- **ratatui** 用于全屏 TUI（支持 60fps，GPU 合成）
- **YAML 皮肤引擎**（`skin_engine.rs`）— 启动时读取 `~/.edgecrab/skin.yaml`；7 个语义颜色 + `prompt_symbol` + `tool_prefix` 均可覆盖
- **斜杠命令** 在 `commands.rs` 中注册为普通类型 — 42 个命令，50+ 个别名
- `CommandResult` 枚举变体从命令处理器调度到 `App::event_loop()`

### 关键斜杠命令

| 类别 | 命令 |
|------|------|
| 导航 | `/help` `/quit` `/clear` `/doctor` `/version` |
| 模型 | `/model [p/m]` `/reasoning [effort]` |
| 会话 | `/new` `/session [list/switch/delete]` `/retry` `/undo` `/stop` `/history` `/save` `/export` `/title` `/resume` |
| 配置 | `/config` `/prompt` `/verbose` `/personality` `/statusbar` |
| 工具 | `/tools` `/toolsets` `/reload-mcp` `/mcp-token` `/plugins` |
| 内存 | `/memory` |
| 分析 | `/cost` `/usage` `/compress` `/insights` |
| 外观 | `/theme` `/paste` `/details` `/tail` |
| 高级 | `/goal` `/subgoal` `/done` `/queue` `/background` `/rollback [checkpoint]` `/debug` `/dump` |
| 网关 | `/platforms` `/approve` `/deny` `/sethome` `/update` |
| 调度 | `/cron` |
| 媒体 | `/voice <on\|off\|status>` |
| 技能 | `/skills [list\|view \<name\>\|install \<path-or-github\>\|remove \<name\>\|hub]` |

### MCP 命令详情

| 命令 | 描述 |
|------|------|
| `/reload-mcp` | 删除所有活动 MCP 连接；下次工具调用时强制重新连接 |
| `/mcp-token set <server> <token>` | 为 HTTP MCP 服务器存储 Bearer 令牌 |
| `/mcp-token remove <server>` | 删除命名服务器的存储令牌 |
| `/mcp-token list` | 列出所有有存储令牌的服务器 |

### 技能命令详情

| 子命令 | 描述 |
|--------|------|
| `/skills` 或 `/skills list` | 列出 `~/.edgecrab/skills/` 中的已安装技能 |
| `/skills view <name>` | 打印技能内容 |
| `/skills install <local-path>` | 将本地 .md 文件或目录复制到技能目录 |
| `/skills install owner/repo/path` | 直接从公共 GitHub 仓库安装技能 |
| `/skills remove <name>` | 删除已安装的技能 |
| `/skills hub` | 显示技能中心使用指南 |

### 活动架与公开

当 `display.activity_shelf: true`（默认）时，实时轮次状态在转录和状态栏之间渲染。

| 命令 | 描述 |
|------|------|
| `/details` | 打开交互式选择器（如 `/reasoning`、`/statusbar`） |
| `/details [mode\|status\|cycle]` | 全局架公开：`hidden`、`collapsed`、`expanded` |
| `/details <section> [mode\|reset]` | 每节覆盖：`thinking`、`tools`、`subagents`、`activity` |
| `/tail <process_id>` | 后台进程输出的全屏覆盖层（4KB tail） |
| `/agents` | 全屏代理监控器（排序，通过 `i` 发送 STOP 引导） |

设置持久化到 `config.yaml` 中的 `display.shelf_details`（与 Hermes `details_mode` 一致）。

### 语音模式

`/voice on`  — 启用 TTS 回读；每个代理响应完成后通过 `text_to_speech` 工具朗读  
`/voice off` — 禁用 TTS 回读  
`/voice status` — 显示当前状态  

需要 `TTS_PROVIDER` 或 `OPENAI_API_KEY`（由 `text_to_speech` 工具使用）。如果 TTS 不可用则无操作。

### 回滚（检查点恢复）

`/rollback` — 提示代理列出可用检查点  
`/rollback <name>` — 提示代理恢复命名检查点  

这会向代理发送自然语言消息，代理会调用 `checkpoint` 工具。检查点在会话期间由 `checkpoint` 工具保存。

### 添加斜杠命令

1. 在 `commands.rs` 中的 `CommandResult` 枚举添加处理程序变体
2. 在 `commands.rs` 调度或 `app.rs` 事件循环中添加匹配
3. 如果网关可见，在 `gateway/run.rs` 中添加调度

---

## 网关架构（edgecrab-gateway）

平台适配器实现 `PlatformAdapter` trait。可用平台：

| 平台 | 适配器 | 所需环境变量 |
|------|--------|-------------|
| Telegram | `telegram.rs` | `TELEGRAM_BOT_TOKEN` |
| Discord | `discord.rs` | `DISCORD_BOT_TOKEN` |
| Slack | `slack.rs` | `SLACK_BOT_TOKEN`、`SLACK_APP_TOKEN` |
| WhatsApp | `whatsapp.rs` | `WHATSAPP_PHONE_NUMBER_ID`、`WHATSAPP_ACCESS_TOKEN` |
| Signal | `signal.rs` | `SIGNAL_CLI_PATH` |
| Webhook | `webhook.rs` | *(任何 HTTP 服务器调用)* |
| SMS | `sms.rs` | `TWILIO_ACCOUNT_SID`、`TWILIO_AUTH_TOKEN`、`TWILIO_PHONE_NUMBER` |
| Matrix | `matrix.rs` | `MATRIX_HOMESERVER`、`MATRIX_ACCESS_TOKEN` |
| Mattermost | `mattermost.rs` | `MATTERMOST_URL`、`MATTERMOST_TOKEN` |
| DingTalk | `dingtalk.rs` | `DINGTALK_APP_KEY`、`DINGTALK_APP_SECRET` |
| Home Assistant | `homeassistant.rs` | `HASS_URL`、`HASS_TOKEN` |
| API Server | `api_server.rs` | `API_SERVER_PORT` *(可选)* |
| Email | `email.rs` | `EMAIL_PROVIDER`、`EMAIL_FROM`、提供商特定的 SMTP/API 凭据 |
| Feishu/Lark | `feishu.rs` | `FEISHU_APP_ID`、`FEISHU_APP_SECRET` |
| WeCom | `wecom.rs` | `WECOM_BOT_ID`、`WECOM_SECRET` |
| BlueBubbles (iMessage) | `bluebubbles.rs` | `BLUEBUBBLES_SERVER_URL`、`BLUEBUBBLES_PASSWORD` |
| WeChat (Weixin) | `weixin.rs` | `WEIXIN_TOKEN`、`WEIXIN_ACCOUNT_ID` |

### 网关功能

| 功能 | 模块 | 描述 |
|------|------|------|
| 流消费者 | `stream_consumer.rs` | 使用流式 LLM token 进行渐进式消息编辑 |
| 频道目录 | `channel_directory.rs` | 每个平台的可访问频道/联系人缓存映射 |
| DM 配对 | `pairing.rs` | 新网关用户的基于代码的 DM 批准流程 |
| 会话镜像 | `mirror.rs` | 跨平台消息传递记录 |

`DeliveryRouter` 将 platform+user_id 映射到发送函数以进行回复路由。
`HookRegistry` 提供生命周期钩子（gateway:startup、message:received 等）。
`SessionManager` 处理每个用户的会话持久化和空闲超时。

### 媒体传递（MEDIA:// 协议）

当代理在响应中包含 `MEDIA:/path/to/file` 时，`DeliveryRouter` 在发送前拦截它，并使用平台的原生媒体上传 API（图片用于图像，语音用于音频，文档用于其他）。

---

## 订阅 OAuth（规范 024）

消费者订阅使用 `edgecrab-core/src/oauth/` 中的 Hermes 兼容 OAuth：

| 目标 | 存储 | 流程 |
|------|------|------|
| `grok` / `nous` | `~/.edgecrab/auth.json` | 代理：PKCE 回环 / 设备代码 |
| `claude-pro` | `~/.edgecrab/.anthropic_oauth.json` | PKCE + 粘贴授权码 |
| `chatgpt-pro` | `auth.json` `providers.openai-codex` | OpenAI 设备代码 |
| `copilot` | `~/.config/edgequake/copilot` | `edgequake_llm` GitHub 设备流程 |

```bash
edgecrab auth add grok          # xAI PKCE 回环（SuperGrok / X Premium+）
edgecrab auth add nous          # Nous 设备代码
edgecrab auth add claude-pro    # Claude Pro / Max OAuth（粘贴代码）
edgecrab auth add chatgpt-pro   # ChatGPT Pro / Codex 设备代码
edgecrab auth login copilot     # GitHub 设备代码 → Copilot 令牌缓存
edgecrab auth status grok
edgecrab auth remove grok
```

TUI：`/login claude-pro`、`/login grok`、`/login chatgpt-pro`（终端切换，如 Copilot）。

共享 PKCE：`edgecrab-core/src/oauth/pkce.rs`。代理回环登录：
`edgecrab-proxy/src/oauth/` + `backend/xai/oauth_login.rs`，
`backend/nous/device_flow.rs`。

当 `ANTHROPIC_API_KEY` 未设置时，`anthropic/…` 模型使用 `.anthropic_oauth.json`。

远程 OAuth：`edgecrab auth add grok --no-browser` 或 `--manual-paste`。

---

## OpenAI 兼容代理（edgecrab-proxy）

本地**提供商桥接**（不是网关代理 API）：将配置的 LLM
提供商暴露给 OpenAI 形状的客户端（Aider、OpenAI SDK、LiteLLM）。

```bash
/proxy                          # TUI 设置向导（EdgeCrab TUI 中的默认值）
/proxy enable grok              # 内联启用，不打开 TUI
edgecrab proxy setup grok       # 引导式：配置 + 令牌 + 客户端代码片段
edgecrab auth add grok          # 一次：SuperGrok OAuth → ~/.edgecrab/auth.json
edgecrab proxy enable grok      # 将 xai_oauth 上游添加到 config.yaml
edgecrab proxy doctor           # 预检（令牌 + OAuth auth.json）
edgecrab proxy client           # OPENAI_API_BASE / Aider 环境代码片段
edgecrab proxy start --provider xai
edgecrab proxy upstreams        # 列出转发上游（别名：providers）
edgecrab proxy status
```

TUI 中心：`edgecrab-cli/src/proxy_hub.rs` + `proxy_setup_tui.rs`（与 CLI `proxy_cmd/` 共享）。

| 模块 | 用途 |
|------|------|
| `edgecrab-proxy/src/server.rs` | axum：`/v1/chat/completions`、`/v1/models`、`/health` |
| `wire/messages.rs` | OpenAI 消息/工具 → `edgequake_llm::ChatMessage` |
| `wire/sse.rs` | `StreamChunk` → OpenAI SSE + `[DONE]` |
| `backend/provider.rs` | 模式 B：`LLMProvider::chat_with_tools`（+ 流回退） |
| `backend/adapter.rs` | `UpstreamAdapter` trait（Hermes `adapters/base.py`） |
| `backend/forwarder.rs` | 模式 A：逐字转发 + 上游 bearer 交换 |
| `backend/nous/` | `NousPortalAdapter` — OAuth 刷新 + 401 重试 |
| `backend/xai/` | `XaiGrokAdapter` — OIDC 刷新 + 429 时池轮换 |
| `backend/auth_store.rs` | `HermesAuthFileAdapter` — 只读 auth.json bearer |
| `auth.rs` | Bearer 检查 vs `proxy.token_path` |

模式 A `proxy.forward_upstreams.<key>.adapter`：`static` | `hermes_auth` | `nous_portal` | `xai_oauth`。
Hermes 内置：`nous`、`xai`（配置为空时使用 `edgecrab proxy upstreams`）。

配置键：`proxy.bind`、`proxy.port`、`proxy.model_aliases`、`proxy.token_path`、
`proxy.max_body_bytes`、`proxy.cors_allow_origins`。与网关不同
`api_server` 运行完整的 ReAct 代理。

---

## ACP 集成（edgecrab-acp）

EdgeCrab 实现了 [Agent Communication Protocol](https://github.com/i-am-bee/acp)（通过 stdio 的 JSON-RPC 2.0）：

```bash
edgecrab acp   # 启动 ACP 服务器 — VS Code Copilot 代理
```

ACP 适配器将 VS Code `agent/run` 请求转换为 `agent.run_conversation()` 调用，并流式传输回 `agent/run/token` 通知。

---

## 上下文压缩（compression.rs）

```text
compress_with_llm(messages, params, provider)
    ├── prune_tool_outputs(old_messages)      ← 步骤 1：免费，无 LLM
    ├── find prior SUMMARY_PREFIX block?       ← 迭代更新
    │     yes → prepend prior summary as context
    ├── llm_summarize(pruned_old) → Ok(text) OR Err
    │     ↓ on Err
    │   build_summary() [结构回退 — 基于统计]
    └── [Message::system_summary(SUMMARY_PREFIX + text), ...recent_messages]
```

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `context_window` | 128,000 | 从模型目录估算 |
| `threshold` | 0.50 | 当使用 50% 的上下文时压缩 |
| `protect_last_n` | 20 | 始终保留最后 20 条消息 |

**缓存注意：** 压缩重建消息列表。系统提示词在压缩时**不会**重新生成 — 只有会话历史被重塑。这保持了 Anthropic 缓存的有效性。

---

## 模型目录（model_catalog.rs）

所有 13 个提供商的唯一真实来源。编译内置的 YAML：

```
~/.edgecrab/models.yaml   ← 用户覆盖（合并在顶部）
model_catalog_default.yaml ← 嵌入的默认值（13 个提供商，200+ 模型）
```

通过 `ModelCatalog::get()` 访问（线程安全的惰性 OnceLock）。

---

## 状态 / 会话（edgecrab-state）

SQLite WAL + FTS5 用于跨会话历史的快速全文搜索。

```rust
let db = SessionDb::open("~/.edgecrab/sessions.db")?;
db.save_session(&session)?;
db.list_sessions(limit)?;
db.search_sessions("query text")?;   // FTS5
db.get_messages(session_id)?;
```

---

## 安全模型

| 层 | 保护 | Crate |
|----|------|-------|
| 文件 I/O | 路径遍历 — 规范化 + 检查允许的根目录 | `edgecrab-security` |
| 网络工具 | SSRF — 阻止私有 IP（10.x、192.168.x、172.16.x、127.x、::1）+ 强化的 HTTP 客户端 | `edgecrab-security` |
| 终端 | 命令注入扫描 — 拒绝参数中的 shell 元字符 | `edgecrab-security` |
| 上下文文件 | 提示注入扫描 — 正则表达式 + 不可见 unicode + 同形字 | `edgecrab-core` |
| 内存写入 | 注入模式在持久化前被阻止 | `edgecrab-tools` |
| LLM 输出 | 编辑管道 — 在显示前剥离机密/令牌 | `edgecrab-core` |
| 状态数据库 | WAL 模式 + 完整性检查 | `edgecrab-state` |
| 网关 webhook | Twilio 签名验证、微信 XML 加密 | `edgecrab-gateway` |
| HTTP 代理 | 出站请求的可选 `HTTPS_PROXY`/`HTTP_PROXY` 支持 | `edgecrab-security` |

---

## 上下文引擎（edgecrab-core）

上下文引擎是一个可插拔系统，用于将外部工具模式注入代理的 ReAct 循环。当 `Arc<dyn ContextEngine>` 提供给 `AgentBuilder::context_engine()` 时，引擎的工具模式在每次 LLM 调用前追加到活动工具定义中。

```rust
AgentBuilder::new("anthropic/claude-opus-4.6")
    .context_engine(my_engine)   // 将引擎工具模式注入循环
    .build()?
```

**配置键：** `config.yaml` 中的 `context.engine`（可选字符串，命名引擎实现）。

---

## Termux / Android 支持

EdgeCrab 可以通过 Termux 为 Android 构建，使用 `termux` 功能标志：

```bash
make build-termux   # 交叉编译：cargo build --release --target aarch64-linux-android --features termux
```

在 Termux 上运行时：
- **TUI 紧凑模式** — `IS_TERMUX` 或终端宽度 < 60 列自动选择 `BasicCompat` UI 配置文件
- **路径监狱** — Termux 数据目录（`$PREFIX` 或 `/data/data/com.termux/files`）作为文件工具的允许根目录添加
- **功能标志** — `#[cfg(feature = "termux")]` 在 `edgecrab-cli` 中控制 Termux 特定行为

---

## 从 hermes-agent 迁移（edgecrab-migrate）

```bash
edgecrab migrate --dry-run    # 预览将导入的内容
edgecrab migrate              # 实时迁移
```

| 资产 | 源 | 目标 |
|------|------|------|
| 配置 | `~/.hermes/config.yaml` | `~/.edgecrab/config.yaml` |
| 记忆 | `~/.hermes/memories/` | `~/.edgecrab/memories/` |
| 技能 | `~/.hermes/skills/` | `~/.edgecrab/skills/` |
| 环境变量 | `~/.hermes/.env` | `~/.edgecrab/.env` |

---

## 已知陷阱 / 请勿

- **请勿在会话中途重建系统提示词。** 缓存破坏迫使 Anthropic 在每轮重新处理提示词。只在显式 `/compress` 或会话开始时重建。
- **请勿在工具处理程序中使用 `unwrap()`。** 改为返回 `ToolError` 变体 — 代理循环优雅处理它们并报告给模型。
- **请勿在未进行 SSRF 检查的情况下发出网络请求。** 在工具代码中的任何 `reqwest` 调用前始终调用 `is_safe_url()`。
- **除非已证明索引是字符边界，否则请勿通过原始字节偏移量切片 Rust 字符串。** 对于前缀扫描和截断，优先使用 `get(..)`、`char_indices()` 或 `safe_char_start()` 等辅助函数。网关/用户文本通常是 Unicode，字节切片会在生产中 panic。
- **请勿在测试中硬编码 `~/.edgecrab/`。** 对任何触及文件系统的测试使用 `TempDir`。在测试夹具中设置 `EDGECRAB_HOME` 环境变量为临时目录路径。
- **请勿将机密（API 密钥、令牌）存储在模型输出或日志中。** 编辑管道捕获大部分，但工具代码不应记录含机密的值。
- **请勿在并发代理实例之间共享 ToolContext 状态。** 每个 `Agent` 有自己的 `ProcessTable` 和 `ToolContext` — 不要共享它们。
- **上下文文件注入扫描已激活：** SOUL.md/AGENTS.md 等中的高严重威胁会导致文件被阻止而不是注入，并使用 `tracing::warn!` 记录。如果您的 AGENTS.md 内容包含对抗性模式，请测试它。

---

## 测试

```bash
cargo test --workspace              # 完整套件（~650+ 测试）
cargo test -p edgecrab-core         # 仅核心 crate
cargo test -p edgecrab-tools        # 仅工具 crate
cargo test -p edgecrab-tools --lib browser   # 特定模块
cargo test --workspace -- --include-ignored     # 包括 E2E 测试（需要 VS Code Copilot）
cargo clippy --workspace -- -D warnings         # lint（必须干净）
cargo doc --no-deps --open          # 浏览生成的文档
```

**测试不得写入 `~/.edgecrab/`。** 对任何触及文件系统的测试使用 `tempfile::TempDir`。在测试夹具中设置 `EDGECRAB_HOME` 环境变量为临时目录路径。

在推送更改前始终运行完整的 `cargo test --workspace` 套件。

---

## 编辑器 / IDE 配置

此项目使用标准 Rust 工具链（`rustup`、`cargo`）。推荐扩展：

- **rust-analyzer** — Rust 的 LSP 服务器
- **CodeLLDB** — 调试适配器
- **Even Better TOML** — Cargo.toml 编辑

除了 `cargo build --workspace` 之外不需要额外设置。