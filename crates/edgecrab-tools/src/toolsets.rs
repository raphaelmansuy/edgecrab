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
/// Mirrors hermes-agent's `_HERMES_CORE_TOOLS`.
pub const CORE_TOOLS: &[&str] = &[
    // Web
    "web_search",
    "web_extract",
    "web_crawl",
    // Terminal + process management
    "terminal",
    "run_process",
    "list_processes",
    "kill_process",
    "get_process_output",
    "wait_for_process",
    "write_stdin",
    // File manipulation
    "read_file",
    "write_file",
    "patch",
    "search_files",
    "pdf_to_markdown",
    // Skills
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
    // TTS
    "text_to_speech",
    // Vision
    "vision_analyze",
    // Audio transcription
    "transcribe_audio",
    // Planning & memory
    "manage_todo_list",
    "memory_read",
    "memory_write",
    // Honcho — persistent cross-session user modeling
    "honcho_conclude",
    "honcho_search",
    "honcho_list",
    "honcho_remove",
    "honcho_profile",
    "honcho_context",
    // Home Assistant smart home control (runtime-gated)
    "ha_list_entities",
    "ha_get_state",
    "ha_list_services",
    "ha_call_service",
    // Session history search
    "session_search",
    // Checkpoints
    "checkpoint",
    // Clarifying questions
    "clarify",
    // Code execution + delegation
    "execute_code",
    "delegate_task",
    // Mixture of Agents — multi-model consensus reasoning
    "mixture_of_agents",
    // Cron job management
    "manage_cron_jobs",
    // MCP
    "mcp_list_tools",
    "mcp_call_tool",
    "mcp_list_resources",
    "mcp_read_resource",
    "mcp_list_prompts",
    "mcp_get_prompt",
    // Cross-platform messaging (runtime-gated, stub)
    "send_message",
    // Image generation (runtime-gated, stub)
    "generate_image",
];

/// ACP (editor integration) tools — coding-focused subset.
/// No clarify, send_message, generate_image, text_to_speech.
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
    "memory_read",
    "memory_write",
    // Honcho — persistent cross-session user modeling
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
    "mixture_of_agents",
    "mcp_list_tools",
    "mcp_call_tool",
    "mcp_list_resources",
    "mcp_read_resource",
    "mcp_list_prompts",
    "mcp_get_prompt",
];

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
/// - `"meta"`         → manage_todo_list, clarify
/// - `"scheduling"`   → manage_cron_jobs
/// - `"delegation"`   → delegate_task
/// - `"code_execution"` → execute_code
/// - `"session"`      → session_search
/// - `"mcp"`          → mcp_list_tools, mcp_call_tool
/// - `"media"`        → text_to_speech, vision_analyze, transcribe_audio
/// - `"messaging"`    → send_message
/// - `"core"`         → checkpoint (the single core-labelled tool)
pub fn resolve_alias(alias: &str) -> Option<&'static [&'static str]> {
    match alias {
        // "core" is the meta-alias users get from `edgecrab setup`. It expands
        // to all built-in non-specialty toolsets so the agent has a full capability
        // set out of the box. Without this expansion the user's config
        // `enabled_toolsets: [core, …]` only enables the `checkpoint` tool.
        //
        // WHY include scheduling/delegation/session/mcp/messaging/media in "core":
        // These are fundamental agent capabilities (cron, sub-agents, history,
        // MCP plugins, cross-platform delivery, and inbound media handling).
        // Every standard installation should have them. Messaging is still
        // runtime-gated by `check_fn` and `gateway_sender`, mirroring
        // hermes-agent's model. Media is included because gateway platforms
        // can receive images/audio by default; excluding `vision_analyze` and
        // `transcribe_audio` from the default tool surface causes attached
        // media scenarios to fail even though the transport itself works.
        //
        // WHY include "browser" in core: Browser tools are runtime-gated by
        // browser_is_available() (requires Chrome binary OR CDP override), so
        // including the toolset label here is safe — tools silently absent when
        // no browser is reachable. Without "browser" in core the LLM only sees
        // mcp_call_tool and calls mcp_call_tool(tool_name="browser_navigate")
        // instead of the direct browser_navigate tool.
        "core" => Some(&[
            "core",           // checkpoint tool
            "file",           // read_file, write_file, patch, search_files
            "meta",           // manage_todo_list, clarify
            "scheduling",     // manage_cron_jobs
            "delegation",     // delegate_task
            "code_execution", // execute_code
            "session",        // session_search
            "mcp",            // mcp_list_tools, mcp_call_tool
            "messaging",      // send_message (runtime-gated in non-gateway sessions)
            "media",          // vision_analyze, transcribe_audio, text_to_speech
            "browser",        // browser_navigate, browser_snapshot, … (runtime-gated)
        ]),
        "coding" => Some(&["file", "terminal", "search", "code_execution"]),
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
    }

    #[test]
    fn resolve_alias_coding() {
        let result = resolve_alias("coding").expect("should be known alias");
        assert!(result.contains(&"file"));
        assert!(result.contains(&"terminal"));
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
        // "meta" contains manage_todo_list and clarify — needed for every session.
        let expanded = resolve_alias("core").expect("'core' must be a registered alias");
        assert!(
            expanded.contains(&"meta"),
            "'core' alias must include 'meta' toolset (manage_todo_list, clarify)"
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
}
