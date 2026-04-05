#!/usr/bin/env python3
"""Write docs batch 3b: features/tools, memory, context-files, cron, browser"""
import os

BASE = "src/content/docs"

# ─── features/tools.md ────────────────────────────────────────────────
tools_md = r"""---
title: Tools & Toolsets
description: All 60+ EdgeCrab tools organized into toolsets, custom groups, toolset aliases, and runtime gating. Grounded in crates/edgecrab-tools/src/toolsets.rs and the tool registry.
sidebar:
  order: 4
---

Tools are the atomic actions EdgeCrab can perform. They are organized into **toolsets** (named groups) and activated via **aliases** in config or CLI.

---

## Enabling and Disabling Tools

### Via CLI

```bash
edgecrab --toolset coding "implement the feature"
edgecrab --toolset file,terminal "run tests"
edgecrab --toolset all "maximum capability"
edgecrab --toolset minimal "safe mode"
```

### Via config

```yaml
tools:
  enabled_toolsets:              # only these toolsets active (null = all)
    - coding
    - memory
  disabled_toolsets:             # remove from enabled set
    - browser
```

### Custom Groups

Define your own toolset alias in config:

```yaml
tools:
  custom_groups:
    my-research:
      - web_search
      - web_extract
      - read_file
```

Then use `--toolset my-research` on the CLI.

---

## Built-in Toolset Aliases

| Alias | Expands to |
|-------|-----------|
| `core` | file + meta + scheduling + delegation + code_execution + session + mcp + browser |
| `coding` | file + terminal + search (`search_files`) + code_execution |
| `research` | web + browser + vision |
| `debugging` | terminal + web + file |
| `safe` | web + vision + image_gen + moa |
| `minimal` | file + terminal |
| `data_gen` | file + terminal + web + code_execution |
| `all` | every tool (no filtering) |

---

## Toolset Reference

### `file` — File Manipulation
| Tool | Description |
|------|-------------|
| `read_file` | Read file contents with optional `start_line`/`end_line` range |
| `write_file` | Write or overwrite a file (creates parent dirs automatically) |
| `patch` | Apply a unified diff patch to a file |
| `search_files` | Regex or glob search across a directory tree |

### `terminal` — Process Management
| Tool | Description |
|------|-------------|
| `terminal` | Run a shell command, return stdout/stderr and exit code |
| `run_process` | Start a background process, returns a process ID |
| `list_processes` | List all background processes started this session |
| `kill_process` | Send SIGTERM/SIGKILL to a background process |
| `get_process_output` | Read stdout/stderr from a running background process |
| `wait_for_process` | Block until a background process exits |
| `write_stdin` | Send bytes to a process's stdin |

### `web` — Web Access
| Tool | Description |
|------|-------------|
| `web_search` | Search DuckDuckGo, returns structured results |
| `web_extract` | Fetch a URL and extract readable text (readability algorithm) |
| `web_crawl` | Recursively crawl a site up to a specified depth |

### `browser` — Browser Automation
| Tool | Description |
|------|-------------|
| `browser_navigate` | Navigate to a URL in a CDP-connected Chrome instance |
| `browser_snapshot` | Get page accessibility tree as structured text |
| `browser_screenshot` | Take a full-page or viewport screenshot |
| `browser_click` | Click an element by CSS selector or text |
| `browser_type` | Type text into a focused input |
| `browser_scroll` | Scroll the page in any direction |
| `browser_console` | Return buffered console.log/warn/error messages |
| `browser_back` | Navigate back in browser history |
| `browser_press` | Press a keyboard key (Enter, Tab, Escape, etc.) |
| `browser_close` | Close the browser and release CDP connection |
| `browser_get_images` | Return base64-encoded images currently visible |
| `browser_vision` | Screenshot + analyze with vision model |

### `memory` — Persistent Memory
| Tool | Description |
|------|-------------|
| `memory_read` | Read a memory file from `~/.edgecrab/memories/` |
| `memory_write` | Write or update a memory file |
| `honcho_conclude` | Commit a Honcho memory entry at end of session |
| `honcho_search` | Semantic search across Honcho user model |
| `honcho_list` | List Honcho memory entries |
| `honcho_remove` | Remove a specific Honcho entry |
| `honcho_profile` | Update the Honcho user profile summary |
| `honcho_context` | Retrieve relevant Honcho context for current task |

### `skills` — Skills Management
| Tool | Description |
|------|-------------|
| `skills_list` | List all installed skills |
| `skills_categories` | List skill categories |
| `skill_view` | Read a skill's `SKILL.md` content |
| `skill_manage` | Install/uninstall/update skills |
| `skills_hub` | Search and browse the public skills hub |

### `meta` — Planning & Interaction
| Tool | Description |
|------|-------------|
| `manage_todo_list` | Create and manage a todo list for the current task |
| `clarify` | Ask the user a clarifying question before proceeding |

### `scheduling` — Cron Jobs
| Tool | Description |
|------|-------------|
| `manage_cron_jobs` | Create, list, enable, disable, and delete cron jobs |

### `delegation` — Subagents
| Tool | Description |
|------|-------------|
| `delegate_task` | Spawn a subagent to complete a parallel subtask |
| `mixture_of_agents` | Run a task through multiple models, synthesize consensus |

### `code_execution` — Sandboxed Execution
| Tool | Description |
|------|-------------|
| `execute_code` | Execute Python, Node.js, or Bash code in an isolated sandbox |

### `session` — History Search
| Tool | Description |
|------|-------------|
| `session_search` | Full-text FTS5 search across all past session messages |

### `mcp` — MCP Server Integration
| Tool | Description |
|------|-------------|
| `mcp_list_tools` | List all tools exposed by connected MCP servers |
| `mcp_call_tool` | Call a named tool on an MCP server |
| `mcp_list_resources` | List resources from MCP servers |
| `mcp_read_resource` | Read a resource from an MCP server |
| `mcp_list_prompts` | List prompts from MCP servers |
| `mcp_get_prompt` | Retrieve and expand an MCP prompt |

### `media` — Vision, TTS, STT
| Tool | Description |
|------|-------------|
| `text_to_speech` | Convert text to audio (edge-tts, OpenAI, ElevenLabs) |
| `vision_analyze` | Analyze an image file with the configured vision model |
| `transcribe_audio` | Transcribe audio with Whisper (local) or Groq/OpenAI |
| `generate_image` | Generate an image via DALL-E or compatible API |

### `core` — Checkpoints
| Tool | Description |
|------|-------------|
| `checkpoint` | Create/list/restore/diff filesystem checkpoints |

---

## Runtime-Gated Tools

Some tools are always present in the toolset but silently unavailable if their runtime dependency is missing:

| Tool | Gated by |
|------|----------|
| `browser_*` | Chrome/Chromium binary or `CDP_URL` env var |
| `text_to_speech` | `edge-tts` binary, OpenAI key, or ElevenLabs key |
| `transcribe_audio` | `whisper-rs` or Groq/OpenAI key |
| `generate_image` | Image generation API key |
| `ha_*` (Home Assistant) | `HA_URL` + `HA_TOKEN` env vars |

When a gated tool is called without its dependency, EdgeCrab returns a structured error explaining what's missing.

---

## Tool Delay

Set `tools.tool_delay` to add a pause between consecutive tool calls. Useful for rate-limited APIs:

```yaml
tools:
  tool_delay: 2.0           # 2 seconds between tool calls
  parallel_execution: false  # disable parallel calls for strict ordering
```
"""

