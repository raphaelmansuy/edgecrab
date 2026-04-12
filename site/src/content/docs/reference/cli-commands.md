---
title: CLI Commands
description: Complete reference for all edgecrab CLI subcommands and global flags. Install via npm, pip, or cargo. Grounded in crates/edgecrab-cli/src/cli_args.rs.
sidebar:
  order: 1
---

All flags, subcommands, and arguments are sourced directly from
`crates/edgecrab-cli/src/cli_args.rs`. Run `edgecrab --help` or
`edgecrab <subcommand> --help` for live output at any time.

Hermes-parity note: EdgeCrab now exposes Hermes-style entrypoints such as
`chat`, `model`, `auth`, `login`, `logout`, `webhook`, `insights`, `dump`,
`logs`, `pairing`, `memory`, `honcho`, `claw`, and `uninstall` directly on
the CLI, while still preserving EdgeCrab-native families like `plugins`,
`mcp`, and `profiles`. For slash-first flows, `edgecrab slash <command...>`
is the generic DRY bridge into the same TUI command registry used by `/help`.

---

## Installing the CLI

EdgeCrab can be installed via **npm**, **pip (PyPI)**, or **cargo** — pick whatever fits your environment.
No Rust toolchain is needed for the npm or pip methods.

### npm

```bash
# Global install — adds `edgecrab` to your PATH
npm install -g edgecrab-cli

# Use without a global install
npx edgecrab-cli setup
npx edgecrab-cli "summarise the git log for today"
```

### pip / PyPI

```bash
pip install edgecrab-cli

# Isolated install with pipx (recommended)
pipx install edgecrab-cli
```

### cargo (compile from source)

```bash
cargo install edgecrab-cli
```

### Verify Installation

```bash
edgecrab version
# EdgeCrab v<current-version>
# Rust 1.86.0
#
# Supported providers (from model catalog):
#   anthropic      — Anthropic (ANTHROPIC_API_KEY)
#   bedrock        — AWS Bedrock (AWS_ACCESS_KEY_ID)
#   copilot        — GitHub Copilot (GITHUB_TOKEN)
#   deepseek       — DeepSeek (Provider configured via model catalog/runtime integration)
#   google         — Google (Provider configured via model catalog/runtime integration)
#   groq           — Groq (Provider configured via model catalog/runtime integration)
#   huggingface    — Hugging Face (HUGGINGFACE_API_KEY)
#   lmstudio       — LM Studio (local) (local, no key)
#   mistral        — Mistral AI (MISTRAL_API_KEY)
#   ollama         — Ollama (local) (local, no key)
#   openai         — OpenAI (OPENAI_API_KEY)
#   openrouter     — OpenRouter (OPENROUTER_API_KEY)
#   vertexai       — Vertex AI (GOOGLE_CLOUD_PROJECT + ADC)
#   xai            — xAI (XAI_API_KEY)
#   zai            — Z.AI Platform (Provider configured via model catalog/runtime integration)
```

---

## Command Map

