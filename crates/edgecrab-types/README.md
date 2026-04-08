# edgecrab-types

> **Why this crate?** Every other crate in EdgeCrab needs to speak the same language.  
> `edgecrab-types` is that language — the single, zero-dependency home for every shared type:  
> messages, tool schemas, roles, errors, and config shapes. By living here, these types are  
> imported by all other crates without creating circular dependencies.

Part of [EdgeCrab](https://www.edgecrab.com) — the Rust SuperAgent.

---

## What's inside

| Module | Key types |
|--------|-----------|
| `message` | `Message`, `Role` (system / user / assistant / tool) |
| `tool` | `ToolSchema`, `ToolCall`, `ToolResult`, `ToolError` |
| `config` | `ModelConfig`, `AppConfig`, `Platform` |
| `error` | `EdgeCrabError` (top-level anyhow-compatible error enum) |

## Add to your crate

```toml
# Cargo.toml
[dependencies]
edgecrab-types = { path = "../edgecrab-types" }
```

## Usage

```rust
use edgecrab_types::{Message, Role, ToolCall, ToolError};

// Build a conversation message
let msg = Message {
    role: Role::User,
    content: "Explain the ReAct loop".into(),
    ..Default::default()
};

// Represent a tool result
let result: Result<String, ToolError> = Ok(r#"{"lines": 42}"#.into());
```

## Design rules

- **Zero runtime dependencies** — only `serde`, `serde_json`, `chrono`, `uuid`, `thiserror`, `regex`, and `edgequake-llm`.
- **All types derive `Serialize` / `Deserialize`** so they round-trip cleanly over JSON.
- **No business logic here** — types only; behaviour lives in `edgecrab-core` or `edgecrab-tools`.

---

> Full docs, guides, and release notes → [edgecrab.com](https://www.edgecrab.com)
