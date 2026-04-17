# EdgeCrab SDK — Tutorial

> **Cross-references:** [WHY](01-WHY.md) | [SPEC](02-SPEC.md) | [DEVELOPER-DOCS](08-DEVELOPER-DOCS.md)

---

## WHY This Tutorial Exists

Every SDK tutorial starts with "install" and "hello world." Most stop there. This tutorial progresses from trivial to production-grade, because real developers don't build hello-world agents.

---

## Prerequisites

```bash
# Python
pip install edgecrab

# Node.js
npm install edgecrab

# Rust
cargo add edgecrab-sdk
```

Set at least one provider API key:

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
# or
export OPENAI_API_KEY="sk-..."
# or any of the 13 supported providers
```

---

## Part 1: First Agent (30 Seconds)

### Python

```python
from edgecrab import Agent

async def main():
    agent = Agent()
    print(await agent.chat("What is the meaning of life?"))

# Or synchronously:
agent = Agent()
print(agent.chat_sync("What is the meaning of life?"))
```

### Node.js

```typescript
import { Agent } from "edgecrab";

const agent = new Agent();
console.log(await agent.chat("What is the meaning of life?"));
```

### Rust

```rust
use edgecrab_sdk::Agent;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let agent = Agent::new("anthropic/claude-sonnet-4").build()?;
    println!("{}", agent.chat("What is the meaning of life?").await?);
    Ok(())
}
```

**What happened:** The SDK loaded `~/.edgecrab/config.yaml`, resolved the API key from environment, created a session in SQLite, ran the LLM call, saved the conversation, and returned the response. All automatic.

---

## Part 2: Choosing a Model

EdgeCrab supports 15 providers and 200+ models. Model names use `provider/model` format:

```python
from edgecrab import Agent

# Anthropic
agent = Agent("anthropic/claude-sonnet-4")
agent = Agent("anthropic/claude-opus-4")
agent = Agent("anthropic/claude-haiku-3")

# OpenAI
agent = Agent("openai/gpt-4o")
agent = Agent("openai/o3-mini")

# Google
agent = Agent("google/gemini-2.5-flash")

# DeepSeek
agent = Agent("deepseek/deepseek-chat")

# Open-source (via Ollama)
agent = Agent("ollama/llama3.3:70b")

# And 8 more providers...
```

### Checking Available Models

```python
from edgecrab import ModelCatalog

catalog = ModelCatalog.get()
for provider_id in catalog.provider_ids():
    models = catalog.models_for(provider_id)
    print(f"{provider_id}: {len(models)} models")
```

---

## Part 3: Streaming Responses

### Python — Async Iterator

```python
from edgecrab import Agent

agent = Agent("anthropic/claude-sonnet-4")

async for event in agent.stream("Write me a haiku about Rust"):
    match event:
        case StreamEvent.Token(text):
            print(text, end="", flush=True)
        case StreamEvent.Reasoning(text):
            print(f"[thinking] {text}")
        case StreamEvent.ToolExec(name, args):
            print(f"\n[tool] {name}({args})")
        case StreamEvent.Done():
            print("\n--- Done ---")
```

### Python — Callback Style

```python
from edgecrab import Agent

agent = Agent(
    "anthropic/claude-sonnet-4",
    on_stream=lambda event: print(event.text, end="") if event.is_token else None,
)
result = await agent.run("Write me a haiku about Rust")
```

### Node.js

```typescript
import { Agent, StreamEvent } from "edgecrab";

const agent = new Agent("anthropic/claude-sonnet-4");

for await (const event of agent.stream("Write me a haiku about Rust")) {
    if (event.type === "token") {
        process.stdout.write(event.text);
    }
}
```

### Rust

```rust
use edgecrab_sdk::{Agent, StreamEvent};
use futures::StreamExt;

let agent = Agent::new("anthropic/claude-sonnet-4").build()?;
let mut stream = agent.stream("Write me a haiku about Rust");

