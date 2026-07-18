//! E2E: Tool Progressive Load (July 2026) — mock provider, no network.
//!
//! - Deferred gate → tool_search → success
//! - toolset pack materialize
//! - Auto threshold Compact vs Indexed
//! - Local hot schemas keep property descriptions
//! - Turn-start BM25 prefetch

use std::sync::{Arc, RwLock};

use edgecrab_core::AgentBuilder;
use edgecrab_tools::{
    AUTO_INDEXED_TOOL_COUNT_THRESHOLD, AppConfigRef, MaterializeSchemaStyle, MaterializedToolSet,
    ToolContext, ToolRegistry, ToolSchemaMode, build_wire_llm_definitions,
    materialize_tool_names, prefetch_tools_for_user_message, read_materialized_set,
    resolve_effective_schema_mode, wire_schemas,
};
use edgecrab_types::{Platform, ToolSchema};
use edgequake_llm::providers::MockAgentProvider;
use edgequake_llm::{FunctionCall, LLMProvider, ToolCall};
use serde_json::json;
use tokio_util::sync::CancellationToken;

fn schema_ctx(registry: Arc<ToolRegistry>, mat: Arc<RwLock<MaterializedToolSet>>) -> ToolContext {
    ToolContext {
        task_id: "e2e".into(),
        cwd: std::env::temp_dir(),
        session_id: "e2e".into(),
        user_task: None,
        cancel: CancellationToken::new(),
        config: AppConfigRef::default(),
        state_db: None,
        platform: Platform::Cli,
        process_table: None,
        provider: None,
        tool_registry: Some(registry),
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
        materialized_tools: Some(mat),
    }
}

#[tokio::test]
async fn deferred_blocked_until_tool_search_then_succeeds() {
    let provider = MockAgentProvider::new();
    provider.add_tool_response_sync(
        "",
        vec![ToolCall {
            id: "c1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "browser_navigate".into(),
                arguments: r#"{"url":"https://example.com"}"#.into(),
            },
            thought_signature: None,
        }],
    );
    provider.add_tool_response_sync(
        "",
        vec![ToolCall {
            id: "c2".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "tool_search".into(),
                arguments: r#"{"tool_names":["browser_navigate"]}"#.into(),
            },
            thought_signature: None,
        }],
    );
    provider.add_tool_response_sync(
        "",
        vec![ToolCall {
            id: "c3".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "browser_navigate".into(),
                arguments: r#"{"url":"https://example.com"}"#.into(),
            },
            thought_signature: None,
        }],
    );
    provider.add_response_sync("Navigated after materialize.");

    let provider: Arc<dyn LLMProvider> = Arc::new(provider);
    let registry = Arc::new(ToolRegistry::new());
    let agent = AgentBuilder::new("mock-agent")
        .provider(provider)
        .tools(registry)
        .tool_schema_mode(ToolSchemaMode::Indexed)
        .skip_context_files(true)
        .skip_memory(true)
        .max_iterations(12)
        .streaming(false)
        .build()
        .expect("build");

    // Avoid turn-start BM25 prefetch matching browser_* (would skip the gate).
    let result = agent
        .run_conversation("Continue with the planned activation sequence.", None, None)
        .await
        .expect("run");

    let tool_msgs: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == edgecrab_types::Role::Tool)
        .collect();
    assert!(
        tool_msgs.iter().any(|m| {
            m.name.as_deref() == Some("browser_navigate")
                && m.text_content().contains("tool_search")
        }),
        "first deferred call must be blocked with tool_search hint"
    );
    assert!(
        tool_msgs
            .iter()
            .any(|m| m.name.as_deref() == Some("tool_search")),
        "tool_search must run"
    );
    // After materialize, a later browser_navigate result should not be the deferred gate.
    let nav_results: Vec<_> = tool_msgs
        .iter()
        .filter(|m| m.name.as_deref() == Some("browser_navigate"))
        .map(|m| m.text_content())
        .collect();
    assert!(
        nav_results
            .iter()
            .any(|t| !t.contains("not on your wire schema")),
        "after tool_search, browser_navigate should succeed past the gate; got {nav_results:?}"
    );
}

#[tokio::test]
async fn toolset_bulk_materializes_pack() {
    let registry = Arc::new(ToolRegistry::new());
    let mat = Arc::new(RwLock::new(MaterializedToolSet::new()));
    let ctx = schema_ctx(registry.clone(), mat.clone());
    let out = registry
        .dispatch("tool_search", json!({ "toolset": "browser" }), &ctx)
        .await
        .expect("toolset dispatch");
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("json");
    let activated = parsed["activated"].as_array().expect("activated");
    assert!(!activated.is_empty());
    assert!(activated.len() <= edgecrab_tools::MAX_TOOLSET_MATERIALIZE);
    assert!(mat.read().unwrap().len() >= 1);
}

