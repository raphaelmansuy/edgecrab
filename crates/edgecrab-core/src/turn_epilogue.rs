//! Turn epilogue — single assess path for mid-loop and end-loop (spec 017 P1-2 / HA-45).
//!
//! DRY: `conversation.rs` calls these functions; no duplicate assess logic.

use edgecrab_tools::{
    HarnessAdvisorySignals, HarnessBuildInput, HarnessSnapshot, MutationTurnState,
    build_harness_snapshot,
};
use edgecrab_types::{
    CompletionDecision, ExitReason, GoalContract, Message, RunOutcome,
};

use crate::completion_assessor::{
    CompletionContext, assess_completion, enrich_verification_with_contract,
};
use crate::config::HarnessConfig;
use crate::evidence_latch::EvidenceAssessSnapshot;
use crate::harness_advisory::HarnessTurnAdvisory;
use crate::task_class::{TaskClass, classify_from_messages, effective_verification_strict};
use crate::turn_completion::{TurnCompletionContext, count_unanswered_tool_calls};

/// Inputs for building a turn harness snapshot (HA-45).
pub struct TurnHarnessBuildParams<'a> {
    pub messages: &'a [Message],
    pub mutation_turn: &'a MutationTurnState,
    pub cwd: &'a std::path::Path,
    pub post_mutation_oracles: bool,
    pub harness_advisory: &'a HarnessTurnAdvisory,
    pub guardrail_halt: bool,
    pub task_class: TaskClass,
}

/// Build a full harness snapshot — never use [`HarnessSnapshot::default`] at assess sites.
pub fn build_turn_harness_snapshot(params: TurnHarnessBuildParams<'_>) -> HarnessSnapshot {
    let unanswered = count_unanswered_tool_calls(params.messages);
    let advisory = HarnessAdvisorySignals {
        visual_act_storm: params
            .harness_advisory
            .is_act_storm_without_perception(params.task_class),
        guardrail_halt: params.guardrail_halt,
    };
    build_harness_snapshot(HarnessBuildInput {
        messages: params.messages,
        mutation_turn: params.mutation_turn,
        cwd: params.cwd,
        post_mutation_oracles: params.post_mutation_oracles,
        advisory,
        unanswered_tool_calls: unanswered,
    })
}

/// Todo counts passed into completion assess.
#[derive(Debug, Clone, Copy, Default)]
pub struct TodoSnapshot {
    pub active: usize,
    pub blocked: usize,
}

/// Run context for completion assessment.
pub struct TurnAssessParams<'a> {
    pub final_response: &'a str,
    pub messages: &'a [Message],
    pub interrupted: bool,
    pub budget_exhausted: bool,
    pub invalid_tool_budget_exhausted: bool,
    pub pending_approval: bool,
    pub pending_clarification: bool,
    pub active_todos: usize,
    pub blocked_todos: usize,
    pub child_runs_in_flight: usize,
    pub harness: HarnessSnapshot,
    pub harness_config: &'a HarnessConfig,
    /// Active goal completion contract (empty = free-form assess).
    pub goal_contract: Option<&'a GoalContract>,
    /// Final assess only: run `GoalContract.verification` once if unmet by agent tools.
    pub harness_contract_verify: bool,
    /// Working directory for harness-run contract verification.
    pub cwd: &'a std::path::Path,
    /// Evidence latch snapshot (022).
    pub evidence: EvidenceAssessSnapshot,
}

