# ADR-005: Give Cheap-Model and MoA Configuration First-Class Selector UX

Status: accepted

## Context

EdgeCrab already had a strong `/model` selector, but adjacent routing controls were uneven:

- smart-routing `cheap_model` existed in config but lacked equivalent interactive UX
- Mixture-of-Agents defaults were effectively hardcoded unless each tool call supplied overrides
- `/config` could summarize model state but could not route the operator into all relevant model-routing controls

That created a quality gap inside the same product area. Users could set the primary model delightfully, but not the cheap-model path or the MoA roster that powers higher-cost multi-model reasoning.

## Decision

EdgeCrab treats model-routing configuration as one coherent operator surface:

- `/model` remains the primary-model selector
- `/cheap_model` reuses the same full-screen selector pattern as `/model`
- `/moa aggregator` reuses the same selector pattern as `/model`
- `/moa references` gets a searchable multi-select overlay optimized for roster editing
- `/config` exposes direct entry points for cheap model, MoA aggregator, and MoA references
- `config.yaml` gains a top-level `moa:` block so the TUI, runtime, and `mixture_of_agents` tool share one source of truth

## Why

This follows first principles:

1. Routing behavior is only trustworthy if the user can inspect and change it easily.
2. Similar tasks should use similar interaction patterns.
3. Tool defaults must come from durable configuration, not duplicated hardcoded state.
4. Multi-model reasoning must keep one valid reference model at all times.

## UX Shape

- Cheap-model selection feels identical to `/model`: open fast from the embedded catalog, then refresh live inventories in place.
- MoA aggregator selection feels identical to `/model` because it is a single-choice model-routing decision.
- MoA references use multi-select because the operator is editing a set, not a scalar value.
- Text commands remain available for automation and power users:
  - `/cheap_model status`
  - `/cheap_model off`
  - `/cheap_model <provider/model>`
  - `/moa status`
  - `/moa aggregator <provider/model>`
  - `/moa add <provider/model>`
  - `/moa remove <provider/model>`
  - `/moa reset`

## Consequences

- Runtime agent state and persisted YAML now stay aligned for smart routing and MoA defaults.
- The `mixture_of_agents` tool becomes more genuinely useful in unattended flows because config-defined defaults apply automatically.
- The config center is now closer to a complete model-routing hub, though not yet a universal inline form editor.
- Reference-roster UX must preserve edge cases:
  - no duplicate models
  - cannot save an empty roster
  - reset restores built-in defaults deterministically

## References

- [`crates/edgecrab-cli/src/app.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/app.rs)
- [`crates/edgecrab-cli/src/commands.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/commands.rs)
- [`crates/edgecrab-core/src/config.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-core/src/config.rs)
- [`crates/edgecrab-tools/src/tools/mixture_of_agents.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-tools/src/tools/mixture_of_agents.rs)
