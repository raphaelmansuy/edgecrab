# 005 — Code Anchors (EdgeCrab + edgequake-llm)

Cross-ref: [006-solution-plan.md](./006-solution-plan.md) · [007-acceptance-criteria.md](./007-acceptance-criteria.md)

Implementation map from first principles to source files.

---

## Policy module (single responsibility)

**File:** `crates/edgecrab-core/src/local_provider_policy.rs`

| Symbol | Role |
|--------|------|
| `is_local_inference_provider` | `lmstudio` \| `ollama` |
| `prefers_nonstreaming_tool_turns` | Atomic tool completions (avoid stream assembly + double GEN) |
| `blocks_transport_retry` | No retry on Timeout/NetworkError (orphan generation) |
| `local_tool_turn_max_tokens` | DRY with mutation policy |
| `effective_completion_options` | Forces `reasoning_effort: none`, caps `max_tokens` |
| `local_tool_choice` | `ToolChoice::required` when tools present |
| `local_prefill_prune_token_budget` | `min(32000, ctx/8)` |
| `should_structural_prefill_prune` | Preflight gate |
| `LocalStructuralPrunePhase` | `Preflight` \| `LengthRecovery` |
| `gate_local_structural_prune` | Phase-specific gate (threshold vs always) |
| `try_apply_structural_tool_output_prune` | Gate + apply; returns `StructuralPruneOutcome` |
| `local_structural_compress_token_threshold` | `ctx × 0.20` (P3/P9 mid-band; env override) |
| `should_local_structural_compress` | Between 20% and 50% ctx |
| `try_local_midband_structural_compress` | Applies `compress_structural_only` when gated |
| `log_local_prefill_prune` | Structured INFO (`preflight` \| `length_recovery`) |
| `log_local_tool_length_failure` | Structured WARN for investigation |

---

## Mutation / output geometry (DRY)

**File:** `crates/edgecrab-tools/src/mutation_turn_policy.rs`

```text
  output_token_budget_for_tool_turn()
       │
       └── min(provider_cap, mutation_payload_tokens + envelope, LOCAL_TOOL_TURN_ABS_MAX_TOKENS)
                    default 8192

  max_tool_argument_bytes()
       │
       └── output_budget × TOOL_ARG_CHARS_PER_TOKEN (4) × 0.85  →  ~27852 B @ 8192
```

| Symbol | Role |
|--------|------|
| `check_tool_argument_budget` | Pre-dispatch reject (only if tool_calls parsed) |
| `length_without_tools_recovery_message` | After `finish_reason=length` + no tools (P2) |
| `stream_interrupted_recovery_message` | After stream interrupt / timeout mid-draft |
| `LOCAL_TOOL_TURN_ABS_MAX_TOKENS` | `8192` (env: `EDGECRAB_LOCAL_TOOL_MAX_TOKENS`; yaml: `local_inference.max_tool_turn_tokens`) |
| `annotate_llm_definitions_for_local_turn` | P7 — appends live budget suffix to mutation tools |
| `patch_tool_parameters_json` | P6 — flat `type: "object"` (nullable mode fields; no top-level `oneOf`) |
| `openai_compatible_tool_parameters` | P6/LH-64 — wire-safe export; flattens top-level `oneOf` before API |

**Gap (documented):** guard runs **post-API**, not pre-API — see [002-first-principles-why.md](./002-first-principles-why.md) Layer 3.

---

## ReAct loop wiring

**File:** `crates/edgecrab-core/src/conversation.rs`

| Location | Behavior |
|----------|----------|
| ~1558–1570 | `refresh_model_metadata` for local providers (262k ctx sync) |
| ~1576–1702 | LLM compression at 50% threshold |
| ~1724–1768 | **P3** mid-band structural compress (`22% < prompt < 50%`) |
| ~1770–1820 | **P1a** preflight structural prune before API |
| ~1868–1885 | `effective_completion_options` + `local_tool_turn_plan` shelf (P5 `max_arg`) |
| ~2256–2275 | **P1b/P2** length-recovery prune + length-specific recovery message |
| ~5023–5040 | `check_tool_argument_budget` before tool dispatch |
| ~4164–4210 | Non-streaming wait heartbeat (`LlmWaitProgress`) |
| `spawn_nonstreaming_wait_heartbeat` | 80% HTTP timeout warning |

---

## Compression / prune primitives

**File:** `crates/edgecrab-core/src/compression.rs`

