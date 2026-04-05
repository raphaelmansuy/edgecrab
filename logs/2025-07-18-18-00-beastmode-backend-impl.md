# Task Log — 2025-07-18-18-00 — Backend Implementation Completion

## Actions
- Fixed integration test `ctx_in→make_ctx` rename and manual ToolContext construction (avoiding `#[cfg(test)]` gating)
- Fixed `e2e_stderr_captured`: `{ cmd; } 2>&1` merges stderr into stdout in persistent shell
- Fixed `e2e_working_directory_respected` 120s hang: added `\n` before sentinel in printf to prevent no-newline output from merging with sentinel in BufReader::read_line
- Removed unused `std::io::Write` import from local.rs
- Used `cargo fix --allow-dirty` to auto-remove 5 other stale imports

## Decisions
- Manual ToolContext construction in integration tests preferred over `test-helpers` feature flag (simpler, no crate changes)
- `{ cmd; } 2>&1` approach: group command (not subshell) so exports/cd persist; stderr merged at bash level
- `printf '\n{sentinel}\n'` not `printf '{sentinel}\n'`: the leading `\n` ensures BufReader sees sentinel on its own line even when command output has no trailing newline

## Next Steps
- Optionally extend `TerminalConfig` in `edgecrab-core/src/config.rs` to surface backend selection in TOML config
- Docker/SSH/Modal integration tests skipped unless test env vars set; CI should gate those with matrix

## Lessons/Insights
- `BufReader::read_line()` merges content until it hits `\n`; sentinel protocol must guarantee a `\n` line boundary before the sentinel token, even for commands that don't end their output with `\n`
- `{ cmd; } 2>&1` group command (curly braces) runs in current shell — does NOT create subshell — all environment mutations persist; `( cmd ) 2>&1` would lose exports
