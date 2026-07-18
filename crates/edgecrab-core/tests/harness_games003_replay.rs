//! games003 harness regression pack (spec 015 HA-27).
//!
//! Deterministic replay of battle-test failure signatures without a live provider.

use edgecrab_security::url_safety::{PreviewPolicy, set_preview_policy};
use edgecrab_tools::artifact_spill::{SpillContext, SpillOutcome, SpillSequence, maybe_spill};
use edgecrab_tools::mutation_turn_policy::check_tool_argument_budget;
use edgecrab_tools::{AppConfigRef, TodoStore, ToolContext, ToolRegistry};
use edgecrab_types::{Platform, ToolError};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

fn spill_config(threshold: usize) -> edgecrab_tools::artifact_spill::SpillConfig {
    edgecrab_tools::artifact_spill::SpillConfig {
        enabled: true,
        threshold,
        preview_lines: 80,
    }
}

#[test]
fn games003_spill_on_large_read_has_actionable_stub() {
    let tmp = TempDir::new().expect("tempdir");
    let seq = SpillSequence::new();
    let body = (1..=120)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let ctx = SpillContext {
        source_path: Some("demo/games003/index.html".into()),
    };

    match maybe_spill(
        "read_file",
        "tc-g3-1",
        body,
        "games003",
        tmp.path(),
        &spill_config(100),
        &seq,
        Some(&ctx),
    ) {
        SpillOutcome::Spilled { stub, .. } => {
            assert!(stub.contains("source_path: demo/games003/index.html"));
            assert!(stub.contains("next: read_file"));
            assert!(stub.contains("[tool_result_spill]"));
        }
        SpillOutcome::Inline(_) => panic!("expected spill for games003-sized read"),
    }
}

