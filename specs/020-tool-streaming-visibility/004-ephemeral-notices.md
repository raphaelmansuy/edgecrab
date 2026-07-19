# 004 — Ephemeral Live Notices

**Extends:** [000-overview.md](000-overview.md) · Focus Tool Pane MVP

## Law

> **Live = now. History = then.**  
> Tool failures live in the transcript. The shelf may hold a notice briefly, then must return to the current phase signal. Provider wait detail belongs in the status-bar compact label / active `llm_wait_detail`, not as permanent shelf graffiti.

## Root cause (fixed)

`activity_feed` was append-only until `reset_turn` (end of user turn). Soft `ToolDone` errors and Copilot SSE wait lines therefore stuck for the whole ReAct loop, even with Activity=`Hidden` (Warn/Error still force-shown).

## Behavior

| Event | Live shelf |
|-------|------------|
| Soft tool fail (`ToolDone` is_error) | Transcript ✗ only — **no** activity_feed push |
| Error/Warn/Info notice | TTL: Error 8s, Warn/Info 12s, then drop |
| `on_model_resuming` / next `ToolExec` | Purge LLM-wait feed lines; clear `llm_wait_*` |
| `llm_wait_label()` | Returns **only** active `llm_wait_detail` (no feed fallback) |
| Between tools (`AwaitingFirstToken`, no detail) | Calm `awaiting model response (Ns)` |
| Activity Summary | Newest notice first |

## Constants

- `NOTICE_ERROR_TTL_SECS = 8`
- `NOTICE_WARN_TTL_SECS = 12`
- `NOTICE_INFO_TTL_SECS = 12`

## Code anchors

- [`turn_activity.rs`](../../crates/edgecrab-cli/src/turn_activity.rs) — `ActivityNotice::created_at`, `expire_notices`, resume purge
- [`response_dispatch.rs`](../../crates/edgecrab-cli/src/app/response_dispatch.rs) — no ToolDone Error push
- [`activity_shelf.rs`](../../crates/edgecrab-cli/src/activity_shelf.rs) — newest-first `visible_notices`

## Acceptance

| ID | Scenario | Pass |
|----|----------|------|
| E1 | Soft tool fail | Transcript ✗; no sticky “skill view failed” on shelf |
| E2 | Between tools after resume | No `vscode-copilot: iter…` from purged feed |
| E3 | Error TTL | Error notice gone after 8s (`expire_notices`) |
| E4 | Active tool stdout | Focus pane still primary (020 unchanged) |
| E5 | New user turn | `reset_turn` still wipes feed |
