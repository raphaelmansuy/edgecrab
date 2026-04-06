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

### Stats

| Metric | Value |
|--------|-------|
| Tests passing | **324** (unit + integration + e2e) |
| Clippy warnings | **0** (`-D warnings`) |
| Binary size | **~15 MB** (static, stripped) |
| Cold startup | **< 50 ms** |
| Rust edition | **2024** |
| MSRV | **1.85.0** |

---

### Added

#### SDKs

- **Python SDK** (`sdks/python/`) — `edgecrab-sdk` on PyPI. Sync and async clients (`Agent` / `AsyncAgent`) with conversation history, streaming via async generators, and a `edgecrab` CLI entry point (`chat`, `models`, `health`). **54 tests.**
- **Node.js SDK** (`sdks/node/`) — `edgecrab-sdk` on npm. TypeScript-first with dual CJS + ESM build. `Agent` class with streaming (async generator), tool customization, and a matching CLI. **24 tests.**

#### CI / CD

- **CI workflow** — Rust build, test, clippy (`-D warnings`), fmt validation on ubuntu / macos / windows matrix. Separate Python and Node.js SDK test jobs. `cargo-audit` security scan.
- **Release workflows** — `release-rust.yml` publishes all 10 crates to crates.io in dependency order. `release-python.yml` builds wheels + sdist for PyPI. `release-node.yml` publishes to npm. `release-docker.yml` builds and pushes multi-arch image to GHCR (`linux/amd64`, `linux/arm64`).

#### Site

- **Astro + Starlight** documentation site published at [edgecrab.com](https://www.edgecrab.com). Landing page, feature guides, provider docs, reference, and gateway setup pages.

#### `edgecrab-core`

- **`Agent::chat_streaming()`** — async method delivering events via `UnboundedSender<StreamEvent>` for real-time TUI rendering.
- **`StreamEvent` enum** — typed stream of `Token(String)`, `Reasoning(String)`, `ToolExec`, `ToolDone`, `SubAgentStart/Finish`, `Clarify`, `Approval`, `ContextPressure`, `Done`, `Error(String)`, and more. Full list in `agent.rs`.
- **Context compression** — `compress_with_llm()` in `compression.rs` summarizes old messages to stay within context windows without losing key facts.
- **`IterationBudget`** — lock-free `AtomicU32` CAS counter enforcing `model.max_iterations` (default: 90). Exhausted budget sets `budget_exhausted: true` in `ConversationResult`.
- **`ApiMode` auto-detection** — routes to `ChatCompletions`, `AnthropicMessages`, or `CodexResponses` based on base URL and model name.

#### `edgecrab-tools`

- **`RunProcessTool`** — real background process spawning via `tokio::process::Command`. Returns a process handle for `list_processes`, `kill_process`, `get_process_output`, `wait_for_process`, `write_stdin`.
- **`WebSearchTool`** — DuckDuckGo Instant Answer API. No API key required.
- **`WebExtractTool`** — fetches URLs via `reqwest`, strips HTML to readable text, protected by `SafeUrl` SSRF guard.
- **`apply_patch` tool** — companion to `patch`; applies unified diff patches to files.
- **`browser_wait_for`, `browser_select`, `browser_hover`** — three new CDP browser control tools.
- **Fuzzy tool dispatch** — Levenshtein distance ≤ 3 fallback for misspelled tool names. Exact match is O(1) HashMap; fuzzy is O(n) with threshold.

#### `edgecrab-cli`

- **YAML skin engine** — `SkinConfig` reads `~/.edgecrab/skin.yaml` at startup. 7 semantic color slots + `prompt_symbol` + `tool_prefix`.
- **Model hot-swap** — `/model provider/model` calls `agent.swap_model()` without restarting.
- **Session management** — `/session new` and `/new` slash commands.

#### `edgecrab-gateway`

- **15 platform adapters** — Telegram, Discord, Slack, WhatsApp, Signal, Matrix, Mattermost, DingTalk, SMS, Email, Home Assistant, Webhook, API Server, Feishu/Lark, WeCom.
- **Gateway slash commands** — `/help`, `/new`, `/reset`, `/stop`, `/retry`, `/status`, `/usage`, `/background`, `/hooks`, `/approve`, `/deny` intercepted before agent dispatch.
- **Inline approval flow** — `ApprovalChoice::{Once, Session, Always, Deny}` with inline buttons on Telegram/Discord, slash commands in TUI.
- **Adapter restart backoff** — 5s → 10s → 20s → … → 120s cap on adapter crashes.

---

### Known Limitations

- Matrix E2E encryption rooms are not supported (plain rooms only).
- Browser tools require Chrome/Chromium or a `CDP_URL` env var.
- `execute_code` sandbox RPC is Unix socket only — not available on Windows hosts.

---

## How to Upgrade

```bash
# Via cargo
cargo install edgecrab --locked

# Via npm
npm install -g edgecrab-sdk@latest

# Via pip
pip install --upgrade edgecrab-sdk
```

Run `edgecrab migrate` after a major version bump to migrate stored sessions and config.

---

## Versioning Policy

EdgeCrab follows [Semantic Versioning](https://semver.org/). The `0.x` series may include breaking config or CLI changes between minor versions — see individual release notes. Starting from `1.0.0`, breaking changes will only appear in major version bumps.

---

## Security Advisories

Report vulnerabilities privately via [GitHub Security Advisories](https://github.com/raphaelmansuy/edgecrab/security/advisories/new). Do not open a public issue.
