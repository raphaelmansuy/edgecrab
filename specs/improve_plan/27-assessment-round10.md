# Round 10 — Long-Text Generation Feedback UX/UI

> **Trigger:** When the LLM generates a long document (research report, audit, code file),
> the user has NO idea how far along the generation is, how many words/chars have been
> written, whether the model has reached a significant section boundary, or how long
> before completion. The status bar shows only `▶ 169tok  21t/s` — raw token velocity
> with no semantic meaning to a user awaiting a 3,000-word document.
>
> **Root question:** What does a user need to see when waiting for a long generation?
>
> **Baseline:** Round 9 (374 tests), FP34–FP38 implemented.

---

## Brutal Honest Comparison: Code Is Law

### What EdgeCrab actually does today

```
STATUS BAR:  ▶ 169tok  21t/s
OUTPUT AREA: [tokens streaming in character by character...]
```

The `DisplayState::Streaming { token_count, started }` variant tracks only:
- Cumulative token count (integer)
- Start instant (for velocity calculation)

**What is absent:**
1. Character/word/line count — users think in words, not tokens
2. Section headings detected in the stream (# Title, ## Heading)
3. Estimated time remaining (can be derived from velocity × typical length)
4. Visual progress gradient (how deep into generation we are)
5. Context window pressure during generation (is the response filling context?)
6. "Still generating" heartbeat when token velocity is low (e.g. mid-thinking pause)

The status bar during streaming:
```rust
DisplayState::Streaming { token_count, started } => {
    let rate_str = if elapsed > 1.0 && *token_count > 5 {
        format!("  {rate:.0}t/s")
    } else { String::new() };
    left_spans.push(Span::styled(
        format!(" ▶ {token_count}tok{rate_str} "),
        ...Color::Rgb(100, 230, 100)...
    ));
}
```

This is the complete story. No chars, no words, no headings, no ETA.

---

### What Hermes does (KawaiiSpinner)

```python
line = f"  {frame} {self.message} ({elapsed:.1f}s)"
```

For tool execution only. During LLM streaming, Hermes relies on prompt_toolkit's
`patch_stdout` and the raw token output — essentially the same as EdgeCrab but worse
because it's a print loop, not a TUI. The spinner is DISABLED during streaming
(`_is_patch_stdout_proxy()` → `while self.running: time.sleep(0.1)`).

**Verdict on Hermes:** Hermes has NO live streaming feedback during LLM text generation.
The user sees tokens streaming to the terminal and that's it. No status bar at all.
EdgeCrab is already ahead of Hermes on streaming UX.

The Hermes gap is real and unaddressed — its architecture (prompt_toolkit + print loop)
makes it structurally hard to add without a full TUI rewrite.

---

### What Claude Code does (REPL.tsx)

```typescript
// Key insight:
const visibleStreamingText = streamingText && showStreamingText
  ? streamingText.substring(0, streamingText.lastIndexOf('\n') + 1) || null
  : null;

// Hide the spinner when streaming text is visible:
const showSpinner = (...) && (!visibleStreamingText || isBriefOnly);
```

Claude Code:
1. Streams text CHARACTER by character into the React state
2. Only shows COMPLETE lines (`lastIndexOf('\n')`) — no partial-line flicker
3. Hides the spinner when text is flowing — the text IS the feedback
4. Uses Ink's 16ms render throttle to batch updates
5. Shows `spinnerMessage` / `spinnerColor` per-state (tool use vs reasoning vs completion)
6. Tracks `setResponseLength` for budgeting

**What Claude Code does NOT have:**
- Word/char count in status
- Section heading detection
- ETA
- Progress bar or gradient

**Verdict on Claude Code:** Better than Hermes (line-complete streaming), roughly equal
to EdgeCrab on the information dimension. Neither shows semantic progress for long docs.

---

## First Principles Analysis

### Principle 1: User mental model mismatch

```
TOKEN:    ~0.75 words average
USER:     thinks "how many words have been written?"
DISPLAY:  "169tok"  ← meaningless to user

FIX:     show words, chars, or lines alongside tokens
         "▶ ~225 words  21t/s" is 10× more informative
```

Token count is an engineering metric. Users measure content in words or lines.
A quick transformation: `chars ÷ 4.5 ≈ words`, derivable from the accumulated
`streaming_line` buffer. Cost: O(1) — the string is already in memory.

---

### Principle 2: Section completion is a natural cognitive milestone

When the LLM generates:
```
## Market Analysis
[content...]

## Competitive Landscape
[content...]
```

Each `## Heading` marks a semantic milestone. Detecting `\n##` in the stream and
displaying the current section name gives users a progress landmark:

```
▶ ~580 words  | ## Competitive Landscape  | 18t/s
```

This is the difference between "bytes flowing" and "I can see where the document is."

---

### Principle 3: Velocity plateau signals "still alive"

Token velocity drops to 0–3 t/s when:
- The model is mid-reasoning before a new section
- A tool call is being prepared but not yet emitted
- Rate limiting / network stall

During these pauses the status bar becomes stale (e.g. shows `21t/s` from 10 seconds ago).
A rolling 3-second average would show the plateau honestly. A heartbeat animation
(pulsing `▶`) confirms the model is alive even at 0 t/s.

---

### Principle 4: Claude Code's "hide spinner when text flows" is correct

EdgeCrab currently shows `▶ 169tok  21t/s` ALONGSIDE the streaming tokens.
Claude Code's design decision: **the text IS the feedback** — no spinner needed.
However, the status bar should stay visible as a progress indicator.

The correct split:
- Output area: streaming tokens (visual feedback)
- Status bar: aggregate progress (words, section, rate, elapsed)

These two signals are complementary, not redundant.

---

### Principle 5: Line-complete rendering prevents horizontal scroll

EdgeCrab currently appends each token to the last line in the output buffer.
For a long markdown document this means the last line grows character by character,
causing layout thrash on every render tick. Claude Code solves this by only showing
lines with a trailing `\n`, keeping the visible text in complete-line units.

EdgeCrab's ratatui rendering should adopt the same pattern: display the current
streaming line as a "ghost" in a dim style, committing it to normal rendering only
when a newline is received.

---

## Gap Matrix (First Principles)

```
+------------------------------------------+------------+------------+-------------+
| UX Signal                                | EdgeCrab   | Hermes     | Claude Code |
+------------------------------------------+------------+------------+-------------+
| Token velocity (t/s)                     | ✓ present  | ✗ absent   | ✗ absent    |
| Word / char count in status bar          | ✗ absent   | ✗ absent   | ✗ absent    |
| Current section heading in status bar    | ✗ absent   | ✗ absent   | ✗ absent    |
| Rolling velocity (3s average)            | ✗ absent   | ✗ absent   | ✗ absent    |
| Heartbeat animation at velocity plateau  | ✗ absent   | ✗ absent   | ✗ absent    |
| Line-complete token rendering            | ✗ partial  | ✗ absent   | ✓ present   |
| ETA (estimated seconds to completion)    | ✗ absent   | ✗ absent   | ✗ absent    |
| Context pressure during generation       | ✓ notice   | ✗ absent   | ✗ absent    |
| Spinner hidden when text flows           | ✗ stale    | ✗ n/a      | ✓ present   |
+------------------------------------------+------------+------------+-------------+
```

EdgeCrab has token velocity — unique among the three. All three lack the semantic
layer (words, section, ETA). EdgeCrab is best positioned to lead here because of
the full ratatui TUI stack.

---

## Improvement Plan (FP39–FP43)

### FP39 — Word/Char Count in Streaming Status Bar

**Problem:** Users see `169tok 21t/s`. They have no idea how long the document is.

**Fix:** Track `chars_written: u64` in `DisplayState::Streaming`. Derive from the
`streaming_line` buffer length after each token append. Display as `~N words` where
`N = chars ÷ 4.5` (rounded to nearest 10 for anti-flicker).

```
BEFORE:  ▶ 169tok  21t/s
AFTER:   ▶ ~225 words  |  169tok  21t/s
```

**Implementation notes:**
- Add `chars_written: u64` to `DisplayState::Streaming`
- Update on every `AgentResponse::Token(text)` in `check_responses()`
- Format in `render_status_bar()`: `format!(" ~{} words ", words_estimate(chars))`
- Anti-flicker: bucket to nearest 10 words (`(chars / 45) * 10`)

**DRY check:** Single mutation point in `check_responses()`, single display point
in `render_status_bar()`. No duplication with token count tracking.

---

### FP40 — Current Section Heading in Streaming Status Bar

**Problem:** No semantic progress landmark during long document generation.

**Fix:** Scan the streaming token buffer for markdown headings. When a `\n## ` or
`\n# ` sequence is completed in the stream, extract the heading text and display it
in the status bar. Keep the last heading seen (don't clear between headings).

```
▶ ~580 words  │ ## Competitive Landscape  │  21t/s
```

**Implementation notes:**
- Add `current_section: Option<String>` to `DisplayState::Streaming`
- Scan in `check_responses()` after appending token to `streaming_line` text
- Heading detection: look for `\n#+ ` in accumulated text suffix
- Truncate to 30 chars max for status bar width safety
- Only extract top-level (`#`) and second-level (`##`) headings

**DRY check:** The accumulated text is already in `self.output[streaming_line_idx].text`.
No new buffer needed — just scan the tail on newline receipt.

---

### FP41 — Rolling 3-Second Velocity Average

**Problem:** Token velocity shown is from `started` to now (total average).
After a long response, the displayed rate is the mean of the whole session,
not the current rate. A mid-document pause shows the rate from 5 minutes ago.

**Fix:** Track a ring buffer of (timestamp, tokens_at_time) pairs. Use a 3-second
sliding window to compute the current rate. Fall back to the total-session average
when the window has fewer than 3 data points.

```
BEFORE:  21t/s   ← always session average
AFTER:   5t/s    ← current 3s rate (model pausing before new section)
         ⬇ clearly signals "still working, just slower now"
```

**Implementation notes:**
- Add `velocity_samples: VecDeque<(Instant, u64)>` to `AppState`
- Push `(Instant::now(), self.turn_stream_tokens)` every 500ms via animation tick
- In `render_status_bar()`, compute windowed rate from last 3s samples
- Fallback: if samples window < 2s elapsed, use session average
- Cap VecDeque at 20 entries (3s window at 100ms sample = 30, but 20 is safe)

**DRY check:** Velocity computation extracted to a standalone `fn rolling_velocity()`.

---

### FP42 — Line-Complete Streaming Render (Ghost Line)

**Problem:** EdgeCrab appends each token directly to the last output line.
For wide terminals, mid-line tokens cause layout thrash on every render tick.
Claude Code only shows complete lines (`lastIndexOf('\n')`).

**Fix:** Split `streaming_line` rendering into two tiers:
1. **Committed lines** (rows ending with `\n`): rendered with full markdown + colors
2. **Ghost line** (current in-progress row): rendered with `Style::default().fg(dim_gray)`

The ghost line shows "where the LLM is right now" without markdown noise.
On `\n` receipt, the ghost commits to a full output line.

**Implementation notes:**
- No new state needed — split at `\n` on render in `render_output_area()`
- The `OutputLine::text` continues to accumulate all tokens
- When rendering the streaming line: `text.split('\n')` → render all but last as
  committed spans, render last as a dim ghost span
- Ghost style: `Color::Rgb(100, 100, 115)` (dimmer than normal assistant text)
- This is purely a render-time transformation — no state change required

**DRY check:** Isolated to the output area rendering function. No state duplication.

---

### FP43 — Elapsed Time Display During Long Generation

**Problem:** No elapsed time shown during streaming. For a 60-second generation,
users have no sense of how long they've been waiting.

**Fix:** Show elapsed time in the status bar once `elapsed > 5s`. Format: `12s`.

```
▶ ~580 words  │ ## Competitive Landscape  │  5t/s  │  12s
```

**Implementation notes:**
- `started` is already in `DisplayState::Streaming`
- In `render_status_bar()`: only render elapsed if `elapsed.as_secs() > 5`
- Format: `format!("  {}s", elapsed.as_secs())` — no millisecond noise
- Already implemented for `ToolExec` state (elapsed ≥ 3s) — same pattern

**DRY check:** Identical pattern to `ToolExec` elapsed display. Extract to
`fn format_elapsed_hint(elapsed: Duration, threshold_secs: u64) -> String` shared
by both states.

---

## Signal Cross-References

| This spec | Depends on / extends |
|---|---|
| FP39 words | `DisplayState::Streaming` (extends field set) |
| FP39 words | `render_status_bar()` Streaming arm (additive) |
| FP40 sections | `check_responses() AgentResponse::Token` (additive scan) |
| FP40 sections | `DisplayState::Streaming` (adds `current_section` field) |
| FP41 rolling rate | `AppState` (new `velocity_samples` VecDeque) |
| FP41 rolling rate | Animation tick (push sample every 500ms) |
| FP42 ghost line | `render_output_area()` streaming line rendering |
| FP42 ghost line | No state change — render-time only |
| FP43 elapsed | `render_status_bar()` — mirrors `ToolExec` elapsed pattern |
| FP43 elapsed | Candidate for `format_elapsed_hint()` helper (DRY with ToolExec) |
| Round 9 FP34 | FILE_OUTPUT_ENFORCEMENT is not affected by these rendering changes |

---

## DRY / SOLID Checklist

- [x] **Single source of truth**: `DisplayState::Streaming` is the single variant
  tracking streaming metadata. FP39+FP40 extend it; FP41 uses a separate VecDeque
  that is the single velocity ring buffer.
- [x] **Single mutation point**: All token accumulation logic is in `check_responses()`.
  FP39/FP40 additions are in that single method.
- [x] **Render separation**: FP42 (ghost line) is a pure render transform with no
  state side-effects. Streaming state logic and display logic stay separated.
- [x] **No duplication**: FP43 elapsed shares a helper with `ToolExec` elapsed.
  FP41 velocity computation is in a standalone `rolling_velocity()` function.
- [x] **Open/Closed**: `DisplayState::Streaming` gets new optional fields with
  defaults. Existing match arms for other states are unchanged.

---

## Edge Cases

1. **Model produces no `\n` headings** (prose-only response): FP40 section display
   remains hidden (Option::None). No false positive.

2. **Ultra-wide terminal**: FP40 heading display truncates at 30 chars.
   The status bar layout uses flexible spans — no overflow.

3. **Narrow terminal (< 60 cols)**: Compact status bar is used. FP39/FP40/FP43
   are not shown in compact mode — existing `compact_status_bar` guard applies.

4. **Token velocity very high (> 100 t/s)**: FP41 rolling window correctly captures
   the peak. FP39 word estimate updates quickly — anti-flicker bucketing prevents
   visual noise.

5. **Generation interrupted by ^C**: `DisplayState` transitions to Idle on interrupt.
   All streaming fields are discarded. No cleanup needed.

6. **Very short response (< 10 tokens)**: FP39 shows `~3 words` which is fine.
   FP43 elapsed is hidden (< 5s threshold). FP40 shows nothing (no headings).
   No regression from current behavior.

7. **`live_token_display_enabled = false`**: FP39/FP40/FP43 still apply to the
   status bar. Only FP42 (ghost line) is irrelevant — but it's a render-only change
   that already skips when no streaming line exists. No harm.
