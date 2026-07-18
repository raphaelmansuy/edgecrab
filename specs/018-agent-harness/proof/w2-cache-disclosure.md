# Proof: W2 Cache + indexed disclosure

**Date:** 2026-07-18

## Law

- Hot set = 5; `write_file` hot
- `input_examples` via materialize / `tool_search` message path only
- Never assign `cached_system_prompt` from materialize
- Doctor surfaces prompt-cache SLO + indexed schema note

## Verify

```bash
cargo test -p edgecrab-tools --lib tool_input_examples
cargo test -p edgecrab-core --test dry_solid_harness_gates indexed_hot
cargo test -p edgecrab-core --test dry_solid_harness_gates materialize_path
```
