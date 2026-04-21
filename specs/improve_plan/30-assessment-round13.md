# Round 13 — Animated Waiting Spinner + Tool Name Harness: First Principles Assessment

## Context

Round 12 (FP48–FP52) fully implemented and confirmed:
- Category-aware status bar color for ToolExec
- Elapsed time in running placeholder lines (tick-based)
- Urgency gradient (amber ≥5s, orange ≥15s)
- write_file "already exists" error includes current file content
- write_file schema steers model toward patch

This round targets **two new problems** captured in live screenshots with
`openrouter/openai/gpt-oss-20b:free` (NousResearch Hermes 3 family):

1. **FP53 — Static waiting spinner in input box**: `"⧗ waiting…"` is a
   frozen character. Users have no visual pulse confirming the agent is alive.

2. **FP54 — Tool name special-token contamination**: Hermes-family models
   (and some other fine-tunes) bleed `<|channel|>commentary` chatml training
   tokens into tool call function name fields, causing every dispatch to fail
   with `Unknown tool: web_extract<|channel|>commentary`.

---

## Problem 1 — Static "⧗ waiting…" title

### What currently happens

When `is_processing = true`, `render_input` sets the input box border title to
the literal string `"⧗ waiting…"`. The `⧗` (U+29D7 BLACK HOURGLASS) is a
static glyph — it never moves.

All animated `DisplayState` variants (`AwaitingFirstToken`, `Thinking`,
`ToolExec`, `BgOp`) carry a `frame: usize` counter that `tick_spinner()`
advances at ~10 fps. The status bar and output area both consume this frame for
braille animations. The input box consumes nothing.

### Root cause

`render_input` ignores `display_state`. It reads `is_processing: bool`
(a coarse flag) and outputs a static title — no frame, no ticker.

### Hermes-agent contrast

`KawaiiSpinner` in `agent/display.py` animates the entire input region —
the braille spinner, the kaomoji face, and the thinking verb all cycle in sync.
Hermes has no equivalent "static waiting title" because the entire idle state
is animated at 100 ms intervals.

### First Principles fix (FP53)

The fix is strictly local to `render_input`. We need to read the current
spinner frame from `display_state` — exactly the same frame the status bar
already uses — and substitute it for `⧗`.

**Design choices:**
- Use the **same** braille frame (`SPINNER_FRAMES`) for consistency with the
  status bar. Do not introduce a separate frame counter.
- Fall back to frame 0 (⠋) if the display_state has no frame (e.g. Idle,
  WaitingForClarify). This is correct because `is_processing` is only `true`
  while `display_state` is one of the animated variants.
- Keep the rest of the title text unchanged ("waiting…") — no word changes.

**Implementation:**
1. Add a `fn current_spinner_frame(&self) -> usize` helper that pattern-matches
   `display_state` to extract the frame, returning 0 as default.
2. In `render_input`, replace `"⧗ waiting…"` with
   `format!("{} waiting…", SPINNER_FRAMES[self.current_spinner_frame() % SPINNER_FRAMES.len()])`.

**No config needed.** No new state. No new fields. Pure read from existing state.

---

## Problem 2 — Tool name special-token contamination

### What currently happens (trace analysis)

From the screenshots, all tool dispatch failures share the pattern:

| LLM output | Registry lookup | Result |
|-----------|----------------|--------|
| `web_extract<\|channel\|>commentary` | `"web_extract<\|channel\|>commentary"` | NotFound |
| `apply_patch<\|channel\|>commentary` | `"apply_patch<\|channel\|>commentary"` | NotFound |
| `read_file<\|channel\|>commentary`  | `"read_file<\|channel\|>commentary"` | NotFound |
| `web extract` (space) | `"web extract"` | NotFound |

**The pattern:** `<|channel|>` is a NousResearch chatml special token used to
annotate the model's channel (e.g. "commentary", "think", "action"). In
structured-output / tool-calling mode, this model appends the token to function
names, treating the tool call as a channel announcement.

This is a **model output defect** (not a schema or toolset issue) that:
1. Has zero recovery path — fuzzy match cannot bridge the distance between
   `web_extract<|channel|>commentary` (35 chars) and `web_extract` (11 chars).
2. Cascades: each failed call wastes a tool-result slot, and the LLM then
   reasons about why every tool is failing rather than doing useful work.

### Hermes-agent contrast

