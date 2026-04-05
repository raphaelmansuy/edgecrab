# 2026-04-01 Streaming & Tool Display UX Overhaul

## Actions
- Audited full streaming state machine in `app.rs` (7447 lines) via first-principles analysis
- Fixed critical bug: `streaming_line` was not reset on `ToolExec`, merging pre/post-tool text
- Added `in_flight_tool_count: u32` for parallel tool tracking
- Added `turn_stream_tokens: u64` for cumulative token count across streaming phases
- Updated `render_status_bar` ToolExec arm to show elapsed time + `^C=stop` hint
- Updated `render_status_bar` Streaming arm: rate only shown after 5+ tokens and 1s elapsed
- Updated Clarify handler: textarea border changes to amber "❓ Reply:" label
- Reset clarify border to normal on user reply dispatch
- All per-turn counters zeroed on `Done` and `Error` events
- `cargo build`, `cargo test` (931+ tests all green), `cargo clippy` (no new warnings in edgecrab-cli)
- Committed as `07f84a4`

## Decisions
- `streaming_line = None` on ToolExec is the minimal correct fix for text-bleed across tool boundaries
- Cumulative `turn_stream_tokens` initialized into each new Streaming phase (not reset to 0) for UX continuity
- Rate display gated on `elapsed > 1.0s && token_count > 5` to avoid "0t/s" startup flicker
- ToolExec elapsed time mirrors Thinking state thresholds (3s → show elapsed, 10s → show stop hint)

## Lessons/Insights
- The streaming buffer (`streaming_line`) must be broken at every tool-call boundary; this is a fundamental invariant for correct temporal ordering of output
- Parallel tool calls require a counter, not just last-wins state, for accurate status bar display
- Token rate needs a minimum sample window; raw instantaneous rate is too noisy to display meaningfully
