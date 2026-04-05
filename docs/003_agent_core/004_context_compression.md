# Context Compression

Verified against `crates/edgecrab-core/src/compression.rs`.

Compression keeps a long session usable without hard-dropping early messages.

## Current pipeline

```text
old history grows
  -> estimate tokens
  -> prune large tool outputs first
  -> keep recent tail intact
  -> summarize older content
  -> inject one system summary message
  -> continue the session with recent messages preserved
```

## Defaults in code

- `context_window`: `128_000`
- `threshold`: `0.50`
- `target_ratio`: `0.20`
- `protect_last_n`: `20`
- pressure warning fires at `85%` of the threshold

## Important constants

- `SUMMARY_PREFIX`: marker used to find and update prior summaries
- `PRUNED_TOOL_PLACEHOLDER`: replacement text for large old tool outputs
- `PROTECT_FIRST_N`: first `3` messages are preserved

## Failure behavior

If the LLM-based summary path fails, compression falls back to a structural summary instead of aborting the whole conversation.

## Why this matters

- it protects recent context
- it avoids re-sending giant historical tool output
- it preserves enough structure for tool-call history to remain coherent after compaction
