# Crate Dependency Graph

Verified against `cargo metadata --no-deps --format-version 1`.

This page answers a practical question: "If I change this crate, which way does the coupling run?"

## Workspace dependency shape

```text
edgecrab-types
  -> edgecrab-security
  -> edgecrab-state
  -> edgecrab-cron

edgecrab-security + edgecrab-state + edgecrab-cron + edgecrab-types
  -> edgecrab-tools

edgecrab-tools + edgecrab-state + edgecrab-security + edgecrab-types
  -> edgecrab-core

edgecrab-core + edgecrab-tools + edgecrab-state + edgecrab-types
  -> edgecrab-acp
  -> edgecrab-gateway

edgecrab-core + edgecrab-tools + edgecrab-state + edgecrab-gateway + edgecrab-acp + edgecrab-migrate
  -> edgecrab-cli

edgecrab-state + edgecrab-types
  -> edgecrab-migrate
```

## Why this matters

The graph is not just trivia. It explains where shared types belong, why some abstractions are trait-based, and why the CLI ends up as the heaviest crate.

- `edgecrab-types` stays reusable because it sits at the bottom.
- `edgecrab-tools` depends on `edgecrab-core` only through traits and callbacks, not direct crate coupling.
- `edgecrab-cli` is intentionally heavy: it is the composition root for most runtime features.
- `edgecrab-acp` and `edgecrab-gateway` are peers that both reuse the same agent runtime.

## Practical consequences

- If a feature belongs in every frontend, it probably belongs in `edgecrab-core` or `edgecrab-tools`.
- If a feature needs platform-specific delivery or chat lifecycle management, it belongs in `edgecrab-gateway`.
- If a type is crossing crate boundaries repeatedly, move it to `edgecrab-types` instead of duplicating it.
