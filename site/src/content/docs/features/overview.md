---
title: Features Overview
description: Complete capability overview for EdgeCrab — the Rust-native autonomous coding agent with ratatui TUI, ReAct loop, multi-provider LLM, and built-in security. Grounded in crates/edgecrab-tools/src/toolsets.rs CORE_TOOLS.
sidebar:
  order: 1
---

EdgeCrab ships as a single static binary with enterprise-grade features.
No Python venv, no Node.js — just one executable.

```
Input --> ContextBuilder --> AgentLoop --> ToolRegistry --> Security checks
                                 ^                              |
                                 +---- ToolResult <------------+
```

---

## Core Features

### Autonomous ReAct Loop

EdgeCrab runs a [Reason-Act-Observe loop](/features/react-loop/) — it
reasons about a task, calls a tool, observes the result, then repeats.
The loop runs up to `model.max_iterations` tool calls (default: 90)
before stopping.

### Ratatui TUI

A full-featured terminal UI with:
- Streaming token display with cost tracking
- Tool execution feed with per-tool timing
- Slash command autocomplete (all installed skills appear as commands)
- Keyboard-driven interface
- Customizable skins, symbols, and personality presets

See [TUI Interface](/features/tui/)

### 13+ LLM Providers

Switch provider and model without restarting:

```
/model anthropic/claude-opus-4
/model openai/gpt-4o
/model ollama/llama3.3
/model copilot/gpt-4.1-mini
```

See [LLM Providers](/providers/overview/)

### Skills System

