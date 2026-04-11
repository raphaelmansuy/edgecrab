# Tool Catalogue 🦀

> **Verified against:** `crates/edgecrab-tools/src/toolsets.rs` ·
> `crates/edgecrab-tools/src/tools/`

91 core tools are exposed through `CORE_TOOLS` in `toolsets.rs`, with handlers
registered through `inventory::submit!`. This page groups them by function and
records which toolset each belongs to.

🦀 *`hermes-agent` (EdgeCrab's Python predecessor) shipped a broad tool set in the same categories. OpenClaw ([TypeScript/Node.js](https://github.com/openclaw)) focuses on browser automation, camera, and productivity integrations. EdgeCrab deploys 91 core tools across every execution domain — and every one runs through a security gate.*

---

## Web

| Tool | Toolset | What it does |
|---|---|---|
| `web_search` | `web` | Search the web; returns ranked snippets |
| `web_extract` | `web` | Fetch and extract text from a URL |
| `web_crawl` | `web` | Follow links up to N depth, aggregate content |

---

## Terminal and process control

| Tool | Toolset | What it does |
|---|---|---|
| `terminal` | `terminal` | Run a shell command; full stdin/stdout/stderr |
| `run_process` | `terminal` | Start a long-running background process |
| `list_processes` | `terminal` | List active background processes in this session |
| `kill_process` | `terminal` | Terminate a background process by ID |
| `get_process_output` | `terminal` | Read buffered stdout/stderr from a background process |
| `wait_for_process` | `terminal` | Block until a background process exits (with timeout) |
| `write_stdin` | `terminal` | Send data to a background process's stdin |

> **Security note:** `terminal` is subject to `CommandScanner` and the
> `ApprovalPolicy` gate before execution. See [Security](../011_security/001_security.md).

---

## Files

| Tool | Toolset | What it does |
|---|---|---|
| `read_file` | `file` | Read a file; respects path jail |
| `write_file` | `file` | Write or overwrite a file |
| `patch` | `file` | Apply a unified diff patch to a file |
| `search_files` | `file` | Ripgrep-style content search across the working tree |

---

## Skills

| Tool | Toolset | What it does |
|---|---|---|
| `skills_list` | `skills` | List all installed skills with descriptions |
| `skills_categories` | `skills` | Group skills by category |
| `skill_view` | `skills` | Show a skill's full content |
| `skill_manage` | `skills` | Install, update, or remove a skill |
| `skills_hub` | `skills` | Browse the remote skills hub |

---

## Browser (full headless control)

| Tool | Toolset | What it does |
|---|---|---|
| `browser_navigate` | `browser` | Navigate to a URL |
| `browser_snapshot` | `browser` | Capture accessibility tree (DOM snapshot) |
| `browser_screenshot` | `browser` | Capture a visual screenshot |
| `browser_click` | `browser` | Click on an element by selector |
| `browser_type` | `browser` | Type text into a focused element |
| `browser_scroll` | `browser` | Scroll by pixel offset or to element |
| `browser_console` | `browser` | Read browser console output |
| `browser_back` | `browser` | Navigate back in history |
| `browser_press` | `browser` | Press a keyboard key (Enter, Tab, F5, …) |
| `browser_close` | `browser` | Close the browser session |
| `browser_get_images` | `browser` | List images on the current page |
| `browser_vision` | `browser` | Analyze the screenshot via vision model |
| `browser_wait_for` | `browser` | Wait for a selector or text to appear |
| `browser_select` | `browser` | Select a dropdown option |
| `browser_hover` | `browser` | Hover over an element |

---

## Media

| Tool | Toolset | What it does |
|---|---|---|
| `text_to_speech` | `media` | Convert text to audio via TTS provider |
| `vision_analyze` | `media` | Analyze an image with a vision model |
| `transcribe_audio` | `media` | Transcribe audio with STT provider |
| `generate_image` | `messaging` | Generate an image with an image-gen model |

---

## Planning, memory, and session history

| Tool | Toolset | What it does |
|---|---|---|
| `manage_todo_list` | `meta` | Create, update, and complete structured todos |
| `memory_read` | `memory` | Read a named memory file from `~/.edgecrab/memories/` |
| `memory_write` | `memory` | Append or overwrite a memory file |
| `session_search` | `session` | FTS5 full-text search over all past sessions |
| `checkpoint` | `core` | Save a named checkpoint of current session state |
| `clarify` | `meta` | Ask the user a clarifying question with 1–4 options |

---

## Honcho (user memory and profile)

[Honcho](https://honcho.dev) is a user-level memory and personalization layer.
These tools only work if `HONCHO_*` environment variables are configured.

| Tool | Toolset | What it does |
|---|---|---|
| `honcho_conclude` | `memory` | Derive insights from the current session |
| `honcho_search` | `memory` | Semantic search over user memory |
| `honcho_list` | `memory` | List memory entries |
| `honcho_remove` | `memory` | Delete a memory entry |
| `honcho_profile` | `memory` | View user profile derived from memory |
| `honcho_context` | `memory` | Inject relevant context from user memory |

---

## Home Assistant

These tools work when `HA_URL` and `HA_TOKEN` are configured.

| Tool | Toolset | What it does |
|---|---|---|
| `ha_list_entities` | `file` | List all Home Assistant entities |
| `ha_get_state` | `file` | Get entity state and attributes |
| `ha_list_services` | `file` | List available Home Assistant services |
| `ha_call_service` | `file` | Call a Home Assistant service |

---

## Execution and delegation

| Tool | Toolset | What it does |
|---|---|---|
| `execute_code` | `code_execution` | Execute code in an isolated sandbox (Docker or local) |
| `delegate_task` | `delegation` | Spawn a sub-agent with a specific goal and toolset |
| `moa` | `moa` | Run N reference models in parallel, then synthesise with the configured or overridden aggregator; hidden when `moa.enabled` is `false` or the active toolset policy excludes `moa`. Legacy alias: `mixture_of_agents` |

> `delegate_depth` in `ToolContext` is capped at 2. A sub-agent cannot spawn
> another sub-agent that spawns a third — recursion protection.

---

## Scheduling

| Tool | Toolset | What it does |
|---|---|---|
| `manage_cron_jobs` | `scheduling` | Create, list, pause, resume, run, or delete cron jobs |

---

## MCP (Model Context Protocol)

These tools proxy operations to configured MCP servers.
See [MCP docs at modelcontextprotocol.io](https://modelcontextprotocol.io).

| Tool | Toolset | What it does |
|---|---|---|
| `mcp_list_tools` | `mcp` | List tools exposed by all configured MCP servers |
| `mcp_call_tool` | `mcp` | Call a tool on an MCP server |
| `mcp_list_resources` | `mcp` | List resources from an MCP server |
| `mcp_read_resource` | `mcp` | Read a resource from an MCP server |
| `mcp_list_prompts` | `mcp` | List prompts from an MCP server |
| `mcp_get_prompt` | `mcp` | Get a prompt from an MCP server |

---

## Messaging

| Tool | Toolset | What it does |
|---|---|---|
| `send_message` | `messaging` | Send a message to a configured gateway platform target |

---

## Runtime-gated tools

Some tools are compiled in but invisible to the model at runtime when the
required capability is absent:

| Tool | Condition for visibility |
|---|---|
| `execute_code` | Docker present, or `sandbox_code_execution.enabled=true` |
| `browser_*` | Playwright / Chromium installed and reachable |
| `ha_*` | `HA_URL` and `HA_TOKEN` configured |
| `honcho_*` | `HONCHO_APP_ID` configured |
| `text_to_speech` | TTS provider configured |
| `transcribe_audio` | STT provider configured |
| `generate_image` | Image generation provider configured |
| `send_message` | At least one gateway platform running |

---

## ACP tool subset (54 tools)

The ACP server (`edgecrab acp`) removes interactive and delivery-specific
tools unsuitable for IDE integration:

**Excluded from ACP:**
`clarify`, `send_message`, `generate_image`, `text_to_speech`,
`transcribe_audio`, `honcho_*`, `ha_*`, `manage_cron_jobs` (some),
and a few others that require user interaction.

---

## Tips

> **Tip: `search_files` before `read_file` for large codebases.**
> `search_files` uses ripgrep under the hood — it finds the exact location in
> milliseconds without reading every file. The model will reach the right answer
> faster if it searches first.

> **Tip: `delegate_task` for parallelisable sub-problems.**
> Running `delegate_task` with `max_iterations=30` for a well-scoped sub-task is
> more reliable than asking the main agent to do everything in 90 iterations.

> **Tip: `checkpoint` before risky operations.**
> Store a named checkpoint before a multi-file refactor. If things go wrong,
> `session_search` + `rollback` can restore the pre-refactor state.

---

## Cross-references

- How tools are dispatched → [Tool Registry](./001_tool_registry.md)
- Which toolset each tool belongs to → [Toolset Composition](./003_toolset_composition.md)
- Execution backends for `terminal` and `execute_code` → [Tools Runtime](./004_tools_runtime.md)
- Security gates on all tools → [Security](../011_security/001_security.md)
