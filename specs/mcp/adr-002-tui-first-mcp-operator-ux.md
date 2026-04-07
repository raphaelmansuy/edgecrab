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
- `/mcp search [query]` opens a dedicated remote browser for official upstream sources and the official MCP Registry.
- Configured entries get explicit health-oriented actions, including doctor/check.
- Configured HTTP entries also need an explicit auth explanation path because OAuth failures are not self-describing from a generic probe error.
- Configured HTTP entries with interactive OAuth must support a first-class login flow (`/mcp login`) rather than pushing the operator into manual refresh-token extraction.
- Browser-loopback OAuth must tolerate real operator environments, including busy local ports; dynamic loopback redirect ports are part of the supported operator path.
- The selector must surface meaningful metadata, not just names.
- Remote search must keep source provenance visible so users can distinguish steering-group references, official integrations, archived entries, and registry listings.
- Search breadth and install breadth are allowed to differ when EdgeCrab cannot launch a registry entry deterministically yet.
- TUI command parsing must be robust enough to accept real-world quoted values.

## References

- `crates/edgecrab-cli/src/app.rs`
- [tui-and-operations.md](./tui-and-operations.md)
