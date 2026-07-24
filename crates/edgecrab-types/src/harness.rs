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
    GuardrailHalt,
    /// Model exhausted the unknown-tool retry budget (Hermes 3-strike partial abort).
    InvalidToolBudget,
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
            Self::GuardrailHalt => "guardrail_halt",
            Self::InvalidToolBudget => "invalid_tool_budget",
        }
    }

    /// Exit-specific headline when more precise than [`CompletionDecision::headline`].
    pub fn headline(self) -> Option<&'static str> {
        match self {
            Self::InvalidToolBudget => Some("Failed — invalid tool call retry budget exhausted."),
            _ => None,
        }
    }

    /// Exit-specific operator hint (structured recovery guidance, not NLP).
    pub fn operator_hint(self) -> Option<&'static str> {
        match self {
            Self::InvalidToolBudget => {
                Some("Use an exact tool name from the active schema; do not invent tool names.")
            }
            _ => None,
        }
    }
}

/// Optional evidence-based completion contract for persistent `/goal` (Hermes Judgment mechanism).
///
/// Empty contract (= all fields blank) preserves free-form goal judging.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GoalContract {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub outcome: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub verification: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub constraints: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub boundaries: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stop_when: String,
}

impl GoalContract {
    pub fn is_empty(&self) -> bool {
        self.outcome.trim().is_empty()
            && self.verification.trim().is_empty()
            && self.constraints.trim().is_empty()
            && self.boundaries.trim().is_empty()
            && self.stop_when.trim().is_empty()
    }

    /// True when the judge / completion policy must require concrete evidence.
    pub fn requires_verification_evidence(&self) -> bool {
        !self.verification.trim().is_empty()
    }

    pub fn to_json(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        serde_json::to_string(self).ok()
    }

    pub fn from_json(raw: Option<&str>) -> Self {
        let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
            return Self::default();
        };
        serde_json::from_str(raw).unwrap_or_default()
    }
}

/// Parse optional inline contract fields from goal text (Hermes-compatible).
///
/// Lines matching `field: value` for known keys are stripped into the contract;
/// remaining non-empty lines become the free-form goal text.
pub fn parse_goal_with_contract(text: &str) -> (String, GoalContract) {
    let mut contract = GoalContract::default();
    let mut goal_lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            if value.is_empty() {
                goal_lines.push(trimmed.to_string());
                continue;
            }
            match key.as_str() {
                "outcome" => contract.outcome = value.to_string(),
                "verification" | "verify" => contract.verification = value.to_string(),
                "constraints" | "constraint" => contract.constraints = value.to_string(),
                "boundaries" | "boundary" | "scope" => contract.boundaries = value.to_string(),
                "stop_when" | "stop-when" | "stop" => contract.stop_when = value.to_string(),
                _ => goal_lines.push(trimmed.to_string()),
            }
        } else {
            goal_lines.push(trimmed.to_string());
        }
    }
    let goal_text = if goal_lines.is_empty() {
        text.trim().to_string()
    } else {
        goal_lines.join("\n")
    };
    (goal_text, contract)
}

/// Summary of whether the task was verified with concrete evidence.
///
/// Single evidence aggregate for VisualUx perception, coding oracles, and goal contracts.
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
    /// When a goal contract sets `verification`, the ledger tracks satisfaction here.
    #[serde(default)]
    pub contract_required: bool,
    #[serde(default)]
    pub contract_satisfied: bool,
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

    /// Prefer exit-specific headline when present (typed exits over generic state).
    pub fn headline(&self) -> &'static str {
        self.exit_reason
            .headline()
            .unwrap_or_else(|| self.state.headline())
    }

    /// Prefer exit-specific hint, else completion-state hint.
    pub fn operator_hint(&self) -> Option<&'static str> {
        self.exit_reason
            .operator_hint()
            .or_else(|| self.state.operator_hint())
    }

    /// `emoji + headline` — single source for TUI / gateway notices.
    pub fn emoji_headline(&self) -> String {
        format!("{} {}", self.state.emoji(), self.headline())
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
    fn invalid_tool_budget_has_typed_headline_and_hint() {
        let outcome = RunOutcome::new(
            CompletionDecision::Failed,
            ExitReason::InvalidToolBudget,
            "Model generated invalid tool call: quick_stock_quote",
        );
        assert_eq!(
            outcome.headline(),
            "Failed — invalid tool call retry budget exhausted."
        );
        assert!(
            outcome
                .operator_hint()
                .expect("invalid-tool budget outcome should include operator hint")
                .contains("exact tool name")
        );
        assert!(outcome.emoji_headline().starts_with('❌'));
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

    #[test]
    fn parse_goal_with_contract_strips_fields() {
        let (goal, contract) = parse_goal_with_contract(
            "Ship auth refactor\nverification: cargo test -p edgecrab-core\nconstraints: keep public API\n",
        );
        assert_eq!(goal, "Ship auth refactor");
        assert_eq!(contract.verification, "cargo test -p edgecrab-core");
        assert_eq!(contract.constraints, "keep public API");
        assert!(contract.requires_verification_evidence());
    }

    #[test]
    fn empty_goal_contract_round_trips() {
        assert!(GoalContract::default().is_empty());
        assert!(GoalContract::default().to_json().is_none());
        assert!(GoalContract::from_json(None).is_empty());
    }
}
