---
title: Python SDK
description: Integrate EdgeCrab into Python applications with the async-first edgecrab-sdk. Agent abstraction, streaming, tool customization, and CLI.
sidebar:
  order: 2
---

The EdgeCrab Python SDK (`edgecrab-sdk`) provides an async-first `Agent` class, streaming support, and a built-in CLI. Compatible with Python 3.10+.

---

## Installation

```bash
pip install edgecrab-sdk
```

---

## Quick Start

```python
from edgecrab import Agent

agent = Agent(model="anthropic/claude-sonnet-4-20250514")
reply = agent.chat("Explain Rust ownership in 3 sentences")
print(reply)
```

---

## Agent Configuration

```python
from edgecrab import Agent

agent = Agent(
    model="openai/gpt-4o",              # provider/model string
    system_prompt="You are a Rust expert.",  # optional system prompt
    api_key="sk-...",                   # optional; falls back to env var
    max_iterations=90,                  # max ReAct iterations (default: 90)
    toolsets=["file", "web"],           # enable specific toolsets
    session_name="my-project",          # named session (persisted)
    base_url=None,                      # custom API endpoint (OpenAI-compatible)
)
```

---

## Synchronous Usage

```python
from edgecrab import Agent

agent = Agent(model="openai/gpt-4o")

# Single turn
reply = agent.chat("List all .rs files in the current directory")
print(reply)

# Multi-turn (maintains history)
agent.chat("Explain the main function in src/main.rs")
agent.chat("Now add error handling to it")
```

---

## Async Usage

```python
import asyncio
from edgecrab import AsyncAgent

async def main():
    agent = AsyncAgent(model="openai/gpt-4o")
    reply = await agent.chat("Run cargo test and summarize failures")
    print(reply)

asyncio.run(main())
```

---

## Streaming

```python
from edgecrab import Agent

agent = Agent(model="anthropic/claude-opus-4-5")

for chunk in agent.stream("Write a Rust async HTTP client"):
    print(chunk, end="", flush=True)
print()
```

Async streaming:

```python
import asyncio
from edgecrab import AsyncAgent

async def main():
    agent = AsyncAgent(model="anthropic/claude-opus-4-5")
    async for chunk in agent.astream("Write a Rust async HTTP client"):
        print(chunk, end="", flush=True)
    print()

asyncio.run(main())
```

---

## Tool Events

Inspect tool calls and results during execution:

```python
from edgecrab import Agent, ToolCallEvent, ToolResultEvent

agent = Agent(model="openai/gpt-4o")

for event in agent.stream_events("Fix the failing tests in src/"):
    if isinstance(event, ToolCallEvent):
        print(f"Tool call: {event.name}({event.args})")
    elif isinstance(event, ToolResultEvent):
        print(f"Result: {event.result[:100]}")
    else:
        print(event.text, end="", flush=True)
```

---

## Custom Tools

Register your own Python functions as tools:

```python
from edgecrab import Agent, tool

@tool(description="Get the current UTC time")
def get_time() -> str:
    from datetime import datetime, timezone
    return datetime.now(timezone.utc).isoformat()

@tool(description="Read a file from a custom secure location")
def read_restricted_file(path: str) -> str:
    # your security logic here
    allowed = ["/data/project/"]
    if not any(path.startswith(p) for p in allowed):
        raise ValueError(f"Path not allowed: {path}")
    with open(path) as f:
        return f.read()

agent = Agent(model="openai/gpt-4o", extra_tools=[get_time, read_restricted_file])
reply = agent.chat("What time is it?")
```

---

## Session Persistence

```python
from edgecrab import Agent

# Named session — history is persisted across runs
agent = Agent(model="openai/gpt-4o", session_name="my-project")
agent.chat("Explain the architecture")

# Later, in a new script — session continues where it left off
agent = Agent(model="openai/gpt-4o", session_name="my-project")
agent.chat("Now add authentication")  # Has context from previous session
```

---

## Built-in CLI

The SDK includes a CLI:

```bash
# Interactive chat
edgecrab chat

# Single prompt
edgecrab chat "Summarize the last 10 git commits"

# Use a specific model
edgecrab chat --model anthropic/claude-opus-4-5 "Explain this codebase"

# List available models
edgecrab models

# Check health
edgecrab health
```

---

## Error Handling

```python
from edgecrab import Agent, EdgeCrabError, ProviderError, ToolError

agent = Agent(model="openai/gpt-4o")

try:
    reply = agent.chat("Read /etc/passwd")
except ToolError as e:
    print(f"Tool failed (likely security): {e}")
except ProviderError as e:
    print(f"LLM provider error: {e}")
except EdgeCrabError as e:
    print(f"General error: {e}")
```

---

## Full SDK Docs

See [sdks/python/README.md](https://github.com/raphaelmansuy/edgecrab/blob/main/sdks/python/README.md) in the repository for the complete API reference.

---

## Pro Tips

- **Use `session_name` for long-running projects**: Named sessions persist their history in `~/.edgecrab/state.db`, so you pick up where you left off even after restarting Python.
- **Use `stream_events` over `stream` when you need tool visibility**: It surfaces `ToolCallEvent` and `ToolResultEvent` so you can log or display exactly what the agent is doing.
- **Gate file tools tightly in production**: Pass `toolsets=['web']` to limit the agent to web-only tools when running in an untrusted pipeline.
- **Set short `max_iterations` for unit tests**: `max_iterations=3` makes tests fast and deterministic by forcing early completion.
- **Errors are typed**: Catch `ToolError` (security rejection, tool failure) and `ProviderError` (API quota, model error) separately for clean error handling.

---

## FAQ

**Does the SDK require a running EdgeCrab server?**
No. `edgecrab-sdk` calls the LLM provider directly using the same logic as the CLI. No local server is needed.

**Can I use the SDK with a self-hosted gateway?**
Yes. Pass `base_url="https://your-gateway.example.com/v1"` to `Agent()` and it will send all requests there.

**Does the SDK respect `~/.edgecrab/config.yaml`?**
Yes. The `.yaml` config is loaded automatically unless overridden by constructor arguments.

**Can I use this with Jupyter notebooks?**
Yes. Use the `AsyncAgent` with `await` in a notebook cell. The sync `Agent` also works but may block the event loop in async contexts.

**What Python versions are supported?**
Python 3.10+. Tested on 3.10, 3.11, 3.12, and 3.13.

---

## See Also

- [Node.js SDK](/integrations/node-sdk/) — TypeScript-first equivalent
- [ACP / VS Code](/integrations/acp/) — use EdgeCrab as a VS Code Copilot agent
- [Self-Hosting](/guides/self-hosting/) — run a shared gateway for your team
