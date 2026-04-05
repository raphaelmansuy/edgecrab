# 013.001 — Library Selection

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 002.002 Crate Dependency Graph](../002_architecture/002_crate_dependency_graph.md)

## 1. Selection Criteria

Each crate was evaluated against:
1. **Maturity** — stable API, 1.0+ preferred, active maintenance
2. **Ecosystem fit** — Tokio-native, serde-compatible
3. **Downloads** — proxy for community trust (crates.io stats)
4. **Minimal deps** — avoid dependency bloat
5. **Feature gates** — optional compilation for binary size

## 2. Core Runtime & Async

| Crate | Version | Purpose | Rationale | Alternative Considered |
|-------|---------|---------|-----------|----------------------|
| `tokio` | 1.x | Async runtime | De facto standard, multi-threaded, mature | `async-std` (smaller ecosystem) |
| `tokio-util` | 0.7 | Codecs, compat layers | Complements tokio | — |
| `futures` | 0.3 | Future combinators | Standard futures utilities | — |

## 3. LLM Integration

| Crate | Version | Purpose | Rationale |
|-------|---------|---------|-----------|
| `edgequake-llm` | 0.3.x | LLM provider abstraction | **Required** — 13 native providers, trait-based, caching, rate limiting, cost tracking, OTel, BM25/RRF reranking, embeddings |

## 4. Serialization & Data

| Crate | Version | Purpose | Rationale | Alternative Considered |
|-------|---------|---------|-----------|----------------------|
| `serde` | 1.x | Serialization framework | Universal standard | — |
| `serde_json` | 1.x | JSON parsing | De facto standard | `simd-json` (faster but unsafe-heavy) |
| `serde_yml` | 0.0.12 | YAML config parsing | Fork of deprecated `serde_yaml`; config files are YAML | `yaml-rust2` (lower-level), `serde_yaml` (deprecated, unmaintained) |
| `toml` | 0.8 | TOML parsing (Cargo.toml) | For workspace config | — |

## 5. HTTP & Networking

| Crate | Version | Purpose | Rationale | Alternative Considered |
|-------|---------|---------|-----------|----------------------|
| `reqwest` | 0.12 | HTTP client | Tokio-native, rustls, multipart, streaming | `hyper` (too low-level) |
| `axum` | 0.7 | HTTP server (API, webhooks) | Tokio-native, tower middleware, ergonomic | `actix-web` (different runtime), `warp` (less maintained) |
| `tower` | 0.4 | Middleware for axum | Standard service middleware | — |

## 6. CLI & TUI

| Crate | Version | Purpose | Rationale | Alternative Considered |
|-------|---------|---------|-----------|----------------------|
| `clap` | 4.6 | CLI argument parsing | Derive macros, completions, 100M+ downloads | `structopt` (merged into clap) |
| `ratatui` | 0.30 | Terminal UI framework | Active, rich widgets, crossterm backend | `tui` (archived, ratatui is the fork) |
| `crossterm` | 0.28 | Terminal I/O backend | Cross-platform, raw mode, events | `termion` (Unix only) |
| `arboard` | 3.x | Clipboard access | Native OS API, no subprocess | `copypasta` (less maintained) |
| `dotenvy` | 0.15 | .env file loading | Drop-in replacement for dotenv | `dotenv` (unmaintained) |

## 7. Database

| Crate | Version | Purpose | Rationale | Alternative Considered |
|-------|---------|---------|-----------|----------------------|
| `rusqlite` | 0.39 | SQLite + FTS5 | Bundled SQLite, FTS5 support, WAL mode | `sqlx` (async but heavier, less FTS5 control) |

## 8. Error Handling

| Crate | Version | Purpose | Rationale |
|-------|---------|---------|-----------|
| `thiserror` | 2.x | Derive Error for lib crates | Zero-overhead error derivation |
| `anyhow` | 1.x | Dynamic errors for bin crates | Ergonomic context/backtrace |

## 9. Logging & Tracing

| Crate | Version | Purpose | Rationale | Alternative Considered |
|-------|---------|---------|-----------|----------------------|
| `tracing` | 0.1 | Structured logging + spans | Async-aware, OpenTelemetry compat | `log` (no spans, no async) |
| `tracing-subscriber` | 0.3 | Log output formatting | env_filter, json, pretty | — |
| `tracing-opentelemetry` | 0.22 | OTel export | Matches edgequake-llm observability | — |

## 10. Gateway Platform Adapters

| Crate | Version | Purpose | Feature Gate | Downloads |
|-------|---------|---------|-------------|-----------|
| `teloxide` | 0.17 | Telegram bot API | `telegram` | 966K |
| `serenity` | 0.12.5 | Discord bot API | `discord` | 4.5M |
| `slack-morphism` | 2.x | Slack API | `slack` | ~50K |
| `matrix-sdk` | 0.7 | Matrix protocol | `matrix` | ~200K |
| `lettre` | 0.11 | SMTP email sending | `email` | 10M+ |
| `imap` | 3.x | IMAP email reading | `email` | ~500K |

