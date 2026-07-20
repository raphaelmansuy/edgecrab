# 024 — TUI Stream UX: Tools · Files · Thinking (Grok Build–inspired)

**Status:** Plan — **W1–W3 + light W4 implemented** (2026-07-19): TurnPhase chrome owner, thinking cards, live tool/edit cards, Read/Search verb groups, Worked-for footer, session edit ledger  
**Date:** 2026-07-19  
**Authority:** First principles · DRY · SOLID · e2e-first  
**Reference tree:** `/Users/raphaelmansuy/Github/03-working/grok-build`  
**Target tree:** `crates/edgecrab-cli` + thin `StreamEvent` extensions in `edgecrab-core`  
**Related:** [015 code-edit display](015-grok-build-tui-code-display.md) · [016 edit plan (A–C landed)](016-tui-edit-display-plan.md) · [008 TUI operator](008-tui-operator-lens.md) · specs `002-tui-hemes-vs-edgecrab/` · `020-tool-streaming-visibility/`

---

## 0. Intent

Make EdgeCrab’s TUI **feel as clear as Grok Build** while the agent:

1. **Thinks** (reasoning stream)  
2. **Runs tools** (especially terminal / long I/O)  
3. **Edits files** (write / patch with legible diffs)

**Non-goals:** Vendor `xai-grok-pager`. Fork the full scrollback engine. Depend on grok-build crates.

**Success signal (operator):** During a typical coding turn you always know *phase* (thinking / tool / reply), *what tool*, *live progress*, and *what files changed* — without reading raw JSON or drowning in monochrome dumps.

---

## 1. First principles (perception jobs)

| ID | Job | Grok Build mechanism | EdgeCrab today |
|----|-----|----------------------|----------------|
| **J1** | Phase is obvious | `TurnActivity::{Thinking, ToolRunning, Responding}` + title | `ShelfPhase` + `DisplayState` — good, partially dual |
| **J2** | Tool identity at a glance | Typed `ToolCallBlock` + header range | `tool_display` captions — good captions, flat lines |
| **J3** | Live tool body streams | `ExecuteToolCallBlock` push chunks, Truncated while run | `ToolProgress` + shelf focus pane (3 lines) — thin |
| **J4** | File edits are first-class | `EditToolCallBlock` + progressive HL | `edit_diff` hunks + verb group (**016 A–C**) — solid base |
| **J5** | Thinking is a block, not noise | `ThinkingBlock` Collapsed / Truncated / Expanded | Shelf thinking + optional transcript spinner — split |
| **J6** | Collapse by default | `DisplayMode` on every block | `expandable_body` — works; not uniform per kind |
| **J7** | Session stats | `tool_usage` bar, hunk tracker | Partial shelf tokens; no session tool bar |
| **J8** | One paint path | Block `output(ctx)` | Shelf + transcript + display_state **three owners** |

**Law:** Model stream events are **facts**. Presentation is a **state machine of blocks**, not ad-hoc string appends.

---

## 2. Architecture comparison (code is law)

### Grok Build (reference)

```text
Stream / agent events
        │
        ▼
scrollback::RenderBlock  (sum type)
  Thinking | AgentMessage | ToolCall(Edit|Execute|Read|Search|…) | …
        │
        ├─ DisplayMode: Collapsed | Truncated | Expanded
        ├─ Lifecycle: streaming start → append → finish
        └─ VerbGroupKind: fold consecutive Reads/Edits/Searches

turn_status / title bar  ← TurnActivity (Thinking / ToolRunning / …)
tool_usage               ← category stats bar
```

Key files:

| Area | Path (approx) |
|------|----------------|
| Block sum | `xai-grok-pager/src/scrollback/block.rs` |
| Thinking | `…/blocks/thinking.rs` (`streaming()`, Truncated last N) |
| Execute | `…/blocks/tool/execute.rs` (stdout chunks, user bash vs agent) |
| Edit | `…/blocks/tool/edit.rs` + `pager/src/diff.rs` |
| Verb groups | `…/blocks/tool/mod.rs` `VerbGroupKind` |
| Turn activity | `notifications/title.rs` `TurnActivity` |

### EdgeCrab (current)

```text
StreamEvent (core)
  Token | Reasoning | ToolGenerating | ToolExec | ToolProgress | ToolDone | …
        │
        ├─ turn_activity / activity_shelf   (live phase + 3-line tool tail)
        ├─ display_state / transcript       (spinner + OutputLine history)
        └─ edit_diff / tool_display         (post-hoc edit cards + captions)
```

