# Context Compression 🦀

> **Verified against:** `crates/edgecrab-core/src/compression.rs`

---

## Why compression exists

A 90-iteration agent working on a large codebase can generate 50,000+ tokens
of conversation history. Most LLMs have context windows of 128,000–200,000
tokens. Without intervention, long sessions either hit the provider's context
limit (hard error) or silently drop early messages (loss of intent and prior decisions).

EdgeCrab uses a 5-pass pipeline to keep sessions alive without losing the
information that matters.

🦀 *`hermes-agent` (Python) raised an unhandled exception at the provider context limit.
OpenClaw silently slices earlier tokens. EdgeCrab compresses intelligently and keeps fighting.*

---

## Defaults from source

```rust
// compression.rs
const DEFAULT_CONTEXT_WINDOW: usize = 128_000;
const DEFAULT_THRESHOLD:      f64   = 0.50;    // compress at 50% full
const DEFAULT_TARGET_RATIO:   f64   = 0.20;    // target 20% of window after
const DEFAULT_PROTECT_LAST_N: usize = 20;      // always keep last 20 messages
const PROTECT_FIRST_N:        usize = 3;       // always keep first 3 messages
const PRESSURE_WARNING_PCT:   f64   = 0.85;    // warn at 85% of threshold
```

Key constants exported:
```rust
pub const SUMMARY_PREFIX: &str =
    "[CONTEXT COMPACTION] Earlier turns were summarised to reclaim context window space.\n\n";

pub const PRUNED_TOOL_PLACEHOLDER: &str =
    "[tool output pruned — reclaimed context window space]";
```

---

## When compression fires

```
  After every LLM response:
  check_compression_status(messages, params, context_window)
        │
        ├── Ok                         → nothing to do
        │
        ├── PressureWarning             → emit StreamEvent::ContextPressure
        │    estimated > 85% of         to warn the user (no compression yet)
        │    threshold×window
        │
        └── Compressed                 → run compress_with_llm() pipeline
             estimated > threshold×window    (currently: > 50% of 128K = 64K)
```

Token estimation uses a fast character-count approximation (~4 chars/token)
rather than a full tokeniser — fast enough for hot-path checks, accurate
enough for triggering decisions.

---

## The 5-pass compression pipeline

```
  Input: full message history

  ┌────────────────────────────────────────────────────────────────┐
  │  PASS 1 — Tool output pruning / spill (no LLM, cheap)          │
  │                                                                │
  │  For every tool_result message:                                │
  │    if content.len() > LARGE_OUTPUT_THRESHOLD:                  │
  │      with spill context: write full result to artifact file    │
  │                         keep preview stub in history            │
  │      without spill context: replace with PRUNED_TOOL_PLACEHOLDER│
  │      "[tool output pruned — reclaimed context window space]"   │
  │                                                                │
  │  Typically removes 60-80% of tokens in long sessions           │
  └────────────────────────────────────────────────────────────────┘
                              │
                              ▼
  ┌────────────────────────────────────────────────────────────────┐
  │  PASS 2 — Boundary determination                               │
  │                                                                │
  │  Identify:                                                     │
  │    protected_head   = messages[0..PROTECT_FIRST_N]   (3)       │
  │    protected_tail   = messages[-protect_last_n..]    (20)      │
  │    compression_zone = messages[3..-20]                         │
  └────────────────────────────────────────────────────────────────┘
                              │
                              ▼
  ┌────────────────────────────────────────────────────────────────┐
  │  PASS 3 — LLM summary of compression_zone                      │
  │                                                                │
  │  System prompt:                                                │
  │    "Summarise the following conversation into 8 sections:      │
  │     1. Goal   2. Constraints   3. Progress   4. Decisions      │
  │     5. Files  6. Next Steps    7. Critical Context  8. Errors" │
  │                                                                │
  │  Result: one system message with SUMMARY_PREFIX prepended      │
  └────────────────────────────────────────────────────────────────┘
                              │
                    ┌─────────┴──────────┐
                    │ LLM failure?        │
                    ▼ Yes                ▼ No (normal)
  ┌──────────────────────┐   ┌───────────────────────────────────┐
  │  PASS 4 — Structural  │   │  Insert SUMMARY_PREFIX message +  │
  │  fallback summary    │   │  reassemble: head + summary +      │
  │                      │   │  tail                              │
  │  Generate summary    │   └───────────────────────────────────┘
  │  from metadata +     │
  │  message types only  │
  │  (no LLM needed)     │
  └──────────────────────┘
                              │
                              ▼
  ┌────────────────────────────────────────────────────────────────┐
  │  PASS 5 — Orphan sanitisation                                  │
  │                                                                │
  │  walk final message list:                                      │
  │    │                                                           │
  │    ├── orphaned tool_result (no matching tool_call)            │
  │    │     → remove (would cause API error)                      │
  │    │                                                           │
  │    └── orphaned tool_call in assistant message                 │
  │          → inject stub tool_result                             │
  │            "[result not available after context compression]"  │
  └────────────────────────────────────────────────────────────────┘
```

