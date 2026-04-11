# Hermes Command Parity

Source of truth for this audit: `/Users/raphaelmansuy/Github/03-working/hermes-agent/hermes_cli/commands.py`

This document tracks command-surface parity from first principles:

- Hermes slash name and aliases come from the upstream registry
- EdgeCrab TUI parity means the slash command exists in `CommandRegistry`
- EdgeCrab CLI parity means the command is reachable either as a dedicated
  clap subcommand or through `edgecrab slash <command...>`

## Current parity matrix

| Hermes command | EdgeCrab TUI slash | EdgeCrab CLI argv | Notes |
|---|---|---|---|
| `new`, `reset` | yes | `edgecrab slash new` | same live-session reset path |
| `clear` | yes | `edgecrab slash clear` | matches Hermes fresh-session behavior |
| `history` | yes | `edgecrab slash history` | live session history view |
| `save` | yes | `edgecrab slash save` | TUI slash plus saved-session exports |
| `retry` | yes | `edgecrab slash retry` | same undo-and-resend flow |
| `undo` | yes | `edgecrab slash undo` | same live-session mutation path |
| `title` | yes | `edgecrab slash title <name>` | sets persisted session title |
| `branch`, `fork` | yes | `edgecrab slash branch [name]` | alias preserved |
| `compress` | yes | `edgecrab slash compress` | same live compression flow |
| `rollback` | yes | `edgecrab slash rollback [name]` | checkpoint tool bridge |
| `stop` | yes | `edgecrab slash stop` | stops current turn |
| `approve` | yes | `edgecrab slash approve [session\|always]` | gateway/runtime approval surface |
| `deny` | yes | `edgecrab slash deny` | gateway/runtime approval surface |
| `background`, `bg` | yes | `edgecrab slash background <prompt>` | isolated background session |
| `btw` | yes | `edgecrab slash btw <question>` | ephemeral side-question path |
| `queue`, `q` | yes | `edgecrab slash queue <prompt>` | queued next-turn prompt |
| `status` | yes | `edgecrab status` or `edgecrab slash status` | dedicated CLI plus slash |
| `profile` | yes | `edgecrab profile ...` or `edgecrab slash profile` | dedicated tree plus slash bridge |
| `sethome`, `set-home` | yes | `edgecrab slash sethome [channel]` | gateway home-channel control |
| `resume` | yes | `edgecrab --resume <id>` or `edgecrab slash resume [id]` | both runtime and slash entrypoints |
| `config` | yes | `edgecrab config ...` or `edgecrab slash config` | dedicated tree plus TUI center |
| `model` | yes | `edgecrab model` or `edgecrab slash model [name]` | dedicated TUI opener plus slash |
| `provider` | yes | `edgecrab slash provider` | slash-driven info surface |
| `prompt` | yes | `edgecrab slash prompt [text]` | persisted override behavior |
| `personality` | yes | `edgecrab slash personality [name]` | session overlay |
| `statusbar`, `sb` | yes | `edgecrab slash statusbar [mode]` | persisted visibility toggle |
| `verbose` | yes | `edgecrab slash verbose [mode]` | same tool-progress policy |
| `yolo` | yes | `edgecrab --yolo` or `edgecrab slash yolo [mode]` | startup flag plus runtime toggle |
| `reasoning` | yes | `edgecrab slash reasoning [mode]` | same reasoning control surface |
| `skin` | yes | `edgecrab slash skin [name]` | `/theme` alias preserved |
| `voice` | yes | `edgecrab slash voice [mode]` | voice/TTS control path |
| `tools` | yes | `edgecrab tools ...` or `edgecrab slash tools` | dedicated tree plus overlay |
| `toolsets` | yes | `edgecrab tools list` or `edgecrab slash toolsets` | dedicated and slash surfaces |
| `skills` | yes | `edgecrab skills ...` or `edgecrab slash skills` | dedicated tree plus overlay |
| `cron` | yes | `edgecrab cron ...` or `edgecrab slash cron` | dedicated tree plus slash |
| `reload-mcp`, `reload_mcp` | yes | `edgecrab slash reload-mcp` | live MCP reconnect |
| `browser` | yes | `edgecrab slash browser [sub]` | CDP control path |
| `plugins` | yes | `edgecrab plugins ...` or `edgecrab slash plugins` | dedicated tree plus overlay |
| `commands` | yes | `edgecrab slash commands [page]` | gateway command catalog |
| `help` | yes | `edgecrab slash help` | same registry help |
| `usage` | yes | `edgecrab slash usage` | live token/cost usage |
| `insights` | yes | `edgecrab insights [--days N]` or `edgecrab slash insights [days]` | now matches Hermes optional day window |
| `platforms`, `gateway` | yes | `edgecrab gateway ...` or `edgecrab slash platforms` | dedicated gateway CLI plus slash info/control |
| `paste` | yes | `edgecrab slash paste` | clipboard-assisted input flow |
| `update` | yes | `edgecrab update` or `edgecrab slash update` | dedicated updater plus TUI/gateway trigger |
| `quit`, `exit`, `q` | yes | `edgecrab slash quit` | interactive-only by design |

## Honest notes

- Parity is strongest on the slash surface itself. Hermes and EdgeCrab now
  expose the same operator vocabulary in the TUI.
- CLI argv parity is intentionally implemented with one generic bridge instead
  of dozens of thin clap wrappers. That is better engineering, but it is not
  identical to Hermes' exact top-level command layout.
- Dedicated top-level CLI entrypoints still exist where they provide real value:
  `chat`, `model`, `insights`, `status`, `profile`, `config`, `tools`,
  `skills`, `cron`, `gateway`, `auth`, `webhook`, and related families.
