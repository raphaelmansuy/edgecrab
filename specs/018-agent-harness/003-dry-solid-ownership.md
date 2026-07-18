# 003 — DRY / SOLID Ownership

## Crate matrix

| Concern | Owner crate | Module |
|---------|-------------|--------|
| `CompletionDecision` / `ExitReason` / `RunOutcome` / `VerificationSummary` / `GoalContract` | `edgecrab-types` | `harness.rs`, goals types |
| Guardrails controller / spill / materialize / recovery / `build_harness_snapshot` | `edgecrab-tools` | `tool_loop_guardrails`, `artifact_spill`, `tool_schema_index`, … |
| Completion policy / turn prologue·epilogue / dispatch policy / goals loop / cache | `edgecrab-core` | `completion_assessor`, `turn_*`, `harness_loop_policy`, `goals/`, `prompt_cache_policy` |
| Goal persistence | `edgecrab-state` | `session_db` (`session_goals.contract_json`) |
| Operator surface | `edgecrab-cli` / gateway | notices, `--json-stream` done |

## Ban list

1. No second assess entry — only `turn_epilogue::assess_turn_outcome`
2. No `HarnessSnapshot::default()` at assess sites
3. No parallel `HermesEvidence` type — extend `VerificationSummary`
4. No new local `Severity` enums when touching skills_guard/plugins — converge
5. Hooks must not assign `session.cached_system_prompt`
6. Hot set size stays ≤ 5 without new meter proof

## SOLID rules

- **S:** `conversation.rs` orchestrates; policy/dispatch live in named modules
- **O:** new task classes extend `TaskClass` + `collect_verification_summary`
- **L:** `CompletionPolicy` impls honor `should_reopen_loop`
- **I:** hooks get `LifecycleEvent` + JSON only
- **D:** loop depends on policy + snapshot builders, not storm/theater details
