//! # edgecrab-tools
//!
//! Tool registry (`ToolHandler` trait), toolset composition, and all tool
//! implementations. Uses `inventory` crate for compile-time registration.
//!
//! ```text
//!   edgecrab-tools
//!     ├── registry.rs     — ToolHandler trait, ToolRegistry, ToolContext
//!     ├── config_ref.rs   — AppConfigRef (lightweight config for tool context)
//!     ├── toolsets.rs      — CORE_TOOLS, ACP_TOOLS, alias resolution
//!     └── tools/           — individual tool implementations (Phase 2.2+)
//! ```

#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![allow(clippy::result_large_err)]

pub mod approval_runtime;
pub mod artifact_spill;
mod command_interaction;
pub mod config_ref;
pub mod delegation_state;
pub mod edit_contract;
pub mod execution_fs;
pub mod execution_tmp;
pub mod fuzzy_match;
mod local_pty;
pub mod kanban_gating;
pub mod lsp_gate;
#[cfg(target_os = "macos")]
pub mod macos_permissions;
#[cfg(not(target_os = "macos"))]
#[path = "macos_permissions_stub.rs"]
pub mod macos_permissions;
pub mod mutations;
pub mod mutation_turn_policy;
pub mod path_utils;
pub mod process_table;
pub mod provider_factory;
pub mod read_tracker;
pub mod recovery_catalog;
pub mod registry;
mod shell_syntax;
pub mod skills;
pub mod smart_approval;
pub mod subagent_ids;
pub mod tool_call_pipeline;
pub mod tool_argument_pipeline;
pub mod tool_name_repair;
pub mod tool_progress_tail;
pub mod tools;
pub mod toolsets;
pub mod vision_models;

/// Truncate `s` to at most `max_bytes` bytes, always stopping at a valid UTF-8
/// char boundary so that multi-byte / emoji characters are never split.
#[inline]
pub(crate) fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let boundary = (0..=max_bytes)
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0);
    &s[..boundary]
}

