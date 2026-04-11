# 🦀 Library Selection

> **WHY**: Every dependency is a trust relationship, a compile-time cost, and a future maintenance burden. The libraries listed here were chosen because they provide capabilities that would take thousands of lines of correct Rust to replicate — and because they are stable, widely-used, and actively maintained.

**Source**: workspace `Cargo.toml` and individual crate manifests.

This page only covers libraries that materially shape the architecture. Transitive dependencies and minor utilities are omitted.

---

## Runtime and Async

### `tokio` — The Async Foundation

```toml
tokio = { version = "1", features = ["full"] }
```

**Why**: The only production-grade async runtime for Rust with work-stealing, `io-uring` support, and a rich ecosystem. EdgeCrab uses the multi-thread runtime everywhere: CLI, gateway servers, tool execution, MCP clients, cron scheduler.

**Shapes the architecture**: every `async fn` in EdgeCrab compiles against Tokio's executor model. The `#[tokio::main]` entry point in `edgecrab-cli` boots the multi-thread scheduler.

- Reference: [tokio.rs](https://tokio.rs) | [docs.rs/tokio](https://docs.rs/tokio)

### `tokio-util` — Cancellation and Helpers

```toml
tokio-util = { version = "0.7", features = ["rt"] }
```

`CancellationToken` from `tokio-util` is the cooperative cancellation primitive used throughout the agent loop, gateway handlers, and tool execution. See [concurrency model](../002_architecture/003_concurrency_model.md).

### `futures` — Stream and Combinator Utilities

Used for streaming LLM output (`Stream<Item = StreamEvent>`), async iteration, and combinator chains. The `futures::stream::StreamExt` trait is ubiquitous in the gateway and tool layers.

---

## Serialisation and Configuration

### `serde` + `serde_json` + `serde_yml`

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yml = "0.0.12"
```

**Why**: `serde` is the de-facto Rust serialisation standard. Every public type in `edgecrab-types` derives `Serialize`/`Deserialize`. `serde_json` handles LLM API payloads and tool arguments. `serde_yml` handles `config.yaml` and skill frontmatter.

- Reference: [serde.rs](https://serde.rs)

---

## Agent and Provider Layer

### `edgequake-llm` — Provider Abstraction

The internal provider crate wraps OpenAI ChatCompletions, Anthropic Messages, and the Codex Responses API behind a single async interface. EdgeCrab calls `edgequake-llm`; `edgequake-llm` handles auth, retries, API-mode translation, and streaming. This is what makes multi-provider support possible without scattering `if anthropic { … } else { … }` branches through the core runtime.

---

## Persistence and Search

### `rusqlite` — Embedded SQLite

```toml
rusqlite = { version = "0.31", features = ["bundled", "bundled-full"] }
```

**Why**: Zero external dependencies. The `bundled` feature compiles SQLite directly into the binary — no `libsqlite3` on the host required. `bundled-full` adds FTS5 support for full-text search over conversation history.

Shapes the architecture: the `edgecrab-state` crate owns all database access. WAL mode + jitter-retry writes make it safe for concurrent CLI and gateway usage on the same file.

- Reference: [docs.rs/rusqlite](https://docs.rs/rusqlite)

---

## CLI and TUI

### `clap` — Argument Parsing

```toml
clap = { version = "4", features = ["derive"] }
```

EdgeCrab's entire subcommand tree (`run`, `chat`, `gateway`, `version`, `skills`, `memory`…) is declared with `clap`'s derive macros. Completions, help text, and validation come for free.

- Reference: [docs.rs/clap](https://docs.rs/clap)

### `ratatui` + `crossterm` — Terminal UI

```toml
ratatui = "0.27"
crossterm = "0.27"
```

The interactive TUI mode (chat pane, tool output pane, status bar) is built on `ratatui`. `crossterm` provides the cross-platform terminal backend (raw mode, event loop, ANSI sequences).

- Reference: [ratatui.rs](https://ratatui.rs)

### `tui-textarea` — Multi-line Input

Provides the multi-line text input widget used in TUI chat mode. Handles Unicode, multi-byte characters, and vim-style keybindings.

---

## HTTP and Servers

### `reqwest` — HTTP Client

```toml
reqwest = { version = "0.12", features = ["json", "stream"] }
```

Used for all outbound HTTP: LLM provider calls (via `edgequake-llm`), URL-fetch tool, web search tool. The `stream` feature enables async streaming of large responses.

### `axum` — HTTP Server

```toml
axum = "0.7"
```

Powers the ACP server (`edgecrab-acp`) and the webhook/API-server gateway adapter. Chosen for its Tokio-native design, tower middleware compatibility, and ergonomic router API.

- Reference: [docs.rs/axum](https://docs.rs/axum)

### `tokio-tungstenite` — WebSocket

WebSocket support for gateway adapters (Discord gateway, Slack RTM, Matrix C-S API) and the ACP streaming protocol.

---

## Tool Registration

### `inventory` — Compile-Time Plugin Registration

```toml
inventory = "0.3"
```

**Why this is architecturally significant**: `inventory` uses linker sections to collect `inventory::submit!` items across all crates into a single global registry — without any central list. Each tool registers itself:

```rust
inventory::submit! { &ReadFileTool as &dyn ToolHandler }
```

At startup, `ToolRegistry::collect()` iterates all submitted items. Adding a new tool requires zero changes outside the tool's own file. This is how EdgeCrab reaches 91 core tools without a monolithic dispatch table.

- Reference: [docs.rs/inventory](https://docs.rs/inventory) | [dtolnay/inventory](https://github.com/dtolnay/inventory)

---

## Concurrency Utilities

### `dashmap` — Concurrent HashMap

```toml
dashmap = "6"
```

`DashMap` provides a sharded concurrent `HashMap` with fine-grained locking. Used for the process table (running tool subprocesses), MCP client registry, and other hot-path concurrent maps. Eliminates `Mutex<HashMap<…>>` anti-patterns.

- Reference: [docs.rs/dashmap](https://docs.rs/dashmap)

---

## Execution Backends

### `bollard` — Docker API Client

```toml
bollard = "0.17"
```

The Docker execution backend communicates with the Docker daemon via `bollard`. Used to create ephemeral containers for tool execution, mount workspaces, and stream stdout/stderr back to the agent.

- Reference: [docs.rs/bollard](https://docs.rs/bollard)

### `openssh` — SSH Client (Unix only)

```toml
[target.'cfg(unix)'.dependencies]
openssh = "0.10"
```

The SSH execution backend uses `openssh` to forward tool execution to remote machines. Gated to `cfg(unix)` — not available on Windows builds.

---

## Security and Text Handling

### `regex` + `aho-corasick` — Pattern Matching

```toml
regex = "1"
aho-corasick = "1"
```

`aho-corasick` is the fast multi-pattern engine in `CommandScanner` — O(n) on input length regardless of pattern count. `regex` handles context-sensitive secondary scans and the redaction pattern matching. Both are used in `edgecrab-security`.

- Reference: [docs.rs/aho-corasick](https://docs.rs/aho-corasick) | [Aho-Corasick algorithm](https://en.wikipedia.org/wiki/Aho%E2%80%93Corasick_algorithm)

### `unicode-normalization` — NFC Normalisation

Required by the injection detection module to canonicalise Unicode before pattern matching. Without NFC normalisation, homoglyph and decomposed-character injection attacks bypass string equality checks.

### `strip-ansi-escapes` — Clean Terminal Output

Strips ANSI colour/formatting escape sequences from shell command output before it is stored or displayed in contexts that don't support colour (logs, non-TUI gateway adapters).

### `secrecy` — Secret Zeroisation

```toml
secrecy = "0.8"
```

Wraps sensitive values (`Secret<String>`) with a `Drop` implementation that zeroes the memory on deallocation. Used for API keys and credentials held in memory during a session.

- Reference: [docs.rs/secrecy](https://docs.rs/secrecy)

---

## Library Selection Principles

| Principle | Application |
|---|---|
| **Bundled over system** | `rusqlite bundled` — no host library required |
| **Tokio-native** | `axum`, `reqwest`, `tokio-tungstenite` — no thread-blocking I/O |
| **Zero-cost compile-time** | `inventory`, `serde derive` — no runtime reflection |
| **cfg-gating for platform libs** | `openssh` only on Unix — clean Windows builds |
| **Security-conscious** | `secrecy` for credentials, `aho-corasick` for fast pattern matching |

---

## Tips

- **Don't add a crate for one function** — if you need a single algorithm, implement it. `inventory` and `dashmap` are load-bearing; a JSON pretty-printer is not.
- **Check `cfg(unix)` before adding OS-specific deps** — `bollard` works on Linux/macOS/Windows (Docker for Windows); `openssh` does not. Follow the existing pattern.
- **`serde_yml` not `serde_yaml`** — the workspace uses the `serde_yml` fork (maintained, no `unsafe` YAML parser). Don't introduce the old `serde_yaml` crate.

---

## Cross-References

- `inventory` registration detail → [`002_architecture/002_crate_dependency_graph.md`](../002_architecture/002_crate_dependency_graph.md)
- `DashMap` in concurrency model → [`002_architecture/003_concurrency_model.md`](../002_architecture/003_concurrency_model.md)
- `CancellationToken` usage → [`002_architecture/003_concurrency_model.md`](../002_architecture/003_concurrency_model.md)
- `rusqlite` WAL detail → [`009_config_state/002_session_storage.md`](../009_config_state/002_session_storage.md)
- Security primitives (`aho-corasick`, `regex`) → [`011_security/001_security.md`](../011_security/001_security.md)
