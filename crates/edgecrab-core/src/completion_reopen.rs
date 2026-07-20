//! Completion reopen policy (025 harness balance).
//!
//! SOLID: single owner for “do not stop yet” decisions. Conversation calls this;
//! epilogue owns the inject message text.

use edgecrab_types::{CompletionDecision, ExitReason, Message, RunOutcome};

use crate::evidence_latch::EvidenceAssessSnapshot;
use crate::turn_epilogue::should_reopen_loop_with_evidence;

/// Per-turn reopen budget (Hermes `verification_stop` max_attempts=2 parity).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompletionReopenGate {
    pub max_reopens: u32,
    pub reopens_used: u32,
}

impl CompletionReopenGate {
    pub fn new(max_reopens: u32) -> Self {
        Self {
            max_reopens,
            reopens_used: 0,
        }
    }

    pub fn remaining(&self) -> u32 {
        self.max_reopens.saturating_sub(self.reopens_used)
    }
}

/// Decision after the model returned final text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReopenDecision {
    /// Inject follow-up and continue the ReAct loop.
    Reopen,
    /// Evidence/debt still missing but reopen budget exhausted — end turn.
    CapReached,
    /// Assessor accepts stop (or terminal halt) — do not reopen.
    DoNotReopen,
}

/// Decide whether to reopen, respecting evidence latches and reopen cap.
pub fn decide_completion_reopen(
    outcome: &RunOutcome,
    messages: &[Message],
    evidence: EvidenceAssessSnapshot,
    gate: &CompletionReopenGate,
) -> ReopenDecision {
    if !should_reopen_loop_with_evidence(outcome, messages, evidence) {
        return ReopenDecision::DoNotReopen;
    }
    if gate.reopens_used >= gate.max_reopens {
        return ReopenDecision::CapReached;
    }
    ReopenDecision::Reopen
}

/// Outcome when reopen cap is hit — terminal, never reopens again.
pub fn reopen_cap_outcome(final_response: &str, prior: &RunOutcome) -> RunOutcome {
    let summary = if final_response.trim().is_empty() {
        "Stopped — verification reopen budget exhausted. Deliverables may exist; \
         further auto-verify was capped to avoid thrash."
            .to_string()
    } else {
        final_response.trim().to_string()
    };
    let mut outcome = RunOutcome::new(
        CompletionDecision::Failed,
        ExitReason::GuardrailHalt,
        summary,
    );
    outcome.evidence = prior.evidence.clone();
    outcome.verification = prior.verification.clone();
    outcome.verification.debt_reason = Some(match prior.verification.debt_reason.as_deref() {
        Some(debt) => format!("Reopen cap reached with unresolved debt: {debt}"),
        None => "Reopen cap reached — verification evidence still incomplete.".into(),
    });
    outcome.active_tasks = prior.active_tasks;
    outcome.blocked_tasks = prior.blocked_tasks;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_types::VerificationSummary;

    fn needs_verify() -> RunOutcome {
        let mut o = RunOutcome::new(
            CompletionDecision::NeedsVerification,
            ExitReason::VerificationPending,
            "needs verify",
        );
        o.verification = VerificationSummary {
            required: true,
            evidence_present: false,
            debt_reason: Some("missing browser evidence".into()),
            evidence: vec![],
            contract_required: false,
            contract_satisfied: false,
        };
        o
    }

    #[test]
    fn reopen_allowed_under_cap() {
        let gate = CompletionReopenGate::new(2);
        assert_eq!(
            decide_completion_reopen(
                &needs_verify(),
                &[],
                EvidenceAssessSnapshot::default(),
                &gate
            ),
            ReopenDecision::Reopen
        );
    }

    #[test]
    fn cap_reached_after_max() {
        let gate = CompletionReopenGate {
            max_reopens: 2,
            reopens_used: 2,
        };
        assert_eq!(
            decide_completion_reopen(
                &needs_verify(),
                &[],
                EvidenceAssessSnapshot::default(),
                &gate
            ),
            ReopenDecision::CapReached
        );
    }

    #[test]
    fn latched_visual_never_reopens() {
        let gate = CompletionReopenGate::new(2);
        let evidence = EvidenceAssessSnapshot {
            visual_complete: true,
            ..Default::default()
        };
        assert_eq!(
            decide_completion_reopen(&needs_verify(), &[], evidence, &gate),
            ReopenDecision::DoNotReopen
        );
    }

    #[test]
    fn reopen_cap_outcome_is_terminal_halt() {
        let prior = needs_verify();
        let out = reopen_cap_outcome("game done", &prior);
        assert_eq!(out.state, CompletionDecision::Failed);
        assert_eq!(out.exit_reason, ExitReason::GuardrailHalt);
        assert!(!should_reopen_loop_with_evidence(
            &out,
            &[],
            EvidenceAssessSnapshot::default()
        ));
    }
}
