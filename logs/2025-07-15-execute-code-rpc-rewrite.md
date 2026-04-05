# Task Log: execute_code RPC Rewrite

## Actions
- Rewrote execute_code.rs with Unix domain socket RPC architecture matching hermes-agent
- Added libc dependency for process group management (setpgid, killpg)
- Wired tool_registry through dispatch_single_tool() in conversation.rs
- Added 13 new tests (20 total, all passing), 781 workspace tests pass

## Decisions
- Used libc directly instead of nix crate to minimize new dependencies
- Kept multi-language support (Python, JS, TS, Bash, Ruby, Perl, Rust) — RPC only for Python
- Non-Python languages run without RPC tool access (same as before)
- test_context keeps tool_registry: None — RPC gracefully skipped in tests

## Next steps
- Investigate skill_view 0ms failure on second invocation
- Add skill_should_show() conditional display
- Add env_passthrough for skill-declared env vars
- Build and test end-to-end with a real skill (e.g. youtube-content)

## Lessons/insights
- The critical gap was dispatch_single_tool passing None for tool_registry — a one-line fix that unlocks all RPC functionality
