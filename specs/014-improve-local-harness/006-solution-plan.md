# 006 — Solution Plan (Verified Only)

Cross-ref: [007-acceptance-criteria.md](./007-acceptance-criteria.md) · [002-first-principles-why.md](./002-first-principles-why.md) · [005-code-anchors.md](./005-code-anchors.md)

This document is the **authoritative harness plan**. It lists **only** work that is **deterministic** and **verified by automated tests** (unit or deterministic e2e fixtures). Nothing else is “in plan.”

---

## Plan admission rules (non-negotiable)

A change enters **Verified plan** only when **all** of the following hold:

| Rule | Rationale |
|------|-----------|
| **Deterministic trigger** | Fixed formula, env override, or API field — never failure counts, turn counts, or “N retries” |
| **Automated gate** | At least one `cargo test` (unit or `tests/*_e2e.rs`) asserts the behavior; test name cited in this doc |
| **No live-server CI dependency** | LM Studio `#[ignore]` tests are **optional** homelab/nightly only — never merge blockers |
| **No model-behavior bets** | Plan does not require “model will choose patch” — only harness invariants |
| **No session heuristics** | No “if last tool was read_file of .py”, “if research phase”, scaffold detection, etc. |

**Manual homelab checklists** (PPT delivery, GEN counters, log eyeballing) are **observability**, not plan items. They live in [004-homelab-evidence.md](./004-homelab-evidence.md) only.

---

## Verified plan (current)

```text
  ID     LAYER   STATUS    CI GATE
  ────   ─────   ───────   ─────────────────────────────────────────
  P0     L2      SHIPPED   LH-01..05, LH-10 (+ optional ignored live)
  P1a    L1      SHIPPED   LH-06..09, LH-30..31 (prefill @ 32k/ctx÷8)
  P1b    L3      SHIPPED   LH-11 (length_recovery mid-band e2e)
  P2     L2      SHIPPED   LH-20 (length-specific recovery message)
  P3     L1      SHIPPED   LH-32..33 (mid-band structural compress @ 22% ctx)
  P4     tools   SHIPPED   LH-40 (local_write_create_dirs config)
  P5     UX      SHIPPED   LH-50..51 (shelf max_arg_bytes)
  P6     L2      SHIPPED   LH-62 + LH-64 (patch flat object schema; LM Studio GEN=0 fix)
  P7     L2      SHIPPED   LH-61 (live budget on tool defs)
  P8     L2      SHIPPED   LH-60 (max_tool_turn_tokens config @ 8192)
  P9     L1      SHIPPED   LH-63 (mid-band @ 20% — homelab 57k gap)
```

### P0 — Local tool-turn completion policy ✅ VERIFIED

**WHY:** Reasoning and unbounded output compete with tool JSON in one `max_tokens` budget ([003-official-references.md](./003-official-references.md)).

| Change | Gate |
|--------|------|
| `reasoning_effort: none` on local tool turns | **LH-01** |
| `tool_choice: required` when tools present | **LH-02** |
| `max_tokens` = `output_token_budget_for_tool_turn` (DRY) | **LH-04**, **LH-05** |
| No transport retry on local Timeout/NetworkError | **LH-03** |
| Non-streaming tool turns for lmstudio/ollama | **LH-01** + `prefers_nonstreaming_tool_turns` unit tests |
| edgequake-llm forwards `reasoning_effort` on non-streaming tool path | **LH-10** |

**Optional live (not plan gate):** `edgequake-llm/tests/e2e_lmstudio_qwen.rs` — `reasoning_effort=none` + `tool_choice=required`.

**Known limit (documented, not a plan gap):** P0 removes reasoning burn; homelab still hits `length` at 37k with `thinking_tokens=0` → **output geometry** ([004-homelab-evidence.md](./004-homelab-evidence.md)). Addressing that requires backlog items with new e2e gates — not heuristics.

---

### P1a — Structural prefill prune (preflight) ✅ VERIFIED

**WHY:** Prompt 46–57k is above slow prefill, below 131k LLM compress ([002-first-principles-why.md](./002-first-principles-why.md)).

