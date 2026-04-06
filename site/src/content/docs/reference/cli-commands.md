---
title: CLI Commands
description: Complete reference for all edgecrab CLI subcommands and global flags. Install via npm, pip, or cargo. Grounded in crates/edgecrab-cli/src/cli_args.rs.
sidebar:
  order: 1
---

All flags, subcommands, and arguments are sourced directly from
`crates/edgecrab-cli/src/cli_args.rs`. Run `edgecrab --help` or
`edgecrab <subcommand> --help` for live output at any time.

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
# EdgeCrab v0.1.0
# Rust 1.85.0
#
# Supported providers (via edgequake-llm):
#   copilot        — GitHub Copilot (GITHUB_TOKEN)
#   openai         — OpenAI (OPENAI_API_KEY)
#   anthropic      — Anthropic (ANTHROPIC_API_KEY)
#   gemini         — Google Gemini (GOOGLE_API_KEY)
#   openrouter     — OpenRouter (OPENROUTER_API_KEY)
#   xai            — xAI Grok (XAI_API_KEY)
#   mistral        — Mistral AI (MISTRAL_API_KEY)
#   ollama         — Ollama (local, no key)
#   lmstudio       — LMStudio (local, no key)
#   azure          — Azure OpenAI (AZURE_OPENAI_API_KEY)
#   bedrock        — AWS Bedrock (AWS_ACCESS_KEY_ID)
#   huggingface    — HuggingFace (HUGGINGFACE_API_KEY)
```

---

## Command Map

```
edgecrab [GLOBAL FLAGS] [PROMPT]   -- interactive TUI (default)
  |
  +-- setup [section] [--force]    -- first-run wizard
  +-- doctor                       -- environment diagnostics
  +-- migrate [--dry-run]          -- import from hermes-agent
  +-- acp [init]                   -- ACP stdio server / VS Code onboarding
  +-- version                      -- build info + provider list
  +-- status                       -- runtime status summary
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
  +-- completion <shell>           -- shell tab-completion script
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

**Agent-only flags** (only apply when running in interactive / one-shot mode):

| Flag | Short | Description |
|------|-------|-------------|
| `--worktree` | `-w` | Create an isolated git worktree for this session |
| `--skill <name>` | `-S` | Preload a skill (repeatable; comma-separated ok) |

---

## Running the Agent

```bash
edgecrab                                    # Interactive TUI
edgecrab "summarise the git log"            # One-shot with initial message
edgecrab -q "explain this codebase"         # Quiet/pipe mode
edgecrab -C                                 # Continue the last session
edgecrab -C "my project"                    # Continue session by title
edgecrab -r abc123 "add more tests"         # Resume session abc123
edgecrab -w "refactor auth module"          # New isolated worktree
edgecrab -S security-audit "audit payment"  # Preload a skill
edgecrab --model anthropic/claude-opus-4    # Override model
edgecrab --toolset coding "write tests"     # Use 'coding' toolset only
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
provider, and writes `~/.edgecrab/config.yaml`.

---

## `edgecrab doctor`

Full diagnostic health check — no flags required.

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
# EdgeCrab v0.1.0  (rustc 1.85.0, git a1b2c3d)
# Rust 1.85.0
# ...providers listed as above...

edgecrab --version   # identical output
```

---

## `edgecrab status`

High-level runtime status: active profile, model, gateway, and session
count.

```bash
edgecrab status
```

---

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
directory under `~/.edgecrab/profiles/<name>/` (config, memories,
skills, sessions).

```bash
edgecrab profile list                              # List all profiles
edgecrab profile create <name>                     # Create a new profile
edgecrab profile create <name> --clone             # Clone current profile (config, .env, SOUL.md)
edgecrab profile create <name> --clone-all         # Clone everything including memories/sessions
edgecrab profile create <name> --clone-from other  # Clone from a specific profile
edgecrab profile use <name>                        # Switch sticky default profile
edgecrab profile show <name>                       # Show profile details (dir, model, disk usage)
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
edgecrab mcp add <name> <cmd> [args...]   # Add a custom MCP server by command
edgecrab mcp remove <name>                # Remove a configured MCP server
```

---

## `edgecrab plugins`

Manage installed plugins.

```bash
edgecrab plugins list              # List discovered plugins
edgecrab plugins install <repo>    # Install a plugin from a git repository
edgecrab plugins update <name>     # Update an installed plugin
edgecrab plugins remove <name>     # Remove an installed plugin
```

---

## `edgecrab skills`

Manage agent skills stored in `~/.edgecrab/skills/`.

```bash
edgecrab skills list                              # List all installed skills
edgecrab skills view <name>                       # Print a skill's SKILL.md
edgecrab skills search <query>                    # Search skills by name
edgecrab skills install <path-or-repo>            # Install from a local path or GitHub URL
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
- **Worktrees for risky refactors**: `edgecrab -w "refactor auth module"` creates a git worktree so changes don't land on the current branch until you're ready.

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
