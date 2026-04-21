use edgecrab_types::{
    CompletionDecision, ExitReason, Message, ReportedTaskStatus, Role, RunOutcome, TaskStatusKind,
    VerificationSummary,
};

/// Snapshot of end-of-run state inspected by the completion policy.
pub struct CompletionContext<'a> {
    pub final_response: &'a str,
    pub messages: &'a [Message],
    pub interrupted: bool,
    pub budget_exhausted: bool,
    pub pending_approval: bool,
    pub pending_clarification: bool,
    pub active_todos: usize,
    pub blocked_todos: usize,
    pub child_runs_in_flight: usize,
}

pub trait CompletionPolicy: Send + Sync {
    fn assess(&self, ctx: &CompletionContext<'_>) -> RunOutcome;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultCompletionPolicy;

pub fn assess_completion(ctx: &CompletionContext<'_>) -> RunOutcome {
    DefaultCompletionPolicy.assess(ctx)
}

impl CompletionPolicy for DefaultCompletionPolicy {
    fn assess(&self, ctx: &CompletionContext<'_>) -> RunOutcome {
        let pending_clarification = ctx.pending_clarification || has_clarify_marker(ctx);
        let pending_approval = ctx.pending_approval || has_approval_marker(ctx);
        let verification = collect_verification_summary(ctx.messages);
        let reported_progress = collect_reported_progress_state(ctx.messages);
        let reported_blocked = matches!(
            reported_progress.latest_status,
            Some(TaskStatusKind::Blocked)
        );
        let reported_in_progress = matches!(
            reported_progress.latest_status,
            Some(TaskStatusKind::InProgress)
        );
        let has_remaining_steps = !reported_progress.remaining_steps.is_empty();

        let mut outcome = if ctx.interrupted {
            RunOutcome::new(
                CompletionDecision::Interrupted,
                ExitReason::Interrupted,
                "Stopped — the run was interrupted.",
            )
        } else if pending_clarification {
            RunOutcome::new(
                CompletionDecision::NeedsUserInput,
                ExitReason::AwaitingClarification,
                "Needs input — clarification is still required.",
            )
        } else if pending_approval || ctx.blocked_todos > 0 || reported_blocked {
            RunOutcome::new(
                CompletionDecision::Blocked,
                if pending_approval {
                    ExitReason::AwaitingApproval
                } else {
                    ExitReason::PendingTasks
                },
                "Blocked — waiting for approval or an unresolved dependency.",
            )
        } else if ctx.budget_exhausted {
            RunOutcome::new(
                CompletionDecision::BudgetExhausted,
                ExitReason::BudgetExhausted,
                "Stopped — the iteration budget was exhausted before the task was complete.",
            )
        } else if ctx.child_runs_in_flight > 0
            || ctx.active_todos > 0
            || reported_in_progress
            || has_remaining_steps
        {
            RunOutcome::new(
                CompletionDecision::Incomplete,
                ExitReason::PendingTasks,
                "Incomplete — progress was reported but work still remains.",
            )
        } else if ctx.final_response.trim().is_empty() {
            RunOutcome::new(
                CompletionDecision::Failed,
                ExitReason::NoMoreToolCalls,
                "Failed — the run ended without a usable final response.",
            )
        } else if verification.required && !verification.evidence_present {
            RunOutcome::new(
                CompletionDecision::NeedsVerification,
                ExitReason::VerificationPending,
                "Needs verification — work was attempted but concrete evidence is still missing.",
            )
        } else {
            RunOutcome::new(
                CompletionDecision::Completed,
                ExitReason::ModelReturnedFinalText,
                "Completed — request satisfied and verified.",
            )
        };

        outcome.evidence = verification.evidence.clone();
        outcome.verification = verification;
        outcome.active_tasks = ctx.active_todos;
        outcome.blocked_tasks = ctx.blocked_todos;
        outcome
    }
}

fn has_clarify_marker(ctx: &CompletionContext<'_>) -> bool {
    ctx.final_response.contains("[CLARIFY]")
        || ctx
            .messages
            .iter()
            .any(|msg| msg.text_content().contains("[CLARIFY]"))
}

fn has_approval_marker(ctx: &CompletionContext<'_>) -> bool {
    let approval_tokens = [
        "approval required",
        "reply /approve",
        "approve session",
        "awaiting approval",
    ];

    approval_tokens
        .iter()
        .any(|needle| ctx.final_response.to_ascii_lowercase().contains(needle))
        || ctx.messages.iter().any(|msg| {
            let lower = msg.text_content().to_ascii_lowercase();
            approval_tokens.iter().any(|needle| lower.contains(needle))
        })
}

#[derive(Debug, Default)]
struct ReportedProgressState {
    latest_status: Option<TaskStatusKind>,
    remaining_steps: Vec<String>,
}

fn collect_reported_progress_state(messages: &[Message]) -> ReportedProgressState {
    let mut state = ReportedProgressState::default();

    for msg in messages {
        if msg.role != Role::Tool || msg.name.as_deref() != Some("report_task_status") {
            continue;
        }

        let Ok(report) = serde_json::from_str::<ReportedTaskStatus>(&msg.text_content()) else {
            continue;
        };

        state.latest_status = Some(report.status);
        state.remaining_steps = report
            .remaining_steps
            .into_iter()
            .filter(|item| !item.trim().is_empty())
            .collect();
    }

    state
}

fn collect_verification_summary(messages: &[Message]) -> VerificationSummary {
    let mut required = false;
    let mut evidence = Vec::new();

    for msg in messages {
        if msg.role != Role::Tool {
            continue;
        }

        let Some(name) = msg.name.as_deref() else {
            continue;
        };
        let content = msg.text_content();

        if name == "report_task_status" {
            required = true;
            if let Ok(report) = serde_json::from_str::<ReportedTaskStatus>(&content) {
                match report.status {
                    TaskStatusKind::Completed => {
                        if report.evidence.is_empty() {
                            if !report.summary.trim().is_empty() {
                                evidence.push(report.summary.trim().to_string());
                            }
                        } else {
                            evidence.extend(
                                report
                                    .evidence
                                    .into_iter()
                                    .filter(|item| !item.trim().is_empty()),
                            );
                        }
                    }
                    TaskStatusKind::Blocked | TaskStatusKind::InProgress => {
                        evidence.extend(
                            report
                                .evidence
                                .into_iter()
                                .filter(|item| !item.trim().is_empty()),
                        );
                    }
                }
            }
            continue;
        }

        if !is_verification_tool(name) {
            continue;
        }

        required = true;
        if looks_like_error(&content) {
            continue;
        }

        let summary = first_nonempty_line(&content)
            .map(|line| truncate(line, 140))
            .filter(|line| !line.trim().is_empty())
            .unwrap_or_else(|| format!("{name} completed"));
        evidence.push(format!("{name}: {summary}"));
    }

    evidence.sort();
    evidence.dedup();

    VerificationSummary {
        required,
        evidence_present: !evidence.is_empty(),
        debt_reason: (required && evidence.is_empty())
            .then_some("No structured verification evidence was recorded.".to_string()),
        evidence,
    }
}

fn is_verification_tool(name: &str) -> bool {
    matches!(
        name,
        "terminal"
            | "run_process"
            | "write_file"
            | "patch"
            | "execute_code"
            | "delegate_task"
            | "manage_cron_jobs"
            | "checkpoint"
            | "lsp_apply_code_action"
            | "lsp_rename"
            | "lsp_format_document"
            | "lsp_format_range"
    )
}

fn looks_like_error(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("tool error")
        || lower.contains("\"response_type\":\"tool_error\"")
        || lower.contains("permission denied")
        || lower.contains("failed")
        || lower.contains("error")
}

fn first_nonempty_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

fn truncate(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_types::Message;

    #[test]
    fn budget_exhausted_is_never_reported_complete() {
        let ctx = CompletionContext {
            final_response: "I ran out of budget.",
            messages: &[],
            interrupted: false,
            budget_exhausted: true,
            pending_approval: false,
            pending_clarification: false,
            active_todos: 0,
            blocked_todos: 0,
            child_runs_in_flight: 0,
        };

        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::BudgetExhausted);
        assert!(!outcome.is_success());
    }

    #[test]
    fn active_todos_keep_run_incomplete() {
        let ctx = CompletionContext {
            final_response: "Done.",
            messages: &[],
            interrupted: false,
            budget_exhausted: false,
            pending_approval: false,
            pending_clarification: false,
            active_todos: 2,
            blocked_todos: 0,
            child_runs_in_flight: 0,
        };

        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::Incomplete);
    }

