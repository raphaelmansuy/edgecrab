---
title: Contributing
description: How to contribute to EdgeCrab — bug reports, pull requests, SDK development, and coding guidelines.
sidebar:
  order: 3
---

We welcome contributions of all sizes! Please read this guide before opening a PR.

---

## Bug Reports

1. Search [existing issues](https://github.com/raphaelmansuy/edgecrab/issues) first.
2. Include: OS, Rust version (`rustc --version`), EdgeCrab version (`edgecrab --version`), and reproduction steps.
3. For security vulnerabilities, email `security@elitizon.com` — do not open a public issue.

---

## Development Setup

```bash
git clone https://github.com/raphaelmansuy/edgecrab
cd edgecrab
cargo build          # Build all crates
cargo test           # Run all tests
cargo clippy -- -D warnings
cargo fmt --all
```

---

## SDK Development

### Python SDK

```bash
cd sdks/python
pip install -e ".[dev]"
pytest tests/ -v
```

### Node.js SDK

```bash
cd sdks/node
npm ci
npm run build
npm test
```

---

## Coding Guidelines

- Follow `cargo fmt` conventions (enforced by CI).
- Zero clippy warnings — run `cargo clippy -- -D warnings` locally.
- Every new public API must have a doc comment.
- New tools in `edgecrab-tools` must pass through `CommandScanner` before execution.
- Security-sensitive changes require an entry in `CHANGELOG.md`.

---

## Pull Requests

1. Fork the repo and create a branch: `git checkout -b feat/my-feature`.
2. Write tests for new functionality.
3. Ensure `cargo test` and `cargo clippy -- -D warnings` both pass.
4. Open a PR against `main` with a clear title and description.

---

## License

By contributing, you agree that your contributions will be licensed under Apache 2.0.