#[test]
fn auto_small_surface_uses_compact() {
    assert_eq!(
        resolve_effective_schema_mode(ToolSchemaMode::Auto, AUTO_INDEXED_TOOL_COUNT_THRESHOLD),
        ToolSchemaMode::Compact
    );
    let registry = ToolRegistry::new();
    let ctx = ToolContext {
        task_id: "t".into(),
        cwd: std::env::temp_dir(),
        session_id: "s".into(),
        user_task: None,
        cancel: CancellationToken::new(),
        config: AppConfigRef {
            tool_schema_mode: ToolSchemaMode::Auto,
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
        materialized_tools: None,
    };
    // Tiny enabled surface: only hot tools → Compact → all on wire, no deferred gate semantics
    let enabled = vec![
        "read_file".into(),
        "write_file".into(),
        "patch".into(),
        "search_files".into(),
        "terminal".into(),
        "tool_search".into(),
    ];
    let schemas = registry.get_definitions(Some(&enabled), None, &ctx);
    assert!(schemas.len() <= AUTO_INDEXED_TOOL_COUNT_THRESHOLD);
    let effective = resolve_effective_schema_mode(ToolSchemaMode::Auto, schemas.len());
    assert_eq!(effective, ToolSchemaMode::Compact);
    let defs = build_wire_llm_definitions(
        &registry,
        &ctx,
        Some(&enabled),
        None,
        ToolSchemaMode::Auto,
        &std::collections::HashSet::new(),
        false,
    );
    assert_eq!(defs.len(), schemas.len());
}

#[test]
fn auto_large_surface_uses_indexed() {
    let registry = ToolRegistry::new();
    let ctx = ToolContext {
        task_id: "t".into(),
        cwd: std::env::temp_dir(),
        session_id: "s".into(),
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
    let schemas = registry.get_definitions(None, None, &ctx);
    assert!(
        schemas.len() > AUTO_INDEXED_TOOL_COUNT_THRESHOLD,
        "full registry must exceed auto threshold"
    );
    assert_eq!(
        resolve_effective_schema_mode(ToolSchemaMode::Auto, schemas.len()),
        ToolSchemaMode::Indexed
    );
    let defs = build_wire_llm_definitions(
        &registry,
        &ctx,
        None,
        None,
        ToolSchemaMode::Auto,
        &std::collections::HashSet::new(),
        false,
    );
    assert!(
        defs.len() < schemas.len(),
        "indexed wire must be smaller than full enabled surface"
    );
}

#[test]
fn local_hot_schemas_keep_property_descriptions() {
    let schemas = vec![ToolSchema {
        name: "read_file".into(),
        description: "Read a file.".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "absolute path" }
            },
            "required": ["path"]
        }),
        strict: None,
    }];
    let wire = wire_schemas(&schemas, &std::collections::HashSet::new(), true);
    assert!(
        wire[0].parameters["properties"]["path"]
            .get("description")
            .is_some()
    );
    let compact = wire_schemas(&schemas, &std::collections::HashSet::new(), false);
    assert!(
        compact[0].parameters["properties"]["path"]
            .get("description")
            .is_none()
    );
}

#[tokio::test]
async fn tool_search_returns_input_examples_for_write_file() {
    let registry = Arc::new(ToolRegistry::new());
    let mat = Arc::new(RwLock::new(MaterializedToolSet::new()));
    let ctx = schema_ctx(registry.clone(), mat);
    let out = registry
        .dispatch(
            "tool_search",
            json!({ "tool_names": ["write_file"] }),
            &ctx,
        )
        .await
        .expect("dispatch");
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("json");
    // write_file is hot — tool_search reports already_on_wire; examples on deferred tools.
    let already = parsed["already_on_wire"].as_array();
    let activated = parsed["activated"].as_array();
    assert!(
        already.is_some_and(|a| a.iter().any(|v| v == "write_file"))
            || activated.is_some_and(|a| a.iter().any(|v| v == "write_file")),
        "write_file must appear in already_on_wire or activated: {parsed}"
    );
    // Prefer deferred multi-action tool for input_examples proof when hot.
    let out2 = registry
        .dispatch(
            "tool_search",
            json!({ "tool_names": ["skill_manage"] }),
            &ctx,
        )
        .await
        .expect("dispatch skill_manage");
    let parsed2: serde_json::Value = serde_json::from_str(&out2).expect("json");
    let examples = parsed2["input_examples"]["skill_manage"]
        .as_array()
        .expect("input_examples.skill_manage");
    assert!(!examples.is_empty());
    assert!(examples.iter().any(|e| e.get("action").is_some()));
}

