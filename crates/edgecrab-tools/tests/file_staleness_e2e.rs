use edgecrab_tools::{AppConfigRef, ToolContext, ToolRegistry};
use edgecrab_types::{Platform, ToolError};
use serde_json::json;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

fn ctx_in(dir: &std::path::Path, session_id: &str) -> ToolContext {
    ToolContext {
        task_id: format!("task-{session_id}"),
        cwd: dir.to_path_buf(),
        session_id: session_id.into(),
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
        watch_notification_tx: None,
    }
}

#[tokio::test]
async fn registry_blocks_stale_write_file_overwrite() {
    let dir = TempDir::new().expect("tmpdir");
    std::fs::write(dir.path().join("stale.txt"), "before\n").expect("seed");

    let registry = ToolRegistry::new();
    let ctx = ctx_in(dir.path(), "stale-e2e-write");

    registry
        .dispatch(
            "read_file",
            json!({"path": "stale.txt", "line_numbers": false}),
            &ctx,
        )
        .await
        .expect("read");

    std::fs::write(dir.path().join("stale.txt"), "external change\n").expect("modify");

    let err = registry
        .dispatch(
            "write_file",
            json!({"path": "stale.txt", "content": "replacement\n"}),
            &ctx,
        )
        .await
        .expect_err("stale write should be blocked");

    match err {
        ToolError::InvalidArgs { tool, message } => {
            assert_eq!(tool, "write_file");
            assert!(message.contains("modified since you last read it"));
        }
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}

#[tokio::test]
async fn registry_blocks_stale_apply_patch_update() {
    let dir = TempDir::new().expect("tmpdir");
    std::fs::write(
        dir.path().join("stale.rs"),
        "fn main() {\n    println!(\"old\");\n}\n",
    )
    .expect("seed");

    let registry = ToolRegistry::new();
    let ctx = ctx_in(dir.path(), "stale-e2e-patch");

    registry
        .dispatch(
            "read_file",
            json!({"path": "stale.rs", "line_numbers": false}),
            &ctx,
        )
        .await
        .expect("read");

    std::fs::write(
        dir.path().join("stale.rs"),
        "fn main() {\n    println!(\"new\");\n}\n",
    )
    .expect("modify");

    let err = registry
        .dispatch(
            "apply_patch",
            json!({
                "patch": "*** Begin Patch\n*** Update File: stale.rs\n@@\n-    println!(\"old\");\n+    println!(\"patched\");\n*** End Patch"
            }),
            &ctx,
        )
        .await
        .expect_err("stale patch should be blocked");

    match err {
        ToolError::InvalidArgs { tool, message } => {
            assert_eq!(tool, "apply_patch");
            assert!(message.contains("modified since you last read it"));
        }
        other => panic!("expected InvalidArgs, got {other:?}"),
    }
}
