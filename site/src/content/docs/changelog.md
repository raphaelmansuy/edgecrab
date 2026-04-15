---
title: Changelog
description: All notable changes to EdgeCrab, following Keep a Changelog.
sidebar:
  order: 2
---

All notable changes to EdgeCrab are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

Last updated: 2026-05-15

---

## [0.6.0] — 2026-05-15

### Added

- **iMessage via BlueBubbles** — webhook-based inbound messages, REST outbound, GUID dedup, tapback filtering, Private API auto-detection, inbound attachment download, improved markdown stripping.
- **WeChat (Weixin) via iLink Bot API** — POST-based long-poll with sync buffer, AES-128-ECB CDN media pipeline (upload + download), session-expired auto-recovery, context token echo, markdown reformatting.
- **WeCom enhancements** — WebSocket messaging, AES-256-CBC decryption, chunked media upload, text batching, 30s heartbeat keep-alive, `aibot_event_callback` support.
- **Pluggable Context Engine** — `ContextEngine` trait for custom context management, `BuiltinCompressorEngine` default, plugin discovery via `edgecrab-plugins`.
- **Background Process Watch Patterns** — `watch_patterns` parameter for pattern-matched output notifications with rate limiting.
- **Unified Proxy Support** — 6-level cascade proxy resolution including macOS `scutil` and SOCKS5.
- **Backup & Import** — Timestamped tar.gz archives with path traversal protection and dry-run mode.
- **Debug/Dump command** — 15 monitored config paths, API key scanning, `/debug` slash command.
- **Termux/Android support** — TUI compact mode, Termux path jail, `termux` feature flag.
- **Security hardening** — SSRF redirect guard, CRLF header injection guard, timing-safe auth, Twilio webhook signature validation.

### Changed

- WeCom `BATCH_LONG_THRESHOLD` raised from 3500 to 3900; `BACKOFF_STEPS` extended with 60s final step.
- BlueBubbles `strip_markdown()` now handles code fences, links, italic, and nested formatting.
- Dump `INTERESTING_OVERRIDES` expanded from 8 to 15 monitored config paths.

---

## [0.5.0] — 2026-04-13

### Added

- **Web search fallback chain — Firecrawl → Brave → Tavily → DuckDuckGo** with `wreq` BoringSSL TLS fingerprint.
- **Per-tool rich result display in TUI done-lines** via `format_tool_result()`.

---

## [Unreleased]

---

## [0.4.1] — 2026-04-13

### Fixed

- **Gateway `vision_analyze` crashes on fresh installs** — sending an image via WhatsApp, Telegram, or any gateway platform could fail with `Cannot resolve allowed root '~/.edgecrab/images': No such file or directory`. The path security layer now gracefully skips optional trusted roots (like `gateway_media/`, `image_cache/`, `images/`) that haven't been created yet, instead of raising a hard error. The fix is general: any tool that passes lazily-created directories as extra trusted roots now works correctly on first run.
- **`pdf_to_markdown` extra-root guard simplified** — the manual `.exists()` workaround added in v0.4.0 is replaced by the corrected behavior in the security layer.

### Added

