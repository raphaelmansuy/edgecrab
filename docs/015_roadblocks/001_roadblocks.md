# 015.001 — Implementation Roadblocks (Python Concepts → Rust)

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 014 Implementation Plan](../014_implementation_plan/001_implementation_plan.md) | [→ 013 Library Selection](../013_library_selection/001_library_selection.md)
> **Scope**: Challenges when implementing Python-native agent patterns in Rust

---

## 1. Async Ecosystem Mismatch

### Problem
hermes-agent uses Python's `asyncio` with liberal `asyncio.run()` / `loop.run_until_complete()` bridge calls from sync code. Rust's async model is fundamentally different: no implicit event loop, all async code must run inside a runtime, and `Send + Sync` bounds are enforced at compile time.

### Specific Risks
- **Sync↔Async bridging**: hermes uses `nest_asyncio` to patch the event loop and calls `asyncio.run()` from within running loops. Rust's tokio forbids blocking inside async contexts. All sync→async boundaries must use `tokio::task::spawn_blocking`.
- **Colored functions**: In hermes, many methods are sync and just call `asyncio.run()`. In Rust, once a function is async, all callers must be async. This forces the entire call stack to be async.
- **Cancellation semantics**: Python's `asyncio.Task.cancel()` raises `CancelledError` that can be caught. Rust's `tokio::select!` drops the losing future — must use `CancellationToken` for cooperative cancellation.
- **Timeouts**: Python uses `asyncio.wait_for()`. Rust uses `tokio::time::timeout()` — similar API but lifetime considerations when borrowing across await points.

### Mitigation
- Use `tokio` exclusively — no mixing runtimes
- Design `Agent::run_conversation()` as fully async from the start
- Use `CancellationToken` pattern (documented in [003.002](../003_agent_core/002_conversation_loop.md))
- Add `#[tokio::main]` only at binary entry points

---

## 2. Dynamic Typing → Static Typing

### Problem
hermes-agent uses Python's dynamic typing extensively: `kwargs`, late-bound attributes, monkey-patching, duck typing, optional fields that are `None` by default.

### Specific Risks
- **AIAgent.__init__** takes 50+ parameters with defaults — Rust needs a builder pattern or config struct
- **Tool handlers** accept `dict` arguments — Rust needs typed parameter structs
- **Message content** can be `str | list[dict]` — Rust needs an enum (`Content::Text(String) | Content::Parts(Vec<ContentPart>)`)
- **API responses** vary by provider — Rust needs careful deserialization with `#[serde(untagged)]` or `#[serde(flatten)]`
- **Global mutable state** (e.g., `registry = ToolRegistry()` singleton) — Rust needs `Arc<RwLock<>>` or `once_cell::sync::Lazy`

### Mitigation
- `AgentBuilder` pattern replaces 50-param constructor
- Typed enums for all union types with `serde(untagged)`
- `Option<T>` for nullable fields with `#[serde(default)]`
- Avoid global mutable state; pass state through function parameters or use `Arc`

---

## 3. Plugin & Extension Ecosystem

### Problem
hermes-agent has a rich skill system (Python scripts loaded from disk) and MCP integration (JSON-RPC subprocesses). Python's dynamic loading makes this trivial; Rust requires careful planning.

### Specific Risks
- **Skills are Python/Markdown files** loaded at runtime — cannot be compiled to Rust
- **MCP tools** are external processes — this actually ports cleanly (JSON-RPC over stdio)
- **Dynamic dispatch** for tool handlers in Python is just `getattr()` — Rust needs trait objects or enum dispatch
- **Community skills** assume Python runtime — must either embed Python or redefine skill format

### Mitigation
- Skills remain as Markdown files (parsed, injected into system prompt — no code execution needed)
- MCP tools work unchanged (subprocess + JSON-RPC)
- Compiled tools use `inventory` crate for registration
- Optional: embed `pyo3` for Python skill/plugin compat (feature-gated, Phase 4)
- Define new Rust-native plugin API (`libloading` based) for dynamic `.so`/`.dylib` loading

---

## 4. Browser Automation Gap

### Problem
hermes-agent uses Playwright (Python) for browser automation. There is no Playwright equivalent in Rust.

