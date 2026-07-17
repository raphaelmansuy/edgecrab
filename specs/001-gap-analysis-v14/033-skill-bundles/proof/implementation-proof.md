# 033 Skill Bundles — Implementation Proof

Shipped:

- `crates/edgecrab-tools/src/skills/bundles.rs` — `get_skill_bundles`, create/delete/list
- Slash: `/bundles`, `/bundles create|delete|list`, and `/<bundle-name>` invocation via `invocation.rs`
- CLI completes bundle names for tab completion (`app.rs`)

Acceptance: named groups of skills load in one command without duplicating skill bodies.
