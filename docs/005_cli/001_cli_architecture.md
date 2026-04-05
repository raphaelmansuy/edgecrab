# CLI Architecture

Verified against:
- `crates/edgecrab-cli/src/main.rs`
- `crates/edgecrab-cli/src/cli_args.rs`
- `crates/edgecrab-cli/src/commands.rs`

The `edgecrab` binary is both a full-screen TUI and a multi-command CLI.

## Entry points

```text
edgecrab [prompt]
  -> interactive TUI by default

edgecrab <subcommand>
  -> clap command path
```

## Current clap subcommands

- `profile`
- `completion`
- `setup`
- `doctor`
- `migrate`
- `acp`
- `version`
- `whatsapp`
- `status`
- `sessions`
- `config`
- `tools`
- `mcp`
- `plugins`
- `cron`
- `gateway`
- `skills`

## Slash-command surface

The slash-command registry currently defines 53 commands. The canonical names include:

- navigation: `help`, `quit`, `clear`, `version`, `status`, `new`
- session: `session`, `retry`, `undo`, `stop`, `history`, `save`, `export`, `title`, `resume`
- model and runtime: `model`, `models`, `provider`, `reasoning`, `stream`, `vision_model`
- config and display: `config`, `prompt`, `verbose`, `personality`, `theme`, `statusbar`, `mouse`
- tools and extensions: `tools`, `toolsets`, `reload-mcp`, `mcp-token`, `plugins`, `skills`, `browser`
- analysis: `cost`, `usage`, `compress`, `insights`
- workflow: `queue`, `background`, `rollback`, `cron`, `voice`, `paste`
- gateway actions: `platforms`, `approve`, `deny`, `sethome`, `update`

## Important runtime modules

- `app.rs`: ratatui event loop
- `runtime.rs`: shared runtime construction
- `setup.rs`: onboarding flow
- `doctor.rs`: diagnostics
- `profile.rs`: profile switching and isolation
- `skin_engine.rs`: theme loading
- `plugins.rs` and `plugins_cmd.rs`: plugin inspection and commands

## Practical note

The CLI is the composition root for most features. If a behavior feels like "the app starts wrong" or "the agent is wired wrong," the fix is often in `main.rs` or `runtime.rs`, not in the TUI widgets themselves.