while let Some(event) = stream.next().await {
    match event {
        StreamEvent::Token(text) => print!("{text}"),
        StreamEvent::Reasoning(text) => print!("[thinking] {text}"),
        StreamEvent::ToolExec { name, .. } => println!("\n[tool] {name}"),
        StreamEvent::Done => println!("\n--- Done ---"),
        _ => {}
    }
}
```

---

## Part 4: Custom Tools

### Simple Tool (Python)

```python
from edgecrab import Agent, Tool

@Tool("get_weather", description="Get weather for a city")
async def get_weather(city: str, units: str = "celsius") -> dict:
    """Get current weather.

    Args:
        city: City name (e.g. "Tokyo")
        units: Temperature units
    """
    # Real implementation would call a weather API
    return {"city": city, "temp": 22, "units": units, "condition": "sunny"}

agent = Agent("claude-sonnet-4", tools=[get_weather])
print(await agent.chat("What's the weather in Tokyo?"))
# The agent will call get_weather("Tokyo") and incorporate the result
```

### Tool with Context (Python)

```python
from edgecrab import Agent, Tool, ToolContext

@Tool("query_db", description="Query the application database")
async def query_db(sql: str, ctx: ToolContext) -> dict:
    """Execute a read-only SQL query.

    Args:
        sql: SQL query to execute
    """
    # ctx provides session_id, cwd, platform, etc.
    print(f"Running in session: {ctx.session_id}")
    print(f"Working directory: {ctx.cwd}")

    # Your database logic here
    return {"rows": [], "count": 0}

agent = Agent("claude-sonnet-4", tools=[query_db])
```

### Multiple Tools (Node.js)

```typescript
import { Agent, Tool } from "edgecrab";

const searchDocs = Tool.create({
    name: "search_docs",
    description: "Search internal documentation",
    parameters: {
        query: { type: "string", description: "Search query" },
        limit: { type: "number", description: "Max results", default: 5 },
    },
    handler: async (args) => {
        // Search implementation
        return { results: ["doc1.md", "doc2.md"], total: 2 };
    },
});

const createTicket = Tool.create({
    name: "create_ticket",
    description: "Create a support ticket",
    parameters: {
        title: { type: "string", description: "Ticket title" },
        priority: { type: "string", description: "low, medium, high" },
    },
    handler: async (args) => {
        return { ticket_id: "TKT-123", status: "created" };
    },
});

const agent = new Agent("claude-sonnet-4", { tools: [searchDocs, createTicket] });
```

### Tool in Rust

```rust
use edgecrab_sdk::{Agent, ToolHandler, ToolSchema, ToolContext, ToolError};
use async_trait::async_trait;
use serde_json::json;

struct WeatherTool;

#[async_trait]
impl ToolHandler for WeatherTool {
    fn name(&self) -> &'static str { "get_weather" }
    fn toolset(&self) -> &'static str { "custom" }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "get_weather".into(),
            description: "Get weather for a city".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "city": { "type": "string", "description": "City name" }
                },
                "required": ["city"]
            }),
            strict: None,
        }
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext)
        -> Result<String, ToolError>
    {
        let city = args["city"].as_str().unwrap_or("unknown");
        Ok(json!({"city": city, "temp": 22}).to_string())
    }
}

let agent = Agent::new("anthropic/claude-sonnet-4")
    .tool(WeatherTool)
    .build()?;
```

---

## Part 5: Using Built-in Tools

EdgeCrab ships with 90+ tools organized into toolsets. By default, CORE_TOOLS are enabled:

```python
from edgecrab import Agent

# Default agent — has file I/O, terminal, web search, memory, etc.
agent = Agent("claude-sonnet-4")

# Ask it to do real work
result = await agent.run("Read the README.md and summarize the project structure")
# Agent uses file_read tool automatically

result = await agent.run("Find all TODO comments in the src/ directory")
# Agent uses file_search tool (ripgrep)

