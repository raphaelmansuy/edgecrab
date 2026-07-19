# 020 — Tool Streaming Visibility

**Status:** MVP implemented  
 
**Parent:** [002-terminal-ux-ui/010-delightful-tui-plan.md](../002-terminal-ux-ui/010-delightful-tui-plan.md)

## Problem

EdgeCrab already streams tool progress (`ToolProgress` → shelf `detail`). Users still feel blind during long tool runs because **presentation and priority** fail:

- Multi-line stdout tails are flattened into one dim `preview · detail` line
- Long-run charms (“still cooking…”) and leftover provider heartbeats compete for the live band
- Activity section order puts notices above tool evidence
- No foreground expand (only `/tail` for background processes)

## Law

> **During tool execution, tool evidence outranks everything else.**  
> Charms, token footers, and provider SSE heartbeats are secondary. History stays in the transcript; the shelf must show the live evidence.

> **Live = now. History = then.**  
> Soft tool failures and stale provider wait lines must not stick in the live band for the rest of a ReAct turn. See [004-ephemeral-notices.md](004-ephemeral-notices.md).

Preserve the EdgeCrab liveness invariant: keep live tails; do not regress to Hermes lifecycle-only.

## Non-goals

- Full PTY / terminal emulator mirror
- Ink rewrite or merging shelf into scrollable transcript trail
- Changing `/details` config schema
- Gateway / quiet-NDJSON richer streams (later)

## Deliverables

| Doc | Purpose |
|-----|---------|
| [001-signal-hierarchy.md](001-signal-hierarchy.md) | Priority table + disclosure ladder |
| [002-mvp-focus-tool-pane.md](002-mvp-focus-tool-pane.md) | UI states for Focus Tool Pane |
| [003-acceptance.md](003-acceptance.md) | Scenario checklist |
| [004-ephemeral-notices.md](004-ephemeral-notices.md) | Sticky-error / wait-noise fix |

## Code anchors

| Module | Role |
|--------|------|
| `crates/edgecrab-cli/src/activity_shelf.rs` | Multi-line focus pane render |
| `crates/edgecrab-cli/src/turn_activity.rs` | Primary tool, progress buffer, caption, charms |
| `crates/edgecrab-cli/src/process_tail_panel.rs` | Foreground live overlay (shared with `/tail`) |
| `crates/edgecrab-cli/src/status_bar.rs` | `t=expand` hint |
| `crates/edgecrab-tools/src/tool_progress_tail.rs` | Unchanged emit contract (3 lines @ ≤5/s) |
