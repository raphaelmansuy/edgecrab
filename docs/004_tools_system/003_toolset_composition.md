# Toolset Composition 🦀

> **Verified against:** `crates/edgecrab-tools/src/toolsets.rs`

---

## Why toolsets exist

Giving the model all 65 tools on every turn has two costs: it inflates the
token count of every API call (tool schemas take tokens), and it gives the model
a larger decision surface for choosing the wrong tool. Toolsets are a
configuration-time policy layer that controls which tools the model can access
for a given use case.

🦀 *The right claw for the right fight. You do not deploy all claws for a
text-summarisation job — just the ones that help.*

---

## Named aliases (user-facing)

These are the strings you pass with `--toolset` or in `config.yaml`:

| Alias | Expands to | Used for |
|---|---|---|
| `core` | all canonical toolsets combined | Default; full power |
| `coding` | `file`, `terminal`, `search`, `code_execution` | Code tasks |
| `research` | `web`, `browser`, `vision` | Web and visual research |
| `debugging` | `terminal`, `web`, `file` | Debugging sessions |
| `safe` | `web`, `vision`, `image_gen`, `moa` | No shell, no filesystem |
| `all` | sentinel — no filtering | Expose everything (including runtime-gated) |
| `minimal` | `file`, `terminal` | Lightweight; file and shell only |
| `data_gen` | `file`, `terminal`, `web`, `code_execution` | Data pipeline tasks |

---

## Canonical toolset names (internal)

These are the atomic units the resolver works with:

| Toolset | Tools |
|---|---|
| `file` | `read_file`, `write_file`, `patch`, `search_files` |
| `terminal` | `terminal`, `run_process`, `list_processes`, `kill_process`, `get_process_output`, `wait_for_process`, `write_stdin` |
| `web` | `web_search`, `web_extract`, `web_crawl` |
| `browser` | All 15 `browser_*` tools |
| `memory` | `memory_read`, `memory_write`, `honcho_*` |
| `skills` | `skills_list`, `skills_categories`, `skill_view`, `skill_manage`, `skills_hub` |
| `meta` | `manage_todo_list`, `clarify` |
| `scheduling` | `manage_cron_jobs` |
| `delegation` | `delegate_task`, `mixture_of_agents` |
| `code_execution` | `execute_code` |
| `session` | `session_search` |
| `mcp` | All 6 `mcp_*` tools |
| `media` | `text_to_speech`, `vision_analyze`, `transcribe_audio` |
| `messaging` | `send_message`, `generate_image` |
| `core` | `checkpoint` (note: `core` the toolset ≠ `core` the alias) |

---

## Resolution pipeline

```
  User config or CLI flag
  ─────────────────────────────────────────────────────────────────

  enabled_toolsets = ["coding", "web"]  (from --toolset or config)
  disabled_toolsets = ["browser"]       (from config)

  Step 1 — Alias expansion
    "coding" → ["file", "terminal", "search", "code_execution"]
    "web"    → ["web"]
    result   = ["file", "terminal", "code_execution", "web"]

  Step 2 — Disabled subtraction
    "browser" → ["browser"]
    result    = ["file", "terminal", "code_execution", "web"]
    (browser was not in the list anyway — subtraction is safe)

  Step 3 — "all" sentinel check
    if "all" in enabled → skip filtering, expose everything
    otherwise → use result from step 2

  Step 4 — resolve_active_toolsets()
    return final list of canonical toolset names
        │
        ▼
  Step 5 — ToolRegistry::get_definitions(enabled, disabled, ctx)
    keep only tools whose toolset() is in the active list
    apply check_fn() per tool
    return Vec<ToolSchema> (schemas sent to LLM as available tools)
```

---

## Expansion examples

