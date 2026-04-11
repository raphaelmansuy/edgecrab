---
title: CLI Interface
description: Complete reference for the EdgeCrab command-line interface — every flag, subcommand, slash command, and keyboard shortcut. Grounded in cli_args.rs and commands.rs.
sidebar:
  order: 1
---

EdgeCrab is driven entirely from the terminal. This page covers every entry point: the interactive TUI, one-shot prompts, global flags, subcommands, and in-session slash commands.

Hermes-parity note: when a feature primarily lives as a slash command, use
`edgecrab slash <command...>` to reach the same handler from argv without
duplicating a separate clap tree.

All information here is sourced directly from `crates/edgecrab-cli/src/cli_args.rs` and `crates/edgecrab-cli/src/commands.rs`.

---

## Starting EdgeCrab

### Interactive TUI

```bash
edgecrab
```

Opens the full ratatui TUI with streaming tool output, message history, slash command autocomplete, and the status bar.

### One-Shot Prompt

```bash
edgecrab "summarise the git log for today"
```

Sends a single prompt. Streams the response. Exits when done.

### Quiet / Headless Mode

```bash
# Suppress banner and TUI — only LLM output goes to stdout
edgecrab --quiet "list all TODO comments in src/"

# Pipe-friendly
edgecrab -q "find all functions with no error handling" | grep -i "unwrap"
```

---

## Interface Layout

The TUI has three areas:

1. **Banner** — shows model, toolset, working directory, and active skills on startup
2. **Conversation stream** — scrollable output area with streaming tool execution feed
3. **Input prompt** — fixed at the bottom with slash-command autocomplete

### Status Bar

A persistent status bar sits above the input area, updating in real time:

```
 🦀 copilot/gpt-4.1-mini │ 8.4K/128K │ [████░░░░░░] 7% │ 12m
```

| Column | Description |
|--------|-------------|
| Model name | Current model (provider/model) |
| Token count | Context tokens used / max window |
| Context bar | Visual fill indicator |
| Duration | Elapsed session time |

Context color coding: green < 50%, yellow 50–80%, orange 80–95%, red ≥ 95%.

### Tool Execution Feed

As the agent works, each tool call appears in the conversation stream:

```
  ┊ 💻 terminal  ls -la  (0.3s)
  ┊ 🔍 web_search (1.2s)
  ┊ 📄 read_file  src/main.rs  (0.1s)
```

Use `/verbose` to cycle display modes: `off → new → all → verbose`.

---

## Global Flags

All flags defined in `CliArgs` in `cli_args.rs`. They work with every subcommand.

```
edgecrab [OPTIONS] [PROMPT]
```

| Flag | Short | Description |
|------|-------|-------------|
| `--model <provider/model>` | `-m` | Override LLM for this session |
| `--toolset <list>` | | Active toolsets (comma-separated alias or name) |
| `--session <id>` | `-s` | Resume a specific session by ID |
| `--continue [title]` | `-C` | Continue the most recent CLI session (optionally by title) |
| `--resume <id-or-title>` | `-r` | Resume a session by ID or title |
| `--quiet` | `-q` | Quiet mode — print result and exit, no TUI |
| `--config <path>` | `-c` | Config file path (default: `~/.edgecrab/config.yaml`) |
| `--debug` | | Enable debug logging |
| `--no-banner` | | Skip ASCII banner display |
| `--worktree` | `-w` | Run in an isolated git worktree |
| `--skill <name>` | `-S` | Preload a skill (may repeat or comma-separate) |
| `--profile <name>` | `-p` | Run under a specific named profile |

### `--model` Examples

```bash
edgecrab --model openai/gpt-4.1-mini "explain this PR"
edgecrab --model anthropic/claude-opus-4-5 "refactor this function"
edgecrab --model ollama/llama3.3 "offline mode"
edgecrab -m deepseek/deepseek-chat "fast, cost-effective"

# Full provider/model format: <provider>/<model-name>
edgecrab -m copilot/gpt-4.1-mini
edgecrab -m gemini/gemini-2.5-flash
```

### `--toolset` Examples

Available toolset aliases (from `toolsets.rs`):