/// Single completion assess entry (provisional + final).
pub fn assess_turn_outcome(params: TurnAssessParams<'_>) -> RunOutcome {
    // Blockable PreVerify (018 P6): non-zero exit forces NeedsVerification.
    // Still cache-safe — never mutates system prompt.
    let pre_ctx = serde_json::json!({
        "final_response_chars": params.final_response.chars().count(),
        "message_count": params.messages.len(),
        "budget_exhausted": params.budget_exhausted,
        "interrupted": params.interrupted,
        "contract_required": params
            .goal_contract
            .map(|c| c.requires_verification_evidence())
            .unwrap_or(false),
        "harness_contract_verify": params.harness_contract_verify,
    });
    let pre_gate = crate::lifecycle_hooks::run_pre_verify_blocking(pre_ctx.clone());
    // Keep fire-and-forget emit for observers that only listen.
    crate::lifecycle_hooks::emit_global(
        crate::lifecycle_hooks::LifecycleEvent::PreVerify,
        pre_ctx,
    );

    let verification_strict = effective_verification_strict(params.harness_config, params.messages);
    let mut outcome = assess_completion(&CompletionContext {
        final_response: params.final_response,
        messages: params.messages,
        interrupted: params.interrupted,
        budget_exhausted: params.budget_exhausted,
        invalid_tool_budget_exhausted: params.invalid_tool_budget_exhausted,
        pending_approval: params.pending_approval,
        pending_clarification: params.pending_clarification,
        active_todos: params.active_todos,
        blocked_todos: params.blocked_todos,
        child_runs_in_flight: params.child_runs_in_flight,
        harness: params.harness,
        verification_strict,
        evidence: params.evidence,
    });

    if pre_gate.denied && matches!(outcome.state, CompletionDecision::Completed) {
        outcome.state = CompletionDecision::NeedsVerification;
        outcome.exit_reason = ExitReason::VerificationPending;
        outcome.user_summary = format!(
            "Needs verification — PreVerify hook denied completion ({})",
            pre_gate.reasons.join("; ")
        );
        outcome.verification.required = true;
        outcome.verification.evidence_present = false;
        outcome.verification.debt_reason = Some(outcome.user_summary.clone());
    }

    // Wave B: coding verify-on-stop (pre-completion checklist).
    if params.harness_config.verify_on_stop
        && matches!(outcome.state, CompletionDecision::Completed)
        && coding_verify_on_stop_debt(params.messages)
    {
        outcome.state = CompletionDecision::NeedsVerification;
        outcome.exit_reason = ExitReason::VerificationPending;
        outcome.user_summary = "Needs verification — run the project's test/check command \
             (terminal exit_code=0) after code changes."
            .into();
        outcome.verification.required = true;
        outcome.verification.evidence_present = false;
        outcome.verification.debt_reason = Some(outcome.user_summary.clone());
    }

    if let Some(contract) = params.goal_contract {
        outcome.verification =
            enrich_verification_with_contract(outcome.verification, contract, params.messages);
        outcome.evidence = outcome.verification.evidence.clone();

        // Wave A: harness-run verification on final assess only.
        if params.harness_contract_verify
            && outcome.verification.contract_required
            && !outcome.verification.contract_satisfied
        {
            let cmd = contract.verification.trim();
            if !cmd.is_empty() {
                let result =
                    crate::contract_verify::run_contract_verification(cmd, params.cwd);
                if result.exit_code == 0 {
                    outcome.verification.contract_satisfied = true;
                    outcome.verification.evidence_present = true;
                    outcome.verification.debt_reason = None;
                    outcome.verification.evidence.push(format!(
                        "contract_verify: exit_code=0 cmd={cmd}"
                    ));
                    outcome.evidence = outcome.verification.evidence.clone();
                    // Harness proof can clear NeedsVerification when that was the only debt.
                    if matches!(
                        outcome.state,
                        CompletionDecision::NeedsVerification | CompletionDecision::Completed
                    ) && !coding_verify_on_stop_debt(params.messages)
                    {
                        outcome.state = CompletionDecision::Completed;
                        outcome.exit_reason = ExitReason::ModelReturnedFinalText;
                        outcome.user_summary =
                            "Completed — harness verified goal contract.".into();
                    }
                } else {
                    outcome.verification.evidence.push(format!(
                        "contract_verify: exit_code={} cmd={cmd}",
                        result.exit_code
                    ));
                    outcome.evidence = outcome.verification.evidence.clone();
                    outcome.verification.debt_reason = Some(format!(
                        "Goal contract verification failed (exit_code={}): {cmd}",
                        result.exit_code
                    ));
                }
            }
        }

        if outcome.verification.contract_required
            && !outcome.verification.contract_satisfied
            && matches!(outcome.state, CompletionDecision::Completed)
        {
            outcome.state = CompletionDecision::NeedsVerification;
            outcome.exit_reason = ExitReason::VerificationPending;
            outcome.user_summary = outcome.verification.debt_reason.clone().unwrap_or_else(|| {
                format!(
                    "Needs verification — goal contract requires tool evidence for: {}",
                    contract.verification.trim()
                )
            });
        }
    }

    outcome
}

