# Round 12 — Tool Call TUI/UX + write_file Token Waste: First Principles Assessment

## Context

Round 11 (FP44–FP47) fully implemented and confirmed in screenshots:
- TTFB tracking and display
- Ghost waiting line in output area
- Color escalation during wait
- 20s stall tier with ⚠ symbol

This round targets **two independent problems** surfaced by direct agent usage:

1. **Tool Call TUI/UX** — the visual experience when a tool is executing  
2. **write_file token waste** — the model generates thousands of tokens, calls `write_file`,  
   and the tool rejects it because the file already exists — all tokens wasted.

---

## Problem 1 — Tool Call TUI/UX

### What currently happens

| Phase | Status bar | Output area |
|-------|-----------|-------------|
| ToolExec fires | Cyan `⠋ {verb} {icon} {preview}` | Cyan placeholder `···` line |
| ToolProgress arrives | Cyan bar, detail shown | Placeholder updated with detail |
| After 3s | Cyan + `Xs` elapsed | Placeholder unchanged (no time) |
| After 10s | Cyan + `Xs  ^C=stop` | Placeholder still unchanged |
| ToolDone | State transitions to Streaming | Placeholder upgraded to done line |

### Three core problems

**P1 — Status bar is always cyan, regardless of tool type**

The output area correctly uses semantic category colors:
- FileWrite → amber `Rgb(255, 185, 50)`
- Terminal → orange `Rgb(255, 145, 60)`
- Search → cyan `Rgb(80, 210, 230)`
- Memory → sage `Rgb(110, 195, 135)`
- AI → violet `Rgb(185, 145, 240)`

But the status bar uses the **same invariant cyan** `Rgb(77, 208, 225)` for ALL tools.
A `write_file` (file mutation, amber) looks identical to a `web_search` (search, cyan).
This breaks the semantic color language established in the output area.

**P2 — No elapsed time in the running placeholder line**

The running line in the output area shows `  ···` forever without any time indication.
Users cannot see how long a tool has been running by looking at the output area —
they must look at the status bar (spatially disconnected). For long-running tools
(web_extract, terminal, delegate_task), the `···` indicator gives no sense of progress.

After 3s, the running line should show `  ···  Xs` — a compact elapsed indicator
that mirrors the status bar's own elapsed display.

**P3 — No urgency gradient for slow tool calls**

A 2s and a 35s tool call show the same visual signal. The only text change is
`^C=stop` after 10s. Users cannot visually distinguish:
- "This tool is normally slow" (web_extract: 5-15s)
- "This tool should be fast but is stuck" (read_file: should be <1s, took 30s)
- Color escalation from category-color → amber → orange encodes urgency
  without requiring the user to read elapsed numbers.

### First Principles

1. **Semantic color language must be consistent** — if amber means FileWrite in the
   output area, the status bar must also show amber during FileWrite execution.
   Inconsistency forces the user to re-learn the language in each display zone.

2. **Focal point principle** — users look at the bottom of the output area (focal point)
   during tool execution. The running placeholder IS the focal point. Temporal
   information (elapsed) must appear there, not only in the peripheral status bar.

3. **Urgency gradient** — visual encoding must escalate with severity. Category color
   (normal) → amber (slow) → orange (stalled) encodes three urgency tiers in a
   way immediately understood without reading numbers.

---

## Problem 2 — write_file Token Waste

### What currently happens

1. Model decides to write a file (e.g., `audit_quanta.md`)
2. Model **generates full file content** (71 lines, 5.3k in the screenshot)
3. Model calls `write_file(path="./audit_quanta.md", content="...")`
4. Tool checks: file exists + no read snapshot in this session
5. Tool returns: `'./audit_quanta.md' already exists and has not been read in this session. Run read_file...`
6. **All 5.3k of generated content is wasted** — the model must now:
   a. Call `read_file("./audit_quanta.md")` — extra round-trip
   b. Call `write_file` AGAIN with new content — another full content generation

**Total waste: 2× full file generation + 1 extra read_file turn**

### Why it's architecturally hard to fully prevent

