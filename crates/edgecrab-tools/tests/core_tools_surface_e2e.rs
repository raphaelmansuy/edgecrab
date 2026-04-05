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

#[test]
fn browser_advantage_tools_are_exposed_in_core_and_acp_surfaces() {
    for tool_name in ["browser_wait_for", "browser_select", "browser_hover"] {
        assert_tool_in_surface(tool_name);
    }
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
        clarify_tx: None,
        approval_tx: None,
        on_skills_changed: None,
        gateway_sender: None,
        origin_chat: None,
        session_key: None,
        todo_store: None,
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
