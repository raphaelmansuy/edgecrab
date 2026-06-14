# 014 — Improve Local Inference Harness (LM Studio / Ollama)

**Status:** Active · **Profile under study:** `~/.edgecrab/profiles/homelab`  
**Model:** `lmstudio/qwen/qwen3.6-35b-a3b` · **Task archetype:** tool-heavy agent (PPT build)

This spec formalizes **why** EdgeCrab appears “stuck” on local providers, what official APIs guarantee (and do not), and a **first-principles remediation plan** with code anchors and homelab evidence.

---

## Document map (read in order)

| # | Document | Purpose |
|---|----------|---------|
| [002-first-principles-why.md](./002-first-principles-why.md) | Physics of one local tool turn; three failure layers; ASCII causality |
| [003-official-references.md](./003-official-references.md) | LM Studio, OpenAI, Qwen3, vLLM — authoritative external grounding |
| [004-homelab-evidence.md](./004-homelab-evidence.md) | Logs + SQLite sessions; quantitative failure signatures |
| [005-code-anchors.md](./005-code-anchors.md) | EdgeCrab / edgequake-llm implementation cross-ref |
| [006-solution-plan.md](./006-solution-plan.md) | **Verified plan** (P0–P9 complete) |
| [007-acceptance-criteria.md](./007-acceptance-criteria.md) | CI gates LH-01..LH-64 |

**Related specs (cross-ref):**

- [specs/002-terminal-ux-ui/006-stuck-scenarios-playbook.md](../002-terminal-ux-ui/006-stuck-scenarios-playbook.md) — user-visible “stuck” taxonomy (extend with **S14 local tool compose**)
- [specs/improve_plan/01-diagnosis.md](../improve_plan/01-diagnosis.md) — generic agent failure causality
- [specs/improve_plan/10-tool-call-repair.md](../improve_plan/10-tool-call-repair.md) — tool-call repair patterns
- [specs/003-ec-vs-hermes/004-tools-toolsets.md](../003-ec-vs-hermes/004-tools-toolsets.md) — tool surface area vs context cost
- [AGENTS.md](../../AGENTS.md) — compression must not rebuild system prompt mid-turn

---

## Executive summary (one screen)

```text
  USER SEES                         ACTUAL STATE
  ─────────                         ────────────
  "awaiting / negotiating"    →     HTTP blocked on LM Studio chat/completions
  "composing tool call 30–187s" →   Phase A (prefill) + Phase B (generate ≤2048 tok)
  tools finished in 1–4s      →     Harness is NOT stuck in tool dispatch

  ROOT CAUSE STACK (independent constraints)
  ┌──────────────────────────────────────────────────────────────┐
  │ L1 INPUT  — large prompt (34–57k) → slow prefill             │
  │ L2 OUTPUT — tool JSON must fit in max_tokens=8192 (~27852 B) │
  │ L3 CONTROL— recovery adds tokens; preflight now covers 34k+ band │
  └──────────────────────────────────────────────────────────────┘
```

**Homelab proof:** 14× `finish_reason=length` + `tool_calls=[]` on 2026-06-14; failures at **37k** prompt after fixes for reasoning burn — output ceiling is now binding.

**Harness direction:** deterministic policies only; **verified plan** = automated tests in [007-acceptance-criteria.md](./007-acceptance-criteria.md). Unverified work lives in **frozen backlog** ([006 § backlog](./006-solution-plan.md)).

---

## Glossary

| Term | Meaning |
|------|---------|
| **Prefill** | Server processes full prompt into KV cache before generating |
| **Tool turn** | ReAct iteration where `tools` non-empty and model must emit `tool_calls` |
| **Non-streaming tool turn** | Single HTTP response after full generation (EdgeCrab policy for LM Studio) |
| **Output budget** | `max_tokens` cap on completion stream (includes reasoning on some stacks) |
| **Arg budget** | Max JSON argument bytes derivable from output budget (~27852 B local @ 8192 tok) |
| **Structural prefill prune** | `prune_tool_outputs` + spill — no LLM summarization |
| **GEN=0** | LM Studio generation counter stuck at 0 — usually schema rejection (400) before prefill, not slow model |

---

## Environment variables (operator)

| Variable | Default | Effect |
|----------|---------|--------|
| `EDGECRAB_LOCAL_PREFILL_PRUNE_TOKENS` | `min(32000, ctx/8)` | Preflight prune threshold |
| `EDGECRAB_LOCAL_STRUCTURAL_COMPRESS_RATIO` | `0.20` | Mid-band structural compress threshold (ctx × ratio) |
| `local_inference.write_create_dirs` | `true` | Default `write_file` create_dirs when omitted |
| `local_inference.max_tool_turn_tokens` | `8192` | Absolute completion cap (env overrides) |
| `EDGECRAB_LOCAL_TOOL_MAX_TOKENS` | `8192` | Env override for absolute completion cap |
| `LMSTUDIO_TIMEOUT_SECONDS` | `600` | HTTP client timeout (no retry on local) |
| `RUST_LOG=edgecrab::local_llm=info` | — | Structured investigation logs |