| Strength | Gap vs Grok |
|----------|-------------|
| Rich `StreamEvent` already | No **typed transcript block** lifecycle for tools |
| Activity shelf (Hermes-inspired) | Shelf ≠ scrollback; thinking not a durable block |
| `edit_diff` hunks + verb groups | Execute tools not first-class stream cards |
| Expand body on some lines | Modes not uniform (Collapsed/Truncated/Expanded) |
| Skin YAML | Diff/thinking colors not fully semantic |

---

## 3. Target architecture (SOLID · DRY)

```text
┌─────────────────────────────────────────────────────────────────────────┐
│ edgecrab-core                                                           │
│  StreamEvent (facts only — extend carefully, version-stable)           │
│    + ToolStdoutChunk { tool_call_id, text }   // optional P1           │
│    + Reasoning (already)                      // keep                   │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────────────────┐
│ edgecrab-cli                                                            │
│                                                                         │
│  presentation/   ← NEW cluster (or grow existing modules with clear S)  │
│    turn_phase.rs       Single TurnPhase (maps StreamEvent → phase)      │
│    blocks/             Typed scrollback entries (in-memory)             │
│      thinking.rs       ThinkingCard: stream, modes, duration            │
│      tool_card.rs      ToolCard: kind, status, body buffer              │
│      edit_card.rs      wraps edit_diff::EditPresentation                 │
│      execute_card.rs   rolling stdout (from ToolProgress / chunk)       │
│    render/             Block → ratatui Lines (DisplayMode)              │
│    verb_group.rs       Extend existing EditVerbGroup to Read/Search     │
│                                                                         │
│  turn_activity.rs      SHELF only — reads same TurnPhase + live cards   │
│  transcript.rs         History of finished BlockIds → OutputLine        │
│  stream_dispatch_*     Thin: event → presentation update → dirty flag   │
│  app.rs                NO paint logic                                   │
└─────────────────────────────────────────────────────────────────────────┘
```

| SOLID | Rule |
|-------|------|
| **S** | Phase state ≠ card body buffer ≠ ratatui paint |
| **O** | New tool kinds register `ToolCardKind` + caption extractor |
| **L** | Any card implements `render(mode, width) → Vec<Line>` |
| **I** | Shelf needs `caption + 3 tail lines`; transcript needs full card |
| **D** | Dispatch depends on presentation trait, not reverse |

| DRY | Rule |
|-----|------|
| One path extract | `tool_display` / edit paths — already shared; keep |
| One edit hunk model | `edit_diff` only — cards call it |
| One phase enum | `TurnPhase` drives shelf **and** status chrome |
| One expand mode | `DisplayMode` on cards; map to `expandable_body` for compat |

---

## 4. Gap matrix (what to build next)

| Area | Status | Priority |
|------|--------|----------|
| Edit hunks + stats + verb group | **Done** (016 A–C in `edit_diff.rs`) | Maintain |
| Thinking as **durable transcript card** | Partial (shelf only) | **P0** |
| Unified `DisplayMode` for thinking/tools/edits | Partial | **P0** |
| Execute tool **live body** in transcript card | Shelf 3-line only | **P0** |
| Single `TurnPhase` owner | Dual shelf/display_state | **P0** |
| Tool category bar (Read/Edit/Exec counts) | Missing | **P1** |
| Session edit ledger strip | Missing (016 E) | **P1** |
| Syntax HL on diffs | Missing (016 D) | **P1** |
| Typed `RenderBlock` sum in transcript | Flat `OutputLine` | **P1** (evolve, don’t big-bang) |
| Syntect progressive HL like Grok | Out of scope P0 | **P2** |

---

## 5. Implementation waves

### Wave 0 — Baseline freeze (0.5 day)

| ID | Work |
|----|------|
| W0.1 | Inventory: document `StreamEvent` → shelf → transcript paths in a one-pager (this file §2) |
| W0.2 | Snapshot golden screenshots / line dumps of: thinking, terminal tool, write_file, patch |
| W0.3 | Guard: `cargo test -p edgecrab-cli --lib edit_` green before any change |

### Wave 1 — Unified phase + thinking card (P0, ~3 days)

**Goal:** One phase source; thinking looks intentional, not a glitch.

