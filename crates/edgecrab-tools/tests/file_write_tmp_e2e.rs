//! E2E: virtual tmp path aliases (`/tmp/…`, `tmp/files/…`) through write_file + read_file.

use edgecrab_tools::{ToolContext, ToolRegistry};
use edgecrab_types::Platform;
use serde_json::json;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

fn ctx_in(workspace: &std::path::Path, edgecrab_home: &std::path::Path) -> ToolContext {
    ToolContext {
        task_id: "file-write-tmp-e2e".into(),
        cwd: workspace.to_path_buf(),
        session_id: "file-write-tmp-e2e-session".into(),
        user_task: None,
        cancel: CancellationToken::new(),
        config: edgecrab_tools::AppConfigRef {
            edgecrab_home: edgecrab_home.to_path_buf(),
            ..Default::default()
        },
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
    }
}

async fn write_then_read(registry: &ToolRegistry, ctx: &ToolContext, path: &str, content: &str) {
    registry
        .dispatch("write_file", json!({"path": path, "content": content}), ctx)
        .await
        .unwrap_or_else(|e| panic!("write_file {path} failed: {e}"));

    let body = registry
        .dispatch(
            "read_file",
            json!({"path": path, "line_numbers": false}),
            ctx,
        )
        .await
        .unwrap_or_else(|e| panic!("read_file {path} failed: {e}"));
    assert!(
        body.contains(content),
        "read back should contain written content for {path}"
    );
}

#[tokio::test]
async fn e2e_write_read_absolute_tmp_alias() {
    let workspace = TempDir::new().expect("workspace");
    let edgecrab_home = TempDir::new().expect("edgecrab_home");
    let registry = ToolRegistry::new();
    let ctx = ctx_in(workspace.path(), edgecrab_home.path());

    write_then_read(&registry, &ctx, "/tmp/osint_report.md", "# OSINT report\n").await;

    let mapped = edgecrab_home.path().join("tmp/files/osint_report.md");
    assert!(
        mapped.is_file(),
        "mapped file must exist at {}",
        mapped.display()
    );
}

#[tokio::test]
async fn e2e_write_read_relative_tmp_files_alias() {
    let workspace = TempDir::new().expect("workspace");
    let edgecrab_home = TempDir::new().expect("edgecrab_home");
    let registry = ToolRegistry::new();
    let ctx = ctx_in(workspace.path(), edgecrab_home.path());

    write_then_read(
        &registry,
        &ctx,
        "tmp/files/raphael_osint_report.md",
        "# Raphaël OSINT\n",
    )
    .await;

    let mapped = edgecrab_home
        .path()
        .join("tmp/files/raphael_osint_report.md");
    assert!(
        mapped.is_file(),
        "relative tmp/files must map to edgecrab_home: {}",
        mapped.display()
    );
    assert!(
        !workspace.path().join("tmp").exists(),
        "must not create workspace-local tmp/ tree"
    );
}

#[tokio::test]
async fn e2e_write_read_nested_tmp_files_subdirs_auto_created() {
    let workspace = TempDir::new().expect("workspace");
    let edgecrab_home = TempDir::new().expect("edgecrab_home");
    let registry = ToolRegistry::new();
    let ctx = ctx_in(workspace.path(), edgecrab_home.path());

    write_then_read(
        &registry,
        &ctx,
        "tmp/files/osint/2026/report.md",
        "nested\n",
    )
    .await;

    let mapped = edgecrab_home.path().join("tmp/files/osint/2026/report.md");
    assert!(mapped.is_file());
}
