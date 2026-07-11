# 网关架构 🦀

> **已验证来源：** `crates/edgecrab-gateway/src/lib.rs` ·
> `crates/edgecrab-gateway/src/platform.rs` ·
> `crates/edgecrab-gateway/src/run.rs` ·
> `crates/edgecrab-gateway/src/session.rs` ·
> `crates/edgecrab-gateway/src/hooks.rs`

---

## 为什么需要网关

大多数 AI agent 每个消息平台需要一个集成。添加 Telegram 意味着在核心 agent 中编写 Telegram 特定代码；添加 Discord 意味着更多核心更改。表面面积随着每个通道增长。

EdgeCrab 的网关分离了问题：一个共享的 `Agent` 运行时，N 个平台适配器。每个适配器将其平台的事件规范化为 `IncomingMessage` 并将 `String` 响应翻译回平台原生格式。agent 只看到标准的 `IncomingMessage`，无论来源如何。

🦀 *`hermes-agent`（EdgeCrab 的 Python 前身）支持多个网关平台。OpenClaw 专注于单用户桌面使用。EdgeCrab 目前提供 15 个网关适配器 — 螃蟹同时在各处战斗。*

---

## 支持的平台

```
  ┌─────────────────────────────────────────────────────────────────┐
  │  edgecrab-gateway 中的平台适配器                                │
  │                                                                  │
  │  消息传递          Social/Dev        IoT/内部                    │
  │  ─────────────────  ───────────────── ──────────────────────    │
  │  telegram           discord           homeassistant             │
  │  whatsapp           slack             webhook                   │
  │  signal             matrix            api_server (REST)         │
  │  email              mattermost                                  │
  │  sms (Twilio)       dingtalk                                    │
  │                     feishu                                      │
  │                     wecom                                       │
  └─────────────────────────────────────────────────────────────────┘
```

---

## 主要请求流程

```
  平台事件 (Telegram 消息、Discord 提及、Webhook POST)
        │
        ▼
  ┌─────────────────────────────────────────┐
  │  PlatformAdapter::start(tx)             │
  │  → 将事件规范化为 IncomingMessage      │
  │  → 发送到 mpsc::Sender<IncomingMessage>│
  └─────────────────┬───────────────────────┘
                    │
                    ▼
  ┌─────────────────────────────────────────┐
  │  GatewayEventProcessor                  │
  │  → 解析 SessionKey                       │
  │    (platform, user_id, channel_id)      │
  │  → SessionManager::resolve()            │
  │    获取或创建 GatewaySession            │
  └─────────────────┬───────────────────────┘
                    │
                    ▼
  ┌─────────────────────────────────────────┐
  │  Hook: gateway:agent:start              │
  │  → HookRegistry::emit()                 │
  └─────────────────┬───────────────────────┘
                    │
                    ▼
  ┌─────────────────────────────────────────┐
  │  Agent::chat_streaming(message, tx)     │
  │  → 完整对话循环                           │
  │  → StreamEvent::Token 事件               │
  └─────────────────┬───────────────────────┘
                    │
                    ▼
  ┌─────────────────────────────────────────┐
  │  DeliveryRouter                         │
  │  → 重新组装令牌流                         │
  │  → 提取 [MEDIA:/path] 标签               │
  │  → 通过 adapter.send() 发送文本           │
  │  → 通过 adapter.send_photo() 上传媒体     │
  └─────────────────┬───────────────────────┘
                    │
                    ▼
  平台收到回复
```

---

## `PlatformAdapter` trait

所有 15 个网关适配器实现此 trait：

```rust
#[async_trait]
pub trait PlatformAdapter: Send + Sync + 'static {
    fn platform(&self) -> Platform;

    // 开始监听并将事件推入 tx
    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()>;

    // 发送文本消息
    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()>;

    // 为此平台格式化响应（markdown → 纯文本用于 SMS 等）
    fn format_response(&self, text: &str, metadata: &MessageMetadata) -> String;

    // 平台能力标志
    fn max_message_length(&self)  -> usize;
    fn supports_markdown(&self)   -> bool;
    fn supports_images(&self)     -> bool;
    fn supports_files(&self)      -> bool;
    fn supports_editing(&self)    -> bool { false }  // 实时消息编辑

    // 可选 — 提供默认实现
    async fn edit_message(&self, id, metadata, text) -> anyhow::Result<String>;
    async fn send_status(&self, text, metadata) -> anyhow::Result<()>;
    async fn send_typing(&self, metadata) -> anyhow::Result<()>;
    async fn send_and_get_id(&self, msg) -> anyhow::Result<Option<String>>;
    async fn send_photo(&self, path, caption, metadata) -> anyhow::Result<()>;
    async fn send_document(&self, path, caption, metadata) -> anyhow::Result<()>;
}
```

