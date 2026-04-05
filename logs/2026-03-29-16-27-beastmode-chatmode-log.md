Task logs

Actions: Added runtime mouse capture toggle in TUI via F6, Ctrl+M, and /mouse on|off|toggle|status; updated status/help UX hints; added unit tests for command dispatch and mouse mode state transitions; ran edgecrab-cli crate tests and diagnostics.
Decisions: Implemented selection support by toggling terminal mouse capture rather than simulating in-widget selection, ensuring native terminal copy/paste behavior across macOS terminals.
Next steps: Optionally add transcript search/jump and click-to-focus zones for model selector/output panes for another UX iteration.
Lessons/insights: Reliable terminal text selection in ratatui/crossterm is best achieved by temporarily disabling mouse capture and clearly signaling mode state to users.