#[test]
fn games003_oversized_write_rejected_with_patch_recovery() {
    let cap = edgecrab_tools::mutation_turn_policy::local_default_max_tool_argument_bytes();
    let big = "x".repeat(cap + 500);
    let args = format!(r#"{{"path":"demo/games003/game.html","content":{big:?}}}"#);
    let violation = check_tool_argument_budget("write_file", &args, cap, None)
        .expect_err("oversized write should be rejected");
    assert_eq!(violation.tool_name, "write_file");
    assert!(violation.argument_bytes > violation.max_bytes);

    let err = edgecrab_tools::recovery_catalog::tool_argument_budget_exceeded(
        "write_file",
        violation.argument_bytes,
        violation.max_bytes,
        violation.estimated_tokens,
    );
    let recovery = err.to_llm_payload().recovery_feedback.expect("recovery");
    let blob = serde_json::to_string(&recovery.suggestions).expect("json");
    assert!(blob.contains("patch"));
}

#[test]
fn games003_preview_enabled_allows_localhost_navigate() {
    set_preview_policy(PreviewPolicy {
        enabled: true,
        allowed_ports: vec![8000, 8888],
        allow_any_loopback_port: false,
    });
    assert!(
        edgecrab_security::url_safety::is_safe_url("http://127.0.0.1:8000/index-ultra.html")
            .expect("parse")
    );

    let registry = ToolRegistry::new();
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let tmp = TempDir::new().expect("tempdir");
    let ctx = ToolContext {
        task_id: "g3".into(),
        cwd: tmp.path().to_path_buf(),
        session_id: "games003".into(),
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

    // SSRF pre-check passes; Chrome may be unavailable — that's ok for HA-05.
    let result = rt.block_on(async {
        registry
            .dispatch(
                "browser_navigate",
                serde_json::json!({"url": "http://127.0.0.1:8000/index-ultra.html"}),
                &ctx,
            )
            .await
    });
    match result {
        Ok(_) => {}
        Err(ToolError::PermissionDenied(msg))
            if msg.contains("SSRF") || msg.contains("blocked") =>
        {
            panic!("preview-enabled localhost should not be SSRF-blocked: {msg}");
        }
        Err(_) => {
            // Browser unavailable in CI — SSRF gate passed.
        }
    }
    set_preview_policy(PreviewPolicy::default());
}

#[test]
fn games003_todo_snapshot_survives_compress_injection() {
    let store = TodoStore::new();
    store.write(vec![
        edgecrab_tools::tools::todo::TodoItem {
            id: 1,
            title: "Start http.server on 8000".into(),
            status: "in-progress".into(),
        },
        edgecrab_tools::tools::todo::TodoItem {
            id: 2,
            title: "browser_navigate preview".into(),
            status: "not-started".into(),
        },
        edgecrab_tools::tools::todo::TodoItem {
            id: 3,
            title: "Done scaffold".into(),
            status: "completed".into(),
        },
    ]);
    let snapshot = store.format_for_injection().expect("active todos");
    assert!(snapshot.contains("in-progress"));
    assert!(snapshot.contains("browser_navigate"));
    assert!(!snapshot.contains("Done scaffold"));
}

#[test]
fn games003_profile_inherits_global_preview() {
    use edgecrab_core::{AppConfig, merge_global_inherited};

    let mut profile = AppConfig::default();
    let mut global = AppConfig::default();
    global.security.preview.enabled = true;
    global.security.preview.allow_localhost_ports = vec![8000];
    let yaml = "model:\n  default: test\n";
    merge_global_inherited(&mut profile, &global, yaml);
    assert!(profile.security.preview.enabled);
    assert!(
        profile
            .security
            .preview
            .allow_localhost_ports
            .contains(&8000)
    );
}

#[test]
fn e16_no_false_completed_on_visual_without_perception() {
    use edgecrab_core::completion_assessor::{CompletionContext, assess_completion};
    use edgecrab_tools::{
        HarnessAdvisorySignals, HarnessBuildInput, MutationTurnState, build_harness_snapshot,
    };

    let messages = vec![
        edgecrab_types::Message::user("make demo/games003 index-ultra beautiful UX"),
        edgecrab_types::Message::tool_result("t1", "terminal", "ok"),
        edgecrab_types::Message::tool_result("t2", "terminal", "ok"),
        edgecrab_types::Message::tool_result("t3", "write_file", r#"{"path":"index.html"}"#),
    ];
    let harness = build_harness_snapshot(HarnessBuildInput {
        messages: &messages,
        mutation_turn: &MutationTurnState::new(),
        cwd: std::path::Path::new("."),
        post_mutation_oracles: false,
        advisory: HarnessAdvisorySignals::default(),
        unanswered_tool_calls: 0,
    });
    let outcome = assess_completion(&CompletionContext {
        final_response: "The UI is complete and verified.",
        messages: &messages,
        interrupted: false,
        budget_exhausted: false,
        invalid_tool_budget_exhausted: false,
        pending_approval: false,
        pending_clarification: false,
        active_todos: 0,
        blocked_todos: 0,
        child_runs_in_flight: 0,
        harness,
        verification_strict: true,
    });
    assert_ne!(outcome.state, edgecrab_types::CompletionDecision::Completed);
}

#[test]
fn games003_browser_recovery_cites_config_not_read_file() {
    let err = edgecrab_tools::recovery_catalog::browser_navigate_blocked(
        "http://127.0.0.1:8000/",
        "SSRF policy",
        &[8000],
    );
    let blob = serde_json::to_string(&err.to_llm_payload()).expect("json");
    assert!(blob.contains("/config") || blob.contains("fix_via"));
    assert!(blob.contains("do_not") || blob.contains("read_file"));
    assert!(blob.contains("preview"));
}

/// Session `0aeef965` (race_gamey): terminal storm after browser block must not Complete.
#[test]
fn session_0aeef965_terminal_storm_blocks_completion_and_act() {
    use edgecrab_core::completion_assessor::{CompletionContext, assess_completion};
    use edgecrab_core::harness_loop_policy::visual_storm_block_result;
    use edgecrab_core::harness_advisory::HarnessTurnAdvisory;
    use edgecrab_tools::{
        HarnessAdvisorySignals, HarnessBuildInput, MutationTurnState, build_harness_snapshot,
    };

    let messages = vec![
        edgecrab_types::Message::user(
            "Create best 3D race car demo/race_gamey with beautiful UX preview",
        ),
        edgecrab_types::Message::tool_result("t1", "write_file", r#"{"path":"index.html"}"#),
        edgecrab_types::Message::tool_result("t2", "terminal", "Serving HTTP on port 8000"),
        edgecrab_types::Message::tool_result(
            "t3",
            "browser_navigate",
            "URL blocked for browser navigation",
        ),
        edgecrab_types::Message::tool_result("t4", "terminal", "ls"),
        edgecrab_types::Message::tool_result("t5", "terminal", "cat"),
        edgecrab_types::Message::tool_result("t6", "terminal", "grep"),
        edgecrab_types::Message::tool_result("t7", "terminal", "ls again"),
        edgecrab_types::Message::tool_result("t8", "terminal", "pwd"),
        edgecrab_types::Message::tool_result(
            "t9",
            "write_file",
            r#"{"path":"VERIFICATION.md"}"#,
        ),
    ];

    let mut advisory = HarnessTurnAdvisory::new();
    for name in [
        "write_file",
        "terminal",
        "browser_navigate",
        "terminal",
        "terminal",
        "terminal",
        "terminal",
        "terminal",
    ] {
        advisory.record_tool(name);
    }
    // Navigate was blocked (SSRF/preview) — must not count as perception evidence.
    advisory.record_browser_navigate_result("URL blocked for browser navigation");
    assert!(
        visual_storm_block_result(&advisory, &messages, "terminal").is_some(),
        "0aeef965-class storm must hard-block further terminal"
    );

    let harness = build_harness_snapshot(HarnessBuildInput {
        messages: &messages,
        mutation_turn: &MutationTurnState::new(),
        cwd: std::path::Path::new("."),
        post_mutation_oracles: false,
        advisory: HarnessAdvisorySignals {
            visual_act_storm: true,
            guardrail_halt: false,
        },
        unanswered_tool_calls: 0,
    });
    let outcome = assess_completion(&CompletionContext {
        final_response: "Done — see VERIFICATION.md",
        messages: &messages,
        interrupted: false,
        budget_exhausted: false,
        invalid_tool_budget_exhausted: false,
        pending_approval: false,
        pending_clarification: false,
        active_todos: 0,
        blocked_todos: 0,
        child_runs_in_flight: 0,
        harness,
        verification_strict: true,
    });
    assert_ne!(outcome.state, edgecrab_types::CompletionDecision::Completed);
}
