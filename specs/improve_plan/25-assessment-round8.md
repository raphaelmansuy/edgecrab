# Round 8 — Prompt Size / Session / Compression Assessment

> **Scope:** Context compression pipeline, prompt cache management, session token tracking, and role-collision correctness in the summary injection step.
> 
> **Reference implementations:** `hermes-agent/agent/context_compressor.py`, `hermes-agent/agent/prompt_caching.py`, `hermes-agent/run_agent.py`, and Claude Code's prompt-cache strategy.
>
> **Baseline:** 353 tests passing after Round 7.

---

## Brutal Honest Gap Matrix

| Area | EdgeCrab (v0.8.x) | Hermes Agent (reference) | Gap |
|------|-------------------|--------------------------|-----|
| **Summary injection role** | Always `system_summary` role | Detects consecutive-role collisions; can merge into tail message | EdgeCrab may produce `[user, summary, user]` sequences (spec-violating on some providers) |
| **Summary failure cooldown** | None — re-attempts every turn | 600s cooldown after failure | EdgeCrab calls the LLM summarizer on every compression trigger even after repeated failures, burning budget on doomed API calls |
| **Compression note in system prompt** | Not injected | Appended to system prompt on first compression ("Some earlier turns compacted...") | Agents don't know compression happened; may redo work without the note |
| **Prompt cache TTL** | Hardcoded `ephemeral` (5 min implied) | Configurable `cache_ttl`: `"5m"` or `"1h"` | No config knob; 1h cache useful for long interactive sessions |
| **Session token tracking** | `CanonicalUsage` per-turn only | Cumulative `session_cache_read_tokens`, `session_cache_write_tokens`, `session_cache_*` | No session-level rollup; `/cost` can't show cache savings across turns |
| **`protect_last_n` vs token budget** | Both implemented ✓ | Both implemented ✓ | Parity |
| **Orphan pair sanitization** | Implemented ✓ | Implemented ✓ | Parity |
| **Iterative summary update** | Implemented (prior summary extraction) ✓ | Implemented ✓ | Parity |
| **Structural fallback** | Implemented (no LLM) ✓ | Static fallback marker ✓ | Parity |
| **Compression circuit breaker** | `FP12` from Round 3 ✓ | 600s cooldown | Different mechanism — both prevent runaway re-compression |
| **Tool output prune 200-char threshold** | 200 chars ✓ | 200 chars ✓ | Parity |
| **8-section summary template** | ✓ (incl. Tools & Patterns section) | ✓ | Parity |
| **Soft ceiling 1.5× on tail budget** | Not present (strict cut) | `soft_ceiling = int(token_budget * 1.5)` | EdgeCrab may cut inside oversized tool outputs more aggressively |

---

## First Principles (FP29–FP33)

### FP29 — Summary Failure Cooldown

**Problem:** `llm_summarize` fails → `unwrap_or_else` calls `build_summary()` (structural fallback). No cooldown. On next turn, threshold is still breached → same LLM call is attempted again, fails again, burns retry budget indefinitely.

**Hermes Solution:** `_summary_failure_cooldown_until = time.monotonic() + 600`. Skips LLM summarization for 10 minutes after any failure; uses structural fallback for that window.

**EdgeCrab Fix:** Add a `last_failure_at: Option<std::time::Instant>` field to `CompressionState` tracked in `SessionState`. When `llm_summarize` fails, set `last_failure_at = Some(Instant::now())`. In `compress_with_llm`, check the cooldown before attempting LLM summarization. Fallback to structural if in cooldown.

**Where:** `crates/edgecrab-core/src/compression.rs` (new `CompressionCooldown` helper) + `crates/edgecrab-core/src/agent.rs` (`SessionState` carries cooldown state).

---

### FP30 — Role-Collision-Safe Summary Injection

**Problem:** `compress_with_llm` always inserts `Message::system_summary(...)`. After Phase 5 assembly:
```
[system, user, assistant, SUMMARY(system_summary), user, ...]
```
The `system_summary` role is EdgeCrab-internal and gets mapped to `role: "system"` in `build_chat_messages`. Most providers allow multiple system messages, but there are edge cases:
1. If the last head message is `system` (e.g. another `system_summary`), adjacent system messages can confuse providers.  
2. Gemini/VertexAI require strict alternation; adjacent `system` messages may be rejected.

