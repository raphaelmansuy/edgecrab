use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use edgecrab_lsp as _;
use edgecrab_tools::config_ref::LspServerConfigRef;
use edgecrab_tools::{ToolContext, ToolRegistry};
use edgecrab_types::Platform;
use edgequake_llm::{LLMProvider, MockProvider};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

fn workspace_manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root")
        .join("Cargo.toml")
}

fn mock_server_command() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn mock_server_args() -> Vec<String> {
    vec![
        "run".to_string(),
        "--quiet".to_string(),
        "--manifest-path".to_string(),
        workspace_manifest_path().display().to_string(),
        "-p".to_string(),
        "edgecrab-lsp".to_string(),
        "--example".to_string(),
        "mock_lsp_server".to_string(),
    ]
}

fn sample_source() -> &'static str {
    "fn compute(value: i32) -> i32 {\n    todo!()\n}\n"
}

fn make_ctx(workspace: &TempDir, home: &TempDir) -> ToolContext {
    let mut ctx = ToolContext {
        task_id: "lsp-test".into(),
        cwd: workspace.path().to_path_buf(),
        session_id: format!("lsp-test-{}", uuid::Uuid::new_v4()),
        user_task: None,
        cancel: CancellationToken::new(),
        config: edgecrab_tools::AppConfigRef::default(),
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
    };
    ctx.config.edgecrab_home = home.path().to_path_buf();
    ctx.config.lsp_servers = HashMap::from([(
        "rust".to_string(),
        LspServerConfigRef {
            command: mock_server_command(),
            args: mock_server_args(),
            file_extensions: vec!["rs".to_string()],
            language_id: "rust".to_string(),
            root_markers: vec!["Cargo.toml".to_string()],
            env: HashMap::new(),
            initialization_options: None,
        },
    )]);
    ctx
}

async fn dispatch_json(
    registry: &ToolRegistry,
    ctx: &ToolContext,
    tool: &str,
    args: Value,
) -> Value {
    let raw = registry
        .dispatch(tool, args, ctx)
        .await
        .unwrap_or_else(|err| panic!("{tool} failed: {err}"));
    serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("{tool} returned non-json: {err}\n{raw}"))
}

#[tokio::test]
async fn lsp_navigation_and_analysis_tools_work_end_to_end() {
    let workspace = TempDir::new().expect("workspace");
    let home = TempDir::new().expect("home");
    std::fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname='mock'\nversion='0.1.0'\n",
    )
    .expect("cargo");
    std::fs::write(workspace.path().join("main.rs"), sample_source()).expect("source");

    let registry = ToolRegistry::new();
    let mut ctx = make_ctx(&workspace, &home);

    let provider = Arc::new(MockProvider::new());
    provider
        .add_response(
            r#"[{"explanation":"The mocked diagnostic indicates a type mismatch.","suggested_fix":"Replace the placeholder with a concrete i32 value."}]"#,
        )
        .await;
    ctx.provider = Some(provider as Arc<dyn LLMProvider>);

    let def = dispatch_json(
        &registry,
        &ctx,
        "lsp_goto_definition",
        json!({"file":"main.rs","line":1,"column":4}),
    )
    .await;
    assert_eq!(def["found"], true);

    let refs = dispatch_json(
        &registry,
        &ctx,
        "lsp_find_references",
        json!({"file":"main.rs","line":1,"column":4}),
    )
    .await;
    assert_eq!(refs["count"], 2);

    let hover = dispatch_json(
        &registry,
        &ctx,
        "lsp_hover",
        json!({"file":"main.rs","line":1,"column":4}),
    )
    .await;
    assert_eq!(hover["found"], true);

    let doc_symbols = dispatch_json(
        &registry,
        &ctx,
        "lsp_document_symbols",
        json!({"file":"main.rs"}),
    )
    .await;
    assert_eq!(doc_symbols["symbols"][0]["name"], "compute");

    let ws_symbols = dispatch_json(
        &registry,
        &ctx,
        "lsp_workspace_symbols",
        json!({"query":"comp"}),
    )
    .await;
    assert_eq!(ws_symbols["symbols"][0]["name"], "compute");

    let impls = dispatch_json(
        &registry,
        &ctx,
        "lsp_goto_implementation",
        json!({"file":"main.rs","line":1,"column":4}),
    )
    .await;
    assert_eq!(impls["found"], true);

    let call_items = dispatch_json(
        &registry,
        &ctx,
        "lsp_call_hierarchy_prepare",
        json!({"file":"main.rs","line":1,"column":4}),
    )
    .await;
    let item = call_items["items"][0].clone();

    let incoming = dispatch_json(
        &registry,
        &ctx,
        "lsp_incoming_calls",
        json!({"item": item.clone()}),
    )
    .await;
    assert_eq!(incoming["calls"][0]["from"]["name"], "compute");

    let outgoing = dispatch_json(
        &registry,
        &ctx,
        "lsp_outgoing_calls",
        json!({"item": item.clone()}),
    )
    .await;
    assert_eq!(outgoing["calls"][0]["to"]["name"], "compute");

    let hints = dispatch_json(
        &registry,
        &ctx,
        "lsp_inlay_hints",
        json!({"file":"main.rs"}),
    )
    .await;
    assert_eq!(hints["hints"][0]["label"], " -> i32");

    let tokens = dispatch_json(
        &registry,
        &ctx,
        "lsp_semantic_tokens",
        json!({"file":"main.rs"}),
    )
    .await;
    assert_eq!(tokens["tokens"][0]["token_type"], "function");

    let signature = dispatch_json(
        &registry,
        &ctx,
        "lsp_signature_help",
        json!({"file":"main.rs","line":1,"column":10}),
    )
    .await;
    assert_eq!(signature["found"], true);

    let type_items = dispatch_json(
        &registry,
        &ctx,
        "lsp_type_hierarchy_prepare",
        json!({"file":"main.rs","line":1,"column":4}),
    )
    .await;
    let type_item = type_items["items"][0].clone();
    let supertypes = dispatch_json(
        &registry,
        &ctx,
        "lsp_supertypes",
        json!({"item": type_item.clone()}),
    )
    .await;
    assert_eq!(supertypes["items"][0]["name"], "BaseType");
    let subtypes = dispatch_json(&registry, &ctx, "lsp_subtypes", json!({"item": type_item})).await;
    assert_eq!(subtypes["items"][0]["name"], "ChildType");

    let linked = dispatch_json(
        &registry,
        &ctx,
        "lsp_linked_editing_range",
        json!({"file":"main.rs","line":1,"column":4}),
    )
    .await;
    assert_eq!(linked["ranges"].as_array().expect("ranges").len(), 2);

    let pull = dispatch_json(
        &registry,
        &ctx,
        "lsp_diagnostics_pull",
        json!({"file":"main.rs"}),
    )
    .await;
    assert_eq!(pull["diagnostics"][0]["code"], "E100");

    let workspace_pull = dispatch_json(
        &registry,
        &ctx,
        "lsp_diagnostics_pull",
        json!({"workspace":true}),
    )
    .await;
    assert_eq!(workspace_pull["reports"][0]["server"], "rust");

    let enriched = dispatch_json(
        &registry,
        &ctx,
        "lsp_enrich_diagnostics",
        json!({"file":"main.rs"}),
    )
    .await;
    assert!(
        enriched["diagnostics"][0]["explanation"]
            .as_str()
            .expect("explanation")
            .contains("type mismatch")
    );

    sleep(Duration::from_millis(100)).await;
    let workspace_errors =
        dispatch_json(&registry, &ctx, "lsp_workspace_type_errors", json!({})).await;
    assert_eq!(workspace_errors["total_errors"], 1);
}

