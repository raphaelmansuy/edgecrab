#!/usr/bin/env python3
"""Write docs batch 3: fix overview.md, messaging section, features docs"""
import os

BASE = "src/content/docs"

# Ensure messaging directory
os.makedirs(f"{BASE}/user-guide/messaging", exist_ok=True)

# ─── features/overview.md (REWRITE with correct tool names) ───────────
overview = r"""---
title: Features Overview
description: Complete capability overview for EdgeCrab — the Rust-native autonomous coding agent with ratatui TUI, ReAct loop, multi-provider LLM, and built-in security.
sidebar:
  order: 1
---

EdgeCrab ships as a single static binary with enterprise-grade features. No Python venv, no Node.js — just one executable.

---

## Core Features

### Autonomous ReAct Loop

EdgeCrab runs a [Reason-Act-Observe loop](/features/react-loop/) — it reasons about a task, calls a tool, observes the result, then repeats. The loop runs up to `model.max_iterations` tool calls (default: 90) before stopping.

### Ratatui TUI

A full-featured terminal UI with:
- Streaming token display with cost tracking
- Tool execution feed
- 42 slash commands
- Keyboard-driven interface (vim-like navigation)
- Customizable skins (colors, symbols, kaomoji)

→ [TUI Interface](/features/tui/)

### 13+ LLM Providers

Switch provider and model without restarting:

```
/model anthropic/claude-opus-4
/model openai/gpt-4o
/model ollama/llama3.3
/model copilot/gpt-4.1-mini
```

→ [LLM Providers](/providers/overview/)

### Skills System

Reusable Markdown workflows that teach EdgeCrab domain-specific tasks:
- Skills are **directories** containing a `SKILL.md` file
- EdgeCrab can create and improve skills during sessions
- Fully compatible with Hermes Agent skill format

→ [Skills System](/features/skills/)

### Persistent Memory

Agent memory stored in `~/.edgecrab/memories/`:
- Auto-written after each session when `memory.auto_flush: true`
- Injected into the system prompt at session start
- Honcho integration for cross-session user modeling

→ [Memory](/features/memory/)

### Browser Automation

Built-in browser control via Chrome DevTools Protocol:
- Navigate, click, type, scroll, take screenshots
- Console log capture
- Session recording as WebM video
- Vision analysis of screenshots

→ [Browser Automation](/features/browser/)

### Multi-Platform Messaging Gateway

Run EdgeCrab as a persistent bot on:
- Telegram, Discord, Slack, Signal, WhatsApp

→ [Messaging Gateway](/user-guide/messaging/)

### Security

Built-in defense in depth:
- SSRF protection (blocks private IPs, cloud metadata endpoints)
- Prompt injection scanning in tool results
- Managed mode (`EDGECRAB_MANAGED=1`) blocks config writes
- Path restrictions and blocked commands

### Checkpoints & Rollback

Shadow git commits before every destructive file operation:

```
/rollback       # interactive checkpoint browser
```

→ [Checkpoints & Rollback](/user-guide/checkpoints/)

### Cron / Scheduled Tasks

Built-in cron scheduler with agent-managed jobs:

```bash
edgecrab cron add "0 9 * * 1-5" "morning standup summary"
edgecrab cron list
```

→ [Cron Jobs](/features/cron/)

---

## Tool Inventory

EdgeCrab ships 60+ tools organized into named toolsets. The toolset aliases used in config and CLI:

| Alias | Expands to |
|-------|-----------|
| `core` | file + meta + scheduling + delegation + code_execution + session + mcp + browser |
| `coding` | file + terminal + search + code_execution |
| `research` | web + browser + vision |
| `debugging` | terminal + web + file |
| `safe` | web + vision + image_gen + moa |
| `minimal` | file + terminal |
| `data_gen` | file + terminal + web + code_execution |
| `all` | every tool (no filtering) |

### File Tools (`file`)
| Tool | Description |
|------|-------------|
| `read_file` | Read file contents with optional line range |
| `write_file` | Write or overwrite a file |
| `patch` | Apply a unified diff patch to a file |
| `search_files` | Regex or glob search across file tree |

### Terminal Tools (`terminal`)
| Tool | Description |
|------|-------------|
| `terminal` | Run a shell command and capture output |
| `run_process` | Start a background process |
| `list_processes` | List running background processes |
| `kill_process` | Kill a process by ID |
| `get_process_output` | Get stdout/stderr from a background process |
| `wait_for_process` | Block until a process exits |
| `write_stdin` | Send input to a process's stdin |

### Web Tools (`web`)
| Tool | Description |
|------|-------------|
| `web_search` | DuckDuckGo search (SSRF-guarded) |
| `web_extract` | Extract text content from a URL |
| `web_crawl` | Recursive site crawl with optional depth limit |

### Browser Tools (`browser`)
| Tool | Description |
|------|-------------|
| `browser_navigate` | Navigate to a URL |
| `browser_snapshot` | Get page accessibility tree as text |
| `browser_screenshot` | Take a screenshot |
| `browser_click` | Click an element |
| `browser_type` | Type text into an input |
| `browser_scroll` | Scroll the page |
| `browser_console` | Capture console logs |
| `browser_back` | Go back in browser history |
| `browser_press` | Press a keyboard key |
| `browser_close` | Close the browser |
| `browser_get_images` | Get images from the page |
| `browser_vision` | Analyze page screenshot with vision model |

### Memory Tools (`memory`)
| Tool | Description |
|------|-------------|
| `memory_read` | Read a memory file |
| `memory_write` | Write or update a memory file |
| `honcho_conclude` | Commit a Honcho memory entry |
| `honcho_search` | Search Honcho user model |
| `honcho_list` | List Honcho memory entries |
| `honcho_remove` | Remove a Honcho entry |
| `honcho_profile` | Update Honcho user profile |
| `honcho_context` | Get relevant Honcho context |

### Skills Tools (`skills`)
| Tool | Description |
|------|-------------|
| `skills_list` | List available skills |
| `skills_categories` | List skill categories |
| `skill_view` | View a skill's content |
| `skill_manage` | Install/uninstall/update a skill |
| `skills_hub` | Browse the skills hub |

### Scheduling Tools (`scheduling`)
| Tool | Description |
|------|-------------|
| `manage_cron_jobs` | Create/list/delete/enable/disable cron jobs |

### Meta Tools (`meta`)
| Tool | Description |
|------|-------------|
| `manage_todo_list` | Create and track a session todo list |
| `clarify` | Ask the user a clarifying question |

### Delegation Tools (`delegation`)
| Tool | Description |
|------|-------------|
| `delegate_task` | Spawn a subagent for a parallel subtask |
| `mixture_of_agents` | Multi-model consensus reasoning |

### Code Execution (`code_execution`)
| Tool | Description |
|------|-------------|
| `execute_code` | Execute Python, Node.js, or Bash code in a sandbox |

### Session Tools (`session`)
| Tool | Description |
|------|-------------|
| `session_search` | Full-text search (FTS5) across session history |

### MCP Tools (`mcp`)
| Tool | Description |
|------|-------------|
| `mcp_list_tools` | List tools from connected MCP servers |
| `mcp_call_tool` | Call a tool on an MCP server |
| `mcp_list_resources` | List resources from MCP servers |
| `mcp_read_resource` | Read a resource from an MCP server |
| `mcp_list_prompts` | List prompts from MCP servers |
| `mcp_get_prompt` | Get a prompt from an MCP server |

### Media Tools (`media`)
| Tool | Description |
|------|-------------|
| `text_to_speech` | Convert text to speech (edge-tts, OpenAI, ElevenLabs) |
| `vision_analyze` | Analyze an image file with a vision model |
| `transcribe_audio` | Transcribe an audio file with Whisper |
| `generate_image` | Generate an image (runtime-gated) |

### Core Tools (`core`)
| Tool | Description |
|------|-------------|
| `checkpoint` | Create/list/restore/diff filesystem checkpoints |

---

## What's Next?

- **[ReAct Tool Loop](/features/react-loop/)** — How the autonomous reasoning engine works
- **[TUI Interface](/features/tui/)** — Full keyboard shortcuts and slash commands
- **[Skills System](/features/skills/)** — Creating and using reusable skills
- **[Memory](/features/memory/)** — Persistent memory and Honcho user modeling
- **[Browser Automation](/features/browser/)** — Browser automation with CDP
- **[SQLite State & Search](/features/state/)** — Session persistence and FTS5 search
"""

