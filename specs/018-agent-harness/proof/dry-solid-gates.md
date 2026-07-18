# Proof: DRY / SOLID acceptance gates

**Date:** 2026-07-18

## Gates

| Gate | Check |
|------|-------|
| Single assess path | No `HarnessSnapshot::default()` at assess call sites in `conversation.rs` |
| Single pre-dispatch | `conversation.rs` uses `pre_dispatch_decision` only |
| Cache law | Hooks/materialize must not assign `cached_system_prompt` |
| Hot set ≤ 5 | `INDEXED_HOT_TOOLS` length |

## CI

`.github/workflows/harness-benchmark.yml` step `dry-solid harness gates` runs:

```bash
cargo test -p edgecrab-core --lib turn_dispatch_policy
cargo test -p edgecrab-core --test dry_solid_harness_gates
```