- **ADR-001: Gateway Path Policy** — full architectural decision record explaining the first-principles analysis, the Hermes comparison, alternatives rejected, and security invariants preserved.
- **Homebrew tap supports GitHub App tokens** — no-secret short-lived authentication for automated formula updates. See the [Homebrew tap setup guide](https://edgecrab.com/setup/homebrew) for configuration steps.

---

## [0.4.0] — 2026-04-13

### Added

- **Large tool outputs can now spill to session-scoped artifact files instead of bloating prompt history** — successful tool results over the configured threshold are persisted under `.edgecrab-artifacts/<session_id>/...`, while the model sees a compact preview stub with metadata and a readable path.
- **Spill behavior is now configurable** — `tools.result_spill`, `tools.result_spill_threshold`, and `tools.result_spill_preview_lines` plus matching `EDGECRAB_TOOL_RESULT_SPILL*` overrides control whether spilling is enabled, when it triggers, and how much preview is kept inline.
- **Config-driven git worktree mode and TUI worktree control surface** — `worktree: true` and `EDGECRAB_WORKTREE=1` now enable isolated git worktrees by default, `/worktree` and `/w` open a dedicated TUI report overlay, and worktree cleanup now preserves branches with unpushed commits.
- **First-class `/log` TUI with persisted log-level control** — `/log` and `/logs` now open a split-pane log browser with entry inspector and live follow; `1-5` or `/log level <level>` persist the default log level.

### Changed

- **Gateway/chat origin metadata now uses a named value type** — the internal `(String, String)` pair has been replaced with `OriginChat { platform, chat_id }`, improving type safety without changing user-facing behavior.
- **Interactive and non-interactive log inspection now share one backend** — `edgecrab logs`, startup logging, config persistence, and TUI browsers all reuse the same file-discovery and log-level helpers.

### Fixed

- **Clippy: useless `format!` call replaced with `.to_string()`** — gateway_cmd.rs WhatsApp state message now uses idiomatic Rust.

### Verification

| Check | Result |
|--------|--------|
| `cargo run -- --version` | **passed locally before cut** |
| `cargo fmt --all --check` | **passed locally before cut** |
| `cargo clippy --workspace -- -D warnings` | **passed locally before cut** |
| `cargo test --workspace` | **passed locally before cut** |

---

## [0.3.3] — Fix Release: Deterministic Tag Reruns and Bounded Rust Publish Waits

### Fixed

- **Rust crates.io release waits now fail soft without hanging a workflow runner** — the helper keeps the intentional propagation delay, but every crates.io probe now uses explicit connection and total-request timeouts before the publish-retry logic takes over.
- **Manual release reruns now rebuild the requested tag, not the current branch tip** — native binaries, Docker, Node SDK, and Python SDK dispatches now check out the selected tag ref so re-publishing a release is reproducible.
- **Release coordinator bumps now include the Node SDK lockfile** — versioned release metadata can no longer drift between `sdks/node/package.json` and `sdks/node/package-lock.json`.

### Verification

| Check | Result |
|--------|--------|
| `cargo run -- --version` | **passed locally before cut** |
| `cargo fmt --all --check` | **passed locally before cut** |
| `./scripts/release-version.sh check` | **passed locally before cut** |

## [0.3.2] — Fix Release: Crates.io Propagation, Version Sync, and Docs Accuracy

### Fixed

- **crates.io release propagation waits are now robust enough for real registry lag** — the Rust release workflow now checks the exact published crate-version endpoint, allows a longer bounded wait, and keeps a short stabilization buffer before publishing dependents.
- **release version syncing now includes `edgecrab-command-catalog` and the Node SDK lockfile header** — the version script and its drift check now cover these previously missed files, preventing silent version skew inside the workspace.
- **CLI reference docs now match the real command output** — `edgecrab version` and `edgecrab --version` are documented separately, with the provider list corrected to the current model catalog output.
- **Astro docs no longer hard-code stale release numbers across install, update, ACP, self-hosting, and FAQ pages** — examples now use explicit verification commands or placeholders where a fixed patch number would rot quickly.

### Verification

| Check | Result |
|--------|--------|
| crates.io wait helper | **validated locally against `edgecrab-types 0.3.1` exact-version endpoint** |
| `pnpm build` in `site/` | **passed** |
| `cargo run -- version` | **passed** |
| `cargo run -- --version` | **passed** |

---

## [0.3.1] — Fix Release: Distribution, Release, and Docs Sync

### Fixed

- **`cargo run` at the workspace root now works** — the workspace default member is `edgecrab-cli`, and the CLI package now declares `default-run = "edgecrab"`.
- **npm CLI wrapper no longer serves stale native binaries** — reinstall and first-run logic now verify the actual native binary version and replace mismatched cached binaries instead of silently reusing them.
- **PyPI CLI wrapper no longer prefers an unrelated older system binary on `PATH`** — by default it now treats the package-managed native binary as authoritative, preventing stale Homebrew or cargo installs from shadowing the upgraded PyPI install.
- **CLI startup does less blocking work before the UI is usable** — bundled skills sync is moved off the blocking startup path, and the initial skills/profile scans are now lazy where safe.
- **Release docs site facts are now source-derived at build time** — homepage version, provider/tool/gateway counts, and related claims are generated from the repository state instead of stale hard-coded values.

### Changed

- **Release automation now updates the external Homebrew tap explicitly** — a dedicated workflow and helper script update `raphaelmansuy/homebrew-tap` from GitHub Release checksums.
- **crates.io publication waits are now version-aware** — dependent crate publishing still waits for propagation, but the release workflow now polls for the exact published version instead of relying only on fixed sleeps.
- **Install and troubleshooting documentation was audited end-to-end** — npm, PyPI, crates.io, Docker, Homebrew, `cargo run`, FAQ, release process, and site copy were all updated for accuracy.

### Verification

| Check | Result |
|--------|--------|
| `cargo run -- --version` | **passed** |
| `cargo build --workspace` | **passed** |
| `cargo check -p edgecrab-cli` | **passed** |
| `pnpm build` in `site/` | **passed** |
| fresh npm install | **`which edgecrab` + `edgecrab --version` verified** |
| fresh PyPI install | **`which edgecrab` + `edgecrab --version` verified** |
| Docker image | **`which edgecrab` + `edgecrab --version` verified** |
| Homebrew tap | **verified stale at `0.2.3` before tap-sync fix release** |

---

## [0.3.0] — Phase 5: Integration & Polish

### Added

#### Language Server Protocol

- **New `edgecrab-lsp` crate** — a dedicated subsystem for language-server lifecycle, document sync, diagnostics, position translation, workspace edits, and LLM-friendly result rendering.
- **25 LSP tools** — EdgeCrab now exposes Claude-parity navigation plus code actions, rename, formatting, inlay hints, semantic tokens, signature help, type hierarchy, pull diagnostics, linked editing, LLM-enriched diagnostics, guided action selection, and workspace-wide type-error scans.
- **LSP-first agent guidance** — when LSP tools are available, the prompt explicitly tells the agent to prefer semantic navigation and edits over plain text search.

#### CI / CD and Docs

- **CI and release automation updated for `edgecrab-lsp`** — CI now runs the dedicated LSP package tests explicitly and the crates.io publish order includes the new crate before `edgecrab-core`.
- **Docs and website refreshed** — the features overview, tool reference, coding workflows guide, and configuration reference now document the LSP feature set and configuration.
- **Release-prep honesty pass** — the README, internal docs, and site now use source-derived provider/tool/gateway counts and the measured stripped macOS arm64 release binary size of about 49 MB instead of stale placeholder numbers.

### Stats

| Metric | Value |
|--------|-------|
| Verification | **`cargo test --workspace` passed** |
| Clippy warnings | **0** (`-D warnings`) |
| Binary size | **~49 MB** (current stripped macOS arm64 release build) |
| Startup profile | **Native binary, no Python/Node bootstrap** |
| Rust edition | **2024** |
| MSRV | **1.86.0** |

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
