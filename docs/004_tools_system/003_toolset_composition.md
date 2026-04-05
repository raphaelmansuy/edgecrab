# 004.003 — Toolset Composition

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 004.001 Tool Registry](001_tool_registry.md) | [→ 009 Config](../009_config_state/001_config_state.md)
> **Source**: `edgecrab-tools/src/toolsets.rs` — verified against real implementation

## 1. Toolset Resolution Pipeline

```
resolve_toolset(enabled_toolsets, disabled_toolsets, config)
├── 1. Start with ALL registered tool names
├── 2. Expand aliases via resolve_alias() (e.g., "core" → 14 toolsets)
├── 3. If enabled_toolsets → intersect (whitelist mode)
├── 4. Remove disabled_toolsets
├── 5. Filter by availability (is_available + check_fn)
└── Return: Set<tool_name>
```

## 2. CORE_TOOLS Constant

The single authoritative list shared across CLI and all gateway platforms. **77 entries** — verified against `toolsets.rs`:

```rust
pub const CORE_TOOLS: &[&str] = &[
    // Web (3)
    "web_search", "web_extract", "web_crawl",
    // Terminal + process management (4)
    "terminal", "run_process", "list_processes", "kill_process",
    // File manipulation (4)
    "read_file", "write_file", "patch", "search_files",
    // Skills (5)
    "skills_list", "skills_categories", "skill_view", "skill_manage", "skills_hub",
    // Browser automation (12)
    "browser_navigate", "browser_snapshot", "browser_screenshot",
    "browser_click", "browser_type", "browser_scroll",
    "browser_console", "browser_back", "browser_press",
    "browser_close", "browser_get_images", "browser_vision",
    // TTS (1)
    "text_to_speech",
    // Vision (1)
    "vision_analyze",
    // Audio transcription (1)
    "transcribe_audio",
    // Planning & memory (3)
    "manage_todo_list", "memory_read", "memory_write",
    // Honcho (6)
    "honcho_conclude", "honcho_search", "honcho_list",
    "honcho_remove", "honcho_profile", "honcho_context",
    // Home Assistant (4, runtime-gated)
    "ha_list_entities", "ha_get_state", "ha_list_services", "ha_call_service",
    // Session (1)
    "session_search",
    // Checkpoint (1)
    "checkpoint",
    // Clarify (1)
    "clarify",
    // Code execution + delegation (2)
    "execute_code", "delegate_task",
    // MoA (1)
    "mixture_of_agents",
    // Cron (1)
    "manage_cron_jobs",
    // MCP (6)
    "mcp_list_tools", "mcp_call_tool", "mcp_list_resources",
    "mcp_read_resource", "mcp_list_prompts", "mcp_get_prompt",
    // Advanced stubs (2, runtime-gated)
    "send_message", "generate_image",
];
```

> **Key corrections from old doc**: `"process"` → `"run_process"/"list_processes"/"kill_process"`, `"todo"` → `"manage_todo_list"`, `"memory"` → `"memory_read"/"memory_write"`, `"cronjob"` → `"manage_cron_jobs"`, added `web_crawl`, `skills_categories`, `skills_hub`, `browser_screenshot`, `honcho_list`, `honcho_remove`, all 6 MCP tools.

## 3. ACP_TOOLS Constant

Coding-focused subset for editor integration (VS Code, Zed, JetBrains). Excludes interactive/messaging tools:

```rust
pub const ACP_TOOLS: &[&str] = &[
    // Same as CORE_TOOLS minus:
    //   - clarify (no interactive UI)
    //   - send_message (no gateway)
    //   - generate_image (not coding)
    //   - text_to_speech (not coding)
    //   - manage_cron_jobs (not coding)
    //   - transcribe_audio (not coding)
    //   - vision_analyze (not coding)
    //   - ha_* (not coding)
    // Retains: file, terminal, web, browser, skills, memory,
    //          honcho, session, code execution, delegation, MoA, MCP
];
```

## 4. Toolset Aliases

`resolve_alias()` maps user-friendly names to toolset groups. Returns `Option<&'static [&'static str]>` (NOT `Vec`):

```rust
pub fn resolve_alias(alias: &str) -> Option<&'static [&'static str]> {
    match alias {
        "core" => Some(&[
            "core",           // checkpoint
            "file",           // read_file, write_file, patch, search_files
            "meta",           // manage_todo_list, clarify
            "scheduling",     // manage_cron_jobs
            "delegation",     // delegate_task
            "code_execution", // execute_code
            "session",        // session_search
            "mcp",            // mcp_list_tools, mcp_call_tool, …
            "browser",        // browser_* (runtime-gated)
        ]),
        "coding"    => Some(&["file", "terminal", "search", "code_execution"]),
        "research"  => Some(&["web", "browser", "vision"]),
        "debugging" => Some(&["terminal", "web", "file"]),
        "safe"      => Some(&["web", "vision", "image_gen", "moa"]),
        "all"       => Some(&[]),  // sentinel: include everything
        "minimal"   => Some(&["file", "terminal"]),
        "data_gen"  => Some(&["file", "terminal", "web", "code_execution"]),
        _ => None,  // not an alias — treat as literal toolset name
    }
}
```