### Specific Risks
- Playwright supports Chromium, Firefox, WebKit with a unified API
- `chromiumoxide` (our chosen crate, v0.9, async/tokio-native) only supports Chromium/CDP
- No Rust crate matches Playwright's auto-wait, network interception, and trace features
- Web research tools depend heavily on browser rendering

### Mitigation
- Use `chromiumoxide` 0.9 (async, CDP-based, actively maintained) for Chromium automation
- For content extraction, prefer HTTP-based tools (`reqwest` + `scraper` for HTML parsing)
- Feature-gate browser automation (`browser` feature)
- Consider MCP bridge to Playwright for full compat (subprocess communication)
- `readability` crate for article extraction (replaces `trafilatura`)

---

## 5. Rich TUI Feature Gap

### Problem
hermes-agent uses Python's `rich` library (Markdown rendering, syntax highlighting, spinners, progress bars, panels, tables) and `prompt_toolkit` (key bindings, completion, history). Rust's `ratatui` is powerful but lower-level.

### Specific Risks
- **Markdown rendering**: `rich` renders Markdown inline. `ratatui` needs manual parsing + widget mapping
- **Syntax highlighting**: `rich` uses Pygments. Rust has `syntect` but integration with ratatui requires custom widgets
- **Key bindings**: `prompt_toolkit` has vi/emacs modes. Must implement from scratch or use `crossterm` raw events
- **Progress bars**: `rich` has rich progress bars. `ratatui` has `Gauge` widget (simpler)
- **Inline code blocks**: `rich` auto-detects code blocks in LLM output. Must parse and highlight manually

### Mitigation
- Use `syntect` for syntax highlighting, render to ratatui styled text via `ansi-to-tui` crate
- Use `pulldown-cmark` to parse Markdown blocks, map to ratatui widgets
- Implement vi-style key bindings with crossterm `KeyEvent` matching
- Build reusable TUI widgets: `MarkdownView`, `CodeBlock`, `ToolProgress`, `StreamingOutput`
- **Advantage**: ratatui gives pixel-level control that `rich` cannot — opportunity to surpass hermes UX

---

## 6. Serialization Edge Cases

### Problem
LLM APIs return inconsistent JSON. hermes-agent handles this with Python's permissive JSON parsing and liberal `try/except`.

### Specific Risks
- **Trailing commas** in JSON (some providers): serde_json strict-rejects
- **NaN/Infinity** in usage stats: serde_json rejects by default
- **Missing fields** that should be null: requires `#[serde(default)]` everywhere
- **Extra fields** in responses: must use `#[serde(deny_unknown_fields)]` carefully
- **Tool call JSON in string field**: need to parse JSON from string, model sometimes returns malformed JSON
- **Unicode edge cases**: Python `str` auto-handles surrogates; Rust `String` is strict UTF-8

### Mitigation
- Use `serde_json::from_str()` with lenient parsing where needed
- Add `#[serde(default)]` to all optional response fields
- Add `#[serde(rename_all = "snake_case")]` consistently
- For malformed tool call JSON: regex extraction fallback, strip markdown fences
- Never use `#[serde(deny_unknown_fields)]` on API response types
- Use `String::from_utf8_lossy()` for external text

---

## 7. Error Handling Philosophy Shift

### Problem
Python uses exceptions with `try/except` broadly. hermes-agent catches exceptions at many levels and continues. Rust's `Result<T, E>` forces explicit handling at every step.

### Specific Risks
- **Hidden panics**: Python `KeyError`, `IndexError` etc. have no Rust equivalent — Rust panics on `unwrap()`
- **Exception swallowing**: hermes catches broad `except Exception` — Rust requires matching specific errors
- **Stack traces**: Python exceptions have full stack traces. Rust errors need `anyhow`/`eyre` for context
- **Retry logic**: hermes wraps API calls in `try/except` with retry loops — Rust needs explicit retry combinators
- **Partial failure**: hermes tools can fail individually while conversation continues — Rust tools must return `Result`

### Mitigation
- Use `thiserror` for library errors (typed) and `anyhow` for binary/CLI errors (contextual)
- `?` operator propagates errors — design error types to flow cleanly upward  
- `tracing::error!` at boundaries for observability
- Retry via closure-based combinators: `retry_with_backoff(|| async { ... }).await`
- Never `unwrap()` in library code — enforce via clippy lint

---

## 8. Platform-Specific Challenges

