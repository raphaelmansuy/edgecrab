# ADR-003: Cross-Platform Command and Path Parsing

Status: accepted

## Context

The TUI `/mcp` command handler currently tokenizes arguments with whitespace splitting.

That is not sufficient for:

- quoted values
- Unix paths with spaces
- Windows paths
- `--path` / `--name` style options

## Decision

EdgeCrab will use a quote-aware parser for inline TUI MCP commands and support both:

- `key=value`
- `--key value`

The parser will preserve backslashes by default so Windows paths are not mangled.

## Why

1. Windows support is a product requirement, not a nice-to-have.
2. Shell-style parsers that reinterpret backslashes with POSIX rules are a bad fit for Windows path literals.
3. The TUI is not a shell. It should implement the small amount of parsing it actually needs and no more.

## Consequences

- `/mcp install filesystem --path "C:\Users\Me\My Project"` must work.
- `/mcp install filesystem path="/Users/me/My Project"` must work.
- Unmatched quotes return explicit usage errors.
- Shared parsing helpers are required so TUI MCP command handling stays DRY.

## References

- `crates/edgecrab-cli/src/app.rs`
- [roadblocks.md](./roadblocks.md)
