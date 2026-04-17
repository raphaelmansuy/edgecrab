# EdgeCrab SDK Examples

This directory contains example code for both the **Rust** and **Python** SDKs.

## Rust Examples

| Example | Description |
|---------|-------------|
| [hello.rs](rust/hello.rs) | Minimal "Hello World" — send one message |
| [streaming.rs](rust/streaming.rs) | Stream tokens in real-time |
| [custom_tool.rs](rust/custom_tool.rs) | Register a custom tool with `#[edgecrab_tool]` |
| [builder.rs](rust/builder.rs) | Configure the agent with the builder API |

### Running Rust Examples

```bash
# From the repository root
cargo run --example hello
cargo run --example streaming
cargo run --example custom_tool
cargo run --example builder
```

> **Note:** Examples require a valid LLM provider API key configured in
> `~/.edgecrab/config.yaml`.

## Python Examples

| Example | Description |
|---------|-------------|
| [hello.py](python/hello.py) | Minimal "Hello World" — sync chat |
| [streaming.py](python/streaming.py) | Stream events from the agent |
| [full_conversation.py](python/full_conversation.py) | Get detailed conversation results |
| [sessions.py](python/sessions.py) | List and search sessions, inspect tools |

### Running Python Examples

```bash
# First, build the native module
cd sdks/python
maturin develop

# Then run examples
cd ../../sdk-examples/python
python hello.py
python streaming.py
python full_conversation.py
python sessions.py
```
