# ADR-002: Add a TUI Config Center Instead of Expanding Flat Text Output

Status: accepted

## Context

Configuration in EdgeCrab spans:

- runtime display preferences
- model routing
- gateway delivery defaults
- voice behavior
- filesystem locations
- persistent YAML state

Before this change, `/config` mostly printed paths. That was insufficient for the importance of the configuration system and inconsistent with the rest of the TUI, which already uses selector-style overlays for model, skills, sessions, skins, and MCP.

## Decision

`/config` becomes an interactive config center with searchable actions and detailed side-panel context.

## Why

This is the lowest-friction way to improve operator UX without prematurely building a large form engine.

It gives the user:

- one entry point for configuration-related tasks
- consistency with existing overlay UX
- enough detail to understand current state
- fast routing into deeper dedicated selectors

## Initial action set

- session/config summary
- important filesystem paths
- model selector
- cheap-model selector
- vision model selector
- image model selector
- MoA aggregator selector
- MoA reference-roster selector
- streaming toggle
- reasoning-pane toggle
- status-bar toggle
- skin browser
- voice status
- gateway home-channel summary
- update status

## Consequences

- The config center is an orchestration surface first, not yet a full editor.
- It keeps EdgeCrab DRY by reusing existing handlers rather than introducing duplicate config mutation logic.
- It creates a clean path for future editable config forms.

## References

- [`crates/edgecrab-cli/src/app.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/app.rs)
- [`crates/edgecrab-core/src/config.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-core/src/config.rs)
- [`docs/feature-docs/04-config-tui.md`](/Users/raphaelmansuy/Github/03-working/edgecrab/docs/feature-docs/04-config-tui.md)
