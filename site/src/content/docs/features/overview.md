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

### 15 LLM Providers

Switch provider and model without restarting:

```
/model openai/gpt-5
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

### Semantic Coding with LSP

Built-in language-server support gives EdgeCrab semantic code intelligence instead of text-only heuristics:
- Go to definition, references, implementation, document symbols, and workspace symbols
- Code actions, semantic rename, whole-document and range formatting
- Inlay hints, semantic tokens, signature help, call hierarchy, and type hierarchy
- Pull diagnostics, workspace type-error scans, linked editing, and LLM-enriched diagnostic explanations

See [Language Server Protocol](/features/lsp/)

### 15 Messaging Gateways

Run EdgeCrab as a persistent bot on 15 platforms:
- Telegram, Discord, Slack, Signal, WhatsApp
- Matrix, Mattermost, DingTalk, SMS, Email
- Home Assistant, Webhook, API Server, Feishu/Lark, WeCom

See [Messaging Gateway](/user-guide/messaging/)

### Security

Built-in defense in depth (7 layers):
- Path traversal prevention (`SanitizedPath` compile-time type)
- SSRF guard with private-IP blocklist (`SafeUrl` compile-time type)
- Aho-Corasick command scanner (8 danger categories)
- Prompt injection detection in context files
- Code execution sandbox (API keys stripped from child env)
- Skills threat scanner (23 patterns — exfiltration, injection, persistence)
- Output redaction (API keys, tokens)

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
| `core` | file + meta + scheduling + delegation + code_execution + lsp + session + mcp + messaging + media + browser (runtime-gated) |
| `coding` | file + terminal + search + code_execution + lsp |
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
| `browser_click` | Click an element |
| `browser_type` | Type text into an input |
| `browser_scroll` | Scroll the page |
| `browser_press` | Press a keyboard key |
| `browser_back` | Navigate back |
| `browser_close` | Close the browser |
| `browser_console` | Capture browser console logs |
| `browser_get_images` | Get images from the page |
| `browser_vision` | Take screenshot and analyze with vision model |
| `browser_wait_for` | Wait for element/text to appear |
| `browser_select` | Select a dropdown option |
| `browser_hover` | Hover to trigger tooltips/states |

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
| `moa` | Multi-model consensus reasoning |

### Code Execution (`code_execution`)

| Tool | Description |
|------|-------------|
| `execute_code` | Execute Python, Node.js, or Bash code in isolation |

### LSP Tools (`lsp`)

| Tool | Description |
|------|-------------|
| `lsp_goto_definition` | Jump to a symbol definition |
| `lsp_find_references` | Find symbol references |
| `lsp_hover` | Get hover docs and type information |
| `lsp_document_symbols` | Enumerate symbols in a file |
| `lsp_workspace_symbols` | Search symbols across the workspace |
| `lsp_goto_implementation` | Jump to concrete implementations |
| `lsp_call_hierarchy_prepare` | Prepare call hierarchy items |
| `lsp_incoming_calls` | Show incoming calls |
| `lsp_outgoing_calls` | Show outgoing calls |
| `lsp_code_actions` | List server-suggested fixes and refactors |
| `lsp_apply_code_action` | Resolve and apply a code action |
| `lsp_rename` | Rename a symbol across files |
| `lsp_format_document` | Format a whole file |
| `lsp_format_range` | Format a selected range |
| `lsp_inlay_hints` | Return inlay hints |
| `lsp_semantic_tokens` | Return semantic token classes |
| `lsp_signature_help` | Show function signature help |
| `lsp_type_hierarchy_prepare` | Prepare a type hierarchy item |
| `lsp_supertypes` | List supertypes |
| `lsp_subtypes` | List subtypes |
| `lsp_diagnostics_pull` | Pull document or workspace diagnostics |
| `lsp_linked_editing_range` | Return linked editing regions |
| `lsp_enrich_diagnostics` | Explain diagnostics with the LLM |
| `lsp_select_and_apply_action` | Pick and apply the best action |
| `lsp_workspace_type_errors` | Summarize workspace-wide type errors |

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
- [Language Server Protocol](/features/lsp/) — Semantic code navigation, edits, and diagnostics
- [SQLite State and Search](/features/state/) — Session persistence and FTS5 search
- [Security Model](/user-guide/security/) — 7-layer defense stack

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
    +--[Tool Dispatch]    <-- file / terminal / web / browser / lsp / memory / mcp
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

All registered tools can be active simultaneously, including the full LSP surface when semantic coding is enabled. Limiting toolsets to what's needed keeps the system prompt shorter and the LLM more focused.

**Q: Edge case: Can the agent call the same tool infinitely?**

No. `model.max_iterations` (default: 90) limits total tool calls per session. The LLM also receives tool results that typically converge toward an answer. If the budget is exhausted, EdgeCrab reports to the user.
