# Task Log: TUI Input Tab Completion Improvements

**Date:** 2026-04-01

## Actions
- Fixed `update_completion()` to split input at first whitespace, use only the command token for prefix/fuzzy matching, and suppress the overlay when an exact command is already typed with arguments.
- Fixed `accept_completion()` to preserve the argument tail (everything after the command token) so accepting a completion never discards typed arguments.
- Added `ghost_hint_next_word()` helper that returns one whitespace-delimited word from the ghost hint.
- Rewrote Tab key handler: Tab at EOL with a ghost hint now accepts the full suggestion (fish-shell priority); Tab without a ghost hint opens command completion; Tab in multi-line mode inserts a literal tab.
- Added `Alt+Right` handler: accepts one word at a time from the ghost hint.
- Added `End` key handler: when cursor is already at EOL and a ghost hint is available, accepts it; otherwise falls through to normal End behaviour.
- Added/updated 6 new focused unit tests covering all new behaviours.

## Decisions
- Ghost hint acceptance has higher priority than command completion on Tab (mirrors fish-shell UX).
- `accept_completion` always preserves arg suffix; the arg tail includes the space separator.
- `update_completion` does not activate for complete-command+args to avoid accidental overlay pop-ups.

## Next Steps
- Manual smoke-test in a live terminal session.
- Consider argument-level completion (e.g. model names for `/model <TAB>`) as a future enhancement.

## Lessons / Insights
- `/mem` is a registered alias for `/memory`; tests using aliases as "partial" prefixes fail the exact-match guard — use truly partial prefixes (e.g. `/hel`) for those tests.
