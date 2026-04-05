#!/usr/bin/env python3
"""Write docs batch 1: tui.md, skills.md, overview.md"""
import os

BASE = "src/content/docs"

# ─── features/tui.md ──────────────────────────────────────────────────
tui = r"""---
title: TUI Interface
description: Full guide to the EdgeCrab ratatui terminal interface — layout, status bar, keyboard shortcuts, all 42 slash commands, themes, and accessibility. Grounded in crates/edgecrab-cli/src/commands.rs.
sidebar:
  order: 3
---

EdgeCrab's TUI is built with [ratatui](https://ratatui.rs) — a Rust-native TUI framework with double-buffered rendering. It features streaming tool output, slash-command autocomplete, and a live status bar.

---

## Interface Layout

```
┌─────────────────────────────────────────────────────────────────┐
│ 🦀 EdgeCrab  ·  anthropic/claude-sonnet-4  ·  ~/my-project      │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  You: explain the ReAct loop                                     │
│                                                                  │
│  EdgeCrab: The ReAct loop alternates between Reasoning           │
│  and Acting — the agent first thinks about which tool to         │
│  call, calls it, then observes the result...                     │
│                                                                  │
│  ┊ 📄 read_file  src/agent/loop.rs  (0.1s)                       │
│  ┊ 💻 terminal  cargo test  (2.4s)                               │
│                                                                  │
├─────────────────────────────────────────────────────────────────┤
│  🦀 claude-sonnet-4 │ 8.4K/200K │ [████░░░░░░] 4% │ $0.02 │ 5m │
├─────────────────────────────────────────────────────────────────┤
│  ❯ _                                                             │
└─────────────────────────────────────────────────────────────────┘
```

Three fixed areas:

1. **Header** — agent brand + active model + working directory
2. **Conversation stream** — scrollable output with streaming tool feed
3. **Status bar + input prompt** — fixed at the bottom

---

## Status Bar

The status bar sits above the input area, updating in real time:

```
🦀 claude-sonnet-4 │ 8.4K/200K │ [████░░░░░░] 4% │ $0.02 │ 5m
```

| Column | Description |
|--------|-------------|
| Model name | Current provider/model |
| Token count | Context tokens used / model max window |
| Context bar | Visual fill indicator with color coding |
| Cost | Estimated session cost (`show_cost = true`) |
| Duration | Elapsed session time |

**Context bar color coding:**

| Color | Range | Meaning |
|-------|-------|---------|
| Green | < 50% | Plenty of room |
| Yellow | 50–80% | Getting full |
| Orange | 80–95% | Approaching limit |
| Red | ≥ 95% | Near overflow — consider `/compress` |

Toggle the status bar with `/statusbar on|off`. Show/hide cost with `display.show_cost` in `config.yaml`.

---

## Tool Execution Feed

Live feedback as the agent works:

```
◐ (｡•́︿•̀｡) pondering...  (1.2s)
✧٩(ˊᗜˋ*)و✧ got it! (2.8s)

┊ 📄 read_file  src/main.rs  (0.1s)
┊ 💻 terminal  cargo test  (2.4s)
┊ 🔍 web_search  "ratatui events"  (1.1s)
```

Cycle through verbosity modes with `/verbose`:

| Mode | What you see |
|------|-------------|
| `off` | Final response only |
| `new` | One indicator per new tool type |
| `all` | Every tool call with preview (default) |
| `verbose` | Full arguments, results, and debug logs |

---

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Submit message |
| `Alt+Enter` or `Ctrl+J` | Insert newline (multi-line) |
| `Ctrl+C` | Interrupt agent (double-press within 2s to force exit) |
| `Ctrl+D` | Exit |
| `↑ / ↓` | Scroll prompt history |
| `PgUp / PgDn` | Scroll output buffer |
| `Tab` | Accept autocomplete suggestion |
| `Ctrl+B` | Toggle voice recording (push-to-talk) |

### Multi-line Input

Two ways to enter multi-line messages:

1. `Alt+Enter` or `Ctrl+J` — insert a newline
2. Backslash continuation — end a line with `\` to continue:

```
❯ Write a Rust function that:\
  1. Parses a TOML file\
  2. Validates required fields\
  3. Returns a typed config struct
```

### Interrupting the Agent

- Type a new message while the agent is working → interrupts and switches to your new prompt
- `Ctrl+C` → interrupt current operation (double-press to force exit)
- In-progress terminal commands are sent `SIGTERM`, then `SIGKILL` after 1s

---

## All 42 Slash Commands

Type `/` in the input bar to open the autocomplete dropdown. All commands are sourced from `CommandResult` enum in `crates/edgecrab-cli/src/commands.rs`. Skills installed in `~/.edgecrab/skills/` are also registered as slash commands automatically.

Commands are case-insensitive — `/HELP` works the same as `/help`.

### Navigation & Session

| Command | Description |
|---------|-------------|
| `/help` | Show all slash commands with descriptions |
| `/quit` | Exit EdgeCrab gracefully |
| `/clear` | Clear the visible output buffer |
| `/new` | Start a fresh session (clears conversation history) |
| `/status` | Show current session status and configuration |
| `/version` | Show EdgeCrab version information |

### Session Management

| Command | Description |
|---------|-------------|
| `/session <id-or-title>` | Load or switch to a session |
| `/retry` | Retry the last user message |
| `/undo` | Remove the last message pair from history |
| `/stop` | Abort the current generation immediately |
| `/history` | Display the current conversation history |
| `/save` | Save the current session to the database |
| `/export [format]` | Export session as `markdown` or `jsonl` |
| `/title <text>` | Set or rename the current session title |
| `/resume <id>` | Resume a previously saved session |

### Model & Intelligence

| Command | Description |
|---------|-------------|
| `/model <provider/model>` | Hot-swap the LLM mid-session |
| `/provider <name>` | Switch to a different provider |
| `/reasoning <level>` | Set reasoning effort: `off`, `low`, `medium`, `high` |
| `/stream <on\|off>` | Toggle streaming token output |

### Configuration

| Command | Description |
|---------|-------------|
| `/config [key] [value]` | Read or set configuration values live |
| `/prompt` | Show the full system prompt for this session |
| `/verbose` | Cycle tool verbosity: `off → new → all → verbose` |
| `/personality <name>` | Switch personality preset mid-session |
| `/statusbar <on\|off>` | Toggle the status bar visibility |

Built-in personalities: `helpful`, `concise`, `technical`, `kawaii`, `pirate`, `philosopher`, `hype`, `shakespeare`, `noir`, `catgirl`, `creative`, `teacher`, `surfer`, `uwu`.

### Tools & Plugins

| Command | Description |
|---------|-------------|
| `/tools` | List all registered tools and their status |
| `/toolsets` | List toolset aliases and their member tools |
| `/reload-mcp` | Hot-reload MCP servers without restarting |
| `/plugins` | List installed plugins |

### Memory

| Command | Description |
|---------|-------------|
| `/memory` | Show all persistent memory files |

### Analysis & Cost

| Command | Description |
|---------|-------------|
| `/cost` | Show token cost breakdown for this session |
| `/usage` | Cumulative API usage statistics |
| `/compress` | Manually trigger conversation compression |
| `/insights` | Show AI-generated session insights summary |

### Advanced Workflow

| Command | Description |
|---------|-------------|
| `/queue <prompt>` | Queue a prompt to send after the current turn finishes |
| `/background <prompt>` | Run a prompt as an isolated background session |
| `/rollback` | Roll back to the last checkpoint (shadow git) |

### Appearance

| Command | Description |
|---------|-------------|
| `/theme` | Reload skin/theme from `~/.edgecrab/skin.yaml` |
| `/paste` | Enter multi-line paste mode |

### Gateway & Automation

| Command | Description |
|---------|-------------|
| `/platforms` | List connected messaging platforms and status |
| `/approve` | Approve a pending gateway action |
| `/deny` | Deny a pending gateway action |
| `/sethome` | Set the current channel as the home channel |
| `/update` | Update EdgeCrab to the latest binary release |
| `/cron` | Show scheduled cron jobs |
| `/voice` | Toggle voice I/O mode (`/voice on`, `/voice tts`) |
| `/doctor` | Run EdgeCrab diagnostics inline |

---

## Background Sessions

Run a task concurrently while continuing to use the CLI:

```
/background Analyze all Python files in this repo for security issues
```

EdgeCrab confirms immediately:

```
🔄 Background task #1 started: "Analyze all Python files..."
   Task ID: bg_143022_a1b2c3
```

When finished, the result appears as a panel in your session:

```
╭─ 🦀 EdgeCrab (background #1) ──────────────────────────────────╮
│ Found 3 potential issues:                                        │
│ 1. SQL injection risk in db.py line 42                          │
│ 2. Hardcoded secret in config.py line 8                         │
│ 3. Unvalidated file path in upload.py line 91                   │
╰──────────────────────────────────────────────────────────────────╯
```

Background sessions inherit your model, toolsets, and reasoning settings but have no knowledge of your foreground session's history.

---

## Theming

EdgeCrab uses a `skin.yaml` file for visual customization. The skin file lives at `~/.edgecrab/skin.yaml`.

### SkinConfig Fields (from `theme.rs`)

| Field | Default | Description |
|-------|---------|-------------|
| `prompt_color` | `"cyan"` | Color of the input prompt |
| `assistant_color` | `"green"` | Color of assistant responses |
| `tool_color` | `"yellow"` | Color of tool output lines |
| `error_color` | `"red"` | Color for error messages |
| `system_color` | `"dim"` | Color for system messages |
| `prompt_symbol` | `"❯ "` | Input prompt symbol |
| `tool_prefix` | `"┊ "` | Prefix for tool execution lines |
| `agent_name` | `"EdgeCrab"` | Name displayed in the UI |
| `welcome_msg` | (auto) | Custom welcome message |
| `goodbye_msg` | (auto) | Custom goodbye message |
| `thinking_verbs` | (list) | Words used in thinking animation |
| `kaomoji_thinking` | (list) | Kaomoji for thinking state |
| `kaomoji_success` | (list) | Kaomoji for success state |
| `spinner_wings` | (list) | Characters for spinner animation |

Example `skin.yaml`:

```yaml
agent_name: "MyAgent"
prompt_symbol: "→ "
tool_prefix: "  ▸ "
assistant_color: "bright_blue"
welcome_msg: "Hello! Ready to work."
goodbye_msg: "See you next time!"
thinking_verbs:
  - "thinking"
  - "processing"
  - "reasoning"
```

Reload the theme without restarting:

```
/theme
```

---

## Accessibility

- **Compact mode**: `display.compact = true` reduces whitespace between messages
- **No banner**: `--no-banner` skips the startup ASCII art
- **Quiet mode**: `--quiet` / `-q` suppresses TUI and streams only the final answer to stdout
- **Color schemes**: Set `display.skin` to override the default color scheme
- **Font compatibility**: kaomoji (｡•́︿•̀｡) require a font with good Unicode coverage; use `--no-banner` on minimal terminals
"""