```
edgecrab [GLOBAL FLAGS] [PROMPT]   -- interactive TUI (default)
  |
  +-- chat [PROMPT...]             -- Hermes-compatible chat entrypoint
  +-- model                        -- open the interactive model picker
  +-- new                          -- interactive wrapper for /new
  +-- clear                        -- interactive wrapper for /clear
  +-- retry                        -- interactive wrapper for /retry
  +-- undo                         -- interactive wrapper for /undo
  +-- btw [QUESTION...]            -- interactive wrapper for /btw
  +-- provider                     -- interactive wrapper for /provider
  +-- prompt [ARGS...]             -- interactive wrapper for /prompt
  +-- personality [ARGS...]        -- interactive wrapper for /personality
  +-- reasoning [ARGS...]          -- interactive wrapper for /reasoning
  +-- yolo [ARGS...]               -- interactive wrapper for /yolo
  +-- verbose [ARGS...]            -- interactive wrapper for /verbose
  +-- statusbar [ARGS...]          -- interactive wrapper for /statusbar
  +-- voice [ARGS...]              -- interactive wrapper for /voice
  +-- browser [ARGS...]            -- interactive wrapper for /browser
  +-- reload-mcp                   -- interactive wrapper for /reload-mcp
  +-- slash <COMMAND...>           -- run any slash command through the TUI registry
  +-- insights [--days N]          -- historical usage analytics
  +-- setup [section] [--force]    -- first-run wizard
  +-- doctor                       -- environment diagnostics
  +-- migrate [--dry-run]          -- import from hermes-agent
  +-- claw migrate [FLAGS...]      -- import from OpenClaw
  +-- acp [init]                   -- ACP stdio server / VS Code onboarding
  +-- version                      -- build info + provider list
  +-- update [--check]             -- channel-aware update workflow
  +-- auth <sub>                   -- Copilot + MCP auth control plane
  +-- login <target>               -- interactive login/import shortcut
  +-- logout [target]              -- clear cached local auth state
  +-- status                       -- runtime status summary
  +-- dump [--all]                 -- shareable support snapshot
  +-- logs <sub>                   -- inspect log files
  +-- pairing <sub>                -- gateway pairing approvals
  +-- memory <sub>                 -- inspect MEMORY.md / USER.md
  +-- honcho <sub>                 -- Honcho-compatible user model
  +-- whatsapp                     -- pair WhatsApp bridge
  |
  +-- profile  <sub>               -- named profile management
  +-- sessions <sub>               -- session history
  +-- config   <sub>               -- config.yaml management
  +-- tools    <sub>               -- tool/toolset inspection
  +-- mcp      <sub>               -- MCP server management
  +-- plugins  <sub>               -- plugin management
  +-- skills   <sub>               -- skill management
  +-- cron     <sub>               -- scheduled prompts
  +-- gateway  <sub>               -- messaging gateway daemon
  +-- webhook  <sub>               -- dynamic webhook subscriptions
  +-- completion <shell>           -- shell tab-completion script
  +-- uninstall                    -- remove EdgeCrab-managed local artifacts
```

---

## Global Flags

These flags are accepted by `edgecrab` and by most subcommands (`global = true`
in clap):

| Flag | Short | Description |
|------|-------|-------------|
| `--model <provider/model>` | `-m` | Override default model, e.g. `openai/gpt-4o` |
| `--toolset <list>` | | Comma-separated toolset names or aliases |
| `--profile <name>` | `-p` | Run under a named profile (does not change sticky default) |
| `--session <id>` | `-s` | Resume a specific session by ID |
| `--continue [title]` | `-C` | Resume the most-recent CLI session (optionally by title) |
| `--resume <id-or-title>` | `-r` | Resume a session by ID or title with fuzzy resolution |
| `--quiet` | `-q` | Suppress TUI; print only final response (pipe-friendly) |
| `--config <path>` | `-c` | Use alternate config file instead of `~/.edgecrab/config.yaml` |
| `--debug` | | Enable debug logging (`RUST_LOG=debug`) |
| `--no-banner` | | Skip the startup ASCII art banner |
| `--yolo` | | Start the session with dangerous-command approvals bypassed |

**Agent-only flags** (only apply when running in interactive / one-shot mode):

| Flag | Short | Description |
|------|-------|-------------|
| `--worktree` | `-w` | Create an isolated git worktree for this session |
| `--skill <name>` | `-S` | Preload a skill (repeatable; comma-separated ok) |

---

## Running the Agent

```bash
edgecrab                                    # Interactive TUI
edgecrab chat                               # Same runtime, Hermes-style entrypoint
edgecrab "summarise the git log"            # One-shot with initial message
edgecrab -q "explain this codebase"         # Quiet/pipe mode
edgecrab -C                                 # Continue the last session
edgecrab -C "my project"                    # Continue session by title
edgecrab -r abc123 "add more tests"         # Resume session abc123
edgecrab -w "refactor auth module"          # New isolated worktree
edgecrab -S security-audit "audit payment"  # Preload a skill
edgecrab --model anthropic/claude-opus-4    # Override model
edgecrab --toolset coding "write tests"     # Use 'coding' toolset only
edgecrab --yolo "fix the build fast"        # Disable dangerous-command approval prompts
```

