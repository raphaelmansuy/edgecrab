---
title: Context Engine
description: Pluggable external tool schema injection for the EdgeCrab ReAct loop. Grounded in crates/edgecrab-core/src/context_engine.rs and agent.rs.
sidebar:
  order: 10
---

The Context Engine is a pluggable system that injects external tool schemas into the agent's ReAct loop at runtime. This allows third-party integrations to expose tools without modifying the core tool registry.

---

## How It Works

```
AgentBuilder
  .context_engine(my_engine)   // Arc<dyn ContextEngine>
  .build()?

// During each ReAct loop iteration:
tool_defs = registry.get_tool_definitions()
          + context_engine.get_tool_schemas()   // ← injected here
provider.chat(model, messages, tool_defs)
```

The engine's tool schemas are appended to the active tool definitions before each LLM call. The LLM sees them as regular tools and can call them. Dispatch is handled by the engine implementation.

---

## Configuration

```yaml
# ~/.edgecrab/config.yaml
context:
  engine: "my_engine_name"   # optional — names the engine implementation
```

---

## Implementing a Context Engine

Implement the `ContextEngine` trait:

```rust
use async_trait::async_trait;
use edgecrab_types::ToolSchema;

#[async_trait]
pub trait ContextEngine: Send + Sync {
    /// Return tool schemas to inject into the ReAct loop
    fn get_tool_schemas(&self) -> Vec<ToolSchema>;

    /// Handle a tool call dispatched by the engine
    async fn handle_tool_call(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<String, String>;
}
```

Then pass it to `AgentBuilder`:

```rust
let engine = Arc::new(MyContextEngine::new());

let agent = AgentBuilder::new("openai/gpt-4o")
    .provider(provider)
    .tools(registry)
    .context_engine(engine)
    .build()?;
```

---

## Use Cases

- **IDE Integration**: VS Code or JetBrains extensions injecting workspace-specific tools
- **Domain Plugins**: Industry-specific tools loaded at runtime
- **Dynamic Tool Discovery**: Tools that appear/disappear based on project context
- **MCP Bridge**: Translating MCP server tools into the agent's native tool format
