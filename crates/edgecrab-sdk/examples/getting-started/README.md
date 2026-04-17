# Rust SDK — Getting Started

## WHY

This path is for the fastest possible first success.

If you are new to the Rust SDK, start here before exploring the bigger workflows.

## WHAT TO RUN

1. [../basic_usage.rs](../basic_usage.rs) — smallest useful agent example
2. [../full_conversation.rs](../full_conversation.rs) — richer response metadata and cost information
3. [../memory_usage.rs](../memory_usage.rs) — persistence and recall

## HOW

From the repository root:

```bash
ollama pull gemma4:latest
cargo run --example basic_usage -p edgecrab-sdk
```

When that feels comfortable, move on to the business demos in [../business/README.md](../business/README.md).
