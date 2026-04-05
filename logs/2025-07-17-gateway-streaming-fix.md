# Gateway Streaming Fix — 2025-07-17

## Summary
Wired progressive streaming into the edgecrab gateway dispatch loop so users
receive incremental updates instead of a single final message.

## Files Modified
- `crates/edgecrab-gateway/src/config.rs` — added `GatewayStreamingConfig`
- `crates/edgecrab-gateway/src/platform.rs` — added `supports_editing()`, `edit_message()`, `send_and_get_id()`, `send_status()` to `PlatformAdapter` trait
- `crates/edgecrab-gateway/src/telegram.rs` — implemented `editMessageText`, `send_message_with_id`, `supports_editing()→true`
- `crates/edgecrab-gateway/src/stream_consumer.rs` — rewritten: edit-mode (Telegram) + batch-mode (WhatsApp/Signal)
- `crates/edgecrab-gateway/src/event_processor.rs` — NEW: routes StreamEvents (Reasoning, ToolExec, Token, Done, Error, Clarify) to platform actions
- `crates/edgecrab-gateway/src/lib.rs` — added `pub mod event_processor`
- `crates/edgecrab-gateway/src/run.rs` — dispatch loop now uses `dispatch_streaming_arc()` for streaming or blocking fallback
- `crates/edgecrab-core/src/agent.rs` — added `chat_streaming_with_origin()`

## Design
- **Edit-capable platforms (Telegram)**: progressive token streaming via `edit_message()` every 300ms, cursor indicator during generation
- **Non-edit platforms (WhatsApp, Signal)**: silent accumulation → single final message (no flooding)
- **Tool progress**: "🔧 tool_name…" status messages during agent execution (opt-in via config)
- **Reasoning**: optional status messages (disabled by default via `show_reasoning: false`)
- **Overflow >4096 chars**: split into sequential message bubbles
- **Already-sent guard**: `AtomicBool` prevents duplicate delivery between stream consumer and run.rs

## Test Results
- 92 tests passed, 0 failed
- Full workspace build: clean (no errors, no warnings)