`edgecrab model` launches the normal TUI and opens the `/model` selector immediately.
It is intentionally not a separate model-management codepath.

`edgecrab slash ...` uses the same command parsing and handlers as the in-session
slash surface. Examples:

```bash
edgecrab slash insights 7
edgecrab slash btw "summarize this branch before I merge it"
edgecrab slash profile
```

High-frequency Hermes flows also have thin top-level wrappers that forward into
the same slash handlers:

```bash
edgecrab new
edgecrab btw "quick side question"
edgecrab prompt clear
edgecrab reasoning high
edgecrab reload-mcp
```

---

## `edgecrab setup`

Interactive first-run wizard. Re-run to reconfigure.

```bash
edgecrab setup                  # Full interactive wizard
edgecrab setup model            # Model and provider section only
edgecrab setup tools            # Toolsets configuration only
edgecrab setup gateway          # Messaging platforms only
edgecrab setup agent            # Agent personality and memory only
edgecrab setup --force          # Overwrite existing config from scratch
```

The wizard detects API keys from the environment, lets you choose a
provider, and writes `~/.edgecrab/config.yaml`. On a fresh install it also
detects OpenClaw homes and offers to import them before proceeding.

---

## `edgecrab doctor`

Full diagnostic health check — no flags required.

---

## `edgecrab update`

Check the latest release and apply the update through the install channel that
owns the current EdgeCrab binary.

```bash
edgecrab update          # check and apply when supported
edgecrab update --check  # report only
```

Channel behavior:

- npm: `npm install -g edgecrab-cli@<latest>`
- pipx: `pipx upgrade edgecrab-cli`
- pip: `python -m pip install --upgrade edgecrab-cli==<latest>`
- cargo: `cargo install edgecrab-cli --locked --force --version <latest>`
- brew: `brew update && brew upgrade edgecrab`
- source/manual binary: print safe manual guidance

```bash
edgecrab doctor
```

Checks:
- Config file existence and validity
- `EDGECRAB_HOME` directories (memories, skills, state db)
- Provider API key presence for each configured provider
- Live provider ping (latency test)
- MCP server reachability
- Chrome/Chromium binary for browser tools
- Gateway platform token presence
- SQLite database integrity (WAL checkpoint)
- Available disk space

---

## `edgecrab migrate`

One-way import from `~/.hermes/` (hermes-agent) into `~/.edgecrab/`.

```bash
edgecrab migrate            # Live migration
edgecrab migrate --dry-run  # Preview without writing any files
edgecrab migrate --source /path/to/.hermes
```

What is imported:

```
Source (~/.hermes/)          Destination (~/.edgecrab/)
------------------------------------------------------------
config.yaml             -->  config.yaml
memories/               -->  memories/
skills/                 -->  skills/
.env                    -->  .env
```

Safe to re-run — existing files are skipped or merged, never
silently overwritten.

---

## `edgecrab version`

Print build info and supported providers.

```bash
edgecrab version
# EdgeCrab v<current-version>
# Rust 1.86.0
# ...providers listed as above...

edgecrab --version
# edgecrab <current-version>
```

---

## `edgecrab status`

High-level runtime status: active profile, model, gateway, and session
count.

```bash
edgecrab status
```

---

## `edgecrab dump`

Print a support-friendly runtime snapshot: version, active profile, key paths,
provider model, gateway state, and optional local inventory counts.

```bash
edgecrab dump
edgecrab dump --all
```

---

## `edgecrab logs`

Inspect log files under `~/.edgecrab/logs/`.

