# 007 — Compression & Context Budget

**Cross-ref:** [003 loop triggers](./003-main-loop-physics.md) · [009 spill](./009-spill-turn-budget-results.md)

Context compression is **J7 (cost/liveness)** — keep the loop alive without blowing token budgets.

---

## Algorithm parity (strong)

Both implement the same four-phase pipeline:

```text
  ┌─────────────┐   ┌─────────────┐   ┌─────────────┐   ┌─────────────┐
  │ 1. Prune    │ → │ 2. Protect  │ → │ 3. Summarize│ → │ 4. Reassemble│
  │ old tools   │   │ head + tail │   │ middle (LLM)│   │ + summary blk│
  └─────────────┘   └─────────────┘   └─────────────┘   └─────────────┘
         │                  │                  │
         │                  │                  └─ iterative update of prior summary
         │                  └─ protect_last_n ≈ 20 messages
         └─ dedupe reads, 1-line summaries, strip old images
```

| Parameter | Hermes | EdgeCrab |
|-----------|--------|----------|
| Default threshold | 50% of context window | 50% (`CompressionParams.threshold`) |
| protect_last_n | ~20 messages | 20 (`protect_last_n`) |
| Summary prefix | `SUMMARY_PREFIX` constant | `SUMMARY_PREFIX` in compression.rs |
| LLM failure fallback | `_build_static_fallback_summary` | `compress_structural_only` |
| Aux model | `auxiliary_client` | Config `auxiliary.compress.model` |

**Code:** Hermes `agent/context_compressor.py::ContextCompressor.compress`  
**Code:** EdgeCrab `compression.rs::compress_with_llm`

---

## Trigger comparison

| Trigger point | Hermes | EdgeCrab |
|---------------|--------|----------|
| **Preflight** | `build_turn_context` — up to 3 passes on rough estimate | Per loop iteration estimate |
| **In-loop post-tools** | `should_compress(last_prompt_tokens)` | `check_compression_status_for_estimate` |
| **On API error** | `ClassifiedError.should_compress` | Partial in provider_call |
| **Defer preflight** | `should_defer_preflight_to_real_usage` | No equivalent |
| **Anti-thrashing** | Skip if last 2 compressions saved <10% each | Circuit breaker: 3 LLM fails → structural-only |

---

## Session lifecycle (Hermes advantage)

Hermes `conversation_compression.py` wraps the algorithm with **DB + session semantics** EdgeCrab lacks:

```text
  HERMES compress_context()
       │
       ├─► SQLite compression lock (prevent concurrent fork)
       │
       ├─► context_compressor.compress(messages)
       │
       ├─► TodoStore.format_for_injection() → synthetic user msg
       │
       └─► Session mode:
             ├─ Legacy: end_session → new child session_id (parent_session_id)
             └─ In-place: archive_and_compact (config compression.in_place)
```

| Feature | Hermes | EdgeCrab |
|---------|--------|----------|
| Compression lock | `try_acquire_compression_lock` | None |
| Todo snapshot post-compress | Yes — injected as user message | Ported intent in compression.rs |
| Session lineage | `parent_session_id` on rotate | **Not ported** — messages reshaped in-place |
| Goal migration | On session switch | Goals in SQLite (survive compress) |
| Memory session switch hooks | `memory_manager.on_session_switch` | Memory files unchanged |

**First principle:** EdgeCrab preserves **goals** outside message history (better for Ralph loop); Hermes preserves **session attribution** via rotation (better for forensics).

---

## Image handling

| Recovery | Hermes | EdgeCrab |
|----------|--------|----------|
| Strip old images in prune | Yes | Yes |
| Provider "image too large" | `try_shrink_image_parts_in_messages` | Provider error path in conversation |
| Computer-use screenshots | Prune in compressor | `prune_computer_use_screenshots` — keep last N |

---

## Compression + cache safety

Both respect **do not rebuild system prompt mid-turn**:

| | Hermes | EdgeCrab |
|---|--------|----------|
| System prompt | `active_system_prompt` in TurnContext; rebuilt only on compress | `cached_system_prompt` in SessionState |
| Dynamic injection | Goals, todos, plugin context → **messages** | Goals, steering, harness → **messages** |
| Anthropic cache | `apply_anthropic_cache_control` | Stable/dynamic prompt blocks |

---

## Structural-only fallback

When LLM summarization fails:

**Hermes:** `_build_static_fallback_summary` — stat-based message counts, tool names, file paths.

**EdgeCrab:** `compress_structural_only` + circuit breaker after 3 failures.

Both preserve recent tail; both may lose nuanced reasoning chains.

---

## Todo injection after compress

Hermes (canonical):

```python
# conversation_compression.py ~L511
todo_snapshot = agent._todo_store.format_for_injection()
# appended as user message — pending/in_progress only, char-capped
```

EdgeCrab ships equivalent synthetic user message in compression path — **Q3 parity intent**.

---

## Borrow list (EdgeCrab ← Hermes)

| Pattern | Hermes anchor | Priority |
|---------|---------------|----------|
| Compression session lock | `conversation_compression.py` | P1 — prevents background_review race |
| `parent_session_id` lineage | session DB rotate | P2 — forensics |
| Defer preflight to real usage | `should_defer_preflight_to_real_usage` | P2 — avoids unnecessary compress |
| Anti-thrashing guard | `<10% savings × 2` | P2 — reduces compress churn |

**Explicit non-borrow:** auto-inject full spill into post-compress context (turn budget explosion).