```sh
# Full coding environment
edgecrab --toolset coding "refactor auth.rs"
# Active: file, terminal, code_execution

# Research only (no filesystem, no shell)
edgecrab --toolset research "summarise latest AI papers"
# Active: web, browser, vision (media/vision subset)

# Safe: web + vision only — no shell, no writes
edgecrab --toolset safe "describe this image"
# Active: web, vision, image_gen, moa

# Multiple toolsets combined
edgecrab --toolset coding,research "find trending Rust crates and scaffold a project"
# Active: file, terminal, code_execution, web, browser, vision

# Exclude a toolset from a wider alias
# (via config; no flag for disabled yet)
# enabled_toolsets: ["core"]
# disabled_toolsets: ["browser"]
```

---

## Toolset and tool relationship diagram

```
  core (alias)
   ├── file     ─── read_file, write_file, patch, search_files
   ├── terminal ─── terminal, run_process, ...
   ├── web      ─── web_search, web_extract, web_crawl
   ├── browser  ─── browser_navigate, browser_snapshot, ...
   ├── memory   ─── memory_read, memory_write, honcho_*
   ├── skills   ─── skills_list, skill_manage, ...
   ├── meta     ─── manage_todo_list, clarify
   ├── scheduling── manage_cron_jobs
   ├── delegation── delegate_task, mixture_of_agents
   ├── code_execution── execute_code
   ├── session  ─── session_search
   ├── mcp      ─── mcp_list_tools, mcp_call_tool, ...
   ├── media    ─── text_to_speech, vision_analyze, transcribe_audio
   ├── messaging─── send_message, generate_image
   └── core     ─── checkpoint
          ↑ note: "core" the canonical toolset != "core" the alias
```

---

## Configuration examples

In `~/.edgecrab/config.yaml`:

```yaml
agent:
  enabled_toolsets:
    - coding
    - web
  disabled_toolsets:
    - browser   # never give the model browser access
```

Programmatically (when building `Agent`):

```rust
let agent = AgentBuilder::new(model)
    .provider(provider)
    .build()?;

// Via AgentConfig
config.enabled_toolsets = vec!["coding".into(), "session".into()];
config.disabled_toolsets = vec!["browser".into()];
```

---

## `ACP_TOOLS` — the editor subset

```
  ACP_TOOLS = CORE_TOOLS minus:
    clarify, send_message, generate_image, text_to_speech,
    transcribe_audio, manage_cron_jobs, browser_*, honcho_*, ha_*
```

ACP runs as a stdio server for VS Code — interactive tools that require terminal
input or produce audio don't make sense in that context.

---

## Tips

> **Tip: Use `--toolset safe` for untrusted or exploratory sessions.**
> `safe` gives the model web search and vision but no shell access and no filesystem
> writes. Ideal for letting a third party use EdgeCrab without risk.

> **Tip: Use `--toolset minimal` for the fastest, cheapest sessions.**
> `file` + `terminal` covers most coding tasks and minimises the tool schema tokens
> sent on every API call.

> **Tip: Toolsets are checked at schema generation, not at registration.**
> A tool in the `browser` toolset will still appear in `edgecrab tools list` even
> if browser toolset is disabled. The model just won't see its schema.

---

## FAQ

**Q: What is the difference between `core` alias and `core` toolset?**
`core` the *alias* expands to all canonical toolsets combined (the full belt).
`core` the *toolset* (canonical) contains exactly one tool: `checkpoint`.
The naming collision is unfortunate but codified in the current source.

**Q: What happens if I specify a toolset that doesn't exist?**
`resolve_alias()` returns `None` for unknown names. Unknown canonical names are
ignored silently. Run `edgecrab tools list --toolsets` to see valid names.

**Q: Can a tool belong to more than one toolset?**
No. Each `ToolHandler` returns exactly one toolset from `fn toolset()`. To expose
a tool in multiple contexts, use an alias that includes all the relevant canonical toolsets.

---

## Cross-references

- All 65 tools by function → [Tool Catalogue](./002_tool_catalogue.md)
- How toolsets are filtered at dispatch → [Tool Registry](./001_tool_registry.md)
- CLI `--toolset` flag → [CLI Architecture](../005_cli/001_cli_architecture.md)
- Config file toolset settings → [Config and State](../009_config_state/001_config_state.md)
