# Task Log — 2026-03-31 browser_type fix

## Actions
- Analysed `browser_type` in `browser.rs` vs hermes-agent's `browser_tool.py`
- Identified root cause: direct `el.value` assignment + generic `Event('input')` breaks React controlled components
- Rewrote `BrowserTypeTool::execute` to use two-step CDP approach: JS focus+select-all → `Input.insertText`
- All 315 tests pass; 88 browser tests pass; 0 failures

## Decisions
- Use CDP `Input.insertText` (Playwright's approach) instead of JS `el.value` hacks
- Keep select-all step in JS so `insertText` replaces existing content cleanly
- Add 100ms pause post-type for framework state propagation

## Root Cause Summary
| Bug | Old | New |
|---|---|---|
| React bypass | `el.value = ''` + `el.value = text` | `el.select()` → `Input.insertText` |
| Wrong event | `new Event('input')` (generic) | CDP-level `InputEvent` via `insertText` |
| No clear | Cleared via `.value = ''` | Replaced via select-all before insert |

## Next Steps
- Test form fill on elitizon.com to verify React form fields now register values in state
- If submit still fails, check whether the submit button needs a real mouse click via CDP `Input.dispatchMouseEvent`

## Lessons
- CDP `Input.insertText` is the only reliable way to type into React/Vue/Angular controlled inputs
- Playwright's `fill()` source confirms this: focus → select-all → `insertText`, no JS value hacks
