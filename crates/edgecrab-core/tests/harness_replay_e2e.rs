//! Wave 2 deterministic harness replay fixtures (NF-E10..NF-E12).
//! No provider, network, sleep, or wall-clock behavior is involved.

use std::fs;
use std::path::Path;

use edgecrab_core::completion_assessor::{CompletionContext, assess_completion};
use edgecrab_core::completion_reopen::{
    CompletionReopenGate, ReopenDecision, decide_completion_reopen,
};
use edgecrab_core::evidence_latch::{EvidenceLatchConfig, EvidencePhase, EvidenceState};
use edgecrab_tools::{ContentClass, HarnessSnapshot};
use edgecrab_types::{CompletionDecision, ExitReason, Message, RunOutcome};
use serde::Deserialize;
use tempfile::TempDir;

#[derive(Debug, Deserialize)]
struct ReplayFixture {
    name: String,
    config: ReplayConfig,
    #[serde(default)]
    prebuilt_files: Vec<String>,
    actions: Vec<ReplayAction>,
    expected_phases: Vec<String>,
    expected_decision: String,
    expected_exit: String,
    max_verify_tools: u32,
}

#[derive(Debug, Deserialize)]
struct ReplayConfig {
    verify_tool_budget: u32,
    thrash_fingerprint_limit: u32,
    heal_budget: u32,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ReplayAction {
    SeedPrebuilt,
    Artifact {
        path: String,
    },
    Preview {
        port: u16,
    },
    Perceive {
        tool: String,
        class: ContentClass,
    },
    Fingerprint {
        tool: String,
        class: ContentClass,
        shape: String,
    },
}

struct ReplayResult {
    evidence: EvidenceState,
    phases: Vec<EvidencePhase>,
    outcome: RunOutcome,
    reopen: ReopenDecision,
}

fn load_fixture(file: &str) -> ReplayFixture {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/harness_replay")
        .join(file);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to load {}: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("invalid fixture {}: {error}", path.display()))
}

fn replay(file: &str) -> (ReplayFixture, ReplayResult) {
    let fixture = load_fixture(file);
    let dir = TempDir::new().expect("temp replay root");
    for relative in &fixture.prebuilt_files {
        let path = dir.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture parent");
        }
        fs::write(path, "<sanitized fixture>").expect("write prebuilt fixture");
    }

    let mut evidence = EvidenceState::new(EvidenceLatchConfig {
        enabled: true,
        verify_tool_budget: fixture.config.verify_tool_budget,
        thrash_fingerprint_limit: fixture.config.thrash_fingerprint_limit,
        heal_budget: fixture.config.heal_budget,
        post_perceive_browser_budget: 0,
    });
    let url = "http://127.0.0.1:8000/";
    let mut phases = Vec::with_capacity(fixture.actions.len());

    for action in &fixture.actions {
        match action {
            ReplayAction::SeedPrebuilt => evidence.seed_artifact_from_demo_dir(dir.path()),
            ReplayAction::Artifact { path } => evidence.note_artifact_path(path),
            ReplayAction::Preview { port } => {
                evidence.count_tool("terminal", false);
                evidence.note_preview_candidate(
                    dir.path().to_path_buf(),
                    *port,
                    format!("http://127.0.0.1:{port}/"),
                    None,
                    Some(200),
                );
            }
            ReplayAction::Perceive { tool, class } => {
                evidence.count_tool(tool, false);
                evidence.note_perceive(tool, url, *class);
            }
            ReplayAction::Fingerprint { tool, class, shape } => {
                evidence.count_tool(tool, false);
                evidence.note_thrash_fingerprint(tool, *class, shape);
            }
        }
        phases.push(evidence.phase);
    }

    let messages = vec![Message::user("create a deterministic visual demo")];
    let snapshot = evidence.assess_snapshot();
    let outcome = assess_completion(&CompletionContext {
        final_response: "Replay complete.",
        messages: &messages,
        interrupted: false,
        budget_exhausted: false,
        invalid_tool_budget_exhausted: false,
        pending_approval: false,
        pending_clarification: false,
        active_todos: 0,
        blocked_todos: 0,
        child_runs_in_flight: 0,
        harness: HarnessSnapshot::default(),
        verification_strict: true,
        evidence: snapshot,
    });
    let reopen =
        decide_completion_reopen(&outcome, &messages, snapshot, &CompletionReopenGate::new(2));

    (
        fixture,
        ReplayResult {
            evidence,
            phases,
            outcome,
            reopen,
        },
    )
}

fn expected_phase(name: &str) -> EvidencePhase {
    match name {
        "create" => EvidencePhase::Create,
        "verify" => EvidencePhase::Verify,
        "heal" => EvidencePhase::Heal,
        "latched_done" => EvidencePhase::LatchedDone,
        "escalated" => EvidencePhase::Escalated,
        other => panic!("unknown expected phase {other}"),
    }
}

fn assert_fixture(fixture: &ReplayFixture, result: &ReplayResult) {
    let expected: Vec<_> = fixture
        .expected_phases
        .iter()
        .map(|phase| expected_phase(phase))
        .collect();
    assert_eq!(result.phases, expected, "{} phase trace", fixture.name);
    assert!(
        result.evidence.verify_tools <= fixture.max_verify_tools,
        "{} used {} verify tools (cap {})",
        fixture.name,
        result.evidence.verify_tools,
        fixture.max_verify_tools
    );
    assert!(
        result.evidence.verify_tools <= fixture.config.verify_tool_budget,
        "{} exceeded configured verify budget",
        fixture.name
    );

    let decision = match fixture.expected_decision.as_str() {
        "completed" => CompletionDecision::Completed,
        "failed" => CompletionDecision::Failed,
        other => panic!("unknown expected decision {other}"),
    };
    let exit = match fixture.expected_exit.as_str() {
        "model_returned_final_text" => ExitReason::ModelReturnedFinalText,
        "guardrail_halt" => ExitReason::GuardrailHalt,
        other => panic!("unknown expected exit {other}"),
    };
    assert_eq!(result.outcome.state, decision, "{} decision", fixture.name);
    assert_eq!(result.outcome.exit_reason, exit, "{} exit", fixture.name);
    assert_eq!(
        result.reopen,
        ReopenDecision::DoNotReopen,
        "{} must not reopen",
        fixture.name
    );
}

/// NF-E10: an artifact already on disk latches DONE and closes browser actions.
#[test]
fn nf_e10_prebuilt_latch_done() {
    let (fixture, result) = replay("prebuilt_latch_done.json");
    assert_fixture(&fixture, &result);
    assert!(result.evidence.visual_evidence_complete());
    assert!(result.evidence.blocks_browser_tools());
    assert_eq!(result.evidence.post_perceive_tools, 0);
}

/// NF-E11: one exact normalized failure fingerprint reaches terminal escalation.
#[test]
fn nf_e11_exact_fingerprint_escalate() {
    let (fixture, result) = replay("exact_fingerprint_escalate.json");
    assert_fixture(&fixture, &result);
    assert!(result.evidence.is_escalated());
    assert!(result.evidence.blocks_browser_tools());
}

/// NF-E12: one content heal is consumed, then good evidence latches DONE.
#[test]
fn nf_e12_content_heal_once() {
    let (fixture, result) = replay("content_heal_once.json");
    assert_fixture(&fixture, &result);
    assert_eq!(
        result
            .phases
            .iter()
            .filter(|phase| matches!(phase, EvidencePhase::Heal))
            .count(),
        1
    );
    assert_eq!(result.evidence.heal_remaining, 0);
    assert!(result.evidence.visual_evidence_complete());
}
