# Task Log — 2025-07-24 — Wave 1 & Wave 3 TUI Improvements

## Actions
- Implemented P-05: banner enhancements (tools/skills count row + async git update check)
- Implemented P-08: skin browser overlay (`FuzzySelector<SkinEntry>`, `/skin list`, hot-reload)
- Implemented P-09: extracted `tool_display.rs` module (~380 LOC moved from app.rs + 11 unit tests)
- Committed all changes: `0c05402`

## Decisions
- P-08: reused existing `SkinEngine` scanner; only added FuzzySelector overlay on top
- P-09: duplicated `unicode_trunc`/`unicode_pad_right` in tool_display.rs for self-containment
- P-05 update check: fire-and-forget `rt_handle.spawn`; only shows SystemMsg if updates found

## Next Steps
- P-10: push-to-talk voice (cpal + Whisper, feature-gated)
- P-11: local memory store + `/memory` command + optional Honcho cloud sync

## Lessons/Insights
- "frobnicate" contains "cat" → matches file-read icon check; use `unknown_op` for test fallback
- All `cargo check` passes with only pre-existing gateway warnings (no new warnings introduced)
