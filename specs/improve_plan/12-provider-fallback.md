# Spec 12: Provider Fallback on Retryable Errors

**Priority:** P1 — High
**Crate:** `edgecrab-core` (conversation.rs)
**Cross-ref:** [09-assessment-round2.md](09-assessment-round2.md) Gap 3
**Status:** ALREADY IMPLEMENTED

## Discovery

Upon implementation, found that EdgeCrab already has a complete provider
fallback system:

- `FallbackConfig` in `config.rs` (line 689)
- `fallback_route()` in `model_router.rs` (line 388)
- Full fallback logic in `execute_loop` (conversation.rs lines 1451-1515)
- Smart-routed model retry before fallback (lines 1408-1450)

The initial assessment missed this because the research focused on
`api_call_with_retry` (which does not contain fallback) rather than
its call site in `execute_loop` (which does).

**No code changes needed.**

```
+---------------------------------------------------------------+
|                   CURRENT (BROKEN)                            |
+---------------------------------------------------------------+
|                                                               |
|  Rate limit on anthropic/claude-sonnet-4                      |
|       |                                                       |
|       v                                                       |
|  Retry 1: anthropic/claude-sonnet-4 → 429                    |
|  Retry 2: anthropic/claude-sonnet-4 → 429 (same wall)        |
|  Retry 3: anthropic/claude-sonnet-4 → 429                    |
|       |                                                       |
|       v                                                       |
|  AgentError → conversation dead                               |
|                                                               |
+---------------------------------------------------------------+

+---------------------------------------------------------------+
|                   FIXED (FALLBACK CHAIN)                      |
+---------------------------------------------------------------+
|                                                               |
|  Rate limit on anthropic/claude-sonnet-4                      |
|       |                                                       |
|       v                                                       |
|  Retry 1: anthropic/claude-sonnet-4 → 429                    |
|       |                                                       |
|       v                                                       |
|  Fallback: openai/gpt-4o → 200 (success!)                    |
|       |                                                       |
|       v                                                       |
|  Next turn: restore anthropic/claude-sonnet-4                 |
|                                                               |
+---------------------------------------------------------------+
```

## Design

**Scope:** Minimal. We add a single `fallback_model` config key. On retryable
errors (rate limit, overloaded, 5xx), if a fallback is configured, we create
a one-shot provider and try once. Primary is restored on next turn.

**Config:**
```yaml
model_config:
  fallback_model: "openai/gpt-4o-mini"  # optional
```

**Implementation site:** The existing retry loop in `execute_loop` at the
API call section. After exhausting MAX_RETRIES on the primary, attempt one
call on the fallback.

## Implementation

**File:** `crates/edgecrab-core/src/conversation.rs`
**Location:** Inside the API retry loop, after the final retry fails.

The change is ~30 lines: after the `for attempt in 0..MAX_RETRIES` loop
exits with a retryable error, check `config.model_config.fallback_model`.
If set, create a provider via `create_provider_for_model` and make one
call. If that also fails, propagate the original error.

**File:** `crates/edgecrab-core/src/config.rs`
**Change:** Add `fallback_model: Option<String>` to `ModelConfig`.

## Edge Cases

1. Fallback model also rate-limited → propagate original primary error
2. No fallback configured → current behavior (retry 3x, fail)
3. Fallback provider creation fails → propagate original error
4. Smart routing active → fallback only kicks in after smart-routed model fails

## Tests

1. Primary 429 + fallback configured → falls back, returns response
2. Primary 429 + no fallback → error after 3 retries (current behavior)
3. Primary 429 + fallback also fails → original primary error propagated

## SOLID

- **SRP:** Fallback logic is a single code path in the retry loop.
- **OCP:** Adding more fallback strategies = extending the chain, not modifying.
- **DIP:** Uses `create_provider_for_model` — no hard-coded provider types.