# Use replace to rewrite the file completely
with open(f"{BASE}/features/overview.md", "w") as f:
    f.write(overview)
print("features/overview.md rewritten")

# ─── user-guide/messaging/index.md ────────────────────────────────────
messaging_index = r"""---
title: Messaging Gateway
description: Run EdgeCrab as a persistent AI agent accessible from Telegram, Discord, Slack, Signal, and WhatsApp. Grounded in crates/edgecrab-gateway/src/.
sidebar:
  order: 11
---

The EdgeCrab Gateway runs as a background process that bridges messaging platforms to the EdgeCrab agent. Each message from a platform creates or resumes an agent session; replies are sent back through the same platform.

---

## Quick Start

```bash
# Start gateway with Telegram
TELEGRAM_BOT_TOKEN=xxxx edgecrab gateway start

# Or with explicit flag
edgecrab gateway start --telegram
```

The gateway listens on `http://127.0.0.1:8080` by default. Platform adapters connect as clients.

---

## How It Works

```
Platform message
    │
    ▼
Gateway HTTP server (127.0.0.1:8080)
    │
    ▼
Platform adapters (Telegram / Discord / Slack / Signal / WhatsApp)
    │
    ▼
Session manager (SQLite — one session per chat/thread/user)
    │
    ▼
EdgeCrab agent loop (full tool access)
    │
    ▼
Reply routed back to platform
```

Each platform maintains independent sessions. A Telegram conversation and a Discord conversation with the same user have separate agent contexts unless manually merged.

---

## Gateway Configuration

```yaml
# ~/.edgecrab/config.yaml
gateway:
  host: "127.0.0.1"       # bind address (use 0.0.0.0 to accept external connections)
  port: 8080
  webhook_enabled: true
  session_timeout_minutes: 30
```

Override with environment variables:

```bash
EDGECRAB_GATEWAY_HOST=0.0.0.0
EDGECRAB_GATEWAY_PORT=9090
```

---

## Platform Setup

| Platform | Guide |
|----------|-------|
| Telegram | [Telegram Setup](/user-guide/messaging/telegram/) |
| Discord | [Discord Setup](/user-guide/messaging/discord/) |
| Slack | [Slack Setup](/user-guide/messaging/slack/) |
| Signal | [Signal Setup](/user-guide/messaging/signal/) |
| WhatsApp | [WhatsApp Setup](/user-guide/messaging/whatsapp/) |

---

## Security

All platforms support an `allowed_users` list. When set, messages from other users are silently ignored:

```yaml
gateway:
  telegram:
    enabled: true
    allowed_users: ["myusername", "teammate"]

  discord:
    enabled: true
    allowed_users: ["123456789012345678"]  # Discord user IDs
```

For maximum security, run the gateway on `127.0.0.1` behind a reverse proxy with TLS.

---

## Home Channel (Proactive Messaging)

When `home_channel` is set, EdgeCrab can send proactive messages — e.g., from cron jobs or completed background tasks:

```yaml
gateway:
  telegram:
    home_channel: "-100123456789"  # chat ID
```

Then from the TUI or the agent:

```
/sethome           # set current channel as home_channel
```

---

## Managing Platforms

```
/platforms         # show status of all configured platforms
```

From the CLI:

```bash
edgecrab gateway status          # gateway status + connected platforms
edgecrab gateway start           # start the gateway daemon
edgecrab gateway stop            # stop the gateway daemon
edgecrab gateway logs            # follow gateway logs
```

---

## Approval Workflow

When `security.approval_required` is set, commands matching those patterns require explicit approval before execution:

```yaml
security:
  approval_required:
    - "rm "
    - "git push"
    - "kubectl delete"
```

The agent sends a confirmation message to the platform; you reply `/approve` or `/deny` (or click the inline button on Telegram/Discord).
"""

