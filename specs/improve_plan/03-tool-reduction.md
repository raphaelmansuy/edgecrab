# 03 — P0: Tool Count Reduction

**Priority**: P0 (highest leverage fix)
**Impact**: Fixes schema overflow, improves LLM accuracy, enables strict mode
**Risk**: Low — tools are moved, not deleted. Features preserved via on-demand loading.
**Cross-ref**: [01-diagnosis.md](01-diagnosis.md) RC-1, [02-hermes-patterns.md](02-hermes-patterns.md) Pattern 1

## WHY This Is P0

Every tool schema averages ~400 tokens. Reducing from 173 to ~41 tools
saves (173-41) * 400 = ~52,800 tokens per API call.

```
BEFORE (173 tools):
+-------------------------------------------------------+
| System prompt (~8K) | Tool schemas (~30K) | Conv (~90K)|
+-------------------------------------------------------+
                       ^^^^^^^^^^^^^^^^^^^^
                       24% of 128K context consumed

AFTER (~41 tools):
+-------------------------------------------------------+
| System prompt (~8K) | Tools (~16K) | Conv (~104K)      |
+-------------------------------------------------------+
                       ^^^^^^^^^^^^
                       12% of context — 14K tokens freed
```

The 14K tokens freed go to actual reasoning, improving:
- Correct tool selection (fewer choices = fewer wrong calls)
- Anthropic strict mode compliance (41 tools < 20-tool limit still
  exceeded, but optional params drop from 142 to ~30)
- Parameter accuracy (more attention budget per tool)

## Implementation: CORE_TOOLS Reduction

### Tools KEPT in CORE_TOOLS (~41)

```rust
pub const CORE_TOOLS: &[&str] = &[
    // Web (3)
    "web_search", "web_extract", "web_crawl",
    // Terminal (2) — run_process replaces 5 granular process tools
    "terminal", "run_process",
    // File (5)
    "read_file", "write_file", "patch", "search_files", "pdf_to_markdown",
    // Skills (3)
    "skills_list", "skill_view", "skill_manage",
    // Browser core (7) — reduced from 15
    "browser_navigate", "browser_snapshot", "browser_screenshot",
    "browser_click", "browser_type", "browser_scroll", "browser_console",
    // Media (3)
    "text_to_speech", "vision_analyze", "transcribe_audio",
    // Planning & memory (5)
    "manage_todo_list", "report_task_status", "memory_read", "memory_write",
    "checkpoint",
    // Session (1)
    "session_search",
    // Clarify (1)
    "clarify",
    // Code execution + delegation (2)
    "execute_code", "delegate_task",
    // Cron (1)
    "manage_cron_jobs",
    // MCP core (2) — reduced from 6
    "mcp_list_tools", "mcp_call_tool",
    // Messaging (1, runtime-gated)
    "send_message",
    // Image generation (1, runtime-gated)
    "generate_image",
    // Home Assistant (4, runtime-gated)
    "ha_list_entities", "ha_get_state", "ha_list_services", "ha_call_service",
];
```

### Tools MOVED to on-demand toolsets

```rust
// Loaded only when explicitly enabled via config or /toolsets command
const LSP_TOOLS: &[&str] = &[
    "lsp_goto_definition", "lsp_find_references", "lsp_hover",
    "lsp_document_symbols", "lsp_workspace_symbols",
    "lsp_goto_implementation", "lsp_call_hierarchy_prepare",
    "lsp_incoming_calls", "lsp_outgoing_calls",
    "lsp_code_actions", "lsp_apply_code_action",
    "lsp_rename", "lsp_format_document", "lsp_format_range",
    "lsp_inlay_hints", "lsp_semantic_tokens", "lsp_signature_help",
    "lsp_type_hierarchy_prepare", "lsp_supertypes", "lsp_subtypes",
    "lsp_diagnostics_pull", "lsp_linked_editing_range",
    "lsp_enrich_diagnostics", "lsp_select_and_apply_action",
    "lsp_workspace_type_errors",
];

const HONCHO_TOOLS: &[&str] = &[
    "honcho_conclude", "honcho_search", "honcho_list",
    "honcho_remove", "honcho_profile", "honcho_context",
];

const PROCESS_MGMT_TOOLS: &[&str] = &[
    "list_processes", "kill_process", "get_process_output",
    "wait_for_process", "write_stdin",
];

const BROWSER_ADVANCED_TOOLS: &[&str] = &[
    "browser_back", "browser_press", "browser_close",
    "browser_get_images", "browser_vision",
    "browser_wait_for", "browser_select", "browser_hover",
];

const MCP_EXTENDED_TOOLS: &[&str] = &[
    "mcp_list_resources", "mcp_read_resource",
    "mcp_list_prompts", "mcp_get_prompt",
];

const SKILLS_EXTENDED_TOOLS: &[&str] = &[
    "skills_categories", "skills_hub",
];

const MOA_TOOLS: &[&str] = &["moa"];
```

## Implementation: ACP_TOOLS as Exclusion (DRY)

```rust
const ACP_EXCLUDED: &[&str] = &[
    "clarify", "send_message", "generate_image",
    "text_to_speech", "transcribe_audio", "vision_analyze",
    "ha_list_entities", "ha_get_state", "ha_list_services", "ha_call_service",
];

pub fn acp_tools() -> Vec<&'static str> {
    CORE_TOOLS.iter()
        .filter(|t| !ACP_EXCLUDED.contains(t))
        .copied()
        .collect()
}
```

## Implementation: expand_alias Update

```rust
pub fn expand_alias(alias: &str) -> Option<Vec<&'static str>> {
    match alias {
        // ... existing aliases ...
        "lsp"         => Some(LSP_TOOLS.to_vec()),
        "honcho"      => Some(HONCHO_TOOLS.to_vec()),
        "process_mgmt"=> Some(PROCESS_MGMT_TOOLS.to_vec()),
        "browser_adv" => Some(BROWSER_ADVANCED_TOOLS.to_vec()),
        "browser_ext" => Some(BROWSER_EXTENDED_TOOLS.to_vec()),
        "mcp_ext"     => Some(MCP_EXTENDED_TOOLS.to_vec()),
        "skills_ext"  => Some(SKILLS_EXTENDED_TOOLS.to_vec()),
        "moa"         => Some(MOA_TOOLS.to_vec()),
        _ => None,
    }
}
```

## Edge Cases Preserved

1. **LSP users**: Can enable via `enabled_toolsets: ["core", "lsp"]` in config
2. **Honcho users**: Can enable via `enabled_toolsets: ["core", "honcho"]`
3. **Process management**: `run_process` + `terminal background=true` cover 95% of use cases.
   The granular tools are available via `enabled_toolsets: ["core", "process_mgmt"]`
4. **Advanced browser**: Available via `enabled_toolsets: ["core", "browser_adv"]`
5. **MCP power users**: `mcp_list_tools` + `mcp_call_tool` handle all cases.
   Extended tools available via `enabled_toolsets: ["core", "mcp_ext"]`
6. **"all" alias**: Still loads everything (CORE + all on-demand toolsets)
7. **Backward compat**: Users with `enabled_toolsets: ["all"]` get same behavior
