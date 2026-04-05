# 004.002 — Tool Catalogue

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 004.001 Tool Registry](001_tool_registry.md) | [→ 004.003 Toolset Composition](003_toolset_composition.md)
> **Source**: `edgecrab-tools/src/tools/` — verified against `mod.rs` and each tool file

## 1. Complete Tool Inventory

All tools are registered at compile time via `inventory`. Each tool implements `ToolHandler` and
belongs to exactly one toolset. Tool names below are **exact** — verified against
`edgecrab-tools/src/toolsets.rs` `CORE_TOOLS` constant (77 entries).

### File Toolset (`file`) — 4 tools

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `read_file` | `file_read.rs` | ✅ | Read file contents with optional line range |
| `write_file` | `file_write.rs` | ❌ | Write or create files (path-jailed) |
| `patch` | `file_patch.rs` | ❌ | Exact string search-replace in files |
| `search_files` | `file_search.rs` | ✅ | Regex + glob search (ripgrep-style) |

### Terminal Toolset (`terminal`) — 4 tools

Three separate process tools — granular, not a single "process" sub-tool:

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `terminal` | `terminal.rs` | ❌ | Execute shell commands (foreground, blocking) |
| `run_process` | `process.rs` | ❌ | Spawn background process via `sh -c`, returns proc ID |
| `list_processes` | `process.rs` | ✅ | List all background processes + status |
| `kill_process` | `process.rs` | ❌ | Send SIGTERM to a background process by ID |

### Web Toolset (`web`) — 3 tools

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `web_search` | `web.rs` | ✅ | Search via SearXNG / Google / Brave |
| `web_extract` | `web.rs` | ✅ | Extract page content (readability + scraper) |
| `web_crawl` | `web.rs` | ✅ | Recursive site crawl (SSRF-guarded) |

### Browser Toolset (`browser`) — 12 tools

Granular CDP Chrome session tools (keyed by `session_id`):

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `browser_navigate` | `browser.rs` | ❌ | Navigate to URL |
| `browser_snapshot` | `browser.rs` | ✅ | DOM + text snapshot |
| `browser_screenshot` | `browser.rs` | ✅ | Full-page screenshot |
| `browser_click` | `browser.rs` | ❌ | Click element by selector |
| `browser_type` | `browser.rs` | ❌ | Type text into focused element |
| `browser_scroll` | `browser.rs` | ❌ | Scroll page / element |
| `browser_console` | `browser.rs` | ✅ | Get browser console logs |
| `browser_back` | `browser.rs` | ❌ | Navigate back |
| `browser_press` | `browser.rs` | ❌ | Press key (Enter, Tab, Escape…) |
| `browser_close` | `browser.rs` | ❌ | Close current tab |
| `browser_get_images` | `browser.rs` | ✅ | List URLs of visible images |
| `browser_vision` | `browser.rs` | ✅ | Analyze page via vision LLM |

### Skills Toolset (`skills`) — 5 tools

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `skills_list` | `skills.rs` | ✅ | List available skills with summaries |
| `skills_categories` | `skills.rs` | ✅ | List skill categories |
| `skill_view` | `skills.rs` | ✅ | View full skill content |
| `skill_manage` | `skills.rs` | ❌ | Create / edit / patch / delete / invalidate skills |
| `skills_hub` | `skills_hub.rs` | ✅ | Remote skill registry — browse + install |

### Memory Toolset (`memory`) — 3 tools

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `memory_read` | `memory.rs` | ✅ | Read MEMORY.md and USER.md |
| `memory_write` | `memory.rs` | ❌ | Update memory files (atomic write) |
| `manage_todo_list` | `todo.rs` | ❌ | Structured task checklist management |

> **Note**: Tool name is `manage_todo_list` (NOT `todo`). The old doc incorrectly listed `todo`.

### Honcho Toolset (`honcho`) — 6 tools

Persistent cross-session user modeling (local JSON store + optional Honcho cloud sync):

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `honcho_conclude` | `honcho.rs` | ❌ | Save observation about user (mutation) |
| `honcho_search` | `honcho.rs` | ✅ | Search past observations by query |
| `honcho_list` | `honcho.rs` | ✅ | List all stored observations |
| `honcho_remove` | `honcho.rs` | ❌ | Remove an observation by ID |
| `honcho_profile` | `honcho.rs` | ✅ | Get user profile summary |
| `honcho_context` | `honcho.rs` | ✅ | Get session context for injection |

### Home Assistant Toolset (`homeassistant`) — 4 tools (runtime-gated)

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `ha_list_entities` | `homeassistant.rs` | ✅ | List all Home Assistant entities |
| `ha_get_state` | `homeassistant.rs` | ✅ | Get entity state |
| `ha_list_services` | `homeassistant.rs` | ✅ | List available HA services |
| `ha_call_service` | `homeassistant.rs` | ❌ | Call service (mutation) |