## 11. Terminal Backends

| Crate | Version | Purpose | Feature Gate | Downloads |
|-------|---------|---------|-------------|-----------|
| `bollard` | 0.20.2 | Docker API (async) | `docker-backend` | 25M |
| `russh` | 0.46 | SSH client | `ssh-backend` | ~500K |

## 12. Security

| Crate | Version | Purpose | Rationale |
|-------|---------|---------|-----------|
| `regex` | 1.x | Pattern matching, injection scanning | Standard, blazing fast |
| `aho-corasick` | 1.x | Multi-pattern string matching | O(n) for multiple patterns simultaneously |
| `url` | 2.x | URL parsing + validation | RFC-compliant, SSRF checks |

## 13. Utilities

| Crate | Version | Purpose | Rationale |
|-------|---------|---------|-----------|
| `uuid` | 1.x | UUID generation (session IDs) | Standard |
| `chrono` | 0.4 | Date/time handling | Feature-rich, tz support |
| `dirs` | 5.x | OS directory paths | `~/.config`, `~/.local` |
| `dashmap` | 6.x | Concurrent HashMap | Lock-free for session maps |
| `inventory` | 0.3.22 | Compile-time plugin registration | dtolnay quality, 77M downloads |
| `strsim` | 0.11 | String similarity (fuzzy match) | Levenshtein for tool name typos |
| `strip-ansi-escapes` | 0.2 | Remove ANSI codes | Terminal output sanitization |
| `unicode-width` | 0.2 | Correct character width | TUI column alignment |
| `similar` | 2.x | Diff/patch algorithm | Unified diff for patch tool |
| `notify` | 7.x | File system watcher | Skills sync, hot reload |
| `libloading` | 0.8 | Dynamic library loading | Plugin system (.so/.dylib) |
| `async-trait` | 0.1 | Async fn in traits | Until RPITIT stabilizes fully |
| `secrecy` | 0.8 | Secret string wrapper (zeroize on drop) | Credential isolation, provider-scoped keys |
| `rayon` | 1.x | Data-parallelism thread pool | RL environment tool execution |
| `tree-sitter` | 0.26 | Incremental code parsing | Code analysis tools, syntax-aware search |
| `chromiumoxide` | 0.9 | Chromium DevTools Protocol (async) | Browser tool (screenshot, DOM, evaluate). Tokio-native, async, actively maintained. | `headless_chrome` (sync, less maintained), `fantoccini` (WebDriver-only) |
| `tiktoken-rs` | 0.6 | Token counting (BPE) | Context window budgeting, compression decisions |
| `unicode-normalization` | 0.1 | Unicode NFKC normalization | Command normalization in approval system (bypass prevention) |
| `rand` | 0.9 | Random number generation | WAL jitter retry, session ID generation |
| `rust_decimal` | 1.x | Decimal arithmetic | Cost tracking (PricingEntry), billing precision |

## 14. Build & Dev Tools

| Crate | Version | Purpose |
|-------|---------|---------|
| `cargo-audit` | — | Dependency vulnerability scanning |
| `cargo-deny` | — | License + advisory checks |
| `cargo-flamegraph` | — | Performance profiling |
| `cargo-llvm-cov` | — | Code coverage |
| `criterion` | 0.5 | Benchmarking |
| `insta` | 1.x | Snapshot testing |
| `mockall` | 0.12 | Mock generation for traits |
| `proptest` | 1.x | Property-based testing |
| `tokio-test` | — | Async test utilities |

## 15. Dependency Count Comparison

| | hermes-agent (Python) | EdgeCrab (Rust) |
|--------|----------------------|-----------------|
| Direct dependencies | ~45 (requirements.txt) | ~35 (Cargo.toml across workspace) |
| Transitive dependencies | ~200+ (pip freeze) | ~150 (cargo tree) |
| Optional dependencies | ~15 (extras) | Feature-gated (compile-time) |
| Binary size | ~150MB (venv) | ~15MB (release, stripped) |
| Startup deps loaded | All | Only feature-gated ones |

## 16. Crates NOT Selected (with rationale)

| Crate | Considered For | Why Rejected |
|-------|---------------|-------------|
| `async-std` | Async runtime | Smaller ecosystem, less Tokio compat |
| `actix-web` | HTTP server | Different runtime, less composable |
| `sea-orm` | Database ORM | Too heavy for simple SQLite needs |
| `diesel` | Database ORM | Sync-only, too heavy |
| `tungstenite` | WebSocket | `axum` provides this built-in |
| `prost` | Protobuf | Only needed if Honcho uses gRPC (HTTP fallback preferred) |
| `native-tls` | TLS | `rustls` preferred (pure Rust, no OpenSSL) |
| `chrono-tz` | Timezone DB | `chrono` built-in tz sufficient |
| `serde_yaml` | YAML parsing | Deprecated and unmaintained (dtolnay); replaced by `serde_yml` fork |
| `headless_chrome` | Chromium CDP | Synchronous, less maintained; `chromiumoxide` is async/tokio-native |
