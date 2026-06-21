//! Turn epilogue — single assess path for mid-loop and end-loop (spec 017 P1-2 / HA-45).
//!
//! DRY: `conversation.rs` calls these functions; no duplicate assess logic.

use edgecrab_tools::{
    HarnessAdvisorySignals, HarnessBuildInput, HarnessSnapshot, MutationTurnState,
    build_harness_snapshot,
};
use edgecrab_types::{CompletionDecision, Message, RunOutcome};

use crate::completion_assessor::{CompletionContext, assess_completion};
use crate::config::HarnessConfig;
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
    pub pending_approval: bool,
    pub pending_clarification: bool,
    pub active_todos: usize,
    pub blocked_todos: usize,
    pub child_runs_in_flight: usize,
    pub harness: HarnessSnapshot,
    pub harness_config: &'a HarnessConfig,
}

/// Single completion assess entry (provisional + final).
pub fn assess_turn_outcome(params: TurnAssessParams<'_>) -> RunOutcome {
    let verification_strict = effective_verification_strict(params.harness_config, params.messages);
    assess_completion(&CompletionContext {
        final_response: params.final_response,
        messages: params.messages,
        interrupted: params.interrupted,
        budget_exhausted: params.budget_exhausted,
        pending_approval: params.pending_approval,
        pending_clarification: params.pending_clarification,
        active_todos: params.active_todos,
        blocked_todos: params.blocked_todos,
        child_runs_in_flight: params.child_runs_in_flight,
        harness: params.harness,
        verification_strict,
    })
}

/// Re-open the loop when assess rejects premature model text.
pub fn should_reopen_loop(outcome: &RunOutcome) -> bool {
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
            pending_approval: false,
            pending_clarification: false,
            active_todos: 0,
            blocked_todos: 0,
            child_runs_in_flight: 0,
            harness,
            harness_config: &HarnessConfig::default(),
        });
        assert_eq!(outcome.state, CompletionDecision::Incomplete);
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
    fn ha11_budget_fallback_message_mentions_limit() {
        let msg = budget_exhausted_fallback_message(3, 5);
        assert!(msg.contains("3/5"));
        assert!(msg.contains("iteration limit"));
    }
}
