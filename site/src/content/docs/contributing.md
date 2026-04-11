---
title: Contributing
description: How to contribute to EdgeCrab — bug reports, pull requests, SDK development, and coding guidelines.
sidebar:
  order: 3
---

We welcome contributions of all sizes. This page covers the basics — see [Developer: Contributing](/developer/contributing/) for the complete technical guide including how to add tools and gateways.

---

## Quick Start

```bash
git clone https://github.com/raphaelmansuy/edgecrab
cd edgecrab
cargo build          # builds all 11 crates
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all
```

---

## Bug Reports

1. Search [existing issues](https://github.com/raphaelmansuy/edgecrab/issues) first.
2. Include: OS, Rust version (`rustc --version`), EdgeCrab version (`edgecrab --version`), and reproduction steps.
3. For security vulnerabilities, use [GitHub Security Advisories](https://github.com/raphaelmansuy/edgecrab/security/advisories/new) — do not open a public issue.

---

## SDK Development

### Python SDK

```bash
cd sdks/python
pip install -e ".[dev]"
pytest tests/ -v     # 54 tests
```

### Node.js SDK

```bash
cd sdks/node
npm ci
npm run build
npm test             # 24 tests
```

---

## What to Work On

| Area | Where |
|------|-------|
| New tool | `crates/edgecrab-tools/src/tools/` — implement `Tool` trait, register with `inventory::submit!` |
| New gateway | `crates/edgecrab-gateway/src/` — implement `PlatformAdapter` trait |
| Config option | `crates/edgecrab-core/src/config.rs` — add field + `Default` + env override |
| Slash command | `crates/edgecrab-cli/src/commands.rs` |
| Security rule | `crates/edgecrab-security/src/` |
| Bug fix | Identify the crate from the error, write a failing test, then fix |

---

## Adding a Gateway Platform

Gateways connect EdgeCrab to messaging platforms. Each adapter implements the `PlatformAdapter` trait:

```rust
// crates/edgecrab-gateway/src/your_platform.rs

pub struct YourPlatformAdapter { /* env var config */ }

#[async_trait]
impl PlatformAdapter for YourPlatformAdapter {
    async fn run(
        &self,
        tx: mpsc::Sender<IncomingMessage>,
        rx: broadcast::Receiver<OutgoingMessage>,
    ) -> Result<()> {
        // 1. Connect to the platform API
        // 2. Forward incoming messages to `tx`
        // 3. Send outgoing messages from `rx`
    }

    fn platform(&self) -> Platform { Platform::YourPlatform }
    fn supports_markdown(&self) -> bool { true }
    fn supports_images(&self) -> bool { false }
    fn max_message_length(&self) -> usize { 4096 }
}
```

Then register in `gateway_catalog.rs` and add the `Platform::YourPlatform` variant to `edgecrab-types/src/config.rs`.

---

## Coding Guidelines

- Follow `cargo fmt` conventions (enforced by CI).
- Zero clippy warnings — run `cargo clippy --workspace -- -D warnings` locally.
- Every new public API must have a doc comment (`///`).
- New tools must pass through `CommandScanner` before any shell execution.
- No `unwrap()` in tool or gateway code — use `?` and typed errors.
- Security-sensitive changes require an entry in `CHANGELOG.md`.

---

## Pull Requests

1. Fork the repo and create a branch: `git checkout -b feat/my-feature`
2. Write tests for new functionality.
3. Ensure `cargo test --workspace` and `cargo clippy --workspace -- -D warnings` both pass.
4. Open a PR against `main` with a clear title and description.
5. PRs that add dependencies are reviewed for binary size impact.

---

## Dependency Policy

EdgeCrab's binary size is tracked, and current stripped macOS arm64 release builds land around 49 MB. New dependencies are evaluated carefully — open an issue before adding one. Prefer the existing ecosystem (`tokio`, `axum`, `reqwest`, `serde_json`, `rusqlite`) over new crates that overlap.

---

## License

By contributing, you agree that your contributions will be licensed under Apache 2.0.