with open(f"{BASE}/user-guide/messaging/index.md", "w") as f:
    f.write(messaging_index)
print("messaging/index.md written")

# ─── user-guide/messaging/telegram.md ─────────────────────────────────
telegram = r"""---
title: Telegram
description: Connect EdgeCrab to Telegram. Grounded in crates/edgecrab-gateway/src/telegram.rs.
sidebar:
  order: 1
---

## Prerequisites

1. Create a bot via [@BotFather](https://t.me/BotFather): `/newbot` → copy the token
2. Add the bot to your group or start a private chat
3. Get your chat ID: message `@userinfobot`

---

## Configuration

### Environment Variable (Quick Start)

```bash
export TELEGRAM_BOT_TOKEN=1234567890:AAF...
edgecrab gateway start
```

Setting `TELEGRAM_BOT_TOKEN` automatically enables the Telegram platform.

### config.yaml

```yaml
gateway:
  telegram:
    enabled: true
    token_env: "TELEGRAM_BOT_TOKEN"  # default env var name
    allowed_users: []                 # empty = all users
    home_channel: ~                   # proactive message target
```

Additional optional env vars:

| Variable | Effect |
|----------|--------|
| `TELEGRAM_ALLOWED_USERS` | Comma-separated allowed usernames |
| `TELEGRAM_HOME_CHANNEL` | Default home channel ID |

---

## Usage

Send any message to your bot to start a session. A new agent session is created per Telegram chat — group chats and private chats have separate session contexts.

**Inline approval buttons:** When `security.approval_required` matches a command, EdgeCrab sends an inline keyboard with ✅ Approve / ❌ Deny buttons.

**Available platform slash commands** (sent as messages):

| Command | Effect |
|---------|--------|
| `/status` | Show current session status |
| `/new` | Start a new session |
| `/stop` | Stop the running task |
| `/help` | Show help |

---

## Security Notes

- Keep your bot token in `.env` — never in `config.yaml` (which may be committed)
- Use `allowed_users` in production deployments
- For group bots, set `privacy mode` off in BotFather settings or use commands with the `/command@botname` syntax
"""

