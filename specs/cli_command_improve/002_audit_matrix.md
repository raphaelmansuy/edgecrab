# Binary CLI Audit Matrix

Status: accepted

## Method

Cross-reference sources:

- CLI definitions: [`crates/edgecrab-cli/src/cli_args.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/cli_args.rs)
- Binary dispatch: [`crates/edgecrab-cli/src/main.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/main.rs)
- Runtime loading: [`crates/edgecrab-cli/src/runtime.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/runtime.rs)
- User docs: [`docs/005_cli/001_cli_architecture.md`](/Users/raphaelmansuy/Github/03-working/edgecrab/docs/005_cli/001_cli_architecture.md)

Legend:

- `green`: correctly wired and substantively useful
- `yellow`: wired but still improvable
- `red`: misleading, fragile, or miswired

## Entry Surface

| Surface | Previous | Current | Notes |
|---|---|---|---|
| `edgecrab --help` | green | green | Clap tree is complete and executable from the built binary. |
| `edgecrab --version` | green | green | Short version output is correct and stable. |
| `edgecrab version` | yellow | green | Provider inventory now comes from the model catalog instead of a stale hardcoded list. |
| Global modifiers `--model`, `--toolset`, `--session`, `--continue`, `--resume`, `--quiet`, `--config`, `--debug`, `--no-banner`, `--worktree`, `--skill`, `--profile` | yellow | green | `--config` now rebases the effective runtime home coherently for binary commands. |

## Top-Level Subcommands

| Commands | Status | Notes |
|---|---|---|
| `profile`, `completion`, `setup`, `doctor`, `migrate`, `acp`, `version`, `whatsapp`, `status`, `sessions`, `config`, `tools`, `mcp`, `plugins`, `cron`, `gateway`, `skills` | green | All top-level commands were observed via built-binary `--help` smoke checks and traced to real handlers. |

## Nested Command Families

| Family | Status | Notes |
|---|---|---|
| `profile` | green | Strong and feature-complete: list/use/create/delete/show/alias/rename/export/import. |
| `sessions` | green | Real DB-backed list, browse, export, rename, prune, and stats flows. |
| `config` | green | `show`, `edit`, `set`, `path`, and `env-path` are all wired; `edit` and `env-path` were hardened in this pass. |
| `tools` | green | Real persistence against toolset config. |
| `mcp` | green | One of the strongest operator surfaces in the binary: list/refresh/search/view/install/test/doctor/auth/login/add/remove. |
| `plugins` | green | Real install/update/remove flows; now coherent with custom config-root home semantics through entry-point home rebasing. |
| `cron` | green | Complete scheduling operator surface. |
| `gateway` | green | Configuration, process control, and diagnostics are all real. |
| `skills` | green | Local, official, and remote skill flows are present and non-placeholder. |

## Main Improvement Findings Closed in This Pass

1. Custom config roots now behave as first-class runtime homes instead of partial overrides.
2. `config edit` now works with argument-bearing editor commands.
3. `version` now reports catalog-backed provider coverage instead of a manually curated subset.

## Remaining Recommendations

1. Add snapshot-style tests for clap help output so wording drift is caught automatically.
2. Add a machine-readable `edgecrab version --json` mode if external automation becomes a primary use case.
3. Consider a richer `status` output that distinguishes active vs available toolsets explicitly.