| Change | Gate |
|--------|------|
| `local_prefill_prune_token_budget()` = `min(32_000, ctx/8)` | **LH-06**, **LH-30** |
| `should_structural_prefill_prune()` | **LH-07**, **LH-30** |
| `structural_prefill_prune()` reclaims tool mass | **LH-08** |
| Homelab band fixture: prune fires, compress does not | **LH-09** (`local_prefill_prune_e2e.rs`) |
| Mid-band (34–40k) preflight fires @ 262k ctx | **LH-30**, **LH-31** |
| Preflight wiring in `conversation.rs` before API | Covered by policy + e2e fixture (no live loop test) |
| Shelf `format_local_prefill_prune_notice` | `tool_progress_tail::local_prefill_prune_notice_mentions_token_drop` |

**Formula (deterministic):**

```text
  prefill_prune_threshold = min(32_000, active_context_length / 8)
  override: EDGECRAB_LOCAL_PREFILL_PRUNE_TOKENS
```

**Mid-band coverage:** At **34–40k** prompt @ 262k ctx, threshold **32_000** → preflight **fires** (**LH-30**, **LH-31**).

---

### P1b — Length-recovery structural prune ✅ VERIFIED

**WHY:** Homelab length failures at **34–37k** sit below preflight budget (43.7k) but still carry fat tool outputs; recovery messages add ~87 tokens/round without shrinking context ([004-homelab-evidence.md](./004-homelab-evidence.md)).

| Change | Gate |
|--------|------|
| `LocalStructuralPrunePhase::LengthRecovery` always attempts prune | **LH-11** |
| `try_apply_structural_tool_output_prune` (gate + apply, DRY) | **LH-11** + unit tests |
| `conversation.rs` uses shared `try_local_structural_prune_request` | **LH-11** |
| Prune before recovery message on `finish_reason=length` + no `tool_calls` | **LH-11** |

**Known limit:** Prune reclaims tool mass; output geometry (6963 B arg cap) still requires incremental mutation — not a heuristic fix.

---

### P2 — Length-specific recovery message ✅ VERIFIED

**WHY:** `finish_reason=length` without tools is an output-budget failure, not a stream interrupt — recovery text must cite exact byte limits (**B2**).

| Change | Gate |
|--------|------|
| `length_without_tools_recovery_message()` (DRY via `tool_turn_budget_hint`) | **LH-20** |
| Wired on local `length` + no `tool_calls` path in `conversation.rs` | **LH-20** |
| Stream-interrupt path keeps `stream_interrupted_recovery_message()` | existing tests |

---

### P3 — Local mid-band structural compress ✅ VERIFIED

**WHY:** ~57k prompts sit above prefill (32k) but below LLM compress (131k @ 262k) — tool prune alone leaves slow prefill mass (**B3**).

| Change | Gate |
|--------|------|
| `LOCAL_STRUCTURAL_COMPRESS_THRESHOLD_RATIO = 0.20` | **LH-32**, **LH-63** |
| `try_local_midband_structural_compress` → `compress_structural_only` | **LH-33** |
| Wired in `conversation.rs` before preflight prune | **LH-33** |
| Shelf `format_local_structural_compress_notice` | **LH-50** |

**Formula:** `estimated > ctx × 0.20` **and** `estimated < ctx × 0.50` (local tool turns only).

**Why 0.20 (not 0.22):** homelab agent.jsonl reports ~56–57k @ 262k ctx; at 0.22 threshold = 57 671 — sessions at 57 000 never compress.

Override: `EDGECRAB_LOCAL_STRUCTURAL_COMPRESS_RATIO`.

---

### P6 — Patch flat `object` schema (LM Studio GEN=0) ✅ VERIFIED

**WHY (two constraints):**

1. `mode:replace` and `mode:patch` require different fields — flat `required` caused InvalidArgs loops on homelab outline fix.
2. LM Studio / OpenAI-compatible validators require top-level `type: "object"`. A top-level `oneOf` is rejected **before inference** (`invalid_union_discriminator` → **GEN stays 0** while EdgeCrab waits on HTTP).

**First-principles fix:** flat nullable object on the wire; mode-specific requirements enforced in Rust only.

| Change | Gate |
|--------|------|
| `patch_tool_parameters_json()` — `type: "object"`, nullable mode fields | **LH-62** |
| `required_fields_from_parameters(..., args_json)` mode-aware | **LH-62** |
| `openai_compatible_tool_parameters()` flattens stray top-level `oneOf` | **LH-64** |
| `all_exported_tool_definitions_have_provider_safe_top_level_schemas` | **LH-64** |

