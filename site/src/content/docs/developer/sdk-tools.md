---
title: Custom Tools
description: Build and register custom tools for EdgeCrab agents using the #[edgecrab_tool] macro (Rust) or native registration.
sidebar:
  order: 4
---

Custom tools extend what your agent can do. EdgeCrab supports registering
tools at compile time (Rust) or at runtime.

## Rust — `#[edgecrab_tool]` Macro

The `#[edgecrab_tool]` attribute macro is the easiest way to create custom
tools. It generates all the boilerplate: struct definition, `ToolHandler`
implementation, JSON schema, and auto-registration via `inventory`.

### Basic Tool

```rust
use edgecrab_sdk::prelude::*;

/// Get the current weather for a city.
#[edgecrab_tool(name = "get_weather", toolset = "demo", emoji = "🌤️")]
async fn get_weather(city: String) -> Result<String, edgecrab_types::ToolError> {
    Ok(format!("The weather in {city} is sunny and 22°C."))
}
```

This generates:

1. A `GetWeather` struct
2. A `ToolHandler` impl with the correct JSON schema
3. An `inventory::submit!()` call for auto-registration

### Tool with Multiple Parameters

```rust
/// Search for files matching a pattern.
#[edgecrab_tool(name = "search_files", toolset = "filesystem")]
async fn search_files(
    pattern: String,
    directory: Option<String>,
    max_results: Option<i64>,
) -> Result<String, edgecrab_types::ToolError> {
    let dir = directory.unwrap_or_else(|| ".".to_string());
    let limit = max_results.unwrap_or(10);
    Ok(format!("Found files matching '{pattern}' in {dir} (limit: {limit})"))
}
```

- `Option<T>` parameters become optional in the JSON schema
- Doc comments become the tool's description
- Parameter types map to JSON schema types automatically

### Tool with Context

Tools can receive a `ToolContext` to access the agent's working directory
and other runtime information:

```rust
#[edgecrab_tool(name = "read_cwd", toolset = "fs")]
async fn read_cwd(ctx: ToolContext) -> Result<String, edgecrab_types::ToolError> {
    let cwd = ctx.working_directory.display();
    Ok(format!("Working directory: {cwd}"))
}
```

The `ToolContext` parameter is automatically detected and excluded from
the tool's parameter schema.

### Macro Attributes

| Attribute | Required | Description |
|-----------|----------|-------------|
| `name` | Yes | Tool name (snake_case) |
| `toolset` | No | Toolset group name |
| `emoji` | No | Display emoji for the tool |

### Type Mapping

| Rust Type | JSON Schema Type |
|-----------|-----------------|
| `String` | `string` |
| `i64`, `i32`, `u32`, `u64` | `integer` |
| `f64`, `f32` | `number` |
| `bool` | `boolean` |
| `Option<T>` | Same as `T`, but not required |

## Manual Tool Registration

For more control, implement `ToolHandler` directly:

```rust
use edgecrab_sdk::prelude::*;
use edgecrab_types::ToolError;

struct MyTool;

#[async_trait]
impl ToolHandler for MyTool {
    fn name(&self) -> &str { "my_tool" }
    fn toolset(&self) -> &str { "custom" }
    fn emoji(&self) -> &str { "🔧" }

    fn schema(&self) -> serde_json::Value {
        serde_json::json!({
            "name": "my_tool",
            "description": "A custom tool",
            "input_schema": {
                "type": "object",
                "properties": {
                    "input": { "type": "string", "description": "The input" }
                },
                "required": ["input"]
            }
        })
    }

    async fn call(
        &self,
        args: &serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let input = args["input"].as_str().unwrap_or("");
        Ok(format!("Processed: {input}"))
    }
}
```

Then register via `inventory::submit!`:

```rust
inventory::submit!(Box::new(MyTool) as Box<dyn ToolHandler>);
```