---

## 消息模型

### Inbound

```rust
pub struct IncomingMessage {
    pub platform: Platform,
    pub user_id:  String,
    pub channel_id: Option<String>,
    pub text:     String,
    pub thread_id: Option<String>,
    pub metadata: MessageMetadata,
}
impl IncomingMessage {
    pub fn is_command(&self) -> bool  // 以 / 开头
    pub fn get_command(&self) -> Option<&str>  // "/help" → "help"
    pub fn get_command_args(&self) -> &str
}
```

### Outbound

```rust
pub struct OutgoingMessage {
    pub text:     String,
    pub metadata: MessageMetadata,
}

pub struct MessageMetadata {
    pub message_id:        Option<String>,
    pub channel_id:        Option<String>,
    pub thread_id:         Option<String>,
    pub user_display_name: Option<String>,
    pub attachments:       Vec<MessageAttachment>,
}
```

---

## Media tag protocol

智能体可以通过在其响应中包含特殊标签来生成媒体文件。`DeliveryRouter` 在发送之前拦截这些：

```
  Agent produces:  "Here is the generated chart: [IMAGE:/tmp/chart.png]"
        │
        ▼
  extract_media_from_response(text)
        │
  ┌─────┴──────────────────────────────────────────────┐
  │  text:   "Here is the generated chart: "          │
  │  media: [MediaRef { path: "/tmp/chart.png",        │
  │                      is_image: true }]             │
  └─────┬──────────────────────────────────────────────┘
        │
        ▼
  adapter.send(OutgoingMessage { text, .. })
  adapter.send_photo("/tmp/chart.png", caption, metadata)
```

Tags: `[IMAGE:/path]`, `[MEDIA:/path]`, `[FILE:/path]`

图像检测启发式方法：扩展名在 `[png, jpg, jpeg, gif, webp, svg, bmp]` 中

---

## 会话管理

```rust
pub struct SessionKey {
    pub platform: Platform,
    pub user_id:  String,
    pub channel_id: Option<String>,
}

pub struct GatewaySession {
    pub session_id:      String,
    pub history:         Vec<Message>,
    pub last_activity:   Instant,
    pub model_override:  Option<String>,
}

pub struct SessionManager {
    sessions:      DashMap<SessionKey, Arc<RwLock<GatewaySession>>>,
    idle_timeout:  Duration,
}
```

会话在 `idle_timeout` 后清理（在 `GatewayConfig` 中可配置）。清理任务在后台 GC 循环上运行。

---

## 钩子

钩子系统允许在每个重要事件运行自定义逻辑。

### 事件目录

| 事件 | 何时触发 | 可取消？ |
|---|---|---|
| `gateway:startup` | 进程启动 | 否 |
| `session:start` | 新用户会话 | 否 |
| `session:end` | 会话结束/超时 | 否 |
| `session:reset` | 用户输入 `/new` | 否 |
| `agent:start` | Agent 开始处理 | 否 |
| `agent:step` | 每次工具调用迭代 | 否 |
| `agent:end` | Agent 返回响应 | 否 |
| `command:*` | 任何斜杠命令 | 是 |
| `tool:pre` | 工具执行前 | 是 |
| `tool:post` | 工具返回后 | 否 |
| `llm:pre` | API 调用前 | 是 |
| `llm:post` | API 响应后 | 否 |
| `cli:start` / `cli:end` | CLI 会话生命周期 | 否 |

### 原生 Rust 钩子

