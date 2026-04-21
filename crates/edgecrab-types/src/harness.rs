use serde::{Deserialize, Serialize};

/// Terminal completion state for a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompletionDecision {
    Completed,
    NeedsUserInput,
    Blocked,
    BudgetExhausted,
    Interrupted,
    Failed,
    #[default]
    Incomplete,
    NeedsVerification,
}

impl CompletionDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::NeedsUserInput => "needs_user_input",
            Self::Blocked => "blocked",
            Self::BudgetExhausted => "budget_exhausted",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
            Self::Incomplete => "incomplete",
            Self::NeedsVerification => "needs_verification",
        }
    }

    pub fn emoji(self) -> &'static str {
        match self {
            Self::Completed => "✅",
            Self::NeedsUserInput => "❓",
            Self::Blocked => "⏸",
            Self::BudgetExhausted => "⚠",
            Self::Interrupted => "⛔",
            Self::Failed => "❌",
            Self::Incomplete => "⚠",
            Self::NeedsVerification => "🔎",
        }
    }

    pub fn headline(self) -> &'static str {
        match self {
            Self::Completed => "Completed — request satisfied and verified.",
            Self::NeedsUserInput => "Needs input — more information is still required.",
            Self::Blocked => "Blocked — waiting on approval or another dependency.",
            Self::BudgetExhausted => {
                "Stopped before completion — the iteration budget was exhausted."
            }
            Self::Interrupted => "Stopped — the run was interrupted.",
            Self::Failed => "Failed — the run ended unexpectedly.",
            Self::Incomplete => "Incomplete — work is still pending.",
            Self::NeedsVerification => "Needs verification — concrete evidence is still missing.",
        }
    }

    pub fn compact_label(self) -> &'static str {
        match self {
            Self::Completed => "done",
            Self::NeedsUserInput => "reply needed",
            Self::Blocked => "blocked",
            Self::BudgetExhausted => "budget hit",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
            Self::Incomplete => "incomplete",
            Self::NeedsVerification => "verify",
        }
    }

    pub fn operator_hint(self) -> Option<&'static str> {
        match self {
            Self::NeedsUserInput => Some("Reply below and EdgeCrab can continue immediately."),
            Self::Blocked => Some("Resolve the dependency or approval to let the run advance."),
            Self::Incomplete => {
                Some("The harness kept the run honest because unfinished work remained.")
            }
            Self::NeedsVerification => {
                Some("The finish line only counts once there is concrete evidence.")
            }
            _ => None,
        }
    }

    pub fn is_success(self) -> bool {
        matches!(self, Self::Completed)
    }
}

/// Concrete reason the run stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    ModelReturnedFinalText,
    #[default]
    NoMoreToolCalls,
    BudgetExhausted,
    Interrupted,
    AwaitingClarification,
    AwaitingApproval,
    PendingTasks,
    VerificationPending,
    ToolFailure,
    ModelError,
}

impl ExitReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ModelReturnedFinalText => "model_returned_final_text",
            Self::NoMoreToolCalls => "no_more_tool_calls",
            Self::BudgetExhausted => "budget_exhausted",
            Self::Interrupted => "interrupted",
            Self::AwaitingClarification => "awaiting_clarification",
            Self::AwaitingApproval => "awaiting_approval",
            Self::PendingTasks => "pending_tasks",
            Self::VerificationPending => "verification_pending",
            Self::ToolFailure => "tool_failure",
            Self::ModelError => "model_error",
        }
    }
}

/// Summary of whether the task was verified with concrete evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct VerificationSummary {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub evidence_present: bool,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debt_reason: Option<String>,
}

/// Structured terminal outcome for a conversation run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RunOutcome {
    pub state: CompletionDecision,
    pub exit_reason: ExitReason,
    pub user_summary: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub verification: VerificationSummary,
    #[serde(default)]
    pub active_tasks: usize,
    #[serde(default)]
    pub blocked_tasks: usize,
}

impl RunOutcome {
    pub fn new(
        state: CompletionDecision,
        exit_reason: ExitReason,
        user_summary: impl Into<String>,
    ) -> Self {
        Self {
            state,
            exit_reason,
            user_summary: user_summary.into(),
            evidence: Vec::new(),
            verification: VerificationSummary::default(),
            active_tasks: 0,
            blocked_tasks: 0,
        }
    }

    pub fn is_success(&self) -> bool {
        self.state.is_success()
    }
}

/// Structured status signal emitted by the model via the report_task_status tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedTaskStatus {
    pub status: TaskStatusKind,
    pub summary: String,
    #[serde(default)]
    pub evidence: Vec<String>,
    #[serde(default)]
    pub remaining_steps: Vec<String>,
}

/// Status variants accepted by the report_task_status tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatusKind {
    InProgress,
    Blocked,
    Completed,
}

impl TaskStatusKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Completed => "completed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_decision_string_labels_are_stable() {
        assert_eq!(CompletionDecision::Completed.as_str(), "completed");
        assert_eq!(CompletionDecision::Completed.emoji(), "✅");
        assert_eq!(CompletionDecision::Completed.compact_label(), "done");
        assert_eq!(
            CompletionDecision::NeedsVerification.as_str(),
            "needs_verification"
        );
    }

    #[test]
    fn run_outcome_defaults_to_incomplete() {
        let outcome = RunOutcome::default();
        assert_eq!(outcome.state, CompletionDecision::Incomplete);
        assert_eq!(outcome.exit_reason, ExitReason::NoMoreToolCalls);
    }

    #[test]
    fn reported_task_status_round_trips() {
        let status = ReportedTaskStatus {
            status: TaskStatusKind::Completed,
            summary: "tests passed".into(),
            evidence: vec!["cargo test --workspace".into()],
            remaining_steps: Vec::new(),
        };

        let json = serde_json::to_string(&status).expect("json");
        let parsed: ReportedTaskStatus = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed.status, TaskStatusKind::Completed);
        assert_eq!(parsed.evidence.len(), 1);
    }
}