pub use artifact_spill::{
    SpillConfig, SpillOutcome, SpillSequence, SpillWritten, WEB_EXTRACT_INLINE_BYTES,
    WEB_SEARCH_INLINE_BYTES,
};
pub use config_ref::AppConfigRef;
pub use execution_fs::{ExecutionFilesystemView, describe_execution_filesystem};
pub use lsp_gate::{
    LspEditContext, LspGate, LspWriteHook, ToolDiagnostic, attach_post_write_diagnostics,
};
pub use mutations::{
    MutationKind, MutationRecord, MutationTurnState, extract_file_mutation_targets,
    file_mutation_result_landed, render_failure_footer, render_success_footer,
    render_success_footer_width,
};
pub use process_table::ProcessTable;
pub use provider_factory::{build_copilot_provider, create_provider_for_model};
pub use registry::{
    SubAgentResult, SubAgentRunner, ToolContext, ToolHandler, ToolProgressUpdate, ToolRegistry,
    to_llm_definitions,
};
pub use tool_argument_pipeline::{
    canonical_tool_args_json, parse_tool_arguments_json, prepare_parsed_tool_arguments,
    repair_stream_tool_arguments, repair_tool_arguments,
};
pub use tool_call_pipeline::{
    PreparedToolCall, MAX_INVALID_TOOL_RETRIES, classify_unknown_tool_batch,
    is_tool_registered, normalize_incoming_tool_call, prepare_tool_call,
    repair_tool_call_arguments_for_api, sanitize_assistant_tool_calls_for_api,
    unknown_tool_error_response, unknown_tool_names,
};
pub use tool_name_repair::{ResolvedToolName, fuzzy_match_tool_name, repair_tool_name, resolve_tool_call_name};
pub use smart_approval::handle_approvals_slash;
pub use edgecrab_security::approval::ApprovalMode;
pub use tools::checkpoint::{
    AutoPruneResult, CheckpointConfig, CheckpointManager, PruneCounts, RollbackOutcome,
    checkpoint_new_turn, clear_all, clear_legacy, format_checkpoint_list, format_store_status,
    handle_rollback_command, maybe_auto_prune_checkpoints, prune_checkpoints, store_status,
};
pub use tools::computer_use::permissions_status;
pub use tools::computer_use::{
    COMPUTER_USE_GUIDANCE_COMPACT, ComputerUseReportContext, ComputerUseStatusConfig,
    collect_snapshot, computer_command_overlay, computer_command_usage, computer_status_one_liner,
    format_computer_command, format_computer_enable_result, format_computer_setup_report,
    install_cua_driver, is_computer_use_toolset_active, open_computer_use_settings,
    parse_install_args, provider_accepts_multimodal_tool_result, render_install_report,
    should_route_capture_to_aux_vision,
};
pub use tools::todo::TodoStore;
pub use tools::web::{
    ResolvedChain, SearchResult, WebChainEditor, WebPickerCatalog, WebSearchBackend,
    WebSearchChainUpdate, WebSearchTool, WebSectionUpdate, active_search_section_override,
    capability_label, chain_summary_after_save, clear_web_search_chain_in_config,
    clear_web_search_section_overrides, clear_web_section_overrides, collect_web_diagnostics,
    default_chain_for_backend, effective_web_search_config, ensure_web_search_config_coherence,
    ensure_web_search_config_coherence_at, format_extract_doctor_detail, format_picker_label,
    format_saved_chain_summary, format_search_chain_summary, format_search_doctor_detail,
    format_web_search_result_count, format_web_search_status_line, format_web_setup_report,
    gateway_web_command_reply, get_web_search_backend, list_web_search_backends,
    load_web_search_config_from_disk, load_web_search_config_from_path,
    migrate_legacy_search_override, persist_search_backend_as_chain, persist_search_chain_order,
    persist_search_chain_with_timeout, persist_web_backend_in_config,
    persist_web_search_chain_in_config, persist_web_section_in_config, print_provider_detail_cli,
    provider_detail_lines, register_web_search_backend, render_web_dashboard, reset_web_to_auto,
    search_override_warning, search_section_override_from_path, summarize_web_search_backend,
    web_command_overlay, web_command_usage, web_menu_status_hint, web_provider_picker_rows,
    web_search_is_available, web_search_result_note, web_status_one_liner,
};
pub use toolsets::{
    ACP_TOOLS, CORE_TOOLS, LSP_TOOLS, MCP_EXTENDED_TOOLS, MOA_TOOLS, acp_tools,
    resolve_active_toolsets, resolve_alias,
};

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::Path;
    use std::sync::{Mutex, MutexGuard};

    use tempfile::TempDir;

    static EDGECRAB_HOME_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) struct TestEdgecrabHome {
        _guard: MutexGuard<'static, ()>,
        dir: TempDir,
        previous: Option<std::ffi::OsString>,
    }

    impl TestEdgecrabHome {
        pub(crate) fn new() -> Self {
            let guard = EDGECRAB_HOME_LOCK.lock().expect("lock");
            let dir = TempDir::new().expect("tempdir");
            let previous = std::env::var_os("EDGECRAB_HOME");
            // SAFETY: serialized by EDGECRAB_HOME_LOCK for the guard lifetime.
            unsafe { std::env::set_var("EDGECRAB_HOME", dir.path()) };
            Self {
                _guard: guard,
                dir,
                previous,
            }
        }

        pub(crate) fn path(&self) -> &Path {
            self.dir.path()
        }
    }

    impl Drop for TestEdgecrabHome {
        fn drop(&mut self) {
            match &self.previous {
                Some(previous) => {
                    // SAFETY: serialized by EDGECRAB_HOME_LOCK for the guard lifetime.
                    unsafe { std::env::set_var("EDGECRAB_HOME", previous) };
                }
                None => {
                    // SAFETY: serialized by EDGECRAB_HOME_LOCK for the guard lifetime.
                    unsafe { std::env::remove_var("EDGECRAB_HOME") };
                }
            }
        }
    }
}
