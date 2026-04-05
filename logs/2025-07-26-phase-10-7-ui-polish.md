# Task Log: Phase 10.7 UI Polish

## Actions
- Changed default prompt symbol from `❯` to `>` in theme.rs
- Replaced all 3 `❯` user echo instances in app.rs with `>`
- Changed startup banner from multi-line `BANNER` const to single-line `EdgeCrab v0.1.0-dev · {model}`
- Fixed `default_theme_builds` test assertion for new prompt symbol
- Verified color scheme already consistent (cyan=tool exec, yellow=thinking, green=streaming, red=error, grey=system)
- Verified no emoji in TUI box-drawing contexts (emoji only in CLI print: setup, doctor, migrate)
- Updated plan.md checkboxes for Phase 8.3, 10.3-10.8

## Decisions
- Kept emoji in CLI output (setup ✅, doctor 🔍, migrate 🚀) — these are println, not TUI rendering
- Banner now includes model name dynamically via format string instead of static const
- Pre-existing clippy warnings (too_many_arguments, dead_code) not addressed — noted in plan.md

## Results
- 525 tests passed, 0 failed, 5 ignored
- Build clean (only pre-existing skin_engine dead_code warnings)
- Clippy clean except pre-existing too_many_arguments + dead_code

## Files Modified
- `crates/edgecrab-cli/src/theme.rs` — prompt_symbol default `❯` → `>`
- `crates/edgecrab-cli/src/app.rs` — 3x `❯` → `>` in push_output calls + comment
- `crates/edgecrab-cli/src/main.rs` — banner push uses format string with model
- `mission/plan.md` — checked off Phase 8.3, 10.3-10.8 items
