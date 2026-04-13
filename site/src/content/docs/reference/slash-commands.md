---
title: Slash Commands
description: All EdgeCrab TUI and gateway slash commands with aliases, arguments, and keyboard shortcuts. Grounded in the shared edgecrab-command-catalog crate and the runtime handlers.
sidebar:
  order: 3
---

Type any slash command at the `>` prompt. Commands are case-insensitive.
Type `/` to open the autocomplete dropdown — installed skills are also
shown there as runnable commands.

EdgeCrab now uses a shared slash-command catalog for both the TUI and the
messaging gateway. That means `/help`, `/commands`, and gateway help text are
derived from one source instead of two drifting hand-maintained lists.

Every built-in slash command is also reachable from the shell through the
generic bridge:

```bash
edgecrab slash <command...>
edgecrab slash insights 14
edgecrab slash btw "sanity-check this migration plan"
```

---

## Quick Reference

```
Navigation   /help /quit /clear /new /status /version
Model        /model /cheap_model /vision_model /image_model /moa /provider /reasoning /stream
Session      /session /sessions /retry /undo /stop /btw /history /save /export /title /resume /branch
Config       /config /prompt /verbose /personality /statusbar /worktree /yolo
Tools        /tools /toolsets /mcp /reload-mcp /plugins
Memory       /memory /skills /profile /profiles
Analysis     /cost /usage /compress /insights
Advanced     /queue /background /rollback
Gateway      /platforms /gateway /commands /approve /deny /sethome /webhook /update
Scheduling   /cron
Media        /voice /browser
Appearance   /skin /theme /paste /image /mouse
Diagnostics  /doctor /copilot-auth /auth /login /logout /uninstall
Auth         /auth /login /logout
MCP          /mcp /mcp-token
```

---

## Navigation & Display

| Command | Aliases | Description |
|---------|---------|-------------|
| `/help` | `/h`, `/?` | Show the help overlay with all commands |
| `/quit` | `/exit`, `/q` | Exit EdgeCrab (auto-saves session) |
| `/clear` | `/cls` | Clear the screen and start a fresh session |
| `/new` | `/reset` | Start a fresh session (clears conversation history) |
| `/status` | | Show model, token count, iteration count, and cost |
| `/version` | | Print EdgeCrab version and build info |

---

## Model & Intelligence

| Command | Description |
|---------|-------------|
| `/model [name]` | Show current model or switch (e.g. `/model ollama/gemma4:latest`) |
| `/cheap_model [spec]` | Open, show, or set the cheap smart-routing model; `/cheap_model off` disables it |
| `/models [provider]` | List models; `/models <provider>` queries live, `/models refresh` refreshes cache |
| `/image_model [spec]` | Open image-model selector, or set/show the default image-generation backend |
| `/moa [subcommand]` | Show, enable, disable, or configure Mixture-of-Agents defaults (`status`, `on`, `off`, `reset`, `aggregator`, `experts`, `add`, `remove`); reset writes a safe baseline for the current chat model |
| `/vision_model [spec]` | Open vision model selector, or set/show the dedicated vision backend |
| `/provider` | List available providers |
| `/reasoning [level]` | Set reasoning effort: `off`, `low`, `medium`, `high` — or `show`/`hide` for think-block visibility (alias: `/think`) |
| `/stream [on\|off\|toggle\|status]` | Toggle live token streaming (alias: `/streaming`) |

---

## Session Management

| Command | Description |
|---------|-------------|
| `/session [id]` | List recent sessions or switch to a session by ID |
| `/sessions [browse\|search\|switch\|delete\|rename\|prune]` | Browse and manage persisted sessions from inside the TUI |
| `/retry` | Re-send the last user message |
| `/undo` | Remove the last user + assistant message pair from history |
| `/stop` | Abort the current in-flight agent request immediately |
| `/btw <question>` | Ask an ephemeral side question using the current context only; no tools, no persistence |
| `/history` | Show session turn count and token usage |
| `/save [path]` | Save conversation to a JSON file |
| `/export [path]` | Export conversation as Markdown |
| `/title <text>` | Set or rename the current session title |
| `/resume [id]` | Resume a previously saved session |
| `/branch [name]` | Fork the current session into a new persisted branch |
| `/session rename <id> <title>` | Rename a session |
| `/session delete <id>` | Delete a session |
| `/session prune <days>` | Delete sessions older than N days |

---

## Configuration

