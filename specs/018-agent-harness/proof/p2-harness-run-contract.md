# Proof: P2 harness-run contract verification

**Date:** 2026-07-18  
**Status:** landed (July 2026 integrity wave A)

## Law

1. On **final** assess only, if `GoalContract.verification` is unmet by agent tools,
   the harness runs the verification command once (`contract_verify::run_contract_verification`).
2. `contract_satisfied` iff harness `exit_code==0` (or prior non-echo agent terminal proof).
3. Echo/printf gaming of the needle is rejected.
4. Provisional mid-loop assess never runs harness verify (`harness_contract_verify: false`).

## Anchors

| Piece | Location |
|-------|----------|
| Runner | `edgecrab_core::contract_verify` |
| Echo reject | `looks_like_echo_gaming` in contract evidence |
| Final assess | `conversation.rs` `harness_contract_verify: true` |
| Assess fold | `turn_epilogue::assess_turn_outcome` |

## Verify

```bash
cargo test -p edgecrab-core --lib contract_verify
cargo test -p edgecrab-core --lib wave_a_
cargo test -p edgecrab-core --lib wave_b_
```

Test names:

- `contract_verify::tests::echo_gaming_detected`
- `contract_verify::tests::harness_true_exits_zero`
- `turn_epilogue::tests::wave_a_echo_gaming_rejected_even_with_exit_zero`
- `turn_epilogue::tests::wave_a_harness_run_satisfies_contract`
- `turn_epilogue::tests::wave_b_verify_on_stop_blocks_mutation_only_completed`