result = await agent.run("What's the latest news about Rust 2025 edition?")
# Agent uses web_search + web_extract tools
```

### Controlling Toolsets

```python
# Only specific toolsets
agent = Agent("claude-sonnet-4", toolsets=["file", "terminal"])

# Disable specific toolsets
agent = Agent("claude-sonnet-4", disabled_toolsets=["web", "browser"])

# Disable specific tools
agent = Agent("claude-sonnet-4", disabled_tools=["terminal"])

# Add custom tools alongside built-ins
agent = Agent(
    "claude-sonnet-4",
    tools=[get_weather],  # Custom tools added to core
)
```

---

## Part 6: Conversations and Sessions

### Multi-turn Conversation

```python
from edgecrab import Agent

agent = Agent("claude-sonnet-4")

# Each chat() call maintains conversation history
await agent.chat("My name is Alice")
await agent.chat("I work on the EdgeCrab project")
response = await agent.chat("What's my name and what do I work on?")
# Agent remembers: "Your name is Alice and you work on EdgeCrab"
```

### Session Persistence

```python
# Sessions are auto-saved to SQLite
agent = Agent("claude-sonnet-4")
session_id = agent.session_id  # e.g., "ses_abc123"

# ... later, resume the session
agent2 = Agent("claude-sonnet-4", session_id=session_id)
# Full conversation history is restored
```

### Session Search

```python
# Search across all past sessions
hits = await agent.search_sessions("database migration")
for hit in hits:
    print(f"Session: {hit.session_id}")
    print(f"Title: {hit.title}")
    print(f"Snippet: {hit.snippet}")
```

### Forking

```python
# Fork creates an independent copy with shared history
child = await agent.fork(model="anthropic/claude-haiku-3")
# child has all previous messages but diverges from here
await child.chat("Summarize our conversation")  # Uses cheaper model
```

---

## Part 7: Cost Tracking

```python
from edgecrab import Agent

agent = Agent("claude-sonnet-4")
result = await agent.run("Analyze the codebase and suggest improvements")

# Detailed cost breakdown
print(f"Total cost: ${result.cost.total_cost:.4f}")
print(f"  Input:  ${result.cost.input_cost:.4f} ({result.usage.input_tokens} tokens)")
print(f"  Output: ${result.cost.output_cost:.4f} ({result.usage.output_tokens} tokens)")
print(f"  Cache:  ${result.cost.cache_read_cost:.4f}")
print(f"API calls: {result.api_calls}")
```

---

## Part 8: MCP Servers

### Stdio MCP Server

```python
from edgecrab import Agent, McpServer

agent = Agent(
    "claude-sonnet-4",
    mcp_servers={
        "postgres": McpServer(
            command="npx",
            args=["-y", "@modelcontextprotocol/server-postgres"],
            env={"DATABASE_URL": "postgresql://localhost/mydb"},
        ),
    },
)

# MCP tools are automatically discovered
result = await agent.run("List all tables in the database")
```

### HTTP MCP Server

```python
agent = Agent(
    "claude-sonnet-4",
    mcp_servers={
        "remote": McpServer(
            url="https://mcp.example.com/rpc",
            bearer_token="sk-...",
        ),
    },
)
```

---

## Part 9: Approval Workflow

For production agents, you want human approval for dangerous operations:

```python
from edgecrab import Agent, ApprovalRequest, ApprovalResponse

async def my_approval(request: ApprovalRequest) -> ApprovalResponse:
    """Called before dangerous commands (rm, git push, etc.)"""
    print(f"Agent wants to run: {request.command}")
    print(f"Reason: {request.reasons}")
    print(f"Risk level: {request.severity}")

    user_input = input("Allow? [y/n/always]: ")
    if user_input == "y":
        return ApprovalResponse.ONCE
    elif user_input == "always":
        return ApprovalResponse.SESSION  # Allow for rest of session
    else:
        return ApprovalResponse.DENY