/// True when mutations landed without a successful terminal/execute_code verify tool.
///
/// July 2026 / Anthropic long-running harness: verification must cover the
/// **latest** mutations. A test run that precedes later writes does not clear debt.
fn coding_verify_on_stop_debt(messages: &[Message]) -> bool {
    let mut last_mutation_idx: Option<usize> = None;
    let mut last_verify_idx: Option<usize> = None;
    for (i, msg) in messages.iter().enumerate() {
        if msg.role != edgecrab_types::Role::Tool {
            continue;
        }
        let Some(name) = msg.name.as_deref() else {
            continue;
        };
        let content = msg.text_content();
        if matches!(name, "write_file" | "patch" | "apply_patch")
            && edgecrab_tools::file_mutation_result_landed(name, &content)
        {
            last_mutation_idx = Some(i);
        }
        if matches!(name, "terminal" | "run_process") {
            if let Some(parsed) = edgecrab_tools::parse_terminal_result(&content)
                && parsed.exit_code == 0
            {
                last_verify_idx = Some(i);
            }
        } else if name == "execute_code"
            && edgecrab_types::parse_tool_error_payload(&content).is_none()
            && !content.trim().is_empty()
        {
            last_verify_idx = Some(i);
        }
    }
    match (last_mutation_idx, last_verify_idx) {
        (Some(m), Some(v)) => m > v, // mutation after last verify
        (Some(_), None) => true,
        _ => false,
    }
}

/// Re-open the loop when assess rejects premature model text.
pub fn should_reopen_loop(outcome: &RunOutcome) -> bool {
    should_reopen_loop_with_messages(outcome, &[])
}

/// Like [`should_reopen_loop`], but never reopens when Document artifact evidence
/// is already present (007 docx never-stop — no “do not stop yet” theater).
pub fn should_reopen_loop_with_messages(outcome: &RunOutcome, messages: &[Message]) -> bool {
    should_reopen_loop_with_evidence(outcome, messages, EvidenceAssessSnapshot::default())
}

/// 022: also respect visual latch done / escalated (closed action set).
pub fn should_reopen_loop_with_evidence(
    outcome: &RunOutcome,
    messages: &[Message],
    evidence: EvidenceAssessSnapshot,
) -> bool {
    // Invalid-tool budget abort is terminal — do not reopen into another invent-retry cycle.
    if outcome.exit_reason == ExitReason::InvalidToolBudget {
        return false;
    }
    if outcome.exit_reason == ExitReason::GuardrailHalt {
        return false;
    }
    // 022 B1/B2: latched complete or escalated — never “do not stop yet”.
    if evidence.visual_complete || evidence.media_complete || evidence.escalated {
        return false;
    }
    if evidence.verify_budget_exhausted {
        return false;
    }
    if !messages.is_empty() && crate::task_class::document_done_latch_ready(messages) {
        return false;
    }
    matches!(
        outcome.state,
        CompletionDecision::Incomplete
            | CompletionDecision::NeedsVerification
            | CompletionDecision::Failed
    )
}

/// User message injected when the model tried to stop early.
pub fn completion_follow_up_message(outcome: &RunOutcome) -> String {
    let mut notes = Vec::new();

    match outcome.state {
        CompletionDecision::Incomplete => {
            notes.push("There is still unfinished work or at least one remaining step.".into());
        }
        CompletionDecision::NeedsVerification => {
            notes.push(
                "Concrete verification evidence is still missing, so the task is not done yet."
                    .into(),
            );
        }
        CompletionDecision::Failed => {
            notes.push("The last response did not produce a usable completion.".into());
        }
        _ => {}
    }

    if outcome.active_tasks > 0 || outcome.blocked_tasks > 0 {
        notes.push(format!(
            "Task ledger snapshot: {} active, {} blocked.",
            outcome.active_tasks, outcome.blocked_tasks
        ));
    }

    if let Some(reason) = outcome.verification.debt_reason.as_deref() {
        notes.push(reason.to_string());
    }

    format!(
        "[system: do not stop yet. {} Continue working until the request is actually complete or explicitly blocked. Briefly communicate progress, use report_task_status after the next milestone, and only finish once you have concrete evidence.]",
        notes.join(" ")
    )
}

