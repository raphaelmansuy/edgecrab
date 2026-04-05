# Dependency Choices

Verified against `Cargo.toml` and workspace crate manifests.

This page only records dependencies that materially shape the architecture.

## Runtime and async

- `tokio`: async runtime across CLI, gateway, tools, and ACP
- `tokio-util`: cancellation and runtime helpers
- `futures`: stream and async utilities

## Serialization and config

- `serde`
- `serde_json`
- `serde_yml`

## Agent and provider layer

- `edgequake-llm`: provider abstraction used by the runtime

## Persistence and search

- `rusqlite` with bundled SQLite and FTS support

## CLI and TUI

- `clap`
- `ratatui`
- `crossterm`
- `tui-textarea`

## HTTP and servers

- `reqwest`
- `axum`
- `tokio-tungstenite`

## Tooling and execution

- `inventory` for tool registration
- `dashmap` for concurrent registries
- `bollard` for Docker backends
- `openssh` for SSH backends on Unix

## Security and text handling

- `regex`
- `aho-corasick`
- `unicode-normalization`
- `strip-ansi-escapes`
- `secrecy`

## Why this page is short

The old version mixed current dependencies with aspirational ones. This version only lists crates that clearly shape the code that exists today.
