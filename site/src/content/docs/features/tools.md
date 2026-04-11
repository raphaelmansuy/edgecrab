---
title: Tools & Toolsets
description: EdgeCrab's built-in tools organized into toolsets, custom groups, LSP semantic coding, and runtime gating. Grounded in crates/edgecrab-tools/src/registry.rs and toolsets.rs.
sidebar:
  order: 4
---

Tools are the atomic actions EdgeCrab can perform. **91 core tools** are exposed through `CORE_TOOLS`, with handlers registered at compile time via `inventory::submit!`. They are organized into **toolsets** (named groups) and activated via **aliases** in config or CLI.

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
| `core` | file + meta + scheduling + delegation + code_execution + lsp + session + mcp + messaging + media + browser |
| `coding` | file + terminal + search (`search_files`) + code_execution + lsp |
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
| `patch` | Generate a unified diff patch for review |
| `apply_patch` | Apply a unified diff patch to a file |
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
| `browser_click` | Click an element by CSS selector or text |
| `browser_type` | Type text into a focused input |
| `browser_scroll` | Scroll the page in any direction |
| `browser_press` | Press a keyboard key (Enter, Tab, Escape, etc.) |
| `browser_back` | Navigate back in browser history |
| `browser_close` | Close the browser and release CDP connection |
| `browser_console` | Return buffered console.log/warn/error messages |
| `browser_get_images` | Return base64-encoded images currently visible |
| `browser_vision` | Screenshot + analyze with vision model |
| `browser_wait_for` | Wait for a selector or text to appear on the page |
| `browser_select` | Select an option in a dropdown element |
| `browser_hover` | Hover over an element to trigger tooltip/hover states |

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

### `lsp` — Semantic Code Intelligence
| Tool | Description |
|------|-------------|
| `lsp_goto_definition` | Jump to the defining symbol location |
| `lsp_find_references` | Find all references to the symbol under the cursor |
| `lsp_hover` | Retrieve hover docs and type information |
| `lsp_document_symbols` | List all symbols in a file |
| `lsp_workspace_symbols` | Search symbols across the workspace |
| `lsp_goto_implementation` | Jump to concrete implementations |
| `lsp_call_hierarchy_prepare` | Prepare call hierarchy items |
| `lsp_incoming_calls` | List callers of a symbol |
| `lsp_outgoing_calls` | List callees from a symbol |
| `lsp_code_actions` | Ask the server for available fixes/refactors |
| `lsp_apply_code_action` | Resolve and apply a chosen code action |
| `lsp_rename` | Perform a semantic cross-file rename |
| `lsp_format_document` | Format an entire file through the server |
| `lsp_format_range` | Format just a selected range |
| `lsp_inlay_hints` | Return inline parameter/type hints |
| `lsp_semantic_tokens` | Return semantic token classifications |
| `lsp_signature_help` | Show active signature and parameter info |
| `lsp_type_hierarchy_prepare` | Prepare type hierarchy nodes |
| `lsp_supertypes` | List direct supertypes |
| `lsp_subtypes` | List direct subtypes |
| `lsp_diagnostics_pull` | Pull fresh diagnostics for a document or workspace |
| `lsp_linked_editing_range` | Return linked editing ranges |
| `lsp_enrich_diagnostics` | Ask the LLM to explain raw diagnostics |
| `lsp_select_and_apply_action` | Choose and apply the best code action |
| `lsp_workspace_type_errors` | Summarize workspace-wide type or compile errors |

### `homeassistant` — Smart Home
| Tool | Description |
|------|-------------|
| `ha_list_entities` | List all Home Assistant entities with their states |
| `ha_get_state` | Get the current state of a specific entity |
| `ha_list_services` | List available HA services for a domain |
| `ha_call_service` | Call a Home Assistant service (e.g. turn on a light) |

> Available only when `HA_URL` and `HA_TOKEN` environment variables are set.

### `scheduling` — Cron Jobs
| Tool | Description |
|------|-------------|
| `manage_cron_jobs` | Create, list, enable, disable, and delete cron jobs |

### `delegation` — Subagents
| Tool | Description |
|------|-------------|
| `delegate_task` | Spawn a subagent to complete a parallel subtask |
| `moa` | Run a task through multiple models, synthesize consensus |

### `code_execution` — Sandboxed Execution
| Tool | Description |
|------|-------------|
| `execute_code` | Execute Python, JavaScript, Bash, Ruby, Perl, or Rust code in an isolated sandbox with 7-tool RPC over Unix socket, 5-min timeout, and API keys stripped from the environment |

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

### `messaging` — Cross-Platform Delivery
| Tool | Description |
|------|-------------|
| `send_message` | Send a message to a gateway chat, channel, or contact |

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

---

## Inspecting Tools at Runtime

```bash
edgecrab tools list               # all registered tools
edgecrab tools show file_read     # schema + description for one tool
edgecrab tools toolsets           # toolset aliases and their expansions
```

Inside a session:
```
/tools             # show currently active tools
```

---

## Pro Tips

**Use the `minimal` toolset for sensitive work.** `--toolset minimal` gives the agent only `file` and `terminal`. No web access, no browser. Useful when you want full control over what the agent can reach.

**Add `--toolset coding` for most development tasks.** This exposes file I/O, terminal, file search, code execution, and the LSP toolset so the agent can use semantic rename, definitions, references, diagnostics, and code actions instead of grep-only heuristics.

**Create project-specific toolset aliases.** Add to `config.yaml`:
```yaml
tools:
  custom_groups:
    backend-dev:
      - read_file
      - write_file
      - patch
      - terminal
      - session_search
    docs-review:
      - read_file
      - web_search
      - vision_analyze
```
Then `edgecrab --toolset backend-dev "add tests"`.

---

## Frequently Asked Questions

**Q: I want the agent to use the terminal but NOT the web. Which toolset?**

```bash
edgecrab --toolset file,terminal "run tests and fix failures"
```
Or define a custom group in `config.yaml` with exactly the tools you need.

**Q: How do I know which tools the agent actually called?**

Watch the `⚙` tool call indicators in the TUI. For a complete log, use:
```bash
edgecrab sessions export <id> --format jsonl
```
This shows every tool call and result from the full session history.

**Q: The browser tools aren't working. What's missing?**

EdgeCrab needs Chrome or Chromium to be installed, or a `CDP_URL` pointing to an existing Chrome instance. Check with `edgecrab doctor` — it reports browser availability.

**Q: Can I write custom tools in Rust?**

Yes, via the tool registry. Implement the `Tool` trait in a custom crate and register with `inventory::submit!`. See [Architecture](/developer/architecture/) and [Contributing](/contributing/) for details.

**Q: The `mcp_*` tools show as available but don't work.**

Add MCP server configuration to `config.yaml`:
```yaml
mcp_servers:
  my-server:
    command: ["npx", "-y", "@my-org/my-mcp-server"]
    env:
      MY_KEY: "${MY_KEY}"
```
Then restart EdgeCrab. Run `edgecrab tools list` to confirm the MCP tools are visible.

---

## See Also

- [ReAct Loop](/features/react-loop/) — How tools are called during the agent loop
- [Browser Automation](/features/browser/) — Detailed browser tool documentation
- [Security Model](/user-guide/security/) — How each tool call is security-checked
- [Configuration](/user-guide/configuration/) — `tools.*` config section reference