---

### P7 — Live byte budget on mutation tool defs ✅ VERIFIED

**WHY:** Model sees exact arg cap at API time, not stale prompt text.

| Change | Gate |
|--------|------|
| `annotate_llm_definitions_for_local_turn` | **LH-61** |
| Wired in `conversation.rs` before local API | **LH-61** (e2e) |

---

### P8 — `local_inference.max_tool_turn_tokens` ✅ VERIFIED

**WHY:** 2048 completion cap → ~6963 B arg budget; homelab PPT scaffolds hit `finish_reason=length`.

| Change | Gate |
|--------|------|
| Default **8192** → ~**27852 B** arg cap | **LH-60** |
| `local_tool_turn_absolute_max_tokens` env > yaml > default | **LH-60** |
| `local_inference.max_tool_turn_tokens` in config | **LH-60** |

---

### P9 — Mid-band threshold lowered to 20% ✅ VERIFIED

**WHY:** Deterministic gap at homelab 57k with 0.22 ratio (57 671 threshold).

| Change | Gate |
|--------|------|
| `LOCAL_STRUCTURAL_COMPRESS_THRESHOLD_RATIO = 0.20` | **LH-63** |
| `local_structural_compress_threshold_ratio()` env override | **LH-63** |

---

### P4 — `local_inference.write_create_dirs` ✅ VERIFIED

**WHY:** Homelab `tmp/pptx_builder.py` failed when model omitted `create_dirs` (**B4**).

| Change | Gate |
|--------|------|
| Config `local_inference.write_create_dirs` → `AppConfigRef.local_write_create_dirs` | **LH-40** |
| `write_file`: `create_dirs \|\| config.local_write_create_dirs` | **LH-40** |

Homelab profile example:

```yaml
local_inference:
  write_create_dirs: true
```

---

### P5 — Shelf shows `max_arg_bytes` ✅ VERIFIED

**WHY:** Operator visibility of output geometry cap without reading logs (**B5**).

| Change | Gate |
|--------|------|
| `LocalToolTurnPlan.max_tool_argument_bytes` in `log_line()` | **LH-51** |
| Preflight shelf passes plan line through | **LH-51** |

---

## Verified plan diagram

```text
  LOCAL TOOL TURN (verified harness path)
  ═══════════════════════════════════════

  [compress @ 50% ctx]          only if estimated > 131k @ 262k ctx
         │
         ▼
  [P3 mid-band compress?]         IF 20% < prompt < 50% ctx         ← LH-32..33, LH-63
         │
         ▼
  [P1a preflight prune?]        IF prompt > min(32k, ctx/8)     ← LH-06..09, LH-30..31
         │                        prune_tool_outputs + spill
         ▼
  [P0 completion policy]        reasoning=none, max_tokens=8192,
         │                        tool_choice=required            ← LH-01..05, LH-60
         ▼
  POST /v1/chat/completions (non-streaming, no retry)           ← LH-03
         │
         ├── tool_calls OK ──► check_tool_argument_budget        ← LH-05 (dispatch)
         │
         └── length + no tools
                  │
                  ├── [P1b prune]  LH-11 mid-band reclaim     ← SHIPPED
                  └── recovery msg P2 length_without_tools     ← LH-20
```

---

## Frozen backlog (not plan until e2e spec + green tests)

Items below are **ideas only**. None may be implemented or marketed as “fixed” until they graduate via **Plan admission rules** and a new **LH-xx** gate.

| ID | Proposal | Blocker | Required e2e (sketch) |
|----|----------|---------|------------------------|
| ~~**B1**~~ | ~~Lower prefill threshold~~ | — | **SHIPPED** → P1a LH-30/31 |
| ~~**B2**~~ | ~~Dedicated length recovery message~~ | — | **SHIPPED** → P2 LH-20 |
| ~~**B3**~~ | ~~Local mid-band structural compress~~ | — | **SHIPPED** → P3 LH-32/33 |
| ~~**B4**~~ | ~~Config local_write_create_dirs~~ | — | **SHIPPED** → P4 LH-40 |
| ~~**B5**~~ | ~~Shelf max_arg_bytes~~ | — | **SHIPPED** → P5 LH-50/51 |

