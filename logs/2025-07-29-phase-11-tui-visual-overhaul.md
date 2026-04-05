# Task log — Phase 11 TUI visual overhaul

## Actions
- Added THINKING_VERBS[15] const + tool_icon() fn to app.rs
- Added thinking_verb_idx/turn_count fields to App struct
- tick_spinner() now advances thinking_verb_idx on each full braille rotation
- render_output() refactored: role-based left accent bars + turn separators
- render_status_bar() updated: EC brand badge, thinking verbs, tool icons, turn counter
- markdown_render.rs: language badge on code fence opening
- main.rs: box-drawing welcome banner with model name

## Decisions
- Used single-width Unicode bar char for accent bars to avoid emoji width issues
- System messages use dot prefix to distinguish from conversation turns
- Tool icons use safe ASCII-area Unicode: search, terminal, write, file, memory, agent, etc.
- Turn separators only injected before user messages that follow prior content

## Next steps
- None pending; 136/136 tests pass, 0 clippy warnings, committed c7775c4

## Lessons
- Keep git commit messages short/ASCII-only to avoid PTY garbling with Unicode
- Two-pass render (mutable render + immutable line-build) cleanly avoids borrow conflicts in Rust