with open(f"{BASE}/features/tools.md", "w") as f:
    f.write(tools_md)
print("features/tools.md written")

# ─── features/memory.md ───────────────────────────────────────────────
memory_md = r"""---
title: Memory
description: Persistent memory system — file-based memory in ~/.edgecrab/memories/ plus Honcho cross-session user modeling. Grounded in crates/edgecrab-tools/src/tools/memory.rs and config.rs.
sidebar:
  order: 6
---

EdgeCrab has two complementary memory layers: file-based session memory and Honcho user modeling.

---

## File-Based Memory

Memory files live in `~/.edgecrab/memories/` (or profile-specific if using profiles). They are plain Markdown files that persist between sessions.

### Auto-Flush

When `memory.auto_flush: true` (the default), EdgeCrab automatically saves all new memory at the end of each session.

```yaml
memory:
  enabled: true
  auto_flush: true
```

Disable for a single session:

```bash
edgecrab --skip-memory "don't remember this session"
```

### Memory Tools

The agent uses these tools to manage memory during a session:

| Tool | Description |
|------|-------------|
| `memory_read` | Read a memory file by name |
| `memory_write` | Write or update a memory file |

Example agent interaction:

```
❯ Remember that this project uses PostgreSQL 16 with the pgvector extension
```

The agent writes a memory entry. Next session, it reads and injects `memories/*.md` into the system prompt.

### Session Injection

At the start of each session, EdgeCrab injects all memory files under `Persistent Memory` in the system prompt. This is exactly the same injection that Hermes Agent does.

---

## Honcho User Modeling

Honcho provides cross-session, semantic user modeling. It stores facts about the user's working patterns, preferences, and context — then retrieves the most relevant facts at the start of each session.

### Configuration

```yaml
honcho:
  enabled: true                  # master switch
  cloud_sync: false              # sync to Honcho cloud (requires HONCHO_API_KEY)
  api_key_env: "HONCHO_API_KEY"
  api_url: "https://api.honcho.dev/v1"
  max_context_entries: 10        # facts injected per session
  write_frequency: 0             # auto-conclude every N messages (0 = manual)
```

Enable cloud sync:

```bash
export HONCHO_API_KEY=sk-honcho-xxx
# Setting the key auto-enables cloud_sync
```

### Honcho Tools

| Tool | Description |
|------|-------------|
| `honcho_conclude` | Commit a structured memory entry (agent calls at session end) |
| `honcho_search` | Semantic search across Honcho user model |
| `honcho_list` | List stored Honcho entries |
| `honcho_remove` | Delete a specific entry |
| `honcho_profile` | Update user profile summary |
| `honcho_context` | Retrieve top-K relevant entries for the current task |

### How It Works

1. **During session:** Honcho context is injected into the system prompt via `honcho_context`
2. **End of session:** Agent calls `honcho_conclude` with a summary of what it learned
3. **Next session:** Top-K relevant entries are retrieved and injected

All Honcho storage is local by default (`cloud_sync: false`). With cloud sync enabled, facts are stored and retrieved from the Honcho API.

---

## Disabling Memory

```bash
# Skip memory for one session
edgecrab --skip-memory "ephemeral task"

# Disable globally
# config.yaml
memory:
  enabled: false

# Disable Honcho
honcho:
  enabled: false
```

---

## The `/memory` Slash Command

From the TUI:

```
/memory           # show all memory files and their sizes
```

This opens a summary view of `~/.edgecrab/memories/` with file names, sizes, and last-modified times.
"""

