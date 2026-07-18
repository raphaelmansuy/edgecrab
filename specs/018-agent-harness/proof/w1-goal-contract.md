# Proof: W1 GoalContract + unified VerificationSummary

**Date:** 2026-07-18  
**Honesty note:** Initial W1 claim of “unified ledger wired end-to-end” was
**partial** until close-lies P0 — see [p0-one-assess-contract.md](./p0-one-assess-contract.md).

## Law

One evidence aggregate (`VerificationSummary`) + optional `GoalContract` on
`session_goals.contract_json`. No parallel HermesEvidence type. One assess
entry: `turn_epilogue::assess_turn_outcome` (now calls
`enrich_verification_with_contract`).

## Anchors

- `edgecrab_types::GoalContract` / `parse_goal_with_contract`
- `session_db` schema v13 `contract_json`
- `goal_judge::contract_evidence_in_messages` (structured terminal `exit_code==0` + needle; not prose / not `report_task_status` alone — see [p1-structured-terminal-evidence.md](./p1-structured-terminal-evidence.md))
- `LifecycleEvent::PreVerify` emit before assess (ceremony only — assess enrich is the gate)

## Verify

```bash
cargo test -p edgecrab-types --lib harness
cargo test -p edgecrab-core --lib goals::
cargo test -p edgecrab-core --lib goal_judge
cargo test -p edgecrab-core --lib p0_contract
cargo test -p edgecrab-state --lib session_db
```
