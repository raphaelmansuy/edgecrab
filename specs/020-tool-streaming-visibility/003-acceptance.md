# 003 — Acceptance

## Scenarios

| ID | Scenario | Pass criteria |
|----|----------|---------------|
| A1 | Long npm/cargo | Within 1s of first `ToolProgress`, shelf shows ≥1 real stdout line (not only “still cooking…”) |
| A2 | Quiet transcript | Shelf on + verbose off → scrollback quiet; shelf stays live |
| A3 | Parallel tools | ≥2 headers; only primary has multi-line body; `+N` honest |
| A4 | Compact width | Caption prefers `terminal · last-line · elapsed`, not provider diagnostics |
| A5 | Expand | Empty input + `t` opens overlay with more output; Esc closes; `^C` still stops |
| A6 | Calm | Coalesce unchanged; no scrollback flood; charms do not replace evidence |
| E1–E5 | Ephemeral notices | See [004-ephemeral-notices.md](004-ephemeral-notices.md) — no sticky tool fails / Copilot wait graffiti |

## Unit tests (anchors) — implemented

- `activity_shelf`: `focus_pane_counts_multiline_body`, `parallel_tools_only_primary_gets_body_budget`, `secondary_activity_notices_detected`, compact caption
- `turn_activity`: `charm_skipped_when_detail_has_evidence`, `progress_log_accumulates_and_caps`, `compact_caption_uses_last_detail_line`, `focus_tool_body_lines_keeps_last_three`, primary = longest elapsed, `on_model_resuming_purges_llm_wait_feed_lines`, `llm_wait_label_no_feed_fallback`, `error_notices_expire_after_ttl`, `awaiting_model_caption_without_stale_wait`
- `process_tail_panel`: `foreground_live_sets_flag`

## Manual smoke

1. Run a long `terminal` tool (e.g. `npm install` or `cargo build --workspace`)
2. Confirm shelf shows header + 2–3 stdout lines updating in place
3. Press `t` with empty prompt → overlay; Esc → back
4. Confirm status shows `t=expand` after ~3s
