# Task logs

- **Actions:** Added a dedicated `/stream` command, unified think/stream preference persistence, fixed the non-streaming fallback UX, and added focused tests for command dispatch and reasoning/stream edge cases.
- **Decisions:** Kept streaming-off mode as a single final answer while still forwarding final reasoning content so think mode works without live token streaming.
- **Next steps:** Optionally smoke-test `/stream off`, `/stream on`, and `/reasoning show` in a live TUI session with a reasoning-capable model.
- **Lessons/insights:** The key UX bug was that “streaming off” still pseudo-streamed final chunks; fixing that at the agent boundary kept the design clean and DRY.
