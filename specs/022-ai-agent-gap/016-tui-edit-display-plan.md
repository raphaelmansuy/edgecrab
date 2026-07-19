# 016 — TUI Code-Edit Display Improvement Plan (DRY · SOLID)

**Status:** Phase A–C implemented (2026-07-19)  
**Date:** 2026-07-19  
**Authority:** [015-grok-build-tui-code-display.md](015-grok-build-tui-code-display.md)  
**Reference:** `/Users/raphaelmansuy/Github/03-working/grok-build` (mechanisms only)  
**Target:** `crates/edgecrab-cli` (+ optional thin types in `edgecrab-types` if needed)

---

## 0. Intent

Make EdgeCrab’s TUI **as legible as Grok Build** for file create/update/patch — without forking the pager, without depending on grok-build crates, and without growing `app.rs`.

**Success signal:** Operator sees, within one glance after `write_file` / `patch`:

1. path + action verb  
2. accurate +N −M  
3. 3–15 lines of hunk context with insert/delete paint  
4. expand for full hunk / collapse by default  
5. optional turn summary: “Edited 4 files (+120 −30)”

---

## 1. First principles (constraints)

| Principle | Rule for this plan |
|-----------|-------------------|
| **Code is law** | Extend `edit_diff.rs` / `tool_display.rs` / `transcript.rs`; cite symbols |
| **DRY** | One owner for “edit presentation”; no second diff engine in shelf vs transcript |
| **SOLID** | Presentation ≠ tool execution; snapshot capture ≠ render; HL optional backend |
| **Performance** | First paint < 5 ms for typical hunk; hard caps on file size/lines |
| **Security** | Keep path jail (`jail_write_path`); never read outside allowed roots for previews |
| **No product bloat** | No full HunkTracker actor / FS notify in phase 1 |

---

## 2. SOLID / DRY ownership map

```text
┌──────────────────────────────────────────────────────────────────┐
│ edgecrab-cli                                                     │
│                                                                  │
│  edit_presentation/   ← NEW module cluster (or expand edit_diff) │
│    snapshot.rs        capture before/after (existing logic)      │
│    hunks.rs           unified → DiffHunk model                   │
│    render.rs          DiffLine → Span/Line (chrome + colors)     │
│    highlight.rs       optional syntect (feature-gated later)     │
│    group.rs           consecutive Edit verb groups               │
│                                                                  │
│  tool_display.rs      captions / stats only (calls presentation) │
│  transcript.rs        OutputLine + EditCardKind                  │
│  activity_shelf.rs    live “Editing path…” (uses same captions)  │
│  app/response_dispatch.rs  thin: capture → render → push lines   │
└──────────────────────────────────────────────────────────────────┘
         ▲
         │ never imports TUI
┌────────┴─────────┐
│ edgecrab-tools   │  write_file / patch results (structured JSON)
└──────────────────┘
```

| Principle | Application |
|-----------|-------------|
| **S** | Snapshot capture does not paint; render does not touch disk after snapshot |
| **O** | New tools (`str_replace`) register path extractors; no rewrite of render |
| **L** | Any edit tool implementing `EditPathSource` works with same renderer |
| **I** | Shelf only needs caption; transcript needs hunks; don’t force full HL on shelf |
| **D** | Highlight backend trait `EditHighlighter` (noop vs syntect) |

### DRY rules

| Forbidden | Required |
|-----------|----------|
| Duplicate path extract in shelf + transcript | Shared `resolve_edit_paths(tool, args)` |
| Second TextDiff for stats | Stats from same hunk model as paint |
| Ad-hoc `+++`/`---` header counts as +/− | Count ChangeTag Insert/Delete lines |
| app.rs reimplements gutters | Only `edit_presentation::render` |

---

## 3. Phased delivery

### Phase A — Diff chrome & accurate stats (P0, 1 week)

**Goal:** Grok-like **readability** without syntax engine.

