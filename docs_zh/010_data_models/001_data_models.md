# 🦀 数据模型

> **为什么：** 十个 crate 需要交换消息、工具调用、使用指标和平台上下文而没有循环依赖。`edgecrab-types` 是使这成为可能的单一 crate 契约 — 可以在任何地方导入它而无需引入运行时。

**来源：** `crates/edgecrab-types/src/message.rs`, `tool.rs`, `usage.rs`, `config.rs`

---

## Crate 角色

```
┌───────────────────────────┐
│      edgecrab-types        │  ← 无运行时依赖；纯数据 + serde
│                           │
│  Message    ToolCall       │
│  Content    ToolSchema     │
│  Usage      Cost           │
│  ApiMode    Platform       │
└─────────────┬─────────────┘
              │ 被以下导入
    ┌─────────┼─────────────────┐
    ▼         ▼                 ▼
edgecrab- edgecrab-   edgecrab-  edgecrab-
  core      tools      state      gateway
```

**规则：** 如果一个类型在多个 crate 中需要，它属于 `edgecrab-types`，而不是更高层次的 crate。

---

## 核心消息模型

```rust
pub struct Message {
    pub role: Role,
    pub content: Content,
    pub tool_calls: Option<Vec<ToolCall>>,   // present on assistant turns
    pub tool_call_id: Option<String>,         // present on tool-result turns
    pub name: Option<String>,                 // tool name for tool-result turns
    pub reasoning: Option<String>,            // chain-of-thought (extended thinking)
    pub finish_reason: Option<String>,        // "stop" | "tool_calls" | "length" …
}
```

### Role 枚举

```rust
pub enum Role {
    System,
    User,
    Assistant,
    Tool,   // carries the result of a ToolCall back to the model
}
```

---

## Content：文本和多模态

```
Content
  ├── Text(String)              ← plain string, most turns
  └── Parts(Vec<ContentPart>)  ← multimodal: text + images mixed

ContentPart
  ├── text   { text: String }
  └── image_url { url: String, detail: Option<String> }
```

相同的 `Message` 类型处理简单的 `"What is 2+2?"` 和带有注释截图的视觉回合。序列化映射到 OpenAI 内容格式，然后 `edgequake-llm` 将其转换为其他提供商 API。

---

## 工具调用类型

```rust
pub struct ToolCall {
    pub id: String,                       // unique per call, echoed in tool result
    pub r#type: String,                   // always "function" today
    pub function: FunctionCall,
    pub thought_signature: Option<String>, // Gemini extended-thinking field
}

pub struct FunctionCall {
    pub name: String,            // matches ToolHandler::name()
    pub arguments: String,       // JSON-encoded arguments string
}

pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,  // JSON Schema object
    pub strict: Option<bool>,           // OpenAI strict mode
}
```

`ToolSchema` 是注册表发送给提供商的内容。提供商返回一个 `ToolCall`；注册表执行它；结果作为 `Message { role: Role::Tool, … }` 返回。

---

## 使用和成本

```rust
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    // Extended fields for provider-specific breakdown
    pub cache_read_tokens: Option<u32>,   // Anthropic prompt cache hits
    pub cache_write_tokens: Option<u32>,  // Anthropic prompt cache misses
    pub reasoning_tokens: Option<u32>,    // o1/extended-thinking tokens
}

pub struct Cost {
    pub input_usd: f64,
    pub output_usd: f64,
    pub total_usd: f64,
}
```

`Usage` 在三种提供商 API 形状之间规范化：

| API | 提供商 | 说明 |
|---|---|---|
| `ChatCompletions` | OpenAI, Mistral, many others | `usage.prompt_tokens` + `usage.completion_tokens` |
| `AnthropicMessages` | Anthropic | adds cache read/write breakdown |
| `CodexResponses` | OpenAI Responses API | includes reasoning token field |

---

## 运行时枚举

### `ApiMode`

```rust
pub enum ApiMode {
    ChatCompletions,    // POST /v1/chat/completions
    AnthropicMessages,  // POST /v1/messages
    CodexResponses,     // POST /v1/responses
}
```

提供商层为每个模型选择 API 模式；运行时的其他部分不需要知道。

### `Platform`

```rust
pub enum Platform {
    Cli,
    Telegram,
    Discord,
    Slack,
    WhatsApp,
    Signal,
    Email,
    Sms,
    Matrix,
    Mattermost,
    DingTalk,
    Feishu,
    Wecom,
    HomeAssistant,
    Webhook,
    ApiServer,
    Acp,
    Cron,
    // … 18 variants total
}
```

`Platform` 与每个会话行一起存储（`sessions` 表中的 `source` 列）并在 `ToolContext` 中，因此工具可以根据交付渠道调整其行为。

---

## 对话不变量

每次请求发送给提供商的消息历史必须满足：

```
[System] [User] [Assistant?] ([User] [Assistant])* [User]
                                                      ^
                                                      current turn
```

工具结果消息（`Role::Tool`）注入在助手工具调用回合和下一个用户回合之间。`conversation.rs` 中的智能体循环维护此不变量；`edgecrab-types` 提供类型但不强制执行顺序。

---

## 序列化契约

所有类型派生 `serde::Serialize` 和 `serde::Deserialize`。字段名称遵循 Rust 中的 `snake_case` 并序列化为 `snake_case` JSON — 直接匹配 OpenAI API 线缆格式。`edgequake-llm` 中的 Anthropic 适配器根据需要转换字段名称。

---

## 提示

- **`arguments` 是 JSON 字符串，不是对象 —** `FunctionCall::arguments` 是 `String`，不是 `serde_json::Value`。在工具处理器内部使用 `serde_json::from_str` 解析它；不要假设它已经是结构化的。
- **`reasoning` 仅面向模型 —** `reasoning` 字段携带扩展思考模型的思维链 token。它存储在 `state.db` 中但默认情况下从不显示给最终用户。
- **`thought_signature` 是 Gemini 特定的 —** 在与 Anthropic 或 OpenAI 模型交谈时不要填充它；它将被忽略。

---

## 常见问题

**问：为什么 `arguments` 是 `String` 而不是 `Value`？**
答：提供商将其作为 JSON 字符串发送。解析两次（提供商 → 类型，类型 → 工具）会增加开销而没有任何好处。工具拥有解析步骤。

**问：`ImageUrl` base64 变体在哪里？**
答：`ContentPart::image_url.url` 可以是包含 base64 的 `data:` URI，或者是 HTTPS URL。`detail` 提示（`"low"` / `"high"` / `"auto"`）控制视觉 API 质量。

**问：我可以在不破坏数据库的情况下向 `Message` 添加新字段吗？**
答：消息表存储完整的 JSON 块，因此新的可选字段对旧行反序列化正常（缺失 → `None`）。迁移只需要新的顶层会话列。

---

## 交叉引用

- 会话存储 schema → [`009_config_state/002_session_storage.md`](../009_config_state/002_session_storage.md)
- 工具分发（如何 `ToolCall` → 执行）→ [`004_tools_system/001_tool_registry.md`](../004_tools_system/001_tool_registry.md)
- 上下文压缩（作用于 `Vec<Message>`）→ [`003_agent_core/004_context_compression.md`](../003_agent_core/004_context_compression.md)
- 网关 `IncomingMessage` → `Message` 转换 → [`006_gateway/001_gateway_architecture.md`](../006_gateway/001_gateway_architecture.md)
