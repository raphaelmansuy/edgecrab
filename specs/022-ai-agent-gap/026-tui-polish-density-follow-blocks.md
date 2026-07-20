# 026 — TUI Polish: Density · Follow · Typed Blocks (post-024)

**Status:** Implemented — Waves A–G (2026-07-20)  
**Date:** 2026-07-20  
**Authority:** First principles · DRY · SOLID · e2e-first  
**Reference:** Grok Build pager (craft) · Claude Code / OpenCode (2026 operator expectations)  
**Prior:** [024](024-tui-stream-ux-from-grok-build.md) W1–W3 + light W4 landed  
**Target:** `crates/edgecrab-cli`

---

## 0. Intent

Make EdgeCrab’s TUI feel Grok Build–class in density and stability while keeping Hermes shelf + `/details`, security, and skin ownership.

**Non-goals:** Vendor `xai-grok-pager`. Big-bang delete of `OutputLine`. Soft “looks done” UI.

**Success:** In a coding turn you always know phase, tool identity, live progress, and file deltas — history stays dense; one key recovers bodies; scroll/resize never fight the stream.

---

## 1. Waves

| Wave | Focus | Priority |
|------|--------|----------|
| **A** | Idle chrome collapse, turn-status row, tool-usage strip, shared phase labels | P0 |
| **B** | `RenderEntry` sum + uniform `render(mode, width)` + `OutputLine` adapter | P0 |
| **C** | Disclosure defaults, expand/collapse keys, muted collapsed, `/details` bridge | P0 |
| **D** | Checkpointed streaming markdown + wrap cache + deferred coalesce | P1 |
| **E** | `FollowMode`, re-engage, sticky user header, resize-safe heights | P1 |
| **F** | Per-tool craft, skin tokens, optional `edit-syntax` | P1 |
| **G** | Harness + golden `tui_stream_ux_e2e` | throughout |

---

## 2. Architecture

```text
StreamEvent
  → stream_presentation (sole lifecycle owner)
      → RenderEntry { Thinking | Tool | Agent | VerbGroup | Footer | User }
           DisplayMode + status + body buffer
  → activity_shelf   (live projection)
  → transcript paint (finished + live via one render path)
  → chrome           (turn-status / usage strip / follow indicator)
```

**Law:** Model events are facts. Presentation is a state machine of entries. Paint stays out of `app.rs` beyond dispatch hooks.

---

## 3. Denylist

- Depend on / copy `xai-grok-pager`
- Second `TurnPhase` / activity enum in shelf
- Uncapped tool stdout in RAM
- Paint logic growth in `app.rs` beyond thin dispatch
- Soft “looks done” UI
- Big-bang delete of `OutputLine` in the first PR

---

## 4. Acceptance (operator)

- [x] Idle chat is mostly transcript + input (shelf 0-height when empty)
- [x] During a tool run, one clear activity sentence is visible (`turn_status_row` + shared phase labels)
- [x] Tool-usage strip: `Exec N · Read N · Edit N · Search N`
- [x] Expanding thinking and tool cards share the same mode/key path (`DisplayMode` + `e`/`E`/`c`)
- [x] Follow mode: browsing does not yank viewport; `G` / Ctrl+G / send re-engages
- [x] Long streamed reply stays O(tail) re-render (`StreamingMarkdown` checkpoints + wrap cache)
- [x] Skin tokens: thinking / insert / delete / tool_error documented in `theme.rs` / skin.yaml

---

## 5. Relationship

| Spec | Role |
|------|------|
| **015 / 016** | Edit display diagnosis + hunks |
| **024** | Stream UX cards (thinking · tools · files) — base |
| **026 (this)** | Density, follow, typed blocks, streaming stability |

*Code is law: Grok Build is the reference for perception; EdgeCrab keeps security, harness, and skin ownership.*
