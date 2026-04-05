# Task Log: Browser Tools Parity Rewrite

## Actions
- Rewrote `crates/edgecrab-tools/src/tools/browser.rs` for hermes-agent parity
- Added: SessionManager (per-task isolation), @eN ref system, `full` snapshot param, LLM summarization, console clear flag + error/warn capture, post-redirect SSRF check, bot detection, CDP Input.dispatchKeyEvent for key presses, inactivity timeout cleanup
- All 39 unit tests pass, full workspace compiles clean

## Decisions
- Kept direct CDP approach (edgecrab's strength) rather than subprocess model
- Used `data-ecref` HTML attributes for @eN ref resolution (JS-based, no CDP Accessibility domain dependency)
- DashMap for concurrent session management, OnceLock for lazy initialization

## Next steps
- Integration test with real Chrome instance if needed
- Consider adding `browser_select_option` tool for `<select>` elements

## Lessons
- CDP's `Input.dispatchKeyEvent` is more reliable than JS KeyboardEvent for form submissions
