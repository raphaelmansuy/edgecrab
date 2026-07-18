# Proof: P0 one assess path + tool-result contract evidence

**Date:** 2026-07-18  
**Status:** landed (close-lies); evidence quality upgraded in
[p1-structured-terminal-evidence.md](./p1-structured-terminal-evidence.md)

## Law

1. `Completed` only via `assess_turn_outcome` → `CompletionPolicy`.
2. Active `GoalContract.verification` is folded into `VerificationSummary` by
   `enrich_verification_with_contract` inside assess.
3. Evidence is **structured tool results** (terminal `exit_code==0` + needle),
   not assistant prose and not failed runs.

## Anchors

| Piece | Location |
|-------|----------|
| Assess params | `TurnAssessParams.goal_contract` |
| Enrich + force `NeedsVerification` | `turn_epilogue::assess_turn_outcome` |
| Tool-result scan | `goal_judge::contract_evidence_in_messages` |
| Ralph reject vibes | `goals::loop_manager::evaluate_goal_after_turn` |
| Call sites | `conversation.rs` provisional + final assess |

## Verify

```bash
cargo test -p edgecrab-core --lib p0_contract
cargo test -p edgecrab-core --lib contract_evidence
cargo test -p edgecrab-core --test goals_ralph_loop contract_
```

Test names:

- `turn_epilogue::tests::p0_contract_blocks_completed_without_tool_evidence`
- `turn_epilogue::tests::p0_contract_satisfied_by_terminal_tool_result`
- `goal_judge::tests::contract_evidence_requires_tool_result_not_prose`
- `goals_ralph_loop::contract_vetoes_judge_done_without_tool_evidence`
- `goals_ralph_loop::contract_allows_done_with_successful_terminal_evidence`