/// Apply operator explainer after final assess.
pub fn enrich_turn_outcome(
    mut outcome: RunOutcome,
    messages: &[Message],
    harness: &HarnessSnapshot,
    copilot_nonstreaming: bool,
) -> RunOutcome {
    let pending_tools = count_unanswered_tool_calls(messages);
    let task_class = classify_from_messages(messages);
    outcome.user_summary = crate::turn_completion::format_turn_completion_explanation(
        &outcome,
        &TurnCompletionContext {
            pending_tool_results: pending_tools,
            harness_block_reason: harness.completion_block_reason(),
            task_class,
            copilot_nonstreaming,
        },
    );
    outcome
}

/// Static fallback when the toolless summary API call fails (HA-11).
pub fn budget_exhausted_fallback_message(used: u32, max: u32) -> String {
    format!(
        "[Agent reached the iteration limit ({used}/{max}) before completing the task. \
         Please try rephrasing your request or increase the iteration budget.]"
    )
}

/// One toolless provider call to summarize partial progress (Hermes `_handle_max_iterations` parity).
pub async fn synthesize_budget_exhausted_message(
    provider: &std::sync::Arc<dyn edgequake_llm::LLMProvider>,
    mut chat_messages: Vec<edgequake_llm::ChatMessage>,
    used: u32,
    max: u32,
) -> String {
    let nudge = format!(
        "[System: iteration budget exhausted ({used}/{max}). \
         Summarize what was accomplished, what remains blocked, and suggested next steps \
         in 2-4 sentences. Do not request or call tools.]"
    );
    chat_messages.push(edgequake_llm::ChatMessage::user(&nudge));
    match provider.chat(&chat_messages, None).await {
        Ok(response) => {
            let text = crate::provider_call::assistant_display_text(&response);
            if text.trim().is_empty() {
                budget_exhausted_fallback_message(used, max)
            } else {
                text
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, "budget-exhausted summary call failed — using static fallback");
            budget_exhausted_fallback_message(used, max)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_types::{CompletionDecision, ExitReason, RunOutcome};

    #[test]
    fn invalid_tool_budget_does_not_reopen_loop() {
        let outcome = RunOutcome::new(
            CompletionDecision::Failed,
            ExitReason::InvalidToolBudget,
            "Model generated invalid tool call: quick_stock_quote",
        );
        assert!(!should_reopen_loop(&outcome));
    }

    #[test]
    fn ha45_assess_uses_built_snapshot_not_default() {
        let messages = vec![edgecrab_types::Message::assistant_with_tool_calls(
            "",
            vec![edgecrab_types::ToolCall {
                id: "orphan".into(),
                r#type: "function".into(),
                function: edgecrab_types::FunctionCall {
                    name: "terminal".into(),
                    arguments: "{}".into(),
                },
                thought_signature: None,
            }],
        )];
        let advisory = HarnessTurnAdvisory::new();
        let turn = MutationTurnState::new();
        let harness = build_turn_harness_snapshot(TurnHarnessBuildParams {
            messages: &messages,
            mutation_turn: &turn,
            cwd: std::path::Path::new("."),
            post_mutation_oracles: false,
            harness_advisory: &advisory,
            guardrail_halt: false,
            task_class: TaskClass::General,
        });
        assert_eq!(harness.unanswered_tool_calls, 1);
        assert!(harness.blocks_completion());
        let outcome = assess_turn_outcome(TurnAssessParams {
            final_response: "Done.",
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
            harness_config: &HarnessConfig::default(),
            goal_contract: None,
            harness_contract_verify: false,
            cwd: std::path::Path::new("."),
            evidence: EvidenceAssessSnapshot::default(),
        });
        assert_eq!(outcome.state, CompletionDecision::Incomplete);
    }

    fn assess_params<'a>(
        final_response: &'a str,
        messages: &'a [edgecrab_types::Message],
        harness: HarnessSnapshot,
        harness_config: &'a HarnessConfig,
        goal_contract: Option<&'a GoalContract>,
        harness_contract_verify: bool,
    ) -> TurnAssessParams<'a> {
        TurnAssessParams {
            final_response,
            messages,
            interrupted: false,
            budget_exhausted: false,
            invalid_tool_budget_exhausted: false,
            pending_approval: false,
            pending_clarification: false,
            active_todos: 0,
            blocked_todos: 0,
            child_runs_in_flight: 0,
            harness,
            harness_config,
            goal_contract,
            harness_contract_verify,
            cwd: std::path::Path::new("."),
            evidence: EvidenceAssessSnapshot::default(),
        }
    }

    #[test]
    fn p0_contract_blocks_completed_without_tool_evidence() {
        let messages = vec![edgecrab_types::Message::assistant("All done, looks good.")];
        let advisory = HarnessTurnAdvisory::new();
        let turn = MutationTurnState::new();
        let harness = build_turn_harness_snapshot(TurnHarnessBuildParams {
            messages: &messages,
            mutation_turn: &turn,
            cwd: std::path::Path::new("."),
            post_mutation_oracles: false,
            harness_advisory: &advisory,
            guardrail_halt: false,
            task_class: TaskClass::General,
        });
        let contract = GoalContract {
            verification: "cargo test -p edgecrab-core".into(),
            ..Default::default()
        };
        let cfg = HarnessConfig::default();
        let outcome = assess_turn_outcome(assess_params(
            "Completed — cargo tests passed.",
            &messages,
            harness,
            &cfg,
            Some(&contract),
            false, // provisional-style: no harness run
        ));
        assert_eq!(outcome.state, CompletionDecision::NeedsVerification);
        assert!(outcome.verification.contract_required);
        assert!(!outcome.verification.contract_satisfied);
    }

    #[test]
    fn p0_contract_satisfied_by_terminal_tool_result() {
        let messages = vec![
            edgecrab_types::Message::assistant("Running tests."),
            edgecrab_types::Message::tool_result(
                "t1",
                "terminal",
                "[terminal_result status=success backend=local cwd=/tmp exit_code=0]\n\
                 running 20 tests\ncargo test -p edgecrab-core\ntest result: ok. 20 passed",
            ),
            edgecrab_types::Message::assistant("Done."),
        ];
        let advisory = HarnessTurnAdvisory::new();
        let turn = MutationTurnState::new();
        let harness = build_turn_harness_snapshot(TurnHarnessBuildParams {
            messages: &messages,
            mutation_turn: &turn,
            cwd: std::path::Path::new("."),
            post_mutation_oracles: false,
            harness_advisory: &advisory,
            guardrail_halt: false,
            task_class: TaskClass::General,
        });
        let contract = GoalContract {
            verification: "cargo test -p edgecrab-core".into(),
            ..Default::default()
        };
        let cfg = HarnessConfig::default();
        let outcome = assess_turn_outcome(assess_params(
            "Completed.",
            &messages,
            harness,
            &cfg,
            Some(&contract),
            false,
        ));
        assert_eq!(outcome.state, CompletionDecision::Completed);
        assert!(outcome.verification.contract_satisfied);
    }

    #[test]
    fn wave_a_echo_gaming_rejected_even_with_exit_zero() {
        let messages = vec![edgecrab_types::Message::tool_result(
            "t1",
            "terminal",
            "[terminal_result status=success backend=local cwd=/tmp exit_code=0]\n\
             echo cargo test\ncargo test\n",
        )];
        let contract = GoalContract {
            verification: "cargo test".into(),
            ..Default::default()
        };
        assert!(!crate::goal_judge::contract_evidence_in_messages(
            &contract, &messages
        ));
    }

    #[test]
    fn wave_a_harness_run_satisfies_contract() {
        let messages = vec![edgecrab_types::Message::assistant("Done.")];
        let advisory = HarnessTurnAdvisory::new();
        let turn = MutationTurnState::new();
        let harness = build_turn_harness_snapshot(TurnHarnessBuildParams {
            messages: &messages,
            mutation_turn: &turn,
            cwd: std::path::Path::new("."),
            post_mutation_oracles: false,
            harness_advisory: &advisory,
            guardrail_halt: false,
            task_class: TaskClass::General,
        });
        let contract = GoalContract {
            verification: "true".into(),
            ..Default::default()
        };
        let cfg = HarnessConfig {
            verify_on_stop: false,
            ..Default::default()
        };
        let outcome = assess_turn_outcome(assess_params(
            "Completed.",
            &messages,
            harness,
            &cfg,
            Some(&contract),
            true,
        ));
        assert!(
            outcome.verification.contract_satisfied,
            "harness true must satisfy: {:?}",
            outcome.verification
        );
        assert_eq!(outcome.state, CompletionDecision::Completed);
    }

    #[test]
    fn wave_b_verify_on_stop_blocks_mutation_only_completed() {
        let messages = vec![
            edgecrab_types::Message::user("fix the bug"),
            edgecrab_types::Message::tool_result(
                "w1",
                "write_file",
                r#"{"ok":true,"bytes":12,"lines":1,"path":"src/main.rs"}"#,
            ),
            edgecrab_types::Message::assistant("Fixed."),
        ];
        let advisory = HarnessTurnAdvisory::new();
        let turn = MutationTurnState::new();
        let harness = build_turn_harness_snapshot(TurnHarnessBuildParams {
            messages: &messages,
            mutation_turn: &turn,
            cwd: std::path::Path::new("."),
            post_mutation_oracles: false,
            harness_advisory: &advisory,
            guardrail_halt: false,
            task_class: TaskClass::CodeChange,
        });
        let cfg = HarnessConfig {
            verify_on_stop: true,
            ..Default::default()
        };
        let outcome = assess_turn_outcome(assess_params(
            "All fixed.",
            &messages,
            harness,
            &cfg,
            None,
            false,
        ));
        assert_eq!(outcome.state, CompletionDecision::NeedsVerification);
        assert!(should_reopen_loop(&outcome));
    }

    #[test]
    fn wave2_mutation_after_verify_still_needs_verification() {
        // Anthropic long-running harness: test then mutate again → still debt.
        let messages = vec![
            edgecrab_types::Message::user("fix then tweak"),
            edgecrab_types::Message::tool_result(
                "w1",
                "write_file",
                r#"{"ok":true,"bytes":12,"lines":1,"path":"src/main.rs"}"#,
            ),
            edgecrab_types::Message::tool_result(
                "t1",
                "terminal",
                "[terminal_result status=success backend=local cwd=/proj exit_code=0]\nok\n",
            ),
            edgecrab_types::Message::tool_result(
                "w2",
                "write_file",
                r#"{"ok":true,"bytes":20,"lines":2,"path":"src/main.rs"}"#,
            ),
            edgecrab_types::Message::assistant("Done."),
        ];
        assert!(
            coding_verify_on_stop_debt(&messages),
            "mutation after successful test must still be debt"
        );
        let advisory = HarnessTurnAdvisory::new();
        let turn = MutationTurnState::new();
        let harness = build_turn_harness_snapshot(TurnHarnessBuildParams {
            messages: &messages,
            mutation_turn: &turn,
            cwd: std::path::Path::new("."),
            post_mutation_oracles: false,
            harness_advisory: &advisory,
            guardrail_halt: false,
            task_class: TaskClass::CodeChange,
        });
        let cfg = HarnessConfig {
            verify_on_stop: true,
            ..Default::default()
        };
        let outcome = assess_turn_outcome(assess_params(
            "Done.",
            &messages,
            harness,
            &cfg,
            None,
            false,
        ));
        assert_eq!(outcome.state, CompletionDecision::NeedsVerification);
    }

    #[test]
    fn wave2_verify_after_last_mutation_clears_debt() {
        let messages = vec![
            edgecrab_types::Message::user("fix"),
            edgecrab_types::Message::tool_result(
                "w1",
                "write_file",
                r#"{"ok":true,"bytes":12,"lines":1,"path":"src/main.rs"}"#,
            ),
            edgecrab_types::Message::tool_result(
                "t1",
                "terminal",
                "[terminal_result status=success backend=local cwd=/proj exit_code=0]\nok\n",
            ),
            edgecrab_types::Message::assistant("Done."),
        ];
        assert!(!coding_verify_on_stop_debt(&messages));
    }

    #[test]
    fn ha48_visual_storm_blocks_in_snapshot() {
        let mut advisory = HarnessTurnAdvisory::new();
        for _ in 0..6 {
            advisory.record_tool("terminal");
        }
        let messages = vec![edgecrab_types::Message::user("make demo beautiful UX")];
        let turn = MutationTurnState::new();
        let harness = build_turn_harness_snapshot(TurnHarnessBuildParams {
            messages: &messages,
            mutation_turn: &turn,
            cwd: std::path::Path::new("."),
            post_mutation_oracles: false,
            harness_advisory: &advisory,
            guardrail_halt: false,
            task_class: TaskClass::VisualUx,
        });
        assert!(harness.visual_act_storm);
        assert!(harness.blocks_completion());
    }

    #[test]
    fn ha46_guardrail_halt_in_snapshot() {
        let advisory = HarnessTurnAdvisory::new();
        let turn = MutationTurnState::new();
        let harness = build_turn_harness_snapshot(TurnHarnessBuildParams {
            messages: &[],
            mutation_turn: &turn,
            cwd: std::path::Path::new("."),
            post_mutation_oracles: false,
            harness_advisory: &advisory,
            guardrail_halt: true,
            task_class: TaskClass::General,
        });
        assert!(harness.guardrail_halt);
        assert!(harness.blocks_completion());
    }

    #[test]
    fn should_reopen_on_needs_verification() {
        let outcome = RunOutcome::new(
            CompletionDecision::NeedsVerification,
            ExitReason::VerificationPending,
            "needs browser",
        );
        assert!(should_reopen_loop(&outcome));
    }

    #[test]
    fn document_artifact_never_reopens_for_verification() {
        let outcome = RunOutcome::new(
            CompletionDecision::NeedsVerification,
            ExitReason::VerificationPending,
            "No structured verification evidence was recorded.",
        );
        let messages = vec![
            edgecrab_types::Message::user("create a word document in ./demo/docx_raphael"),
            edgecrab_types::Message::assistant_with_tool_calls(
                "",
                vec![edgecrab_types::ToolCall {
                    id: "t1".into(),
                    r#type: "function".into(),
                    function: edgecrab_types::FunctionCall {
                        name: "terminal".into(),
                        arguments: r#"{"command":"python create_doc.py"}"#.into(),
                    },
                    thought_signature: None,
                }],
            ),
            edgecrab_types::Message::tool_result(
                "t1",
                "terminal",
                r#"{"ok":true,"exit_code":0,"stdout":"Saved ./demo/docx_raphael/Profile.docx"}"#,
            ),
        ];
        assert!(crate::task_class::document_done_latch_ready(&messages));
        assert!(!should_reopen_loop_with_messages(&outcome, &messages));
    }

    #[test]
    fn ls_style_reference_still_reopens_when_incomplete() {
        // P9: inspect-only ls must not suppress reopen via false Done latch.
        let outcome = RunOutcome::new(
            CompletionDecision::Incomplete,
            ExitReason::ModelReturnedFinalText,
            "Model stopped without deliverable.",
        );
        let messages = vec![
            edgecrab_types::Message::user(
                r#"Create powerpoint from style "/tmp/style.pptx" in ./demos/raphael"#,
            ),
            edgecrab_types::Message::assistant_with_tool_calls(
                "",
                vec![edgecrab_types::ToolCall {
                    id: "t1".into(),
                    r#type: "function".into(),
                    function: edgecrab_types::FunctionCall {
                        name: "terminal".into(),
                        arguments: r#"{"command":"ls /tmp/style.pptx"}"#.into(),
                    },
                    thought_signature: None,
                }],
            ),
            edgecrab_types::Message::tool_result(
                "t1",
                "terminal",
                r#"{"ok":true,"exit_code":0,"stdout":"-rw-r--r-- 1 me staff 1K /tmp/style.pptx\n"}"#,
            ),
        ];
        assert!(!crate::task_class::document_done_latch_ready(&messages));
        assert!(should_reopen_loop_with_messages(&outcome, &messages));
    }

    #[test]
    fn ha11_budget_fallback_message_mentions_limit() {
        let msg = budget_exhausted_fallback_message(3, 5);
        assert!(msg.contains("3/5"));
        assert!(msg.contains("iteration limit"));
    }
}