```bash
edgecrab logs list
edgecrab logs path
edgecrab logs path gateway
edgecrab logs show gateway --lines 200
edgecrab logs tail gateway
```

---

## `edgecrab pairing`

Manage gateway pairing approvals and the approved-user list.

```bash
edgecrab pairing list
edgecrab pairing list --pending
edgecrab pairing approve <platform> <code>
edgecrab pairing revoke <platform> <user-id>
edgecrab pairing clear-pending --platform telegram
```

---

## `edgecrab memory`

Inspect or edit persistent memory files stored under `~/.edgecrab/memories/`.

```bash
edgecrab memory show
edgecrab memory show user
edgecrab memory edit memory
edgecrab memory path
```

---

## `edgecrab honcho`

Hermes-compatible Honcho control plane for EdgeCrab's local user-model store.
EdgeCrab keeps the local JSON store as the source of truth and can optionally
enable cloud-sync style behavior through config.

```bash
edgecrab honcho status
edgecrab honcho setup
edgecrab honcho setup --cloud-sync
edgecrab honcho mode
edgecrab honcho mode local
edgecrab honcho tokens --context 12 --write-frequency 4
edgecrab honcho list
edgecrab honcho search rust
edgecrab honcho add preference "prefers terse code review findings"
edgecrab honcho remove <id-prefix>
edgecrab honcho identity ./SOUL.md
edgecrab honcho path
```

---

## `edgecrab claw`

Hermes-compatible OpenClaw migration entrypoint. This imports the EdgeCrab-native
subset of an OpenClaw home directory and archives the unsupported remainder for
manual review under `~/.edgecrab/migration/openclaw/`.

```bash
edgecrab claw migrate
edgecrab claw migrate --dry-run
edgecrab claw migrate --preset user-data
edgecrab claw migrate --migrate-secrets
edgecrab claw migrate --overwrite
edgecrab claw migrate --source /path/to/.openclaw
edgecrab claw migrate --workspace-target /absolute/workspace
edgecrab claw migrate --skill-conflict rename
```

Key flags:

| Flag | Description |
|------|-------------|
| `--source <path>` | Override the detected OpenClaw directory (`~/.openclaw`, `~/.clawdbot`, `~/.moldbot`) |
| `--dry-run` | Preview only |
| `--preset {user-data,full}` | `user-data` excludes secrets; `full` includes allowlisted secrets |
| `--overwrite` | Replace conflicting target files instead of skipping |
| `--migrate-secrets` | Import allowlisted secrets even when not using `--preset full` |
| `--workspace-target <abs-path>` | Copy OpenClaw `AGENTS.md` into a workspace |
| `--skill-conflict {skip,overwrite,rename}` | Control how skill name conflicts are handled |

Imported directly:

- `SOUL.md`
- `MEMORY.md` / `USER.md`
- workspace and shared skills into `~/.edgecrab/skills/openclaw-imports/`
- `command_allowlist.json`
- messaging `.env` keys such as Telegram / Discord / Slack / Signal allowlists
- allowlisted provider secrets
- `model.default`, `tts.*`, `mcp_servers`, `terminal.timeout`, `timezone`, `reasoning_effort`

Archived for manual review:

- gateway/session/browser/approval/skills-registry/ui/logging config that does not map 1:1 to EdgeCrab
- supplemental workspace docs such as `IDENTITY.md`, `TOOLS.md`, `HEARTBEAT.md`, `BOOTSTRAP.md`

---

## `edgecrab insights`

Read historical analytics from the session database.

```bash
edgecrab insights
edgecrab insights --days 7
```

## `edgecrab whatsapp`

Pair and configure the WhatsApp bridge interactively.

```bash
edgecrab whatsapp
```

---

## `edgecrab acp`

Run the ACP stdio server for editor integration, or generate workspace-local
VS Code onboarding files.

```bash
edgecrab acp
edgecrab acp init
edgecrab acp init --workspace /path/to/repo
edgecrab acp init --force
```

