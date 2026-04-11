---
title: Contributing
description: How to build EdgeCrab from source, run tests, add a tool, and submit a pull request.
sidebar:
  order: 2
---

## Prerequisites

- Rust 1.86.0+ (`rustup update stable`)
- `cargo` (ships with Rust)
- Optional: Chrome/Chromium for browser tool tests
- Optional: `nextest` for parallel test execution: `cargo install cargo-nextest`

---

## Building

```bash
# Clone
git clone https://github.com/raphaelmansuy/edgecrab
cd edgecrab

# Debug build (fast compile)
cargo build

# Release build (optimized)
cargo build --release

# The binary is at:
./target/release/edgecrab
```

---

## Running Tests

```bash
# All tests (unit + integration)
cargo test --workspace

# Parallel (faster)
cargo nextest run --workspace

# Single crate
cargo test -p edgecrab-tools

# With output
cargo test --workspace -- --nocapture

# E2E tests (requires --release)
cargo test --workspace --release --test '*'
```

---

## Code Structure

Before working on a feature, read the crate that owns it:

| Feature | Crate |
|---------|-------|
| New tool | `edgecrab-tools/src/tools/` |
| Config option | `edgecrab-core/src/config.rs` |
| Slash command | `edgecrab-cli/src/commands.rs` |
| CLI flag | `edgecrab-cli/src/cli_args.rs` |
| Platform adapter | `edgecrab-gateway/src/` |
| Security rule | `edgecrab-security/src/` |
| Database schema | `edgecrab-state/src/` + `edgecrab-migrate/` |

---

## Adding a Tool

Tools are registered at **compile time** via the `inventory` crate — zero runtime startup cost.

1. Create `crates/edgecrab-tools/src/tools/your_tool.rs`
2. Implement the `Tool` trait:
   ```rust
   pub struct YourTool;

   impl Tool for YourTool {
       fn name(&self) -> &'static str { "your_tool" }
       fn description(&self) -> &'static str { "What this tool does" }
       fn parameters(&self) -> serde_json::Value { /* JSON Schema object */ }
       async fn call(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult { ... }
   }
   ```
3. Register with `inventory::submit!` at the bottom of the file:
   ```rust
   inventory::submit! {
       // The exact wrapper type is defined in edgecrab-tools/src/registry.rs
       ToolRegistration::new(YourTool)
   }
   ```
4. Add to the appropriate toolset in `toolsets.rs`
5. Add tests in `crates/edgecrab-tools/tests/`

> **Note:** Commands issued by tools are checked by `CommandScanner` before execution. Tools that shell out must pass their command through the scanner — see existing terminal tools for examples.

---

## Adding a Config Option

1. Add the field to the appropriate struct in `edgecrab-core/src/config.rs`
2. Set a sensible `Default` value
3. Add env var override in `apply_env_overrides()` if needed
4. Document in [Configuration Reference](/reference/configuration/)

---

## Code Style

- `cargo fmt --all` before committing
- `cargo clippy --workspace -- -D warnings` must pass
- No `unwrap()` in tool implementations — use `?` and `ToolError`
- All public APIs must have doc comments (`///`)
- Integration tests in `tests/` subdirectory, unit tests in `#[cfg(test)]` modules

---

## Pull Request Process

1. Fork the repository
2. Create a feature branch: `git checkout -b feat/my-feature`
3. Make changes with tests
4. Run the full test suite: `cargo test --workspace`
5. Run clippy: `cargo clippy --workspace -- -D warnings`
6. Format: `cargo fmt --all`
7. Open a PR against `main` with a clear description

---

## Crate Versioning

All crates in the workspace share the same version number (set in the workspace `Cargo.toml`). Version bumps are done via a single PR that updates the workspace version.

---

## License

Apache-2.0. By contributing, you agree to license your contribution under the Apache 2.0 license.

---

## Pro Tips

- **Run `cargo nextest run --workspace`** before opening a PR — it runs tests in parallel and is 3-4x faster than `cargo test`.
- **Start small**: one tool, one crate, one test. Keeping PRs focused makes review faster.
- **Read the crate README** before editing — each crate in `crates/` has a short README explaining its role in the graph.
- **Use `RUST_LOG=debug edgecrab ...`** to trace exactly which code paths your change touches before writing tests.
- **Check `clippy` early**: `cargo clippy --workspace -- -D warnings` will save you a round-trip after opening the PR.

---

## FAQ

**Can I add a dependency without approval?**
Open an issue first. EdgeCrab's binary size and startup time are tracked metrics. New dependencies are evaluated against both.

**My test requires a real API key — how do I mark it?**
Gate it with `#[ignore]` and document the env var in the test function's doc comment. CI will skip it; contributors with keys can run it manually.

**Where do I add a new config option?**
In `crates/edgecrab-core/src/config.rs`, in the relevant struct. Then add an `EDGECRAB_*` override in `apply_env_overrides()` and document it in the [Environment Variables](/reference/environment-variables/) and [Configuration](/reference/configuration/) pages.

**How do I run only one test file?**
```bash
cargo test -p edgecrab-tools --test checkpoint_test
```

---

## See Also

- [Architecture](/developer/architecture/) — understand the crate graph before making changes
- [Configuration Reference](/reference/configuration/) — full config schema
- [CLI Commands](/reference/cli-commands/) — commands you can invoke to test your work