### Problem
hermes-agent runs on Linux, macOS, and Windows. Rust cross-compilation is excellent but platform-specific areas need attention.

### Specific Risks
- **Terminal**: `crossterm` handles most differences, but some escape codes vary
- **File paths**: macOS is case-insensitive by default; Linux is case-sensitive
- **Docker socket**: `/var/run/docker.sock` on Linux/Mac, named pipe on Windows
- **Clipboard**: `arboard` works cross-platform but Wayland support is newer
- **Signal handling**: `SIGINT`/`SIGTERM` on Unix; `CTRL_C_EVENT` on Windows
- **Home directory**: `~/.config/edgecrab` on Linux/Mac; `%APPDATA%` on Windows

### Mitigation
- Use `dirs` crate for platform-appropriate config/data directories
- Use `crossterm` exclusively for terminal I/O
- Conditional compilation: `#[cfg(unix)]` / `#[cfg(windows)]` where needed
- Docker socket path as config option (not hardcoded)
- Use `tokio::signal` for cross-platform signal handling
- CI matrix: test Linux (x86_64, aarch64), macOS (universal), Windows (x86_64)

---

## 9. Testing & Mocking Challenges

### Problem
Python's `unittest.mock` makes it trivial to mock any function/class. Rust requires trait-based mocking with more ceremony.

### Specific Risks
- **HTTP mocking**: Python uses `responses` or `httpretty`. Rust needs `wiremock` or `mockito`
- **LLM mocking**: Python patches `openai.ChatCompletion.create()`. Rust needs a mock `LLMProvider` impl
- **File system**: Python uses `tempfile`. Rust has `tempfile` crate (similar API)
- **Time**: Python patches `time.time()`. Rust uses `tokio::time::pause()` in tests
- **No monkey patching**: Can't replace functions at runtime — must design for testability from day one

### Mitigation
- Define traits for all external boundaries: `LLMProvider` (from edgequake-llm), `TerminalBackend`, `PlatformAdapter`
- Create mock implementations: `MockProvider`, `MockTerminal`, `MockPlatform`
- Use `wiremock` for HTTP integration tests
- Use `tokio::time::pause()` + `advance()` for time-dependent tests
- Use `tempfile::TempDir` for filesystem tests
- Design with dependency injection (constructor params, not globals)

---

## 10. Compile Time & Iteration Speed

### Problem
Python has zero compile time — edit and run. Rust compilation can take minutes for large projects.

### Specific Risks
- Full workspace clean build: may take 3-5 minutes
- Incremental build: 5-30 seconds depending on changed crate
- Feature combinations multiply compile time
- Template-heavy code (generic tools, serde derives) adds to compilation
- Developer iteration loop significantly slower

### Mitigation
- Cargo workspace: only changed crates recompile
- Heavy use of `cargo check` (faster than full build)
- `sccache` or `mold` linker for faster linking
- Feature gates limit what gets compiled during development
- `cargo-watch` for automatic rebuild on file change
- Crate boundaries chosen to minimize recompilation blast radius
- Consider `cargo-nextest` for faster test execution

---

## 11. Dependency Weight

### Problem
hermes-agent has ~50 Python dependencies (pip install). EdgeCrab will have ~35-40 Rust crate dependencies (transitive count will be 200+).

### Specific Risks
- **Compile time**: More crates = longer first build
- **Supply chain**: More transitive deps = more audit surface
- **Version conflicts**: Multiple crates depending on different versions of the same transitive dep
- **Feature bloat**: Some crates pull in large dependency trees by default

### Mitigation
- `cargo-deny` for license + advisory checks in CI
- Pin crate versions in `Cargo.lock` (always committed)
- Feature-gate heavy deps (teloxide, serenity, bollard, russh)
- Default features: disable unused features (`default-features = false`)
- Dependency count comparison already lower than Python equivalent (see [013](../013_library_selection/001_library_selection.md))

---

## 12. String Processing Performance Trap

### Problem
Python string operations are high-level and allocation-heavy but transparent. Rust string handling is faster but requires attention to avoid unnecessary allocations.