| Symbol | Role |
|--------|------|
| `prune_tool_outputs` | Replace tool results >200 chars with placeholder or spill stub |
| `apply_structural_tool_output_prune` | Idempotent prune entry + `StructuralPruneOutcome` metrics |
| `try_apply_structural_tool_output_prune` | Phase gate + apply (preflight vs length recovery) |
| `LocalStructuralPrunePhase` | Preflight \| LengthRecovery |
| `compress_structural_only` | Prune + stat summary (circuit breaker path) |
| `PRUNED_TOOL_PLACEHOLDER` | Deterministic replacement text |

**File:** `crates/edgecrab-core/src/tool_result_spill.rs` — artifact spill when enabled in config.

---

## Shelf / operator UX

**File:** `crates/edgecrab-tools/src/tool_progress_tail.rs`

| Formatter | When |
|-----------|------|
| `format_local_tool_turn_preflight` | Plan line before API |
| `format_nonstreaming_llm_wait` | Heartbeat during blocked HTTP |
| `format_local_length_without_tools_notice` | After length failure |
| `format_local_prefill_prune_notice` | After structural prune |
| `format_local_transport_stall_notice` | Timeout without retry |

---

## Provider transport (edgequake-llm)

**File:** `../edgequake-llm/src/providers/openai_compatible.rs`

| Fix | Lines (approx.) | WHY |
|-----|-----------------|-----|
| Non-streaming `reasoning_effort` forward | ~918, ~1046 | Was hardcoded `None` on tool path — Qwen3 reasoning burned budget |
| Tests | `test_chat_request_with_tools_reasoning_effort_serialized` | Regression gate |

**File:** `../edgequake-llm/src/providers/lmstudio.rs` — placeholder API key, completion normalization.

**E2E:** `../edgequake-llm/tests/e2e_lmstudio_qwen.rs` — live tool + `reasoning_effort=none`.

---

## File tools (secondary homelab failure)

**File:** `crates/edgecrab-tools/src/tools/file_write.rs`

| Behavior | Default | Homelab impact |
|----------|---------|----------------|
| `create_dirs` | `false` | `tmp/pptx_builder.py` failed without parent dir |
| Path jail | `path_policy` | `mkdir -p` via terminal succeeded separately |

Schema documents `create_dirs` in parameters — model must set `true` for new nested paths.

---

## Tests (deterministic, non-flaky)

| Test file | Covers |
|-----------|--------|
| `crates/edgecrab-core/src/local_provider_policy.rs` (unit) | Budget formula, reasoning cap, tool_choice |
| `crates/edgecrab-core/src/compression.rs` (unit) | `structural_prefill_prune_reclaims_tool_output_tokens` |
| `crates/edgecrab-core/tests/local_prefill_prune_e2e.rs` | Homelab band: >43.7k prune, <131k no compress; **LH-11** mid-band length recovery |
| `crates/edgecrab-tools/src/mutation_turn_policy.rs` (unit) | 6963 B derivation |
| `crates/edgecrab-tools/src/tool_progress_tail.rs` (unit) | Shelf formatters |

---

## Dependency graph (harness hot path)

```text
  execute_loop (conversation.rs)
       │
       ├── local_provider_policy ──► completion_options, tool_choice, prune threshold
       │
       ├── compression ────────────► structural_prefill_prune, compress_structural_only
       │
       ├── api_call_with_retry ────► edgequake-llm openai_compatible (LM Studio HTTP)
       │         │
       │         └── blocks_transport_retry (no duplicate GEN)
       │
       ├── length recovery ────────► prune + stream_interrupted_recovery_message
       │
       └── tool dispatch ────────► mutation_turn_policy::check_tool_argument_budget
```

---

## P1b length-recovery prune

**Code:** `local_provider_policy::try_apply_structural_tool_output_prune` + `conversation.rs::try_local_structural_prune_request`.

**Plan:** ✅ **Verified** — **LH-11** e2e + unit gates ([007-acceptance-criteria.md](./007-acceptance-criteria.md)).

---

## Frozen backlog (not plan)

See [006-solution-plan.md § Frozen backlog](./006-solution-plan.md) — B1..B5 require new LH-xx gates before implementation.

---

## Cross-ref to specs/playbooks

**S14 — Local LM tool compose stall** in [006-stuck-scenarios-playbook.md](../002-terminal-ux-ui/006-stuck-scenarios-playbook.md) — documents visible stall vs failure loop; not a CI gate.