with open(f"{BASE}/user-guide/messaging/telegram.md", "w") as f:
    f.write(telegram)
print("messaging/telegram.md written")

# ─── user-guide/messaging/discord.md ──────────────────────────────────
discord = r"""---
title: Discord
description: Connect EdgeCrab to Discord. Grounded in crates/edgecrab-gateway/src/discord.rs.
sidebar:
  order: 2
---

## Prerequisites

1. Create a Discord application at https://discord.com/developers/applications
2. Add a Bot under the "Bot" tab — enable **Message Content Intent**
3. Copy the bot token
4. Invite the bot: `https://discord.com/api/oauth2/authorize?client_id=<id>&permissions=2048&scope=bot`

---

## Configuration

### Environment Variable (Quick Start)

```bash
export DISCORD_BOT_TOKEN=OTY3...
edgecrab gateway start
```

### config.yaml

```yaml
gateway:
  discord:
    enabled: true
    token_env: "DISCORD_BOT_TOKEN"
    allowed_users: []                 # empty = all users; use numeric Discord IDs
    home_channel: ~                   # channel ID for proactive messages
```

Additional optional env vars:

| Variable | Effect |
|----------|--------|
| `DISCORD_ALLOWED_USERS` | Comma-separated Discord user IDs |
| `DISCORD_HOME_CHANNEL` | Default home channel ID |

---

## Usage

Mention the bot or send a DM to start a session. Each Discord channel or DM thread gets its own agent session.

**Approval flow:** When a command requires approval, EdgeCrab adds ✅/❌ reaction buttons to its message.

**Platform slash commands** (use as normal Discord slash commands if registered):

| Command | Effect |
|---------|--------|
| `/status` | Session status |
| `/new` | New session |
| `/stop` | Stop current task |

---

## Required Bot Permissions

| Permission | Reason |
|-----------|--------|
| `Send Messages` | Reply to users |
| `Read Message History` | Context for thread-based conversations |
| `Read Messages/View Channels` | Receive messages |
| `Message Content Intent` | (Privileged) receive message content |
"""

with open(f"{BASE}/user-guide/messaging/discord.md", "w") as f:
    f.write(discord)
print("messaging/discord.md written")

# ─── user-guide/messaging/slack.md ────────────────────────────────────
slack = r"""---
title: Slack
description: Connect EdgeCrab to Slack using Socket Mode. Grounded in crates/edgecrab-gateway/src/slack.rs.
sidebar:
  order: 3
---

## Prerequisites

Slack requires **two tokens** — a bot token and an app-level token for Socket Mode:

1. Create a Slack app at https://api.slack.com/apps
2. Under **OAuth & Permissions**, add bot scopes: `chat:write`, `app_mentions:read`, `im:history`, `im:read`
3. Install to workspace → copy **Bot User OAuth Token** (`xoxb-...`)
4. Enable **Socket Mode** → generate an app-level token with `connections:write` scope → copy token (`xapp-...`)
5. Enable **Event Subscriptions** → subscribe to `app_mention`, `message.im`

---

## Configuration

### Environment Variables (Quick Start)

```bash
export SLACK_BOT_TOKEN=xoxb-...
export SLACK_APP_TOKEN=xapp-...
edgecrab gateway start
```

Both `SLACK_BOT_TOKEN` and `SLACK_APP_TOKEN` must be set to auto-enable Slack.

### config.yaml

```yaml
gateway:
  slack:
    enabled: true
    bot_token_env: "SLACK_BOT_TOKEN"   # xoxb-...
    app_token_env: "SLACK_APP_TOKEN"   # xapp-...
    allowed_users: []                   # Slack user IDs (U...)
    home_channel: ~                     # channel ID for proactive messages
```

Additional optional env var:

| Variable | Effect |
|----------|--------|
| `SLACK_ALLOWED_USERS` | Comma-separated Slack user IDs |

---

## Usage

Mention `@EdgeCrab` in any channel, or send a direct message. Each Slack channel/DM gets its own session.

**Approval flow:** EdgeCrab adds Block Kit action buttons (Approve / Deny) to the approval message.
"""

