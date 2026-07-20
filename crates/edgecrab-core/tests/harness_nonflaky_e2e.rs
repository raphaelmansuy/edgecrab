//! Non-flaky harness e2e (019 Wave D) — no network, no sleep, no LLM.
//!
//! Golden-shaped sequences for pinguin thrash + happy path + media render.

use edgecrab_core::completion_assessor::{
    CompletionContext, assess_completion, visual_perception_evidence_ok,
};
use edgecrab_core::config::HarnessConfig;
use edgecrab_core::evidence_latch::{EvidenceAssessSnapshot, EvidenceLatchConfig, EvidenceState};
use edgecrab_core::task_class::{
    TaskClass, TaskClassifier, classify_from_messages, media_artifact_evidence_present,
};
use edgecrab_core::turn_dispatch::{TurnDispatchTrackers, TurnDispatchTrackersView};
use edgecrab_core::turn_dispatch_policy::pre_dispatch_decision;
use edgecrab_tools::{
    ContentClass, HarnessAdvisorySignals, HarnessBuildInput, MutationTurnState,
    StructuredBrowserResult, build_harness_snapshot, is_browser_esm_artifact,
    oracle_command_for_path,
};
use edgecrab_types::Message;
use std::fs;
use tempfile::TempDir;

#[test]
fn nf_e1_pinguin_shaped_thrash_trips_breaker_and_skips_esm_oracle() {
    let dir = TempDir::new().expect("temp");
    fs::write(
        dir.path().join("index.html"),
        r#"<!DOCTYPE html><script type="importmap">{"imports":{}}</script>
<script type="module" src="game.js"></script>"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("game.js"),
        "import * as THREE from 'three';\nconst x = 1;\n",
    )
    .unwrap();

    assert!(is_browser_esm_artifact(dir.path(), "game.js"));
    assert!(oracle_command_for_path(dir.path(), "game.js").is_none());

    let turn = MutationTurnState::new();
    turn.push_success(edgecrab_tools::MutationRecord {
        path: "game.js".into(),
        kind: edgecrab_tools::MutationKind::Add,
        lines_added: 2,
        lines_removed: 0,
    });
    let snap = build_harness_snapshot(HarnessBuildInput {
        messages: &[],
        mutation_turn: &turn,
        cwd: dir.path(),
        post_mutation_oracles: true,
        advisory: HarnessAdvisorySignals::default(),
        unanswered_tool_calls: 0,
    });
    assert!(
        snap.oracle_failures.is_empty(),
        "ESM must not fail node --check: {:?}",
        snap.oracle_failures
    );

    let mut evidence = EvidenceState::new(EvidenceLatchConfig {
        enabled: true,
        verify_tool_budget: 12,
        thrash_fingerprint_limit: 3,
        heal_budget: 1,
        post_perceive_browser_budget: 0,
    });
    evidence.note_artifact_path("game.js");
    // Candidate only (reuse path) — content fail must enter Heal, not stick latch.
    evidence.note_preview_candidate(
        dir.path().to_path_buf(),
        8000,
        "http://127.0.0.1:8000/".into(),
        None,
        Some(200),
    );

    let fail = StructuredBrowserResult::navigate_ok("http://127.0.0.1:8000/", "Error response");
    evidence.note_perceive(
        "browser_navigate",
        "http://127.0.0.1:8000/",
        fail.content_class,
    );
    assert_eq!(
        evidence.phase,
        edgecrab_core::evidence_latch::EvidencePhase::Heal,
        "first http_error_page must enter Heal"
    );
    assert!(
        !evidence.blocks_preview_serve(8000, dir.path()),
        "Heal must allow re-serve"
    );

    // Consume heal, then thrash to Escalated.
    evidence.note_preview_candidate(
        dir.path().to_path_buf(),
        8000,
        "http://127.0.0.1:8000/".into(),
        None,
        Some(200),
    );
    for _ in 0..3 {
        evidence.note_thrash_fingerprint(
            "browser_navigate",
            ContentClass::HttpErrorPage,
            "http://127.0.0.1:8000/",
        );
    }
    assert!(evidence.is_escalated());
    assert!(evidence.blocks_browser_tools());

    let harness = HarnessConfig::default();
    let mut trackers = TurnDispatchTrackers::with_harness(3, &harness);
    trackers.evidence = evidence;
    let view = TurnDispatchTrackersView {
        harness_advisory: &trackers.harness_advisory,
        tool_guardrail: &trackers.tool_guardrail,
        evidence: &trackers.evidence,
    };
    let messages = vec![Message::user(
        "create 3D pinguin game in demos/pinguin html threejs",
    )];
    let blocked = pre_dispatch_decision(
        &view,
        &messages,
        "browser_navigate",
        r#"{"url":"http://127.0.0.1:8000/"}"#,
        "",
    );
    assert!(
        blocked.as_deref().is_some_and(|b| {
            b.contains("halt")
                || b.contains("ESCALATED")
                || b.contains("Blocked")
                || b.contains("thrash")
        }),
        "got {blocked:?}"
    );
}

