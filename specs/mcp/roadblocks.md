# EdgeCrab MCP Roadblocks and Edge Cases

This document tracks the real failure modes implied by the current code. See [architecture.md](./architecture.md) for the design boundary and [ADR-003](./adr-003-cross-platform-command-and-path-parsing.md) for the parsing decision.

## 1. Quoted Paths and Windows Paths in the TUI

Severity: HIGH

Problem:

- `handle_mcp_command()` currently tokenizes with `split_whitespace()`.
- This breaks quoted values.
- This breaks paths containing spaces.
- This is especially dangerous on Windows, where project directories often contain spaces and backslashes.

Mitigation:

- Use a quote-aware inline command tokenizer in the CLI crate.
- Preserve backslashes by default; do not assume POSIX shell escaping rules.
- Support both `key=value` and `--key value` forms.

Residual risk:

- Unmatched quotes should return a clean usage error, not partial execution.

## 2. Stdio Command Resolution Is More Subtle Than PATH Lookup

Severity: MEDIUM

Problem:

- Some commands are plain executables resolved via `PATH`.
- Others may be relative or absolute paths.
- `which::which()` is not the whole story for `./server`, `bin/server`, or Windows executable paths.

Mitigation:

- Diagnose path-like commands separately from PATH lookups.
- Report whether a configured command is path-based or PATH-based.

## 3. `cwd` Can Be Present but Invalid

Severity: HIGH

Problem:

- Current config permits `cwd`.
- If `cwd` is missing or is a file rather than a directory, stdio spawn fails late.

Mitigation:

- Doctor flow must stat `cwd` before probe.
- Report `missing`, `not a directory`, or `ok`.

## 4. HTTP Auth Precedence Can Be Confusing

Severity: MEDIUM

Problem:

- Auth can come from config `bearer_token`, token store, or custom headers.
- Custom headers may override `Authorization`.
- Users can think auth is configured while the effective request is different.

Mitigation:

- Doctor must render the effective auth source clearly.
- When both bearer token and `Authorization` header exist, the output must state the override risk.

## 5. Include/Exclude Filters Can Hide Everything

Severity: MEDIUM

Problem:

- `tools.include` and `tools.exclude` already exist.
- A user can configure a working server and still end up with zero visible tools.

Mitigation:

- Doctor output must show filter summary.
- A successful transport probe with zero visible tools should not be rendered as unqualified success.

## 6. Cached Connections Can Become Stale

Severity: MEDIUM

Problem:

- MCP connections are cached in a process-global `DashMap`.
- External servers can restart independently.

Mitigation:

- Keep `/reload-mcp`.
- Prefer diagnostics that tell the user to reload when probe failures are likely due to stale connections.

## 7. Catalog Install Success Does Not Equal Runtime Success

Severity: LOW but common

Problem:

- Installing a preset only writes config.
- Runtime success still depends on external binaries, API keys, network reachability, and server behavior.

Mitigation:

- Every install success message should direct the user to `mcp test` or `mcp doctor`.

