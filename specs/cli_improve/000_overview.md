# CLI Improve Overview

Status: accepted

This spec set audits the current EdgeCrab command surface and records the first implementation pass for CLI/TUI usability fixes.

## Scope

- Binary CLI subcommands defined in [`crates/edgecrab-cli/src/cli_args.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/cli_args.rs) and dispatched in [`crates/edgecrab-cli/src/main.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/main.rs)
- TUI slash commands defined in [`crates/edgecrab-cli/src/commands.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/commands.rs) and handled in [`crates/edgecrab-cli/src/app.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/app.rs)
- Config-system UX spanning [`crates/edgecrab-core/src/config.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-core/src/config.rs), [`crates/edgecrab-cli/src/setup.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/setup.rs), and the TUI runtime

## Documents

- [001_adr_command_surface.md](./001_adr_command_surface.md)
- [002_adr_config_center.md](./002_adr_config_center.md)
- [003_audit_matrix.md](./003_audit_matrix.md)
- [004_test_plan.md](./004_test_plan.md)

## Implemented in this pass

- `/config` now opens a real in-TUI config center and supports useful inspection subcommands.
- `/statusbar` now has real state, persistence, and runtime rendering behavior.
- `/theme` with no args now opens the skin browser as the help text implies; `reload` is explicit.
- `/approve` and `/deny` now resolve the live TUI approval/clarify state instead of printing placeholders.
- `/sethome` now reads and writes supported gateway home-channel config.
- `/update` now reports actionable local git-based upgrade status instead of a fake “latest version” message.

## Follow-on work

- Expand the config center from action launcher to full editable forms.
- Normalize config writes so env-derived values are not serialized back to disk.
- Add a first-class update/install strategy for non-git installations.