agent = Agent(
    "claude-sonnet-4",
    approval_handler=my_approval,
)

result = await agent.run("Delete all .tmp files and restart the server")
# Agent will trigger approval before destructive commands
```

---

## Part 10: Production Patterns

### Error Handling

```python
from edgecrab import Agent, AgentError, ToolError

agent = Agent("claude-sonnet-4")

try:
    result = await agent.run("Do something complex")
except AgentError.RateLimited as e:
    print(f"Rate limited by {e.provider}, retry after {e.retry_after_ms}ms")
except AgentError.BudgetExhausted as e:
    print(f"Used {e.used}/{e.max} iterations")
except AgentError.ContextLimit as e:
    print(f"Context window exhausted: {e.used}/{e.limit} tokens")
except AgentError as e:
    print(f"Agent error: {e}")

# Check for tool errors in successful runs
if result.tool_errors:
    for err in result.tool_errors:
        print(f"Tool {err.tool} failed: {err.message}")
```

### Configuration from File

```python
from edgecrab import Agent, Config

# Load custom config
config = Config.load_from("./my-agent-config.yaml")
agent = Agent("claude-sonnet-4", config=config)
```

### Multi-Agent Pipeline

```python
from edgecrab import Agent

# Stage 1: Research
researcher = Agent("claude-sonnet-4", toolsets=["web", "file"])
research = await researcher.run("Research the latest Rust async patterns")

# Stage 2: Write
writer = Agent("claude-sonnet-4", toolsets=["file"])
draft = await writer.run(
    f"Based on this research, write a blog post:\n\n{research.response}"
)

# Stage 3: Review (cheaper model)
reviewer = Agent("anthropic/claude-haiku-3")
review = await reviewer.run(
    f"Review this draft for accuracy:\n\n{draft.response}"
)
```

### Background Monitoring

```python
from edgecrab import Agent
import asyncio

agent = Agent("claude-sonnet-4", toolsets=["terminal", "file"])

# Run a monitoring agent in the background
async def monitor():
    while True:
        result = await agent.run(
            "Check if the server at localhost:8080 is healthy. "
            "If not, read the logs and report what went wrong."
        )
        if "unhealthy" in result.response.lower():
            # Alert logic here
            pass
        await asyncio.sleep(300)  # Check every 5 minutes
```

---

## Part 11: Testing Your Agent

```python
import pytest
from edgecrab import Agent

@pytest.mark.asyncio
async def test_weather_agent():
    agent = Agent("claude-sonnet-4", tools=[get_weather])
    result = await agent.run("What's the weather in Paris?")

    assert result.response is not None
    assert "Paris" in result.response
    assert result.cost.total_cost > 0

@pytest.mark.asyncio
async def test_agent_session_persistence():
    agent = Agent("claude-sonnet-4")
    await agent.chat("Remember the number 42")

    # Fork and verify memory
    agent2 = Agent("claude-sonnet-4", session_id=agent.session_id)
    response = await agent2.chat("What number did I ask you to remember?")
    assert "42" in response
```

---

## Brutal Honest Assessment

### What This Tutorial Gets Right
- Progressive complexity — from 1 line to production patterns
- Real code that would actually work (once the SDK exists)
- All three languages shown where they differ meaningfully
- Production patterns section covers what most tutorials skip

### What's Missing
- **Real-world examples with specific APIs** (e.g., Stripe, GitHub, AWS) — needs SDK to be built first
- **Deployment tutorial** — how to run an EdgeCrab agent in Docker, Kubernetes, Lambda
- **Performance tuning guide** — when to use compression, how to optimize token usage
- **Migration guide from competing SDKs** — "switching from Pydantic AI" tutorial

### Improvements Made After Assessment
- Added session forking example (unique to EdgeCrab)
- Added cost tracking section — developers care about money
- Added testing section — SDK must be testable from day one
- Noted that MCP examples use real npm package names (verifiable)
