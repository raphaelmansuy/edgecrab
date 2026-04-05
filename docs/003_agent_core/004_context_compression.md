# 003.004 — Context Compression & Prompt Caching

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 003.002 Conversation Loop](002_conversation_loop.md) | [→ 003.003 Prompt Builder](003_prompt_builder.md)  
> **Source**: `edgecrab-core/src/compression.rs` — verified against implementation  
> **Parity**: hermes-agent `agent/context_compressor.py` v0.4.x

---

## 1. Why Context Compression

Long conversations accumulate tokens until they exceed the model's context window. Hard truncation loses important early context (original goals, key decisions, file paths changed). EdgeCrab's compression pipeline summarises old messages while preserving recent ones verbatim.

```
Before compression:
  [system] [user1] [asst1] [tool1] ... [userN-5] ... [userN]
  ├─ head ─┤                           └──── tail (recent) ───┘

After compression:
  [system] [user1] [asst1]  [SUMMARY_MSG]  [userN-5] ... [userN]
  ├── head ──────────────┤  └─ LLM text ─┘  └──── tail kept ────┘
```

---

## 2. Six-Phase Compression Pipeline

```
compress_with_llm(messages, params, provider)
│
├── Phase 1: Prune tool outputs (cheap, no LLM)
│       Replace large tool results (>200 chars) with PRUNED_TOOL_PLACEHOLDER.
│       Often halves the prompt before any LLM work.
│
├── Phase 2: Boundary determination
│       head_end = align_boundary_forward(PROTECT_FIRST_N=3)
│       tail_token_budget = threshold_tokens × target_ratio (default 0.20)
│       tail_start = find_tail_cut_by_tokens(head_end, tail_budget, protect_last_n)
│       Both boundaries aligned to avoid splitting tool_call/tool_result groups.
│
├── Phase 3: Extract prior summary (iterative update)
│       Search for a SUMMARY_PREFIX block in message history.
│       When found → feed as "prior summary" for an UPDATE rather than
│       re-summarising from scratch (cheaper, more coherent).
│
├── Phase 4: LLM summarization
│       Prompt template: 8 sections (see §6).
│       On LLM failure → build_summary() structural fallback (always succeeds).
│
├── Phase 5: Assemble
│       result = head_messages + [summary_message] + tail_messages
│
└── Phase 6: Orphan sanitization
        Remove tool results whose parent tool_call was summarised away.
        Inject one-line stub results for tool_calls that lost their results.
        → Always produces an API-compliant message list.
```

---

## 3. Constants

```rust
// edgecrab-core/src/compression.rs

pub const SUMMARY_PREFIX: &str =
    "[CONTEXT COMPACTION] Earlier turns were summarised to reclaim context window space.\n\n";

pub const PRUNED_TOOL_PLACEHOLDER: &str =
    "[tool output pruned — reclaimed context window space]";

const PROTECT_FIRST_N: usize = 3;       // always keep system + first exchange
const MIN_SUMMARY_TOKENS: usize = 2_000;
const SUMMARY_RATIO: f32 = 0.20;        // content_tokens × 0.20 → budget
const SUMMARY_TOKENS_CEILING: usize = 12_000;
const CHARS_PER_TOKEN: usize = 4;       // rough estimation without tokenizer
const STUB_TOOL_RESULT: &str = "[Result from earlier conversation — see context summary above]";
```

---

## 4. CompressionParams

```rust
pub struct CompressionParams {
    pub context_window: usize,   // default: 128_000 tokens
    pub threshold: f32,          // compress when estimated ≥ window × threshold (default 0.50)
    pub target_ratio: f32,       // tail_budget = threshold_tokens × target_ratio (default 0.20)
    pub protect_last_n: usize,   // floor: always keep at least N recent messages (default 20)
}
```

### Default values

| Field | Default | Notes |
|---|---|---|
| `context_window` | 128 000 | API response `usage.prompt_tokens` gives exact count |
| `threshold` | 0.50 | Compression fires at 50 % of context window |
| `target_ratio` | 0.20 | Tail = 20 % of threshold_tokens |
| `protect_last_n` | 20 | Floor: at least 20 recent messages always kept |

---

## 5. CompressionStatus — Pressure Warnings

```rust
pub enum CompressionStatus {
    Ok,                 // below 85% of threshold
    PressureWarning,    // between 85% and 100% of threshold → UI warning emitted
    NeedsCompression,   // at or above threshold → compression fires
}
```

`check_compression_status()` is called every iteration of the conversation loop:

```
estimated_tokens ≥ threshold_tokens              → NeedsCompression (compress now)
estimated_tokens ≥ threshold_tokens × 0.85       → PressureWarning  (emit event once)
otherwise                                        → Ok
```

When `PressureWarning` fires, the conversation loop emits `StreamEvent::ContextPressure { estimated_tokens, threshold_tokens }` exactly once per pressure episode (suppressed on subsequent iterations until compression clears the warning level).

---

## 6. Eight-Section Summary Template

All LLM-generated summaries use this exact Markdown structure (hermes-agent 0.4.x format):

