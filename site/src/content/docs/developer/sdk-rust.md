---
title: Rust SDK
description: Complete guide to the EdgeCrab Rust SDK — SdkAgent, builder pattern, streaming, custom tools, and session management.
sidebar:
  order: 2
---

The Rust SDK provides zero-cost abstractions over the EdgeCrab agent runtime.
It's the same engine that powers the CLI, exposed through a stable,
well-documented API.

## Installation

```toml
[dependencies]
edgecrab-sdk = "0.6"
tokio = { version = "1", features = ["full"] }
```

## Creating an Agent

### Minimal

```rust
use edgecrab_sdk::prelude::*;

let agent = SdkAgent::new("openai/gpt-4o")?;
```

### With Builder

```rust
use edgecrab_sdk::prelude::*;

let agent = SdkAgent::builder("copilot/gpt-5-mini")?
    .max_iterations(20)
    .temperature(0.7)
    .streaming(true)
    .quiet_mode(true)
    .session_id("my-session-id".to_string())
    .build()?;
```

### Builder Options

| Method | Type | Default | Description |
|--------|------|---------|-------------|
| `max_iterations(n)` | `u32` | `25` | Maximum tool-use iterations per turn |
| `temperature(t)` | `f32` | `0.7` | Sampling temperature |
| `streaming(b)` | `bool` | `false` | Enable token streaming |
| `quiet_mode(b)` | `bool` | `false` | Suppress internal logging |
| `session_id(id)` | `String` | auto | Resume a specific session |
| `platform(p)` | `Platform` | `Sdk` | Platform identifier |
| `instructions(s)` | `impl Into<String>` | none | Custom system prompt appended to base |
| `enabled_toolsets(v)` | `Vec<String>` | all | Only enable these toolsets |
| `disabled_toolsets(v)` | `Vec<String>` | none | Disable these toolsets |
| `disabled_tools(v)` | `Vec<String>` | none | Disable specific tools by name |

### From Config

```rust
use edgecrab_sdk::prelude::*;

let config = SdkConfig::load()?;
let agent = SdkAgent::from_config(&config)?;
```

## Simple Chat

```rust
let reply = agent.chat("What is EdgeCrab?").await?;
println!("{reply}");
```

`chat()` sends a message and returns the agent's text response. The agent
may use tools internally — you get the final answer.

### Chat in a Specific Directory

```rust
let reply = agent.chat_in_cwd("List files here", "/tmp/myproject").await?;
```

## Full Conversation

```rust
let result = agent.run("Analyze this codebase").await?;

println!("Response: {}", result.final_response);
println!("API calls: {}", result.api_calls);
println!("Model: {}", result.model);
println!("Input tokens: {}", result.usage.input_tokens);
println!("Output tokens: {}", result.usage.output_tokens);
println!("Cost: ${:.4}", result.cost.total_cost);
println!("Interrupted: {}", result.interrupted);
println!("Budget exhausted: {}", result.budget_exhausted);
```

### ConversationResult Fields

| Field | Type | Description |
|-------|------|-------------|
| `final_response` | `String` | The agent's final text answer |
| `messages` | `Vec<Message>` | Full message history |
| `session_id` | `String` | Session identifier |
| `api_calls` | `u32` | Number of LLM API calls made |
| `interrupted` | `bool` | Whether the user interrupted |
| `budget_exhausted` | `bool` | Whether max iterations was hit |
| `model` | `String` | Model used |
| `usage` | `Usage` | Token counts |
| `cost` | `Cost` | Cost breakdown |
| `tool_errors` | `Vec<ToolErrorRecord>` | Errors from tool executions |

## Streaming

```rust
let mut rx = agent.stream("Write a poem").await?;

while let Some(event) = rx.recv().await {
    match &event {
        StreamEvent::Token(text) => print!("{text}"),
        StreamEvent::Done => println!("\n[done]"),
        StreamEvent::ToolExec { name, .. } => println!("[tool: {name}]"),
        StreamEvent::ToolDone { result_preview, .. } => {
            println!("[result: {result_preview}]");
        }
        _ => {} // other events
    }
}
```

