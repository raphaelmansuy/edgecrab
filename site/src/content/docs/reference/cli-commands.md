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
# EdgeCrab 0.1.0  (rustc 1.85.0, 2025-02-20)
#   providers: copilot openai anthropic gemini xai deepseek huggingface zai openrouter ollama lmstudio
```

---

## Command Map

```
edgecrab [GLOBAL FLAGS] [PROMPT]   -- interactive TUI (default)
  |
  +-- setup [section] [--force]    -- first-run wizard
  +-- doctor                       -- environment diagnostics
  +-- migrate [--dry-run]          -- import from hermes-agent
  +-- acp                          -- ACP stdio server (VS Code)
  +-- version                      -- build info + provider list
  +-- status                       -- runtime status summary
  +-- whatsapp                     -- pair WhatsApp bridge
  |
  +-- profile  <sub>               -- named profile management
  +-- sessions <sub>               -- session history
  +-- config   <sub>               -- config.yaml management
  +-- tools    <sub>               -- tool/toolset inspection
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

## `edgecrab acp`

Start an ACP JSON-RPC 2.0 stdio server for editor integration.

```bash
edgecrab acp
```

Reads requests from stdin, writes responses to stdout. Used by the VS
Code GitHub Copilot extension and any ACP-compatible runner. See
[ACP Integration](/integrations/acp/) for configuration and manifest
details.

---

## `edgecrab version`

Print build info and supported providers.

```bash
edgecrab version
# EdgeCrab 0.1.0  (rustc 1.85.0, 2025-02-20, git a1b2c3d)
#   providers: copilot openai anthropic gemini xai deepseek
#              huggingface zai openrouter ollama lmstudio

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
edgecrab profile list                        # List all profiles
edgecrab profile create <name>               # Create a new profile
edgecrab profile create <name> --clone       # Clone current profile
edgecrab profile use <name>                  # Switch sticky default profile
edgecrab profile show [name]                 # Show profile metadata
edgecrab profile path [name]                 # Print profile home directory
edgecrab profile delete <name>              # Delete a profile (requires confirm)
```

Running `edgecrab -p <name> "prompt"` overrides the sticky profile
for a single invocation without changing the default.

---

## `edgecrab sessions`

Manage conversation history stored in the SQLite state database.

```bash
edgecrab sessions list                       # List recent sessions (newest first)
edgecrab sessions show <id>                  # Show messages in a session
edgecrab sessions search <query>             # Full-text search via FTS5
edgecrab sessions delete <id>               # Delete a session
edgecrab sessions rename <id> <new-title>    # Rename a session
edgecrab sessions export <id> [format]      # Export: markdown, json, or text
edgecrab sessions prune --older-than 30     # Delete sessions older than N days
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
edgecrab config edit-soul                    # Open SOUL.md in $EDITOR
```

Key path examples: `model.default_model`, `tools.enabled_toolsets`,
`memory.auto_flush`, `display.show_cost`.

---

## `edgecrab tools`

Inspect registered tools and toolset composition. Useful for debugging
toolset configuration.

```bash
edgecrab tools list                          # List all registered tools
edgecrab tools show <name>                   # Show tool schema and description
edgecrab tools toolsets                      # List toolset aliases and expansions
```

---

## `edgecrab gateway`

Manage the messaging gateway daemon that connects EdgeCrab to external
messaging platforms.

```bash
edgecrab gateway start                       # Start gateway daemon (background)
edgecrab gateway start --foreground          # Start gateway in foreground (logs to stdout)
edgecrab gateway stop                        # Stop gateway daemon
edgecrab gateway status                      # Show daemon + per-platform status
edgecrab gateway logs                        # Follow live gateway logs
edgecrab gateway restart                     # Stop then start
edgecrab gateway configure                   # Interactive platform setup wizard
edgecrab gateway configure telegram          # Configure a specific platform
```

Platforms are enabled and configured via environment variables or `config.yaml` gateway section —
not via `gateway start` flags. See [User Guide → Messaging](/user-guide/messaging/) for per-platform
setup.

---

## `edgecrab migrate`

Re-listed here for clarity — see full entry above.

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
- **`edgecrab sessions search "my query"`**: FTS5 full-text search across all conversation history — faster than scrolling session lists.
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
