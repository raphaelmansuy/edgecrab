# edgecrab-core

> **Why this crate?** Reasoning + tools alone don't make an agent. You need a loop that knows  
> when to call a tool, when to stop, how to compress a 200-message history without losing  
> context, and which of 14 LLM providers to route to. `edgecrab-core` is that loop — the  
> orchestration brain that turns raw LLM calls and tool results into coherent, goal-directed  
> behaviour.

Part of [EdgeCrab](https://www.edgecrab.com) — the Rust SuperAgent.

---

## What's inside

| Module | Responsibility |
|--------|---------------|
| `agent.rs` | `AgentBuilder` + `Agent` — hot-swap model, streaming, session binding |
| `conversation.rs` | `execute_loop()` — the ReAct tool-call loop (max 90 iterations) |
| `prompt_builder.rs` | System prompt assembly from ~12 sources (identity, memory, skills, AGENTS.md …) |
| `compression.rs` | Context compression: structural fallback + LLM-based summarisation |
| `model_catalog.rs` | 13 providers × 200+ models — single source of truth, user-overridable YAML |
| `model_router.rs` | Provider factory + smart routing |
| `pricing.rs` | Token cost calculation per provider |
| `sub_agent_runner.rs` | Sub-agent delegation runner |

## Add to your crate

```toml
[dependencies]
edgecrab-core = { path = "../edgecrab-core" }
```

## Simple usage

```rust
use edgecrab_core::{AgentBuilder, Agent};
use edgecrab_tools::ToolRegistry;

let registry = ToolRegistry::default();  // all built-in tools
let agent: Agent = AgentBuilder::new("anthropic/claude-opus-4.6")
    .tools(registry)
    .build()?;

// One-shot chat
let reply: String = agent.chat("Refactor this function for clarity").await?;
println!("{reply}");
```

## Streaming usage

```rust
use edgecrab_core::StreamEvent;
use tokio::sync::mpsc;

let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
agent.chat_streaming("Explain the ReAct loop", tx).await?;

while let Some(event) = rx.recv().await {
    match event {
        StreamEvent::Token(t) => print!("{t}"),
        StreamEvent::ToolCall { name, .. } => println!("\n[tool: {name}]"),
        StreamEvent::Done => break,
    }
}
```

## ReAct loop in brief

```
while turns < max_iterations && budget.ok() {
    if context > threshold → compress_with_llm()
    response = provider.chat(model, messages, tools)
    if response.has_tool_calls()  → dispatch tools → push results
    else                          → return final text
}
```

## Prompt caching note

The system prompt is assembled **once per session** and cached.  
**Do not rebuild or mutate it mid-conversation** — this invalidates Anthropic's prompt cache and multiplies costs. The only sanctioned mid-session mutation is `/compress`.

## Model catalog

```yaml
# ~/.edgecrab/models.yaml  (user overrides — merged on top of defaults)
providers:
  - name: anthropic
    models:
      - id: claude-opus-4.6
        context_window: 200000
        cost_per_1m_input: 15.00
        cost_per_1m_output: 75.00
```

---

> Full docs, guides, and release notes → [edgecrab.com](https://www.edgecrab.com)