Hermes-agent does not encounter this problem because it only supports
Anthropic/OpenAI flagship models whose tool-calling is well-calibrated. It has
no special-token sanitization layer in `handle_function_call`. This is a gap
we must fill for the broader model ecosystem edgecrab supports.

### Root cause

`dispatch_single_tool` in `conversation.rs` passes `name: &str` directly to
the registry dispatch and fingerprinting code without any normalization.
The registry does an exact `HashMap` lookup — no tolerance for training-bleed.

### First Principles fix (FP54)

The sanitization must be:
1. **Pure / idempotent** — already-clean names pass through with zero
   allocation (a `Cow::Borrowed` fast path).
2. **Applied at the single trust boundary** — `dispatch_single_tool` receives
   all tool calls from ALL code paths (parallel + sequential).  One fix point
   covers the entire harness.
3. **Narrow scope** — only strips known contaminants; does not do speculative
   fuzzy rewriting. A misspelled tool name should still get the "Did you mean?"
   fuzzy response from the registry.
4. **Observable** — logs a `tracing::info!` when a name is changed so
   operators can detect model-quality issues.

**Contaminants to strip (in order):**
1. `<|…|>` special tokens: find the first `<|` and truncate everything from
   that position onward.  This covers ALL chatml channel annotations
   regardless of the token content.
2. Space→underscore normalization: `"read file"` → `"read_file"`.  Some
   weaker models output tool names with word-boundary spaces instead of the
   snake_case form in the schema.
3. Hyphen→underscore normalization: `"web-extract"` → `"web_extract"`.  A few
   models normalize dashes to underscores inconsistently.
4. Trim leading/trailing whitespace.

**What we explicitly do NOT do:**
- Do not rewrite unknown tool names to their closest match (that's the
  registry's fuzzy match job and must be visible in the error response).
- Do not strip trailing digits or other valid identifier characters.
- Do not lowercase (all edgecrab tool names are already lowercase, but we
  should not silently recase third-party / engine tool names).

**Implementation:**
1. Add `fn sanitize_tool_name(name: &str) -> std::borrow::Cow<'_, str>` as a
   free function in `conversation.rs`.
2. At the top of `dispatch_single_tool`, shadow `name` with its sanitized form:
   ```rust
   let sanitized = sanitize_tool_name(name);
   let name: &str = sanitized.as_ref();
   ```
3. Log when sanitization occurs.
4. Add unit tests covering:
   - `<|channel|>commentary` suffix stripping
   - Space normalization
   - Hyphen normalization
   - Combined (space + channel token)
   - Already-clean name → Borrowed (zero allocation)

---

## Implementation Plan

| ID | File | Change |
|----|------|--------|
| FP53 | `edgecrab-cli/src/app.rs` | `current_spinner_frame()` helper + animated waiting title |
| FP54 | `edgecrab-core/src/conversation.rs` | `sanitize_tool_name()` + shadow at dispatch entry |

**Acceptance criteria:**
- `⧗ waiting…` is replaced by animated braille that cycles with the status bar
- Tool calls from Hermes 3 models with `<|channel|>` suffixes dispatch correctly
- `cargo test --workspace` passes with 0 failures
- `cargo clippy --workspace -- -D warnings` is clean

---

## Edge Cases

**FP53 edge cases:**
- `is_processing = true` but `display_state = Idle`: returns frame 0 (⠋). 
  This is a transient startup race and displays correctly.
- Very narrow terminal: the braille character is 1 cell wide, no overflow risk.
- ASCII glyph profile: the `current_spinner_frame()` helper returns a raw index;
  the caller can apply the profile check the same way `tick_spinner` does.
  NOTE: for simplicity, in `render_input` we use `SPINNER_FRAMES` directly
  since the input border title is always in the primary charset area.

**FP54 edge cases:**
- Tool name is only `<|channel|>` (no base): sanitized result is empty string →
  falls through to the registry's "Unknown tool: ''" branch and returns a
  NotFound error. Correct.
- Multiple `<|` sequences: we truncate at the first, so `a<|b|>c<|d|>e` →
  `a`. Correct — the base name always precedes the first token.
- Engine-domain tools have names from external schemas; they are typically
  already clean. The fast path (`!contains("<|")`) skips them at zero cost.
- Argument JSON is NOT sanitized by this function (args are handled by the
  existing `repair_tool_call_arguments` path).