with open(f"{BASE}/features/memory.md", "w") as f:
    f.write(memory_md)
print("features/memory.md written")

# ─── features/context-files.md ────────────────────────────────────────
context_files_md = r"""---
title: Context Files
description: How EdgeCrab auto-loads SOUL.md, AGENTS.md, and other context files into the system prompt. Grounded in crates/edgecrab-core/src/context.rs.
sidebar:
  order: 7
---

Context files tell EdgeCrab who it is and how to behave. They are injected into the system prompt at session start — before any user message.

---

## How Context Loading Works

At each session start, EdgeCrab scans the following paths in order and injects any files it finds:

1. `~/.edgecrab/SOUL.md` — Global agent identity
2. `~/.edgecrab/AGENTS.md` — Global project-agnostic instructions  
3. `AGENTS.md` in the current working directory — Project-specific instructions
4. `AGENTS.md` traversed up from CWD (like git, stops at filesystem root)
5. All files in `~/.edgecrab/memories/` — Persistent memory

### Injection Order (in system prompt)

```
[1] SOUL.md (identity/persona)
[2] ~/.edgecrab/AGENTS.md (global instructions)
[3] ./AGENTS.md (project instructions, if present)
[4] Memory files (persistent facts)
```

---

## SOUL.md — Agent Identity

`~/.edgecrab/SOUL.md` defines the agent's core personality and directives. It is the first thing in every system prompt.

Example:

```markdown
# EdgeCrab Agent Identity

You are an expert Rust and TypeScript software engineer.
You write clean, idiomatic, well-tested code.
You always run tests before declaring a task complete.
You prefer explicit error handling over panics.
You explain your reasoning concisely — no filler text.
```

Edit it:

```bash
edgecrab config edit-soul      # opens SOUL.md in $EDITOR
# or directly:
$EDITOR ~/.edgecrab/SOUL.md
```

---

## AGENTS.md — Project Instructions

Place `AGENTS.md` in your project root to give EdgeCrab project-specific context:

```markdown
# Project: my-rust-api

## Build
cargo build --workspace

## Test
cargo test --workspace -- --nocapture

## Code Style
- All public APIs must have doc comments
- Use `thiserror` for error types
- Prefer `Arc<Mutex<T>>` over raw pointers

## Architecture
Services are in `crates/*/`, shared types in `crates/types/`.
The HTTP layer uses Axum 0.8.
```

EdgeCrab reads this automatically when it starts in or navigates into `my-rust-api/`.

---

## Skipping Context Files

Disable for one session:

```bash
edgecrab --skip-context-files "ignore SOUL.md and AGENTS.md"
```

Or via environment variable:

```bash
EDGECRAB_SKIP_CONTEXT_FILES=1 edgecrab "task"
```

---

## Profile Context Files

Each [profile](/user-guide/profiles/) has its own `SOUL.md`:

```
~/.edgecrab/profiles/work/SOUL.md
~/.edgecrab/profiles/personal/SOUL.md
```

Profile files override the global `~/.edgecrab/SOUL.md` when the profile is active.
"""

