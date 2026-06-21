# 011 — Borrow / Reject Matrix

**Cross-ref:** All 001–010 · [015/011 parity map](../015-improve-harness-and-agent/011-hermes-parity-map.md)

**Rule:** adopt **mechanism**, not file layout — one owner module per row (DRY).

---

## Summary matrix

```text
  ┌──────────────────────────────┬─────────┬──────────┬─────────────────────────┐
  │ Pattern                      │ Hermes  │ EdgeCrab │ Action                  │
  ├──────────────────────────────┼─────────┼──────────┼─────────────────────────┤
  │ Turn prologue module         │    ✓    │    ✗     │ BORROW → turn_prologue  │
  │ Turn epilogue module         │    ✓    │    △     │ BORROW → turn_epilogue  │
  │ Unified error classifier     │    ✓    │    △     │ BORROW → failover.rs    │
  │ Compression session lock     │    ✓    │    ✗     │ BORROW                  │
  │ parent_session_id lineage    │    ✓    │    ✗     │ BORROW (P2)             │
  │ Background memory review     │    ✓    │    ✗     │ BORROW (optional)       │
  │ Budget exhaustion summary    │    ✓    │    △     │ BORROW                  │
  │ Continuation by failure class│    ✓    │    ✓     │ KEEP (parity)           │
  │ 3-layer spill stack          │    ✓    │    ✓     │ KEEP (parity)           │
  │ Tool loop guardrails         │    ✓    │    ✓     │ KEEP (defaults differ)  │
  │ CompletionPolicy types       │    ✗    │    ✓     │ KEEP (EdgeCrab leads)     │
  │ HarnessSnapshot gates        │    △    │    ✓     │ KEEP + harden enforce   │
  │ HarnessTurnAdvisory          │    ✗    │    ✓     │ KEEP (EdgeCrab-only)      │
  │ harness_analyzer doctor      │    ✗    │    ✓     │ KEEP                    │
  │ replay CI harness tests      │    △    │    ✓     │ KEEP                    │
  │ Global allow_private_urls    │    ✓    │    ✗     │ REJECT                  │
  │ hard_stop default OFF        │    ✓    │    ✗     │ REJECT for EdgeCrab     │
  │ Auto-inject spill in context │    ✗    │    ✗     │ REJECT (both)           │
  │ Profile-isolated security    │    △    │    △     │ REJECT → merge on load  │
  └──────────────────────────────┴─────────┴──────────┴─────────────────────────┘

  Legend: ✓ = shipped   △ = partial   ✗ = absent
```

---

## High-ROI borrows (EdgeCrab ← Hermes)

| Priority | Pattern | Hermes anchor | EdgeCrab target | Why |
|----------|---------|---------------|-----------------|-----|
| **P0** | Turn prologue extract | `turn_context.py::build_turn_context` | `turn_prologue.rs` | Testability; MCP refresh parity |
| **P0** | Turn epilogue extract | `turn_finalizer.py::finalize_turn` | `turn_epilogue.rs` | Stop mid-loop/end-loop divergence |
| **P1** | Unified `FailoverReason` | `error_classifier.py` | `failover.rs` or extend `provider_call.rs` | Doctor + metrics + loop single brain |
| **P1** | Compression lock | `conversation_compression.py` | `compression.rs` + state DB | Race with background work |
| **P1** | Budget exhaustion summary | `_handle_max_iterations` | `execute_loop` epilogue | Operator gets model summary not just Incomplete |
| **P2** | Session lineage | `parent_session_id` rotate | `edgecrab-state` sessions | Forensics after compress |
| **P2** | Defer preflight compress | `should_defer_preflight_to_real_usage` | `compression.rs` | Less compress churn |
| **P2** | Unanswered tool_call gate | finalizer WARNING on last=tool | `completion_assessor.rs` | "Agent just stopped" fix |
| **P3** | Background review | `background_review.py` | new module | Post-turn memory/skill curation |
| **P3** | Image shrink recovery | `try_shrink_image_parts_in_messages` | provider error path | Anthropic 5MB recovery |

---

## High-ROI exports (Hermes ← EdgeCrab)

| Pattern | EdgeCrab anchor | Why Hermes should adopt |
|---------|-----------------|-------------------------|
| `RecoveryFeedbackBuilder` | `recovery_catalog.rs` | Structured tool errors > prose |
| `RunOutcome` / `CompletionDecision` | `edgecrab-types` | Eliminate string-only exit reasons |
| `HarnessSnapshot` gates | `harness_gates.rs` | Deterministic completion block |
| Port-scoped preview | `url_safety.rs` + `security.preview` | Safer than global private URLs |
| `harness_analyzer` | `harness_analyzer.rs` | Post-mortem without reading 4k-line loop |
| Replay CI | `harness_games003_replay.rs` | Regression on real failure sessions |