    #[test]
    fn clarify_marker_maps_to_needs_user_input() {
        let msg = Message::assistant("[CLARIFY] Which file should I edit?");
        let messages = vec![msg];
        let ctx = CompletionContext {
            final_response: "[CLARIFY] Which file should I edit?",
            messages: &messages,
            interrupted: false,
            budget_exhausted: false,
            pending_approval: false,
            pending_clarification: false,
            active_todos: 0,
            blocked_todos: 0,
            child_runs_in_flight: 0,
        };

        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::NeedsUserInput);
    }

    #[test]
    fn blocked_todos_map_to_blocked() {
        let ctx = CompletionContext {
            final_response: "Need approval.",
            messages: &[],
            interrupted: false,
            budget_exhausted: false,
            pending_approval: false,
            pending_clarification: false,
            active_todos: 0,
            blocked_todos: 1,
            child_runs_in_flight: 0,
        };

        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::Blocked);
    }

    #[test]
    fn explicit_pending_approval_maps_to_blocked() {
        let ctx = CompletionContext {
            final_response: "Waiting.",
            messages: &[],
            interrupted: false,
            budget_exhausted: false,
            pending_approval: true,
            pending_clarification: false,
            active_todos: 0,
            blocked_todos: 0,
            child_runs_in_flight: 0,
        };

        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::Blocked);
        assert_eq!(outcome.exit_reason, ExitReason::AwaitingApproval);
    }

    #[test]
    fn explicit_pending_clarification_maps_to_needs_user_input() {
        let ctx = CompletionContext {
            final_response: "Waiting.",
            messages: &[],
            interrupted: false,
            budget_exhausted: false,
            pending_approval: false,
            pending_clarification: true,
            active_todos: 0,
            blocked_todos: 0,
            child_runs_in_flight: 0,
        };

        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::NeedsUserInput);
        assert_eq!(outcome.exit_reason, ExitReason::AwaitingClarification);
    }

    #[test]
    fn reported_task_status_supplies_verification_evidence() {
        let report = serde_json::json!({
            "status": "completed",
            "summary": "cargo test passed",
            "evidence": ["test suite passed"],
            "remaining_steps": []
        })
        .to_string();
        let messages = vec![Message::tool_result("tc_1", "report_task_status", &report)];
        let ctx = CompletionContext {
            final_response: "All set.",
            messages: &messages,
            interrupted: false,
            budget_exhausted: false,
            pending_approval: false,
            pending_clarification: false,
            active_todos: 0,
            blocked_todos: 0,
            child_runs_in_flight: 0,
        };

        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::Completed);
        assert!(outcome.verification.evidence_present);
    }

    #[test]
    fn in_progress_report_keeps_run_incomplete() {
        let report = serde_json::json!({
            "status": "in_progress",
            "summary": "wired the UI",
            "evidence": ["patched app.rs"],
            "remaining_steps": ["run tests", "polish status copy"]
        })
        .to_string();
        let messages = vec![Message::tool_result("tc_2", "report_task_status", &report)];
        let ctx = CompletionContext {
            final_response: "Almost done.",
            messages: &messages,
            interrupted: false,
            budget_exhausted: false,
            pending_approval: false,
            pending_clarification: false,
            active_todos: 0,
            blocked_todos: 0,
            child_runs_in_flight: 0,
        };

        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::Incomplete);
    }

    #[test]
    fn completed_report_with_remaining_steps_stays_incomplete() {
        let report = serde_json::json!({
            "status": "completed",
            "summary": "implemented the change",
            "evidence": ["files updated"],
            "remaining_steps": ["verify with tests"]
        })
        .to_string();
        let messages = vec![Message::tool_result("tc_3", "report_task_status", &report)];
        let ctx = CompletionContext {
            final_response: "Done.",
            messages: &messages,
            interrupted: false,
            budget_exhausted: false,
            pending_approval: false,
            pending_clarification: false,
            active_todos: 0,
            blocked_todos: 0,
            child_runs_in_flight: 0,
        };

        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::Incomplete);
    }
}
