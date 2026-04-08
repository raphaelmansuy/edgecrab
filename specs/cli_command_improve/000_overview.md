# CLI Command Improve Overview

Status: accepted

This spec set audits the full terminal command surface exposed by the `edgecrab`
binary, not the in-TUI slash-command layer.

## Scope

- Clap entry surface defined in [`crates/edgecrab-cli/src/cli_args.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/cli_args.rs)
- Binary dispatch and helper functions in [`crates/edgecrab-cli/src/main.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/main.rs)
- Runtime home/config loading in [`crates/edgecrab-cli/src/runtime.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/runtime.rs)
- Binary command helpers such as [`crates/edgecrab-cli/src/status_cmd.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/status_cmd.rs), [`crates/edgecrab-cli/src/gateway_cmd.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/gateway_cmd.rs), [`crates/edgecrab-cli/src/plugins_cmd.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/plugins_cmd.rs), and [`crates/edgecrab-cli/src/profile.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/profile.rs)

## Documents

- [001_adr_binary_command_contract.md](./001_adr_binary_command_contract.md)
- [002_audit_matrix.md](./002_audit_matrix.md)
- [003_adr_runtime_home_and_editor_semantics.md](./003_adr_runtime_home_and_editor_semantics.md)
- [004_test_plan.md](./004_test_plan.md)

## Implemented in this pass

- `--config /path/to/config.yaml` now redefines the effective runtime home for binary commands, so `.env`, `state.db`, plugins, skills, and sibling paths stay coherent with the chosen config root.
- `edgecrab config edit` now accepts realistic `$EDITOR` and `$VISUAL` values that include arguments, such as `code --wait`.
- `edgecrab config env-path` now reports the `.env` file adjacent to the active config path instead of always pointing at the default home.
- `edgecrab version` now derives its provider inventory from the model catalog instead of a drift-prone hardcoded list.

## Cross-References

- TUI/slash-command audit → [../cli_improve/000_overview.md](/Users/raphaelmansuy/Github/03-working/edgecrab/specs/cli_improve/000_overview.md)
- User-facing CLI architecture → [docs/005_cli/001_cli_architecture.md](/Users/raphaelmansuy/Github/03-working/edgecrab/docs/005_cli/001_cli_architecture.md)
