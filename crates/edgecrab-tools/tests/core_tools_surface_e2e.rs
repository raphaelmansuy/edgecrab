use edgecrab_tools::{ACP_TOOLS, AppConfigRef, CORE_TOOLS, ToolContext, ToolRegistry};
use edgecrab_types::{Platform, ToolError};
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn assert_tool_in_surface(tool_name: &str) {
    assert!(
        CORE_TOOLS.contains(&tool_name),
        "CORE_TOOLS should expose {tool_name}"
    );
    assert!(
        ACP_TOOLS.contains(&tool_name),
        "ACP_TOOLS should expose {tool_name}"
    );
}

const CLAUDE_CODE_LSP_BASELINE: &[&str] = &[
    "lsp_goto_definition",
    "lsp_find_references",
    "lsp_hover",
    "lsp_document_symbols",
    "lsp_workspace_symbols",
    "lsp_goto_implementation",
    "lsp_call_hierarchy_prepare",
    "lsp_incoming_calls",
    "lsp_outgoing_calls",
];

const EDGECRAB_LSP_ADVANTAGE: &[&str] = &[
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

#[test]
fn browser_advantage_tools_are_exposed_in_core_and_acp_surfaces() {
    for tool_name in ["browser_wait_for", "browser_select", "browser_hover"] {
        assert_tool_in_surface(tool_name);
    }
}

#[test]
fn moa_tool_is_exposed_in_core_and_acp_surfaces() {
    assert_tool_in_surface("moa");
}

#[test]
fn claude_code_lsp_parity_tools_are_exposed_in_core_and_acp_surfaces() {
    for tool_name in CLAUDE_CODE_LSP_BASELINE {
        assert_tool_in_surface(tool_name);
    }
}

#[test]
fn edgecrab_lsp_advantage_tools_are_exposed_in_core_and_acp_surfaces() {
    for tool_name in EDGECRAB_LSP_ADVANTAGE {
        assert_tool_in_surface(tool_name);
    }
}

#[test]
fn edgecrab_lsp_surface_exceeds_claude_code_baseline() {
    let core_lsp_count = CORE_TOOLS
        .iter()
        .filter(|name| name.starts_with("lsp_"))
        .count();
    let acp_lsp_count = ACP_TOOLS
        .iter()
        .filter(|name| name.starts_with("lsp_"))
        .count();

    assert_eq!(
        CLAUDE_CODE_LSP_BASELINE.len(),
        9,
        "baseline list should track Claude Code's 9 documented LSP operations"
    );
    assert!(
        core_lsp_count > CLAUDE_CODE_LSP_BASELINE.len(),
        "CORE_TOOLS should expose more LSP operations than the 9-operation baseline"
    );
    assert!(
        acp_lsp_count > CLAUDE_CODE_LSP_BASELINE.len(),
        "ACP_TOOLS should expose more LSP operations than the 9-operation baseline"
    );
    assert_eq!(
        core_lsp_count,
        CLAUDE_CODE_LSP_BASELINE.len() + EDGECRAB_LSP_ADVANTAGE.len(),
        "CORE_TOOLS should expose the full parity-plus LSP surface"
    );
    assert_eq!(
        acp_lsp_count,
        CLAUDE_CODE_LSP_BASELINE.len() + EDGECRAB_LSP_ADVANTAGE.len(),
        "ACP_TOOLS should expose the full parity-plus LSP surface"
    );
}

#[tokio::test]
async fn browser_advantage_tools_dispatch_through_registry_with_edge_case_validation() {
    let registry = ToolRegistry::new();
    let ctx = ToolContext {
        task_id: "test-task".into(),
        cwd: std::env::temp_dir(),
        session_id: "test-session".into(),
        user_task: None,
        cancel: CancellationToken::new(),
        config: AppConfigRef::default(),
        state_db: None,
        platform: Platform::Cli,
        process_table: None,
        provider: None,
        tool_registry: None,
        delegate_depth: 0,
        sub_agent_runner: None,
        delegation_event_tx: None,
        clarify_tx: None,
        approval_tx: None,
        on_skills_changed: None,
        gateway_sender: None,
        origin_chat: None,
        session_key: None,
        todo_store: None,
        current_tool_call_id: None,
        current_tool_name: None,
        injected_messages: None,
        tool_progress_tx: None,
    };

    let wait_for_err = registry
        .dispatch("browser_wait_for", json!({}), &ctx)
        .await
        .expect_err("browser_wait_for should reject empty conditions");
    match wait_for_err {
        ToolError::InvalidArgs { tool, message } => {
            assert_eq!(tool, "browser_wait_for");
            assert!(message.contains("Provide at least one of"));
        }
        other => panic!("expected InvalidArgs for browser_wait_for, got {other:?}"),
    }

    let select_err = registry
        .dispatch("browser_select", json!({"ref": "@e1"}), &ctx)
        .await
        .expect_err("browser_select should reject missing option");
    match select_err {
        ToolError::InvalidArgs { tool, .. } => assert_eq!(tool, "browser_select"),
        other => panic!("expected InvalidArgs for browser_select, got {other:?}"),
    }

    let hover_err = registry
        .dispatch("browser_hover", json!({}), &ctx)
        .await
        .expect_err("browser_hover should reject missing ref");
    match hover_err {
        ToolError::InvalidArgs { tool, .. } => assert_eq!(tool, "browser_hover"),
        other => panic!("expected InvalidArgs for browser_hover, got {other:?}"),
    }
}
