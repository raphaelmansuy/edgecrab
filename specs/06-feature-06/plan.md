# v0.6 Implementation Plan

Master plan for implementing all 9 ADRs from Feature v0.6.
Each checkbox is marked only when the feature is fully implemented, tested, and documented.

---

## Phase 1 — Security & Infrastructure

### ADR-0607: Comprehensive Security Hardening
- [x] 1.1 SSRF Redirect Guard — `build_ssrf_safe_client()` in `url_safety.rs`
- [x] 1.2 CRLF Header Injection Guard — `validate_header_value()` in `edgecrab-security/src/lib.rs`
- [x] 1.3 API Server Auth Enforcement — timing-safe Bearer token + bind guard in `api_server.rs`
- [x] 1.4 Twilio Webhook Signature Validation — `validate_twilio_signature()` in `sms.rs`
- [x] 1.5 Unit tests for all security hardening (SSRF redirect, CRLF, auth, bind guard)
- [x] 1.6 Web tools updated to use `build_ssrf_safe_client()`
- [x] 1.7 Add `subtle` dependency to edgecrab-security Cargo.toml

### ADR-0606: Unified Proxy Support
- [x] 2.1 Create `proxy.rs` in edgecrab-gateway — `resolve_proxy_url()` + `build_proxy_client()`
- [x] 2.2 macOS system proxy detection via `scutil --proxy`
- [x] 2.3 Enable `reqwest` socks feature in edgecrab-gateway Cargo.toml
- [x] 2.4 Register `pub mod proxy` in gateway lib.rs
- [x] 2.5 Unit tests for proxy resolution cascade, SOCKS5, macOS detection
- [x] 2.6 Integration point notes in web.rs / adapters (lazy adoption)

### ADR-0604: Background Process Watch Patterns
- [x] 3.1 Add `WatchState`, `WatchEvent`, `WatchEventType` to `process_table.rs`
- [x] 3.2 Add `check_watch_patterns()` function with rate limiting
- [x] 3.3 Add `watch_patterns` param to `run_process` tool schema in `process.rs`
- [x] 3.4 Integrate pattern checking in drain loop
- [x] 3.5 Add `watch_notification_tx` to `ToolContext`
- [x] 3.6 Unit tests: match, no-match, rate limit, overload disable, output trim, multi-pattern
- [x] 3.7 Block `watch_patterns` in `execute_code` tool

---

## Phase 2 — New Platforms

### ADR-0602: WeChat (Weixin) & WeCom Enhancements
- [x] 4.1 Add `Platform::Weixin` and `Platform::BlueBubbles` to Platform enum in edgecrab-types
- [x] 4.2 Create `weixin_crypto.rs` — AES-128-ECB CDN crypto helpers
- [x] 4.3 Create `weixin.rs` — WeixinAdapter implementing PlatformAdapter
- [x] 4.4 Weixin: long-poll loop, context token echo, markdown reformatting, dedup
- [x] 4.5 WeCom enhancements: chunked media upload, text batching, AES-256-CBC decrypt
- [x] 4.6 Add `aes`, `cbc` deps to edgecrab-gateway Cargo.toml
- [x] 4.7 Register modules in gateway lib.rs and run.rs
- [x] 4.8 Unit tests for crypto, context token, dedup, chunked upload, text batching

### ADR-0601: iMessage via BlueBubbles
- [x] 5.1 Create `bluebubbles.rs` — BlueBubblesAdapter implementing PlatformAdapter
- [x] 5.2 Webhook server (axum) for inbound messages
- [x] 5.3 REST client for outbound messages, attachments
- [x] 5.4 GUID cache, tapback filtering, markdown stripping
- [x] 5.5 Crash-recovery webhook dedup
- [x] 5.6 Register in gateway lib.rs, run.rs, gateway_catalog.rs
- [x] 5.7 Unit tests: auth, skip filters, GUID cache, markdown strip, message split

---

## Phase 3 — User Tools

