use edgecrab_tools::{AppConfigRef, CORE_TOOLS, LSP_TOOLS, ToolContext, ToolRegistry, acp_tools};
use edgecrab_types::{Platform, ToolError};
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn assert_tool_in_acp_surface(tool_name: &str) {
    let acp = acp_tools();
    assert!(
        acp.contains(&tool_name),
        "acp_tools() should expose {tool_name}"
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
        assert!(
            CORE_TOOLS.contains(&tool_name),
            "CORE_TOOLS should expose {tool_name}"
        );
        assert_tool_in_acp_surface(tool_name);
    }
}

#[test]
fn moa_tool_is_opt_in_not_in_core_or_base_acp() {
    assert!(
        !CORE_TOOLS.contains(&"moa"),
        "MOA should not be in CORE_TOOLS"
    );
    assert!(
        !acp_tools().contains(&"moa"),
        "MOA should be opt-in via enabled_toolsets, not base acp_tools()"
    );
}

#[test]
fn lsp_tools_are_opt_in_not_in_base_acp_const() {
    for tool_name in CLAUDE_CODE_LSP_BASELINE {
        assert!(
            LSP_TOOLS.contains(tool_name),
            "LSP_TOOLS should expose {tool_name}"
        );
        assert!(
            !acp_tools().contains(tool_name),
            "LSP should load via enabled_toolsets in ACP, not base acp_tools()"
        );
    }
}

#[test]
fn edgecrab_lsp_advantage_tools_are_in_lsp_tools_only() {
    for tool_name in EDGECRAB_LSP_ADVANTAGE {
        assert!(
            LSP_TOOLS.contains(tool_name),
            "LSP_TOOLS should expose {tool_name}"
        );
        assert!(
            !acp_tools().contains(tool_name),
            "LSP advantage tools are opt-in, not base acp_tools()"
        );
    }
}

#[test]
fn edgecrab_lsp_surface_exceeds_claude_code_baseline() {
    let lsp_tools_count = LSP_TOOLS.len();

    assert_eq!(
        CLAUDE_CODE_LSP_BASELINE.len(),
        9,
        "baseline list should track Claude Code's 9 documented LSP operations"
    );
    assert!(
        lsp_tools_count > CLAUDE_CODE_LSP_BASELINE.len(),
        "LSP_TOOLS should expose more LSP operations than the 9-operation baseline"
    );
    assert_eq!(
        lsp_tools_count,
        CLAUDE_CODE_LSP_BASELINE.len() + EDGECRAB_LSP_ADVANTAGE.len(),
        "LSP_TOOLS should expose the full parity-plus LSP surface"
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
        delegate_agent_id: None,
        delegate_parent_id: None,
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
        watch_notification_tx: None,
        mutation_turn: None,
        lsp_gate: None,
        kanban_task_id: None,
        materialized_tools: None,
    };

    for tool_name in ["browser_wait_for", "browser_select", "browser_hover"] {
        let wait_for_err = registry.dispatch(tool_name, json!({}), &ctx).await;
        // Without a live browser, dispatch may return InvalidArgs (validation) or
        // a runtime unavailable error — either is acceptable.
        assert!(
            wait_for_err.is_err(),
            "dispatch for {tool_name} should fail without browser"
        );
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
