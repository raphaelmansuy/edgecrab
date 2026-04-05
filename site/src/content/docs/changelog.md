---
title: Changelog
description: All notable changes to EdgeCrab, following Keep a Changelog.
sidebar:
  order: 2
---

All notable changes to EdgeCrab are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

---

## [Unreleased] — Phase 5: Integration & Polish

### Added

#### SDKs & CI/CD
- **Python SDK** (`sdks/python/`) — `edgecrab-sdk` on PyPI. Sync/async clients, Agent/AsyncAgent with conversation history, streaming, CLI (`edgecrab chat/models/health`). 54 tests.
- **Node.js SDK** (`sdks/node/`) — `edgecrab-sdk` on npm. TypeScript-first with Agent, streaming (async generator), CLI. CJS + ESM dual build. 24 tests.
- **CI workflow** — Rust build/test/clippy/fmt on ubuntu/macos/windows matrix, Python & Node.js SDK tests, cargo-audit security scan.
- **Release workflows** — `release-rust.yml` (10 crates to crates.io), `release-python.yml` (wheels + sdist to PyPI), `release-node.yml` (npm publish), `release-docker.yml` (multi-arch GHCR image).
- **Site** — Astro + Starlight documentation site published at [edgecrab.com](https://www.edgecrab.com).

#### `edgecrab-core`
- **`Agent::chat_streaming()`** — async method delivering tokens via `UnboundedSender<StreamEvent>`.
- **`StreamEvent` enum** — `Token(String)`, `Done`, `Error(String)`.
- **LLM compression** — `compress_with_llm()` summarizes old messages to stay within context windows.

#### `edgecrab-tools`
- **`RunProcessTool`** — real background process spawning via `tokio::process::Command`.
- **`WebSearchTool`** — DuckDuckGo Instant Answer API. No API key required.
- **`WebExtractTool`** — fetches URLs via `reqwest`, strips HTML, SSRF-protected.

#### `edgecrab-cli`
- **YAML skin engine** — `SkinConfig` reads `~/.edgecrab/skin.yaml` at startup.
- **Model hot-swap** — `/model provider/model` creates a new provider and calls `agent.swap_model()`.
- **Session management** — `/session new` and `/new`.
- **Streaming display** — tokens arrive via `AgentResponse::Token(text)` in the TUI event loop.

### Stats
- Tests: **324 passing** (unit + integration + e2e)
- Clippy: **0 warnings** with `-D warnings`
- Binary size: **~15 MB**
- Cold startup: **< 50 ms**