with open(f"{BASE}/user-guide/messaging/slack.md", "w") as f:
    f.write(slack)
print("messaging/slack.md written")

# ─── user-guide/messaging/signal.md ───────────────────────────────────
signal_md = r"""---
title: Signal
description: Connect EdgeCrab to Signal via signal-cli HTTP daemon. Grounded in crates/edgecrab-gateway/src/signal.rs.
sidebar:
  order: 4
---

## Prerequisites

Signal requires a running [signal-cli](https://github.com/AsamK/signal-cli) HTTP daemon:

```bash
# Install signal-cli
brew install signal-cli             # macOS
apt install signal-cli              # Debian

# Register your number
signal-cli -u +1234567890 register
signal-cli -u +1234567890 verify 123456

# Start HTTP daemon
signal-cli -u +1234567890 daemon --http 127.0.0.1:8090
```

---

## Configuration

### Environment Variables (Quick Start)

```bash
export SIGNAL_HTTP_URL=http://127.0.0.1:8090
export SIGNAL_ACCOUNT=+1234567890
edgecrab gateway start
```

Both `SIGNAL_HTTP_URL` and `SIGNAL_ACCOUNT` must be set to auto-enable Signal.

### config.yaml

```yaml
gateway:
  signal:
    enabled: true
    http_url: ~          # signal-cli HTTP daemon URL (from SIGNAL_HTTP_URL)
    account: ~           # registered phone number (from SIGNAL_ACCOUNT)
    allowed_users: []    # phone numbers allowed to interact
```

---

## Usage

Send a message to the registered Signal number from any Signal client to start a session. Each sender gets their own agent session.

Signal provides end-to-end encryption — wire traffic between the Signal network and your device is encrypted. The plaintext is only visible to signal-cli and EdgeCrab on your server.
"""

with open(f"{BASE}/user-guide/messaging/signal.md", "w") as f:
    f.write(signal_md)
print("messaging/signal.md written")

# ─── user-guide/messaging/whatsapp.md ────────────────────────────────
whatsapp = r"""---
title: WhatsApp
description: Connect EdgeCrab to WhatsApp via a local WA bridge. Grounded in crates/edgecrab-gateway/src/whatsapp.rs.
sidebar:
  order: 5
---

## Prerequisites

WhatsApp integration uses a local bridge (whatsapp-web.js or baileys) that EdgeCrab can install automatically.

---

## Configuration

### Environment Variable (Quick Start)

```bash
export WHATSAPP_ENABLED=1
edgecrab gateway start
```

EdgeCrab will auto-install bridge dependencies on first run if `whatsapp.install_dependencies: true` (default).

### config.yaml

```yaml
gateway:
  whatsapp:
    enabled: false               # set true or use WHATSAPP_ENABLED=1
    bridge_port: 3000            # local bridge HTTP port
    bridge_url: ~                # custom bridge URL (overrides port)
    mode: "self-chat"            # "self-chat" | "any-sender"
    allowed_users: []            # phone numbers (include country code)
    reply_prefix: "⚕ *EdgeCrab Agent*\n------------\n"
    install_dependencies: true   # auto-install bridge on start
```

Additional optional env vars:

| Variable | Effect |
|----------|--------|
| `WHATSAPP_MODE` | Bridge mode (`self-chat` or `any-sender`) |
| `WHATSAPP_ALLOWED_USERS` | Comma-separated phone numbers |
| `WHATSAPP_BRIDGE_PORT` | Override bridge port (default: 3000) |
| `WHATSAPP_BRIDGE_URL` | Override full bridge URL |
| `WHATSAPP_SESSION_PATH` | Path to bridge session storage |
| `WHATSAPP_REPLY_PREFIX` | Text prepended to all replies |

---

## Modes

| Mode | Description |
|------|-------------|
| `self-chat` | Only responds to messages from the linked phone number (safe for personal use) |
| `any-sender` | Responds to messages from any sender in `allowed_users` |

---

## First Run

On first start, the bridge opens a QR code in the terminal. Scan it with the WhatsApp app (Linked Devices → Link a device) to authenticate. The session is persisted and survives restarts.

```bash
edgecrab gateway start --whatsapp
# Scan QR code in terminal
```
"""

with open(f"{BASE}/user-guide/messaging/whatsapp.md", "w") as f:
    f.write(whatsapp)
print("messaging/whatsapp.md written")

print("\nBatch 3a (overview + messaging) complete")
