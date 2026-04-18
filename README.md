# EdgeCrab 🦀

> **"Your SuperAgent — built in Rust."**

[![License](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.86%2B-orange.svg)](https://www.rust-lang.org/)
[![crates.io](https://img.shields.io/crates/v/edgecrab-cli.svg)](https://crates.io/crates/edgecrab-cli)
[![PyPI](https://img.shields.io/pypi/v/edgecrab-cli.svg)](https://pypi.org/project/edgecrab-cli/)
[![npm](https://img.shields.io/npm/v/edgecrab-cli.svg)](https://www.npmjs.com/package/edgecrab-cli)
[![CI](https://github.com/raphaelmansuy/edgecrab/actions/workflows/ci.yml/badge.svg)](https://github.com/raphaelmansuy/edgecrab/actions/workflows/ci.yml)
[![Website](https://img.shields.io/badge/Website-edgecrab.com-orange.svg)](https://www.edgecrab.com)

[![Changelog](https://img.shields.io/badge/Changelog-CHANGELOG.md-blue.svg)](CHANGELOG.md)

EdgeCrab is a **SuperAgent** — a personal assistant and coding agent forged in Rust. It carries the soul of **Nous Hermes Agent** (autonomous reasoning, persistent memory, user-first alignment) and the always-on presence of **OpenClaw** (17 messaging gateways, smart-home integration), packaged as a stripped native release binary of about **49 MB** on current macOS arm64 builds, with zero Python or Node.js runtime dependencies. Runs on Linux, macOS, and Android (Termux).

> **Latest release: v0.7.0** — first-class SDK coverage across Rust, Python, Node.js, and WASM, with refreshed examples, release-safe docs, and a cleaner onboarding path.


## Architecture

![Architecture](./assets/edgecrab-archi.jpg)

```
hermes-agent soul  +  OpenClaw vision  =  EdgeCrab
   (reasoning)          (presence)        (Rust)
```

| Metric              | EdgeCrab 🦀                     | hermes-agent ☤   |
| ------------------- | ------------------------------ | ---------------- |
| Binary              | ~49 MB stripped release build  | Python venv + uv |
| Runtime bootstrap   | None                           | Python + uv      |
| Memory              | Workload-dependent native process | ~80–150 MB    |
| LLM providers       | 15 built-in                    | varies           |
| Messaging platforms | 17 gateways                    | 7 platforms      |
| Tests               | 1629 passing (Rust)            | —                |
| Migrate from hermes | `edgecrab migrate`             | N/A              |

![EdgeCrab — The Clash of the Crustaceans](assets/edgecrab-hero.jpeg)

---

## Table of Contents

- [EdgeCrab 🦀](#edgecrab-)
  - [Architecture](#architecture)
  - [Table of Contents](#table-of-contents)
  - [Why EdgeCrab?](#why-edgecrab)
  - [Quick Start (90 seconds)](#quick-start-90-seconds)
    - [Option A — npm (no Rust required)](#option-a--npm-no-rust-required)
    - [Option B — pip (no Rust required)](#option-b--pip-no-rust-required)
    - [Option C — cargo](#option-c--cargo)
    - [Option D — build from source](#option-d--build-from-source)
    - [Guided Setup Output](#guided-setup-output)
    - [First Prompts](#first-prompts)
  - [What EdgeCrab Can Do](#what-edgecrab-can-do)
    - [ReAct Tool Loop](#react-tool-loop)
    - [Built-in Tools](#built-in-tools)
    - [Semantic Code Intelligence (LSP)](#semantic-code-intelligence-lsp)
      - [File Tools (`file` toolset)](#file-tools-file-toolset)
      - [Terminal Tools (`terminal` toolset)](#terminal-tools-terminal-toolset)
      - [Web Tools (`web` toolset)](#web-tools-web-toolset)
      - [Browser Tools (`browser` toolset)](#browser-tools-browser-toolset)
      - [Memory \& Honcho Tools (`memory` + `honcho` toolsets)](#memory--honcho-tools-memory--honcho-toolsets)
      - [Skills Tools (`skills` toolset)](#skills-tools-skills-toolset)
      - [Session \& Search (`session` toolset)](#session--search-session-toolset)
      - [Delegation \& MoA (`delegation` + `moa` toolsets)](#delegation--moa-delegation--moa-toolsets)
      - [Code Execution (`code_execution` toolset)](#code-execution-code_execution-toolset)
      - [MCP Tools (`mcp` toolset)](#mcp-tools-mcp-toolset)
      - [Media Tools (`vision` / `tts` / `transcribe` toolsets)](#media-tools-vision--tts--transcribe-toolsets)
      - [Automation Tools](#automation-tools)
    - [Sub-agent Delegation](#sub-agent-delegation)
    - [Sandboxed Code Execution](#sandboxed-code-execution)
    - [Browser Automation](#browser-automation)
    - [17 Messaging Gateways](#17-messaging-gateways)
    - [Persistent Memory \& Learning](#persistent-memory--learning)
    - [Skills Library](#skills-library)
    - [Skills Vs Plugins](#skills-vs-plugins)
    - [Plugin System](#plugin-system)
    - [Cron Scheduling](#cron-scheduling)
    - [Checkpoints \& Rollback](#checkpoints--rollback)
    - [Profiles \& Worktrees](#profiles--worktrees)
    - [Vision, TTS \& Transcription](#vision-tts--transcription)
  - [15 LLM Providers](#15-llm-providers)
  - [6 Terminal Backends](#6-terminal-backends)
  - [MCP Server Integration](#mcp-server-integration)
  - [ACP / VS Code Copilot Integration](#acp--vs-code-copilot-integration)
  - [ratatui TUI](#ratatui-tui)
  - [All CLI Commands](#all-cli-commands)
  - [All Slash Commands](#all-slash-commands)
  - [Security Model](#security-model)
  - [Architecture](#architecture-1)
  - [Configuration](#configuration)
  - [SDKs](#sdks-one-edgecrab-experience)
    - [Python SDK (`edgecrab`)](#python-sdk-edgecrab)
    - [Node.js SDK (`edgecrab`)](#nodejs-sdk-edgecrab)
  - [Docker](#docker)
  - [Migrating from hermes-agent](#migrating-from-hermes-agent)
  - [Testing](#testing)
  - [Project Structure](#project-structure)
  - [Requirements \& Build](#requirements--build)
  - [Contributing](#contributing)
  - [Release Channels](#release-channels)
  - [License](#license)

---

## Why EdgeCrab?

Most AI agents are either too constrained (coding agents that forget you exist after the session) or too heavy (Python runtimes, Node daemons, GBs of RAM). EdgeCrab is different.

**It learns.** Like Nous Hermes Agent, EdgeCrab maintains persistent memory across sessions, auto-generates reusable skills, and builds a cross-session Honcho user model that gets smarter over time.

**It's everywhere.** Like OpenClaw, EdgeCrab lives in your channels — Telegram, Discord, Slack, WhatsApp, Signal, Matrix, Mattermost, DingTalk, SMS, Email, Home Assistant, and more. Send it a voice memo on WhatsApp and get a PR back.

**It's fast and lean.** Unlike Python agents, EdgeCrab ships as a native Rust binary instead of a Python or Node.js runtime stack. Current stripped macOS arm64 release builds land around 49 MB, and security is compiled in — path jails, SSRF guards, command scanners — not runtime patches.

**It's extensible.** MCP servers, custom Rust tools, Python/JS sandboxes, sub-agents, Mixture-of-Agents consensus — the full toolkit for heavy-duty automation.

**It's now plugin-native.** Skill plugins inject prompt expertise, tool-server plugins expose external JSON-RPC tools, and script plugins run safe Rhai logic, all from `~/.edgecrab/plugins/` with persisted enable/disable policy.

---

## Quick Start (90 seconds)

### Option A — npm (no Rust required)

```bash
npm install -g edgecrab-cli
edgecrab update              # channel-aware updater
edgecrab setup               # interactive wizard — detects API keys, writes config
edgecrab doctor              # verify health
edgecrab                     # launch TUI
```

### Option B — pip (no Rust required)

```bash
pip install edgecrab-cli
# OR: pipx install edgecrab-cli  (isolated install)
edgecrab update
edgecrab setup && edgecrab doctor && edgecrab
```

### Option C — cargo

```bash
cargo install edgecrab-cli
edgecrab update --check
edgecrab setup && edgecrab doctor && edgecrab
```

### Option D — build from source

```bash
git clone https://github.com/raphaelmansuy/edgecrab
cd edgecrab
cargo build --workspace --release         # ~30 s first build
./target/release/edgecrab setup
```

### Guided Setup Output

```
EdgeCrab Setup Wizard
──────────────────────────────────────────────────────────────
✓ Detected GitHub Copilot (GITHUB_TOKEN)
✓ Detected OpenAI (OPENAI_API_KEY)

Choose LLM provider:
  [1] copilot      (GitHub Copilot — GPT-5 / Claude / Gemini catalog)  ← auto-detected
  [2] openai       (OpenAI — GPT-4.1, GPT-5, o3/o4)
  [3] anthropic    (Anthropic — Claude Opus 4.6)
  [4] ollama       (local — llama3.3)
  ...
Provider [1]: 1

✓ Config written to ~/.edgecrab/config.yaml
✓ Created ~/.edgecrab/memories/
✓ Created ~/.edgecrab/skills/

Run `edgecrab` to start chatting!
```

### First Prompts

```bash
edgecrab "summarise the git log for today and open PRs"
edgecrab --model openai/gpt-5 "review this codebase for security issues"
edgecrab --model ollama/llama3.3 "explain this code offline"
edgecrab --quiet "count lines in src/**/*.rs"   # pipe-safe, no banner
edgecrab -C "continue-my-refactor"              # resume named session
edgecrab -w "explore that perf idea"            # isolated git worktree
```

---

## What EdgeCrab Can Do

EdgeCrab is an autonomous agent. Give it a goal in natural language; it reasons, calls tools, observes results, and loops until the task is done. Here's what it can actually reach.

### ReAct Tool Loop

EdgeCrab uses a **Reason → Act → Observe** loop (ReAct pattern) implemented in `crates/edgecrab-core/src/conversation.rs`. Each turn:

1. **System prompt built once** per session (SOUL.md, AGENTS.md, memories, skills, date/time, cwd) — cached for Anthropic prompt cache hits
2. **LLM decides** what to do next (including parallel tool calls)
3. **Security check** runs before every tool execution (path jail, SSRF guard, command scan)
4. **Tool executes** — file I/O, shell, web, code, sub-agents, browser, etc.
5. **Result injected** back into context
6. **Loop** until no more tool calls (task done), `Ctrl-C`, or 90-iteration budget exhausted
7. **Context compression** fires at 50% of context window — prunes old tool outputs, then LLM-summarizes
8. **Learning reflection** auto-fires after ≥5 tool calls — agent can save new skills and update memory

The budget default is **90 iterations** (`max_iterations` in config). Increase it for long autonomous tasks.

### Built-in Tools

Tools are registered at compile time via the `inventory` crate — zero startup cost. The `ToolRegistry` dispatches by exact name with fuzzy (Levenshtein ≤3) fallback suggestions.

### Semantic Code Intelligence (LSP)

EdgeCrab now exposes a dedicated LSP subsystem through the `lsp` toolset. When a language server is configured, the agent can prefer semantic operations over grep-style guesses:

- Claude-parity navigation: `lsp_goto_definition`, `lsp_find_references`, `lsp_hover`, `lsp_document_symbols`, `lsp_workspace_symbols`, `lsp_goto_implementation`, `lsp_call_hierarchy_prepare`, `lsp_incoming_calls`, `lsp_outgoing_calls`
- EdgeCrab-only semantic edits: `lsp_code_actions`, `lsp_apply_code_action`, `lsp_rename`, `lsp_format_document`, `lsp_format_range`
- Deep analysis: `lsp_inlay_hints`, `lsp_semantic_tokens`, `lsp_signature_help`, `lsp_type_hierarchy_prepare`, `lsp_supertypes`, `lsp_subtypes`
- Diagnostics: `lsp_diagnostics_pull`, `lsp_linked_editing_range`, `lsp_enrich_diagnostics`, `lsp_select_and_apply_action`, `lsp_workspace_type_errors`

Built-in default server definitions now cover Rust, TypeScript, JavaScript, Python, Go, C, C++, Java, C#, PHP, Ruby, Bash, HTML, CSS, and JSON.

#### File Tools (`file` toolset)
| Tool           | What it does                                                                 |
| -------------- | ---------------------------------------------------------------------------- |
| `read_file`    | Read file with optional `start_line`/`end_line` — path-jailed, canonicalized |
| `write_file`   | Write or create file (parent dirs auto-created)                              |
| `patch_file`   | Search-and-replace patch — exact string match, atomic write                  |
| `search_files` | Regex + glob search across a directory tree                                  |

#### Terminal Tools (`terminal` toolset)
| Tool             | What it does                                                         |
| ---------------- | -------------------------------------------------------------------- |
| `terminal`       | Execute shell command — persistent shell per task, env-var blocklist |
| `manage_process` | Start/stop/list/kill/read background processes                       |

#### Web Tools (`web` toolset)
| Tool          | What it does                                                             |
| ------------- | ------------------------------------------------------------------------ |
| `web_search`  | Web search via Firecrawl → Tavily → Brave → DuckDuckGo fallback chain    |
| `web_extract` | Full-page extraction — HTML strip + PDF parse (EdgeParse) — SSRF-guarded |

#### Browser Tools (`browser` toolset)
| Tool                 | What it does                                  |
| -------------------- | --------------------------------------------- |
| `browser_navigate`   | Navigate Chrome via CDP                       |
| `browser_snapshot`   | Accessibility tree snapshot (text, not pixel) |
| `browser_click`      | Click element by `@eN` ref ID from snapshot   |
| `browser_type`       | Type text into focused input                  |
| `browser_screenshot` | Annotated screenshot with numbered elements   |
| `browser_console`    | Capture/clear browser console log             |

#### Memory & Honcho Tools (`memory` + `honcho` toolsets)
| Tool             | What it does                                                    |
| ---------------- | --------------------------------------------------------------- |
| `memory_read`    | Read `MEMORY.md` and `USER.md` from `~/.edgecrab/memories/`     |
| `memory_write`   | Write/append to memory files (prompt-injection scanned)         |
| `honcho_profile` | Get/set user profile facts via Honcho cross-session model       |
| `honcho_context` | Retrieve contextually relevant Honcho memories for current task |

#### Skills Tools (`skills` toolset)
| Tool           | What it does                             |
| -------------- | ---------------------------------------- |
| `skill_manage` | Create, view, patch, delete, list skills |

#### Session & Search (`session` toolset)
| Tool             | What it does                                          |
| ---------------- | ----------------------------------------------------- |
| `session_search` | SQLite FTS5 full-text search across all past sessions |

#### Delegation & MoA (`delegation` + `moa` toolsets)
| Tool                | What it does                                                                                             |
| ------------------- | -------------------------------------------------------------------------------------------------------- |
| `delegate_task`     | Fork a sub-agent — single task or batch of up to 3 in parallel                                           |
| `mixture_of_agents` | Run task through Claude Opus 4.6, Gemini 2.5 Pro, GPT-4.1, DeepSeek R1 in parallel; synthesize consensus |

#### Code Execution (`code_execution` toolset)
| Tool           | What it does                                                              |
| -------------- | ------------------------------------------------------------------------- |
| `execute_code` | Sandboxed Python / JS / Bash / Ruby / Perl / Rust execution with tool RPC |

#### MCP Tools (`mcp` toolset)
| Tool             | What it does                                    |
| ---------------- | ----------------------------------------------- |
| `mcp_list_tools` | List tools exposed by all connected MCP servers |
| `mcp_call_tool`  | Call a named tool on any connected MCP server   |

#### Media Tools (`vision` / `tts` / `transcribe` toolsets)
| Tool               | What it does                                                 |
| ------------------ | ------------------------------------------------------------ |
| `vision_analyze`   | Analyze image via multimodal model (URL or local path)       |
| `text_to_speech`   | Generate audio from text (OpenAI TTS or configured provider) |
| `transcribe_audio` | Transcribe audio file (Whisper or Groq/OpenAI)               |

#### Automation Tools
| Tool                    | What it does                                                   |
| ----------------------- | -------------------------------------------------------------- |
| `manage_todo_list`      | Structured checklist — create, update, complete, delete items  |
| `manage_cron_jobs`      | Schedule recurring and one-shot cron jobs                      |
| `checkpoint`            | Filesystem snapshot for rollback (create, list, restore, diff) |
| `clarify`               | Ask user a clarifying question (with optional choices)         |
| `send_message`          | Send message via gateway to any connected platform             |
| `ha_get_states`         | Fetch Home Assistant entity states                             |
| `ha_call_service`       | Call HA service (e.g. `light.turn_on`)                         |
| `ha_trigger_automation` | Trigger HA automation                                          |
| `ha_get_history`        | Fetch HA entity history                                        |

**Control which toolsets are active:**
```bash
edgecrab --toolset file,terminal "add tests"        # minimal dev
edgecrab --toolset all "go wild"                    # full capability
edgecrab --toolset coding "refactor this module"    # file+terminal+search+exec+lsp
edgecrab --toolset research "investigate this bug"  # web+browser+vision
```

---

### Sub-agent Delegation

EdgeCrab can spawn sub-agents that run the full ReAct loop with their own session state. This enables parallelism for complex tasks.

```
# Example: agent delegates 3 subtasks in parallel
delegate_task([
  { task: "Review auth module for security issues" },
  { task: "Write unit tests for the payment service" },
  { task: "Update API documentation" }
])
# → 3 sub-agents run concurrently, results aggregated
```

**How it works** (`crates/edgecrab-tools/src/tools/delegate_task.rs`):
- Sub-agents share LLM provider Arc + tool registry Arc
- Each child gets its own `SessionState`, `ProcessTable`, `TodoStore`, `IterationBudget`
- Max concurrent: **3 sub-agents in parallel** (configurable via `delegation.max_subagents`)
- Max depth: **2 levels** (parent → child → grandchild blocked)
- Children cannot use `delegation`, `clarify`, `memory`, `code_execution`, or `messaging` toolsets

Configure delegation:
```yaml
delegation:
  enabled: true
  model: "openai/gpt-4o"   # use a capable shared model for sub-agents
  max_subagents: 3
  max_iterations: 50
```

---

### Sandboxed Code Execution

The `execute_code` tool runs code in an isolated subprocess with strict resource limits:

- **Languages**: Python, JavaScript, Bash, Ruby, Perl, Rust
- **Tool RPC**: Scripts can call 7 tools via Unix domain socket — `web_search`, `web_extract`, `read_file`, `write_file`, `search_files`, `terminal`, `session_search`
- **Limits**: 50-tool call limit, 5-minute timeout, 50 KB stdout cap, 10 KB stderr cap
- **Security**: API keys/tokens stripped from child environment before execution

```python
# Example: agent writes and executes this in a sandbox
import subprocess
result = subprocess.run(['cargo', 'test', '-p', 'edgecrab-core'], capture_output=True)
print(result.stdout.decode())
```

---

### Browser Automation

Chrome DevTools Protocol-based browser automation — no Selenium, no Playwright dependency. ElementCrab connects directly to a CDP endpoint.

```
Requirements: Chrome/Chromium binary, or set CDP_URL to an existing instance
Check:         edgecrab doctor  (reports browser availability)
```

The `browser_snapshot` tool returns an accessibility tree — not pixels — so the LLM can reason about page structure without vision costs. `browser_screenshot` adds numbered element overlays for precise clicking.

---

### 17 Messaging Gateways

Start the gateway server and EdgeCrab becomes an always-on assistant in 17 messaging platforms simultaneously:

```bash
edgecrab gateway start           # runs in background
edgecrab gateway start --foreground   # keep in foreground
edgecrab gateway status          # check which platforms are live
edgecrab gateway stop
```

| Platform           | Transport                               | Auth                                              |
| ------------------ | --------------------------------------- | ------------------------------------------------- |
| **Telegram**       | Long-poll REST                          | `TELEGRAM_BOT_TOKEN`                              |
| **Discord**        | WebSocket gateway                       | `DISCORD_BOT_TOKEN`                               |
| **Slack**          | Socket Mode WebSocket                   | `SLACK_BOT_TOKEN` + `SLACK_APP_TOKEN`             |
| **WhatsApp**       | Baileys bridge (local Node subprocess)  | `edgecrab whatsapp` QR pairing                    |
| **Signal**         | signal-cli HTTP + SSE                   | `SIGNAL_HTTP_URL` + `SIGNAL_ACCOUNT`              |
| **Matrix**         | Client-Server REST + long-poll sync     | `MATRIX_HOMESERVER` + `MATRIX_ACCESS_TOKEN`       |
| **Mattermost**     | REST v4 + WebSocket                     | `MATTERMOST_URL` + `MATTERMOST_TOKEN`             |
| **DingTalk**       | Stream SDK (no public webhook)          | `DINGTALK_APP_KEY` + `DINGTALK_APP_SECRET`        |
| **SMS**            | Twilio REST v2010                       | `TWILIO_ACCOUNT_SID` + `TWILIO_AUTH_TOKEN`        |
| **Email**          | SMTP (lettre, rustls) + inbound webhook | `EMAIL_PROVIDER` + `EMAIL_FROM` + `EMAIL_API_KEY` |
| **Home Assistant** | WebSocket + REST                        | `HASS_URL` + `HASS_TOKEN`                         |
| **Webhook**        | axum HTTP POST                          | any HTTP caller                                   |
| **API Server**     | axum OpenAI-compatible HTTP             | `API_SERVER_PORT` (optional)                      |
| **Feishu/Lark**    | REST                                    | `FEISHU_APP_ID` + `FEISHU_APP_SECRET`             |
| **WeCom**          | WebSocket + REST + heartbeat            | `WECOM_BOT_ID` + `WECOM_SECRET`                   |
| **iMessage**       | BlueBubbles REST + webhook + attachments| `BLUEBUBBLES_SERVER_URL` + `BLUEBUBBLES_PASSWORD` |
| **WeChat**         | iLink Bot API POST-poll + AES CDN media | `WEIXIN_TOKEN` + `WEIXIN_ACCOUNT_ID`              |

**Streaming delivery**: Edit-mode platforms (Telegram, Discord, Slack) receive live token streaming with 300ms edit intervals. Batch-mode platforms (WhatsApp, Signal, SMS, Email) accumulate the full response and send once.

**Built-in gateway slash commands** (send via chat):
```
/help      /new       /reset     /stop      /retry
/status    /usage     /background  /approve   /deny
```

**Setup WhatsApp** (one-time QR pairing):
```bash
edgecrab whatsapp      # launches QR code scanner wizard
# Scan with your phone — session persists across restarts
edgecrab gateway start
```

**Cron-triggered messages**: Schedule the agent to proactively message you:
```yaml
# ~/.edgecrab/cron/daily-standup.json
schedule: "0 9 * * 1-5"     # every weekday at 9am
task: "Summarize open PRs and blockers for today's standup"
target: telegram             # deliver to your Telegram
```

---

### Persistent Memory & Learning

EdgeCrab has a three-layer memory system:

**Layer 1 — MEMORY.md** (`~/.edgecrab/memories/MEMORY.md`): Free-form notes. The agent reads this at session start and can update it. You can also edit it directly.

**Layer 2 — SQLite session history** (`~/.edgecrab/state.db`): Every conversation stored in WAL-mode SQLite with FTS5 full-text search. Browse, search, and export sessions:
```bash
edgecrab sessions list                           # list recent sessions
edgecrab sessions search "auth bug from last week"  # FTS5 search
edgecrab sessions export <id> --format jsonl     # export session
edgecrab sessions browse                         # interactive browser
```

**Layer 3 — Honcho cross-session user model**: EdgeCrab builds a semantic model of you — your preferences, projects, working style — via the Honcho API. This context is injected at the start of new sessions to provide continuity.

**Auto-learning**: After ≥5 tool calls in a session, a learning reflection fires automatically. The agent can save new skills, update MEMORY.md, and record useful patterns without being asked.

---

### Skills Library

Skills are reusable agent procedures — markdown files that define prompts, steps, and best practices for recurring tasks. Think recipe cards for your agent.

```bash
# Create a skill
edgecrab skills list                    # browse installed skills
edgecrab skills view git-workflow       # read a skill
edgecrab skills install my-skill.md    # install from file
edgecrab skills search "diagram"       # search remote skill sources
edgecrab skills install edgecrab:diagramming/ascii-diagram-master
edgecrab skills install hermes-agent:research/ml-paper-writing
edgecrab skills install raphaelmansuy/edgecrab/skills/research/ml-paper-writing
edgecrab skills update                 # refresh all remote-installed skills
edgecrab skills update ml-paper-writing

# Use a skill in a session
edgecrab -S git-workflow "review this branch for prod readiness"
edgecrab -S security,refactor          # load multiple skills
```

Inside TUI: `/skills` opens the installed-skill browser, and `/skills search [query]` opens the remote-skill browser with live search, source notes, and install/update actions.

Skills are saved to `~/.edgecrab/skills/` and loaded on demand. The agent can also create new skills mid-session during learning reflection.

Claude-style skill bundles with helper scripts are supported in the standalone skills runtime:

- bundled helper files under `references/`, `templates/`, `scripts/`, and `assets/`
- `${CLAUDE_SKILL_DIR}` substitution to the concrete skill directory
- `${CLAUDE_SESSION_ID}` substitution to the active EdgeCrab session id
- the same bundle rendering for `skill_view` and preloaded `--skill` / `skills.preloaded` flows
- parsing and display of `when_to_use`, `arguments`, `argument-hint`, `allowed-tools`,
  `user-invocable`, `disable-model-invocation`, `context`, and `shell`

Current boundary: EdgeCrab does not auto-execute Claude inline prompt-shell
blocks and does not auto-fork a dedicated skill sub-agent from those metadata
fields alone.

### Skills Vs Plugins

First principles:

- A `skill` is reusable guidance for the model.
- A `plugin` is an installable runtime unit that EdgeCrab discovers, enables, disables, updates, and audits.

That leads to a clean operational split:

- Use `skills` when the extension is instructions-first: procedures, examples, checklists, workflow scaffolding, or bundled helper files/scripts that the agent uses through normal tools.
- Use `plugins` when the extension needs executable code, tool registration, hooks, readiness checks, trust metadata, or install lifecycle management.
- A plain skill changes prompt behavior. It can bundle helper files such as `scripts/`, `references/`, `templates/`, and `assets/`, but it still does not register a new runtime service or plugin lifecycle on its own.
- A plugin may bundle a `SKILL.md`, but that bundled skill is still part of a plugin-managed runtime bundle.

Concrete examples:

- `~/.edgecrab/skills/security-review/SKILL.md` is a standalone skill.
- `~/.edgecrab/skills/security-review/scripts/check.py` can be bundled with that skill and referenced from `SKILL.md`.
- `~/.edgecrab/plugins/github-tools/plugin.toml` is a plugin.
- `~/.edgecrab/plugins/calculator/plugin.yaml` plus `__init__.py` is a Hermes plugin.
- A plugin of kind `skill` is still managed through `edgecrab plugins ...`, not `edgecrab skills ...`.

---

### Plugin System

Plugins extend EdgeCrab beyond the built-in tool inventory without forking the repo.

```bash
edgecrab plugins list
edgecrab plugins info github-tools
edgecrab plugins status
edgecrab plugins install github:edgecrab/plugins/github-tools
edgecrab plugins install hub:community/github-tools
edgecrab plugins install https://example.com/github-tools.zip
edgecrab plugins install ./plugins/github-tools
edgecrab plugins enable github-tools
edgecrab plugins disable github-tools
edgecrab plugins toggle [github-tools]
edgecrab plugins audit --lines 20
edgecrab plugins search github
edgecrab plugins search --source hermes weather
edgecrab plugins search --source hermes-evey telemetry
edgecrab plugins browse
edgecrab plugins update
edgecrab plugins remove github-tools
```

Inside the TUI, `/plugins search ...` and `/plugins browse` now open the same
kind of async remote browser EdgeCrab already uses for skills and MCP:
fuzzy filtering, background search, split-detail view, and one-key install or
replace from official registries.

EdgeCrab now supports four plugin kinds:

- `skill` plugins load `SKILL.md` content from `~/.edgecrab/plugins/<name>/` into the session prompt with Hermes-compatible frontmatter, readiness checks, and platform filtering.
- `tool-server` plugins spawn a subprocess and proxy MCP-compatible newline-delimited JSON-RPC over stdio, including reverse `host:*` calls for platform info, memory/session access, secret reads, safe conversation message injection, logging, and delegated tool execution.
- `script` plugins load Rhai code for lightweight local extension points and tool handlers without shipping a separate daemon.
- `hermes` plugins load Hermes-style Python directory plugins with `plugin.yaml` + `__init__.py register(ctx)` compatibility, including `requires_env` setup gating, bundled `SKILL.md` loading, `post_tool_call`, `on_session_start`, `pre_llm_call`, and `on_session_end`.

EdgeCrab also discovers legacy Hermes plugin roots from `~/.hermes/plugins/`, plus `./.hermes/plugins/` when `HERMES_ENABLE_PROJECT_PLUGINS=true`. Plugin installs now stage in quarantine, run a static security scan, resolve trust from their source, and stamp `plugin.toml` with a directory checksum before activation. Plugin state persists in `config.yaml` under `plugins:`. Disabled or setup-needed plugins are excluded from tool exposure or prompt injection without uninstalling them.

Runtime exposure is live:

- enabled plugin tools are registered into the `plugins` toolset and appear in `/tools`
- disabling a plugin removes its tools from the active registry without restarting EdgeCrab
- re-enabling a plugin re-exposes those tools immediately in the same TUI session

Inside the TUI you can verify that directly:

```text
/plugins                 # open the installed-plugin browser overlay
/tools                   # shows active built-in + plugin tools
/plugins disable demo
/tools                   # demo plugin tools are gone
/plugins enable demo
/tools                   # demo plugin tools are back under the plugins toolset
```

Remote plugin search is cached by first principles:

- hub indexes and repo-backed source trees are cached under `~/.edgecrab/plugins/.hub/cache/`
- repo-backed plugin descriptions are cached separately so repeated searches do not refetch `plugin.yaml` or `SKILL.md`
- expired cache is refreshed when possible, but stale cache is still used on refresh failure so plugin search degrades gracefully instead of going empty

Example: install a Hermes guide-style local plugin with a bundled skill:

```text
calculator/
├── plugin.yaml
├── __init__.py
├── schemas.py
├── tools.py
├── SKILL.md
└── data/
    └── units.json
```

```bash
edgecrab plugins install ./calculator
edgecrab plugins info calculator
edgecrab plugins status
```

This repository also ships official Hermes-format examples that are indexed by
the `edgecrab-official` search source:

```bash
edgecrab plugins search --source edgecrab calculator
edgecrab plugins search --source edgecrab json

edgecrab plugins install ./plugins/productivity/calculator
edgecrab plugins install ./plugins/developer/json-toolbox

edgecrab plugins info calculator
edgecrab plugins info json-toolbox
```

Those examples prove two different Hermes runtime surfaces:

- `plugins/productivity/calculator` registers tools plus a `post_tool_call` hook
- `plugins/developer/json-toolbox` registers tools plus a top-level CLI command

Example: install real Hermes assets directly from a local clone of `NousResearch/hermes-agent`:

```bash
edgecrab plugins install ~/src/hermes-agent/plugins/memory/holographic
edgecrab plugins info holographic

# pip entry-point plugins are discovered through the selected Python runtime
EDGECRAB_PLUGIN_PYTHON=~/.venvs/hermes/bin/python \
  edgecrab plugins list
EDGECRAB_PLUGIN_PYTHON=~/.venvs/hermes/bin/python \
  edgecrab entry-demo status
```

Standalone Hermes skills are browsed from the skills surface instead of the
plugin browser:

```bash
edgecrab skills search 1password
edgecrab skills install hermes-agent:security/1password
```

Example: search and install curated community Hermes plugins from `42-evey/hermes-plugins`:

```bash
edgecrab plugins search --source hermes-evey telemetry
edgecrab plugins install hub:hermes-evey/evey-telemetry
edgecrab plugins install hub:hermes-evey/evey-status
edgecrab plugins info evey-telemetry
```

For a step-by-step authoring tutorial, see `docs/007_memory_skills/005_building_hermes_style_plugins.md` and the site guide at `site/src/content/docs/guides/build-hermes-plugin.md`.

Compatibility proof currently covers:

- official repo Hermes examples `calculator` and `json-toolbox`, including search visibility and local end-to-end install/runtime proof
- guide-style Hermes plugin install and end-to-end tool execution from the upstream "Build a Hermes Plugin" contract
- real upstream Hermes plugin install and runtime execution for `holographic`
- real upstream Hermes optional-skill compatibility for `1password` via local bundle install
- real upstream Python import/runtime shims plus `cli.py register_cli(subparser)` CLI bridging for `honcho`
- real `42-evey/hermes-plugins` runtime execution for `evey-telemetry` and `evey-status`
- pip entry-point discovery and top-level Hermes CLI command execution through `ctx.register_cli_command()`
- Hermes hub indexing for upstream `plugins/...` directories and `42-evey` repo-root Hermes directories in the plugin browser
- full Hermes `VALID_HOOKS` surface in the CLI runtime: `pre_tool_call`, `post_tool_call`, `pre_llm_call`, `post_llm_call`, `pre_api_request`, `post_api_request`, `on_session_start`, `on_session_end`, `on_session_finalize`, `on_session_reset`
- gateway per-chat session isolation and session-boundary parity proof for `on_session_start`, `on_session_end`, `on_session_finalize`, and `on_session_reset`

---

### Cron Scheduling

Schedule recurring or one-shot tasks:

```bash
edgecrab cron list
edgecrab cron add "0 9 * * 1-5" "Summarize open PRs for standup"
edgecrab cron add "@daily" "Update MEMORY.md with project progress"
edgecrab cron pause <id>
edgecrab cron resume <id>
edgecrab cron remove <id>
edgecrab cron run <id>      # manual trigger
edgecrab cron tick          # process due jobs (called by system cron)
```

Or from within a TUI session:
```
/cron list
/cron add "0 18 * * 5" "Generate weekly summary"
```

The `manage_cron_jobs` tool also lets the agent schedule its own follow-ups autonomously.

---

### Checkpoints & Rollback

Before destructive operations, EdgeCrab creates filesystem snapshots:

```bash
# Manual checkpoint
edgecrab sessions
# → checkpoint auto-created before every file write

# Inside TUI
/rollback                    # restore last checkpoint
/rollback checkpoint-abc123  # restore specific checkpoint
```

Configuration:
```yaml
checkpoints:
  enabled: true
  max_snapshots: 50    # keep last 50 checkpoints per session
```

The `checkpoint` tool is also available to the agent itself — it can snapshot before risky operations and offer rollback if something goes wrong.

---

### Profiles & Worktrees

**Profiles** give EdgeCrab isolated runtime homes with separate `config.yaml`,
`.env`, `SOUL.md`, memories, skills, plugins, hooks, MCP tokens, and
`state.db`. EdgeCrab now seeds three starter profiles by default:
`work`, `research`, and `homelab`.

```bash
edgecrab profile list                # default + bundled starters
edgecrab profile show work
edgecrab profile use work            # sticky default profile
edgecrab -p research "compare SDKs"  # one-shot override
edgecrab profile alias work --name w
edgecrab profile list
```

Starter profile examples:

```yaml
# ~/.edgecrab/profiles/work/config.yaml
model:
  default: "openai/gpt-5"
  max_iterations: 90

display:
  personality: "technical"
  tool_progress: "verbose"
  show_cost: true

reasoning_effort: "high"
```

```yaml
# ~/.edgecrab/profiles/research/config.yaml
model:
  default: "openai/gpt-5"
  max_iterations: 120

display:
  personality: "teacher"

reasoning_effort: "high"
```

In the TUI, `/profile` now mirrors Hermes and shows the active profile name
plus its effective home directory. `/profiles` opens the interactive browser,
and `/profile show <name>` jumps that browser to a specific profile. Inside it: `Enter` switch,
`C` config, `S` SOUL, `M` memory, `T` tools, `A` alias, `E` export,
`D` delete, `N` create, `I` import, `O` rename, `Tab` or `Left`/`Right`
cycle detail views, and `Home`/`End` jump through results. The runtime
switch is live, not deferred to the next launch.

**Worktrees** isolate each agent session in a separate git worktree:

```bash
edgecrab -w "explore that refactor idea safely"
# Creates .worktrees/edgecrab-<id>/ inside the current git repo
# Changes stay isolated on an ephemeral branch until you merge or discard them
```

You can also enable always-on worktree mode in config:

```yaml
# ~/.edgecrab/config.yaml
worktree: true
```

Inside the TUI, `/worktree` opens a report overlay for the current checkout and saved launch policy, and `/worktree on|off|toggle` updates that default for future launches.

`/log` opens a split-pane browser for `~/.edgecrab/logs/`, and `Enter` drills into a per-entry inspector for the selected file tail. The overlay now live-follows by default, `F` toggles follow mode, and `1-5` or `/log level <error|warn|info|debug|trace>` persist the default log verbosity in `config.yaml`; the current process reloads its filter immediately when runtime log reloading is available.

Cleanup is conservative by design: EdgeCrab removes clean disposable worktrees on exit, but keeps worktrees that contain unpushed commits so the agent cannot silently destroy branch-local work.

---

### Vision, TTS & Transcription

```bash
# Vision: analyze an image
edgecrab "What's in this screenshot?" --attach screenshot.png

# TTS: speak the response
edgecrab --quiet "Write a haiku about Rust" | say   # pipe to macOS say
# Or the agent can generate audio directly via text_to_speech tool

# Transcription: send a voice note via WhatsApp gateway
# → EdgeCrab transcribes it with Whisper and responds
```

Vision providers: any multimodal model (Claude, GPT-4o, Gemini).
TTS providers: OpenAI TTS, edge-tts (offline).
Transcription: Whisper (local), Groq Whisper, OpenAI Whisper.

---

## 15 LLM Providers

EdgeCrab ships with 15 LLM providers out of the box (13 cloud, 2 local). Over 200 models are compiled in, with user override via `~/.edgecrab/models.yaml`.

| Provider      | Env Var                          | Notable Models                                    |
| ------------- | -------------------------------- | ------------------------------------------------- |
| `copilot`     | `GITHUB_TOKEN` or VS Code auth cache | `copilot/auto`, GPT-5 mini, GPT-4.1 — routed by GitHub Copilot |
| `openai`      | `OPENAI_API_KEY`                 | GPT-4.1, GPT-5, o3, o4-mini                       |
| `anthropic`   | `ANTHROPIC_API_KEY`              | Claude Opus 4.6, Sonnet 4.6, Haiku 4.5            |
| `google`      | `GOOGLE_API_KEY`                 | Gemini 2.5 Pro, Gemini 2.5 Flash                  |
| `vertexai`    | `GOOGLE_APPLICATION_CREDENTIALS` | Gemini via Google Cloud                           |
| `xai`         | `XAI_API_KEY`                    | Grok 3, Grok 4                                    |
| `deepseek`    | `DEEPSEEK_API_KEY`               | DeepSeek V3, DeepSeek R1                          |
| `mistral`     | `MISTRAL_API_KEY`                | Mistral Large, Mistral Small                      |
| `groq`        | `GROQ_API_KEY`                   | Llama 3.3 70B, Gemma2 9B (blazing fast inference) |
| `huggingface` | `HUGGING_FACE_HUB_TOKEN`         | Any HF Inference API model                        |
| `zai`         | `ZAI_API_KEY`                    | Z.AI / GLM series                                 |
| `openrouter`  | `OPENROUTER_API_KEY`             | 600+ models via one endpoint                      |
| `ollama`      | *(none)*                         | Any model — `ollama serve` on port 11434          |
| `lmstudio`    | *(none)*                         | Any model — LM Studio on port 1234                |

**Switch provider at any time:**
```bash
edgecrab --model openai/gpt-5 "deep code review"
edgecrab --model ollama/llama3.3 "work offline"
edgecrab --model groq/llama-3.3-70b-versatile "quick task"
```

**Hot-swap inside TUI:**
```
/model groq/llama-3.3-70b-versatile
/reasoning high                      # enable extended thinking (Anthropic/OpenAI)
```

**Why `copilot/auto` is now the best default:** GitHub Copilot decides which chat-capable model and billing path are valid for your live session. Following that server choice avoids avoidable model-specific throttles and keeps EdgeCrab aligned with the real VS Code experience.

**Smart routing** (experimental): automatically selects cheap vs full model by turn complexity:
```yaml
model:
  smart_routing:
    enabled: true
    cheap_model: "groq/llama-3.3-70b-versatile"
```

**Mixture of Agents**: Run a single prompt through 4 frontier models simultaneously and get a synthesized consensus:
```
/model moa    # Claude Opus 4.6 + Gemini 2.5 Pro + GPT-4.1 + DeepSeek R1 → aggregated
```

---

## 6 Terminal Backends

The `terminal` tool is pluggable. Select your execution environment:

| Backend             | How to activate                   | Use case                              |
| ------------------- | --------------------------------- | ------------------------------------- |
| **Local** (default) | `EDGECRAB_TERMINAL_BACKEND=local` | Persistent shell on your machine      |
| **Docker**          | `backend: docker`                 | Isolated container per task           |
| **SSH**             | `backend: ssh`                    | Remote server via ControlMaster       |
| **Modal**           | `backend: modal`                  | Cloud sandbox (Modal.com)             |
| **Daytona**         | `backend: daytona`                | Persistent cloud dev sandbox          |
| **Singularity**     | `backend: singularity`            | HPC/Apptainer with persistent overlay |

```yaml
terminal:
  backend: docker
  docker:
    image: "python:3.12-slim"
    container_name: "edgecrab-sandbox"
```

---

## MCP Server Integration

EdgeCrab is a full MCP (Model Context Protocol) client. Connect any MCP server and its tools become available to the agent automatically.

```yaml
# ~/.edgecrab/config.yaml
mcp_servers:
  filesystem:
    command: "npx"
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp/workspace"]

  my-api-server:
    url: "https://my-server.example.com/mcp"
    bearer_token: "${MY_API_TOKEN}"   # env-backed bearer token works
    enabled: true
```

```bash
edgecrab mcp list                                 # show configured MCP servers
edgecrab mcp install filesystem --path "/tmp/ws" # install a curated preset
edgecrab mcp doctor                              # static checks + live probe
edgecrab mcp doctor filesystem                   # diagnose one configured server
edgecrab mcp remove server-name
/mcp                                             # open the TUI MCP browser
/reload-mcp                                      # hot-reload in TUI without restart
```

The agent uses `mcp_list_tools` and `mcp_call_tool` to discover and invoke MCP server capabilities.
The TUI MCP browser supports install, view, test, diagnose, and remove flows, and quoted
`--path` / `name=` values are parsed safely for Unix and Windows-style paths.
HTTP MCP servers that rely on OAuth-style bearer access tokens are supported through
either `bearer_token`, `/mcp-token set <server> <token>`, or env-backed config values such
as `bearer_token: "${MY_API_TOKEN}"`.

---

## ACP / VS Code Copilot Integration

EdgeCrab implements the [Agent Communication Protocol](https://github.com/i-am-bee/acp) — JSON-RPC 2.0 over stdio — enabling it to run as a VS Code Copilot agent, in Zed, JetBrains, and any ACP-compatible runner.

```bash
edgecrab acp           # starts ACP server on stdin/stdout
edgecrab acp init      # scaffold agent.json manifest for a workspace
```

The `acp_registry/agent.json` manifest declares capabilities for extension discovery. The ACP adapter uses a restricted `ACP_TOOLS` subset that excludes interactive-only tools (`clarify`, `send_message`, `text_to_speech`).

---

## ratatui TUI

60 fps capable, GPU-composited full-screen TUI built with [ratatui](https://ratatui.rs/).

**Layout:**
```
┌────────────────────────────────────────────────────────────┐
│  output area (markdown-rendered, mouse-scrollable)          │
│  ⚙  file_read  src/main.rs                                  │
│     → 342 lines read                                        │
│                                                             │
│  The `main` function initializes the agent loop and...      │
├────────────────────────────────────────────────────────────┤
│ ● openai/gpt-5              1,234t  $0.023  [/commands]  │
├────────────────────────────────────────────────────────────┤
│ ❯ Type your message…                                        │
└────────────────────────────────────────────────────────────┘
```

**Features:**
- Streaming output with token-by-token rendering
- Fish-style ghost text (type-ahead) completion
- Tab-complete slash commands with fuzzy match overlay
- Multi-line input (Shift+Enter for newlines)
- Mouse scroll in output area
- Approval dialogs for dangerous operations (inline, non-blocking)
- Clarify dialogs — agent asks questions without blocking the loop
- Secret-request overlays — prompt for missing API keys mid-session
- Session spinner + model name + token count + cost in status bar

**Theme customization** (`~/.edgecrab/skin.yaml`):
```yaml
user_fg:      "#89b4fa"   # Catppuccin blue
assistant_fg: "#a6e3a1"   # Catppuccin green
system_fg:    "#f9e2af"   # Catppuccin yellow
error_fg:     "#f38ba8"   # Catppuccin red
tool_fg:      "#cba6f7"   # Catppuccin mauve
status_bg:    "#313244"
status_fg:    "#cdd6f4"
border_fg:    "#6c7086"
prompt_symbol: "❯"
tool_prefix:   "⚙"
```

---

## All CLI Commands

```bash
# Launch
edgecrab                          # interactive TUI
edgecrab "prompt here"            # TUI + auto-submit
edgecrab --quiet "prompt"         # no banner, pipe-safe output
edgecrab --model p/m "prompt"     # specify LLM
edgecrab --toolset web,file "p"   # restrict toolsets
edgecrab --session id "p"         # use specific session
edgecrab --resume title "p"       # resume by title
edgecrab -C "p"                   # continue last session
edgecrab -w "p"                   # isolated git worktree
edgecrab -S skill1,skill2 "p"     # preload skills

# Setup & diagnostics
edgecrab setup [--section s] [--force]    # interactive wizard
edgecrab doctor                           # full health check
edgecrab version                          # version + providers
edgecrab migrate [--dry-run]              # import hermes-agent state

# Sessions
edgecrab sessions list
edgecrab sessions browse
edgecrab sessions export <id> [--format jsonl]
edgecrab sessions delete <id>
edgecrab sessions rename <id> <title>
edgecrab sessions prune [--older-than 30d]
edgecrab sessions stats

# Configuration
edgecrab config show
edgecrab config edit
edgecrab config path
edgecrab config set <key> <value>

# Tools
edgecrab tools list
edgecrab tools enable <toolset>
edgecrab tools disable <toolset>

# Providers
edgecrab auth list
edgecrab auth status [copilot|provider/<name>|mcp/<server>]
edgecrab auth add copilot --token <github-token>
edgecrab auth add provider/openai --token <api-token>   # writes ~/.edgecrab/.env and ~/.edgecrab/auth.json
edgecrab auth add mcp/<server> --token <bearer-token>
edgecrab auth login [copilot|mcp/<server>]
edgecrab login [target]                                  # defaults to copilot
edgecrab logout [target]                                 # clears local auth cache; provider targets also clear auth.json metadata

# If GitHub Copilot needs a fresh login, EdgeCrab opens a dedicated plain-terminal
# auth screen so the one-time code stays easy to read and easy to select by mouse.
edgecrab mcp list
edgecrab mcp add <name>
edgecrab mcp remove <name>

# Plugins
edgecrab plugins list
edgecrab plugins info <name>
edgecrab plugins status
edgecrab plugins install <source>
edgecrab plugins audit [--lines 20]
edgecrab plugins search <query>
edgecrab plugins search --source hermes <query>
edgecrab plugins browse
edgecrab plugins refresh
edgecrab plugins toggle [name]
edgecrab plugins update [name]
edgecrab plugins remove <name>

# Cron
edgecrab cron list
edgecrab cron add "<schedule>" "<task>"
edgecrab cron run <id>
edgecrab cron tick
edgecrab cron remove <id>
edgecrab cron pause <id>
edgecrab cron resume <id>

# Gateway
edgecrab gateway start [--foreground]
edgecrab gateway stop
edgecrab gateway restart
edgecrab gateway status
edgecrab gateway configure [--platform <name>]
edgecrab webhook subscribe <name> [--events push,pull_request] [--skill code-review] [--deliver github_comment] [--deliver-extra repo=org/repo] [--deliver-extra pr_number=42] [--rate-limit 30] [--max-body-bytes 1048576]
edgecrab webhook list
edgecrab webhook test <name>
edgecrab webhook path
edgecrab whatsapp               # WhatsApp QR pairing wizard
edgecrab status                 # overall gateway status

# Cleanup
edgecrab uninstall --dry-run
edgecrab uninstall --purge-data --yes

# Skills
edgecrab skills list
edgecrab skills view <name>
edgecrab skills search <query>
edgecrab skills install <path|edgecrab:path|owner/repo/path>
edgecrab skills update [name]
edgecrab skills remove <name>

# Profiles
edgecrab profile list
edgecrab profile use <name>
edgecrab profile create <name>
edgecrab profile delete <name>
edgecrab profile show [name]
edgecrab profile alias <name> [--name alias]
edgecrab profile rename <old> <new>
edgecrab profile export <name> [--output path]
edgecrab profile import <path> [--name name]

# ACP
edgecrab acp                    # start ACP stdio server
edgecrab acp init [--workspace] [--force]

# Shell completion
edgecrab completion bash
edgecrab completion zsh
edgecrab completion fish
```

---

## All Slash Commands

Type these inside the TUI (after `❯`):

Every built-in slash command is also reachable from argv with
`edgecrab slash <command...>`.

| Command                                  | Action                                          |
| ---------------------------------------- | ----------------------------------------------- |
| `/help`                                  | List all slash commands with descriptions       |
| `/quit` / `/exit`                        | Exit EdgeCrab                                   |
| `/clear`                                 | Clear the screen and start a fresh session      |
| `/new`                                   | Start a fresh session                           |
| `/model [provider/model]`                | Hot-swap LLM without restart                    |
| `/reasoning [effort]`                    | Set reasoning effort (low/medium/high/auto)     |
| `/retry`                                 | Retry the last message                          |
| `/undo`                                  | Remove the last turn from history               |
| `/stop`                                  | Interrupt current tool execution and generation |
| `/history`                               | Show session message history                    |
| `/save [title]`                          | Save session with a title                       |
| `/export [format]`                       | Export session (jsonl, markdown)                |
| `/title <title>`                         | Rename current session                          |
| `/resume [id-or-title]`                  | Resume a past session                           |
| `/session [list/switch/delete]`          | Manage sessions                                 |
| `/config [show/set]`                     | View or update config                           |
| `/prompt`                                | Show, clear, or set the custom system prompt    |
| `/verbose`                               | Cycle tool progress or set it explicitly        |
| `/personality [preset]`                  | Switch agent personality (14 presets)           |
| `/statusbar`                             | Toggle status bar                               |
| `/log [open\|level <level>]`             | Browse local logs, live-follow tails, and set the saved log level |
| `/worktree [status\|on\|off\|toggle]`    | Show current git checkout status and saved worktree launch policy |
| `/tools`                                 | List active toolsets and tools                  |
| `/toolsets`                              | Show toolset aliases and expansions             |
| `/mcp [subcommand]`                      | Browse, install, test, diagnose, or remove MCP servers |
| `/reload-mcp`                            | Hot-reload MCP servers (no restart needed)      |
| `/mcp-token <server> <token>`            | Set MCP bearer token at runtime                 |
| `/plugins [info/status/install/enable/disable/toggle/audit/hub]` | Browse installed plugins and manage plugin actions |
| `/memory [show/edit]`                    | View or edit agent memory                       |
| `/cost`                                  | Show token costs for this session               |
| `/usage`                                 | Detailed usage breakdown                        |
| `/compress`                              | Force context compression now                   |
| `/insights [days]`                       | Show session statistics and N-day historical analytics |
| `/skin [preset]`                         | Browse or switch skins (`/theme` alias)         |
| `/paste`                                 | Toggle paste mode (multi-line clipboard input)  |
| `/queue <message>`                       | Queue a message while agent is running          |
| `/background`                            | Fork current task to background, free the TUI   |
| `/rollback [checkpoint]`                 | Restore filesystem to a checkpoint              |
| `/platforms`                             | Show connected gateway platforms                |
| `/approve`                               | Approve a pending agent action                  |
| `/deny`                                  | Deny a pending agent action                     |
| `/sethome`                               | Configure gateway home channel                  |
| `/update`                                | Check for EdgeCrab updates                      |
| `/cron [list/add/remove]`                | Manage cron jobs inline                         |
| `/voice <on/off/status>`                 | Toggle voice output                             |
| `/skills [list/view/install/remove/hub]` | Manage skills                                   |
| `/doctor`                                | Run inline health diagnostics                   |
| `/version`                               | Show version and provider info                  |

Keyboard shortcuts:

| Key                      | Action                                              |
| ------------------------ | --------------------------------------------------- |
| `Enter`                  | Submit prompt                                       |
| `Shift+Enter`            | New line in input                                   |
| `Ctrl+C`                 | Interrupt running agent                             |
| `Ctrl+L`                 | Clear output area                                   |
| `Ctrl+U`                 | Clear input line                                    |
| `Ctrl+B` / `Ctrl+F`      | Fallback page up/down when the terminal swallows PgUp/PgDn |
| `Alt+↑` / `Alt+↓`        | Scroll output                                       |
| `Ctrl+Home` / `Ctrl+End` | Jump to top/bottom of output                        |
| `Tab`                    | Accept ghost text / cycle slash command completions |

Terminal troubleshooting:

- If `PgUp` / `PgDn` do not reach EdgeCrab, use `Ctrl+B` / `Ctrl+F`.
- On macOS Terminal.app, EdgeCrab now starts in a conservative compatibility mode: mouse capture is off by default and the fallback paging keys are enabled automatically.
- You can force that mode in any terminal with `EDGECRAB_TUI_COMPAT=1 edgecrab`.

---

## Security Model

Security is compiled in — not an afterthought. EdgeCrab applies defense-in-depth at seven independent layers:

| Layer                      | Mechanism                                                                                                                                                     | Where                             |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------- |
| **File I/O**               | All paths canonicalized, checked against `allowed_roots`. `SanitizedPath` is a distinct Rust type — bypassing it is a compile error.                          | `edgecrab-security::path_safety`  |
| **Web tools**              | SSRF guard blocks private IP ranges (10.x, 192.168.x, 172.16.x, 127.x, ::1) before any outbound HTTP call. `SafeUrl` distinct type.                           | `edgecrab-security::ssrf`         |
| **Terminal**               | Command injection scan (Aho-Corasick + regex) over 8 danger categories rejects shell metacharacters and forbidden patterns.                                   | `edgecrab-security::command_scan` |
| **Context files**          | Prompt injection patterns (regex + invisible Unicode + homoglyphs) scanned in SOUL.md, AGENTS.md, .cursor/rules. High-severity blocked with `[BLOCKED: ...]`. | `prompt_builder.rs`               |
| **Code execution sandbox** | API keys/tokens stripped from child env. Only 7 whitelisted tool stubs exposed via Unix socket RPC. `SIGTERM→SIGKILL` escalation on timeout.                  | `execute_code.rs`                 |
| **Skills installation**    | External skills run through a 23-pattern threat scanner (exfiltration, injection, destructive ops, persistence, obfuscation) before install.                  | `skills_guard`                    |
| **LLM output**             | Redaction pipeline strips secrets and tokens before displaying or logging any LLM response.                                                                   | `edgecrab-security::redact`       |

Path safety and SSRF use Rust's **type system** as the primary control — not runtime checks alone. If your code doesn't have a `SanitizedPath`, it can't call file I/O. Period.

---

## Architecture

EdgeCrab is an 11-crate Rust workspace. The dependency graph is a strict DAG — no circular dependencies, no feature flags that reverse the graph.

```
edgecrab-types      (shared types — no deps on other crates)
       ↑
edgecrab-security   (path safety, SSRF, cmd scan — types only)
edgecrab-cron       (standalone cron store + schedule parser)
       ↑
edgecrab-tools      (ToolRegistry + built-in tool implementations)
edgecrab-lsp        (language-server client, document sync, semantic tools)
edgecrab-state      (SQLite WAL + FTS5 session store)
       ↑
edgecrab-core       (Agent, ReAct loop, prompt builder, compression)
       ↑
edgecrab-cli    edgecrab-gateway    edgecrab-acp    edgecrab-migrate
```

| Crate               | Responsibility                                                                                                               |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `edgecrab-types`    | `Message`, `Role`, `ToolCall`, `ToolSchema`, `Usage`, `Cost`, `AgentError`, `Trajectory` — all shared with no business logic |
| `edgecrab-security` | Path jail, SSRF, command scan, redaction, approval engine                                                                    |
| `edgecrab-state`    | SQLite WAL + FTS5 session storage (`~/.edgecrab/state.db`)                                                                   |
| `edgecrab-cron`     | Cron expression parser, job store (`~/.edgecrab/cron/`)                                                                      |
| `edgecrab-tools`    | `ToolRegistry`, `ToolHandler` trait, `ToolContext`, the built-in tool surface including browser, MCP, media, and LSP       |
| `edgecrab-lsp`      | Language-server manager, JSON-RPC client, document sync, diagnostics, edit application, and `lsp_*` tool handlers          |
| `edgecrab-core`     | `Agent`, `AgentBuilder`, `execute_loop()`, `PromptBuilder`, compression, routing, 200+ model catalog                         |
| `edgecrab-cli`      | ratatui TUI, 42 slash commands, all CLI subcommands, skin engine, profiles                                                   |
| `edgecrab-gateway`  | axum HTTP + 15 platform adapters, streaming delivery, `MEDIA://` protocol                                                    |
| `edgecrab-acp`      | ACP JSON-RPC 2.0 stdio adapter for VS Code / Zed / JetBrains                                                                 |
| `edgecrab-migrate`  | hermes-agent → EdgeCrab state import, schema migrations                                                                      |

**Key design decisions (from the code):**

1. **Single binary** — Static linking embeds all deps (TLS, SQLite, Aho-Corasick). No shared libraries except OS.
2. **Type-level security** — `SanitizedPath` and `SafeUrl` are distinct types in `edgecrab-types`. Bypassing sanitization is a compile error.
3. **Compile-time tool registry** — `inventory::submit!()` registers tools at link time. Zero startup cost. All tools present or absent by feature flag, not runtime config.
4. **Single system prompt per session** — Built once, cached in `SessionState.cached_system_prompt`. Compression never rebuilds it (preserves Anthropic prompt cache hits).
5. **Hot-swappable model** — `RwLock<Arc<dyn LLMProvider>>` in `Agent`. In-flight conversations keep their Arc clone; swap affects only new turns.

---

## Configuration

EdgeCrab uses layered config: `defaults → ~/.edgecrab/config.yaml → EDGECRAB_* env vars → CLI flags`. Later layers win.

```yaml
# ~/.edgecrab/config.yaml

model:
  default_model: "ollama/gemma4:latest"
  max_iterations: 90          # ReAct loop budget per session
  streaming: true
  smart_routing:
    enabled: false
    cheap_model: ""

display:
  skin: "catppuccin"
  show_reasoning: false

logging:
  level: "info"              # error | warn | info | debug | trace

worktree: false               # true = launch agent sessions in isolated git worktrees by default

tools:
  enabled_toolsets: null       # null = all toolsets active
  disabled_toolsets: null
  file:
    allowed_roots: []          # empty = cwd only
  custom_groups:
    backend-dev:
      - read_file
      - write_file
      - terminal
      - session_search

lsp:
  enabled: true
  file_size_limit_bytes: 10000000
  servers:
    rust:
      command: "rust-analyzer"
      args: []
      file_extensions: ["rs"]
      language_id: "rust"
      root_markers: ["Cargo.toml", "rust-project.json"]

memory:
  enabled: true

skills:
  enabled: true
  preloaded: []

delegation:
  enabled: true
  model: null                  # null = use default model
  max_subagents: 3
  max_iterations: 50

terminal:
  backend: local               # local | docker | ssh | modal | daytona | singularity
  docker:
    image: "ubuntu:22.04"

browser:
  record_sessions: false

checkpoints:
  enabled: true
  max_snapshots: 50

gateway:
  host: "0.0.0.0"
  port: 8642
  enabled_platforms: []        # ["telegram", "discord", ...]
  whatsapp:
    enabled: false
    mode: "self-chat"          # self-chat | any-sender
    allowed_users: []

security:
  path_restrictions: []

mcp_servers:
  my-server:
    command: "npx"
    args: ["-y", "@modelcontextprotocol/server-example"]
    enabled: true
```

Key environment variables:
```bash
EDGECRAB_MODEL=openai/gpt-5
EDGECRAB_MAX_ITERATIONS=120
EDGECRAB_TERMINAL_BACKEND=docker
EDGECRAB_SKIP_MEMORY=false
EDGECRAB_SAVE_TRAJECTORIES=true
```

---

## SDKs: one EdgeCrab experience

EdgeCrab ships first-class SDK surfaces for Rust, Python, Node.js, and WASM.
The published package names stay simple — `edgecrab`, `edgecrab-sdk`, and `@edgecrab/wasm`.
The canonical Python SDK now lives directly under `sdks/python` for release and publication.

### Python SDK (`edgecrab`)

**Python 3.10+ — async-first, streaming, sessions, and E2E-backed examples.**

```bash
pip install edgecrab
```

```python
from edgecrab import Agent

# Simple chat
agent = Agent(model="openai/gpt-4o")
reply = agent.chat("Explain Rust ownership in 3 sentences")
print(reply)

# Async streaming
import asyncio
from edgecrab import AsyncAgent

async def main():
    agent = AsyncAgent(model="copilot/gpt-5-mini")
    async for token in agent.stream("Write a Rust hello-world"):
        print(token, end="", flush=True)

asyncio.run(main())
```

Built-in CLI:
```bash
edgecrab chat "Hello, EdgeCrab!"
edgecrab models
edgecrab health
```

Full docs: [Python SDK README](sdks/python/README.md)

### Node.js SDK (`edgecrab`)

**Node 18+ — TypeScript-first, streaming, and native runtime access.**

```bash
npm install edgecrab
```

```typescript
import { Agent } from 'edgecrab';

// Simple chat
const agent = new Agent({ model: 'openai/gpt-4o' });
const reply = await agent.chat('Explain Rust ownership');
console.log(reply);

// Streaming
for await (const token of agent.stream('Write a README')) {
  process.stdout.write(token);
}
```

CLI via npx:
```bash
npx edgecrab chat "Hello!"
npx edgecrab models
```

Full docs: [Node.js SDK README](sdks/nodejs-native/README.md)

---

## Docker

Run EdgeCrab as a gateway server in a container:

```bash
# Pull multi-arch GHCR image
docker pull ghcr.io/raphaelmansuy/edgecrab:latest

# Run gateway server
docker run -p 8642:8642 \
  -e ANTHROPIC_API_KEY="$ANTHROPIC_API_KEY" \
  -e TELEGRAM_BOT_TOKEN="$TELEGRAM_BOT_TOKEN" \
  -v "$HOME/.edgecrab:/root/.edgecrab" \
  ghcr.io/raphaelmansuy/edgecrab:latest

# Or with docker-compose
docker compose up -d
```

The Docker image is multi-stage, ~50 MB (distroless final stage). Multi-arch: `linux/amd64` + `linux/arm64`. Uses `rustls-tls` — no OpenSSL dependency for clean cross-compilation.

---

## Migrating from hermes-agent

EdgeCrab imports your entire hermes-agent state in one command:

```bash
# Preview first (no changes made)
edgecrab migrate --dry-run

# Live migration
edgecrab migrate

# OpenClaw import
edgecrab claw migrate --dry-run
edgecrab claw migrate
```

| What        | From                    | To                        |
| ----------- | ----------------------- | ------------------------- |
| Config      | `~/.hermes/config.yaml` | `~/.edgecrab/config.yaml` |
| Memories    | `~/.hermes/memories/`   | `~/.edgecrab/memories/`   |
| Skills      | `~/.hermes/skills/`     | `~/.edgecrab/skills/`     |
| Environment | `~/.hermes/.env`        | `~/.edgecrab/.env`        |

The migrator is in `crates/edgecrab-migrate/`. It returns a `MigrationReport` with per-item `MigrationStatus` (Success/Skipped/Failed). Config format differences are handled automatically.

For OpenClaw, EdgeCrab imports the parts that map cleanly into EdgeCrab-native
state (`SOUL.md`, memories, skills, selected `.env` keys, selected config
sections) and archives unsupported OpenClaw-only config under
`~/.edgecrab/migration/openclaw/` for manual review.

---

## Testing

```bash
# Root convenience target
cargo run

# Run all unit + integration tests
cargo test --workspace

# Run only a specific crate
cargo test -p edgecrab-core
cargo test -p edgecrab-tools
cargo test -p edgecrab-gateway

# Run E2E tests (requires a configured LLM provider)
cargo test --workspace -- --include-ignored

# Lint (zero warnings policy)
cargo clippy --workspace -- -D warnings

# Format check
cargo fmt --check

# Build documentation
cargo doc --no-deps --open
```

Current: **1629 tests passing** (unit + integration). The codebase has a zero-clippy-warnings policy enforced in CI.

> **Note:** 8 gap-audit tests in `edgecrab-cli` require the hermes-agent source tree at `../hermes-agent/`. Skip them when developing standalone: `cargo test --workspace --exclude edgecrab-cli`

---

## Project Structure

```
edgecrab/
├── crates/
│   ├── edgecrab-types/         Shared types — Message, Role, ToolCall, errors
│   ├── edgecrab-security/      Path jail, SSRF, cmd scanner, injection, redact
│   ├── edgecrab-state/         SQLite WAL + FTS5 session store
│   ├── edgecrab-cron/          Cron parser, job store, scheduler
│   ├── edgecrab-tools/         ToolRegistry + built-in tool implementations
│   │   └── tools/
│   │       ├── file.rs         read_file, write_file, patch_file, search_files
│   │       ├── terminal.rs     terminal, manage_process
│   │       ├── web.rs          web_search, web_extract
│   │       ├── browser.rs      CDP browser automation (6 tools)
│   │       ├── memory.rs       memory_read, memory_write, Honcho tools
│   │       ├── delegate_task.rs Sub-agent delegation + batch parallelism
│   │       ├── execute_code.rs Sandboxed multi-language code execution
│   │       ├── vision.rs       vision_analyze, text_to_speech, transcribe_audio
│   │       └── ...             session, cron, checkpoint, skills, mcp, todo, HA
│   ├── edgecrab-lsp/           LSP client, document sync, diagnostics, semantic edits
│   ├── edgecrab-core/
│   │   └── src/
│   │       ├── agent.rs        AgentBuilder, Agent, StreamEvent, fork_isolated
│   │       ├── conversation.rs execute_loop() — the ReAct engine
│   │       ├── compression.rs  Context window compression
│   │       ├── prompt_builder.rs System prompt assembly from 9+ sources
│   │       ├── model_router.rs Smart routing (cheap vs full model)
│   │       └── model_catalog.rs 200+ models, user-overridable YAML
│   ├── edgecrab-cli/           ratatui TUI, slash commands, skin engine, profiles
│   ├── edgecrab-gateway/       axum + 15 platform adapters, streaming delivery
│   ├── edgecrab-acp/           ACP JSON-RPC 2.0 stdio adapter
│   └── edgecrab-migrate/       hermes-agent import + schema migrations
├── sdks/
│   ├── python/                 Python SDK (edgecrab on PyPI)
│   └── node/                   Node.js SDK (edgecrab-sdk on npm)
├── site/                       Astro documentation website
├── docs/                       Specification documents
├── acp_registry/
│   └── agent.json              VS Code Copilot agent manifest
├── .github/workflows/          CI + 4 release workflows (Rust/Python/Node/Docker)
├── Dockerfile                  Multi-stage, distroless, multi-arch
└── docker-compose.yml          One-command gateway deployment
```

---

## Requirements & Build

| Tool  | Version               |
| ----- | --------------------- |
| Rust  | 1.86+                 |
| Cargo | bundled with Rust     |
| OS    | macOS, Linux, Windows |

```bash
# Debug build (fast iteration)
cargo build --workspace

# Release build (optimized, ~3× faster startup than debug)
cargo build --workspace --release

# Cross-compile for Linux on macOS
cargo build --release --target x86_64-unknown-linux-musl
```

The release binary is statically linked — no OpenSSL, no libc versions to worry about. Drop it on any Linux box and it runs.

---

## Contributing

EdgeCrab welcomes contributions. The codebase has a zero-clippy-warnings policy and enforces `cargo fmt`.

```bash
git clone https://github.com/raphaelmansuy/edgecrab
cd edgecrab
cargo build --workspace                    # verify it compiles
cargo test --workspace                     # run test suite
cargo clippy --workspace -- -D warnings    # must be warning-free
```

**Adding a new tool:**
1. Create `crates/edgecrab-tools/src/tools/my_tool.rs`
2. Implement the `ToolHandler` trait (name, schema, execute, toolset, emoji)
3. Register with `inventory::submit!(RegisteredTool { handler: &MyTool })`
4. Declare in `crates/edgecrab-tools/src/tools/mod.rs`

**Adding a new gateway:**
1. Create `crates/edgecrab-gateway/src/my_platform.rs`
2. Implement `PlatformAdapter` trait
3. Register in `crates/edgecrab-gateway/src/run.rs`

**Security reporting:** `security@elitizon.com`

See [CONTRIBUTING.md](CONTRIBUTING.md) for full details.

---

## Release Channels

| Channel        | Artifact                                           | Install                                                             |
| -------------- | -------------------------------------------------- | ------------------------------------------------------------------- |
| **npm**        | `edgecrab-cli` (binary wrapper — no Rust required) | `npm install -g edgecrab-cli`                                       |
| **pip**        | `edgecrab-cli` (binary wrapper — no Rust required) | `pip install edgecrab-cli`                                          |
| **cargo**      | Rust crates (12 crates published)                  | `cargo install edgecrab-cli`                                        |
| **Python SDK** | `edgecrab`                                         | `pip install edgecrab`                                              |
| **Node SDK**   | `edgecrab-sdk`                                     | `npm install edgecrab-sdk`                                          |
| **Docker**     | GHCR multi-arch                                    | `docker pull ghcr.io/raphaelmansuy/edgecrab:latest`                 |
| **Binary**     | GitHub Release archives                            | [Releases page](https://github.com/raphaelmansuy/edgecrab/releases) |

Release automation: `.github/workflows/release-rust.yml`, `release-python.yml`, `release-node.yml`, `release-docker.yml`.

---

## License

Apache-2.0 — see [LICENSE](LICENSE).

Built by [Elitizon](https://elitizon.com) · inspired by [Nous Hermes Agent](https://github.com/NousResearch) and [OpenClaw](https://github.com/openclaw).
