# Proof: P1 structured terminal evidence

**Date:** 2026-07-18  
**Status:** landed (structured evidence cliff)

## Law

1. `terminal` / `run_process` contract proof requires
   `[terminal_result … exit_code=0]` **and** verification needle in the body.
2. Failed terminals (`exit_code ≠ 0`) never satisfy contracts and never populate
   free-form `VerificationSummary.evidence`.
3. `report_task_status` alone cannot satisfy a GoalContract (model-authored).
4. Headerless terminal blobs are not evidence.

## Anchors

| Piece | Location |
|-------|----------|
| Parser | `edgecrab_tools::parse_terminal_result` |
| Contract gate | `goal_judge::contract_evidence_in_messages` |
| Free-form assess | `completion_assessor::collect_verification_summary` |
| Ralph veto | `goals::loop_manager::evaluate_goal_after_turn` |

## Prior interim (superseded)

P0 close-lies used tool-role substring match. That was honesty plumbing;
structured `exit_code` is the integrity law after this pass.

## Verify

```bash
cargo test -p edgecrab-tools --lib terminal_result
cargo test -p edgecrab-core --lib contract_evidence
cargo test -p edgecrab-core --lib failed_terminal_does_not_count
cargo test -p edgecrab-core --test goals_ralph_loop contract_
```

Test names:

- `terminal_result::tests::parse_success_header` / `parse_error_header`
- `goal_judge::tests::contract_evidence_rejects_failed_terminal_even_with_needle`
- `goal_judge::tests::contract_evidence_rejects_report_task_status_alone`
- `completion_assessor::tests::failed_terminal_does_not_count_as_verification_evidence`
- `goals_ralph_loop::contract_vetoes_judge_done_without_tool_evidence`
- `goals_ralph_loop::contract_allows_done_with_successful_terminal_evidence`