| ID | Work | Owner |
|----|------|-------|
| W1.1 | Introduce `TurnPhase { Idle, Thinking, Tool { id, name }, Responding }` in `presentation/turn_phase.rs` | cli |
| W1.2 | Map all `StreamEvent` variants → phase transitions in **one** function; shelf + display_state call it | cli |
| W1.3 | `ThinkingCard`: accumulate `Reasoning` chunks; finish on first Token/ToolExec; store duration | cli |
| W1.4 | Modes: Collapsed (“Thought · 2.1s”) / Truncated (last 6 lines) / Expanded (cap 80 lines) | cli |
| W1.5 | Persist finished thinking into transcript as expandable line (not only shelf) | transcript |
| W1.6 | `/details thinking` already exists — wire to card mode | shelf_details |
| W1.7 | Unit + harness: reasoning stream → phase Thinking → card Truncated | tests |

**Grok borrow:** `ThinkingBlock::streaming()`, default Truncated last N, collapse header “Thought for Xs”.

### Wave 2 — Tool cards with live body (P0, ~4 days)

**Goal:** Terminal and long tools feel live in **both** shelf and history.

| ID | Work | Owner |
|----|------|-------|
| W2.1 | `ToolCard` state machine: Generating → Running → Done/Error | cli |
| W2.2 | On `ToolExec`: open card (header caption from `tool_display`) | dispatch |
| W2.3 | On `ToolProgress`: append to rolling buffer (4KB, line-capped) — same buffer for shelf + card | cli |
| W2.4 | On `ToolDone`: finalize header (✓/✗, duration, stats); body becomes expandable | cli |
| W2.5 | **Execute specialization:** detect `terminal` / `run_process`; prefer stdout tail presentation | execute_card |
| W2.6 | Optional core event `ToolStdoutChunk` if progress spam is too coarse — only if W2.3 insufficient | core |
| W2.7 | Focus tool pane remains 3 lines; card holds full buffer | shelf |
| W2.8 | e2e: ToolExec → N progress → ToolDone → transcript has header + expand body | tests |

**Grok borrow:** `ExecuteToolCallBlock` push chunk; agent tools start Collapsed; interactive can Truncate while running.

### Wave 3 — File edit polish (P0/P1, ~2 days)

**Goal:** Make 016 work feel Grok-native in the stream.

| ID | Work | Owner |
|----|------|-------|
| W3.1 | On `ToolExec` for write/patch: show **running** “Editing path…” card (not only post-done) | dispatch |
| W3.2 | On done: attach `EditPresentation` hunks into ToolCard body (reuse edit_diff) | edit_card |
| W3.3 | Default Truncated: header + 8 hunk lines; expand full capped hunks | render |
| W3.4 | Fix any remaining +/− accuracy edge cases on multi-file patches | tool_display |
| W3.5 | Status bar / shelf footer: session `files N  +X −Y` from lightweight ledger | session_edits |
| W3.6 | e2e: write_file create → green inserts; patch → red/green hunk | tests |

### Wave 4 — Verb groups beyond edit + tool usage bar (P1, ~3 days)

| ID | Work |
|----|------|
| W4.1 | Generalize verb group: Read · Search · Edit (Grok `VerbGroupKind`) |
| W4.2 | Fold “Read 4 files” consecutive successes |
| W4.3 | Mini tool-usage strip: `Exec 2 · Read 5 · Edit 3` (session or turn) |
| W4.4 | e2e group merge + non-merge on kind change |

### Wave 5 — Optional syntax highlight (P1/P2, ~1 week)

| ID | Work |
|----|------|
| W5.1 | `EditHighlighter` trait; noop default |
| W5.2 | Feature `edit-syntax` + syntect if not already heavy |
| W5.3 | Caps: 2 MiB / 50k lines; unhighlighted first paint |
| W5.4 | Skin-mapped styles |

---

## 6. Data model (minimal, no flaky heuristics)

```rust
// presentation/blocks/mod.rs — conceptual

pub enum DisplayMode { Collapsed, Truncated, Expanded }

pub enum CardStatus { Generating, Running, Success, Error }

pub struct ThinkingCard {
    pub text: String,
    pub started: Instant,
    pub finished: Option<Instant>,
    pub mode: DisplayMode,
}

pub struct ToolCard {
    pub tool_call_id: String,
    pub name: String,
    pub kind: ToolCardKind, // Edit | Execute | Read | Search | Other
    pub status: CardStatus,
    pub caption: String,           // from tool_display
    pub body: RollingBuffer,       // progress / stdout / hunk text
    pub edit: Option<EditPresentation>,
    pub mode: DisplayMode,
    pub duration_ms: Option<u64>,
}

pub enum TurnPhase {
    Idle,
    Thinking,
    Tool { id: String, name: String },
    Responding,
}
```

