# CLI/TUI Audit Matrix

Status: accepted

## Method

Cross-reference sources:

- CLI parsing: [`crates/edgecrab-cli/src/cli_args.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/cli_args.rs)
- CLI dispatch: [`crates/edgecrab-cli/src/main.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/main.rs)
- Slash registry: [`crates/edgecrab-cli/src/commands.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/commands.rs)
- TUI handlers: [`crates/edgecrab-cli/src/app.rs`](/Users/raphaelmansuy/Github/03-working/edgecrab/crates/edgecrab-cli/src/app.rs)
- User-facing docs: [`docs/005_cli/001_cli_architecture.md`](/Users/raphaelmansuy/Github/03-working/edgecrab/docs/005_cli/001_cli_architecture.md), [`docs/feature-docs/04-config-tui.md`](/Users/raphaelmansuy/Github/03-working/edgecrab/docs/feature-docs/04-config-tui.md)

Legend:

- `green`: wired and substantively useful
- `yellow`: wired but thin or improvable
- `red`: miswired, misleading, or placeholder

## Binary CLI Subcommands

| Surface | Status | Notes |
|---|---|---|
| `setup`, `doctor`, `migrate`, `acp`, `status`, `completion`, `whatsapp` | green | Proper clap parsing and dedicated dispatch paths exist. |
| `profile`, `sessions`, `config`, `tools`, `mcp`, `plugins`, `cron`, `gateway`, `skills` | green | Real subcommand trees with targeted handlers in `main.rs` or helper modules. |
| `config edit` | yellow | Useful, but editor launch behavior is outside the TUI and not mirrored interactively. |
| `mcp` operator surface | green | Particularly strong: list/search/install/test/doctor/auth/login/remove. |

## TUI Slash Commands: Navigation and Session

| Commands | Status | Notes |
|---|---|---|
| `/help`, `/quit`, `/clear`, `/version`, `/status`, `/new`, `/retry`, `/undo`, `/stop`, `/history`, `/save`, `/export`, `/title`, `/resume`, `/session` | green | Properly mapped to live TUI/agent state. |
| `/session` no args | green | Opens the session browser through real DB-backed state. |

## TUI Slash Commands: Models and Analysis

| Commands | Status | Notes |
|---|---|---|
| `/model`, `/models`, `/vision_model`, `/image_model`, `/provider`, `/reasoning`, `/stream`, `/cost`, `/usage`, `/compress`, `/insights` | green | Wired to live selectors, routing state, or agent snapshots. |

## TUI Slash Commands: Config and Appearance

| Commands | Previous | Current | Notes |
|---|---|---|---|
| `/config` | red | green | Replaced path dump with config center plus useful text subcommands. |
| `/prompt`, `/verbose`, `/personality`, `/mouse`, `/paste` | green | green | Already useful. |
| `/theme` | red | green | No-arg behavior now matches browser-centric help; reload is explicit. |
| `/statusbar` | red | green | Now toggles a real persisted display setting. |

## TUI Slash Commands: Tools, MCP, Skills, Browser

| Commands | Status | Notes |
|---|---|---|
| `/tools`, `/toolsets`, `/mcp`, `/reload-mcp`, `/mcp-token`, `/plugins`, `/skills`, `/browser` | green | Backed by real registry/browser/runtime behavior. |

## TUI Slash Commands: Workflow, Gateway, Voice

| Commands | Previous | Current | Notes |
|---|---|---|---|
| `/queue`, `/background`, `/rollback`, `/cron`, `/voice`, `/platforms` | green | green | Backed by real runtime behavior. |
| `/approve`, `/deny` | red | green | Now resolve the active approval/clarify state instead of printing placeholders. |
| `/sethome` | red | green | Now reads/writes supported home-channel config. |
| `/update` | red | green | Now reports local git-based upgrade state with action guidance. |

## Main UX Recommendations Beyond This Patch

1. Add editable forms to the config center for common structured settings instead of only action launchers.
2. Add a proper non-git update strategy for packaged installs.
3. Move gateway home-channel editing into a richer selector with platform-aware validation.
4. Unify CLI `config edit` and TUI `/config edit` around a terminal-safe editor handoff flow.
5. Audit persistence helpers that currently serialize env-resolved values back into YAML.
