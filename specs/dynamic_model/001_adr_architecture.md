# ADR 001: Dynamic Discovery Architecture

## Status

Accepted

## Context

Current dynamic discovery lives in `edgecrab-cli` as a single module with
provider matching, HTTP details, caching, and fallback logic mixed together.
That makes the CLI the owner of discovery policy, which is the wrong boundary.

EdgeCrab already has a core catalog abstraction in `edgecrab-core`. Dynamic
discovery should extend that domain, not sit as a CLI-only utility.

## Decision

Move dynamic model discovery into `edgecrab-core` and expose a reusable
adapter-driven API:

- `discover_provider_models(provider)`
- `discover_multiple(providers)`
- `live_discovery_availability(provider)`
- `live_discovery_providers()`
- `discovery_provider_statuses()`
- `merge_grouped_catalog_with_dynamic(...)`

Implementation structure:

```text
edgecrab-core::model_discovery
  ├── provider alias normalization
  ├── adapter registry
  ├── per-provider cache read/write
  ├── static catalog fallback
  └── public query helpers for CLI/TUI
```

Each adapter owns exactly one provider integration.

## Consequences

### Positive

- DRY: one discovery implementation for CLI, TUI, and future setup flows.
- SOLID: provider-specific behavior is isolated.
- Easier tests: adapters, cache policy, alias normalization, and merge behavior
  can be tested independently.
- Future providers can be added without editing a monolithic `match` block.

### Negative

- `edgecrab-core` gains lightweight network-facing discovery code.
- The first extraction adds some indirection compared with a small inline helper.

## Rejected alternatives

### Keep discovery in CLI

Rejected because discovery policy is not presentation logic.

### Make the static catalog fully dynamic

Rejected because pricing, labels, defaults, and curated model metadata still
need a stable source of truth.
