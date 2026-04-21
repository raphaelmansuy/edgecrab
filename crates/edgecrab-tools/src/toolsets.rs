//! # Toolset composition — resolution, aliases, and platform defaults
//!
//! WHY separate from registry: The registry owns the tool map; this module
//! owns the *policy* for which tools are active. Separation lets us test
//! toolset logic independently and swap policies without touching dispatch.
//!
//! ```text
//!   resolve_toolset(enabled, disabled, config)
//!       ├── expand aliases ("coding" → ["file", "terminal", "search"])
//!       ├── intersect with enabled (whitelist mode)
//!       ├── subtract disabled
//!       └── return Set<toolset_name>
//! ```

/// Core tools shared across CLI and all gateway platforms.
///
/// Editing this list once updates all platforms. Platform differentiation
/// happens via runtime gating (check_fn), not static tool lists.
///
/// Design principle (hermes-agent `_HERMES_CORE_TOOLS` pattern):
/// Keep the core set minimal (~45 tools) so the LLM schema payload stays
/// under ~18K tokens. Specialised tools (LSP, MOA) live in on-demand
/// toolsets loaded via `enabled_toolsets: ["core", "lsp"]`.
pub const CORE_TOOLS: &[&str] = &[
    // Web (3)
    "web_search",
    "web_extract",
    "web_crawl",
    // Terminal + process management (8)
    "terminal",
    "run_process",
    "list_processes",
    "kill_process",
    "get_process_output",
    "wait_for_process",
    "write_stdin",
    "process",
    // File manipulation (5)
    "read_file",
    "write_file",
    "patch",
    "search_files",
    "pdf_to_markdown",
    // Skills (5)
    "skills_list",
    "skills_categories",
    "skill_view",
    "skill_manage",
    "skills_hub",
    // Browser (14)
    "browser_navigate",
    "browser_snapshot",
    "browser_screenshot",
    "browser_click",
    "browser_type",
    "browser_scroll",
    "browser_console",
    "browser_back",
    "browser_press",
    "browser_close",
    "browser_get_images",
    "browser_vision",
    "browser_wait_for",
    "browser_select",
    "browser_hover",
    // Media (4)
    "text_to_speech",
    "vision_analyze",
    "transcribe_audio",
    "generate_image",
    // Planning & memory (4)
    "manage_todo_list",
    "report_task_status",
    "memory_read",
    "memory_write",
    // Honcho — persistent cross-session user modeling (6, runtime-gated)
    "honcho_conclude",
    "honcho_search",
    "honcho_list",
    "honcho_remove",
    "honcho_profile",
    "honcho_context",
    // Home Assistant smart home control (4, runtime-gated)
    "ha_list_entities",
    "ha_get_state",
    "ha_list_services",
    "ha_call_service",
    // Session history search (1)
    "session_search",
    // Checkpoints (1)
    "checkpoint",
    // Clarifying questions (1)
    "clarify",
    // Code execution + delegation (2)
    "execute_code",
    "delegate_task",
    // Cron job management (1)
    "manage_cron_jobs",
    // MCP core (2) — extended MCP tools available via "mcp_ext" toolset
    "mcp_list_tools",
    "mcp_call_tool",
    // Cross-platform messaging (1, runtime-gated)
    "send_message",
];

/// On-demand LSP tools — loaded via `enabled_toolsets: ["core", "lsp"]`.
///
/// WHY separate: 19 LSP tools add ~7.6K tokens to schema payload.
/// Most tasks don't need semantic code intelligence; loading LSP tools
/// by default wastes 6% of the context window on every API call.
pub const LSP_TOOLS: &[&str] = &[
    "lsp_goto_definition",
    "lsp_find_references",
    "lsp_hover",
    "lsp_document_symbols",
    "lsp_workspace_symbols",
    "lsp_goto_implementation",
    "lsp_call_hierarchy_prepare",
    "lsp_incoming_calls",
    "lsp_outgoing_calls",
    "lsp_code_actions",
    "lsp_apply_code_action",
    "lsp_rename",
    "lsp_format_document",
    "lsp_format_range",
    "lsp_inlay_hints",
    "lsp_semantic_tokens",
    "lsp_signature_help",
    "lsp_type_hierarchy_prepare",
    "lsp_supertypes",
    "lsp_subtypes",
    "lsp_diagnostics_pull",
    "lsp_linked_editing_range",
    "lsp_enrich_diagnostics",
    "lsp_select_and_apply_action",
    "lsp_workspace_type_errors",
];

