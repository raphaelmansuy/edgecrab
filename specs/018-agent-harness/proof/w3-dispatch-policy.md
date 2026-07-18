# Proof: W3 turn_dispatch_policy

**Date:** 2026-07-18  
**Honesty note:** First pass was a rename facade. Close-lies **P1** moved the
body of `guardrail_before_dispatch_checked_with_session` into
`turn_dispatch_policy.rs`. `turn_dispatch` keeps thin deprecated re-exports.

## Law

`conversation.rs` routes all pre-dispatch mediation through
`turn_dispatch_policy::pre_dispatch_decision` (storm → port → nav → theater →
spill → guardrail). Implementation ownership is in the policy module.

## Verify

```bash
cargo test -p edgecrab-core --lib turn_dispatch_policy
cargo test -p edgecrab-core --test dry_solid_harness_gates
cargo test -p edgecrab-core --test harness_games003_replay
cargo test -p edgecrab-core --test visual_preview_lifecycle_e2e
```

Gate: `turn_dispatch_policy_owns_body_not_facade` fails if policy becomes a
wrapper again.
