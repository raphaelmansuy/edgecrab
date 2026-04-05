# Task logs

- **Actions:** Wired native provider reasoning/token streaming into `edgecrab-core`, surfaced think-mode blocks in the TUI, added regression tests, and cleaned related clippy issues.
- **Decisions:** Kept native streaming gated by `config.streaming` and avoided retry/fallback duplication once partial output has already reached the UI.
- **Next steps:** Use `/reasoning show` in the TUI to verify the live UX manually against a reasoning-capable provider session.
- **Lessons/insights:** The vendor layer already supported `ThinkingContent`; the missing piece was forwarding those deltas cleanly through the ReAct loop and UI state machine.