```rust
// 在网关钩子模块中实现
pub struct MyHook;

#[async_trait]
impl GatewayHook for MyHook {
    fn name(&self)   -> &'static str { "my_hook" }
    fn events(&self) -> &'static [&'static str] { &["agent:end"] }

    async fn handle(&self, ctx: &HookContext) -> HookResult {
        println!("Agent responded: {:?}", ctx.extra.get("response"));
        HookResult::Continue
    }
}
```

### 基于文件的脚本钩子

在 `~/.edgecrab/hooks/<hook-name>/` 放置钩子：
- `HOOK.yaml` — 元数据（名称、事件）
- `handler.py` / `handler.js` / `handler.ts` — 脚本

EdgeCrab 通过 stdin 将 `HookContext` 作为 JSON 传递。对于可取消事件，脚本可以通过向 stdout 写入 `{"cancel": true}` 来取消。

```yaml
# ~/.edgecrab/hooks/log-responses/HOOK.yaml
name: log-responses
events: [agent:end]
```

```python
# ~/.edgecrab/hooks/log-responses/handler.py
import json, sys
ctx = json.load(sys.stdin)
with open("/tmp/responses.log", "a") as f:
    f.write(ctx.get("response", "") + "\n")
```

---

## 流式传输交付

对于支持消息编辑的平台（例如 Telegram），网关可以在令牌到达时就地更新消息 — 类似于 Claude.ai 的流式效果：

```
  用户发送消息
  → 显示输入指示器
  → 前 N 个令牌后：创建初始消息
  → 后续令牌：就地编辑消息（受速率限制）
  → 最终令牌：消息完成
```

由 `gateway.config.streaming_edits`（每平台标志）控制。

---

## 配对流程

新的 Telegram/WhatsApp/Signal 用户需要在 agent 响应之前进行配对：

```
  未知用户发送消息
        │
        ▼
  pairing.rs: 生成 6 位数字验证码
        │
        ▼
  "要使用 EdgeCrab，请访问 https://... 并输入验证码：123456"
        │
        ▼
  管理员在 CLI 中批准：edgecrab gateway configure
        │
        ▼
  用户现在已授权；会话正常恢复
```

---

## 授权（auth.rs）

每个入站消息在到达 agent 之前都经过 `check_authorization()`。授权链按严格优先级顺序评估规则 — 第一个匹配项获胜：

| 步骤 | 规则 | 结果 |
|---|---|---|
| 1a | 系统平台绕过（Webhook、HomeAssistant、Cron、Api） | `Allowed(PlatformBypass)` |
| 1b | WhatsApp 自聊：`WHATSAPP_MODE=self-chat` | `Allowed(PlatformBypass)` |
| 1c | 通用自聊：`{PREFIX}_SELF_CHAT=true`（任何平台） | `Allowed(PlatformBypass)` |
| 2 | 群组策略：群组/频道消息的 `GroupPolicy::Disabled` | `Denied(GroupPolicyDeny)` |
| 3 | 全局允许全部：`GATEWAY_ALLOW_ALL_USERS=true` | `Allowed(GlobalAllowAll)` |
| 4 | 每平台允许全部：`{PREFIX}_ALLOW_ALL_USERS=true` | `Allowed(PlatformAllowAll)` |
| 5 | 配对存储匹配 | `Allowed(PairingApproved)` |
| 6 | 白名单匹配：`GATEWAY_ALLOWED_USERS` 或 `{PREFIX}_ALLOWED_USERS` | `Allowed(Allowlist)` |
| 7 | 无匹配 — 默认安全 | `Denied(NoAllowlistDeny)` |

---

## WhatsApp 自聊模式

自聊模式让用户通过自己的 WhatsApp 号码与 EdgeCrab agent 对话 — 给自己发消息。agent 只接收用户自己的消息，从不回复群组或其他联系人。

### 三层防御