/// On-demand extended MCP tools — loaded via `enabled_toolsets: ["core", "mcp_ext"]`.
pub const MCP_EXTENDED_TOOLS: &[&str] = &[
    "mcp_list_resources",
    "mcp_read_resource",
    "mcp_list_prompts",
    "mcp_get_prompt",
];

/// On-demand Mixture-of-Agents tool — loaded via `enabled_toolsets: ["core", "moa"]`.
pub const MOA_TOOLS: &[&str] = &["moa"];

/// Tools EXCLUDED from ACP (editor integration) mode.
///
/// DRY: Instead of duplicating the full CORE_TOOLS list, we define only
/// the exclusions. `acp_tools()` derives the ACP set from CORE_TOOLS
/// minus these exclusions. Mirrors hermes-agent's pattern.
const ACP_EXCLUDED: &[&str] = &[
    "clarify",
    "send_message",
    "generate_image",
    "text_to_speech",
    "transcribe_audio",
    "vision_analyze",
    "ha_list_entities",
    "ha_get_state",
    "ha_list_services",
    "ha_call_service",
];

/// ACP (editor integration) tools — coding-focused subset of CORE_TOOLS.
///
/// Derived from CORE_TOOLS minus interactive/messaging/media tools.
/// Also includes LSP tools (always relevant in editor context).
pub fn acp_tools() -> Vec<&'static str> {
    let mut tools: Vec<&str> = CORE_TOOLS
        .iter()
        .filter(|t| !ACP_EXCLUDED.contains(t))
        .copied()
        .collect();
    tools.extend_from_slice(LSP_TOOLS);
    tools
}

