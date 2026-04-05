# Task Log: Setup Reconfiguration & Config Parity

**Date:** 2026-03-29 15:30

## Actions
- Rewrote `setup.rs` (~700 lines) to support reconfiguration menu, section-specific setup (`model`, `tools`, `gateway`, `agent`), and `--force` flag
- Added `section` positional arg and `--force` flag to `Setup` CLI command in `cli_args.rs`
- Wired `run_with_options(section, force)` in `main.rs` dispatch
- Added `config edit` (opens `$EDITOR`) and `config env-path` subcommands
- Added 7 new tests (setup: load_config, save_round_trip, preserve_unknown_keys, sections_valid; cli: parse_setup_force, parse_setup_section)

## Decisions
- Round-trip config via `serde_yml::Value` (preserves unknown YAML keys)
- Keep backward-compatible `run()` wrapper (suppressed dead_code warning)
- Default to reconfigure menu when config exists (no more "delete and re-run")

## Next Steps
- Consider adding `config check` / `config migrate` subcommands for full hermes parity
- Niche platform adapters (email, matrix, mattermost) remain unimplemented

## Lessons/Insights
- Hermes's multi-section setup wizard UX is significantly better than "delete config and re-run" — worth porting
