# 001 — Methodology

**Cross-ref:** [002-rubric](./002-first-principles-rubric.md) · [003-forensics](./003-session-forensics.md) · [015/002](../015-improve-harness-and-agent/002-first-principles.md)

---

## Question

Does EdgeCrab's agentic session harness meet **Jun 2026 best practice** for closing **INTENT → PLAN → ACT → VERIFY → DONE**, using Hermes Agent as the primary comparator?

---

## Sources (in priority order)

| Source | Path | What we extracted |
|--------|------|-------------------|
| **Harness JSONL** | `~/.edgecrab/profiles/homelab/logs/harness.jsonl` | Per-tool timelines, `exit_reason`, `decision`, api_call_count |
| **Agent log** | `~/.edgecrab/profiles/homelab/logs/agent.log` | Spill events, recovery steers, provider stalls |
| **Session DB** | `~/.edgecrab/profiles/homelab/state.db` | `sessions` table: title, end_reason, message_count, tool_call_count |
| **Workspace artifacts** | `demo/race_gamey/`, `demo/race_game_z/` | File-system outcome vs claimed verification |
| **EdgeCrab code** | `crates/edgecrab-core/`, `crates/edgecrab-tools/`, `crates/edgecrab-cli/` | Loop owners, completion policy, guardrails |
| **Hermes code** | `hermes-agent/agent/`, `tools/`, `run_agent.py` | Turn prologue/epilogue, compression, guardrails, tool search |
| **Live doctor** | `edgecrab doctor harness` | Operator-facing config + log analysis |
| **Prior specs** | `specs/015-improve-harness-and-agent/` | Regression baselines, acceptance gates HA-01..43 |

---

## Analysis method

```text
  1. FIRST PRINCIPLES — define harness invariants (Q1–Q5, J1–J7)
  2. FORENSICS      — quantify live homelab visual-UX battle tests
  3. CODE AUDIT     — map each invariant to module owner; find gaps
  4. HERMES DIFF    — mechanism-level borrow list (not file-layout cosplay)
  5. VERDICT        — scorecard + unified root-cause chain
  6. BACKLOG        — impact-ranked fixes with acceptance gates
```

**Not used:** synthetic mock-only tests as success evidence; model-quality excuses; "works on Opus" hand-waving.

---

## Session cohort

Homelab profile, model `copilot/claude-haiku-4.5`, repeated visual-UX prompts on `demo/games003`, `demo/race_gamey`, `demo/race_game_z`:

| Session | Title / task | DB end_reason | Harness decision |
|---------|--------------|---------------|------------------|
| `927f4d85` | games003 beautify | `interrupted` | `interrupted` |
| `e22c0a28` | games003 beautify | **`model_returned_final_text`** | **`completed`** |
| `2e720f47` | games003 ultra | `interrupted` | `interrupted` |
| `07ffeba0` | 3D racing HTML | `budget_exhausted` | `budget_exhausted` |
| `0aeef965` | race_gamey 3D | `interrupted` | `interrupted` |
| `d4d6b6b4` | race_game_z (in flight) | — | — |
| `cron-234…` | web research (cron) | `model_returned_final_text` | `completed` |

Visual-UX CLI cohort: **6 sessions, 0 with successful browser perception, 1 marked completed anyway** (`e22c0a28`).

Details: [003-session-forensics](./003-session-forensics.md)

---

## Comparator choice: Hermes Agent

Hermes is the right reference because:

1. EdgeCrab explicitly ports patterns (`tool_loop_guardrails.rs`, `turn_completion.rs`, BM25 `tool_search`).
2. Same battle-test failure class (games003 visual UX, spill, preview).
3. Mature turn prologue/epilogue split (`turn_context.py`, `turn_finalizer.py`).
4. Different security posture on localhost (`allow_private_urls` vs port allowlist) — forces explicit design choice.

Hermes path: `/Users/raphaelmansuy/Github/03-working/hermes-agent`

Full map: [004-hermes-comparator](./004-hermes-comparator.md)

---

## Limitations

| Limitation | Impact |
|------------|--------|
| Single operator profile (`homelab`) | Findings are dogfood-real, not multi-tenant |
| Haiku-class model | Stresses harness; does not excuse missing gates |
| `harness_analyzer` JSONL gap | Doctor under-reports until parser fixed |
| Sessions not flushed mid-flight | DB message_count=0 for some in-flight captures (015 EVIDENCE) |
