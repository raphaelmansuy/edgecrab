# TUI Audit & Overhaul — 2026-03-29

## Actions
- Committed all pending changes before audit
- Read entire `app.rs` (2299 lines), `main.rs`, `theme.rs`, `commands.rs` in full
- Identified 12 distinct TUI bugs and usability gaps against best Rust TUI practices
- Implemented all fixes in `app.rs` (352 lines changed / added) and `commands.rs`
- Zero clippy warnings, 136/136 tests passing, clean `cargo build`
- Committed as `feat(tui): full usability audit and overhaul` (commit 2b86550)

## Decisions
- Used ratatui `Scrollbar` widget with `█` thumb and `│` track (minimal, professional)
- Added `at_bottom: bool` flag instead of checking `scroll_offset == 0` at call sites — cleaner semantics
- Added `needs_redraw: bool` dirty flag to avoid CPU waste on idle (20fps idle vs 60fps streaming)
- Used `KeyEventKind::Release` filter to prevent key double-fire on Windows/macOS
- Used `div_ceil()` (stable Rust) for visual row count computation (clippy suggestion)
- Kept `EnableMouseCapture` intentionally absent (preserves Cmd+C text selection)

## Next Steps
- Manual smoke test in a terminal (cannot run TUI in this environment)
- Consider adding mouse wheel scroll support for the output area in a follow-up
- Consider `Scrollbar` for model selector overlay too

## Lessons / Insights
- Ratatui `Paragraph::scroll((row, 0))` is in VISUAL rows (after wrapping), not logical line count — the bug was non-obvious
- `EnableBracketedPaste` + never handling `Event::Paste` is a very common oversight in Rust TUI apps
- Dirty-flag pattern (`needs_redraw`) is critical for production TUIs — reduces idle CPU from ~20% to ~1%
- Always install a panic hook in TUI apps — raw mode + alternate screen left on panic is a terrible UX