#[tokio::test]
async fn lsp_mutation_tools_apply_workspace_edits() {
    let workspace = TempDir::new().expect("workspace");
    let home = TempDir::new().expect("home");
    std::fs::write(
        workspace.path().join("Cargo.toml"),
        "[package]\nname='mock'\nversion='0.1.0'\n",
    )
    .expect("cargo");
    std::fs::write(workspace.path().join("main.rs"), sample_source()).expect("source");

    let registry = ToolRegistry::new();
    let ctx = make_ctx(&workspace, &home);

    let actions = dispatch_json(
        &registry,
        &ctx,
        "lsp_code_actions",
        json!({"file":"main.rs","start_line":2,"start_column":5,"end_line":2,"end_column":12}),
    )
    .await;
    let action = actions["actions"][0]["action"].clone();
    assert_eq!(actions["actions"][0]["title"], "Replace todo with 42");

    let applied = dispatch_json(
        &registry,
        &ctx,
        "lsp_apply_code_action",
        json!({"action": action}),
    )
    .await;
    assert_eq!(applied["applied"], true);
    assert!(
        std::fs::read_to_string(workspace.path().join("main.rs"))
            .expect("read")
            .contains("42")
    );

    std::fs::write(workspace.path().join("main.rs"), sample_source()).expect("reset");
    let selected = dispatch_json(
        &registry,
        &ctx,
        "lsp_select_and_apply_action",
        json!({"file":"main.rs","start_line":2,"start_column":5,"end_line":2,"end_column":12,"action_index":0}),
    )
    .await;
    assert_eq!(selected["applied"], true);

    std::fs::write(workspace.path().join("main.rs"), sample_source()).expect("reset");
    let renamed = dispatch_json(
        &registry,
        &ctx,
        "lsp_rename",
        json!({"file":"main.rs","line":1,"column":4,"new_name":"renamed_value"}),
    )
    .await;
    assert_eq!(renamed["renamed"], true);

    std::fs::write(workspace.path().join("main.rs"), sample_source()).expect("reset");
    let formatted = dispatch_json(
        &registry,
        &ctx,
        "lsp_format_document",
        json!({"file":"main.rs"}),
    )
    .await;
    assert_eq!(formatted["formatted"], true);

    std::fs::write(workspace.path().join("main.rs"), sample_source()).expect("reset");
    let range_formatted = dispatch_json(
        &registry,
        &ctx,
        "lsp_format_range",
        json!({"file":"main.rs","start_line":2,"start_column":5,"end_line":2,"end_column":12}),
    )
    .await;
    assert_eq!(range_formatted["formatted"], true);
}