#[test]
fn nf_e2_happy_path_snapshot_is_visual_evidence() {
    let ok = StructuredBrowserResult::snapshot_ok(
        "http://127.0.0.1:8000/",
        "canvas game hud fish",
        Some(8),
    );
    assert_eq!(ok.content_class, ContentClass::Ok);
    assert!(ok.ok);
    assert!(visual_perception_evidence_ok(
        "browser_snapshot",
        &ok.to_tool_result_text()
    ));

    let messages = vec![
        Message::user("make demos/pinguin/index.html threejs game"),
        Message::tool_result(
            "w1",
            "write_file",
            r#"{"ok":true,"path":"demos/pinguin/index.html","bytes":100}"#,
        ),
        Message::tool_result("s1", "browser_snapshot", &ok.to_tool_result_text()),
    ];
    assert_eq!(classify_from_messages(&messages), TaskClass::VisualUx);

    let harness = build_harness_snapshot(HarnessBuildInput {
        messages: &messages,
        mutation_turn: &MutationTurnState::new(),
        cwd: std::path::Path::new("."),
        post_mutation_oracles: false,
        advisory: HarnessAdvisorySignals::default(),
        unanswered_tool_calls: 0,
    });
    let outcome = assess_completion(&CompletionContext {
        final_response: "done",
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
        evidence: Default::default(),
    });
    assert!(
        outcome.verification.evidence_present,
        "snapshot should count as evidence: {:?}",
        outcome.verification
    );
}

#[test]
fn nf_e3_media_render_class_and_file_evidence() {
    let msg = "create a video (use Hyperframe), 15 seconds in demos/raphael_video";
    assert_eq!(TaskClassifier::classify(msg, &[]), TaskClass::MediaRender);
    let messages = vec![
        Message::user(msg),
        Message::tool_result(
            "w1",
            "write_file",
            r#"{"ok":true,"path":"demos/raphael_video/out.mp4","bytes":4096}"#,
        ),
    ];
    assert!(media_artifact_evidence_present(&messages));
    assert_eq!(classify_from_messages(&messages), TaskClass::MediaRender);

    let harness = build_harness_snapshot(HarnessBuildInput {
        messages: &messages,
        mutation_turn: &MutationTurnState::new(),
        cwd: std::path::Path::new("."),
        post_mutation_oracles: false,
        advisory: HarnessAdvisorySignals::default(),
        unanswered_tool_calls: 0,
    });
    let outcome = assess_completion(&CompletionContext {
        final_response: "rendered",
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
        verification_strict: false,
        evidence: Default::default(),
    });
    assert!(
        outcome.verification.evidence_present,
        "media file should count as evidence: {:?}",
        outcome.verification
    );
}

#[test]
fn nf_e4_error_response_never_visual_evidence() {
    let bad = StructuredBrowserResult::navigate_ok("http://127.0.0.1:8000/", "Error response");
    assert_eq!(bad.content_class, ContentClass::HttpErrorPage);
    let content = bad.to_tool_result_json();
    assert!(!visual_perception_evidence_ok("browser_navigate", &content));
}