### Specific Risks
- **String concatenation in loops**: Must use `String::with_capacity()` or `push_str()`
- **Regex compilation**: Must compile once, not per-call — use `lazy_static!` or `std::sync::LazyLock`
- **UTF-8 validation**: Every `String::from()` validates UTF-8; use `from_utf8_unchecked()` only when source is trusted
- **Token counting**: hermes uses `tiktoken` (Python C extension). Rust needs `tiktoken-rs` or approximate counting

### Mitigation
- Use `Cow<'_, str>` for prompt builder sections (avoid cloning)
- Pre-compile all regex patterns at startup
- Token estimation via word count heuristic (fast) with optional `tiktoken-rs` (accurate, feature-gated)
- String builder patterns for prompt assembly

---

## 13. Deployment & Distribution

### Problem
Python deployment: `pip install` or Docker. Rust: single binary, but cross-compilation and packaging are more involved.

### Specific Risks
- **Cross-compilation**: Need toolchains for x86_64-linux, aarch64-linux, x86_64-macos, aarch64-macos, x86_64-windows
- **OpenSSL**: Links dynamically by default — prefer `rustls` for static builds
- **SQLite**: `rusqlite` can bundle SQLite (avoids system dependency) — adds compile time
- **Binary size**: Without optimization, debug builds can be 100MB+

### Mitigation
- Use `cross` for cross-compilation
- Use `rustls` instead of native TLS (zero system deps)
- Use `rusqlite` with `bundled` feature (static SQLite)
- Release profile: `opt-level = "z"`, `lto = true`, `strip = true` → ~15-25MB binary
- GitHub Actions matrix for all targets
- **Advantage**: Single binary distribution is massively simpler than Python + venv

---

## 14. Python↔Rust Interop (Optional Bridge)

### Problem
Some users may want to use existing Python skills, tools, or scripts with EdgeCrab during transition.

### Specific Risks
- PyO3 adds significant complexity and a Python runtime dependency
- Python GIL can block async Rust code
- Version compatibility (Python 3.11+ requirement)
- Build complexity (need Python dev headers)

### Mitigation
- Feature-gated: `python-compat` feature flag
- Only for plugin/skill loading, not core engine
- Use `pyo3-asyncio` for async bridging
- Document clearly as optional transition feature
- Prefer subprocess-based bridges (MCP-style) over in-process Python

---

## 15. OAuth Token Lifecycle Management

### Problem
hermes-agent v0.4.0 supports Claude Code OAuth (sk-ant-oat-* tokens) with automatic refresh.
This is a complex credential lifecycle that needs careful Rust implementation.

### Specific Risks
- **Token refresh races**: Multiple concurrent sessions may try to refresh simultaneously
- **Token expiry during request**: Mid-stream token expiry can corrupt responses
- **Credential file format**: Claude stores creds in `~/.claude/credentials.json` — format may change
- **OAuth flow**: Needs HTTP server for callback (Copilot OAuth) or file-based flow

### Mitigation
- Use `RwLock<CredentialCache>` with compare-and-swap refresh
- Pre-check expiry before API call; 30-second buffer window
- Abstract credential sources behind trait: `CredentialSource::resolve() → String`
- Monitor Claude CLI updates for credential format changes

---

## 16. Prompt Cache Preservation Across Conversations

### Problem
Anthropic prompt caching saves significant cost ($0 for cache hits vs full input pricing).
hermes v0.4.0 caches AIAgent instances in gateway to preserve cache state.

### Specific Risks
- **Cache invalidation**: Any system prompt change breaks the cache
- **Memory pressure**: Cached agents hold conversation history in memory
- **Session affinity**: Gateway must route returning users to the same agent instance
- **Cache TTL**: Anthropic caches expire after 5 minutes of inactivity

### Mitigation
- Freeze system prompt snapshot at session start (already designed in 007)
- Implement LRU eviction with configurable max cached agents
- Session manager maps user+platform → agent instance
- Background keep-alive pings for active sessions approaching cache TTL

---

## 17. Tool Call Parser Porting (11 Model-Specific Parsers)

### Problem
hermes-agent implements 11 model-specific tool call parsers for Phase 2 RL (VLLM ManagedServer). Each parser handles a different XML/JSON format for extracting tool calls from raw model output.

