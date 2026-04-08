# Binary CLI Improvement Test Plan

Status: accepted

## Objectives

- prove every documented top-level binary command has a live help path
- prove binary command dispatch remains wired after the runtime-home fix
- prove custom `--config` paths rebase the effective runtime home
- prove `config edit` accepts editor commands with arguments
- prove `version` output stays aligned with the model catalog

## Automated Verification

1. `cargo fmt --all --check`
2. `cargo clippy -p edgecrab-cli -- -D warnings`
3. `cargo test -p edgecrab-cli`
4. `cargo test --workspace`
5. `cargo test --workspace -- --include-ignored`

Note: real-browser launch tests remain opt-in even under `--include-ignored`.
Set `EDGECRAB_RUN_BROWSER_LAUNCH_TESTS=1` only in a deliberate headless environment.

## Key Regression Tests

- `main::tests::runtime_home_for_config_override_uses_parent_directory`
- `main::tests::activate_runtime_home_from_config_sets_edgecrab_home`
- `main::tests::editor_command_from_env_supports_editor_arguments`
- `main::tests::version_report_covers_catalog_providers`
- existing clap parser coverage in [`crates/edgecrab-cli/src/cli_args.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/cli_args.rs)

## Built-Binary Smoke Sweep

- `target/debug/edgecrab --help`
- `target/debug/edgecrab --version`
- `target/debug/edgecrab version`
- `target/debug/edgecrab status`
- `target/debug/edgecrab profile --help`
- `target/debug/edgecrab completion --help`
- `target/debug/edgecrab setup --help`
- `target/debug/edgecrab doctor --help`
- `target/debug/edgecrab migrate --help`
- `target/debug/edgecrab acp --help`
- `target/debug/edgecrab whatsapp --help`
- `target/debug/edgecrab sessions --help`
- `target/debug/edgecrab config --help`
- `target/debug/edgecrab tools --help`
- `target/debug/edgecrab mcp --help`
- `target/debug/edgecrab plugins --help`
- `target/debug/edgecrab cron --help`
- `target/debug/edgecrab gateway --help`
- `target/debug/edgecrab skills --help`