/// Static ACP_TOOLS for backward compatibility with const contexts.
/// Prefer `acp_tools()` for runtime use.
pub const ACP_TOOLS: &[&str] = &[
    "web_search",
    "web_extract",
    "web_crawl",
    "terminal",
    "run_process",
    "list_processes",
    "kill_process",
    "get_process_output",
    "wait_for_process",
    "write_stdin",
    "process",
    "read_file",
    "write_file",
    "patch",
    "search_files",
    "pdf_to_markdown",
    "skills_list",
    "skills_categories",
    "skill_view",
    "skill_manage",
    "skills_hub",
    "browser_navigate",
    "browser_snapshot",
    "browser_screenshot",
    "browser_click",
    "browser_type",
    "browser_scroll",
    "browser_console",
    "browser_back",
    "browser_press",
    "browser_close",
    "browser_get_images",
    "browser_vision",
    "browser_wait_for",
    "browser_select",
    "browser_hover",
    "manage_todo_list",
    "report_task_status",
    "memory_read",
    "memory_write",
    "honcho_conclude",
    "honcho_search",
    "honcho_list",
    "honcho_remove",
    "honcho_profile",
    "honcho_context",
    "session_search",
    "checkpoint",
    "execute_code",
    "delegate_task",
    "manage_cron_jobs",
    "mcp_list_tools",
    "mcp_call_tool",
    // LSP tools always included in editor context
    "lsp_goto_definition",
    "lsp_find_references",
    "lsp_hover",
    "lsp_document_symbols",
    "lsp_workspace_symbols",
    "lsp_goto_implementation",
    "lsp_call_hierarchy_prepare",
    "lsp_incoming_calls",
    "lsp_outgoing_calls",
    "lsp_code_actions",
    "lsp_apply_code_action",
    "lsp_rename",
    "lsp_format_document",
    "lsp_format_range",
    "lsp_inlay_hints",
    "lsp_semantic_tokens",
    "lsp_signature_help",
    "lsp_type_hierarchy_prepare",
    "lsp_supertypes",
    "lsp_subtypes",
    "lsp_diagnostics_pull",
    "lsp_linked_editing_range",
    "lsp_enrich_diagnostics",
    "lsp_select_and_apply_action",
    "lsp_workspace_type_errors",
    // Extended MCP in ACP context
    "mcp_list_resources",
    "mcp_read_resource",
    "mcp_list_prompts",
    "mcp_get_prompt",
    // MOA in editor context
    "moa",
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolsetEnableSync {
    pub added_to_enabled: bool,
    pub removed_from_disabled: bool,
    pub still_blocked: bool,
}

/// Expand a toolset alias to its component toolset names.
///
/// WHY aliases: Users think in terms of workflows ("coding", "research"),
/// not individual toolset names. Aliases map intent to implementation.
///
/// Returns None for unknown aliases (caller should treat as literal).
///
/// ## Toolset name → tools mapping
/// - `"file"`         → read_file, write_file, patch, search_files
/// - `"terminal"`     → terminal, run_process, list_processes, kill_process
/// - `"web"`          → web_search, web_extract, web_crawl
/// - `"browser"`      → browser_navigate, browser_snapshot, …
/// - `"memory"`       → memory_read, memory_write, honcho_*
/// - `"skills"`       → skills_list, skills_categories, skill_view, skill_manage, skills_hub
/// - `"meta"`         → manage_todo_list, report_task_status, clarify
/// - `"scheduling"`   → manage_cron_jobs
/// - `"delegation"`   → delegate_task
/// - `"code_execution"` → execute_code
/// - `"session"`      → session_search
/// - `"mcp"`          → mcp_list_tools, mcp_call_tool
/// - `"media"`        → text_to_speech, vision_analyze, transcribe_audio
/// - `"messaging"`    → send_message
/// - `"lsp"`          → lsp_* language-server operations
/// - `"core"`         → checkpoint (the single core-labelled tool)
pub fn resolve_alias(alias: &str) -> Option<&'static [&'static str]> {
    match alias {
        // "core" is the meta-alias users get from `edgecrab setup`. It expands
        // to all built-in non-specialty toolsets so the agent has a full capability
        // set out of the box.
        //
        // DESIGN: LSP (19 tools, ~7.6K tokens) and MOA (1 tool) are excluded
        // from core to keep the default schema payload under ~18K tokens.
        // Users enable them explicitly: `enabled_toolsets: ["core", "lsp", "moa"]`
        //
        // WHY include scheduling/delegation/session/mcp/messaging/media in "core":
        // These are fundamental agent capabilities (cron, sub-agents, history,
        // MCP plugins, cross-platform delivery, and inbound media handling).
        // Every standard installation should have them. Messaging is still
        // runtime-gated by `check_fn` and `gateway_sender`, mirroring
        // hermes-agent's model. Media is included because gateway platforms
        // can receive images/audio by default.
        //
        // WHY include "browser" in core: Browser tools are runtime-gated by
        // browser_is_available() (requires Chrome binary OR CDP override), so
        // including the toolset label here is safe — tools silently absent when
        // no browser is reachable.
        "core" => Some(&[
            "core",           // checkpoint tool
            "file",           // read_file, write_file, patch, search_files
            "meta",           // manage_todo_list, report_task_status, clarify
            "scheduling",     // manage_cron_jobs
            "delegation",     // delegate_task
            "code_execution", // execute_code
            "session",        // session_search
            "mcp",            // mcp_list_tools, mcp_call_tool
            "messaging",      // send_message (runtime-gated in non-gateway sessions)
            "media",          // vision_analyze, transcribe_audio, text_to_speech
            "browser",        // browser_navigate, browser_snapshot, … (runtime-gated)
        ]),
        "coding" => Some(&["file", "terminal", "search", "code_execution", "lsp"]),
        "research" => Some(&["web", "browser", "vision"]),
        "debugging" => Some(&["terminal", "web", "file"]),
        "safe" => Some(&["web", "vision", "image_gen", "moa"]),
        "all" => Some(&[]), // sentinel: include everything
        "minimal" => Some(&["file", "terminal"]),
        "data_gen" => Some(&["file", "terminal", "web", "code_execution"]),
        _ => None, // not an alias — treat as literal toolset name
    }
}