| Alias | Tools included |
|-------|----------------|
| `coding` | file + terminal + web + meta |
| `all` | every tool |
| `file` | `read_file`, `write_file`, `patch`, `search_files` |
| `terminal` | `terminal`, `run_process`, `list_processes`, `kill_process` |
| `web` | `web_search`, `web_extract`, `web_crawl` |
| `browser` | all `browser_*` tools |
| `memory` | `memory_read`, `memory_write`, `honcho_*` |
| `skills` | `skills_list`, `skills_categories`, `skill_view`, `skill_manage`, `skills_hub` |
| `meta` | `manage_todo_list`, `clarify` |
| `scheduling` | `manage_cron_jobs` |
| `delegation` | `delegate_task` |
| `code_execution` | `execute_code` |
| `session` | `session_search` |
| `mcp` | `mcp_list_tools`, `mcp_call_tool`, `mcp_list_resources`, `mcp_read_resource`, `mcp_list_prompts`, `mcp_get_prompt` |

```bash
edgecrab --toolset file,terminal "run tests and fix errors"
edgecrab --toolset web "research recent Rust async changes"
edgecrab --toolset coding "full coding workflow"
edgecrab --toolset all "enable everything"
edgecrab --toolset browser,web "scrape and summarize"
```

### `--worktree` (`-w`)

Creates a temporary branch and worktree under `.worktrees/` in the current repository, then runs EdgeCrab from that isolated directory. This lets you run multiple agents in parallel on the same repo without conflicts:

```bash
# Terminal 1 — refactor auth
edgecrab -w "refactor authentication to use JWT"

# Terminal 2 — parallel feature work
edgecrab -w "add rate limiting to the API"
```

Each invocation gets its own worktree and branch automatically. Clean worktrees are removed on exit; dirty ones are kept for manual recovery.

### `--skill` (`-S`)

Preload one or more skills into the session-scoped preloaded-skill set before the first turn:

```bash
edgecrab -S code-review "review the PR"
edgecrab -S security,testing "audit the codebase"
edgecrab --skill security --skill testing "audit"
```

Skills are loaded from `~/.edgecrab/skills/<name>/SKILL.md`. See [Skills System](/features/skills/).

### `--continue` (`-C`) vs `--resume` (`-r`)

```bash
edgecrab -C                       # resume the most recent CLI session
edgecrab -C "auth refactor"       # resume latest session matching that title
edgecrab -r abc123                # resume by session ID
edgecrab --session abc123         # also by session ID
```

---

## Subcommands

All subcommands defined in `Command` enum in `cli_args.rs`:

```
edgecrab setup [section] [--force]  — interactive setup wizard
edgecrab doctor                     — diagnostics check
edgecrab migrate [--dry-run]        — hermes → edgecrab migration
edgecrab claw migrate [FLAGS...]    — OpenClaw → edgecrab migration
edgecrab acp                        — ACP stdio server for editors
edgecrab version                    — detailed version info
edgecrab whatsapp                   — pair and configure WhatsApp bridge
edgecrab status                     — high-level runtime status summary
edgecrab sessions <sub>             — session management
edgecrab config <sub>               — inspect or modify config.yaml
edgecrab tools <sub>                — inspect registered tools and toolsets
edgecrab mcp <sub>                  — manage MCP servers
edgecrab plugins <sub>              — manage installed plugins
edgecrab cron <sub>                 — manage scheduled prompts
edgecrab gateway <sub>              — run and manage messaging gateway
edgecrab skills <sub>               — manage agent skills
edgecrab profile <sub>              — manage named profiles
edgecrab completion <shell>         — generate shell completion script
```

### `setup`

```bash
edgecrab setup           # full interactive wizard
edgecrab setup model     # model & provider section only
edgecrab setup tools     # toolsets configuration
edgecrab setup gateway   # messaging platform wiring
edgecrab setup agent     # agent & behaviour settings
edgecrab setup --force   # overwrite existing config from scratch
```

### `doctor`

Checks config, env vars, API key presence, and provider connectivity:

```
EdgeCrab Doctor
────────────────────────────────────────────────────────────────
✓  Config file          /Users/you/.edgecrab/config.yaml
✓  State directory      /Users/you/.edgecrab/
✓  Memories directory   /Users/you/.edgecrab/memories/
✓  Skills directory     /Users/you/.edgecrab/skills/
✓  GitHub Copilot       GITHUB_TOKEN set
✓  OpenAI               OPENAI_API_KEY set
✗  Anthropic            ANTHROPIC_API_KEY not set
✓  Provider ping        copilot/gpt-4.1-mini → OK (312 ms)
────────────────────────────────────────────────────────────────
1 warning(s). Run `edgecrab setup` to configure missing providers.
```

### `migrate`

Import config, memories, skills, and env from a hermes-agent installation:

```bash
edgecrab migrate --dry-run   # preview what will be migrated
edgecrab migrate             # execute migration
edgecrab migrate --source /path/to/.hermes
```

See [Migration](/user-guide/migration/).

### `claw migrate`

Import the EdgeCrab-native subset of an OpenClaw home and archive the rest for
manual review:

```bash
edgecrab claw migrate
edgecrab claw migrate --dry-run
edgecrab claw migrate --preset user-data
edgecrab claw migrate --migrate-secrets
edgecrab claw migrate --workspace-target /absolute/workspace
edgecrab claw migrate --skill-conflict rename
```

Use `--preset full` to include allowlisted secrets. Unsupported advanced
OpenClaw-only settings are archived under `~/.edgecrab/migration/openclaw/`.

### `acp`

Starts the ACP (Agent Communication Protocol) JSON-RPC 2.0 server on stdin/stdout. Used by VS Code, Zed, JetBrains, and other ACP-compatible editors:

```bash
edgecrab acp
```

See [ACP Integration](/integrations/acp/).

### `sessions` subcommands

```bash
edgecrab sessions list [--limit N] [--source <platform>]
edgecrab sessions browse [--query <text>] [--limit N]
edgecrab sessions export <id> [--output <file>] [--format markdown|jsonl]
edgecrab sessions delete <id>
edgecrab sessions rename <id> <new title…>
edgecrab sessions prune [--older-than N] [--source <platform>] [--yes]
edgecrab sessions stats
```

### `config` subcommands

```bash
edgecrab config show          # print active config as YAML
edgecrab config edit          # open config.yaml in $EDITOR
edgecrab config set <key> <value>
edgecrab config path          # print config.yaml path
edgecrab config env-path      # print .env file path
```

### `tools` subcommands

```bash
edgecrab tools list             # list all registered tools and toolsets
edgecrab tools enable <name>    # enable a toolset in config
edgecrab tools disable <name>   # disable a toolset in config
```

### `mcp` subcommands

```bash
edgecrab mcp list                           # list configured MCP servers
edgecrab mcp add <name> <command> [args…]   # add or update a server
edgecrab mcp remove <name>                  # remove a server
```

### `cron` subcommands

```bash
edgecrab cron list [--all]
edgecrab cron status
edgecrab cron tick                              # run due jobs once and exit
edgecrab cron create "<schedule>" "<prompt>" [--name <n>] [--skill <s>] [--repeat N] [--deliver local|<platform>:<chat_id>]
edgecrab cron edit <id> [--schedule …] [--prompt …] [--name …] [--skill …]
edgecrab cron pause <id>
edgecrab cron resume <id>
edgecrab cron run <id>                          # run immediately
edgecrab cron remove <id>
```

Cron schedule examples: `"every 1 hour"`, `"0 9 * * mon-fri"`, `"every 30 minutes"`.

### `gateway` subcommands

```bash
edgecrab gateway start [--foreground]   # start gateway process
edgecrab gateway stop                   # stop gateway
edgecrab gateway restart
edgecrab gateway status                 # show runtime status
edgecrab gateway configure [platform]  # interactive wizard
```

Platforms: `telegram`, `discord`, `slack`, `signal`, `whatsapp`.

### `profile` subcommands

```bash
edgecrab profile list                   # list all profiles (* = active)
edgecrab profile use <name>             # set active profile
edgecrab profile create <name> [--clone]
edgecrab profile delete <name>
edgecrab profile show [name]            # show profile config
edgecrab profile path [name]            # print profile home directory
```