| Command | Description |
|---------|-------------|
| `/config` | Show config file paths and `EDGECRAB_HOME` directory |
| `/prompt` | Show, clear, or set the persisted custom system prompt override: `/prompt`, `/prompt clear`, `/prompt <text>` |
| `/verbose` | Cycle tool verbosity: `off` → `new` → `all` → `verbose`; use `/verbose <mode>` to set directly or `/verbose open` for the picker |
| `/personality [name]` | Show active personality or switch preset mid-session |
| `/skin [name]` | Switch skin preset (primary Hermes-compatible name; alias: `/theme`) |
| `/statusbar` | Toggle the status bar visibility |
| `/log [open\|level <level>]` | Open the log browser, live-follow local log tails, or persist the default log level (alias: `/logs`) |
| `/worktree [status\|on\|off\|toggle]` | Open the git-worktree status overlay or change the saved default for future launches (alias: `/w`) |
| `/yolo [on\|off\|toggle\|status]` | Toggle session-scoped dangerous-command approval bypass |

---

## Tools & Plugins

| Command | Description |
|---------|-------------|
| `/tools` | List all currently registered tools and their status, including live plugin tools under the `plugins` toolset |
| `/toolsets` | List toolset aliases and their member tools |
| `/mcp [subcommand]` | Browse, install, test, diagnose, or remove MCP servers |
| `/reload-mcp` | Drop and reconnect all MCP server connections |
| `/plugins [info\|status\|install\|enable\|disable\|toggle\|audit\|hub search\|hub browse\|hub refresh]` | Open the installed-plugin browser by default, stage enable/disable changes, search hubs, inspect audit history, and refresh the live runtime tool inventory |
| `/mcp-token [set\|remove\|list] <name> [token]` | Manage MCP OAuth Bearer tokens |

---

## Memory & Skills

| Command | Description |
|---------|-------------|
| `/memory` | Show all persistent memory files with sizes |
| `/skills [browse\|search\|install\|update\|remove]` | Open the installed-skills browser, launch the remote-skills browser, install, update, or remove skills; toggled skills become session-scoped preloaded skills |
| `/profile [subcommand]` | With no args, show the active profile name and effective home path; subcommands route into overlay modes or inline modal actions: `list`, `show`, `config`, `soul`, `memory`, `tools`, `use`, `create`, `delete`, `rename`, `alias`, `export`, `import` |
| `/profiles [subcommand]` | Open the interactive profile browser when called without args; in the browser use `Enter` to switch, `C` config, `S` SOUL, `M` memory, `T` tools, `A` alias, `E` export, `D` delete, `N` create, `I` import, `O` rename, `Tab` or `Left`/`Right` to cycle views, `H` or `?` for help, and `Home`/`End` to jump |

---

## Analysis & Cost

| Command | Description |
|---------|-------------|
| `/cost` | Show token usage and estimated USD cost for the session |
| `/usage` | Alias for `/cost` with full per-model breakdown |
| `/compress` | Manually trigger conversation compression (summarisation) |
| `/insights [days]` | Show current-session metrics plus historical analytics for the requested day window (default: 30) |

---

## Advanced Workflow

| Command | Description |
|---------|-------------|
| `/queue <prompt>` | Queue a prompt to run after the current turn finishes |
| `/background <prompt>` | Run a prompt as an isolated background session |
| `/rollback [name]` | List checkpoints or restore to checkpoint `<name>` |

---

## Appearance & Input

| Command | Description |
|---------|-------------|
| `/skin [name]` | Open the skin browser, reload `~/.edgecrab/skin.yaml`, or switch named preset |
| `/theme [name]` | Alias for `/skin` |
| `/paste` | Paste clipboard image or text into the input |
| `/image <path>` | Queue a local image file for the next prompt without using the clipboard |
| `/mouse [on\|off\|toggle\|status]` | Manage terminal mouse-capture mode |

---

## Gateway & Automation

| Command | Description |
|---------|-------------|
| `/platforms` | Show status of all configured messaging platforms |
| `/gateway [action]` | Show gateway status or control the local gateway runtime from the TUI |
| `/commands [page]` | Browse the gateway command catalog and installed skill commands |
| `/approve` | Approve a pending gateway action (inline button equivalent) |
| `/deny` | Deny a pending gateway action |
| `/sethome [channel]` | Set the current channel as the home notification channel |
| `/webhook [subcommand]` | List, create, test, or remove dynamic webhook subscriptions with event filters, duplicate suppression, rate limits, prompt templating, session skill preload, and Hermes-style final delivery routing |
| `/update` | Check for and install EdgeCrab binary updates |
| `/cron [subcommand]` | Show or manage scheduled cron jobs |
| `/voice [on\|off\|tts]` | Toggle voice input/output mode |
| `/browser [sub]` | Chrome CDP: `connect`, `disconnect`, `status`, `tabs`, `recording on\|off` |
| `/auth [subcommand]` | List auth targets, inspect status, add env-backed provider tokens, or remove cached auth state |
| `/login <target>` | Shortcut for one auth login/import flow; useful targets are `copilot`, `provider/openai`, and `mcp/<server>` |
| `/logout [target]` | Clear one target or all EdgeCrab-managed local auth caches |
| `/doctor` | Run diagnostics inline (providers, tools, platforms) |
| `/copilot-auth` | Trigger GitHub Copilot device-code authentication flow |
| `/uninstall [flags]` | Preview or execute a local uninstall plan; in the TUI it refuses destructive execution unless `--yes` is supplied |