**Hermes Solution:** Checks `last_head_role` and `first_tail_role`. Picks `"user"` or `"assistant"` to avoid consecutive same-role. Falls back to merging summary content into the first tail message when both roles would collide.

**EdgeCrab Fix:** After assembling `result`, detect role collision and either:
- Use `Message::user(prefixed)` / `Message::assistant(prefixed)` for the summary (not `system_summary`), OR
- Merge `prefixed` content into the first tail message's text.

The `system_summary` role is still the correct choice for most cases (system messages are allowed to be non-contiguous). Add a guard only when the immediately preceding head message is already `system_summary`.

---

### FP31 — Prompt Cache TTL Configuration

**Problem:** `CachePromptConfig::default()` uses a hardcoded `ephemeral` marker with no TTL field. Anthropic now supports `cache_control: {type: "ephemeral", ttl: "1h"}` for 1-hour cache windows (introduced 2025-Q1). Interactive dev sessions benefit enormously from 1h TTL.

**Hermes Solution:** `apply_anthropic_cache_control(messages, cache_ttl="5m"|"1h")`. User sets `prompt_cache_ttl: "1h"` in config.yaml.

**EdgeCrab Fix:** Add `cache_ttl: Option<CacheTtl>` to `CachePromptConfig` in `edgequake-llm/src/cache.rs`. Add `prompt_cache_ttl` key to `ModelConfig` / `AppConfig`. Thread through `prompt_cache_config_for()` in `conversation.rs`.

**Where:** `crates/edgequake-llm/src/cache.rs`, `crates/edgecrab-core/src/config.rs`, `crates/edgecrab-core/src/conversation.rs`.

---

### FP32 — Session-Level Cache Token Rollup

**Problem:** `CanonicalUsage` captures per-turn usage (prompt, completion, cache_read, cache_write). There is no session-level aggregation. `/cost` shows per-call estimates. Users cannot see total cache savings for the session.

**Hermes Solution:** `session_cache_read_tokens`, `session_cache_write_tokens` counters incremented every turn. Summary displayed on session end.

**EdgeCrab Fix:** Add `session_cache_read_tokens: u64` + `session_cache_write_tokens: u64` + `session_total_cost_usd: f64` to `SessionState`. Accumulate in `execute_loop` after each API call. Expose via `ConversationResult` (new `session_totals` field).

**Where:** `crates/edgecrab-core/src/agent.rs` (`SessionState`), `crates/edgecrab-core/src/conversation.rs` (accumulate after each turn).

---

### FP33 — Compression Note in System Prompt (One-Shot)

**Problem:** When compression fires on turn 12, the agent's frozen `cached_system_prompt` does not change. The agent has no hint that earlier context was compacted. Without this hint, the agent may:
- Look for files/state it remembers from earlier turns but that were replaced by the summary.
- Express uncertainty about whether work was actually done.

**Hermes Solution:** On first compression, appends `"\n\n[Note: Some earlier conversation turns have been compacted into a handoff summary...]"` to the system prompt's content at index 0.

**EdgeCrab Fix:** Track `first_compression_done: bool` in `SessionState`. After the first `compress_with_llm` call, append a one-time note to `session.cached_system_prompt`. Subsequent compressions do NOT modify the system prompt (to preserve cache validity).

**Why one-shot and not repeated:** Modifying the system prompt invalidates Anthropic's prompt cache. We do this ONCE on first compression (cache must be re-built anyway because the message list changed), never again.

**Where:** `crates/edgecrab-core/src/agent.rs` (`SessionState.first_compression_done`), `crates/edgecrab-core/src/conversation.rs` (post-compression hook).

---

## Implementation Plan

All five FPs (29–33) are small, surgical changes. No cross-crate interface changes required except FP31 (which touches `edgequake-llm`'s `CachePromptConfig`).

**Execution order:** FP29 → FP30 → FP31 → FP32 → FP33

Each has unit tests. FP31 has one cross-crate test in `edgecrab-core` (verifying TTL propagation). FP32 has a mock-provider test for accumulation. FP33 has a test verifying the note is injected exactly once.

**Risk:** Low. All changes are additive (new fields with sensible defaults). No existing API surface changes. Cache semantics unchanged for providers that don't support 1h TTL.
