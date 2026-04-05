# Task Log: edgecrab/hermes-agent process parity — Phase 2

## Actions
- toolsets.rs: Added `get_process_output`, `wait_for_process`, `write_stdin` to CORE_TOOLS + ACP_TOOLS
- process_table.rs: Added `session_key: String` + `stdin_tx: Option<UnboundedSender<String>>` to ProcessRecord; updated `register()` to 3-arg; added `set_stdin_tx()`, `get_stdin_tx()`, `has_active_for_session()`
- registry.rs (ToolContext): Added `session_key: Option<String>` field
- conversation.rs: Populate `session_key` from `origin_chat` (gateway) or `conversation_session_id` (CLI)
- process.rs: Wired session_key + stdin pipe in RunProcessTool; added `WriteStdinTool`
- agent.rs: Added `gc_cancel: CancellationToken`, wired `spawn_gc_task()` in build(), added `Drop for Agent`
- Fixed delegate_task.rs, execute_code.rs, terminal_backends.rs test for new ToolContext field
- Converted 1 sync test to `#[tokio::test]` (spawn_gc_task requires a Tokio runtime)
- All 385+ tests pass; committed as `32211fb`

## Decisions
- Separate `gc_cancel` token (not sharing the conversation `cancel`) so GC survives `interrupt()` turns
- `session_key` in ToolContext populated from `origin_chat` (gateway) or `conversation_session_id` (CLI) — mirrors hermes pattern without changing the external API
- `write_stdin` takes `newline: Option<bool>` (default true) — matches hermes `submit_stdin` semantics without needing a separate tool

## Next steps
- Deeper audit: agent cache, fallback model activation, smart routing parity
- Consider checkpoint/crash recovery (hermes writes `~/.hermes/processes.json` — lower priority)
- Consider `read_log()` offset pagination as optional enhancement to `get_process_output`

## Lessons
- Always check CORE_TOOLS/ACP_TOOLS when adding new tools — tools registered via inventory compile-time but toolset policy gates LLM visibility
- GC tasks spawned in `build()` require Tokio runtime; sync tests that call `build()` must be `#[tokio::test]`
