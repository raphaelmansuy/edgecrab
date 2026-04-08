# CLI/TUI Improvement Test Plan

Status: accepted

## Objectives

- prove new command wiring is correct
- prove persistence works for the new display preference
- prove smart-routing cheap-model edits update runtime and persisted config
- prove MoA aggregator and reference-roster edits update runtime and persisted config
- prove binary CLI entry surfaces (`--help`, `--version`, `version`, `status`) are wired and truthful
- prove the config center opens and exposes actions
- prove approval slash commands interact with real TUI state
- prove gateway home-channel edits persist

## Implemented tests

- `commands::tests::dispatch_config_and_statusbar_commands`
- `commands::tests::dispatch_theme_no_args_opens_browser_and_reload_is_explicit`
- `commands::tests::dispatch_approve_deny`
- `commands::tests::dispatch_cheap_model_commands`
- `commands::tests::dispatch_moa_commands`
- `app::tests::persist_display_preferences_round_trip_includes_status_bar`
- `app::tests::persist_smart_routing_round_trip`
- `app::tests::persist_moa_config_round_trip`
- `app::tests::approve_command_resolves_pending_overlay`
- `app::tests::sethome_updates_single_enabled_platform_without_explicit_platform_arg`
- `app::tests::config_command_opens_overlay`
- `app::tests::cheap_model_command_updates_agent_and_config`
- `app::tests::moa_reference_selector_tracks_saved_selection`
- manual smoke:
  - `target/debug/edgecrab --help`
  - `target/debug/edgecrab --version`
  - `target/debug/edgecrab version`
  - `target/debug/edgecrab status`
  - `target/debug/edgecrab <top-level-subcommand> --help` for every documented binary subcommand
  - `target/debug/edgecrab config --help`
  - `target/debug/edgecrab mcp --help`

## Verification sequence

1. `cargo test -p edgecrab-cli`
2. `cargo fmt --all --check`
3. `cargo clippy -p edgecrab-cli -- -D warnings`
4. `cargo test`

## Known verification risk

Full-workspace verification can be limited by local disk pressure because the workspace is large and macOS link steps are expensive. If disk pressure recurs, free build-cache space before rerunning the full matrix.