Tool arguments (including `content`) are generated token-by-token during streaming.
The tool executes only AFTER all arguments are fully received. There is no mechanism
to "interrupt" the model while it is generating the content field of a tool call.

The only way to prevent this is:
1. **Pre-flight signaling**: Detect the `path` argument early in streaming and cancel
   the generation. **Too complex and fragile** — partial JSON parsing required.
2. **Make the error so actionable that recovery is immediate and cheap**: If the error
   includes the current file content, the model can immediately use `patch` without
   an extra `read_file` turn. This cuts 1 turn from the recovery path.
3. **Steer the model away from write_file for modifications** via better schema
   description. This reduces frequency of the error.
4. **Guidance for write_file schema to be explicit**: "Use write_file only for NEW files
   or full replacements. For ANY modification to an existing file, use patch/apply_patch."

### First Principles

1. **Error messages are part of the UX** — when an error is inevitable (the tool will
   fire AFTER content is generated), the error must maximize recovery efficiency.
   Including the current file content in the error saves 1 full LLM turn.

2. **Schema descriptions are instructions** — the model reads the tool description
   when selecting tools. A description that explicitly warns "only for NEW files"
   reduces incorrect tool selection at the intent-formation stage (before generation).

3. **Separation of concerns**: `write_file` for creating new files; `patch`/`apply_patch`
   for modifying existing files. This is the hermes-agent approach (verified from code).
   hermes description: "OVERWRITES the entire file — use 'patch' for targeted edits".

---

## Solutions

### FP48 — Tool-category color in status bar during execution

**Change**: During `DisplayState::ToolExec`, use `tool_category(name)` to select the
status bar foreground color, instead of hardcoded cyan `Rgb(77, 208, 225)`.

**Implementation**:
- Import `tool_category` and `ToolCategory` in `app.rs` (already used for output lines)
- Replace the single `Span::styled(content, cyan_style)` with category-aware color logic
- Use the same `category.name_color()` function as the output area

**Color mapping** (from `ToolCategory::name_color()`):
| Category | Color | Tools |
|----------|-------|-------|
| Search | cyan `Rgb(80, 210, 230)` | web_search, search_files |
| WebBrowser | teal `Rgb(64, 188, 212)` | web_extract, browser_* |
| FileRead | slate `Rgb(150, 165, 195)` | read_file |
| FileWrite | amber `Rgb(255, 185, 50)` | write_file, patch |
| Terminal | orange `Rgb(255, 145, 60)` | terminal, execute_code |
| Memory | sage `Rgb(110, 195, 135)` | memory |
| Plan | periwinkle `Rgb(140, 170, 255)` | todo |
| AI | violet `Rgb(185, 145, 240)` | delegate_task, vision |
| MCP | steel `Rgb(130, 165, 210)` | mcp_* |
| HA | green `Rgb(100, 195, 145)` | ha_* |
| Other | gray `Rgb(170, 180, 205)` | remaining |

**Edge case**: When multiple tools are running in parallel (`summarize_active_tools`
returns a summary), use the summary's icon to determine category, or fall back to cyan.

---

### FP49 — Elapsed time in running placeholder line

**Change**: After 3s, the `  ···` tail of the running line becomes `  ···  Xs`.

**Implementation**:
1. Add `elapsed_secs: Option<u64>` parameter to `build_tool_running_line_width`
2. Append `  Xs` (using `format_duration_aligned`) to the `···` span when elapsed ≥ 3s
3. Rebuild running lines on each render tick in the App event loop:
   - In the tick handler, iterate `pending_tool_lines`
   - For each, recompute `elapsed_secs = started_at.elapsed().as_secs()`  
   - If elapsed ≥ 3s, rebuild the spans and mark `needs_redraw = true`

**Note**: `PendingToolLine` currently stores `tool_name`, `args_json`, `line_idx`,
`edit_snapshot`. We need `started_at: Instant` — already available because
`active_tools` (HashMap<tool_call_id, ActiveToolStatus>) already stores `started_at`.
So in the tick handler, we look up `active_tools[id].started_at` to get elapsed.

---

### FP50 — Urgency gradient in status bar