---

## The 8-section summary format

The LLM is instructed to produce this exact structure in Pass 3:

| Section | Content |
|---|---|
| **Goal** | The user's original overall objective |
| **Constraints** | Rules and limits established during the session |
| **Progress** | What has been completed or confirmed |
| **Decisions** | Key decisions made and the reasoning behind them |
| **Files touched** | Files read, written, or modified |
| **Next steps** | What was planned before compression |
| **Critical context** | Any fact that must not be lost |
| **Errors encountered** | Failures and how they were resolved |

---

## Example: before and after

**Before compression** (simplified):
```
  system:    "You are EdgeCrab..."  [5,000 tokens]
  user:      "Refactor the auth module"
  assistant: "I'll start by reading the files"
  tool:      [read_file content: 8,000 tokens of source]
  tool:      [read_file content: 6,000 tokens of source]
  assistant: "I've read both files. Here's my plan..."
  user:      "Proceed"
  assistant: "Writing the refactored version..."
  tool:      [write_file result]
  ...  [60 more messages]
  Total: ~95,000 tokens
```

**After compression** (Pass 1 + 3):
```
  system:    "You are EdgeCrab..."  [5,000 tokens]
  system:    "[CONTEXT COMPACTION] Goal: Refactor auth module.
              Progress: Read auth.rs and session.rs. Wrote
              new auth.rs with JWT support.
              Files touched: src/auth.rs, src/session.rs
              ..."  [~800 tokens]
  user:      [last 20 messages preserved]
  ...
  Total: ~15,000 tokens
```

When the compressor has spill context, pruned tool messages can instead become:

```text
[tool_result_spill]
tool: file_search
lines: 2847
bytes: 98304
artifact: .edgecrab-artifacts/ses_abc123/file_search_001.md
showing: 80/2847 lines (first 3%)
```

---

## Tips

> **Tip: `StreamEvent::ContextPressure` is the early warning sign.**
> When you see "context pressure" in the TUI, the next few iterations will
> trigger compression. If you're mid-task, consider finishing the current
> sub-task before the compression fires — it may lose some tool output detail.

> **Tip: `force_compress()` on `Agent` triggers compression immediately.**
> Useful in tests or when you want to checkpoint a long session before
> handing off to a sub-agent.

> **Tip: The `protect_last_n` default of 20 means the most recent 20 messages
> are ALWAYS preserved verbatim.** Compression never truncates your immediate
> context — only older history is summarised.

---

## FAQ

**Q: Does compression lose information?**
Some detail is lost in the summary, but critical facts are preserved by the
8-section format. The LLM is instructed to retain all decisions, errors, and
file paths — this is more structured than `hermes-agent` or OpenClaw's naive truncation.

**Q: What if the LLM fails to produce a summary?**
Pass 4 kicks in: a structural summary is generated from message type metadata
(no content). This is lower quality but never crashes the session.

**Q: How does compression affect tool call history integrity?**
Pass 5 (orphan sanitisation) ensures the final message list is always valid.
Orphaned `tool_result` messages (whose `tool_call` was pruned) are removed.
Orphaned `tool_call` references get stub results inserted.

**Q: If a tool result was spilled to disk, can the agent still use it after compression?**
Yes. The stub keeps the relative artifact path, and the artifact lives under the
active workspace so existing file tools can inspect it later.

**Q: Can I disable compression?**
Set `compression.enabled = false` in `~/.edgecrab/config.yaml`. The agent
will hit the provider's context limit instead and get a hard `ContextLimit` error.
Not recommended for long sessions.

---

## Cross-references

- Where compression is triggered in the loop → [Conversation Loop](./002_conversation_loop.md)
- Token estimation and cost tracking → [Data Models](../010_data_models/001_data_models.md)
- Session message format preserved through compression → [Session Storage](../009_config_state/002_session_storage.md)