with open(f"{BASE}/features/tui.md", "w") as f:
    f.write(tui)
print("tui.md written")

# ─── features/skills.md ───────────────────────────────────────────────
skills = r"""---
title: Skills System
description: Agent-created procedural memory — how EdgeCrab discovers, loads, and installs skills from directories containing SKILL.md. Grounded in crates/edgecrab-tools/src/tools/skills.rs.
sidebar:
  order: 4
---

Skills are **portable, reusable procedural instructions** that EdgeCrab loads into its system prompt at session start. Think of them as "how-to guides" the agent can follow: a `security-audit` skill might walk it through OWASP Top 10 checks; a `deploy-k8s` skill might outline a safe Kubernetes rollout sequence.

Skills are compatible with [agentskills.io](https://agentskills.io) — the shared open-source skills registry used by all Nous Research agents.

---

## Directory Structure

Each skill is a **directory** containing a `SKILL.md` file — not a flat `.md` file:

```
~/.edgecrab/skills/
├── rust-test-fixer/
│   └── SKILL.md
├── security-audit/
│   └── SKILL.md
│   └── checklist.md        # optional extra context file
└── deploy-k8s/
    └── SKILL.md
    └── examples/
        └── deployment.yaml
```

EdgeCrab resolves skills in this order:

1. `~/.edgecrab/skills/` — primary user skills (highest priority)
2. Directories in `skills.external_dirs` in `config.yaml`
3. Skills bundled with the binary (read-only)

Local user skills always win when a name conflicts with external or bundled skills.

---

## SKILL.md Format

`SKILL.md` uses a YAML frontmatter block followed by Markdown instructions:

```markdown
---
name: security-audit
description: Systematic OWASP Top 10 security audit for web applications.
category: security
platforms:
  - cli
  - telegram
read_files:
  - checklist.md
---

# Security Audit Workflow

You are performing a security audit. Follow these steps:

1. Check authentication mechanisms for common weaknesses...
2. Test for SQL injection entry points...
3. Review session management...
```

### Frontmatter Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | `string` | yes | Unique identifier (becomes a slash command) |
| `description` | `string` | yes | Short summary shown in `/skills` listing |
| `category` | `string` | no | Hub category (e.g. `security`, `devops`, `coding`) |
| `platforms` | `string[]` | no | Limit to platforms (e.g. `["cli", "telegram"]`). Omit = all. |
| `read_files` | `string[]` | no | Extra files in the skill dir to inject as context |

---

## Installing Skills

### From the Hub

```bash
edgecrab skills list                               # list installed skills
edgecrab skills view security-audit               # read a skill
edgecrab skills install official/security-audit   # install from agentskills.io
edgecrab skills install raphaelmansuy/rust-fixer  # install from GitHub
edgecrab skills remove security-audit             # uninstall
```

From inside the TUI:

```
/skills browse         # browse hub
/skills install react  # install a skill
```

### Manual Installation

```bash
mkdir -p ~/.edgecrab/skills/my-skill
cat > ~/.edgecrab/skills/my-skill/SKILL.md << 'EOF'
---
name: my-skill
description: My custom workflow
category: custom
---

# My Skill

When this skill is active, follow these instructions...
EOF
```

No restart needed — EdgeCrab picks up new skills automatically.

---

## Loading Skills

### At Launch

```bash
edgecrab -S security-audit "audit the payment service"
edgecrab -S "security-audit,code-review" "full review"
edgecrab --skill rust-test-fixer --skill code-review
```

### Inside the TUI

```
/security-audit        # load skill, it prompts for input
```

Every installed skill is auto-registered as a slash command. Typing `/security-audit some context` loads the skill and sends `some context` as the first message.

### Permanently in Config

```yaml
# ~/.edgecrab/config.yaml
skills:
  preloaded:
    - security-audit
    - code-review
```

---

## Disabling Skills

Globally disable without uninstalling:

```yaml
skills:
  disabled:
    - heavy-skill
```

Platform-specific disable:

```yaml
skills:
  platform_disabled:
    telegram:
      - heavy-skill   # disabled in Telegram, active in CLI
```

---

## External Skill Directories

Share skills across projects or teams:

```yaml
# ~/.edgecrab/config.yaml
skills:
  external_dirs:
    - ~/.agents/skills           # another agent's directory
    - /shared/team/skills        # team skills
    - ${SKILLS_REPO}/skills      # env-var reference
```

Supports `~` expansion and `${VAR}` substitution. External directories are read-only.

---

## Skills vs Memory vs Context Files

| Concept | What it is | How it's populated |
|---------|------------|---------------------|
| Skills | Procedural workflow instructions | Written by you or hub-installed |
| Memory | Persistent facts about you and your projects | Auto-written by the agent |
| Context files | Project-level instructions (AGENTS.md, etc.) | You write; auto-discovered |

Skills are loaded on demand. Memory is always loaded (unless `--skip-memory`). Context files are project-scoped.
"""

with open(f"{BASE}/features/skills.md", "w") as f:
    f.write(skills)
print("skills.md written")

print("Batch 1 complete")
