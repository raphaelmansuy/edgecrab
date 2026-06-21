//! Turn completion explainer + pending-tool detection (Hermes turn_finalizer parity).

use edgecrab_types::{Message, Role, RunOutcome};

use crate::task_class::TaskClass;

/// Extra context for end-of-turn operator messaging.
#[derive(Debug, Default)]
pub struct TurnCompletionContext {
    pub pending_tool_results: usize,
    pub harness_block_reason: Option<String>,
    pub task_class: TaskClass,
    pub copilot_nonstreaming: bool,
}

/// Count assistant tool calls that never received a matching tool result.
pub fn count_unanswered_tool_calls(messages: &[Message]) -> usize {
    let mut pending: std::collections::HashMap<String, ()> = std::collections::HashMap::new();
    for msg in messages {
        if msg.role == Role::Assistant
            && let Some(calls) = msg.tool_calls.as_ref()
        {
            for call in calls {
                pending.insert(call.id.clone(), ());
            }
        }
        if msg.role == Role::Tool
            && let Some(id) = msg.tool_call_id.as_deref()
        {
            pending.remove(id);
        }
    }
    pending.len()
}

/// Hermes-style multi-line turn completion explanation for TUI / gateway.
pub fn format_turn_completion_explanation(
    outcome: &RunOutcome,
    ctx: &TurnCompletionContext,
) -> String {
    let mut lines = vec![format!(
        "{} {}",
        outcome.state.emoji(),
        outcome.state.headline()
    )];

    let summary = outcome.user_summary.trim();
    if !summary.is_empty()
        && summary != outcome.state.headline()
        && !lines.iter().any(|l| l.contains(summary))
    {
        lines.push(summary.to_string());
    }

    if ctx.pending_tool_results > 0 {
        lines.push(format!(
            "Warning: {n} tool call(s) ended without results — history may be inconsistent.",
            n = ctx.pending_tool_results
        ));
    }

    if let Some(reason) = ctx
        .harness_block_reason
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        lines.push(format!("Harness: {reason}"));
    }

    match ctx.task_class {
        TaskClass::VisualUx => {
            if outcome.state == edgecrab_types::CompletionDecision::NeedsVerification {
                lines.push(
                    "Visual task: open a browser preview or capture a screenshot before finishing."
                        .into(),
                );
            }
        }
        TaskClass::CodeChange => {}
        TaskClass::Research | TaskClass::General => {}
    }

    if ctx.copilot_nonstreaming {
        lines.push(
            "Provider: Copilot tool turn used non-streaming compose (expect longer waits).".into(),
        );
    }

    if let Some(hint) = outcome.state.operator_hint() {
        lines.push(hint.to_string());
    }

    if outcome.active_tasks > 0 || outcome.blocked_tasks > 0 {
        lines.push(format!(
            "Tasks remaining: {} active, {} blocked.",
            outcome.active_tasks, outcome.blocked_tasks
        ));
    }

    if let Some(evidence) = outcome.evidence.iter().find(|item| !item.trim().is_empty()) {
        lines.push(format!("Evidence: {}", crate::safe_truncate(evidence, 120)));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_types::{CompletionDecision, ExitReason, RunOutcome};

    #[test]
    fn pending_tool_warning_in_explanation() {
        let outcome = RunOutcome::new(
            CompletionDecision::Incomplete,
            ExitReason::NoMoreToolCalls,
            "Incomplete — work remains.",
        );
        let text = format_turn_completion_explanation(
            &outcome,
            &TurnCompletionContext {
                pending_tool_results: 2,
                ..Default::default()
            },
        );
        assert!(text.contains("without results"));
    }
}
