# 🦀 Data Models

> **WHY**: Ten crates need to exchange messages, tool calls, usage metrics, and platform context without circular dependencies. `edgecrab-types` is the single-crate contract that makes this possible — import it anywhere without pulling in the runtime.

**Source**: `crates/edgecrab-types/src/message.rs`, `tool.rs`, `usage.rs`, `config.rs`

---

## Crate Role

```
┌───────────────────────────┐
│      edgecrab-types        │  ← no runtime deps; pure data + serde
│                           │
│  Message    ToolCall       │
│  Content    ToolSchema     │
│  Usage      Cost           │
│  ApiMode    Platform       │
└─────────────┬─────────────┘
              │  imported by
    ┌─────────┼─────────────────┐
    ▼         ▼                 ▼
edgecrab- edgecrab-   edgecrab-  edgecrab-
  core      tools      state      gateway
```

**Rule**: if a type is needed in more than one crate, it belongs in `edgecrab-types`, not in a higher-level crate.

---

## Core Message Model

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

### Role Enum

```rust
pub enum Role {
    System,
    User,
    Assistant,
    Tool,   // carries the result of a ToolCall back to the model
}
```

---

## Content: Text and Multimodal

```
Content
  ├── Text(String)              ← plain string, most turns
  └── Parts(Vec<ContentPart>)  ← multimodal: text + images mixed

ContentPart
  ├── text   { text: String }
  └── image_url { url: String, detail: Option<String> }
```

The same `Message` type handles both a simple `"What is 2+2?"` and a vision turn with annotated screenshots. Serialisation maps to the OpenAI content format, which `edgequake-llm` then translates for other provider APIs.

---

## Tool-Calling Types

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

`ToolSchema` is what the registry sends to the provider in the request. The provider returns a `ToolCall`; the registry executes it; the result comes back as a `Message { role: Role::Tool, … }`.

---

## Usage and Cost

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

`Usage` normalises across three provider API shapes:

| API | Provider | Notes |
|---|---|---|
| `ChatCompletions` | OpenAI, Mistral, many others | `usage.prompt_tokens` + `usage.completion_tokens` |
| `AnthropicMessages` | Anthropic | adds cache read/write breakdown |
| `CodexResponses` | OpenAI Responses API | includes reasoning token field |

---

## Runtime Enums

### `ApiMode`

```rust
pub enum ApiMode {
    ChatCompletions,    // POST /v1/chat/completions
    AnthropicMessages,  // POST /v1/messages
    CodexResponses,     // POST /v1/responses
}
```

The provider layer selects the API mode per model; the rest of the runtime doesn't need to know.

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

`Platform` is stored with every session row (`source` column in `sessions`) and in `ToolContext` so tools can adapt their behaviour to the delivery channel.

---

## Conversation Invariant

The message history that is sent to the provider in each request must satisfy:

```
[System] [User] [Assistant?] ([User] [Assistant])* [User]
                                                      ^
                                                      current turn
```

Tool result messages (`Role::Tool`) are injected between the assistant tool-call turn and the next user turn. The agent loop in `conversation.rs` maintains this invariant; `edgecrab-types` provides the types but does not enforce the ordering.

---

## Serialisation Contract

All types derive `serde::Serialize` and `serde::Deserialize`. Field names follow `snake_case` in Rust and serialise to `snake_case` JSON — matching the OpenAI API wire format directly. The Anthropic adapter in `edgequake-llm` translates field names as needed.

---

## Tips

- **`arguments` is a JSON string, not an object** — `FunctionCall::arguments` is `String`, not `serde_json::Value`. Parse it with `serde_json::from_str` inside the tool handler; don't assume it's already structured.
- **`reasoning` is model-facing only** — the `reasoning` field carries chain-of-thought tokens from extended-thinking models. It is stored in `state.db` but never displayed to end users by default.
- **`thought_signature` is Gemini-specific** — don't populate it when talking to Anthropic or OpenAI models; it will be ignored.

---

## FAQ

**Q: Why is `arguments` a `String` rather than a `Value`?**
A: The provider sends it as a JSON string. Parsing it twice (provider → types, types → tool) adds overhead with no benefit. Tools own the parse step.

**Q: Where is the `ImageUrl` base64 variant?**
A: `ContentPart::image_url.url` can be a `data:` URI containing base64, or an HTTPS URL. The `detail` hint (`"low"` / `"high"` / `"auto"`) controls vision API quality.

**Q: Can I add a new field to `Message` without breaking the database?**
A: The messages table stores the full JSON blob, so a new optional field deserialises fine for old rows (missing → `None`). Migrations are only needed for new top-level session columns.

---

## Cross-References

- Session storage schema → [`009_config_state/002_session_storage.md`](../009_config_state/002_session_storage.md)
- Tool dispatch (how `ToolCall` → execution) → [`004_tools_system/001_tool_registry.md`](../004_tools_system/001_tool_registry.md)
- Context compression (acts on `Vec<Message>`) → [`003_agent_core/004_context_compression.md`](../003_agent_core/004_context_compression.md)
- Gateway `IncomingMessage` → `Message` conversion → [`006_gateway/001_gateway_architecture.md`](../006_gateway/001_gateway_architecture.md)