**Operator-only (not backlog, not plan):** `EDGECRAB_LOCAL_PREFILL_PRUNE_TOKENS` — override homelab threshold without changing verified defaults.

---

## Explicitly rejected (never plan)

These violate admission rules or official transport semantics. Do not re-propose without a new first-principles doc + e2e.

| Proposal | WHY rejected |
|----------|--------------|
| Failure-count adaptive `max_tokens` | Non-deterministic; flaky across models |
| Auto `/compress` every N length failures | Side-effect heuristic |
| Transport retry on local timeout | Duplicate LM Studio GEN ([P0 LH-03](../../crates/edgecrab-core/src/local_provider_policy.rs)) |
| Unbounded `max_tokens` increase | Longer stalls; does not fit 15kB arg in one completion ([002 § Layer 2](./002-first-principles-why.md)) |
| Default streaming tool turns on LM Studio | Chunk assembly failures ([LM Studio tools](https://lmstudio.ai/docs/developer/openai-compat/tools)) |
| **Scaffold / research-phase detection** (e.g. “if read_file .py then steer execute_code”) | Session heuristic; model-dependent |
| **One-shot session hints** tied to failure count or path patterns | Heuristic; not reproducible in CI |
| **“Model guided to patch within 2 turns”** (old LH-23) | Model behavior — not harness invariant |
| Homelab PPT delivery as merge gate | Manual; flaky; observability only |

---

## Implementation order (verified work only)

```text
  DONE     P0  ── cargo test LH-01..05, LH-10
  DONE     P1a ── cargo test -p edgecrab-core --test local_prefill_prune_e2e (LH-06..09)
  DONE     P1b ── LH-11 in local_prefill_prune_e2e.rs
  DONE     P2  ── LH-20 length_without_tools_recovery_message
  DONE     P3  ── LH-32/33 mid-band structural compress
  DONE     P4  ── LH-40 local_write_create_dirs
  DONE     P5  ── LH-50/51 shelf max_arg_bytes
  DONE     P6  ── LH-62 patch flat object + LH-64 provider-safe gate
  DONE     P7  ── LH-61 annotate tool defs
  DONE     P8  ── LH-60 max_tool_turn_tokens @ 8192
  DONE     P9  ── LH-63 mid-band @ 20%
  FROZEN   B6 only (optional AGENTS.md link)
```

**CI command (verified plan regression):**

```bash
cargo test -p edgecrab-core --test local_prefill_prune_e2e
cargo test -p edgecrab-core --test local_harness_geometry_e2e
cargo test -p edgecrab-core prefill structural_prefill lh60 lh61 lh62 lh63
cargo test -p edgecrab-tools mutation_turn lh20_length local_prefill
cargo clippy -p edgecrab-core -p edgecrab-tools -- -D warnings
```

---

## Success criteria (verified plan scope only)

| Criterion | Verified by |
|-----------|-------------|
| Local tool turns always use deterministic completion policy | LH-01..05 |
| Prefill prune formula stable @ 262k ctx | LH-06, LH-07 |
| Structural prune reclaims tool output tokens | LH-08, LH-09 |
| No duplicate GEN on timeout | LH-03 |
| Mid-band preflight (34–40k) | **LH-30**, **LH-31** |
| Length-recovery loop broken | **LH-11** |
| Length recovery message cites max bytes | **LH-20** |
| High-band structural compress (57k+) | **LH-32**, **LH-33** |
| Nested write_file with config create_dirs | **LH-40** |
| Preflight shelf shows max_arg_bytes | **LH-51** |

Homelab outcomes (PPT delivered, ≤1 length failure, token spend) remain **evidence** in [004-homelab-evidence.md](./004-homelab-evidence.md) — not plan pass/fail.

---

## Documentation shipped (non-code)

| Item | Status |
|------|--------|
| Spec 014 (this tree) | Shipped |
| Stuck playbook **S14** | Shipped ([006-stuck-scenarios-playbook.md](../002-terminal-ux-ui/006-stuck-scenarios-playbook.md)) |
| AGENTS.md link | Backlog **B6** (docs-only; optional) |
