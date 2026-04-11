---
title: Hermes Command Parity
description: Exact command-surface parity audit between Hermes Agent and EdgeCrab, using Hermes source as the contract.
sidebar:
  order: 2
---

Source of truth for this audit: `hermes_cli/commands.py` from the Hermes repo.

Parity is evaluated on two axes:

- **TUI slash parity**: the Hermes slash command exists in EdgeCrab's `CommandRegistry`
- **CLI argv parity**: the command is reachable from the shell either as a dedicated subcommand or via `edgecrab slash <command...>`

## Parity matrix

| Hermes command | EdgeCrab TUI slash | EdgeCrab CLI argv | Notes |
|---|---|---|---|
| `new`, `reset` | yes | `edgecrab slash new` | same live-session reset path |
| `clear` | yes | `edgecrab slash clear` | fresh-session clear |
| `history` | yes | `edgecrab slash history` | live history view |
| `save` | yes | `edgecrab slash save` | slash save/export path |
| `retry` | yes | `edgecrab slash retry` | retry last user turn |
| `undo` | yes | `edgecrab slash undo` | remove last exchange |
| `title` | yes | `edgecrab slash title <name>` | set persisted title |
| `branch`, `fork` | yes | `edgecrab slash branch [name]` | alias preserved |
| `compress` | yes | `edgecrab slash compress` | manual context compression |
| `rollback` | yes | `edgecrab slash rollback [name]` | checkpoint restore/list |
| `stop` | yes | `edgecrab slash stop` | interrupt current run |
| `approve` | yes | `edgecrab slash approve [session\|always]` | approval resolution |
| `deny` | yes | `edgecrab slash deny` | approval deny |
| `background`, `bg` | yes | `edgecrab slash background <prompt>` | background session |
| `btw` | yes | `edgecrab slash btw <question>` | ephemeral side-question |
| `queue`, `q` | yes | `edgecrab slash queue <prompt>` | next-turn queue |
| `status` | yes | `edgecrab status` or `edgecrab slash status` | dedicated plus slash |
| `profile` | yes | `edgecrab profile ...` or `edgecrab slash profile` | dedicated plus slash |
| `sethome`, `set-home` | yes | `edgecrab slash sethome [channel]` | gateway home-channel |
| `resume` | yes | `edgecrab --resume <id>` or `edgecrab slash resume [id]` | runtime and slash |
| `config` | yes | `edgecrab config ...` or `edgecrab slash config` | dedicated tree plus TUI center |
| `model` | yes | `edgecrab model` or `edgecrab slash model [name]` | dedicated opener plus slash |
| `provider` | yes | `edgecrab slash provider` | provider info |
| `prompt` | yes | `edgecrab slash prompt [text]` | persisted prompt override |
| `personality` | yes | `edgecrab slash personality [name]` | session persona |
| `statusbar`, `sb` | yes | `edgecrab slash statusbar [mode]` | persisted status bar toggle |
| `verbose` | yes | `edgecrab slash verbose [mode]` | tool-progress policy |
| `yolo` | yes | `edgecrab --yolo` or `edgecrab slash yolo [mode]` | startup flag plus toggle |
| `reasoning` | yes | `edgecrab slash reasoning [mode]` | reasoning control |
| `skin` | yes | `edgecrab slash skin [name]` | `/theme` alias preserved |
| `voice` | yes | `edgecrab slash voice [mode]` | voice/TTS controls |
| `tools` | yes | `edgecrab tools ...` or `edgecrab slash tools` | dedicated plus overlay |
| `toolsets` | yes | `edgecrab tools list` or `edgecrab slash toolsets` | dedicated and slash |
| `skills` | yes | `edgecrab skills ...` or `edgecrab slash skills` | dedicated plus overlay |
| `cron` | yes | `edgecrab cron ...` or `edgecrab slash cron` | dedicated plus slash |
| `reload-mcp`, `reload_mcp` | yes | `edgecrab slash reload-mcp` | live reconnect |
| `browser` | yes | `edgecrab slash browser [sub]` | CDP control |
| `plugins` | yes | `edgecrab plugins ...` or `edgecrab slash plugins` | dedicated plus overlay |
| `commands` | yes | `edgecrab slash commands [page]` | gateway catalog |
| `help` | yes | `edgecrab slash help` | registry help |
| `usage` | yes | `edgecrab slash usage` | live usage |
| `insights` | yes | `edgecrab insights [--days N]` or `edgecrab slash insights [days]` | optional day window matches Hermes |
| `platforms`, `gateway` | yes | `edgecrab gateway ...` or `edgecrab slash platforms` | dedicated gateway plus slash |
| `paste` | yes | `edgecrab slash paste` | clipboard input |
| `update` | yes | `edgecrab update` or `edgecrab slash update` | updater plus slash |
| `quit`, `exit`, `q` | yes | `edgecrab slash quit` | interactive-only by design |

## What this means

- The Hermes slash vocabulary is now present in EdgeCrab.
- EdgeCrab does **not** try to clone Hermes' exact top-level CLI layout for
  every slash command. Instead, it uses one generic bridge: `edgecrab slash`.
- That is a deliberate DRY/SOLID choice. One command grammar and one handler
  graph are easier to keep correct than dozens of duplicate wrappers.

## Related docs

- [CLI Commands](/reference/cli-commands/)
- [Slash Commands](/reference/slash-commands/)