### Session Toolset (`session`) — 1 tool

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `session_search` | `session_search.rs` | ✅ | FTS5 full-text search over past sessions |

### Checkpoint Toolset (`core`) — 1 tool

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `checkpoint` | `checkpoint.rs` | ❌ | Filesystem snapshot for `/rollback` |

### Clarify Toolset (`meta`) — 1 tool

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `clarify` | `clarify.rs` | ❌ | Ask user clarifying question (one-shot channel to TUI) |

### Media Toolset (`media`) — 3 tools

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `text_to_speech` | `tts.rs` | ❌ | Text-to-speech synthesis |
| `vision_analyze` | `vision.rs` | ✅ | Analyze images via multimodal LLM |
| `transcribe_audio` | `transcribe.rs` | ✅ | Audio transcription (Whisper) |

### Code Execution Toolset (`code_execution`) — 1 tool

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `execute_code` | `execute_code.rs` | ❌ | Sandboxed Python/JS/shell execution |

### Delegation Toolset (`delegation`) — 1 tool

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `delegate_task` | `delegate_task.rs` | ❌ | Spawn subagent with full tool access |

### Mixture of Agents Toolset (`moa`) — 1 tool

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `mixture_of_agents` | `mixture_of_agents.rs` | ❌ | Query N LLMs, aggregate responses |

### Cron Toolset (`scheduling`) — 1 tool

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `manage_cron_jobs` | `cron.rs` | ❌ | Create / list / delete / run cron jobs |

> **Note**: Tool name is `manage_cron_jobs` (NOT `cronjob`). The old doc incorrectly listed `cronjob`.

### MCP Toolset (`mcp`) — 6 static + N dynamic

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `mcp_list_tools` | `mcp_client.rs` | ✅ | List tools from all connected MCP servers |
| `mcp_call_tool` | `mcp_client.rs` | varies | Call a tool on an MCP server |
| `mcp_list_resources` | `mcp_client.rs` | ✅ | List MCP resources |
| `mcp_read_resource` | `mcp_client.rs` | ✅ | Read an MCP resource |
| `mcp_list_prompts` | `mcp_client.rs` | ✅ | List MCP prompts |
| `mcp_get_prompt` | `mcp_client.rs` | ✅ | Get a specific MCP prompt |

Dynamic `McpToolProxy` instances are registered per-server via `register_dynamic()`.

### Advanced Toolset (`advanced`) — 2 tools (runtime-gated stubs)

| Tool | Source | Parallel-Safe | Description |
|------|--------|:-------------:|-------------|
| `send_message` | `advanced.rs` | ❌ | Send to another platform (gateway-gated) |
| `generate_image` | `advanced.rs` | ✅ | Image generation (fal.ai / DALL-E) |

### Support Modules (not tools — no ToolHandler impl)

| Module | Source | Description |
|--------|--------|-------------|
| `skills_guard` | `skills_guard.rs` | Security scanner for external skill files |
| `skills_sync` | `skills_sync.rs` | Manifest-based skill sync / seeding |

## 2. Tool Count Summary

```
┌────────────────────┬────────────────────────────┬───────┐
│  Toolset           │  Tools                     │ Count │
├────────────────────┼────────────────────────────┼───────┤
│  file              │  read_file, write_file,    │   4   │
│                    │  patch, search_files       │       │
├────────────────────┼────────────────────────────┼───────┤
│  terminal          │  terminal, run_process,    │   4   │
│                    │  list_processes,           │       │
│                    │  kill_process              │       │
├────────────────────┼────────────────────────────┼───────┤
│  web               │  web_search, web_extract,  │   3   │
│                    │  web_crawl                 │       │
├────────────────────┼────────────────────────────┼───────┤
│  browser           │  browser_navigate,         │  12   │
│                    │  browser_snapshot, …       │       │
├────────────────────┼────────────────────────────┼───────┤
│  skills            │  skills_list,              │   5   │
│                    │  skills_categories,        │       │
│                    │  skill_view, skill_manage, │       │
│                    │  skills_hub                │       │
├────────────────────┼────────────────────────────┼───────┤
│  memory            │  memory_read, memory_write,│   3   │
│                    │  manage_todo_list          │       │
├────────────────────┼────────────────────────────┼───────┤
│  honcho            │  honcho_conclude, _search, │   6   │
│                    │  _list, _remove, _profile, │       │
│                    │  _context                  │       │
├────────────────────┼────────────────────────────┼───────┤
│  homeassistant     │  ha_list_entities,         │   4   │
│                    │  ha_get_state,             │       │
│                    │  ha_list_services,         │       │
│                    │  ha_call_service           │       │
├────────────────────┼────────────────────────────┼───────┤
│  session           │  session_search            │   1   │
│  core              │  checkpoint                │   1   │
│  meta              │  clarify                   │   1   │
│  media             │  text_to_speech,           │   3   │
│                    │  vision_analyze,           │       │
│                    │  transcribe_audio          │       │
│  code_execution    │  execute_code              │   1   │
│  delegation        │  delegate_task             │   1   │
│  moa               │  mixture_of_agents         │   1   │
│  scheduling        │  manage_cron_jobs          │   1   │
│  mcp               │  mcp_list_tools, …         │  6+N  │
│  advanced          │  send_message,             │   2   │
│                    │  generate_image            │       │
├────────────────────┼────────────────────────────┼───────┤
│  TOTAL (static)    │                            │  59   │
│  TOTAL (CORE_TOOLS)│                            │  77   │
│  TOTAL (incl. MCP) │                            │ 59+N  │
└────────────────────┴────────────────────────────┴───────┘
```

