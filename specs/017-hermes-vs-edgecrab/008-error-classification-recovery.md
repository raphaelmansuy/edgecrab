# 008 — Error Classification & Recovery

**Cross-ref:** [003 loop recovery](./003-main-loop-physics.md) · [005 dispatch recovery](./005-tool-dispatch-and-parallelism.md)

API and tool failures must map to a **single next action**. Hermes centralizes this; EdgeCrab splits across modules.

---

## Architecture comparison

```text
  HERMES                              EDGECRAB
  ══════                              ════════

  classify_api_error()                api_call_with_retry()
       │                                   │
       ▼                                   ├─ rate limit → backoff + retry
  ClassifiedError                          ├─ context overflow → compress branch
       │                                   ├─ stream stall → FINISH_REASON_STREAM_INTERRUPTED
       ├─ retryable: bool                  └─ Copilot/local → skip double retry
       ├─ should_compress: bool
       ├─ should_rotate_credential
       └─ should_fallback
            │
            ▼
  conversation_loop branches          mutation_turn_policy::continuation_user_message
  (retry / failover / compress)            +
                                       recovery_catalog (tool errors)
```

---

## Hermes `FailoverReason` taxonomy

**Code:** `agent/error_classifier.py`

| Category | Values |
|----------|--------|
| Auth | `auth`, `auth_permanent` |
| Quota | `billing`, `rate_limit` |
| Server | `overloaded`, `server_error` |
| Transport | `timeout` |
| Context | `context_overflow`, `payload_too_large`, `image_too_large` |
| Model/policy | `model_not_found`, `provider_policy_blocked`, `content_policy_blocked` |
| Format | `format_error`, `invalid_encrypted_content`, `multimodal_tool_content_unsupported` |
| Provider-specific | `thinking_signature`, `long_context_tier`, `oauth_long_context_beta_forbidden`, `llama_cpp_grammar_pattern` |
| Catch-all | `unknown` |

**Priority pipeline:** content-policy → thinking-signature → tier gates → HTTP status → error code → message patterns → SSL → disconnect+large-session → transport → unknown.

---

## EdgeCrab error handling (distributed)

| Failure class | Owner | Behavior |
|---------------|-------|----------|
| Rate limit / 429 | `provider_call.rs` | Parse retry-after, jittered backoff, cancel race |
| Context pressure | `compression.rs` + loop | Trigger compress before retry |
| Stream interrupt | `provider_call.rs` | `FINISH_REASON_STREAM_INTERRUPTED` → continuation inject |
| Length cap (no tools) | `mutation_turn_policy.rs` | `ContinuationFailureClass::LengthWithoutTools` |
| Partial stream tools | `mutation_turn_policy.rs` | `StreamInterruptedAfterPartial` |
| Invalid tool JSON | `mutation_turn_policy.rs` | `InvalidToolArguments` + recovery msg |
| Tool policy reject | `recovery_catalog.rs` | Structured `ToolError` + suggestions |
| Provider failover | `model_router.rs` / agent | Hot-swap provider on auth/billing |

**Gap:** No single `FailoverReason` enum consumed by loop + doctor + TUI.

---

## Continuation prompts (failure-class branching)

Hermes `_get_continuation_prompt(is_partial_stub, dropped_tools)` in `conversation_loop.py`:

| Condition | Prompt intent |
|-----------|---------------|
| Partial stream stub | Re-emit dropped tool calls |
| Length cap | Summarize progress, continue with smaller scope |
| Empty response | Nudge model to produce output |

EdgeCrab `continuation_user_message(class: ContinuationFailureClass, ...)` in `mutation_turn_policy.rs`:

| Class | Prompt intent |
|-------|---------------|
| `LengthWithoutTools` | Continue without repeating full context |
| `StreamInterruptedNoTools` | Recover from draft stall |
| `StreamInterruptedAfterPartial` | Complete partial tool JSON |
| `InvalidToolArguments` | Fix args, use patch not full write |

**Parity:** Mechanism aligned; Hermes has more provider-specific branches (Ollama context, Nous entitlement, image shrink).

---

## Tool-level recovery (EdgeCrab advantage)

EdgeCrab `recovery_catalog.rs` produces structured JSON recovery for **deterministic** tool failures:

| Function | Scenario |
|----------|----------|
| `tool_argument_budget_exceeded` | Pre-dispatch arg too large |
| `write_file_path_exists_abort` | Accidental overwrite |
| `stale_file_context` | Patch without fresh read |
| `mutation_payload_too_large` | Incremental edit guidance |
| `browser_navigate_blocked` | SSRF/preview recovery |
| `unknown_tool` | Invalid tool name |

Hermes returns similar guidance but less uniformly structured — more ad-hoc per tool.

**First principle:** Structured recovery beats prose parsing for loop stability.

---

## Credential rotation & failover

| | Hermes | EdgeCrab |
|---|--------|----------|
| Pool rotate on 429 | `credential_pool.py` | Provider-specific in proxy/upstream |
| Failover runtime | `_restore_primary_runtime` in prologue | Provider hot-swap via `Agent` |
| Nous 401 guidance | `_print_nous_entitlement_guidance` | OAuth in `edgecrab-core/src/oauth/` |
| Content policy | `_content_policy_blocked_result` — deterministic exit | Provider error surface |

---

## Compression-on-error path

```text
  API error
      │
      ▼
  classify_api_error (Hermes)          provider_call retry loop (EdgeCrab)
      │                                      │
      ├─ should_compress=true ───────────────┼─► compress_with_llm
      │                                      │
      └─ should_fallback=true ───────────────┼─► switch provider/model
```

Hermes makes compress-vs-failover an **explicit classifier output**. EdgeCrab often discovers overflow via error message parsing inline.

---

## Borrow matrix

| Borrow into EdgeCrab | Hermes source | ROI |
|---------------------|---------------|-----|
| Unified `FailoverReason` enum | `error_classifier.py` | High — doctor + loop + metrics |
| Ollama context preflight | `_ollama_context_limit_error` | Medium |
| Image shrink recovery | `try_shrink_image_parts_in_messages` | Medium |
| 402 disambiguation (rate vs billing) | `_classify_402` | Medium |
| Nous entitlement inline guidance | `_print_nous_entitlement_guidance` | Low (provider-specific) |

**Keep in EdgeCrab:** `recovery_catalog.rs` structured tool errors — Hermes should adopt this pattern, not vice versa.