```markdown
## Goal
[What the user is trying to accomplish]

## Constraints & Preferences
[User preferences, coding style, constraints, important decisions]

## Progress
### Done
[Completed work — specific file paths, commands run, results obtained]
### In Progress
[Work currently underway]
### Blocked
[Any blockers or issues encountered]

## Key Decisions
[Important technical decisions and why they were made]

## Relevant Files
[Files read, modified, or created — with brief note on each]

## Next Steps
[What needs to happen next to continue the work]

## Critical Context
[Specific values, error messages, configuration details, or data that would be lost without explicit preservation]
```

On iterative compression (prior summary exists), the prompt asks the LLM to **UPDATE** rather than re-summarise — cheaper and more coherent.

### Summary Token Budget

```
budget = content_tokens × 0.20
ceiling = min(context_window × 0.05, 12_000)
final_budget = max(2_000, min(budget, ceiling))
```

Example: 128 K context window → ceiling = `min(6_400, 12_000)` = **6 400 tokens**.

---

## 7. Boundary Alignment

Two functions ensure boundaries never split a `tool_call`/`tool_result` group:

| Function | Purpose |
|---|---|
| `align_boundary_forward(messages, idx)` | Slides head boundary forward past leading tool results |
| `align_boundary_backward(messages, idx)` | Pulls tail boundary before a parent assistant-with-tool-calls |

**Why**: An assistant message with `tool_calls` must always be followed by a tool result for each call_id. If compression splits this group, the API rejects the message list. Alignment guarantees the entire group (assistant + results) is either in the summarised region or in the tail.

---

## 8. Orphan Sanitization

After assembling `head + summary + tail`, `sanitize_orphan_pairs()` runs a final check:

1. **Orphaned results** (result references a call_id that was summarised away) → **removed**.  
2. **Orphaned calls** (assistant has tool_calls but result was dropped) → **stub result injected**:  
   `"[Result from earlier conversation — see context summary above]"`

This ensures the assembled list is always API-compliant regardless of where compression boundaries fall.

---

## 9. Prompt Caching (Anthropic)

EdgeCrab supports Anthropic's prompt caching via `CachePromptConfig`:

```rust
// conversation.rs — per-turn config
let cache_cfg = if config.model_config.prompt_caching {
    Some(CachePromptConfig::default())
} else {
    None
};
let chat_messages = build_chat_messages(
    session.cached_system_prompt.as_deref(),
    &session.messages,
    cache_cfg.as_ref(),
);
```

`apply_cache_control()` (edgequake-llm) pins `cache_control: {"type": "ephemeral"}` breakpoints on:
- The system prompt (slot 1 — most stable, cheapest re-use)
- The last assistant message (slot 2 — changes each turn)

**Why stable system prompt matters**: The LLM-generated summary is injected as a `Message::system_summary()` (role=`system`), not prepended to the cached system prompt. This way the main system prompt (identity, tools, skills) stays entirely stable across turns and can always be cache-hit, even as the summary message changes.

---

## 10. Token Estimation

```rust
pub fn estimate_tokens(messages: &[Message]) -> usize {
    messages.iter()
        .map(|m| (m.text_content().len() / 4) + 4)
        .sum()
}
```

- ~4 chars/token (GPT/Claude heuristic — fast, no tokenizer dependency)  
- 4-token overhead per message (role/metadata)  
- Good enough for the compression threshold check; exact counts come from API response `usage` fields

---

## 11. Before/After Example

```
Before (30 messages, estimated ~65 000 tokens > 50% threshold):
  [system][user1][asst1][user2][asst2+tools][tool][tool]...[user28][asst29][user30]

After compress_with_llm():
  [system][user1][asst1]
  [system_summary: "[CONTEXT COMPACTION] ... ## Goal\n... ## Progress\n..."]
  [user28][asst29][user30]
  ← 6 messages, roughly 8 000 tokens
```

---

## 12. Configuration Reference

```toml
# ~/.edgecrab/config.toml (or CLI flags)
[model]
prompt_caching = true      # enable Anthropic cache_control breakpoints

# CompressionParams are currently hardcoded to defaults.
# Future: expose compression.threshold / compression.target_ratio in config.
```

See [→ 009 Config & State](../009_config_state/001_config_state.md) for full config schema.


## 6. Compression Triggers

The conversation loop triggers compression in two phases:

```
Phase 1 — Preflight (before main loop begins):
  if needs_compression(messages, params):
      compress_with_llm() up to 3 passes

Phase 2 — Mid-loop (after tool calls generate large output):
  if needs_compression(messages, params):
      compress_with_llm() once before next API call
```

## 7. Structural Fallback

When the LLM summarization call fails, a stat-based summary is generated:

```
[CONTEXT COMPACTION] <N> earlier messages summarized.
  Turns: <user_count> user, <assistant_count> assistant
  Tool calls made: <M>

  Key excerpts:
  [first 200 chars of earliest user message]
  [first 200 chars of last assistant message before cutoff]
```

## 8. Iterative Updates

Each new compression pass checks for an existing `SUMMARY_PREFIX` block in the messages. If found, it's included as prior context in the summarization prompt:

```
Summarization prompt structure:
  "Prior summary: <existing summary>"
  "New messages to integrate: <pruned old messages>"
  "Produce an updated structured summary covering both."
```

Summaries **improve** with each subsequent compaction rather than restarting from scratch.

## 9. Config Integration

```yaml
# config.yaml
compression:
  enabled: true
  threshold: 0.50       # Compress at 50% of context window
  protect_last_n: 20    # Always keep last 20 messages
  summary_model: null   # Optional: use cheaper model for summaries
```