#[test]
fn prefetch_materializes_before_first_llm_call() {
    let registry = ToolRegistry::new();
    let ctx = ToolContext {
        task_id: "t".into(),
        cwd: std::env::temp_dir(),
        session_id: "s".into(),
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
    let schemas = registry.get_definitions(None, None, &ctx);
    let mat = Arc::new(RwLock::new(MaterializedToolSet::new()));
    let empty = read_materialized_set(Some(&mat));
    let hits = prefetch_tools_for_user_message(
        "please navigate the headless browser to a url",
        &schemas,
        &empty,
        3,
    );
    assert!(
        hits.iter().any(|n| n.contains("browser")),
        "prefetch should hit browser tools: {hits:?}"
    );
    let outcome = materialize_tool_names(
        &hits,
        &schemas,
        &mat,
        12,
        MaterializeSchemaStyle::Compact,
    );
    assert!(!outcome.activated.is_empty());
    let wire = build_wire_llm_definitions(
        &registry,
        &ctx,
        None,
        None,
        ToolSchemaMode::Indexed,
        &mat.read().unwrap().names(),
        false,
    );
    let wire_names: Vec<_> = wire.iter().map(|d| d.function.name.as_str()).collect();
    assert!(
        hits.iter().any(|h| wire_names.contains(&h.as_str())),
        "prefetched tools must appear on wire; hits={hits:?} wire={wire_names:?}"
    );
}

#[test]
fn game001_indexed_wire_includes_write_file_not_skill_manage() {
    let registry = ToolRegistry::new();
    let ctx = ToolContext {
        task_id: "t".into(),
        cwd: std::env::temp_dir(),
        session_id: "s".into(),
        user_task: None,
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
        materialized_tools: None,
    };
    let wire = build_wire_llm_definitions(
        &registry,
        &ctx,
        None,
        None,
        ToolSchemaMode::Indexed,
        &std::collections::HashSet::new(),
        false,
    );
    let names: Vec<_> = wire.iter().map(|d| d.function.name.as_str()).collect();
    assert!(
        names.contains(&"write_file"),
        "create-path requires write_file on hot wire: {names:?}"
    );
    assert!(
        !names.contains(&"skill_manage"),
        "skill_manage must stay deferred at turn 1: {names:?}"
    );
    assert!(
        !names.contains(&"web_search"),
        "web_search is deferred after create-path swap: {names:?}"
    );
}

#[test]
fn game001_create_intent_prefetch_skips_skill_manage() {
    let registry = ToolRegistry::new();
    let ctx = ToolContext {
        task_id: "t".into(),
        cwd: std::env::temp_dir(),
        session_id: "s".into(),
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
    let schemas = registry.get_definitions(None, None, &ctx);
    let hits = prefetch_tools_for_user_message(
        "Write a complete html5 and javascript 3D game in ./demo/game001",
        &schemas,
        &std::collections::HashSet::new(),
        3,
    );
    assert!(
        !hits.iter().any(|n| n == "skill_manage"),
        "create intent must not prefetch skill_manage: {hits:?}"
    );
}

#[tokio::test]
async fn game001_create_path_write_file_succeeds_without_skill_manage() {
    use tempfile::TempDir;

    let workspace = TempDir::new().expect("workspace");
    let provider = MockAgentProvider::new();
    provider.add_tool_response_sync(
        "",
        vec![ToolCall {
            id: "w1".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "write_file".into(),
                arguments: r#"{"path":"demo/game001/index.html","content":"<html></html>\n","create_dirs":true}"#.into(),
            },
            thought_signature: None,
        }],
    );
    provider.add_response_sync("Created the game scaffold.");

    let provider: Arc<dyn LLMProvider> = Arc::new(provider);
    let registry = Arc::new(ToolRegistry::new());
    let agent = AgentBuilder::new("mock-agent")
        .provider(provider)
        .tools(registry)
        .tool_schema_mode(ToolSchemaMode::Indexed)
        .skip_context_files(true)
        .skip_memory(true)
        .max_iterations(8)
        .streaming(false)
        .build()
        .expect("build");

    let result = agent
        .run_conversation_in_cwd(
            "Write a complete html5 and javascript 3D game in ./demo/game001",
            None,
            None,
            workspace.path(),
        )
        .await
        .expect("run");

    let tool_names: Vec<_> = result
        .messages
        .iter()
        .filter(|m| m.role == edgecrab_types::Role::Tool)
        .filter_map(|m| m.name.clone())
        .collect();
    assert!(
        tool_names.iter().any(|n| n == "write_file"),
        "expected write_file dispatch: {tool_names:?}"
    );
    assert!(
        !tool_names.iter().any(|n| n == "skill_manage"),
        "create path must not require skill_manage: {tool_names:?}"
    );
    let written = workspace.path().join("demo/game001/index.html");
    assert!(
        written.is_file(),
        "write_file must land under workspace cwd"
    );
}
