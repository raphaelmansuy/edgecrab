//! E2E / unit proof: VisualUx preview lifecycle (game002 deadlock).
//!
//! Law: serve before perceive; never recommend localhost port shopping.

use edgecrab_core::harness_advisory::HarnessTurnAdvisory;
use edgecrab_core::harness_loop_policy::visual_storm_block_result_with_args;
use edgecrab_core::task_class::TaskClass;
use edgecrab_tools::dev_server::{
    is_preview_server_command, record_session_http_port_unchecked, session_http_server_ports,
};
use edgecrab_tools::recovery_catalog::{
    browser_navigate_blocked, infer_preview_serve_directory_from_text,
    preview_serve_then_navigate_recipe, tools_to_materialize_from_error_json,
};
use edgecrab_types::{Message, RecoveryAction};
use edgecrab_tools::{
    MaterializeSchemaStyle, MaterializedToolSet, ToolContext, ToolRegistry, ToolSchemaMode,
    build_wire_llm_definitions, materialize_tool_names, read_materialized_set, AppConfigRef,
};
use edgecrab_types::Platform;
use std::sync::{Arc, RwLock};
use tokio_util::sync::CancellationToken;

#[test]
fn game002_preview_server_exempt_from_visual_storm() {
    let mut adv = HarnessTurnAdvisory::new();
    for _ in 0..6 {
        adv.record_tool("write_file");
    }
    let messages = vec![Message::user(
        "Make demo/game002/index.html a beautiful HTML game UX and verify in the browser",
    )];
    assert_eq!(
        edgecrab_core::task_class::classify_from_messages(&messages),
        TaskClass::VisualUx
    );
    let serve_args = r#"{"command":"python3 -m http.server 8000 --directory demo/game002","background":true}"#;
    assert!(
        visual_storm_block_result_with_args(&adv, &messages, "terminal", serve_args).is_none(),
        "preview-server start must remain callable after create writes"
    );
    let ls_args = r#"{"command":"ls -la demo/game002"}"#;
    assert!(
        visual_storm_block_result_with_args(&adv, &messages, "terminal", ls_args).is_some(),
        "non-preview terminal storm must still block"
    );
}

#[test]
fn game002_empty_ports_recovery_is_exact_serve_recipe() {
    let dir = infer_preview_serve_directory_from_text(
        "Write a complete html5 game in ./demo/game002",
    );
    assert_eq!(dir, "demo/game002");
    let recipe = preview_serve_then_navigate_recipe(&dir);
    assert_eq!(recipe["tool"], "terminal");
    assert!(
        recipe["command"]
            .as_str()
            .unwrap_or("")
            .contains("http.server 8000 --directory demo/game002")
    );
    assert_eq!(recipe["then_url"], "http://127.0.0.1:8000/");

    let err = browser_navigate_blocked("http://127.0.0.1:8080/", "SSRF policy", &[]);
    let recovery = err.to_llm_payload().recovery_feedback.expect("recovery");
    assert!(
        recovery
            .suggestions
            .iter()
            .any(|s| s.action == RecoveryAction::CallToolFirst),
        "empty ports → CallToolFirst(terminal)"
    );
    let blob = serde_json::to_string(&recovery.suggestions).expect("json");
    assert!(blob.contains("http.server"));
    assert!(
        blob.contains("try other localhost ports") || blob.contains("forbidden"),
        "must forbid port shopping: {blob}"
    );
    // Recipe then_url is only :8000 — forbidden list may cite bad ports as examples.
    assert!(
        blob.contains("\"then_url\":\"http://127.0.0.1:8000/\""),
        "recovery then_url must be the canonical 8000 URL: {blob}"
    );
    assert!(
        !blob.contains("recommended_urls"),
        "empty known_ports must not emit recommended_urls: {blob}"
    );

    let targets = tools_to_materialize_from_error_json(&err.to_llm_response());
    assert!(
        targets.iter().any(|n| n == "terminal"),
        "recovery should auto-materialize terminal: {targets:?}"
    );
}

#[test]
fn game002_session_port_grounds_navigate_url() {
    let sid = "game002-e2e-session";
    // Test fixture: simulate a verified listen (unchecked) without binding a real server.
    record_session_http_port_unchecked(sid, 8000);
    let ports = session_http_server_ports(sid);
    assert_eq!(ports, vec![8000]);

    let err = browser_navigate_blocked("http://127.0.0.1:5050/", "SSRF policy", &ports);
    let recovery = err.to_llm_payload().recovery_feedback.expect("recovery");
    let blob = serde_json::to_string(&recovery.suggestions).expect("json");
    assert!(blob.contains("detected_http_server_ports"));
    assert!(blob.contains("http://127.0.0.1:8000/"));
    assert!(
        !blob.contains("5050"),
        "known ports must not advertise the failed guess: {blob}"
    );
}

#[test]
fn game002_port_shopping_halt_after_one_loopback_failure() {
    let mut adv = HarnessTurnAdvisory::new();
    let fail = edgecrab_tools::StructuredBrowserResult::navigate_err(
        "http://127.0.0.1:8000/",
        "Navigation error: net::ERR_CONNECTION_REFUSED",
    )
    .to_tool_result_json();
    adv.record_browser_navigate_result(&fail);
    let block = adv
        .maybe_loopback_port_shopping_block(
            "browser_navigate",
            r#"{"url":"http://127.0.0.1:5050/"}"#,
            &[],
            "demo/game002",
        )
        .expect("block");
    assert!(block.contains("Blocked") || block.contains("loopback_port_shopping_block"));
    assert!(is_preview_server_command(
        "python3 -m http.server 8000 --directory demo/game002"
    ));
}

#[test]
fn game002_visual_ux_materializes_browser_verify_tools() {
    let registry = ToolRegistry::new();
    let mat = Arc::new(RwLock::new(MaterializedToolSet::new()));
    let ctx = ToolContext {
        task_id: "t".into(),
        cwd: std::env::temp_dir(),
        session_id: "s".into(),
        user_task: Some(
            "Write a complete html5 game in ./demo/game002 and verify visually".into(),
        ),
        cancel: CancellationToken::new(),
        config: AppConfigRef {
            tool_schema_mode: ToolSchemaMode::Indexed,
            ..AppConfigRef::default()
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
        materialized_tools: Some(mat.clone()),
    };
    let schemas = registry.get_definitions(None, None, &ctx);
    let class = edgecrab_core::task_class::classify_from_messages(&[Message::user(
        "Write a complete html5 game in ./demo/game002/index.html with beautiful UX and verify in browser",
    )]);
    assert_eq!(class, TaskClass::VisualUx);

    let visual_tools = vec![
        "browser_navigate".to_string(),
        "browser_snapshot".to_string(),
    ];
    let outcome = materialize_tool_names(
        &visual_tools,
        &schemas,
        &mat,
        12,
        MaterializeSchemaStyle::Compact,
    );
    assert!(
        outcome.activated.iter().any(|n| n == "browser_navigate")
            || read_materialized_set(Some(&mat)).contains("browser_navigate"),
        "browser_navigate must materialize for VisualUx"
    );
    let wire = build_wire_llm_definitions(
        &registry,
        &ctx,
        None,
        None,
        ToolSchemaMode::Indexed,
        &mat.read().unwrap().names(),
        false,
    );
    let names: Vec<_> = wire.iter().map(|d| d.function.name.as_str()).collect();
    assert!(
        names.contains(&"browser_navigate"),
        "browser_navigate on wire: {names:?}"
    );
    assert!(
        names.contains(&"browser_snapshot"),
        "browser_snapshot on wire: {names:?}"
    );
}
