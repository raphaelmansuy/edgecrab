# 015 — Grok Build TUI vs EdgeCrab: Code Edit / Update Display

**Status:** Code-is-law analysis  
**Date:** 2026-07-19  
**Sources:**

| Tree | Path | Role |
|------|------|------|
| **Grok Build** | `/Users/raphaelmansuy/Github/03-working/grok-build` | Reference TUI (pager + tool blocks + hunk tracker) |
| **EdgeCrab** | this repo `crates/edgecrab-cli` | Target TUI (ratatui + activity shelf + `edit_diff`) |

**Related:** [000-code-is-law.md](000-code-is-law.md) · [016-tui-edit-display-plan.md](016-tui-edit-display-plan.md) · prior TUI specs `002-tui-hemes-vs-edgecrab/`

---

## 1. First principles (what operators need when agents edit code)

Strip product names. A coding-agent TUI is good at file edits only if it answers, at a glance:

| Job | Question | Failure if missing |
|-----|----------|-------------------|
| **J-E1** | *What* changed? | Path + create/update/delete |
| **J-E2** | *How much?* | +lines / −lines / files |
| **J-E3** | *What does the change look like?* | Readable unified/hunk diff |
| **J-E4** | *Is it code or noise?* | Syntax-aware paint, not monochrome dump |
| **J-E5** | *Did it just happen or is it done?* | Running vs success vs error timing |
| **J-E6** | *Can I expand without drowning?* | Collapse by default, expand on demand |
| **J-E7** | *What did the agent change this session?* | Session-level edit inventory |
| **J-E8** | *Who wrote this hunk?* | Agent vs external/user (optional advanced) |

These are orthogonal to Hermes feature parity. They are **perception jobs** for coding agents (2026).

---

## 2. Architecture contrast (code is law)

### Grok Build — typed scrollback + specialized renderers

```text
Agent tools
    │
    ▼
ToolCallBlock sum type          scrollback/blocks/tool/mod.rs
  Read | Edit | Execute | Search | …
    │
    ├─ EditToolCallBlock        scrollback/blocks/tool/edit.rs (~2850 LOC)
    │     progressive syntect HL (hunk-only → file-scoped)
    │     DiffLineOutput + gutters + insert/delete bg
    │
    ├─ build_diff_hunks         pager/src/diff.rs (~1370 LOC)
    │     SearchReplaceEditDetail → DiffHunk (context_before/after)
    │
    └─ HunkTracker              xai-hunk-tracker/
          agent vs external edits, session stats, unified patches

ToolUsageStats                  pager/src/tool_usage.rs
  category bar: Execute/Read/Edit/Search …

TurnStatus                      views/turn_status.rs
  spinner + activity + timers + stop
```

**Property:** Edit display is a **first-class block type** with its own lifecycle (start → finish → expand), not a string line in a chat dump.

### EdgeCrab — flat transcript + optional snapshot diff

```text
StreamEvent::ToolExec / ToolDone
    │
    ▼
tool_display.rs                 captions, colors, path extract
    │
    ├─ capture_local_edit_snapshot   edit_diff.rs (~580 LOC)
    │     before-text map of target paths
    │
    └─ render_edit_diff_lines
          TextDiff unified → Span lines (no syntect, limited lines)
    │
    ▼
OutputLine (transcript.rs)      text | prebuilt_spans | expandable_body
Activity shelf                  live tool pane (not full hunk viewer)
```

**Property:** Diff is a **side-car** rendered after tool done; transcript model is still mostly linear text. Good foundation; not yet Grok-class perception.

---

## 3. Dimension matrix (borrow signal)

| Dimension | Grok Build (law) | EdgeCrab (law) | Score | Borrow? |
|-----------|------------------|----------------|-------|---------|
| Typed edit blocks | `ToolCallBlock::Edit` | flat `OutputLine` | **GB** | Yes — introduce `TranscriptBlock` sum or edit-specialized line kind |
| Inline hunk diff | Full gutter + dual optional line # | Unified text, max 80 lines / 6 files | **GB** | Yes — enhance `edit_diff` |
| Syntax highlight | Progressive syntect + caps 2MiB/50k lines | None on diffs | **GB** | Yes — optional phase, performance-capped |
| Context lines | `context_before` / `context_after` in edit detail | Full-file before/after only | **GB** | Partial — use similar for patch tools |
| Collapse / expand | Display modes on blocks | `expandable_body` for terminal mainly | **GB** slight | Extend expand to edit hunks |
| Verb grouping | `VerbGroupKind::EditFile` → "Edited 3 files" | Per-tool lines only | **GB** | Yes — group consecutive writes |
| Session edit inventory | `HunkTracker` + stats | No session hunk ledger | **GB** | Phase 2 — lightweight file summary strip |
| Live running state | Block status Running/Success/Failed | Shelf phase ToolExec | **= / EC shelf** | Align shelf ↔ transcript edit card |
| Path safety on preview | Sandbox-aware tools | `jail_write_path` + allowed roots | **EC** | KEEP |
| Theme / skin | Rich themes | YAML skin engine | **=** | Map edit colors into skin |
| Performance caps | Explicit HL caps | MAX_INLINE_DIFF_LINES=80 | **= intent / GB depth** | Adopt GB-style caps for HL |
| Diff stat accuracy | Real +/− from TextDiff | Patch stats count `+++`/`---` file headers (weak) | **GB** | Fix stats in tool_display |