`edgecrab acp init` creates `.edgecrab/acp_registry/agent.json` and
`.vscode/settings.json` in the target workspace. This removes the manual
`registryDir` setup that Hermes still requires.

---

## `edgecrab completion`

Generate shell tab-completion scripts. Output the script and source it
in your shell's init file.

```bash
edgecrab completion bash >> ~/.bashrc
edgecrab completion zsh  >> ~/.zshrc
```

---

## `edgecrab profile`

Manage named profiles. Each profile gets its own isolated home
directory under `~/.edgecrab/profiles/<name>/` with its own config,
SOUL, memory, skills, plugins, hooks, MCP tokens, and SQLite session
store. EdgeCrab seeds bundled starter profiles (`work`, `research`,
`homelab`) automatically on normal startup and profile commands.

```bash
edgecrab profile list                              # List all profiles
edgecrab profile create <name>                     # Create a new profile
edgecrab profile create <name> --clone             # Clone current profile (config, .env, SOUL.md)
edgecrab profile create <name> --clone-all         # Clone everything including memories/sessions
edgecrab profile create <name> --clone-from other  # Clone from a specific profile
edgecrab profile use <name>                        # Switch sticky default profile
edgecrab profile show [name]                       # Show a named profile, or print the active profile + home if omitted
edgecrab profile alias <name>                      # Generate a shell wrapper alias
edgecrab profile alias <name> --name myalias       # Alias with a custom name
edgecrab profile alias <name> --remove             # Remove the shell alias
edgecrab profile rename <old> <new>                # Rename a profile
edgecrab profile export <name>                     # Export profile as tar.gz archive
edgecrab profile export <name> -o ./backup.tar.gz  # Export to a specific path
edgecrab profile import ./backup.tar.gz            # Import a profile archive
edgecrab profile delete <name>                     # Delete a profile (requires confirm or -y)
```

Running `edgecrab -p <name> "prompt"` overrides the sticky profile
for a single invocation without changing the default.

---

## `edgecrab sessions`

Manage conversation history stored in the SQLite state database.

```bash
edgecrab sessions list                            # List recent sessions (newest first)
edgecrab sessions browse                          # Browse sessions interactively
edgecrab sessions browse --query <term>           # Full-text search via FTS5
edgecrab sessions delete <id>                     # Delete a session
edgecrab sessions rename <id> <new-title>         # Rename a session
edgecrab sessions export <id> [--format jsonl]    # Export: markdown (default) or jsonl
edgecrab sessions prune --older-than 30           # Delete sessions older than N days
edgecrab sessions stats                           # Show session statistics (counts, DB size)
```

---

## `edgecrab config`

Inspect and modify `~/.edgecrab/config.yaml` without opening a text
editor.

```bash
edgecrab config show                         # Print active config as YAML
edgecrab config edit                         # Open in $EDITOR
edgecrab config set <key> <value>            # Set a config key (dotted path)
edgecrab config path                         # Print path to config.yaml
edgecrab config env-path                     # Print path to .env
```

Key path examples: `model.default_model`, `tools.enabled_toolsets`,
`memory.auto_flush`, `display.show_cost`.

---

## `edgecrab tools`

Inspect registered tools and toolset composition. Useful for debugging
toolset configuration.

```bash
edgecrab tools list                          # List all registered tools and toolsets
edgecrab tools enable <toolset>              # Enable a toolset in config.yaml
edgecrab tools disable <toolset>             # Disable a toolset in config.yaml
```

---

## `edgecrab mcp`

Manage external MCP (Model Context Protocol) servers. Includes a curated preset catalogue for one-command server setup.

