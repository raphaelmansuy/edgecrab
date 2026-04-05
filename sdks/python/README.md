# edgecrab-sdk

[![PyPI](https://img.shields.io/pypi/v/edgecrab-sdk.svg)](https://pypi.org/project/edgecrab-sdk/)
[![Python](https://img.shields.io/badge/python-3.10%2B-blue.svg)](https://python.org)

Python SDK for **EdgeCrab** — a Rust-native autonomous coding agent.

## Install

```bash
pip install edgecrab-sdk
```

## Quick Start

```python
from edgecrab import EdgeCrabClient

# Connect to a running EdgeCrab API server
client = EdgeCrabClient(
    base_url="http://127.0.0.1:8642",
    api_key="your-api-key",  # optional
)

# Simple chat
reply = client.chat("Explain Rust ownership in 3 sentences")
print(reply)

# With system prompt
reply = client.chat(
    "Refactor this function",
    system="You are a senior Rust developer",
    model="anthropic/claude-sonnet-4-20250514",
)
```

## Async

```python
import asyncio
from edgecrab import AsyncEdgeCrabClient

async def main():
    async with AsyncEdgeCrabClient() as client:
        reply = await client.chat("Hello!")
        print(reply)

asyncio.run(main())
```

## Agent API (recommended)

```python
from edgecrab import Agent

agent = Agent(
    model="anthropic/claude-sonnet-4-20250514",
    system_prompt="You are a helpful coding assistant",
)

# Chat with automatic conversation history
reply = agent.chat("Explain Rust ownership")
print(reply)

# Continue the conversation
follow_up = agent.chat("Give me an example")
print(follow_up)

# Full run with result metadata
result = agent.run("Refactor this function")
print(result.response)
print(f"Turns: {result.turns_used}, Tokens: {result.usage.total_tokens}")
```

## Streaming

```python
from edgecrab import EdgeCrabClient, ChatMessage

with EdgeCrabClient() as client:
    messages = [ChatMessage(role="user", content="Write a haiku about Rust")]
    for chunk in client.stream_completion(messages=messages):
        for choice in chunk.choices:
            if choice.delta.content:
                print(choice.delta.content, end="", flush=True)
    print()
```

## CLI

```bash
edgecrab chat "What is the meaning of life?"
edgecrab chat --model gpt-4 --system "Be concise" "Explain monads"
edgecrab chat --stream "Tell me a story"
edgecrab models
edgecrab health
```

### Environment Variables

| Variable | Description |
|---|---|
| `EDGECRAB_BASE_URL` | API server URL (default: `http://127.0.0.1:8642`) |
| `EDGECRAB_API_KEY` | Bearer token for authentication |

## API Reference

### `EdgeCrabClient`

| Method | Description |
|---|---|
| `chat(message, *, model, system, temperature, max_tokens)` | Simple chat — returns string |
| `create_completion(messages, *, model, temperature, max_tokens, tools)` | Full completion — returns `ChatCompletionResponse` |
| `stream_completion(messages, *, model, temperature, max_tokens, tools)` | Streaming — yields `StreamChunk` |
| `list_models()` | List available models |
| `health()` | Health check |

### `AsyncEdgeCrabClient`

Same API as `EdgeCrabClient`, but all methods are `async`.

### `Agent` / `AsyncAgent`

| Method | Description |
|---|---|
| `chat(message)` | Send message, return reply. Maintains history. |
| `run(message, *, max_turns)` | Full conversation run — returns `AgentResult` |
| `add_message(role, content)` | Inject a message into history |
| `reset()` | Clear history, start new session |
| `get_messages()` | Get conversation history |
| `get_turn_count()` | Number of completed turns |
| `get_usage()` | Accumulated token usage |
| `list_models()` | List available models |
| `health()` | Check server health |

## Links

- [GitHub](https://github.com/raphaelmansuy/edgecrab)
- [Node.js SDK](https://github.com/raphaelmansuy/edgecrab/tree/main/sdks/node)
- [EdgeCrab Documentation](https://github.com/raphaelmansuy/edgecrab/tree/main/docs)