---

## Explicit rejects

| Hermes pattern | Reject for EdgeCrab | Reason |
|----------------|---------------------|--------|
| `allow_private_urls` global | Yes | SSRF regression — keep port allowlist |
| `hard_stop_enabled: false` default | Yes | EdgeCrab targets cheap local models; loops are costly |
| Auto-inject full spill post-compress | Yes | Turn budget explosion (both agree) |
| Profile fully isolated security keys | Yes | Caused games003 E15 — merge unset keys from global |
| Python dynamic tool registration | N/A | Rust `inventory::submit!` is fine |
| God-file re-expansion | Yes | Don't inline extracted modules back into conversation.rs |

| EdgeCrab pattern | Reject for Hermes | Reason |
|------------------|-------------------|--------|
| Monolithic 7k-line loop | Yes | Hermes already decomposed |
| Shadow judge veto | Maybe | Adds latency; optional only |

---

## Default divergence ledger (do not "parity" blindly)

| Setting | Hermes | EdgeCrab | Recommendation |
|---------|--------|----------|----------------|
| Guardrail hard stop | OFF | ON | **Keep different** — product positioning |
| Preview URLs | Broad private | Port allowlist | **Keep EdgeCrab stricter** |
| Config file | Single `~/.hermes/config.yaml` | Global + profile YAML | **Merge security keys** on profile load |
| MCP refresh | Between-turn prologue | Manual `/reload-mcp` | **Borrow** auto-refresh |
| Iteration refund (execute_code) | Yes | No | **Evaluate borrow** |

---

## Implementation sequencing

```text
  Phase 1 — Structure (unlock everything else)
  ────────────────────────────────────────────
  turn_prologue.rs + turn_epilogue.rs extract from conversation.rs

  Phase 2 — Single error brain
  ────────────────────────────
  FailoverReason enum + classify_provider_error() consumed by loop + doctor

  Phase 3 — VERIFY hardening
  ──────────────────────────
  Full HarnessSnapshot at LoopAction::Done (no default())
  visual_storm blocks → completion gate coupling

  Phase 4 — Optional Hermes parity
  ────────────────────────────────
  background_review, session lineage, compression lock
```

---

## File index (quick lookup)

### Hermes

| Topic | Path |
|-------|------|
| Main loop | `agent/conversation_loop.py` |
| Prologue | `agent/turn_context.py` |
| Epilogue | `agent/turn_finalizer.py` |
| Tool dispatch | `agent/tool_executor.py` |
| Guardrails | `agent/tool_guardrails.py` |
| Error classifier | `agent/error_classifier.py` |
| Compression | `agent/context_compressor.py`, `agent/conversation_compression.py` |
| Spill | `tools/tool_result_storage.py`, `tools/budget_config.py` |
| Background review | `agent/background_review.py` |
| Entry | `run_agent.py` |

### EdgeCrab

| Topic | Path |
|-------|------|
| Main loop | `crates/edgecrab-core/src/conversation.rs` |
| Agent facade | `crates/edgecrab-core/src/agent.rs` |
| Dispatch | `crates/edgecrab-core/src/turn_dispatch.rs` |
| Completion UX | `crates/edgecrab-core/src/turn_completion.rs` |
| Completion assess | `crates/edgecrab-core/src/completion_assessor.rs` |
| Advisories | `crates/edgecrab-core/src/harness_advisory.rs` |
| Guardrail policy | `crates/edgecrab-core/src/harness_loop_policy.rs` |
| Doctor | `crates/edgecrab-core/src/harness_analyzer.rs` |
| Compression | `crates/edgecrab-core/src/compression.rs` |
| Provider retry | `crates/edgecrab-core/src/provider_call.rs` |
| Spill | `crates/edgecrab-tools/src/artifact_spill.rs` |
| Guardrails | `crates/edgecrab-tools/src/tool_loop_guardrails.rs` |
| Recovery | `crates/edgecrab-tools/src/recovery_catalog.rs` |
| Gates | `crates/edgecrab-tools/src/harness_gates.rs` |

---

## Closing verdict

Hermes and EdgeCrab are **siblings**, not duplicates. The harness comparison is not "who is better" but **which mechanisms belong where**:

- **Hermes** optimizes **loop maintainability** (module split), **operator UX paths** (summary on budget hit, background review), and **provider error battle scars** (classifier).
- **EdgeCrab** optimizes **typed completion truth**, **security defaults**, **task-class advisories**, and **post-mortem tooling**.

The Jun 2026 gap for **both** is the same: **VERIFY as loop physics**, not markdown theater.