| Step | Work | Owner |
|------|------|-------|
| A1 | Introduce `DiffHunk` / `DiffLine { text, lo, ln, kind: Equal\|Insert\|Delete }` in `edit_diff` or `edit_presentation/hunks.rs` | cli |
| A2 | Build hunks from `LocalEditSnapshot` via `similar::TextDiff` (already dep) | cli |
| A3 | Render: gutter line #, `+`/`−`/space, optional content background green/red | cli |
| A4 | Hunk separator `"… N unchanged lines"` between distant hunks | cli |
| A5 | Fix `build_verbose_content_stat` for patch: real +/− from hunks | `tool_display` |
| A6 | Caps: `MAX_HUNK_LINES` (default 40 shown), `MAX_FILES` (6) — rest “+K more” | cli |
| A7 | Unit tests: snapshot → hunks → span count / colors | cli tests |

**Grok sample to mirror (mechanism):** `pager/src/diff.rs` `DiffLine` + `build_diff_hunks` context trimming; not the full progressive HL.

### Phase B — Expand / collapse edit cards (P0, 0.5 week)

| Step | Work | Owner |
|------|------|-------|
| B1 | Extend `OutputLine` with `EditCard { summary_spans, collapsed_hunks, expanded }` **or** reuse `expandable_body` with prebuilt collapsed spans | `transcript` |
| B2 | Default:  header + 8 lines; keybind (existing expand) shows full capped hunks | app |
| B3 | Shelf focus pane: path + +/− only (no full diff) — same caption API | shelf |

### Phase C — Verb grouping (P1, 0.5 week)

| Step | Work | Owner |
|------|------|-------|
| C1 | `EditVerbGroup` buffer: consecutive successful edit tools merge header | `edit_presentation/group.rs` |
| C2 | Display: `✎ Edited 3 files  +42 −7` expandable to per-file list | transcript |
| C3 | Flush group on non-edit tool / user message / turn end | response_dispatch |

**Grok sample:** `VerbGroupKind::EditFile` in `scrollback/blocks/tool/mod.rs`.

### Phase D — Optional syntax highlight (P1, 1 week)

| Step | Work | Owner |
|------|------|-------|
| D1 | Feature `edit-syntax` or always-on with caps if syntect already in workspace | cli |
| D2 | Trait `EditHighlighter::highlight(path, line, kind) -> Vec<(Style, &str)>` | cli |
| D3 | Phase paint: unhighlighted first; if file < 2 MiB and < 50k lines, optional full-file scope | background task |
| D4 | Theme-aware: map to `skin.yaml` success/error/muted | skin |

**Grok sample:** `EditHighlightPhase` + caps in `edit.rs` — reimplement thin, do not vendor.

### Phase E — Session edit strip (P2, 1 week)

| Step | Work | Owner |
|------|------|-------|
| E1 | `SessionEditLedger` in-memory: path → {creates, +/−, last_tool_id} | cli or core |
| E2 | Status bar / shelf footer: `files: 4  +120 −30` | status_bar / shelf |
| E3 | `/edits` or overlay list files touched this session | commands |
| E4 | **No** external FS watch in this phase | — |

**Grok sample:** `HunkTracker` *data model* only (`FileSummary`, `TurnSummary`), not actor.

---

## 4. Data model (minimal)

```rust
// Conceptual — final names in edit_presentation
pub enum DiffLineKind { Equal, Insert, Delete }

pub struct DiffLine {
    pub text: String,      // no trailing newline
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub kind: DiffLineKind,
}

pub struct DiffHunk {
    pub path: PathBuf,
    pub lines: Vec<DiffLine>,
}

pub struct EditPresentation {
    pub path_display: String,
    pub action: EditAction, // Create | Update | Delete | Patch
    pub stats: EditStats,   // plus, minus, files
    pub hunks: Vec<DiffHunk>,
    pub truncated: bool,
}
```

Wire from existing:

```text
capture_local_edit_snapshot → after tool success → build EditPresentation → render
```

---

## 5. E2E / unit test plan

| ID | Test | Asserts |
|----|------|---------|
| **TUI-E1** | `edit_hunks_from_write_snapshot` | insert/delete kinds correct |
| **TUI-E2** | `edit_render_has_gutter_and_colors` | span styles non-default for Insert |
| **TUI-E3** | `edit_stats_match_hunk_counts` | +/− equals insert/delete lines |
| **TUI-E4** | `edit_caps_truncate_with_more_marker` | 100-line change → truncated flag |
| **TUI-E5** | `edit_path_jail_still_blocks` | outside root → no snapshot |
| **TUI-E6** | `verb_group_merges_three_writes` | one group header |
| **TUI-E7** | `expand_edit_card_roundtrip` | collapsed → expanded line count ↑ |
| **TUI-E8** | Regression: terminal expandable_body still works | existing |

