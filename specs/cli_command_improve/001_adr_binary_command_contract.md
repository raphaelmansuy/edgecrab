# ADR-001: Treat the Binary CLI Surface as a Product Contract

Status: accepted

## Context

The `edgecrab` binary exposes a large command surface before the TUI even starts:

- top-level entry flags such as `--help`, `--version`, `--config`, and `--profile`
- top-level subcommands such as `status`, `config`, `mcp`, `gateway`, and `skills`
- nested subcommand trees for sessions, profiles, cron, MCP, and gateway workflows

These are not internal plumbing. They are a user-facing interface and must be
reviewed with the same rigor as the TUI command surface.

## Decision

Every terminal command exposed by clap is governed by four requirements:

1. It must parse correctly.
2. It must dispatch to the intended implementation.
3. Its output/help text must describe real behavior.
4. It must remain coherent under global runtime modifiers such as `--config`, `--profile`, and `--quiet`.

## Why

The audit found binary-specific risks that static parsing alone does not catch:

- a custom `--config` path did not consistently redefine the effective runtime home for all helper modules
- `config edit` assumed `$EDITOR` was a bare executable path, which breaks common values like `code --wait`
- `version` used a hand-maintained provider list that can drift from the actual model catalog

## Consequences

- Binary command audits must cross-reference clap definitions, dispatch code, helper modules, and runtime config semantics together.
- A command is not considered complete just because clap parses it.
- Global runtime modifiers must be treated as part of every command contract.

## References

- [`crates/edgecrab-cli/src/cli_args.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/cli_args.rs)
- [`crates/edgecrab-cli/src/main.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/main.rs)
- [002_audit_matrix.md](./002_audit_matrix.md)
