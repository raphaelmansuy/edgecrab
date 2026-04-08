# ADR-003: Make `--config` Rebase Runtime Home and Make `config edit` Shell-Realistic

Status: accepted

## Context

Before this change, binary commands had an inconsistency:

- `load_runtime()` used the parent of `--config` as the effective home for config, `.env`, and `state.db`
- some helper modules still consulted `EDGECRAB_HOME` or `edgecrab_home()` independently
- the process environment was not updated when `--config` was supplied

Separately, `edgecrab config edit` launched `$EDITOR` as if it were a single executable path,
which is not how many users configure editors.

## Decision

- When a runtime command uses `--config <path>`, EdgeCrab sets `EDGECRAB_HOME` to the parent directory of that config file before command dispatch.
- `config edit` parses `$EDITOR` and `$VISUAL` with shell-style tokenization, then launches the program with its declared arguments.
- `config env-path` resolves relative to the active config file’s directory.

## Why

This is the smallest DRY fix that repairs multiple command families at once.

It preserves one consistent rule:

> the directory that owns the active `config.yaml` also owns the sibling `.env`,
> `state.db`, plugins, skills, and other runtime-local state

## Consequences

- Helper modules no longer silently drift back to the default home when a custom config root is in use.
- Binary command behavior is more predictable for managed deployments, test fixtures, and non-default operator setups.
- `config edit` now works with common values like `code --wait`, `nvim -u NORC`, and similar editor wrappers.

## References

- [`crates/edgecrab-cli/src/main.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/main.rs)
- [`crates/edgecrab-cli/src/runtime.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/runtime.rs)
- [`crates/edgecrab-cli/src/plugins_cmd.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/plugins_cmd.rs)