---

## Keyboard Shortcuts

Sourced from the `/help` output in `commands.rs`:

### Scrolling

| Key | Action |
|-----|--------|
| `PgUp` / `PgDn` | Scroll output up / down one page |
| `Shift+Up` / `Shift+Down` | Scroll output 3 rows |
| `Alt+Up` / `Alt+Down` | Scroll output 5 rows |
| `Ctrl+Home` | Jump to top of output |
| `Ctrl+End` | Jump to bottom (live view) |

### Input Editing

| Key | Action |
|-----|--------|
| `Enter` | Submit message |
| `Shift+Enter` | Insert newline (multi-line input) |
| `Up` / `Down` | Navigate prompt history |
| `Right` / `Tab` | Accept ghost-text hint / tab complete |
| `Ctrl+U` | Clear the input field |
| `Ctrl+L` | Clear the screen |

### Agent Control

| Key | Action |
|-----|--------|
| `Ctrl+C` | Clear input / interrupt agent (double-press to force exit) |
| `Ctrl+D` | Exit (on empty input) |
| `Ctrl+M` | Toggle mouse-capture / text-selection mode |

---

## Personality Presets

Use `/personality <preset>` to overlay a conversational style for the
current session (does not change the underlying model):

| Preset | Style |
|--------|-------|
| `helpful` | Default — clear and professional |
| `concise` | Ultra-terse: no prose, just answers |
| `technical` | Deep technical detail; no hand-holding |
| `kawaii` | Enthusiastic and cute |
| `pirate` | Arr, matey |
| `philosopher` | Every reply is a meditation |
| `hype` | Maximum hype energy |
| `shakespeare` | Early Modern English |
| `noir` | Hard-boiled detective |
| `catgirl` | Anime catgirl |
| `creative` | Creative writing focus |
| `teacher` | Patient step-by-step explanations |
| `surfer` | Chill vibes |
| `uwu` | uwu speech mode |

---

## Pro Tips

- **Type `/` and press Tab**: The autocomplete dropdown shows all commands plus your installed skills — no need to memorize the full list.
- **Use `/config key value` for live tuning**: Change `max_iterations`, `reasoning_effort`, or `personality` mid-session without restarting.
- **`/rollback` lists checkpoints first**: Run `/rollback` with no arguments to see the snapshot list before committing to a restore.
- **`/verbose all` for debugging tool calls**: Shows every tool input and output inline. Switch back with `/verbose off` to declutter.
- **`/queue` chains prompts**: `/queue run cargo test` will run cargo test automatically after the current turn completes — useful for long refactors.
- **`/compress` before a long task**: Manually trigger history summarisation to free up context window before starting a compute-intensive task.

---

## FAQ

**What's the difference between `/new`, `/session`, and `/sessions`?**
`/new` (alias `/reset`) starts a fresh session immediately. `/session` focuses on the live current session. `/sessions` opens the persisted session archive and management flows.

**Can I run a slash command while the agent is responding?**
Yes for `/stop` and `/approve`/`/deny` — they interrupt or gate the current turn. Other commands are queued until the turn finishes.

**How does `/undo` work with tool calls?**
`/undo` removes the last user message and the assistant's reply (including all tool call turns) as a unit. Useful for retrying a prompt that went wrong.

**Does `/save` include tool call history?**
Yes — the JSON file includes the full message array with tool call and tool result turns.

**Can I alias a slash command?**
Not via config. But you can put a skill named `pr-review` and invoke it as `/pr-review` from the TUI — skills appear in the `/` autocomplete alongside built-in commands.

---

## See Also

- [CLI Commands](/reference/cli-commands/) — `edgecrab` subcommands (not TUI slash commands)
- [Configuration Reference](/reference/configuration/) — values editable with `/config`
- [Sessions](/user-guide/sessions/) — session persistence and the session DB
