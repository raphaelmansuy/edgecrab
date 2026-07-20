//! Turn completion explainer + pending-tool detection (Hermes turn_finalizer parity).
//!
//! Single responsibility: format operator-facing run notices from structured
//! [`RunOutcome`] fields. TUI and gateway must call [`format_operator_notice`]
//! rather than re-wrapping headlines (DRY).

use edgecrab_types::{ExitReason, Message, Role, RunOutcome};

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

fn line_already_present(lines: &[String], candidate: &str) -> bool {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return true;
    }
    lines.iter().any(|l| {
        let existing = l.trim();
        existing == trimmed || existing.ends_with(trimmed) || trimmed.ends_with(existing)
    })
}

/// Hermes-style multi-line turn completion explanation for TUI / gateway.
pub fn format_turn_completion_explanation(
    outcome: &RunOutcome,
    ctx: &TurnCompletionContext,
) -> String {
    let headline = outcome.headline();
    let emoji_headline = outcome.emoji_headline();
    let mut lines = vec![emoji_headline.clone()];

    let summary = outcome.user_summary.trim();
    if !summary.is_empty() && summary != headline && summary != emoji_headline.as_str() {
        for summary_line in summary.lines() {
            let trimmed = summary_line.trim();
            if trimmed.is_empty() || trimmed == headline || trimmed == emoji_headline {
                continue;
            }
            // Skip emoji-prefixed headline duplicates from a prior enrich pass.
            let without_emoji = trimmed
                .strip_prefix('❌')
                .or_else(|| trimmed.strip_prefix('⚠'))
                .or_else(|| trimmed.strip_prefix('✅'))
                .or_else(|| trimmed.strip_prefix('🔎'))
                .or_else(|| trimmed.strip_prefix('❓'))
                .or_else(|| trimmed.strip_prefix('⏸'))
                .or_else(|| trimmed.strip_prefix('⛔'))
                .map(str::trim)
                .unwrap_or(trimmed);
            if without_emoji == headline || line_already_present(&lines, trimmed) {
                continue;
            }
            lines.push(trimmed.to_string());
        }
    }

    // Invalid-tool budget abort closes tool calls before exit — pending warning would be a false alarm.
    if ctx.pending_tool_results > 0 && outcome.exit_reason != ExitReason::InvalidToolBudget {
        let warning = format!(
            "Warning: {n} tool call(s) ended without results — history may be inconsistent.",
            n = ctx.pending_tool_results
        );
        if !line_already_present(&lines, &warning) {
            lines.push(warning);
        }
    }

    if let Some(reason) = ctx
        .harness_block_reason
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        let harness_line = format!("Harness: {reason}");
        if !line_already_present(&lines, reason) && !line_already_present(&lines, &harness_line) {
            lines.push(harness_line);
        }
    }

    match ctx.task_class {
        TaskClass::VisualUx => {
            if outcome.state == edgecrab_types::CompletionDecision::NeedsVerification {
                let hint =
                    "Visual task: open a browser preview or capture a screenshot before finishing.";
                if !line_already_present(&lines, hint) {
                    lines.push(hint.into());
                }
            }
        }
        TaskClass::Document => {
            if outcome.state == edgecrab_types::CompletionDecision::NeedsVerification {
                let hint = "Document task: confirm the artifact exists on disk (non-zero .pptx/.pdf bytes).";
                if !line_already_present(&lines, hint) {
                    lines.push(hint.into());
                }
            }
        }
        TaskClass::MediaRender => {
            if outcome.state == edgecrab_types::CompletionDecision::NeedsVerification {
                let hint =
                    "Media task: confirm render output exists on disk (non-zero .mp4/.webm bytes).";
                if !line_already_present(&lines, hint) {
                    lines.push(hint.into());
                }
            }
        }
        TaskClass::CodeChange => {}
        TaskClass::Research | TaskClass::General => {}
    }

    if ctx.copilot_nonstreaming {
        let hint = "Provider: Copilot tool turn used non-streaming compose (expect longer waits).";
        if !line_already_present(&lines, hint) {
            lines.push(hint.into());
        }
    }

    if let Some(hint) = outcome.operator_hint()
        && !line_already_present(&lines, hint)
    {
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

/// Single operator notice for TUI / gateway (DRY — do not re-prefix enriched summaries).
///
/// If `user_summary` was already produced by [`format_turn_completion_explanation`]
/// (via `enrich_turn_outcome`), return it unchanged. Otherwise build a fresh notice.
pub fn format_operator_notice(outcome: &RunOutcome) -> String {
    let summary = outcome.user_summary.trim();
    let emoji_headline = outcome.emoji_headline();
    if !summary.is_empty() {
        let first = summary.lines().next().unwrap_or("").trim();
        if first == emoji_headline
            || summary == emoji_headline
            || summary.starts_with(&emoji_headline)
        {
            return summary.to_string();
        }
    }
    format_turn_completion_explanation(outcome, &TurnCompletionContext::default())
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

    #[test]
    fn invalid_tool_budget_skips_pending_tool_warning() {
        let outcome = RunOutcome::new(
            CompletionDecision::Failed,
            ExitReason::InvalidToolBudget,
            "Model generated invalid tool call: quick_stock_quote",
        );
        let text = format_turn_completion_explanation(
            &outcome,
            &TurnCompletionContext {
                pending_tool_results: 1,
                ..Default::default()
            },
        );
        assert!(!text.contains("without results"));
        assert!(text.contains("quick_stock_quote"));
        assert!(text.contains("invalid tool call retry budget exhausted"));
        assert!(text.contains("do not invent tool names"));
    }

    #[test]
    fn format_operator_notice_does_not_double_wrap_enriched_summary() {
        let mut outcome = RunOutcome::new(
            CompletionDecision::Failed,
            ExitReason::InvalidToolBudget,
            "Model generated invalid tool call: quick_stock_quote",
        );
        outcome.user_summary =
            format_turn_completion_explanation(&outcome, &TurnCompletionContext::default());
        let notice = format_operator_notice(&outcome);
        assert_eq!(
            notice.matches("❌").count(),
            1,
            "emoji must appear once:\n{notice}"
        );
        assert_eq!(
            notice
                .matches("invalid tool call retry budget exhausted")
                .count(),
            1,
            "typed headline once:\n{notice}"
        );
        assert_eq!(
            notice
                .matches("Model generated invalid tool call: quick_stock_quote")
                .count(),
            1,
            "abort reason once:\n{notice}"
        );
    }

    #[test]
    fn dedupes_headline_and_operator_hint_when_summary_already_has_them() {
        let mut outcome = RunOutcome::new(
            CompletionDecision::Incomplete,
            ExitReason::NoMoreToolCalls,
            "Incomplete — work is still pending.",
        );
        outcome.user_summary = format!(
            "⚠ {}\n{}\n{}",
            CompletionDecision::Incomplete.headline(),
            "Incomplete — 1 tool call(s) ended without results.",
            CompletionDecision::Incomplete.operator_hint().unwrap()
        );
        let text = format_turn_completion_explanation(
            &outcome,
            &TurnCompletionContext {
                harness_block_reason: Some(
                    "Incomplete — 1 tool call(s) ended without results.".into(),
                ),
                ..Default::default()
            },
        );
        let headline_count = text
            .matches(CompletionDecision::Incomplete.headline())
            .count();
        assert_eq!(headline_count, 1, "headline must appear once:\n{text}");
        let hint = CompletionDecision::Incomplete.operator_hint().unwrap();
        assert_eq!(
            text.matches(hint).count(),
            1,
            "operator hint must appear once:\n{text}"
        );
        assert_eq!(
            text.matches("Incomplete — 1 tool call(s) ended without results.")
                .count(),
            1,
            "harness reason must not duplicate summary:\n{text}"
        );
    }
}
