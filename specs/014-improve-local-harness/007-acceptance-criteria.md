# 007 — Acceptance Criteria (Verified Plan Only)

Cross-ref: [006-solution-plan.md](./006-solution-plan.md) · [005-code-anchors.md](./005-code-anchors.md)

**Scope:** Tests that **gate the verified plan** (P0 + P1a + P1b + P2). Items marked **BACKLOG** are not merge requirements until promoted in [006](./006-solution-plan.md).

**Principle:** No manual homelab scenario, model-behavior assertion, or failure-count check is a CI gate.

---

## Verified plan matrix

```text
  PHASE   LH IDs              Test command
  ─────   ──────              ────────────
  P0      LH-01 .. LH-05      cargo test -p edgecrab-core -p edgecrab-tools (unit)
          LH-10               edgequake-llm unit (serialization)
  P1a     LH-06 .. LH-09      cargo test -p edgecrab-core --test local_prefill_prune_e2e
          LH-30 .. LH-31      lh30 lh31
  P1b     LH-11               lh11
  P2      LH-20               cargo test -p edgecrab-tools lh20_length
  P3      LH-32 .. LH-33      cargo test -p edgecrab-core --test local_prefill_prune_e2e lh3
  P4      LH-40               cargo test -p edgecrab-tools lh40
  P5      LH-50 .. LH-51      cargo test -p edgecrab-tools lh5
  P6-P8   LH-60 .. LH-64      cargo test -p edgecrab-core --test local_harness_geometry_e2e
                               cargo test -p edgecrab-tools --lib lh64 all_exported_tool
  P9      LH-63               cargo test -p edgecrab-core --test local_prefill_prune_e2e lh63
  BACKLOG B6                  optional AGENTS.md link
```

---

## P0 — Local completion policy (CI required)

| ID | Invariant | Test |
|----|-----------|------|
| **LH-01** | Local tool turns force `reasoning_effort: none` | `local_provider_policy::caps_local_tool_turn_max_tokens_and_forces_reasoning_none` |
| **LH-02** | `tool_choice: required` when tools + local provider | `local_provider_policy::local_tool_choice_required_for_local_tool_turns` |
| **LH-03** | No transport retry on local Timeout/NetworkError | `local_provider_policy::blocks_transport_retry_only_for_local_timeout_and_network` |
| **LH-04** | API `max_tokens` = `output_token_budget_for_tool_turn` | `mutation_turn_policy` + `local_provider_policy` DRY tests |
| **LH-05** | Max arg bytes = **27852** @ default (`8192×4×0.85`) | `mutation_turn_policy` unit (`local_default_max_tool_argument_bytes`) |
| **LH-10** | Non-streaming tool request serializes `reasoning_effort` | `edgequake-llm` `test_chat_request_with_tools_reasoning_effort_serialized` |

### P0 optional live (ignored — not plan gate)

```bash
cd ../edgequake-llm
cargo test -p edgequake-llm --test e2e_lmstudio_qwen -- --ignored --nocapture
```

| Observation | Expected (homelab/nightly) |
|-------------|----------------------------|
| `reasoning_effort=none` tool turn | `thinking_tokens=0` |
| `tool_choice=required` | Valid `tool_calls` or `finish_reason=tool_calls` |

Failure of live e2e does **not** revert verified plan — fix provider/model stack separately.

---

## P1a — Preflight structural prune (CI required)

| ID | Invariant | Test |
|----|-----------|------|
| **LH-06** | Budget @ 262k = `min(32_000, 262_144/8)` = **32_000** | `local_provider_policy::prefill_prune_budget_is_min_of_cap_and_context_eighth` |
| **LH-07** | 37k and 46k trigger prune; 30k does not (@ 262k) | `local_provider_policy::should_prefill_prune_when_prompt_exceeds_budget` |
| **LH-08** | Prune drops tool-output tokens >4× | `compression::structural_prefill_prune_reclaims_tool_output_tokens` |
| **LH-09** | Research fixture: above prefill budget, below 131k compress | `tests/local_prefill_prune_e2e.rs` (3 tests) |
| **LH-09b** | Shelf formatter | `tool_progress_tail::local_prefill_prune_notice_mentions_token_drop` |
| **LH-30** | Mid-band (34–40k) fixture triggers preflight @ 32k budget | `local_prefill_prune_e2e::lh30_mid_band_triggers_preflight_prune` |
| **LH-31** | Post-preflight prune estimate ≤ budget for mid-band fixture | `local_prefill_prune_e2e::lh31_mid_band_preflight_prune_drops_below_budget` |

### P1a CI block

```bash
cargo test -p edgecrab-core --test local_prefill_prune_e2e
cargo test -p edgecrab-core prefill structural_prefill
cargo test -p edgecrab-tools local_prefill
cargo clippy -p edgecrab-core -p edgecrab-tools -- -D warnings
```

---

## P1b — Length-recovery prune ✅ CI required

| ID | Invariant | Test |
|----|-----------|------|
| **LH-11** | Length-recovery prune drops tokens and clears long tool outputs (even with recovery msgs) | `local_prefill_prune_e2e::lh11_length_recovery_prune_drops_tokens_in_mid_band` |
| **LH-11b** | `gate_local_structural_prune(LengthRecovery)` always true below preflight budget | `local_provider_policy::length_recovery_gate_always_attempts_prune` |
| **LH-11c** | `try_apply_structural_tool_output_prune` reclaims fat outputs when preflight would skip | `local_provider_policy::try_apply_length_recovery_prune_reclaims_fat_tool_outputs` |