with open(f"{BASE}/features/context-files.md", "w") as f:
    f.write(context_files_md)
print("features/context-files.md written")

# ─── features/cron.md ─────────────────────────────────────────────────
cron_md = r"""---
title: Cron Jobs
description: Schedule recurring EdgeCrab agent tasks with the built-in cron engine. Grounded in crates/edgecrab-cron/src/lib.rs and the manage_cron_jobs tool.
sidebar:
  order: 8
---

EdgeCrab includes a built-in cron scheduler. Scheduled jobs run the EdgeCrab agent on a recurring schedule — the same agent loop, same tools, same config.

---

## Cron Syntax

EdgeCrab uses standard 5-field cron syntax:

```
┌──── minute (0-59)
│ ┌──── hour (0-23)
│ │ ┌──── day of month (1-31)
│ │ │ ┌──── month (1-12)
│ │ │ │ ┌──── day of week (0-7, 0/7=Sunday)
│ │ │ │ │
* * * * *
```

Examples:

```
0 9 * * 1-5          # 9 AM weekdays
0 */4 * * *          # every 4 hours
0 0 1 * *            # midnight on 1st of each month
*/15 * * * *         # every 15 minutes
```

---

## Managing Cron Jobs

### CLI

```bash
# List all jobs
edgecrab cron list

# Add a job
edgecrab cron add "0 9 * * 1-5" "summarize overnight GitHub activity"

# Add with a name
edgecrab cron add --name morning-standup "0 9 * * 1-5" "morning standup summary"

# Enable/disable
edgecrab cron enable morning-standup
edgecrab cron disable morning-standup

# Delete
edgecrab cron delete morning-standup

# Run now (one-shot)
edgecrab cron run morning-standup
```

### Agent Tool (`manage_cron_jobs`)

The agent can manage its own schedule during a session:

```
❯ Schedule a daily report at 6 PM that summarizes today's git commits
```

The agent calls `manage_cron_jobs` with:

```json
{
  "action": "create",
  "name": "daily-git-report",
  "schedule": "0 18 * * *",
  "task": "Summarize today's git commits across all repositories in ~/projects/"
}
```

---

## Cron Storage

Jobs are stored in `~/.edgecrab/cron/`:

```
~/.edgecrab/cron/
├── morning-standup.json
├── daily-git-report.json
└── weekly-review.json
```

Each file is a JSON descriptor:

```json
{
  "name": "morning-standup",
  "schedule": "0 9 * * 1-5",
  "task": "Summarize overnight GitHub notifications and Slack messages",
  "enabled": true,
  "model": null,
  "toolsets": null,
  "last_run": "2025-05-14T09:00:00Z",
  "next_run": "2025-05-15T09:00:00Z"
}
```

---

## Cron with Gateway

When the gateway is running, cron job results can be sent to a messaging platform:

```yaml
gateway:
  telegram:
    home_channel: "-100123456789"  # results sent here
```

Set the home channel from the TUI:

```
/sethome
```

---

## Timezone

Cron schedules respect the `timezone` config key:

```yaml
timezone: "America/New_York"
```

Override with env var:

```bash
EDGECRAB_TIMEZONE=Europe/London edgecrab cron list
```

---

## The `/cron` Slash Command

From the TUI:

```
/cron                # open cron job manager UI
```

Shows all jobs with their next run time, status, and last run output.
"""

