# 2026-04-01 — Streaming + Tool-Call Display UX Overhaul

## Actions
- Added `WaitingForClarify` to `DisplayState`; wired into `tick_spinner`, `render_status_bar`, `check_responses`
- Added `pending_tool_line_idxs: VecDeque<usize>` to `App` for per-tool in-flight line tracking
- Added `build_tool_running_line()` — cyan `···` placeholder pushed to output on ToolExec
- Added `tool_action_verb()` — maps tool name to "searching"/"executing"/"reading"/… for status bar verb
- Updated `check_responses::ToolExec`: pushes live placeholder + records idx in deque (vs only updating status bar)
- Updated `check_responses::ToolDone`: upgrades placeholder in-place (no layout shift); appends if no pending idx
- Updated `check_responses::Done` and `Error`: clear `pending_tool_line_idxs`
- Updated `check_responses::Clarify`: sets `WaitingForClarify` (not `Idle`)
- Updated `render_status_bar::ToolExec`: uses `tool_action_verb()` + parallel count when `in_flight_tool_count > 1`
- Updated right-side hints: shows `↵=send reply` when `clarify_pending_tx.is_some()`

## Decisions
- FIFO VecDeque for parallel tool tracking (pop_front on ToolDone matches LLM dispatch order)
- In-place upgrade avoids layout shift and double-line for same tool call
- `WaitingForClarify` separate from `Idle` prevents "agent hung?" confusion during interactive Q&A
- Tool-specific verbs in status bar only (not output area) to avoid verbosity

## Next steps
- Consider `result_preview: Option<String>` in `StreamEvent::ToolDone` for inline result excerpt
- Consider collapsible reasoning block (expand on keypress)
- Consider streaming-while-scrolled indicator when content arrives while user is scrolled up

## Lessons/insights
- Exhaustive Rust match arms require updating ALL sites when adding an enum variant; always grep `DisplayState::` first
- In-place span upgrade needs `output[idx].rendered = None` to invalidate the ratatui render cache