### Specific Risks
- **Parser list**: hermes, mistral, llama, qwen, qwen3_coder, deepseek_v3, deepseek_v3_1, glm45, glm47, kimi_k2, longcat
- Each parser has model-specific XML tags, JSON formats, and edge cases (partial output, malformed closing tags)
- Python uses regex + string slicing; Rust needs careful `nom` or regex-based parsing with proper UTF-8 boundary handling
- Parsers must handle streaming (partial tool calls mid-token)
- New models may require new parsers — must be extensible

### Mitigation
- Define `ToolCallParser` trait with `parse(raw: &str) → Vec<ToolCall>` method
- Implement each parser as a separate struct behind the trait
- Use `nom` for structured parsers where XML/JSON is well-defined; regex for loose formats
- Feature-gate entire parser set (`tool-call-parsers` feature for RL Phase 2)
- Add snapshot tests (`insta`) with real model output samples

---

## 18. PersistentShell File-Based IPC

### Problem
The `PersistentShell` module uses a file-based IPC protocol for maintaining long-running shell sessions across tool invocations. The protocol writes commands to a stdin file, reads output from a stdout file, and uses marker-based synchronization.

### Specific Risks
- **Race conditions**: File writes and reads must be carefully ordered (stdin flush → marker write → poll stdout)
- **Platform differences**: Temp directory paths, file permissions, line endings (LF vs CRLF on Windows)
- **Session lifecycle**: Shell processes can die silently; need heartbeat/detection + auto-restart
- **Pre-existing output**: Must drain any buffered output before issuing new command
- **Exponential polling**: Python uses sleep loops with backoff; Rust should use `tokio::time::sleep` with `inotify`/`kqueue` for efficiency

### Mitigation
- Use `tokio::fs` for non-blocking file I/O
- Use `notify` crate for file change events (replaces polling where possible)
- Implement heartbeat via periodic echo command
- Use marker UUIDs for command/response correlation
- Add `session_id` to temp file names for parallel session isolation
- Platform-specific interrupt signaling: `SIGINT` on Unix, `CTRL_C_EVENT` on Windows

---

## Risk Matrix

| # | Roadblock | Impact | Likelihood | Difficulty | Phase |
|---|-----------|--------|------------|------------|-------|
| 1 | Async mismatch | High | Certain | Medium | 1 |
| 2 | Dynamic → static types | Medium | Certain | Medium | 0-1 |
| 3 | Plugin ecosystem | Medium | High | High | 4 |
| 4 | Browser automation gap | High | Certain | High | 2 |
| 5 | TUI feature gap | Medium | High | Medium | 2 |
| 6 | Serialization edge cases | Medium | High | Low | 0-1 |
| 7 | Error handling shift | Low | Certain | Low | 0 |
| 8 | Platform-specific issues | Low | Medium | Low | 5 |
| 9 | Testing & mocking | Medium | High | Medium | 1-5 |
| 10 | Compile time | Low | Certain | Low | 0 |
| 11 | Dependency weight | Low | Medium | Low | 0 |
| 12 | String processing | Low | Medium | Low | 1 |
| 13 | Deployment | Low | Low | Low | 5 |
| 14 | Python interop | Low | Low | High | 4 |
| 15 | OAuth token lifecycle | Medium | High | Medium | 1 |
| 16 | Prompt cache preservation | Medium | High | Medium | 3 |
| 17 | Tool call parser porting | Medium | High | Medium | 4 |
| 18 | PersistentShell IPC | Medium | High | Medium | 4 |
| 16 | Prompt cache preservation | Medium | High | Medium | 3 |
| 17 | Tool call parser porting | Medium | Certain | Medium | 4 |
| 18 | PersistentShell file IPC | Medium | High | Medium | 4 |

**Legend**: Impact = effect on project success. Likelihood = chance of occurrence. Difficulty = effort to mitigate.

---

## Key Insight: Where Rust Wins

1. **10-50x lower memory** — no GC overhead, no object headers, no dict per instance
2. **Single binary** — no venv, no pip, no Python version management
3. **Compile-time safety** — no `AttributeError` at runtime, no `None` dereferences
4. **True parallelism** — no GIL, tool dispatch and gateway sessions run truly concurrent
5. **Predictable latency** — no GC pauses, no JIT warmup
6. **Feature gates** — compile only what you need (gateway without Docker, CLI without browser)
7. **Cross-compilation** — one CI, all platforms
8. **Supply chain** — `cargo-deny` + `cargo-audit` catch issues before release
