---
title: Python SDK
description: Complete guide to the EdgeCrab Python SDK — Agent class, streaming, sessions, and configuration.
sidebar:
  order: 3
---

The Python SDK provides native bindings to the EdgeCrab agent runtime via
PyO3. It's not a REST wrapper — it runs the same Rust engine in-process for
maximum performance.

## Installation

```bash
pip install edgecrab
```

Or build from source:

```bash
cd sdks/python
maturin develop  # development mode
maturin build    # build wheel
```

**Requirements:** Python 3.10+

## Creating an Agent

```python
from edgecrab import Agent

# Minimal
agent = Agent("openai/gpt-4o")

# With options
agent = Agent(
    "copilot/gpt-5-mini",
    max_iterations=20,
    temperature=0.7,
    streaming=True,
    quiet_mode=True,
    session_id="my-session",
)
```

### Constructor Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `model` | `str` | required | Model string (e.g., `"openai/gpt-4o"`) |
| `max_iterations` | `int \| None` | `25` | Maximum tool-use iterations |
| `temperature` | `float \| None` | `0.7` | Sampling temperature |
| `streaming` | `bool \| None` | `False` | Enable token streaming |
| `session_id` | `str \| None` | auto | Resume a specific session |
| `quiet_mode` | `bool \| None` | `False` | Suppress internal logging |
| `instructions` | `str \| None` | `None` | Custom system prompt appended to the base prompt |
| `toolsets` | `list[str] \| None` | all | Enable only these toolsets |
| `disabled_toolsets` | `list[str] \| None` | `None` | Disable specific toolsets |
| `disabled_tools` | `list[str] \| None` | `None` | Disable specific tools by name |

```python
# Example with advanced options
agent = Agent(
    "openai/gpt-4o",
    instructions="Always respond in French.",
    toolsets=["code", "file"],
    disabled_tools=["terminal_run"],
    max_iterations=30,
)
```

## Simple Chat (Sync)

```python
reply = agent.chat_sync("What is EdgeCrab?")
print(reply)
```

## Full Conversation

```python
result = agent.run_sync("Analyze this codebase")

print(f"Reply: {result.response}")
print(f"Model: {result.model}")
print(f"API calls: {result.api_calls}")
print(f"Tokens: {result.input_tokens} in / {result.output_tokens} out")
print(f"Cost: ${result.total_cost:.4f}")
print(f"Interrupted: {result.interrupted}")
print(f"Budget exhausted: {result.budget_exhausted}")
```

### ConversationResult Properties

| Property | Type | Description |
|----------|------|-------------|
| `response` | `str` | The agent's final text answer |
| `session_id` | `str` | Session ID used |
| `model` | `str` | Model used |
| `api_calls` | `int` | Number of LLM API calls |
| `interrupted` | `bool` | Was the agent interrupted? |
| `budget_exhausted` | `bool` | Hit iteration limit? |
| `input_tokens` | `int` | Total input tokens |
| `output_tokens` | `int` | Total output tokens |
| `total_cost` | `float` | Total cost in USD |

### Run Conversation with System Prompt

```python
result = agent.run_conversation(
    "Explain this code",
    system="You are a senior code reviewer.",
    history=["What does this project do?"],
)
```

## Streaming

```python
events = agent.stream_sync("Write a haiku about Rust")

for event in events:
    if event.event_type == "token":
        print(event.data, end="", flush=True)
    elif event.event_type == "done":
        print("\n--- Done ---")
    elif event.event_type == "tool_exec":
        print(f"[tool: {event.data}]")
    elif event.event_type == "tool_result":
        print(f"[result: {event.data}]")
```

### StreamEvent Properties

| Property | Type | Description |
|----------|------|-------------|
| `event_type` | `str` | Event type: `"token"`, `"reasoning"`, `"tool_exec"`, `"tool_result"`, `"done"`, `"error"`, `"info"` |
| `data` | `str` | Event payload (token text, tool name, error message, etc.) |

## Session Management

```python
# Current session ID
print(agent.session_id)  # e.g., "ses_abc123" or None

# Conversation history
for msg in agent.history:
    print(f"[{msg['role']}] {msg.get('content', '')[:80]}")

# Start a fresh session
agent.new_session()

# Fork agent for parallel work
forked = agent.fork()
result = forked.run_sync("Analyze tests independently")

# Export full session (snapshot + messages)
export = agent.export()
print(f"Model: {export['model']}")
print(f"Messages: {len(export['messages'])}")
print(f"API calls: {export['api_call_count']}")

# List recent sessions
sessions = agent.list_sessions(limit=10)
for s in sessions:
    print(f"{s.session_id[:8]}... | {s.model} | {s.message_count} messages")

# Search sessions
hits = agent.search_sessions("kubernetes", limit=5)
for h in hits:
    print(f"score={h.score:.2f} | {h.snippet[:80]}")
```