### Alias Expansion Helper

```rust
/// Expand toolset names/aliases to canonical names, deduplicating.
pub fn expand_toolset_names(names: &[String]) -> Vec<String>

/// Check if "all" sentinel is present.
pub fn contains_all_sentinel(names: &[String]) -> bool
```

When `"all"` is present, caller passes `None` to `get_definitions()` instead of a whitelist (no filtering).

## 5. Toolset → Tool Name Mapping

| Toolset Name | Tool Names (exact) | Count |
|-------------|-------------------|-------|
| `file` | `read_file`, `write_file`, `patch`, `search_files` | 4 |
| `terminal` | `terminal`, `run_process`, `list_processes`, `kill_process` | 4 |
| `web` | `web_search`, `web_extract`, `web_crawl` | 3 |
| `browser` | `browser_navigate`, `browser_snapshot`, `browser_screenshot`, `browser_click`, `browser_type`, `browser_scroll`, `browser_console`, `browser_back`, `browser_press`, `browser_close`, `browser_get_images`, `browser_vision` | 12 |
| `skills` | `skills_list`, `skills_categories`, `skill_view`, `skill_manage`, `skills_hub` | 5 |
| `memory` | `memory_read`, `memory_write`, `manage_todo_list` | 3 |
| `honcho` | `honcho_conclude`, `honcho_search`, `honcho_list`, `honcho_remove`, `honcho_profile`, `honcho_context` | 6 |
| `homeassistant` | `ha_list_entities`, `ha_get_state`, `ha_list_services`, `ha_call_service` | 4 |
| `session` | `session_search` | 1 |
| `core` | `checkpoint` | 1 |
| `meta` | `clarify` | 1 |
| `media` | `text_to_speech`, `vision_analyze`, `transcribe_audio` | 3 |
| `code_execution` | `execute_code` | 1 |
| `delegation` | `delegate_task` | 1 |
| `moa` | `mixture_of_agents` | 1 |
| `scheduling` | `manage_cron_jobs` | 1 |
| `mcp` | `mcp_list_tools`, `mcp_call_tool`, `mcp_list_resources`, `mcp_read_resource`, `mcp_list_prompts`, `mcp_get_prompt` | 6 |
| `advanced` | `send_message`, `generate_image` | 2 |

## 6. Platform Defaults

| Platform | Default Tools | Notes |
|----------|--------------|-------|
| CLI | `CORE_TOOLS` (77) | Full interactive access |
| Telegram | `CORE_TOOLS` (77) | `send_message` gated on gateway |
| Discord | `CORE_TOOLS` (77) | `send_message` gated on gateway |
| WhatsApp | `CORE_TOOLS` (77) | `send_message` gated on gateway |
| Slack | `CORE_TOOLS` (77) | `send_message` gated on gateway |
| Signal | `CORE_TOOLS` (77) | `send_message` gated on gateway |
| Email | `CORE_TOOLS` (77) | `send_message` gated on gateway |
| SMS | `CORE_TOOLS` (77) | `send_message` gated on gateway |
| HomeAssistant | `CORE_TOOLS` (77) | HA tools gated on `HASS_TOKEN` |
| ACP (editor) | `ACP_TOOLS` subset | No clarify, send_message, image_gen, TTS |

> **Key insight**: All 8 messaging platforms share *identical* `CORE_TOOLS`. Platform differentiation happens via **runtime gating** (`check_fn`), NOT static tool lists. Only ACP has a reduced static subset.

## 7. Runtime-Gated Tools (check_fn)

These tools are in `CORE_TOOLS` but conditionally available at dispatch time:

| Tool | Gate Condition | Why |
|------|---------------|-----|
| `send_message` | Gateway process running | No messaging without gateway |
| `honcho_*` (6 tools) | Honcho enabled in config | No Honcho without backend |
| `ha_*` (4 tools) | `HASS_TOKEN` env var present | No HA without credentials |
| `browser_*` (12 tools) | Chrome binary or CDP override available | No browser without runtime |
| `generate_image` | Image generation API configured | No images without provider |

In EdgeCrab, runtime gating uses the `check_fn()` method on `ToolHandler`:

```rust
impl ToolHandler for SendMessageTool {
    fn check_fn(&self, ctx: &ToolContext) -> bool {
        ctx.gateway_sender.is_some()
    }
    // ...
}
```

## 8. Config.yaml Toolset Section

```yaml
tools:
  enabled_toolsets:
    - core            # expands to 14 toolset names via resolve_alias
    - web
    - terminal
    - memory
  disabled_toolsets:
    - homeassistant   # exclude HA tools even if HASS_TOKEN present
```