```bash
cargo test -p edgecrab-cli --lib edit_
cargo test -p edgecrab-cli --lib tool_display
# if binary tests:
cargo test -p edgecrab-cli --test edit_presentation_e2e
```

---

## 6. Acceptance criteria

### Phase A done when

- [x] After `write_file` / `patch`, transcript shows path + accurate +N −M  
- [x] At least one hunk with green/red paint and line numbers  
- [x] Caps enforced; huge files never freeze TUI  
- [x] Path jail tests green  

### Phase B done when

- [x] Default collapsed ≤ ~10 visual lines per edit  
- [x] Existing expand key shows more hunk content  

### Phase C done when

- [x] Three consecutive writes collapse to one “Edited 3 files” group  

### Phase D done when

- [ ] Optional HL on for files under caps; off or hunk-only above caps  

### Phase E done when

- [ ] Session strip or `/edits` lists touched files  

---

## 6b. Implementation assessment (2026-07-19)

| Item | Result |
|------|--------|
| **Owner** | `edgecrab-cli/src/edit_diff.rs` — single presentation module (snapshot → hunks → paint → verb group) |
| **Wire** | `app::push_edit_presentation` + `response_dispatch` ToolDone; `tool_display` patch stats call `count_patch_line_stats` |
| **Chrome** | Dual gutters (old/new line #), `+`/`−` markers, insert green / delete red content bg, `… N unchanged` separators |
| **Caps** | `MAX_COLLAPSED_LINES=8`, `MAX_EXPANDED_LINES=80`, `MAX_INLINE_DIFF_FILES=6` |
| **Expand** | `EditDiffCard.expandable_body` → `OutputLine::attach_expandable_body` (Ctrl+Shift+T) |
| **Verb group** | `EditVerbGroup` on `App`; consecutive successful edits rewrite header to `✎ Edited N files +X −Y` |
| **DRY** | Stats caption shared; no second TextDiff for verbose patch stats |
| **Tests** | TUI-E1…E7 unit tests in `edit_diff::tests` + `test_verbose_patch_stats_count_content_lines_not_headers` |
| **Deferred** | Phase D (syntect HL), Phase E (session ledger / `/edits`) |

```bash
cargo test -p edgecrab-cli edit_   # presentation + related
cargo test -p edgecrab-cli test_verbose_patch_stats
```

---

## 7. Explicit non-goals

| Item | Why |
|------|-----|
| Depend on `xai-grok-pager` / `xai-hunk-tracker` crates | Coupling / license / size |
| Full scrollback rewrite | Too large |
| External FS notify for “user edited same file” | Phase E+ only if demanded |
| Pixel-perfect Grok theme clone | Map to skin semantics |
| Blocking full-file syntect on main thread | AE performance |

---

## 8. Suggested sprint order

```text
Week 1   Phase A (hunks + chrome + stats) + TUI-E1…E5
Week 2   Phase B + C (expand + verb group) + TUI-E6…E7
Week 3   Phase D (optional HL) if capacity
Week 4   Phase E (session ledger) if demand
```

---

## 9. Cross-refs

| Doc | Role |
|------|------|
| [015](015-grok-build-tui-code-display.md) | Analysis + borrow/reject |
| [014](014-improvement-plan.md) | Agent harness / MCP plan (separate) |
| `edgecrab-cli/src/edit_diff.rs` | Current snapshot diff |
| `edgecrab-cli/src/tool_display.rs` | Captions / colors |
| `edgecrab-cli/src/transcript.rs` | OutputLine model |
| Grok `pager/src/diff.rs` | Hunk builder sample |
| Grok `scrollback/blocks/tool/edit.rs` | Progressive HL sample |
| Grok `xai-hunk-tracker` | Session model sample (phase E) |

---

## 10. One-line summary

**Steal Grok’s edit-perception architecture (typed hunks, gutters, progressive caps, verb groups) into EdgeCrab’s existing `edit_diff` + transcript pipeline — one SOLID presentation owner, e2e-locked, no pager rewrite.**