### SessionSummary Properties

| Property | Type | Description |
|----------|------|-------------|
| `session_id` | `str` | Session UUID |
| `source` | `str` | Session source |
| `model` | `str \| None` | Model used |
| `started_at` | `float` | Unix timestamp |
| `message_count` | `int` | Number of messages |
| `title` | `str \| None` | Session title |

## Agent Control

```python
# Get the current model
print(agent.model)  # property

# List tool names
tools = agent.tool_names()

# Toolset summary
for name, count in agent.toolset_summary():
    print(f"  {name}: {count} tools")

# Change reasoning effort
agent.set_reasoning_effort("high")
agent.set_reasoning_effort(None)  # reset

# Toggle streaming
agent.set_streaming(True)

# Interrupt (from another thread)
agent.interrupt()
print(agent.is_cancelled())
```

## Configuration

```python
from edgecrab import Config, edgecrab_home, ensure_edgecrab_home

# EdgeCrab home directory
print(edgecrab_home())          # ~/.edgecrab
print(ensure_edgecrab_home())   # creates if missing

# Load configuration
config = Config.load()                    # from default location
config = Config.load_from("/path/to/config.yaml")
config = Config.default_config()          # built-in defaults

print(config.default_model)  # e.g., "openai/gpt-4o"
```

## Model Catalog

```python
from edgecrab import ModelCatalog

catalog = ModelCatalog()

# List providers
providers = catalog.provider_ids()
print(providers)  # ["anthropic", "openai", "google", ...]

# List models for a provider
models = catalog.models_for_provider("openai")
for model_id, display_name in models:
    print(f"  {model_id}: {display_name}")

# Get context window
window = catalog.context_window("openai", "gpt-4o")
print(f"Context: {window} tokens")
```

## Message and Role Types

```python
from edgecrab import Message, Role

# Create messages
msg = Message.user("Hello!")
sys = Message.system("You are a helpful assistant.")
asst = Message.assistant("Hi there!")
tool = Message.tool_result("call_123", "get_weather", '{"temp": 22}')

# Access properties
print(msg.role)     # Role.USER
print(msg.content)  # "Hello!"
print(msg.text)     # "Hello!" (extracts text from multimodal content)
print(msg.to_dict())  # {"role": "user", "content": "Hello!"}

# Role enum
print(Role.SYSTEM)     # "system"
print(Role.USER)       # "user"
print(Role.ASSISTANT)  # "assistant"
print(Role.TOOL)       # "tool"
```

## Custom Tools with @Tool Decorator

```python
from edgecrab import Agent, Tool

@Tool("get_weather", description="Get weather for a city")
def get_weather(city: str, units: str = "celsius") -> dict:
    """Get current weather for a city.

    Args:
        city: City name
        units: Temperature units (celsius or fahrenheit)
    """
    return {"city": city, "temp": 22, "units": units}

# Inspect the tool
print(get_weather.name)       # "get_weather"
print(get_weather.schema)     # ToolSchema with JSON Schema
print(get_weather.to_dict())  # Full serialized definition

# Execute directly
result = get_weather.execute({"city": "Paris"})
print(result)  # {"city": "Paris", "temp": 22, "units": "celsius"}
```

### @Tool Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `name` | `str` | required | Tool name (used by the LLM) |
| `description` | `str \| None` | from docstring | Tool description |
| `toolset` | `str` | `"custom"` | Toolset category |
| `emoji` | `str` | `"🔧"` | Display emoji |

## Error Handling

All SDK methods raise standard Python exceptions:

| Exception | When |
|-----------|------|
| `ValueError` | Configuration errors, serialization failures |
| `RuntimeError` | Agent errors, tool errors, uninitialized components |

```python
try:
    reply = agent.chat_sync("hello")
except ValueError as e:
    print(f"Config/input error: {e}")
except RuntimeError as e:
    print(f"Runtime error: {e}")
```

## Type Stubs

The Python SDK ships with PEP 561 type stubs (`py.typed` + `_native.pyi`),
so you get full IDE autocompletion and type checking with mypy/pyright.