**Change**: Layer urgency color on top of category color for slow tool calls.

**Tiers** (elapsed from `started_at` or `summarize_active_tools.elapsed_secs`):
| Tier | Threshold | Color | Notes |
|------|-----------|-------|-------|
| Normal | 0–5s | Category color | Fast tools (read_file, write_file) |
| Slow | 5–15s | Amber `Rgb(255, 200, 80)` | Web, terminal, delegate |
| Stalled | 15s+ | Orange `Rgb(255, 140, 50)` | Possible network issue |

**Rationale**: 5s is a reasonable "this is now slow" threshold for most tools.
15s+ indicates either a known-slow operation (web crawl, delegate) or a stall.
The color change ensures users don't need to read the elapsed number to assess urgency.

---

### FP51 — Include file content in write_file "already exists" error

**Change**: When `write_file` rejects because the file exists and has no read snapshot,
include the first 1500 chars of the existing file content in the error message.

**New error format**:
```
'./audit_quanta.md' already exists and has not been read in this session.
PREFERRED: Use patch/apply_patch for targeted edits to existing files — much more token-efficient.
If you must fully replace the file, call read_file first.

Current file content (first 1500 chars):
---
[content here]
---
```

**Benefits**:
1. Model can immediately use `patch` in the next turn (no extra `read_file` needed)
2. Saves 1 full LLM round-trip
3. If patch is used, saves the next write_file content generation (only the diff is needed)
4. Worst case (model still calls write_file): same cost as before, but now read_file is
   unnecessary (content was included in error)

**Implementation**: In `execute()` of `WriteFileTool`, when returning the "already exists"
error, also read the file (sync, bounded to 1500 chars) and include it in the message.

**Limit**: Cap at 1500 chars to avoid bloating the context. Add a note if truncated.
Use UTF-8 safe truncation (`safe_char_start` or `chars().take()` logic).

---

### FP52 — Improve write_file schema description

**Change**: Rewrite the schema description to strongly steer the model toward `patch`
for modifications and away from `write_file` for existing files.

**Current description** (key sentence):
> "For an existing non-empty file, read it in the current session before using write_file
> so the overwrite is based on fresh file state."

**Problem**: This ALLOWS overwriting (just requires a prior read). It doesn't steer
the model toward `patch`. The model reads this as "first read, then write" — which is
exactly the wasteful pattern we want to avoid.

**New description**:
> "Creates or fully replaces a file. ONLY use for NEW files or when you need to replace
> the ENTIRE content. For ANY modification or edit to an existing file, use
> patch/apply_patch instead — it is far more token-efficient and safe.
> Attempting to write_file on an existing file without first reading it in this session
> will be rejected with the current content included in the error."

The key change: "ONLY use for NEW files" + naming `patch/apply_patch` as the
alternative + warning that a rejection will include the file content (so the model
knows it will get what it needs without an extra read_file).

---

## Test Matrix

| Change | Test |
|--------|------|
| FP48 | Manual: run write_file → status bar should show amber |
| FP48 | Manual: run terminal → status bar should show orange |
| FP48 | Automated: `cargo test -p edgecrab-cli` |
| FP49 | Manual: trigger 5s+ tool (web_extract) → running line shows `···  5s` |
| FP50 | Manual: trigger 6s tool → status bar shifts to amber |
| FP50 | Manual: trigger 16s tool → status bar shifts to orange |
| FP51 | `cargo test -p edgecrab-tools -- write_file` (existing tests + new) |
| FP51 | New test: `write_file_error_includes_existing_content` |
| FP52 | Schema string test: description contains "ONLY use for NEW files" |

---

## Impact Summary

| FP | Effort | Impact | Risk |
|----|--------|--------|------|
| FP48 | Low (1-line color change) | Medium (visual clarity) | Minimal |
| FP49 | Medium (tick loop update) | Medium (focal-point feedback) | Low |
| FP50 | Low (threshold logic) | High (urgency signal) | Minimal |
| FP51 | Medium (file read in error) | High (saves 1 LLM turn + tokens) | Low |
| FP52 | Trivial (string change) | High (reduces error frequency) | None |