```bash
edgecrab mcp list                         # List configured MCP servers
edgecrab mcp search                       # Browse the curated MCP preset catalogue
edgecrab mcp search github                # Search presets matching "github"
edgecrab mcp view <preset>                # Show details for a curated preset
edgecrab mcp install <preset>             # Install a preset into config.yaml
edgecrab mcp install filesystem --path /tmp  # Install with path override
edgecrab mcp test                         # Probe all configured servers (connectivity + tool count)
edgecrab mcp test <name>                  # Probe a specific server
edgecrab mcp doctor                       # Static checks + live probe for all configured servers
edgecrab mcp doctor <name>                # Diagnose one configured server
edgecrab mcp add <name> <cmd> [args...]   # Add a custom MCP server by command
edgecrab mcp remove <name>                # Remove a configured MCP server
```

HTTP MCP servers can authenticate with bearer tokens from:

- `bearer_token` in `config.yaml`
- `/mcp-token set <server> <token>`
- env-expanded config values such as `bearer_token: "${MY_API_TOKEN}"`

---

## `edgecrab auth`

Manage the authentication state EdgeCrab actually owns today:

- GitHub Copilot token import and local cache
- env-backed provider API keys stored in `~/.edgecrab/.env`
- structured provider state stored in `~/.edgecrab/auth.json`
- MCP bearer-token and OAuth token cache state

This is not Hermes' general multi-provider credential-pool subsystem. EdgeCrab
does not yet ship pooled provider rotation, per-provider cooldown reset, or
provider-wide OAuth/API-key inventory management.

```bash
edgecrab auth list                            # List Copilot, provider, and MCP auth targets
edgecrab auth status                          # Same as list, concise overview
edgecrab auth status copilot                 # Detailed Copilot cache state
edgecrab auth status provider/openai         # Show provider token state in ~/.edgecrab/.env and ~/.edgecrab/auth.json
edgecrab auth status mcp/github              # Detailed MCP auth path for one server
edgecrab auth add copilot --token <gh-token> # Save a GitHub token for Copilot
edgecrab auth add provider/openai --token <tok> # Save one provider token to ~/.edgecrab/.env and record provider metadata in ~/.edgecrab/auth.json
edgecrab auth add mcp/github --token <tok>   # Save a bearer token for one MCP server
edgecrab auth login copilot                  # Import VS Code Copilot token and warm cache
edgecrab auth login mcp/github               # Run interactive OAuth login for one MCP server
edgecrab auth remove provider/openai         # Remove one provider token from ~/.edgecrab/.env and clear its auth.json entry
edgecrab auth remove copilot                 # Clear EdgeCrab's local Copilot token cache
edgecrab auth remove mcp/github              # Remove one cached MCP token
edgecrab auth reset                          # Clear all EdgeCrab-managed local auth caches
```

Target syntax:

- `copilot`
- `provider/<name>` for env-backed providers such as `openai`, `anthropic`, `gemini`, `openrouter`, `xai`, `deepseek`, `mistral`, `groq`, `cohere`, `perplexity`, `huggingface`, and `zai`
- `mcp/<server>`
- `<server>` for a configured MCP server name

---

## `edgecrab login` and `edgecrab logout`

Hermes-style shortcuts over `edgecrab auth`.

```bash
edgecrab login copilot        # Equivalent to: edgecrab auth login copilot
edgecrab login provider/openai  # Provider targets are env-backed and instruct you to use auth add
edgecrab login mcp/github     # Equivalent to: edgecrab auth login mcp/github
edgecrab logout               # Clear all local Copilot + provider + MCP auth caches
edgecrab logout copilot       # Clear only Copilot cache
edgecrab logout provider/openai # Clear one provider token from ~/.edgecrab/.env and its auth.json entry
edgecrab logout mcp/github    # Clear one cached MCP token
```

---

## `edgecrab webhook`

Manage dynamic gateway webhook subscriptions stored in
`~/.edgecrab/webhook_subscriptions.json`.

```bash
edgecrab webhook list
edgecrab webhook subscribe github --events push,pull_request --prompt "Summarise the repo event"
edgecrab webhook subscribe github --skill code-review --deliver github_comment --deliver-extra repo=org/repo --deliver-extra pr_number=42
edgecrab webhook subscribe alerts --deliver telegram --deliver-extra chat_id=12345 --deliver-extra thread_id=17
edgecrab webhook subscribe github --secret _INSECURE_NO_AUTH --rate-limit 60 --max-body-bytes 2097152
edgecrab webhook remove github
edgecrab webhook test github
edgecrab webhook path
```