/// Expand a list of toolset names / aliases to canonical component names.
///
/// Resolves each name through `resolve_alias()` and deduplicates the result.
/// Used in the conversation loop to normalise `config.enabled_toolsets` before
/// passing to `ToolRegistry::get_definitions()`.
///
/// # Example
/// ```
/// # use edgecrab_tools::toolsets::expand_toolset_names;
/// let names = vec!["core".to_string(), "web".to_string(), "terminal".to_string()];
/// let expanded = expand_toolset_names(&names);
/// assert!(expanded.contains(&"file".to_string()));
/// assert!(expanded.contains(&"scheduling".to_string()));
/// assert!(expanded.contains(&"terminal".to_string()));
/// ```
pub fn expand_toolset_names(names: &[String]) -> Vec<String> {
    let mut result: Vec<String> = names
        .iter()
        .flat_map(|s| match resolve_alias(s) {
            Some(expanded) if !expanded.is_empty() => {
                expanded.iter().map(|e| e.to_string()).collect::<Vec<_>>()
            }
            Some(_empty) => vec![], // "all" sentinel — caller should treat as None
            None => vec![s.clone()], // literal toolset name
        })
        .collect();
    result.sort();
    result.dedup();
    result
}

/// Check if the names list contains the "all" sentinel alias.
///
/// When present, the list means "no filtering" and `None` should be passed
/// to `get_definitions()` instead of an expanded set.
pub fn contains_all_sentinel(names: &[String]) -> bool {
    // A name is the "all" sentinel either as a literal "all" string OR as any
    // alias that resolves to the empty slice (the contract for the sentinel).
    names
        .iter()
        .any(|n| n == "all" || resolve_alias(n) == Some(&[]))
}

/// Whether a toolset is reachable under the current whitelist/blacklist policy.
pub fn toolset_enabled(
    enabled: Option<&[String]>,
    disabled: Option<&[String]>,
    toolset: &str,
) -> bool {
    let allowed = enabled.is_none_or(|sets| {
        sets.is_empty()
            || contains_all_sentinel(sets)
            || expand_toolset_names(sets)
                .iter()
                .any(|candidate| candidate == toolset)
    });
    let blocked = disabled.is_some_and(|sets| {
        expand_toolset_names(sets)
            .iter()
            .any(|candidate| candidate == toolset)
    });
    allowed && !blocked
}

/// Whether a specific tool is reachable under the current policy.
///
/// Precedence is explicit and stable:
/// 1. `disabled_tools` always blocks the tool.
/// 2. `enabled_tools` force-enables the tool even if its toolset is filtered out.
/// 3. Otherwise the containing toolset policy decides.
pub fn tool_enabled(
    enabled_toolsets: Option<&[String]>,
    disabled_toolsets: Option<&[String]>,
    enabled_tools: Option<&[String]>,
    disabled_tools: Option<&[String]>,
    tool_name: &str,
    toolset: &str,
) -> bool {
    if disabled_tools.is_some_and(|tools| tools.iter().any(|candidate| candidate == tool_name)) {
        return false;
    }

    if enabled_tools.is_some_and(|tools| tools.iter().any(|candidate| candidate == tool_name)) {
        return true;
    }

    toolset_enabled(enabled_toolsets, disabled_toolsets, toolset)
}

