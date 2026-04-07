# ADR-002: TUI-First MCP Operator UX

Status: accepted

## Context

EdgeCrab already has a ratatui-based MCP selector in `crates/edgecrab-cli/src/app.rs`.

That selector is not ornamental. It is the natural MCP operations console for interactive users.

## Decision

The TUI will be treated as a first-class MCP operations surface, not a thin wrapper over text commands.

## Why

1. MCP setup is iterative and failure-prone.
2. Users need browse, inspect, install, test, diagnose, and remove actions in one place.
3. Text-only workflows are slower and more error-prone for repeated MCP operations.

## Consequences

- `/mcp` with no args opens the selector.
- Configured entries get explicit health-oriented actions, including doctor/check.
- The selector must surface meaningful metadata, not just names.
- TUI command parsing must be robust enough to accept real-world quoted values.

## References

- `crates/edgecrab-cli/src/app.rs`
- [tui-and-operations.md](./tui-and-operations.md)