### StreamEvent Variants

| Variant | Description |
|---------|-------------|
| `Token(String)` | A text token from the LLM |
| `ToolExec { tool_call_id, name, args_json }` | Tool execution started |
| `ToolProgress { tool_call_id, message }` | Tool progress update |
| `ToolDone { tool_call_id, name, result_preview, duration_ms, is_error }` | Tool completed |
| `SubAgentStart { agent_id, model }` | Sub-agent spawned |
| `SubAgentFinish { agent_id }` | Sub-agent completed |
| `Done` | Conversation finished |
| `Error(String)` | An error occurred |

## Session Management

```rust
// Get current session ID
if let Some(id) = agent.session_id().await {
    println!("Session: {id}");
}

// Get conversation history
let messages = agent.messages().await;
for msg in &messages {
    println!("[{:?}] {:?}", msg.role, msg.content);
}

// Start a fresh session
agent.new_session().await;

// Fork agent for parallel work
let forked = agent.fork().await?;
let result = forked.run("Analyze independently").await?;

// Export full session (snapshot + messages)
let export = agent.export().await;
println!("Session: {:?}", export.snapshot.session_id);
println!("Messages: {}", export.messages.len());

// List recent sessions
let sessions = agent.list_sessions(20)?;
for s in &sessions {
    println!("{} | {} | {} messages", s.id, s.model.as_deref().unwrap_or("?"), s.message_count);
}

// Search sessions
let hits = agent.search_sessions("kubernetes", 10)?;
for h in &hits {
    println!("score={:.2} | {}", h.score, h.snippet);
}
```

### Run Conversation with System Prompt

```rust
let result = agent.run_conversation(
    "Explain this code",
    Some("You are a senior code reviewer."),
    None,
).await?;
```

## Agent Control

```rust
// Change reasoning effort at runtime
agent.set_reasoning_effort(Some("high".to_string())).await;

// Toggle streaming
agent.set_streaming(true).await;

// Interrupt a running conversation (from another task)
agent.interrupt();

// Check cancellation
if agent.is_cancelled() {
    println!("Agent was interrupted");
}
```

## Configuration

```rust
use edgecrab_sdk::prelude::*;

// Load from ~/.edgecrab/config.yaml
let config = SdkConfig::load()?;

// Load from a specific path
let config = SdkConfig::load_from("/path/to/config.yaml")?;

// Get the default model
let model = config.default_model();

// Profile / home directory
let home = edgecrab_home();
let home = ensure_edgecrab_home()?; // creates if missing
```

## Model Catalog

```rust
use edgecrab_sdk::prelude::*;

// List all providers
let providers = SdkModelCatalog::provider_ids();
println!("Providers: {providers:?}");

// List models for a provider
let models = SdkModelCatalog::models_for_provider("openai");
for (id, name) in &models {
    println!("  {id}: {name}");
}

// Get context window
if let Some(window) = SdkModelCatalog::context_window("openai", "gpt-4o") {
    println!("Context: {window} tokens");
}

// Get pricing
if let Some(pricing) = SdkModelCatalog::pricing("openai", "gpt-4o") {
    println!("Input: ${}/1M tokens", pricing.input);
}
```

## Error Handling

All SDK methods return `Result<T, SdkError>`. The error type has these variants:

| Variant | Description |
|---------|-------------|
| `Agent(AgentError)` | Error from the agent runtime |
| `Tool(ToolError)` | Error from a tool execution |
| `Config(String)` | Configuration error |
| `Provider { model, message }` | Provider creation failed |
| `Serialization(serde_json::Error)` | JSON serialization error |
| `NotInitialized(String)` | Component not initialized |

```rust
match agent.chat("hello").await {
    Ok(reply) => println!("{reply}"),
    Err(SdkError::Agent(e)) => eprintln!("Agent error: {e}"),
    Err(SdkError::Config(msg)) => eprintln!("Config: {msg}"),
    Err(e) if e.is_retryable() => {
        // Rate limited or transient — safe to retry
        eprintln!("Retryable: {e}");
    }
    Err(e) => eprintln!("Error: {e}"),
}
```