**No soft scores.** Caps are constants; modes are explicit user/config toggles.

---

## 7. StreamEvent policy

| Event | Presentation action |
|-------|---------------------|
| `Reasoning` | Ensure ThinkingCard; append; phase=Thinking |
| `Token` | Close thinking if open; phase=Responding; stream assistant line |
| `ToolGenerating` | Optional sub-caption “composing tool…” (rate-limit) |
| `ToolExec` | Open ToolCard Running; phase=Tool |
| `ToolProgress` | Append body; update shelf focus |
| `ToolDone` | Finalize card; maybe open edit presentation; phase idle/thinking |
| SubAgent* | Keep existing subagent shelf section; later SubAgentCard |

Prefer **not** bloating `StreamEvent` until ToolProgress proves insufficient.

---

## 8. E2E / test plan

| ID | Layer | Assert |
|----|-------|--------|
| **TUI-S1** | unit | `turn_phase` transitions for full event sequence |
| **TUI-S2** | unit | ThinkingCard Truncated shows last N only |
| **TUI-S3** | unit | ToolCard buffer rolls at 4KB |
| **TUI-S4** | unit | Edit ToolCard reuses edit_diff stats |
| **TUI-S5** | harness | `stream_dispatch_harness` ToolExec→Progress→Done updates phase |
| **TUI-S6** | lib | Verb group still merges edits; read group merges |
| **TUI-S7** | e2e | Scripted mock provider: reason + write_file + terminal progress → transcript line counts |
| **TUI-S8** | regression | `/details` modes still gate shelf sections |
| **TUI-S9** | regression | Expand key toggles thinking and tool cards |

```bash
cargo test -p edgecrab-cli --lib turn_phase
cargo test -p edgecrab-cli --lib presentation
cargo test -p edgecrab-cli --lib edit_
cargo test -p edgecrab-cli --lib stream_dispatch
cargo test -p edgecrab-cli --test tui_stream_ux_e2e   # new
```

---

## 9. Acceptance criteria (operator)

### Wave 1 done

- [x] During reasoning, UI says **Thinking** (one place, consistent)  
- [x] After tools start, thinking becomes a **collapsed transcript card** with duration  
- [x] Expand shows last/full reasoning under caps  

### Wave 2 done

- [x] While `terminal` runs, live lines visible in shelf **and** recoverable in card  
- [x] Done shows ✓/✗ + duration; expand shows buffered output  
- [x] No multi-megabyte freezes (caps)  

### Wave 3 done

- [x] write/patch show “Editing…” then hunk card with +/−  
- [x] Session footer `files · +/−` optional but accurate  

### Overall

- [x] Zero paint logic growth in `app.rs` beyond dispatch hooks  
- [x] All new modules unit-tested; one e2e golden stream  
- [ ] Skin colors for thinking / insert / delete / error documented (edit chrome already semantic; skin.yaml mapping deferred)  

---

## 10. Explicit denylist

| Forbidden | Why |
|-----------|-----|
| Depend on `xai-grok-pager` | License / size / coupling |
| Copy 2.8k-line `edit.rs` wholesale | Overkill; mechanisms only |
| Second phase enum in shelf | Dual source of truth (J8 failure) |
| Soft “looks done” UI | Harness owns done; TUI only presents |
| Uncapped tool stdout in RAM | OOM / jank |
| Rebuild system prompt for UI | Cache-breaking; out of scope |

---

## 11. Suggested ship order

```text
W0 baseline
  → W1 phase + thinking card     (biggest clarity win)
  → W2 tool live cards           (terminal delight)
  → W3 edit stream polish        (finish 016 story)
  → W4 verb groups + usage bar
  → W5 syntax HL (optional)
```

**Estimate:** W0–W3 ≈ 1.5–2 weeks focused; W4–W5 optional second week.

---

## 12. Relationship to prior work

| Spec | Role |
|------|------|
| **015** | Diagnosis: Grok vs EC code-edit display |
| **016** | Edit-only plan; A–C **implemented** in `edit_diff.rs` |
| **024 (this)** | Full **stream UX** (thinking + tools + files); reuses 016 |

Do **not** re-implement edit hunks. Extend them into a **card lifecycle** shared with tools and thinking.

---

## 13. Next action

1. Approve this plan (or scope to W1–W2 only).  
2. Implement W0 + W1 in a focused PR.  
3. E2E golden before W2.  

*Code is law: Grok Build is the reference for perception, EdgeCrab keeps security, harness, and skin ownership.*