Reusable Markdown workflows that teach EdgeCrab domain-specific tasks:
- Skills are **directories** containing a `SKILL.md` file
- EdgeCrab can create and improve skills during sessions
- Compatible with [agentskills.io](https://agentskills.io)

See [Skills System](/features/skills/)

### Persistent Memory

Agent memory stored in `~/.edgecrab/memories/`:
- Auto-written after each session when `memory.auto_flush: true`
- Injected into the system prompt at session start
- Honcho integration for cross-session user modeling

See [Memory](/features/memory/)

### Browser Automation

Built-in browser control via Chrome DevTools Protocol (CDP):
- Navigate, click, type, scroll, take screenshots
- Console log capture
- Vision analysis of screenshots
- Session recording as WebM

See [Browser Automation](/features/browser/)

### Multi-Platform Messaging Gateway

Run EdgeCrab as a persistent bot on 10 platforms:
- Telegram, Discord, Slack, Signal, WhatsApp
- Matrix, Mattermost, DingTalk, SMS, Email

See [Messaging Gateway](/user-guide/messaging/)

### Security

Built-in defense in depth (6 layers):
- Path traversal prevention (`SanitizedPath` compile-time type)
- SSRF guard with DNS-rebinding protection
- Aho-Corasick command scanner (8 danger categories, 38 patterns)
- Prompt injection detection
- Output redaction (API keys, tokens)
- Approval policy (off / smart / manual)

See [Security Model](/user-guide/security/)

### Checkpoints and Rollback

Shadow git commits before every destructive file operation. Roll back
at any time:

```
/rollback       # interactive checkpoint browser
```

See [Checkpoints](/user-guide/checkpoints/)

### Cron / Scheduled Tasks

Built-in cron scheduler with agent-managed jobs:

```bash
edgecrab cron add "0 9 * * 1-5" "morning standup summary"
```

See [Cron Jobs](/features/cron/)

---

## Tool Inventory

All tools sourced from `CORE_TOOLS` constant in
`crates/edgecrab-tools/src/toolsets.rs`.

### Toolset Aliases

| Alias | Expands to |
|-------|-----------|
| `core` | file + meta + scheduling + delegation + code_execution + session + mcp + browser (runtime-gated) |
| `coding` | file + terminal + search + code_execution |
| `research` | web + browser + vision |
| `debugging` | terminal + web + file |
| `safe` | web + vision + image_gen + moa |
| `minimal` | file + terminal |
| `data_gen` | file + terminal + web + code_execution |
| `all` | every registered tool (no filtering) |

### File Tools (`file`)

| Tool | Description |
|------|-------------|
| `read_file` | Read file contents with optional line range |
| `write_file` | Write or overwrite a file (creates checkpoint) |
| `patch` | Apply a unified diff patch to a file (creates checkpoint) |
| `search_files` | Regex or glob search across file tree |

### Terminal Tools (`terminal`)

| Tool | Description |
|------|-------------|
| `terminal` | Run a shell command and capture output |
| `run_process` | Start a long-running background process |
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

Runtime-gated: requires Chrome or Chromium. If no browser is
reachable, these tools are silently absent from the tool list.

| Tool | Description |
|------|-------------|
| `browser_navigate` | Navigate to a URL |
| `browser_snapshot` | Get page accessibility tree as text |
| `browser_screenshot` | Take a screenshot |
| `browser_click` | Click an element |
| `browser_type` | Type text into an input |
| `browser_scroll` | Scroll the page |
| `browser_console` | Capture browser console logs |
| `browser_back` | Navigate back |
| `browser_press` | Press a keyboard key |
| `browser_close` | Close the browser |
| `browser_get_images` | Get images from the page |
| `browser_vision` | Analyze page screenshot with vision model |

### Memory Tools (`memory`)

| Tool | Description |
|------|-------------|
| `memory_read` | Read a memory file |
| `memory_write` | Write or update a memory file |
| `honcho_conclude` | Commit a Honcho cross-session memory entry |
| `honcho_search` | Search the Honcho user model |
| `honcho_list` | List Honcho memory entries |
| `honcho_remove` | Remove a Honcho entry |
| `honcho_profile` | Update the Honcho user profile |
| `honcho_context` | Get relevant Honcho context for current task |

### Skills Tools (`skills`)

| Tool | Description |
|------|-------------|
| `skills_list` | List available skills |
| `skills_categories` | List skill categories |
| `skill_view` | View a skill's content |
| `skill_manage` | Install / uninstall / update a skill |
| `skills_hub` | Browse the agentskills.io hub |

### Scheduling Tools (`scheduling`)

| Tool | Description |
|------|-------------|
| `manage_cron_jobs` | Create / list / delete / enable / disable cron jobs |

### Meta Tools (`meta`)

| Tool | Description |
|------|-------------|
| `manage_todo_list` | Create and track a session todo list |
| `clarify` | Ask the user a clarifying question (TUI and gateway only) |

### Delegation Tools (`delegation`)

| Tool | Description |
|------|-------------|
| `delegate_task` | Spawn a subagent for a parallel subtask |
| `mixture_of_agents` | Multi-model consensus reasoning |

### Code Execution (`code_execution`)

| Tool | Description |
|------|-------------|
| `execute_code` | Execute Python, Node.js, or Bash code in isolation |

### Session Tools (`session`)

| Tool | Description |
|------|-------------|
| `session_search` | Full-text search (FTS5) across all session history |

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
| `generate_image` | Generate an image (runtime-gated: requires provider support) |

### Home Assistant Tools (`homeassistant`)

Runtime-gated: requires `HA_URL` and `HA_TOKEN` environment variables.

| Tool | Description |
|------|-------------|
| `ha_list_entities` | List all Home Assistant entities |
| `ha_get_state` | Get current state of an entity |
| `ha_list_services` | List available Home Assistant services |
| `ha_call_service` | Call a Home Assistant service |

### Core Tools (`core`)

| Tool | Description |
|------|-------------|
| `checkpoint` | Create / list / restore / diff filesystem checkpoints |

### Messaging Tools (runtime-gated)

Only available in gateway sessions (Telegram, Discord, Slack, etc.):

| Tool | Description |
|------|-------------|
| `send_message` | Send a message to the current chat (gateway only) |

---

## What's Next?

- [ReAct Tool Loop](/features/react-loop/) — How the autonomous reasoning engine works
- [TUI Interface](/features/tui/) — Full keyboard shortcuts and slash commands
- [Skills System](/features/skills/) — Creating and using reusable skills
- [Memory](/features/memory/) — Persistent memory and Honcho user modeling
- [Browser Automation](/features/browser/) — Browser automation with CDP
- [SQLite State and Search](/features/state/) — Session persistence and FTS5 search
- [Security Model](/user-guide/security/) — 6-layer defense stack

---

## Feature Coverage at a Glance

```
User Input
    |
    v
[Context Builder]  <-- SOUL.md + AGENTS.md + memories + skills
    |
    v
[Agent Loop]       -- LLM reasons, calls tools, observes results
    |
    +--[Security checks]  <-- path jail + SSRF + command scan + approval
    |
    +--[Tool Dispatch]    <-- file / terminal / web / browser / memory / mcp
    |
    v
[State DB (SQLite)]  -- every message stored, FTS5 indexed
    |
    v
[TUI / Gateway]     -- rendered in ratatui or sent to Telegram/Discord/...
```

---

## Frequently Asked Questions

**Q: What's the difference between "tools" and "skills"?**

Tools are atomic Rust functions (read a file, run a command, search the web). Skills are Markdown documents that guide *how* the agent uses tools. A skill says "when doing a security audit, first do X, then Y, then Z." Tools are the verbs; skills are the recipe.

**Q: How does EdgeCrab compare to agentic frameworks like LangChain?**

EdgeCrab is a self-contained binary, not a framework. It's designed to be run, not extended _programmatically_ (though SDKs exist). You configure it via YAML and Markdown, not code. For programmatic use, the Python/Node.js SDKs provide an API surface.

**Q: Can EdgeCrab run without internet access?**

Yes. Use `--model ollama/llama3.3` (local Ollama) and `--toolset file,terminal,memory`. All tools used by those toolsets work offline.

**Q: How many tools can be active at once?**

All registered tools (60+) can be active simultaneously. The LLM receives tool schemas in the system prompt. Limiting toolsets to what's needed keeps the system prompt shorter and the LLM more focused.

**Q: Edge case: Can the agent call the same tool infinitely?**

No. `tools.max_loop_depth` (default: 20) limits total tool calls per user turn. The LLM also receives tool results that typically converge toward an answer. Infinite loops in practice are extremely rare and always bounded by the depth limit.
