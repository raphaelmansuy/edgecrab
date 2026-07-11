# 错误处理 🦀

> **已验证来源：** `crates/edgecrab-types/src/error.rs` ·
> `crates/edgecrab-core/src/conversation.rs` ·
> `crates/edgecrab-tools/src/registry.rs`

---

## 为什么错误模型是这样的

`hermes-agent` — EdgeCrab 的 Python 前身 — 通过 Python 异常和字符串消息暴露失败：易于抛出，不通过 `isinstance` 检查或字符串解析就无法针对特定失败模式分支。工具错误被格式化为纯文本并传递回模型，丢失了所有结构。OpenClaw（[TypeScript/Node.js](https://github.com/openclaw)）将工具失败作为无类型的 JavaScript `Error` 对象暴露——在不同的运行时中存在相同的限制。

EdgeCrab 使用类型化错误（`thiserror` 枚举）有两个具体原因：

1. **调用者可以根据变体分支** —— 代理循环对 `RateLimited`、`BudgetExhausted` 和 `ToolExecution` 的处理方式不同。使用字符串类型错误无法做到这一点。

2. **工具失败变成结构化的 LLM 输入** —— 当工具返回 `Err(ToolError)` 时，循环不会传播 Rust 错误。它将错误序列化为 JSON `ToolErrorResponse` 并附加到对话历史中。模型读取它，理解出了什么问题，并做出调整——而不是盲目循环。

🦀 *这是 EdgeCrab 在工具使用方面的决定性优势：当钳子错过时，不是暴露一个不透明的错误字符串，而是螃蟹告诉自己确切是哪个变体失败了以及下次应该尝试什么角度。*

---

## 两个错误枚举

### `AgentError` — 代理 / 提供商层

```rust
// edgecrab-types/src/error.rs
pub enum AgentError {
    Llm(String),
    ToolExecution { tool: String, message: String },
    ContextLimit { used: usize, limit: usize },
    BudgetExhausted { used: u32, max: u32 },
    Interrupted,
    Config(String),
    Database(String),
    Io(#[from] std::io::Error),
    Serde(#[from] serde_json::Error),
    RateLimited { provider: String, retry_after_ms: u64 },
    CompressionFailed(String),
    ApiRefusal(String),
    MalformedToolCall(String),
    Plugin { plugin: String, message: String },
    GatewayDelivery { platform: String, message: String },
    Migration(String),
    安全(String),
    Validation(String),
}

pub type Result<T> = std::result::Result<T, AgentError>;
```

### `ToolError` — 工具执行层

```rust
pub enum ToolError {
    NotFound(String),
    InvalidArgs { tool: String, message: String },
    Unavailable { tool: String, reason: String },
    Timeout { tool: String, seconds: u64 },
    PermissionDenied(String),
    ExecutionFailed { tool: String, message: String },
    CapabilityDenied {
        tool: String,
        code: String,
        message: String,
        suppression_key: Option<String>,  // prevents infinite retry loops
        suggested_tool: Option<String>,   // guides the model to a fallback
        suggested_action: Option<String>, // human-readable next step
    },
    Other(String),
}
```

---

## `ToolError` → JSON 负载 → 模型输入

当 `ToolHandler::execute()` 返回 `Err(ToolError)` 时，执行路径如下：

```
  tool returns  Err(ToolError::ExecutionFailed { tool: "write_file",
                                                  message: "Read-only filesystem" })
        │
        ▼
  ToolError::to_llm_payload()
        │
        ▼
  ToolErrorResponse {
      response_type:     "error",
      category:          "execution",
      code:              "execution_failed",
      error:             "Read-only filesystem",
      retryable:         true,
      suppress_retry:    false,
      suppression_key:   None,
      tool:              Some("write_file"),
      suggested_tool:    None,
      suggested_action:  Some("Use a writable path under /tmp or the project root"),
  }
        │
        ▼
  serde_json::to_string(&response) → JSON string
        │
        ▼
  Message::tool_result(tool_call_id, "write_file", json_string)
        │
        ▼
  Appended to conversation history
        │
        ▼
  LLM reads it on next iteration, adjusts its approach
```

模型看到的是一个结构化的 JSON 对象——不是堆栈跟踪，不是 panic，不是静默。

---

## 重试和抑制逻辑

`ToolError` 有三个分类方法，调度器在决定失败后该做什么之前使用：

```
  ToolError::is_retryable()
  ─────────────────────────────────────────────────────────────────────
  ExecutionFailed, Timeout  → true   (transient; try again)
  NotFound, PermissionDenied → false  (structural; retrying won't help)

  ToolError::should_suppress_retry()
  ─────────────────────────────────────────────────────────────────────
  CapabilityDenied with suppression_key  → true
  (don't feed back into the model loop — it clearly can't do this)

  ToolError::suppression_key()
  ─────────────────────────────────────────────────────────────────────
  Stable string key used to deduplicate retry loops:
    "execute_code:no_docker"  prevents the model from requesting
    Docker-based code execution 5 times in a row when Docker is absent
```

---

## 循环中的 `AgentError` 恢复

对话循环 (`execute_loop`) 对每个 `AgentError` 变体的处理方式不同：

```
  execute_loop
        │
        ├── AgentError::RateLimited { retry_after_ms }
        │       └── sleep(retry_after_ms) + exponential backoff
        │           base=500ms, max retries=3
        │
        ├── AgentError::ContextLimit { used, limit }
        │       └── trigger compression pipeline → retry API call
        │
        ├── AgentError::BudgetExhausted { used, max }
        │       └── break loop
        │           ConversationResult::budget_exhausted = true
        │
        ├── AgentError::Interrupted
        │       └── break loop
        │           ConversationResult::interrupted = true
        │
        ├── AgentError::MalformedToolCall
        │       └── log warning + continue loop
        │           (model issued bad JSON; give it another chance)
        │
        └── AgentError::Llm / AgentError::Serde
                └── propagate to caller (unrecoverable turn failure)
```

---

## 模糊匹配 on `ToolError::NotFound`

当注册表无法通过精确名称找到工具时，它会在放弃前应用 Levenshtein 距离 ≤ 3：

```
  model requests "write_fiel"  (typo)
        │
        ▼
  registry.dispatch("write_fiel", ...)
        │
        ▼
  exact match? No
        │
        ▼
  fuzzy_match("write_fiel")
        │
        ▼
  Levenshtein("write_fiel", "write_file") = 1  ≤ 3
        │
        ▼
  ToolError::NotFound("write_fiel. Did you mean: write_file?")
```

"Did you mean" 提示包含在反馈给模型的 `ToolErrorResponse` 中——它通常在一步内自我纠正。

**参考：** [莱文斯坦距离](https://en.wikipedia.org/wiki/Levenshtein_distance)

---

## `#[from]` 隐式转换

`AgentError` 为标准库错误类型派生 `#[from]`，启用 `?` 语法：

```rust
fn read_config(path: &Path) -> crate::Result<AppConfig> {
    let text = std::fs::read_to_string(path)?;  // io::Error → AgentError::Io
    let cfg = serde_yaml::from_str(&text)
        .map_err(|e| AgentError::Config(e.to_string()))?;
    Ok(cfg)
}
```

---

## `#![deny(clippy::unwrap_used)]`

在 `edgecrab-types`（所有其他 crate 都导入的叶子 crate）中强制执行。`#[cfg(test)]` 之外不允许使用 `.unwrap()` 或 `.expect()`。违反时编译失败。

---

## 实用规则

> **如果失败应该作为对话的一部分对模型可见 → 使用 `ToolError`。**
> **如果它应该中止或短路对话机制 → 使用 `AgentError`。**

不要从 `ToolHandler` 返回 `AgentError`。进行映射：

```rust
// Wrong:
async fn execute(...) -> Result<String, ToolError> {
    do_something()?  // AgentError leaks through
}

// Right:
async fn execute(...) -> Result<String, ToolError> {
    do_something()
        .map_err(|e| ToolError::ExecutionFailed {
            tool: self.name().to_string(),
            message: e.to_string(),
        })
}
```

---

## 提示

> **提示：使用 `ToolError::capability_denied()` 处理软性"无法执行此操作"的情况。**
> 设置 `.with_suggested_action()` 和 `.with_suppression_key()` 引导模型远离无限重试循环。

> **提示：`ToolErrorRecord` 存储在 `ConversationResult::tool_errors` 中。**
> 会话结束后，您可以检查每个工具失败，包括完整参数和发送回模型的确切响应。对于调试代理行为很有用。

> **提示：`AgentError::安全(String)` 由 `edgecrab-security` 检查使用。**
> 如果路径逃逸了 jail 或命令匹配了危险模式，检查会返回 `Err(AgentError::安全(...))` ——循环将其转换为模型可见的 `ToolError::PermissionDenied` 响应。

---

## 常见问题

**问：如果工具 panic 会发生什么？**
Tokio 任务 panic 不会崩溃进程。对话循环捕获失败的 join 并合成 `ToolError::ExecutionFailed` 响应。

**问：为什么 `Database(String)` 是字符串而不是 `rusqlite::Error`？**
在 `edgecrab-types` 中暴露 `rusqlite::Error` 会强制所有 10 个 crate 依赖 `rusqlite`。`edgecrab-state` crate 在跨 crate 边界之前将错误转换为字符串。

**问：LLM 能看到 `AgentError` 详情吗？**
不能。只有序列化为 `ToolErrorResponse` 的 `ToolError` 会进入对话历史。`AgentError` 传播到前端，由前端决定如何呈现。

---

## 交叉引用

- 循环中错误处理的位置 → [对话循环](../003_agent_core/002_conversation_loop.md)
- 产生 `ToolError` 的工具分发 → [工具注册表](../004_tools_system/001_tool_registry.md)
- 安全错误 (`AgentError::安全`) → [安全](../011_security/001_security.md)