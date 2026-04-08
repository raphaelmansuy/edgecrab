
# Config & Interactive TUI (Deep Dive)

EdgeCrab provides a **full-featured TUI** (built with `ratatui`), a CLI-driven config system, a setup wizard, slash commands, and a skin/theme engine.

## TUI/CLI Features

- **TUI**: [`app.rs`](../../crates/edgecrab-cli/src/app.rs) — scrollable output, markdown rendering, animated spinner, status bar (tokens, cost, model), tab completion overlay for 40+ slash commands, multi-line input, ghost text from history.
- **Slash commands**: [`commands.rs`](../../crates/edgecrab-cli/src/commands.rs) — 46 commands, 54+ aliases, covering navigation, model/session/config/tools/memory/analysis/appearance/advanced/gateway/media/diagnostics.
- **Setup wizard**: [`setup.rs`](../../crates/edgecrab-cli/src/setup.rs) — interactive first-run and reconfiguration, provider/model/toolset/gateway/agent settings, env var detection, user-friendly prompts.
- **Skin engine**: [`skin_engine.rs`](../../crates/edgecrab-cli/src/skin_engine.rs) — hermes-agent-compatible YAML skins, built-in and user-defined, controls colors, spinner, branding, tool prefix.
- **Config system**: [`config.rs`](../../crates/edgecrab-core/src/config.rs) — layered config (defaults, YAML, env, CLI), all fields have sane defaults, user can override via config.yaml or CLI args.

## Interactive Menus & UX

- **Tab completion**: TUI supports fuzzy-matched tab completion for all slash commands.
- **Setup wizard**: Interactive, sectioned, with env var detection for provider keys.
- **Skins**: User can switch skins at runtime (`/theme` opens the browser, `/theme reload` explicitly reloads `skin.yaml`), or install custom YAML skins.
- **Model-routing UX is consistent**: `/model`, `/cheap_model`, and `/moa aggregator` all use the same full-screen selector pattern; `/moa references` uses a searchable multi-select roster editor.
- **Config Center**: `/config` opens a searchable TUI control surface for state summary, paths, primary/cheap/vision/image/MoA model routing, display toggles, voice status, gateway home channels, and update checks.
- **No `inquire` menus**: Unlike EdgeCode, EdgeCrab does not use `inquire` for config menus; all interaction stays in TUI overlays and CLI prompts.
- **User-focused UX**: TUI is designed for speed (60+ FPS), minimal memory (<5MB), and keyboard-driven workflows.

## Key Code & Docs

- [main.rs](../../crates/edgecrab-cli/src/main.rs)
- [app.rs](../../crates/edgecrab-cli/src/app.rs)
- [commands.rs](../../crates/edgecrab-cli/src/commands.rs)
- [setup.rs](../../crates/edgecrab-cli/src/setup.rs)
- [skin_engine.rs](../../crates/edgecrab-cli/src/skin_engine.rs)
- [config.rs](../../crates/edgecrab-core/src/config.rs)
- [site/.../getting-started/quick-start.md](../../site/.../getting-started/quick-start.md)

## Limitations & TODOs

- **Config Center is orchestration-first**: `/config` launches existing handlers and selectors, but it is not yet a full inline form editor for every structured config block.
- **Structured routing fields still need inline editors**: `cheap_base_url`, `cheap_api_key_env`, and other non-selector smart-routing fields still rely on YAML or CLI-level editing.
- **No web-based config UI**: All configuration is terminal-based.

---
**TODOs:**
- Add editable config forms for common structured settings inside the config center
- Add inline forms for advanced smart-routing and MoA settings
- Add web-based config UI (optional)