with open(f"{BASE}/features/cron.md", "w") as f:
    f.write(cron_md)
print("features/cron.md written")

# ─── features/browser.md ──────────────────────────────────────────────
browser_md = r"""---
title: Browser Automation
description: Control Chrome/Chromium from EdgeCrab using the Chrome DevTools Protocol. Grounded in crates/edgecrab-tools/src/tools/browser.rs and BrowserConfig in config.rs.
sidebar:
  order: 9
---

EdgeCrab includes a full suite of browser automation tools using the Chrome DevTools Protocol (CDP). The agent can navigate, interact with, screenshot, and visually analyze web pages.

---

## Prerequisites

Browser tools require Chrome or Chromium:

```bash
# macOS
brew install --cask google-chrome
# or
brew install --cask chromium

# Linux
snap install chromium

# Or point to any CDP-compatible endpoint
export CDP_URL=http://localhost:9222
```

If no browser binary is found and `CDP_URL` is not set, browser tools return a structured error explaining what's missing.

---

## Browser Tools

| Tool | Description |
|------|-------------|
| `browser_navigate` | Navigate to a URL; waits for page load |
| `browser_snapshot` | Capture the accessibility tree (text-based page view) |
| `browser_screenshot` | Take a viewport or full-page screenshot as base64 PNG |
| `browser_click` | Click an element by CSS selector, text, or coordinate |
| `browser_type` | Type text into the focused or specified input element |
| `browser_scroll` | Scroll the page by pixels or to an element |
| `browser_console` | Return buffered `console.log`/`warn`/`error` messages |
| `browser_back` | Navigate back in browser history |
| `browser_press` | Press a keyboard key (e.g. `Enter`, `Tab`, `Escape`) |
| `browser_close` | Close the browser and release the CDP connection |
| `browser_get_images` | Return all images visible on the page as base64 |
| `browser_vision` | Take a screenshot and analyze it with the vision model |

---

## Configuration

```yaml
browser:
  command_timeout: 30          # CDP call timeout in seconds
  record_sessions: false       # record sessions as WebM video
  recording_max_age_hours: 72  # auto-delete recordings older than this
```

---

## Session Recording

Enable session recording to capture everything the agent does in the browser as a WebM video:

```yaml
browser:
  record_sessions: true
  recording_max_age_hours: 72  # auto-delete after 72h
```

Recordings are saved to `~/.edgecrab/browser_recordings/` with timestamps.

---

## Using the Browser Toolset

Enable browser tools:

```bash
edgecrab --toolset browser "research competitors at stripe.com"
edgecrab --toolset research "find the latest release notes for Rust"
```

Browser tools are included in the `core` and `research` toolset aliases.

---

## Example Agent Session

```
❯ Take a screenshot of https://crates.io and tell me what's featured
```

Agent workflow:
1. `browser_navigate("https://crates.io")` — navigates to the page
2. `browser_screenshot()` — captures a PNG
3. `browser_vision({screenshot})` — analyzes with vision model
4. Returns: "The featured crates this week are..."

---

## Vision Analysis

`browser_vision` combines `browser_screenshot` with visual model analysis in one call:

```
❯ Is the login button visible on the current page?
❯ What does the error message say?
❯ Describe the layout of this dashboard
```

The vision call uses the model configured in `model.default` (or `auxiliary.model` if set). Models that don't support vision fall back to `browser_snapshot` (accessibility tree text).

---

## Enabling via Toolset Config

```yaml
tools:
  enabled_toolsets:
    - core        # includes browser (runtime-gated)
    - web         # web_search + web_extract + web_crawl
```

Or to include browser explicitly:

```yaml
tools:
  enabled_toolsets:
    - browser     # only browser tools
    - file        # add file tools
```
"""

with open(f"{BASE}/features/browser.md", "w") as f:
    f.write(browser_md)
print("features/browser.md written")

print("\nBatch 3b (features) complete")
