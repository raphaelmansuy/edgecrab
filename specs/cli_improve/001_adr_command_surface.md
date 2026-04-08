# ADR-001: Treat the Command Surface as Product, Not Plumbing

Status: accepted

## Context

EdgeCrab already has a rich command surface, but richness alone is not quality.

The project exposes two operator-facing layers:

- binary subcommands through clap
- interactive slash commands inside the TUI

Those layers are user interface, not internal implementation detail. A command that is registered but shallow, misleading, or inconsistent is a UX bug.

## Decision

EdgeCrab will treat every documented command as a contract with four required properties:

1. It must be discoverable.
2. It must be correctly wired.
3. It must do something genuinely useful.
4. It must align with the help text and surrounding UX.

## Why

The audit found several command-surface failures that were not type-safety failures:

- `/theme` help promised browsing, but no-arg execution reloaded the current theme.
- `/statusbar` was a placeholder message, not stateful behavior.
- `/approve` and `/deny` ignored the actual TUI approval state.
- `/sethome` was not connected to the config system.
- `/update` claimed the install was current without checking anything.
- `/config` was underpowered relative to the central role of configuration in EdgeCrab.

## Consequences

- Slash commands should prefer specific `CommandResult` variants over generic string output when the TUI has real state to mutate.
- Help text must describe actual behavior, not intended behavior.
- Commands that bridge into persistent config must persist and reflect runtime state where safe.
- The audit matrix becomes part of the maintenance model: adding a command now implies checking docs, registry, handler, persistence, and tests together.

## References

- [`crates/edgecrab-cli/src/commands.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/commands.rs)
- [`crates/edgecrab-cli/src/app.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/app.rs)
- [003_audit_matrix.md](./003_audit_matrix.md)