---

## 4. High-signal code samples to borrow (mechanisms, not crates)

### 4.1 Structured edit detail → hunks (Grok)

```text
// grok-build pager/src/diff.rs
build_diff_hunks(&[SearchReplaceEditDetail {
  old_string, new_string, old_line, new_line,
  context_before, context_after, line_prefix, …
}]) -> Vec<DiffHunk>
```

**Borrow:** When EdgeCrab `patch` returns structured replace results (or when we can recover old/new from snapshot), render **hunks with context**, not only full-file unified dump.

### 4.2 Progressive highlight phases (Grok)

```text
// scrollback/blocks/tool/edit.rs
EditHighlightPhase::HunkOnly
  → Pending { job_id }   // background full-file
  → FileScoped { by_new_line, theme }
EDIT_HL_MAX_BYTES = 2 MiB
EDIT_HL_MAX_LINES = 50_000
```

**Borrow:** First paint cheap (line-oriented styles or unhighlighted); optional upgrade; **never** block TUI on multi-MB files.

### 4.3 Diff line chrome (Grok)

```text
DiffLineOutput {
  line, background, content_start_col,
  gutter_span_count, content_text, joiner, is_separator
}
// dual_line_numbers, hunk_separator "… N unchanged lines"
```

**Borrow:** Green/red content bg (not just `+`/`-` prefix), gutter line numbers, soft separators between hunks.

### 4.4 Verb groups (Grok)

```text
VerbGroupKind::EditFile → ("Edited", "Editing") + ("file", "files")
```

**Borrow:** Collapse `write_file` ×3 into one transcript header with expand-to-list.

### 4.5 HunkTracker session model (Grok) — selective

```text
HunkSource::AgentEdit { prompt_index }
HunkSource::ExternalEditOnAgentFile
SessionStats / TurnSummary / FileSummary
```

**Borrow carefully:** Full actor + FS watch is heavy. Phase 2: **in-memory turn/session edit ledger** (path, +/−, tool id) without external FS watcher.

### 4.6 EdgeCrab KEEP (do not replace)

```text
edit_diff::LocalEditSnapshot          // before-map + path jail
tool_display color families           // semantic tool colors
activity_shelf + focus tool pane      // live liveness
OutputLine.expandable_body            // expand pattern exists
skin.yaml                             // theme surface
```

---

## 5. Gap root causes (5 whys, condensed)

1. Why is Grok better at “seeing” edits? → Specialized **Edit** block + HL pipeline.  
2. Why doesn’t EdgeCrab? → Transcript is string-centric; edit_diff is a post-hoc attachment.  
3. Why string-centric? → Fast path from StreamEvent → OutputLine; no block type system.  
4. Why no HL? → No syntect path in edit_diff; perf fear without caps.  
5. Fix: introduce **edit presentation owner** with caps, without rewriting the whole TUI.

---

## 6. Non-borrows (reject)

| Grok pattern | Reject for EdgeCrab | Why |
|--------------|---------------------|-----|
| Full pager scrollback rewrite | Yes | Months of work; keep ratatui transcript |
| Full HunkTracker actor + FS notify | Yes (phase 1) | Complexity; start with tool-driven ledger |
| GBoom / mermaid / credit bar | Yes | Not coding-edit core |
| Copy Grok theme wholesale | Yes | Map into `skin.yaml` semantics |
| Depend on grok-build crates | Yes | License/coupling; reimplement mechanisms |

---

## 7. Scorecard (perception only)

| Job | Leader |
|-----|--------|
| J-E1 What path | **=** |
| J-E2 How much | **GB** (accurate +/−) |
| J-E3 Diff readability | **GB** |
| J-E4 Syntax paint | **GB** |
| J-E5 Running/done | **= / EC shelf** |
| J-E6 Collapse/expand | **GB** |
| J-E7 Session inventory | **GB** |
| J-E8 Agent vs external | **GB** (optional) |
| Path safety | **EC** |
| Single-binary simplicity | **EC** |

---

## 8. Next

Implementation plan: **[016-tui-edit-display-plan.md](016-tui-edit-display-plan.md)**