```
  ┌─────────────────────────────────────────────────────┐
  │  第 1 层：bridge.js (JavaScript)                    │
  │  ─────────────────────────────────────              │
  │  • fromMe=true  → 跳过群组，跳过机器人回声，       │
  │                    只允许 isSelfChat 消息           │
  │  • fromMe=false → 在自聊模式下丢弃                │
  │  • 回声防护：recentlySentIds + REPLY_PREFIX       │
  └──────────────────────┬──────────────────────────────┘
                         │ HTTP POST /events
                         ▼
  ┌─────────────────────────────────────────────────────┐
  │  第 2 层：WhatsApp Rust 适配器 (whatsapp.rs)        │
  │  ─────────────────────────────────────              │
  │  • mode != "bot" && !event.from_me → 丢弃          │
  │  • 深度防御：捕获过期/旧桥接                        │
  └──────────────────────┬──────────────────────────────┘
                         │ mpsc::send(IncomingMessage)
                         ▼
  ┌─────────────────────────────────────────────────────┐
  │  第 3 层：网关授权 (auth.rs)                        │
  │  ─────────────────────────────────────              │
  │  • WHATSAPP_MODE=self-chat → PlatformBypass        │
  │  • 自聊模式下不需要白名单                           │
  └─────────────────────────────────────────────────────┘
```

### 配置

在 `~/.edgecrab/config.yaml` 中：

```yaml
gateway:
  enabled_platforms:
    - whatsapp
  whatsapp:
    enabled: true
    mode: self-chat       # "self-chat" 或 "bot"
    bridge_port: 3000
    allowed_users: []     # 自聊模式下不需要
    reply_prefix: "⚕ *EdgeCrab Agent*"
```

### 回声预防

当 EdgeCrab 在自聊模式下发送回复时，回复显示为来自同一 WhatsApp 帐户的消息。没有回声预防，这会创建一个无限循环。桥接通过两种机制防止这种情况：

1. **`recentlySentIds`** — 最近由 agent 发送的消息 ID 集合。当新的 `fromMe` 消息匹配时，它被静默丢弃。
2. **`REPLY_PREFIX`** — 以配置的前缀（例如 `⚕ *EdgeCrab Agent*`）开头的消息被识别为 agent 回声并丢弃。

### `from_me` 字段

`WhatsAppInboundEvent` 结构体包含一个 `from_me: bool` 字段（`#[serde(rename = "fromMe", default)]`）。它默认为 `false`（保守），这样未知来源的消息在自聊模式下被视为联系人消息并被丢弃。

---

## 提示

> **提示：检查 `ADAPTER_RETRY_DELAY = 5s` 和 `ADAPTER_MAX_RETRY_DELAY = 60s`。**
> 无法连接的适配器（网络问题、错误令牌）以指数退避重试，上限为 60 秒。观察日志中的重复重试消息以诊断配置错误的平台凭据。

> **提示：镜像模式跨平台复制会话。**
> `mirror.rs` 实现跨平台会话镜像 — Telegram 会话可以镜像到 Slack，这样相同的对话出现在两者中。通过配置中的 `gateway.mirrors` 配置。

> **提示：REST API 适配器（`api_server`）是最快的集成路径。**
> 如果你正在构建自定义前端，向网关的 HTTP API 发送 POST 而不是实现完整的适配器。

---

## 常见问题

**问：网关可以处理多少并发用户？**
每个 `SessionKey` 一个 `Agent`。Agent 作为 Tokio 任务运行 — 并发性受内存限制（每个 agent 保留其对话历史）和提供商速率限制。`SessionManager` 中的 `DashMap` 确保会话查找不会在全局锁上序列化。

**问：单个用户可以在多个平台上拥有会话吗？**
可以，但默认情况下每个 `(platform, user_id)` 是一个单独的会话。会话镜像（`mirror.rs`）可以将它们链接起来 — 相同的对话出现在两者中。

**问：网关使用与 CLI 相同的 SQLite 数据库吗？**
是的，默认情况下。两者都使用 WAL 模式下的 `~/.edgecrab/state.db`。`SessionDb` 中的抖动重试策略处理两个进程的并发写入而不损坏。

---

## 交叉引用

- `GatewaySender` trait（工具层）→ [工具注册表](../004_tools_system/001_tool_registry.md)
- 与 CLI 共享的 Session DB → [会话存储](../009_config_state/002_session_storage.md)
- Hook 配置 → [钩子](../hooks.md)
- 会话扇出的并发模型 → [并发模型](../002_architecture/003_concurrency_model.md)
- Path security 和 `jail_read_path` → `edgecrab-security/path_policy.rs`
- WhatsApp bridge source → `scripts/whatsapp-bridge/bridge.js`