> **77 vs 59**: `CORE_TOOLS` in `toolsets.rs` has 77 entries because it lists each browser tool individually (12), each honcho tool (6), each HA tool (4), each MCP tool (6), etc. The "59 static" count groups dynamic MCP entries separately.

## 3. Source File Map

Verified against `edgecrab-tools/src/tools/mod.rs` — 30 modules total:

```
tools/
├── mod.rs              ← this index
├── file_read.rs        ← read_file
├── file_write.rs       ← write_file
├── file_patch.rs       ← patch
├── file_search.rs      ← search_files
├── terminal.rs         ← terminal
├── process.rs          ← run_process, list_processes, kill_process
├── web.rs              ← web_search, web_extract, web_crawl
├── browser.rs          ← browser_* (12 tools)
├── skills.rs           ← skills_list, skills_categories, skill_view, skill_manage
├── skills_hub.rs       ← skills_hub
├── skills_guard.rs     ← security scanning (not a tool)
├── skills_sync.rs      ← manifest sync (not a tool)
├── memory.rs           ← memory_read, memory_write
├── todo.rs             ← manage_todo_list
├── honcho.rs           ← honcho_* (6 tools)
├── homeassistant.rs    ← ha_* (4 tools)
├── session_search.rs   ← session_search
├── checkpoint.rs       ← checkpoint
├── clarify.rs          ← clarify
├── tts.rs              ← text_to_speech
├── vision.rs           ← vision_analyze
├── transcribe.rs       ← transcribe_audio
├── execute_code.rs     ← execute_code
├── delegate_task.rs    ← delegate_task
├── mixture_of_agents.rs ← mixture_of_agents
├── cron.rs             ← manage_cron_jobs
├── mcp_client.rs       ← mcp_* (6 static + N dynamic)
└── advanced.rs         ← send_message, generate_image
```

## 4. Parallel Execution

Tools self-declare parallel safety via the `parallel_safe()` trait method:

```
parallel_safe() == true   → can run concurrently with other safe tools
parallel_safe() == false  → serialised (state-mutating or session-bound)
```

The `ToolRegistry` groups simultaneous tool calls from the LLM and uses `tokio::join_all` for parallel-safe groups while sequencing unsafe ones.

**Parallel-safe tools** (read-only or idempotent):
- `read_file`, `search_files`
- `web_search`, `web_extract`, `web_crawl`
- `browser_snapshot`, `browser_screenshot`, `browser_console`, `browser_get_images`, `browser_vision`
- `skills_list`, `skills_categories`, `skill_view`, `skills_hub`
- `memory_read`
- `honcho_search`, `honcho_list`, `honcho_profile`, `honcho_context`
- `ha_list_entities`, `ha_get_state`, `ha_list_services`
- `session_search`, `vision_analyze`, `transcribe_audio`, `generate_image`
- `list_processes`
- `mcp_list_tools`, `mcp_list_resources`, `mcp_read_resource`, `mcp_list_prompts`, `mcp_get_prompt`

**Sequential-only tools** (state-mutating):
- `write_file`, `patch`, `terminal`, `run_process`, `kill_process`
- `browser_navigate`, `browser_click`, `browser_type`, `browser_scroll`, `browser_back`, `browser_press`, `browser_close`
- `skill_manage`, `memory_write`, `manage_todo_list`
- `honcho_conclude`, `honcho_remove`
- `ha_call_service`, `checkpoint`, `clarify`, `text_to_speech`
- `execute_code`, `delegate_task`, `mixture_of_agents`, `manage_cron_jobs`, `send_message`