Behavior:

- The gateway exposes `POST /webhooks/<name>` for saved subscriptions.
- Requests are authenticated with `X-Hub-Signature-256: sha256=...`, a raw 64-char hex HMAC, or `X-Webhook-Secret`.
- `--secret _INSECURE_NO_AUTH` disables secret checking for a route. This is compatibility-focused and should only be used on trusted internal networks.
- Event filters use `X-Event-Type`, `X-GitHub-Event`, or `payload.event_type`.
- Duplicate deliveries are rejected when a stable delivery ID is present.
- Per-route rate limits and maximum body sizes are enforced at ingress.
- Prompt templates support dot-path placeholders and `{__raw__}` for the full JSON payload.
- `--skill` preloads named skills into the webhook session before the turn runs.
- `--deliver` supports Hermes-style final-response routing such as `log`, `origin`, `telegram`, `discord`, `slack`, `signal`, and `github_comment`.
- `--deliver-extra key=value` is template-rendered against the JSON payload before delivery, so values like `repo={repository.full_name}` and `pr_number={pull_request.number}` work the same way Hermes operators expect.
- Matching requests are converted into agent messages and queued into the running gateway, with delivery metadata attached for the final response path.

---

## `edgecrab plugins`

Manage installed plugins.

```bash
edgecrab plugins list              # List discovered plugins
edgecrab plugins info <name>       # Show one plugin in detail
edgecrab plugins status            # Show plugin runtime state
edgecrab plugins install <source>  # Install from GitHub, hub:, https://zip, or a local directory
edgecrab plugins enable <name>     # Enable without reinstalling
edgecrab plugins disable <name>    # Disable without uninstalling
edgecrab plugins toggle [<name>]   # Flip enabled/disabled state or print TUI guidance
edgecrab plugins audit --lines 20  # Show recent install/remove audit entries
edgecrab plugins search <query>    # Search remote plugin registries
edgecrab plugins search --source hermes <query> # Search Hermes-oriented registries only
edgecrab plugins browse            # List plugin search sources and examples
edgecrab plugins refresh           # Clear cached plugin hub indices
edgecrab plugins update [name]     # Update one plugin or all git-backed plugins
edgecrab plugins remove <name>     # Remove an installed plugin
```

Supported kinds:

- `skill` plugins inject `SKILL.md` into the system prompt
- `tool-server` plugins expose subprocess tools over JSON-RPC
- `script` plugins expose Rhai-powered local tools

---

## `edgecrab skills`

Manage agent skills stored in `~/.edgecrab/skills/`.

```bash
edgecrab skills list                              # List all installed skills
edgecrab skills view <name>                       # Print a skill's SKILL.md
edgecrab skills search <query>                    # Search local + remote skill sources
edgecrab skills install <path>                    # Install from a local path
edgecrab skills install edgecrab:<category/path>  # Install from a curated remote source
edgecrab skills install owner/repo/path           # Install from GitHub
edgecrab skills update [name]                     # Refresh one or all remote-installed skills
edgecrab skills install official/<cat>/<skill>    # Install from the official catalogue
edgecrab skills remove <name>                     # Remove an installed skill
```

---

## `edgecrab cron`

Manage scheduled prompts.

```bash
edgecrab cron list                              # List scheduled jobs
edgecrab cron status                            # Show scheduler status
edgecrab cron create "0 9 * * *" "daily brief" # Create a cron job (cron expr + prompt)
edgecrab cron create --name daily "0 9 * * *" "brief" --skill reporter
edgecrab cron edit <id> --schedule "0 8 * * *" # Edit a scheduled job
edgecrab cron run <id>                          # Run a job immediately
edgecrab cron pause <id>                        # Pause a job
edgecrab cron resume <id>                       # Resume a paused job
edgecrab cron remove <id>                       # Delete a scheduled job
edgecrab cron tick                              # Fire all due jobs once and exit
```