### ADR-0605: Pluggable Context Engine
- [x] 6.1 Create `context_engine.rs` trait in edgecrab-core
- [x] 6.2 `BuiltinCompressorEngine` in same file (wraps compression.rs)
- [x] 6.3 Add `context_engine` field to Agent/AgentBuilder in agent.rs
- [x] 6.4 Inject engine tool schemas in conversation.rs + dispatch routing
- [x] 6.5 Add `context.engine` config key in config.rs
- [x] 6.6 Create `context.rs` in edgecrab-plugins for manifest discovery
- [x] 6.7 `PluginContextEngine` JSON-RPC stdio adapter in context_engine.rs
- [x] 6.8 `build_agent()` async + config-driven engine loading in runtime.rs
- [x] 6.9 Unit tests: default engine, tool injection, fallback, lifecycle, tool cap

### ADR-0608: Backup & Import
- [x] 7.1 Create `backup.rs` in edgecrab-cli
- [x] 7.2 `edgecrab backup` subcommand with exclusions (.env, mcp-tokens, sessions.db)
- [x] 7.3 `edgecrab import` subcommand with path traversal protection
- [x] 7.4 Symlink blocking, dry-run mode, force flag
- [x] 7.5 Atomic extraction (temp dir → rename)
- [x] 7.6 Register subcommands in main.rs
- [x] 7.7 Unit tests: create, exclude, traversal, symlink, roundtrip, dry-run, force

### ADR-0609: Debug & Dump
- [x] 8.1 Enhance existing `dump_cmd.rs` with API key status, config overrides, redaction
- [x] 8.2 Add `--show-keys` flag with first4+last4 redaction
- [x] 8.3 Add `/debug` slash command in commands.rs
- [x] 8.4 Handle `/debug` in app.rs (alias for /dump dispatches to run_dump)
- [x] 8.5 Plain-text output (no ANSI codes) for copy-paste friendliness
- [x] 8.6 Unit tests: format, redaction, missing config, show-keys flag

---

## Phase 4 — Platform Support

### ADR-0603: Termux / Android Support
- [x] 9.1 Add `is_termux()` + `IS_TERMUX` lazy static in edgecrab-types
- [x] 9.2 Add `termux` feature flag to edgecrab-cli Cargo.toml
- [x] 9.3 TUI compact mode for narrow terminals (< 60 cols) + IS_TERMUX BasicCompat
- [x] 9.4 Doctor command adaptations for Termux (check_termux_storage, feature warnings)
- [x] 9.5 Path jail: add Termux data directory (/data/data/com.termux/files) in config_ref.rs
- [x] 9.6 Build target documentation in Makefile (build-termux target)
- [x] 9.7 Unit tests: detection (is_termux_false_on_desktop), doctor (check_termux_storage_returns_check)

---

## Cross-Cutting: Documentation & Release

- [ ] 10.1 Update CHANGELOG.md with v0.6.0 entries
- [ ] 10.2 Update README.md (platform count, new features)
- [ ] 10.3 Astro doc site: security hardening page
- [ ] 10.4 Astro doc site: proxy support page
- [ ] 10.5 Astro doc site: watch patterns page
- [ ] 10.6 Astro doc site: new platforms (iMessage, WeChat, WeCom)
- [ ] 10.7 Astro doc site: context engine page
- [ ] 10.8 Astro doc site: backup & import page
- [ ] 10.9 Astro doc site: debug/dump page
- [ ] 10.10 Astro doc site: Termux/Android page
- [ ] 10.11 Update AGENTS.md with new crate info
- [ ] 10.12 `cargo clippy --workspace -- -D warnings` clean
- [ ] 10.13 `cargo test --workspace` passes
- [ ] 10.14 Final assessment: verify feature parity with hermes-agent v0.9.0

---

## Acceptance Verification

- [ ] All 9 ADR acceptance criteria met
- [ ] All security audit items verified
- [ ] DRY: no duplicated logic across features
- [ ] SOLID: each module has single responsibility
- [ ] Edge cases: all documented edge cases handled
- [ ] E2E: integration tests for critical paths
- [ ] Hermes-agent parity: exceeds on all implemented features