Profiles live at `~/.edgecrab/profiles/<name>/` with isolated `config.yaml`, `.env`, `SOUL.md`, `memories/`, `skills/`, and `state.db`. See [Profiles](/user-guide/profiles/).

### `skills` subcommands

```bash
edgecrab skills list [--category <cat>]
edgecrab skills view <name>
edgecrab skills install <path-or-url>
edgecrab skills remove <name>
```

### `completion`

```bash
edgecrab completion bash >> ~/.bashrc
edgecrab completion zsh  >> ~/.zshrc
```

---

## Session Management

### Resuming Sessions

When you exit a CLI session, a resume command is printed:

```
Resume this session with:
  edgecrab --resume 20260403_143052_a1b2c3

Session:   20260403_143052_a1b2c3
Duration:  12m 34s
Messages:  28 (5 user, 18 tool calls)
```

Resume options:

```bash
edgecrab --continue                      # resume the most recent CLI session
edgecrab -C                              # short form
edgecrab -C "auth refactor"              # resume named session (latest in lineage)
edgecrab --resume 20260403_143052_a1b2c3 # resume by session ID
edgecrab -r "auth refactor"             # resume by title
```

### Session Storage

All sessions are stored in `~/.edgecrab/state.db` (SQLite WAL). Each session records:
- Session metadata (ID, title, timestamps, token counters, source platform)
- Full message history (user + assistant + tool calls)
- Lineage across resumed/compressed sessions
- Full-text search indexes for `session_search` tool

---

## Background Sessions

Run a prompt in a separate background session while you continue using the CLI:

```
/background Analyze the logs in /var/log and summarize any errors from today
```

EdgeCrab immediately confirms the task:

```
🔄 Background task #1 started: "Analyze the logs in /var/log..."
   Task ID: bg_143022_a1b2c3
```

When the task finishes, the result appears as a panel:

```
╭─ 🦀 EdgeCrab (background #1) ──────────────────────────────────╮
│ Found 3 errors in syslog from today:                            │
│ 1. OOM killer invoked at 03:22 — killed nginx                   │
│ 2. Disk I/O error on /dev/sda1 at 07:15                         │
│ 3. Failed SSH login attempts from 192.168.1.50 at 14:30         │
╰─────────────────────────────────────────────────────────────────╯
```

Background sessions inherit your model, toolsets, and reasoning settings. They are fully isolated — no shared history with the foreground session.

---

## Slash Commands (Inside the TUI)

All 42 slash commands sourced from `CommandResult` enum in `commands.rs`. Type `/` in the input bar to see autocomplete.

Skills installed in `~/.edgecrab/skills/` are automatically registered as slash commands.

### Navigation & Session

| Command | Description |
|---------|-------------|
| `/help` | List all slash commands |
| `/quit` | Exit EdgeCrab |
| `/clear` | Clear the screen and start a fresh session |
| `/new` | Start fresh session (clears history) |
| `/status` | Show current session status |
| `/version` | Show version info |
| `/session <id-or-title>` | Load or create a session |
| `/retry` | Retry the last prompt |
| `/undo` | Undo last message pair |
| `/stop` | Stop current generation |
| `/history` | Show message history |
| `/save` | Save session to disk |
| `/export [format]` | Export as markdown or JSONL |
| `/title <text>` | Set session title |
| `/resume <id>` | Resume a saved session |

### Model & Intelligence

| Command | Description |
|---------|-------------|
| `/model <provider/model>` | Hot-swap LLM mid-session |
| `/provider <name>` | Switch provider |
| `/reasoning <off\|low\|medium\|high>` | Set reasoning effort |
| `/stream <on\|off>` | Toggle streaming mode |

### Configuration

| Command | Description |
|---------|-------------|
| `/config [key] [value]` | Show or set config values |
| `/prompt` | Show, clear, or set the custom system prompt override |
| `/verbose` | Cycle tool progress: off → new → all → verbose; `/verbose <mode>` sets directly |
| `/personality <name>` | Switch agent personality |
| `/statusbar <on\|off>` | Toggle the status bar |

### Tools & Memory