#[test]
fn nf_e5_document_latch_still_classifies() {
    let messages = vec![
        Message::user("create report.pptx for the board"),
        Message::tool_result(
            "w1",
            "write_file",
            r#"{"ok":true,"path":"out/report.pptx","bytes":5000}"#,
        ),
    ];
    assert_eq!(classify_from_messages(&messages), TaskClass::Document);
}

/// Golden: chess deadlock session 4f94111e — Error response enters Heal, re-serve allowed.
#[test]
fn nf_e6_chess_deadlock_heal_then_done() {
    use edgecrab_core::evidence_latch::EvidencePhase;
    use edgecrab_types::{CompletionDecision, ExitReason};

    let mut evidence = EvidenceState::new(EvidenceLatchConfig {
        enabled: true,
        verify_tool_budget: 12,
        thrash_fingerprint_limit: 3,
        heal_budget: 1,
        post_perceive_browser_budget: 0,
    });
    evidence.note_artifact_path("demos/chess/index.html");
    // Wrong process reuse candidate
    evidence.note_preview_candidate(
        std::path::PathBuf::from("demos/chess"),
        8000,
        "http://127.0.0.1:8000/".into(),
        None,
        Some(200),
    );
    assert!(!evidence.blocks_preview_serve(8000, std::path::Path::new("demos/chess")));

    let err = StructuredBrowserResult::navigate_ok("http://127.0.0.1:8000/", "Error response");
    evidence.note_perceive(
        "browser_navigate",
        "http://127.0.0.1:8000/",
        err.content_class,
    );
    assert_eq!(evidence.phase, EvidencePhase::Heal);
    assert!(!evidence.blocks_preview_serve(8000, std::path::Path::new("demos/chess")));

    // Heal re-serve allowed by policy
    let harness = HarnessConfig::default();
    let mut trackers = TurnDispatchTrackers::with_harness(3, &harness);
    trackers.evidence = evidence.clone();
    let view = TurnDispatchTrackersView {
        harness_advisory: &trackers.harness_advisory,
        tool_guardrail: &trackers.tool_guardrail,
        evidence: &trackers.evidence,
    };
    let messages = vec![Message::user(
        "Write a full 3D chess game using threeJs in ./demos/chess",
    )];
    let serve = pre_dispatch_decision(
        &view,
        &messages,
        "terminal",
        r#"{"command":"python3 -m http.server 8000 --directory demos/chess","background":true}"#,
        "",
    );
    assert!(
        serve.is_none(),
        "Heal must allow exact re-serve, got {serve:?}"
    );
    let ls = pre_dispatch_decision(
        &view,
        &messages,
        "terminal",
        r#"{"command":"ls -la demos/chess"}"#,
        "",
    );
    assert!(ls.is_some(), "Heal must block non-heal shell");

    // After heal serve + good snapshot → Done
    evidence.note_preview_candidate(
        std::path::PathBuf::from("demos/chess"),
        8000,
        "http://127.0.0.1:8000/".into(),
        None,
        Some(200),
    );
    let ok = StructuredBrowserResult::snapshot_ok(
        "http://127.0.0.1:8000/",
        "heading 3D Chess canvas Turn White",
        Some(6),
    );
    evidence.note_perceive(
        "browser_snapshot",
        "http://127.0.0.1:8000/",
        ok.content_class,
    );
    assert!(evidence.visual_evidence_complete());
    assert_eq!(evidence.phase, EvidencePhase::LatchedDone);

    trackers.evidence = evidence;
    let view = TurnDispatchTrackersView {
        harness_advisory: &trackers.harness_advisory,
        tool_guardrail: &trackers.tool_guardrail,
        evidence: &trackers.evidence,
    };
    let post = pre_dispatch_decision(&view, &messages, "browser_get_images", r#"{}"#, "");
    assert!(post.is_some(), "post-perceive browser thrash must block");

    let snap = trackers.evidence.assess_snapshot();
    let harness_snap = build_harness_snapshot(HarnessBuildInput {
        messages: &messages,
        mutation_turn: &MutationTurnState::new(),
        cwd: std::path::Path::new("."),
        post_mutation_oracles: false,
        advisory: HarnessAdvisorySignals::default(),
        unanswered_tool_calls: 0,
    });
    let outcome = assess_completion(&CompletionContext {
        final_response: "Chess is ready at demos/chess",
        messages: &messages,
        interrupted: false,
        budget_exhausted: false,
        invalid_tool_budget_exhausted: false,
        pending_approval: false,
        pending_clarification: false,
        active_todos: 1, // sticky todos must not block when latched
        blocked_todos: 0,
        child_runs_in_flight: 0,
        harness: harness_snap,
        verification_strict: true,
        evidence: snap,
    });
    assert_eq!(outcome.state, CompletionDecision::Completed);
    assert!(
        !edgecrab_core::turn_epilogue::should_reopen_loop_with_evidence(&outcome, &messages, snap,)
    );
    let _ = ExitReason::ModelReturnedFinalText;
}

/// Golden: escalated closed set never reopens with "do not stop yet".
#[test]
fn nf_e7_escalated_no_reopen() {
    use edgecrab_types::CompletionDecision;

    let mut evidence = EvidenceState::new(EvidenceLatchConfig {
        enabled: true,
        verify_tool_budget: 4,
        thrash_fingerprint_limit: 2,
        heal_budget: 0,
        post_perceive_browser_budget: 0,
    });
    evidence.note_artifact_path("demos/chess/index.html");
    evidence.escalate("test");
    let snap = evidence.assess_snapshot();
    assert!(snap.escalated);

    let messages = vec![Message::user("chess in demos/chess")];
    let harness_snap = build_harness_snapshot(HarnessBuildInput {
        messages: &messages,
        mutation_turn: &MutationTurnState::new(),
        cwd: std::path::Path::new("."),
        post_mutation_oracles: false,
        advisory: HarnessAdvisorySignals::default(),
        unanswered_tool_calls: 0,
    });
    let outcome = assess_completion(&CompletionContext {
        final_response: "Could not verify browser; files are in demos/chess",
        messages: &messages,
        interrupted: false,
        budget_exhausted: false,
        invalid_tool_budget_exhausted: false,
        pending_approval: false,
        pending_clarification: false,
        active_todos: 0,
        blocked_todos: 0,
        child_runs_in_flight: 0,
        harness: harness_snap,
        verification_strict: true,
        evidence: snap,
    });
    assert_eq!(outcome.state, CompletionDecision::Failed);
    assert!(
        !edgecrab_core::turn_epilogue::should_reopen_loop_with_evidence(&outcome, &messages, snap,)
    );
}

/// Golden 025 / session 50d96e9d: pre-built demo + Ok snapshot → no reopen.
#[test]
fn nf_e8_prebuilt_demo_snapshot_stops_reopen() {
    use edgecrab_types::CompletionDecision;

    let dir = TempDir::new().expect("temp");
    fs::write(
        dir.path().join("index.html"),
        "<!DOCTYPE html><title>SimCity</title>",
    )
    .unwrap();
    fs::create_dir(dir.path().join("js")).unwrap();
    fs::write(dir.path().join("js/game.js"), "export const ok = true;\n").unwrap();

    let mut trackers = TurnDispatchTrackers::with_harness(3, &HarnessConfig::default());
    let index_path = dir.path().join("index.html");
    let index_s = index_path.to_string_lossy();
    // Resume path: read existing file (no write this turn).
    trackers.record_tool_outcome(
        "read_file",
        &format!(r#"{{"path":"{index_s}"}}"#),
        &format!(r#"{{"ok":true,"path":"{index_s}"}}"#),
        false,
    );
    assert!(
        trackers.evidence.artifact,
        "read of existing demo must seed Artifact"
    );

    trackers.evidence.note_preview_candidate(
        dir.path().to_path_buf(),
        8000,
        "http://127.0.0.1:8000/".into(),
        None,
        Some(200),
    );
    let snap_json = StructuredBrowserResult::snapshot_ok(
        "http://127.0.0.1:8000/",
        "SimCity Builder HUD buttons",
        Some(12),
    )
    .to_tool_result_json();
    trackers.record_tool_outcome("browser_snapshot", r#"{}"#, &snap_json, false);

    let snap = trackers.evidence.assess_snapshot();
    assert!(
        snap.visual_complete,
        "prebuilt + snapshot Ok ⇒ visual_complete"
    );

    let messages = vec![
        Message::user("Create a sim city game in ./demos/simcity"),
        Message::assistant("Already built and verified."),
    ];
    let harness_snap = build_harness_snapshot(HarnessBuildInput {
        messages: &messages,
        mutation_turn: &MutationTurnState::new(),
        cwd: dir.path(),
        post_mutation_oracles: false,
        advisory: HarnessAdvisorySignals::default(),
        unanswered_tool_calls: 0,
    });
    let outcome = assess_completion(&CompletionContext {
        final_response: "SimCity already built & verified",
        messages: &messages,
        interrupted: false,
        budget_exhausted: false,
        invalid_tool_budget_exhausted: false,
        pending_approval: false,
        pending_clarification: false,
        active_todos: 0,
        blocked_todos: 0,
        child_runs_in_flight: 0,
        harness: harness_snap,
        verification_strict: true,
        evidence: snap,
    });
    assert_eq!(outcome.state, CompletionDecision::Completed);
    let gate = edgecrab_core::completion_reopen::CompletionReopenGate::new(2);
    assert_eq!(
        edgecrab_core::completion_reopen::decide_completion_reopen(
            &outcome, &messages, snap, &gate
        ),
        edgecrab_core::completion_reopen::ReopenDecision::DoNotReopen
    );
}

/// Golden 025: reopen cap ends thrash instead of infinite do-not-stop-yet.
#[test]
fn nf_e9_reopen_cap_ends_turn() {
    use edgecrab_types::{CompletionDecision, ExitReason, VerificationSummary};

    let mut outcome = edgecrab_types::RunOutcome::new(
        CompletionDecision::NeedsVerification,
        ExitReason::VerificationPending,
        "needs browser",
    );
    outcome.verification = VerificationSummary {
        required: true,
        evidence_present: false,
        debt_reason: Some(edgecrab_core::completion_assessor::visual_ux_debt_reason(
            &[],
        )),
        evidence: vec![],
        contract_required: false,
        contract_satisfied: false,
    };
    let nav_json = StructuredBrowserResult::navigate_ok("http://127.0.0.1:8000/", "SimCity")
        .to_tool_result_json();
    let nav_msg = Message::tool_result("n1", "browser_navigate", &nav_json);
    let debt = edgecrab_core::completion_assessor::visual_ux_debt_reason(&[nav_msg]);
    assert!(
        debt.contains("browser_snapshot") || debt.contains("browser_vision"),
        "debt after nav Ok must ask for snapshot, got: {debt}"
    );
    assert!(
        !debt.starts_with("Visual/UX task: enable security.preview and verify"),
        "stale enable-preview debt forbidden after nav Ok"
    );

    let gate = edgecrab_core::completion_reopen::CompletionReopenGate {
        max_reopens: 2,
        reopens_used: 2,
    };
    assert_eq!(
        edgecrab_core::completion_reopen::decide_completion_reopen(
            &outcome,
            &[],
            EvidenceAssessSnapshot::default(),
            &gate
        ),
        edgecrab_core::completion_reopen::ReopenDecision::CapReached
    );
    let capped = edgecrab_core::completion_reopen::reopen_cap_outcome("done-ish", &outcome);
    assert_eq!(capped.exit_reason, ExitReason::GuardrailHalt);
    assert!(!edgecrab_core::turn_epilogue::should_reopen_loop(&capped));
}
