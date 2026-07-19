# 001 — First Principles + July 2026 Agent Engineering

**Lens:** Ontology · jobs-to-be-done · SOTA harness principles  
**Authority:** [000-code-is-law.md](000-code-is-law.md)  
**Date:** 2026-07-19 (re-assessed)

---

## 1. Strip the brands

Both systems implement the same abstract machine:

```text
  intent → assemble context → model step → (tool* | final)
                ↑__________________|
         budgeted, recoverable, human-interruptible
```

Language (Rust vs Python) and UI (ratatui vs Ink/desktop) are **implementation choices**, not first principles.

---

## 2. Jobs-to-be-done (J1–J10)

| ID | Job | Failure if missing |
|----|-----|--------------------|
| J1 | Understand intent | Wrong tools, thrash |
| J2 | Act with tools safely | RCE, SSRF, data loss |
| J3 | Observe results honestly | False done, hallucinated success |
| J4 | Stay within budgets | Infinite loop, $ blow-up |
| J5 | Recover from API/tool failure | Stuck turns |
| J6 | Compress without amnesia | Overflow or lost goals |
| J7 | Persist across sessions/windows | Lost mission |
| J8 | Accept human mid-course correction | Unsteerable train |
| J9 | Deliver on the right surface | CLI/gateway silo |
| J10 | Extend without forking core | Plugin dumping ground |

### Code ownership (law)

| Job | EdgeCrab | Hermes |
|-----|----------|--------|
| J1 | `prompt_builder.rs`, skills, memory | `prompt_builder.py`, `system_prompt.py` |
| J2 | `edgecrab-security` + tool handlers | path/url safety, approval, tirith |
| J3 | `completion_assessor`, `contract_verify`, spill, document latch | `verification_stop`, finalizer strings |
| J4 | hard-stop ON, iteration budget, cost guard | IterationBudget, soft guardrails |
| J5 | `failover.rs`, `provider_call.rs` | `error_classifier.py`, loop guidance |
| J6 | `compression.rs` (LLM+structural, todo snapshot, defer preflight) | `context_compressor.py`, `conversation_compression.py` |
| J7 | goals SQLite, `parent_session_id`, memory | parent_session_id, curator, memory plugins |
| J8 | steering HINT/REDIRECT/STOP, grants, clarify | interrupt, approvals, clarify |
| J9 | cli + gateway + SDKs | cli + gateway + desktop + web + Ink |
| J10 | inventory tools, MCP OAuth, plugins, SDKs | Python plugins, MCP OAuth manager |

---

## 3. July 2026 SOTA principles (AE1–AE10)

Derived from production coding-agent practice and Anthropic’s long-running harness research (multi-window progress, evidence-based completion, clean session state). Full mapping: [000 §0–10](000-code-is-law.md).

| AE | Principle | EC (law) | H (law) | Score |
|----|-----------|----------|---------|-------|
| AE1 | Bounded autonomy | hard-stop **ON** | hard-stop **OFF** default | **EC** |
| AE2 | Cross-window progress | goals+contracts+parent_session+learning reflection | parent_session+curator+bg review+learning graph | **≠ both strong** |
| AE3 | Completion = evidence | RunOutcome, verify_on_stop true, shadow judge, doc latch | string exit + nudge | **EC** |
| AE4 | Tool truth / spill | artifact_spill 913 LOC + spill-blind write block | tool_result_storage + concurrent exec | **EC slight** |
| AE5 | Cache-stable prompts | prompt_cache_policy + stable/dynamic law | prompt_caching system_and_3 | **=** |
| AE6 | Classify → recover | failover 21 reasons | classifier 23 + credential_pool | **H slight** |
| AE7 | Mediated side effects | security crate + preview grants | distributed + global private URL footgun | **EC** |
| AE8 | Human sovereignty | typed steer + grants | interrupt + approvals + apps | **=** |
| AE9 | Observability | harness_analyzer + OTEL | observability plugins + trajectory | **≠** |
| AE10 | Extend without bloat | SDK embed lead; MCP OAuth present | plugin gravity lead | **≠** |

---

## 4. Non-negotiable invariants (I1–I7)

| ID | Invariant | EC | H |
|----|-----------|----|---|
| I1 | Bounded autonomy | Strong default | Soft default |
| I2 | Mediated side effects | Crate-level | Feature-rich, looser defaults |
| I3 | Observable tool truth | Strong spill+block | Strong spill, less pre-dispatch block |
| I4 | Completion is a claim | Typed+enforced paths | String+nudge |
| I5 | Context is scarce | Strong compressor | Stronger heuristics volume |
| I6 | Human sovereign | Strong | Strong + app UX |
| I7 | Extend without core bloat | SDK path; loop still huge | Plugins; loop better split |

---

## 5. First-principle deltas (strategy, not feature envy)

| # | Delta | SOTA link | Action |
|---|-------|-----------|--------|
| PΔ1 | Ecosystem gravity (plugins/desktop) | AE10 surface | MCP/skills first; no parity KPI |
| PΔ2 | Loop mass in `conversation.rs` | maintainability | Finish prologue; cap growth |
| PΔ3 | Classifier residual (+2 reasons, pools, copy) | AE6 | Narrow P0 — not greenfield |
| PΔ4 | External E2E VERIFY still soft | AE3 Anthropic | Browser-as-user / contract evidence |
| PΔ5 | Trust defaults diverge | AE1/AE7 | KEEP EC; explicit permissive profile |
| PΔ6 | Embed vs enclose | AE10 product | Double-down SDK |
| PΔ7 | Personal-agent curation depth | AE2 lifestyle | Optional curator depth; reject pet |

---

## 6. What this analysis refuses

- LOC as quality  
- Feature parity as goal  
- Language tribalism  
- Docs/marketing as evidence  
- “Both weak on X” when code shows one has already shipped X  

---

## Next

- Structure: [002](002-systems-architect-lens.md)  
- Harness physics: [003](003-ai-engineer-harness-lens.md)