### P1b CI block

Same as P1a:

```bash
cargo test -p edgecrab-core --test local_prefill_prune_e2e
```

---

## P2 — Length-specific recovery message ✅ CI required

| ID | Invariant | Test |
|----|-----------|------|
| **LH-20** | Recovery cites exact `max_tool_argument_bytes()` and `finish_reason=length` (not stream-interrupt text) | `mutation_turn_policy::lh20_length_without_tools_recovery_message_cites_exact_max_bytes` |

```bash
cargo test -p edgecrab-tools lh20_length
```

---

## P3 — Mid-band structural compress ✅ CI required

| ID | Invariant | Test |
|----|-----------|------|
| **LH-32** | ~57k fixture triggers local structural compress, not LLM compress @ 50% | `local_prefill_prune_e2e::lh32_high_band_triggers_local_structural_compress_not_llm` |
| **LH-33** | Structural compress shrinks high-band fixture >2× | `local_prefill_prune_e2e::lh33_local_structural_compress_reduces_high_band_tokens` |
| **LH-33b** | Gate formula mid-band only | `local_provider_policy::should_local_structural_compress_mid_band_only` |
| **LH-50** | Shelf compress notice shows token drop | `tool_progress_tail::lh50_local_structural_compress_notice_mentions_token_drop` |
| **LH-63** | Homelab ~57k exceeds 20% threshold (was gap @ 0.22) | `local_prefill_prune_e2e::lh63_homelab_57k_band_triggers_structural_compress_at_20pct` |

---

## P6–P8 — Output geometry + schema (CI required)

| ID | Invariant | Test |
|----|-----------|------|
| **LH-60** | Config `max_tool_turn_tokens` resolves; default 8192 → 27852 B | `local_harness_geometry_e2e::lh60_e2e_*`, `local_provider_policy::lh60_*` |
| **LH-61** | Local turn annotates mutation tool defs with live budget | `local_harness_geometry_e2e::lh61_e2e_*`, `registry::lh61_*` |
| **LH-62** | Patch flat object + mode-aware `required_fields` on InvalidArgs | `local_harness_geometry_e2e::lh62_e2e_patch_flat_schema_*`, `file_patch::lh62_*`, `registry::lh62_*` |
| **LH-64** | Every exported tool has top-level `type: "object"` (no `oneOf` on wire) | `registry::all_exported_tool_definitions_have_provider_safe_top_level_schemas`, `registry::lh64_*`, `registry::openai_compatible_tool_parameters_*` |

```bash
cargo test -p edgecrab-core --test local_harness_geometry_e2e
cargo test -p edgecrab-tools --lib all_exported_tool_definitions lh64 openai_compatible
```

---

## P4 — local_write_create_dirs ✅ CI required

| ID | Invariant | Test |
|----|-----------|------|
| **LH-40** | Nested path succeeds when `local_write_create_dirs=true` and flag omitted | `file_write::lh40_local_write_create_dirs_default_writes_nested_path` |

Config key: `local_inference.write_create_dirs: true` in `~/.edgecrab/config.yaml`.

---

## P5 — Shelf max_arg_bytes ✅ CI required

| ID | Invariant | Test |
|----|-----------|------|
| **LH-51** | Plan log line includes `max_arg={N}B` | `local_provider_policy::local_tool_turn_plan_includes_max_arg_bytes` |
| **LH-51b** | Preflight shelf passes max_arg through | `tool_progress_tail::lh51_local_tool_turn_preflight_passes_through_max_arg_plan_line` |

---

## Frozen backlog gates

| ID | Criterion | Status |
|----|-----------|--------|
| **B6** | AGENTS.md link to spec 014 | optional docs |

**Removed (heuristic / flaky):**

- ~~LH-21~~ — “≤100 tokens without prune” — replaced by **LH-11** token delta on fixture  
- ~~LH-23~~ — “model guided within 2 turns” — model behavior, rejected  
- ~~Section F homelab PPT pass~~ — manual; moved to [004-homelab-evidence.md](./004-homelab-evidence.md) observability  

---

## Negative invariant (P1b / LH-11 target)

When LH-11 lands, it should encode:

```text
  Given: fixture with 8× fat tool results + recovery user messages
  When:  structural_prefill_prune applied (length_recovery path)
  Then:  count_long_tool_outputs == 0
         estimate_tokens(after) < estimate_tokens(before)
```

This replaces manual log checks (“+87 three times without prune log”).

---

## Traceability (verified plan only)

```text
  SYMPTOM                    VERIFIED BY          NOT CLAIMED YET
  ───────                    ───────────          ───────────────
  reasoning ate max_tokens   LH-01, LH-10         —
  length @ 37k, think=0      LH-05, LH-11, LH-20   —
  slow prefill @ 34–46k       LH-09, LH-30, LH-31   —
  composing 30–187s          S14 docs             not a test gate
  tmp write fail             —                    B4 only
```

---

## Documentation checklist (non-blocking)

- [x] Spec 014 tree
- [x] S14 in stuck playbook
- [ ] AGENTS.md link (optional backlog B6)