| Command | Description |
|---------|-------------|
| `/tools` | List all available tools |
| `/toolsets` | List toolset aliases and members |
| `/reload-mcp` | Hot-reload MCP servers without restart |
| `/plugins` | List installed plugins |
| `/memory` | Show all memory files |

### Analysis

| Command | Description |
|---------|-------------|
| `/cost` | Show token cost for this session |
| `/usage` | Cumulative API usage stats |
| `/compress` | Manually compress conversation history |
| `/insights [days]` | Show current-session insights plus N-day history (default: 30) |

### Advanced

| Command | Description |
|---------|-------------|
| `/queue <prompt>` | Queue a prompt to run after current finishes |
| `/background <prompt>` | Run prompt as isolated background session |
| `/rollback` | Roll back to last checkpoint (shadow git) |

### Appearance

| Command | Description |
|---------|-------------|
| `/skin` | Open the skin browser or reload `~/.edgecrab/skin.yaml` (`/theme` alias) |
| `/paste` | Enter multi-line paste mode |

### Gateway & Scheduling

| Command | Description |
|---------|-------------|
| `/platforms` | List connected messaging platforms |
| `/approve` | Approve pending gateway action |
| `/deny` | Deny pending gateway action |
| `/sethome` | Set gateway home channel |
| `/update` | Update EdgeCrab binary |
| `/cron` | Show scheduled jobs |
| `/voice` | Toggle voice I/O mode |
| `/doctor` | Run diagnostics inline |

---

## Keyboard Shortcuts

All shortcuts sourced from the `/help` text in `commands.rs`.

| Key | Action |
|-----|--------|
| `Enter` | Submit prompt |
| `Shift+Enter` | Insert newline (multi-line input) |
| `Ctrl+C` | Clear input / cancel generation / exit (on empty) |
| `Ctrl+D` | Exit (on empty input) |
| `Ctrl+L` | Clear screen |
| `Ctrl+U` | Clear input line |
| `Tab` | Accept autocomplete / slash command |
| `Up / Down` | Scroll through command history |
| `Right` | Accept ghost autocomplete hint |
| `PgUp / PgDn` | Scroll output buffer |
| `Shift+Up / Shift+Down` | Scroll output 3 rows |
| `Alt+Up / Alt+Down` | Scroll output 5 rows |
| `Ctrl+Home` | Jump to top of output |
| `Ctrl+End` / `Ctrl+G` | Jump to bottom (live view) |
| `Ctrl+M` | Toggle mouse capture / selection mode |

### Multi-line Input

Use `Shift+Enter` to add a newline inside the input bar:

```
❯ Write a function that:<Shift+Enter>
  1. Takes a list of numbers<Shift+Enter>
  2. Returns the sum
```

Alternatively, end a line with `\` (backslash continuation):

```
❯ First line \
  Second line
```

---

## Environment Variable Overrides

Key `EDGECRAB_*` env vars (from `config.rs: apply_env_overrides`):

| Variable | Description |
|----------|-------------|
| `EDGECRAB_MODEL` | Override default model |
| `EDGECRAB_MAX_ITERATIONS` | Override max agent iterations (default: 90) |
| `EDGECRAB_TIMEZONE` | Override timezone (IANA format) |
| `EDGECRAB_SAVE_TRAJECTORIES` | Enable trajectory logging (`1/true/yes`) |
| `EDGECRAB_SKIP_CONTEXT_FILES` | Skip auto-loading context files |
| `EDGECRAB_SKIP_MEMORY` | Disable all memory for this session |
| `EDGECRAB_GATEWAY_HOST` | Gateway bind host |
| `EDGECRAB_GATEWAY_PORT` | Gateway bind port |
| `EDGECRAB_TTS_PROVIDER` | TTS provider override |
| `EDGECRAB_TTS_VOICE` | TTS voice override |
| `EDGECRAB_REASONING_EFFORT` | Reasoning effort (`low`, `medium`, `high`, `xhigh`) |
| `EDGECRAB_HOME` | Override `~/.edgecrab` home directory |
| `EDGECRAB_MANAGED` | Set `1` to block config writes (managed deployments) |

See [Configuration Reference](/reference/configuration/) for all config keys.