/// Ensure a literal toolset is reachable without mutating broader aliases.
pub fn ensure_literal_toolset_enabled(
    enabled: &mut Option<Vec<String>>,
    disabled: &mut Option<Vec<String>>,
    toolset: &str,
) -> ToolsetEnableSync {
    let mut sync = ToolsetEnableSync::default();

    if let Some(enabled_sets) = enabled.as_mut()
        && !enabled_sets.is_empty()
        && !contains_all_sentinel(enabled_sets)
        && !expand_toolset_names(enabled_sets)
            .iter()
            .any(|candidate| candidate == toolset)
    {
        enabled_sets.push(toolset.to_string());
        sync.added_to_enabled = true;
    }

    if let Some(disabled_sets) = disabled.as_mut() {
        let before = disabled_sets.len();
        disabled_sets.retain(|candidate| candidate != toolset);
        sync.removed_from_disabled = before != disabled_sets.len();
    }

    sync.still_blocked = !toolset_enabled(enabled.as_deref(), disabled.as_deref(), toolset);
    sync
}

/// Resolve which toolsets are active given enabled/disabled lists.
///
/// Pipeline:
/// 1. Expand aliases in enabled list
/// 2. Expand aliases in disabled list
/// 3. If enabled is specified, only those toolsets are active (whitelist)
/// 4. Remove disabled toolsets
pub fn resolve_active_toolsets(
    enabled: &[String],
    disabled: &[String],
    all_toolsets: &[&str],
) -> Vec<String> {
    // Helper: expand a list of names, treating aliases and literals
    let expand = |names: &[String]| -> Vec<String> {
        names
            .iter()
            .flat_map(|s| match resolve_alias(s) {
                Some(expanded) => expanded.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
                None => vec![s.clone()], // literal toolset name
            })
            .collect()
    };

    let enabled_expanded = if enabled.is_empty() {
        all_toolsets.iter().map(|s| s.to_string()).collect()
    } else {
        expand(enabled)
    };

    let disabled_expanded = expand(disabled);

    // Filter
    enabled_expanded
        .into_iter()
        .filter(|s| !disabled_expanded.contains(s))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_tools_not_empty() {
        assert!(!CORE_TOOLS.is_empty());
        assert!(CORE_TOOLS.contains(&"read_file"));
        assert!(CORE_TOOLS.contains(&"terminal"));
        assert!(CORE_TOOLS.contains(&"web_search"));
        assert!(CORE_TOOLS.contains(&"web_crawl"));
        assert!(CORE_TOOLS.contains(&"clarify"));
        assert!(CORE_TOOLS.contains(&"memory_read"));
        assert!(CORE_TOOLS.contains(&"manage_todo_list"));
        assert!(CORE_TOOLS.contains(&"checkpoint"));
        assert!(CORE_TOOLS.contains(&"mcp_list_tools"));
        assert!(CORE_TOOLS.contains(&"browser_wait_for"));
        assert!(CORE_TOOLS.contains(&"browser_select"));
        assert!(CORE_TOOLS.contains(&"browser_hover"));
    }

    #[test]
    fn lsp_tools_in_dedicated_constant() {
        assert!(!LSP_TOOLS.is_empty());
        assert!(LSP_TOOLS.contains(&"lsp_goto_definition"));
        assert!(LSP_TOOLS.contains(&"lsp_find_references"));
        assert!(LSP_TOOLS.contains(&"lsp_workspace_type_errors"));
        // LSP tools should NOT be in CORE_TOOLS (moved to on-demand)
        for tool in LSP_TOOLS {
            assert!(
                !CORE_TOOLS.contains(tool),
                "LSP tool '{tool}' should not be in CORE_TOOLS (moved to on-demand LSP_TOOLS)"
            );
        }
    }

    #[test]
    fn moa_not_in_core_tools() {
        assert!(
            !CORE_TOOLS.contains(&"moa"),
            "MOA should not be in CORE_TOOLS (moved to on-demand MOA_TOOLS)"
        );
        assert!(MOA_TOOLS.contains(&"moa"));
    }

    #[test]
    fn mcp_extended_not_in_core_tools() {
        for tool in MCP_EXTENDED_TOOLS {
            assert!(
                !CORE_TOOLS.contains(tool),
                "MCP extended tool '{tool}' should not be in CORE_TOOLS"
            );
        }
    }

    #[test]
    fn acp_tools_no_interactive() {
        assert!(!ACP_TOOLS.contains(&"clarify"));
        assert!(!ACP_TOOLS.contains(&"send_message"));
        assert!(!ACP_TOOLS.contains(&"generate_image"));
        assert!(!ACP_TOOLS.contains(&"text_to_speech"));
        assert!(ACP_TOOLS.contains(&"web_crawl"));
        assert!(ACP_TOOLS.contains(&"checkpoint"));
        assert!(ACP_TOOLS.contains(&"mcp_list_tools"));
        assert!(ACP_TOOLS.contains(&"browser_wait_for"));
        assert!(ACP_TOOLS.contains(&"browser_select"));
        assert!(ACP_TOOLS.contains(&"browser_hover"));
        // ACP includes LSP (editor context) and MOA
        assert!(ACP_TOOLS.contains(&"lsp_goto_definition"));
        assert!(ACP_TOOLS.contains(&"lsp_workspace_type_errors"));
        assert!(ACP_TOOLS.contains(&"moa"));
    }

    #[test]
    fn acp_tools_fn_matches_static_const() {
        let dynamic = acp_tools();
        // Every tool in the dynamic list should be in the static list
        for tool in &dynamic {
            assert!(
                ACP_TOOLS.contains(tool),
                "acp_tools() produced '{tool}' not in ACP_TOOLS const"
            );
        }
    }

    #[test]
    fn resolve_alias_coding() {
        let result = resolve_alias("coding").expect("should be known alias");
        assert!(result.contains(&"file"));
        assert!(result.contains(&"terminal"));
        assert!(
            result.contains(&"lsp"),
            "'coding' alias must include 'lsp' so semantic navigation and edits are discoverable in coding sessions"
        );
    }

    #[test]
    fn resolve_alias_unknown_is_none() {
        assert!(resolve_alias("custom_toolset").is_none());
    }

    #[test]
    fn resolve_alias_all_is_empty_sentinel() {
        let result = resolve_alias("all").expect("should be known alias");
        assert!(result.is_empty());
    }

    #[test]
    fn resolve_active_toolsets_whitelist() {
        let all = vec!["file", "terminal", "web", "browser"];
        let enabled = vec!["coding".to_string()];
        let disabled = vec![];

        let result = resolve_active_toolsets(&enabled, &disabled, &all);
        assert!(result.contains(&"file".to_string()));
        assert!(result.contains(&"terminal".to_string()));
        assert!(!result.contains(&"web".to_string()));
    }

    #[test]
    fn resolve_active_toolsets_blacklist() {
        let all = vec!["file", "terminal", "web", "browser"];
        let enabled = vec![];
        let disabled = vec!["web".to_string()];

        let result = resolve_active_toolsets(&enabled, &disabled, &all);
        assert!(result.contains(&"file".to_string()));
        assert!(!result.contains(&"web".to_string()));
    }

    #[test]
    fn tool_enabled_respects_explicit_tool_enable() {
        let enabled_toolsets = vec!["file".to_string()];
        let disabled_toolsets = vec!["web".to_string()];
        let enabled_tools = vec!["web_search".to_string()];

        assert!(tool_enabled(
            Some(&enabled_toolsets),
            Some(&disabled_toolsets),
            Some(&enabled_tools),
            None,
            "web_search",
            "web",
        ));
    }

    #[test]
    fn tool_enabled_respects_explicit_tool_disable() {
        let enabled_toolsets = vec!["web".to_string()];
        let disabled_tools = vec!["web_search".to_string()];

        assert!(!tool_enabled(
            Some(&enabled_toolsets),
            None,
            None,
            Some(&disabled_tools),
            "web_search",
            "web",
        ));
    }

    // ── New regression tests for the "core" alias and expand_toolset_names ──

    #[test]
    fn resolve_alias_core_includes_file_toolset() {
        // Critical regression: "core" MUST expand to include "file" so that
        // read_file / write_file / patch / search_files are available to the
        // agent when the default config `enabled_toolsets: [core, ...]` is used.
        let expanded = resolve_alias("core").expect("'core' must be a registered alias");
        assert!(
            expanded.contains(&"file"),
            "'core' alias must include 'file' toolset (read_file, write_file, patch)"
        );
    }

    #[test]
    fn resolve_alias_core_includes_meta_toolset() {
        // "meta" contains manage_todo_list, report_task_status, and clarify — needed for every session.
        let expanded = resolve_alias("core").expect("'core' must be a registered alias");
        assert!(
            expanded.contains(&"meta"),
            "'core' alias must include 'meta' toolset (manage_todo_list, report_task_status, clarify)"
        );
    }

    #[test]
    fn resolve_alias_core_includes_scheduling() {
        let expanded = resolve_alias("core").expect("'core' must be a registered alias");
        assert!(
            expanded.contains(&"scheduling"),
            "'core' alias must include 'scheduling' toolset (manage_cron_jobs)"
        );
    }

    #[test]
    fn resolve_alias_core_includes_lsp_toolset() {
        // LSP is no longer in "core" — it's an on-demand toolset.
        // Users enable it via `enabled_toolsets: ["core", "lsp"]`.
        let expanded = resolve_alias("core").expect("'core' must be a registered alias");
        assert!(
            !expanded.contains(&"lsp"),
            "'core' alias should NOT include 'lsp' — LSP is on-demand to reduce default tool count"
        );
    }

    #[test]
    fn resolve_alias_lsp_available_as_explicit_toolset() {
        // LSP can be enabled explicitly alongside core
        let names = vec!["core".to_string(), "lsp".to_string()];
        let expanded = expand_toolset_names(&names);
        assert!(expanded.contains(&"lsp".to_string()));
    }

    #[test]
    fn resolve_alias_core_includes_browser_toolset() {
        // Critical regression: "core" MUST include "browser" so that browser_navigate,
        // browser_snapshot, etc. appear in the LLM schema when a Chrome/CDP session is
        // active. Without "browser" in the expansion the LLM only sees mcp_call_tool
        // and incorrectly calls mcp_call_tool(tool_name="browser_navigate").
        // Browser tools are runtime-gated by browser_is_available() so this is safe
        // on machines without Chrome — tools are silently absent from the schema.
        let expanded = resolve_alias("core").expect("'core' must be a registered alias");
        assert!(
            expanded.contains(&"browser"),
            "'core' alias must include 'browser' toolset so browser_navigate is available when CDP is connected"
        );
    }

    #[test]
    fn resolve_alias_core_excludes_moa_toolset() {
        // MOA is no longer in "core" — it's an on-demand toolset.
        let expanded = resolve_alias("core").expect("'core' must be a registered alias");
        assert!(
            !expanded.contains(&"moa"),
            "'core' alias should NOT include 'moa' — MOA is on-demand"
        );
    }

    #[test]
    fn resolve_alias_core_includes_messaging_toolset() {
        let expanded = resolve_alias("core").expect("'core' must be a registered alias");
        assert!(
            expanded.contains(&"messaging"),
            "'core' alias must include 'messaging' so send_message is visible when gateway delivery is wired"
        );
    }

    #[test]
    fn resolve_alias_core_includes_media_toolset() {
        let expanded = resolve_alias("core").expect("'core' must be a registered alias");
        assert!(
            expanded.contains(&"media"),
            "'core' alias must include 'media' so inbound images/audio are usable in default sessions"
        );
    }

    #[test]
    fn expand_toolset_names_expands_core_alias() {
        // Simulates what conversation.rs does with the user's config.
        let config_toolsets = vec![
            "core".to_string(),
            "web".to_string(),
            "terminal".to_string(),
            "memory".to_string(),
            "skills".to_string(),
        ];
        let expanded = expand_toolset_names(&config_toolsets);
        // Must contain core-expanded toolsets
        assert!(
            expanded.contains(&"file".to_string()),
            "file must be in expanded list"
        );
        assert!(
            expanded.contains(&"meta".to_string()),
            "meta must be in expanded list"
        );
        assert!(
            expanded.contains(&"scheduling".to_string()),
            "scheduling must be in expanded list"
        );
        assert!(
            expanded.contains(&"messaging".to_string()),
            "messaging must be in expanded list"
        );
        assert!(
            expanded.contains(&"media".to_string()),
            "media must be in expanded list"
        );
        // LSP and MOA are NOT in core expansion (on-demand)
        assert!(
            !expanded.contains(&"lsp".to_string()),
            "lsp must NOT be in core-only expansion"
        );
        assert!(
            !expanded.contains(&"moa".to_string()),
            "moa must NOT be in core-only expansion"
        );
        // Must still contain explicit ones
        assert!(expanded.contains(&"web".to_string()));
        assert!(expanded.contains(&"terminal".to_string()));
        assert!(expanded.contains(&"memory".to_string()));
        assert!(expanded.contains(&"skills".to_string()));
        // No duplicates
        let mut sorted = expanded.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            expanded.len(),
            sorted.len(),
            "expanded list must be deduplicated"
        );
    }

    #[test]
    fn expand_toolset_names_passes_through_literal() {
        let names = vec!["terminal".to_string(), "web".to_string()];
        let expanded = expand_toolset_names(&names);
        assert_eq!(expanded, vec!["terminal".to_string(), "web".to_string()]);
    }

    #[test]
    fn expand_toolset_names_empty_input() {
        let expanded = expand_toolset_names(&[]);
        assert!(expanded.is_empty());
    }

    #[test]
    fn contains_all_sentinel_detects_all_alias() {
        let names = vec!["core".to_string(), "all".to_string()];
        assert!(contains_all_sentinel(&names));
    }

    #[test]
    fn contains_all_sentinel_false_for_normal_list() {
        let names = vec!["core".to_string(), "web".to_string()];
        assert!(!contains_all_sentinel(&names));
    }

    #[test]
    fn toolset_enabled_respects_alias_expansion() {
        let enabled = vec!["core".to_string()];
        // MOA is no longer in "core" — needs explicit enable
        assert!(!toolset_enabled(Some(&enabled), None, "moa"));
        // But file toolset IS in core
        assert!(toolset_enabled(Some(&enabled), None, "file"));
    }

    #[test]
    fn ensure_literal_toolset_enabled_adds_missing_whitelist_entry() {
        let mut enabled = Some(vec!["web".to_string(), "terminal".to_string()]);
        let mut disabled = Some(vec!["moa".to_string()]);

        let sync = ensure_literal_toolset_enabled(&mut enabled, &mut disabled, "moa");

        assert!(sync.added_to_enabled);
        assert!(sync.removed_from_disabled);
        assert!(!sync.still_blocked);
        assert!(enabled.expect("enabled").contains(&"moa".to_string()));
        assert!(
            !disabled
                .expect("disabled")
                .iter()
                .any(|candidate| candidate == "moa")
        );
    }

    #[test]
    fn ensure_literal_toolset_enabled_reports_alias_blocker() {
        let mut enabled = None;
        let mut disabled = Some(vec!["safe".to_string()]);

        let sync = ensure_literal_toolset_enabled(&mut enabled, &mut disabled, "moa");

        assert!(!sync.added_to_enabled);
        assert!(!sync.removed_from_disabled);
        assert!(sync.still_blocked);
    }
}