---

## `edgecrab gateway`

Manage the messaging gateway daemon that connects EdgeCrab to external
messaging platforms.

```bash
edgecrab gateway start                       # Start gateway daemon (background)
edgecrab gateway start --foreground          # Start gateway in foreground (logs to stdout)
edgecrab gateway stop                        # Stop gateway daemon
edgecrab gateway restart                     # Stop then start
edgecrab gateway status                      # Show daemon + per-platform status
edgecrab gateway configure                   # Interactive platform setup wizard
edgecrab gateway configure telegram          # Configure a specific platform
```

Platforms are enabled and configured via environment variables or `config.yaml` gateway section —
not via `gateway start` flags. See [User Guide → Messaging](/user-guide/messaging/) for per-platform
setup.

---

## `edgecrab uninstall`

Remove EdgeCrab-managed local artifacts safely.

```bash
edgecrab uninstall --dry-run
edgecrab uninstall --purge-data --yes
edgecrab uninstall --purge-data --purge-auth-cache --remove-binary --yes
```

Safe defaults:

- stops the local background gateway if it is running
- removes profile wrapper scripts created in `~/.local/bin/`
- only removes `~/.edgecrab/` when `--purge-data` is passed
- only removes the local Copilot cache when `--purge-auth-cache` is passed
- only removes the current `edgecrab` binary when `--remove-binary` is passed

Unlike Hermes, EdgeCrab does not blindly delete a source checkout or guess at
shell `PATH` edits during uninstall.

---

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | General error (configuration, provider error) |
| `2` | Usage error (bad arguments) |
| `130` | Interrupted by Ctrl-C |

---

## Pro Tips

- **`edgecrab -q "prompt" | your-tool`**: The `--quiet` flag suppresses the TUI and prints only the final response. Combine with pipes for scripting and CI.
- **`edgecrab --debug 2>&1 | grep edgecrab_core`**: Filter debug logs to the agent loop only, cutting out the noise from other crates.
- **`edgecrab sessions browse --query "my query"`**: FTS5 full-text search across all conversation history — faster than scrolling session lists.
- **`edgecrab config set key value`** avoids opening the YAML editor for single-value changes.
- **`edgecrab completion zsh >> ~/.zshrc`** adds tab-completion for all subcommands and flags.
- **Worktrees for risky refactors**: `edgecrab -w "refactor auth module"` creates a repo-local `.worktrees/...` checkout so changes don't land on the current branch until you're ready. Set `worktree: true` or `EDGECRAB_WORKTREE=1` to make that the default for future launches.

---

## FAQ

**What's the difference between `--quiet` and piping the output?**
`--quiet` disables the TUI entirely and streams the final text response to stdout. Without it, the TUI overrides terminal control and can't be piped reliably.

**Can I resume a specific session from the command line?**
Yes: `edgecrab --session <id>` opens a specific session. Use `edgecrab sessions list` to find session IDs, or `edgecrab --continue` to resume the most recent.

**How do I run the agent in a cron job?**
Use `--quiet` mode so there's no TUI, and redirect stderr to a log file:
```bash
edgecrab -q "summarise new emails and append to daily-report.md" >> /var/log/edgecrab-cron.log 2>&1
```

**Does `edgecrab doctor` fix problems it finds?**
No — it diagnoses and reports. To fix, follow the suggested remediation steps it prints.

**What does `--no-banner` do?**
Suppresses the ASCII art banner on startup. Useful in narrow terminals or environments where the banner garbles output.

---

## See Also

- [Slash Commands](/reference/slash-commands/) — TUI commands (different from CLI subcommands)
- [Configuration Reference](/reference/configuration/) — `config.yaml` and `edgecrab config set`
- [Environment Variables](/reference/environment-variables/) — env vars that modify CLI behaviour
