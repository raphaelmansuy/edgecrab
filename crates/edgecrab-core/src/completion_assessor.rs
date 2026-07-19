use edgecrab_tools::HarnessSnapshot;
use edgecrab_types::{
    CompletionDecision, ExitReason, Message, ReportedTaskStatus, Role, RunOutcome, TaskStatusKind,
    VerificationSummary, parse_tool_error_payload,
};

use crate::evidence_latch::EvidenceAssessSnapshot;
use crate::task_class::{
    TaskClass, classify_from_messages, document_artifact_evidence_present,
    is_verification_tool_for_class,
};

/// Snapshot of end-of-run state inspected by the completion policy.
pub struct CompletionContext<'a> {
    pub final_response: &'a str,
    pub messages: &'a [Message],
    pub interrupted: bool,
    pub budget_exhausted: bool,
    /// Model exhausted unknown-tool retry budget (PartialAbort).
    pub invalid_tool_budget_exhausted: bool,
    pub pending_approval: bool,
    pub pending_clarification: bool,
    pub active_todos: usize,
    pub blocked_todos: usize,
    pub child_runs_in_flight: usize,
    /// Deterministic harness gates (mutation debt, oracles, structured tool errors).
    pub harness: HarnessSnapshot,
    /// When true, visual tasks require preview evidence before `Completed`.
    pub verification_strict: bool,
    /// Evidence latch assess snapshot (022 — visual done / escalated / heal).
    pub evidence: EvidenceAssessSnapshot,
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
        // First principles: approval is a typed session flag only — never prose bags.
        let pending_approval = ctx.pending_approval;
        let task_class = classify_from_messages(ctx.messages);
        let mut verification =
            collect_verification_summary(ctx.messages, task_class, ctx.verification_strict);
        // 022: latch graph is authoritative for visual/media evidence.
        if ctx.evidence.visual_complete || ctx.evidence.media_complete {
            verification.required = true;
            verification.evidence_present = true;
            verification.debt_reason = None;
            if verification.evidence.is_empty() {
                verification.evidence.push(if ctx.evidence.visual_complete {
                    "evidence_latch: visual_evidence_complete".into()
                } else {
                    "evidence_latch: media_evidence_complete".into()
                });
            }
        }
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
        let harness_blocked = ctx.harness.blocks_completion();
        // 022 B3: sticky in_progress / todos ignored when latched complete or escalated.
        let ledger_complete_override =
            ctx.evidence.visual_complete || ctx.evidence.media_complete || ctx.evidence.escalated;
        let sticky_incomplete = !ledger_complete_override
            && (ctx.child_runs_in_flight > 0
                || ctx.active_todos > 0
                || reported_in_progress
                || has_remaining_steps);

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
        } else if ctx.invalid_tool_budget_exhausted {
            let summary = if ctx.final_response.trim().is_empty() {
                "Failed — invalid tool call retry budget exhausted.".to_string()
            } else {
                ctx.final_response.trim().to_string()
            };
            RunOutcome::new(
                CompletionDecision::Failed,
                ExitReason::InvalidToolBudget,
                summary,
            )
        } else if ctx.evidence.escalated || ctx.evidence.verify_budget_exhausted {
            // 022 B2/B4: closed action set — terminal outcome, not Incomplete reopen.
            let summary = if ctx.final_response.trim().is_empty() {
                "Stopped — verification escalated (preview/heal exhausted or verify budget). \
                 Deliverables may exist on disk; browser evidence could not be completed."
                    .to_string()
            } else {
                ctx.final_response.trim().to_string()
            };
            RunOutcome::new(
                CompletionDecision::Failed,
                ExitReason::GuardrailHalt,
                summary,
            )
        } else if ctx.evidence.visual_complete || ctx.evidence.media_complete {
            // 022 B1: latch done → Completed (ignore sticky ledger).
            let summary = if ctx.final_response.trim().is_empty() {
                "Completed — request satisfied and verified (evidence latch)."
                    .to_string()
            } else {
                ctx.final_response.trim().to_string()
            };
            RunOutcome::new(
                CompletionDecision::Completed,
                ExitReason::ModelReturnedFinalText,
                summary,
            )
        } else if sticky_incomplete {
            RunOutcome::new(
                CompletionDecision::Incomplete,
                ExitReason::PendingTasks,
                "Incomplete — progress was reported but work still remains.",
            )
        } else if harness_blocked {
            let reason = ctx
                .harness
                .completion_block_reason()
                .unwrap_or_else(|| "Incomplete — deterministic harness gates did not pass.".into());
            let exit = if ctx.harness.guardrail_halt {
                ExitReason::GuardrailHalt
            } else {
                ExitReason::NoMoreToolCalls
            };
            RunOutcome::new(CompletionDecision::Incomplete, exit, reason)
        } else if has_recent_critical_tool_failure(ctx.messages) {
            RunOutcome::new(
                CompletionDecision::Incomplete,
                ExitReason::NoMoreToolCalls,
                "Incomplete — a required tool failed; the task was not fully satisfied.",
            )
        } else if ctx.final_response.trim().is_empty() {
            RunOutcome::new(
                CompletionDecision::Failed,
                ExitReason::NoMoreToolCalls,
                "Failed — the run ended without a usable final response.",
            )
        } else if ctx.verification_strict
            && task_class == TaskClass::VisualUx
            && visual_browser_navigate_exhausted(ctx.messages)
        {
            RunOutcome::new(
                CompletionDecision::NeedsVerification,
                ExitReason::VerificationPending,
                "Needs verification — browser navigation failed repeatedly with no successful page load. \
                 Enable security.preview for localhost URLs, then browser_navigate + browser_snapshot.",
            )
        } else if ctx.verification_strict
            && task_class == TaskClass::VisualUx
            && markdown_theater_without_perception(ctx.messages)
        {
            RunOutcome::new(
                CompletionDecision::NeedsVerification,
                ExitReason::VerificationPending,
                "Needs verification — markdown report files are not browser/screenshot evidence.",
            )
        } else if ctx.verification_strict
            && task_class == TaskClass::VisualUx
            && verification.required
            && !verification.evidence_present
        {
            RunOutcome::new(
                CompletionDecision::NeedsVerification,
                ExitReason::VerificationPending,
                "Needs verification — visual/UX tasks require browser or screenshot evidence.",
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

/// Recent structured failure on tools that gate user-facing answers.
fn has_recent_critical_tool_failure(messages: &[Message]) -> bool {
    const CRITICAL: &[&str] = &["web_search", "web_extract", "web_crawl"];
    messages.iter().rev().take(12).any(|msg| {
        msg.role == Role::Tool
            && msg.name.as_deref().is_some_and(|n| CRITICAL.contains(&n))
            && parse_tool_error_payload(&msg.text_content()).is_some()
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

fn collect_verification_summary(
    messages: &[Message],
    task_class: TaskClass,
    verification_strict: bool,
) -> VerificationSummary {
    let mut required = false;
    let mut evidence = Vec::new();
    let mut debt_reason: Option<String> = None;

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
                // Summary prose alone never counts as evidence (018 F1 / H7).
                evidence.extend(
                    report
                        .evidence
                        .into_iter()
                        .filter(|item| !item.trim().is_empty()),
                );
            }
            continue;
        }

        if !is_verification_tool_for_class(name, task_class) {
            continue;
        }

        required = true;
        if parse_tool_error_payload(&content).is_some() {
            continue;
        }

        if is_mutation_verification_tool(name)
            && !edgecrab_tools::file_mutation_result_landed(name, &content)
        {
            continue;
        }

        // Structured terminal proof: only exit_code==0 counts as free-form evidence.
        if matches!(name, "terminal" | "run_process") {
            match edgecrab_tools::parse_terminal_result(&content) {
                Some(parsed) if parsed.exit_code == 0 => {}
                _ => continue,
            }
        }

        // VisualUx: typed StructuredBrowserResult only (no prose heuristics).
        if task_class == TaskClass::VisualUx && !visual_perception_evidence_ok(name, &content) {
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

    if task_class == TaskClass::VisualUx && verification_strict {
        required = true;
    }

    // Document: landed office artifact is sufficient evidence (006 pptx forensics).
    if task_class == TaskClass::Document && document_artifact_evidence_present(messages) {
        required = true;
        if evidence.is_empty() {
            evidence.push("document: artifact path present".into());
        }
    }

    // MediaRender: non-zero media file is sufficient (019) — no browser thrash.
    if task_class == TaskClass::MediaRender
        && crate::task_class::media_artifact_evidence_present(messages)
    {
        required = true;
        if evidence.is_empty() {
            evidence.push("media: render output path present".into());
        }
    }

    if task_class == TaskClass::VisualUx && verification_strict && required && evidence.is_empty() {
        debt_reason = Some(
            "Visual/UX task: enable security.preview and verify with browser or screenshot."
                .to_string(),
        );
    }

    if task_class == TaskClass::Document && required && evidence.is_empty() {
        debt_reason = Some(
            "Document task: confirm .pptx/.pdf/.docx exists with non-zero size.".to_string(),
        );
    }

    VerificationSummary {
        required,
        evidence_present: !evidence.is_empty(),
        debt_reason: debt_reason.or((required && evidence.is_empty())
            .then_some("No structured verification evidence was recorded.".to_string())),
        evidence,
        // Contract fields filled by [`enrich_verification_with_contract`] when a goal is active.
        contract_required: false,
        contract_satisfied: false,
    }
}

/// Fold goal-contract requirements into the single evidence ledger (DRY — no parallel type).
pub fn enrich_verification_with_contract(
    mut summary: VerificationSummary,
    contract: &edgecrab_types::GoalContract,
    messages: &[Message],
) -> VerificationSummary {
    if !contract.requires_verification_evidence() {
        return summary;
    }
    summary.contract_required = true;
    summary.required = true;
    let satisfied = crate::goal_judge::contract_evidence_in_messages(contract, messages);
    summary.contract_satisfied = satisfied;
    if !satisfied {
        summary.evidence_present = false;
        summary.debt_reason = Some(format!(
            "Goal contract verification unmet — need tool evidence for: {}",
            contract.verification.trim()
        ));
    } else if !summary.evidence.iter().any(|e| {
        e.to_ascii_lowercase()
            .contains(&contract.verification.trim().to_ascii_lowercase())
    }) {
        summary.evidence.push(format!(
            "contract:{}",
            contract.verification.trim()
        ));
        summary.evidence_present = true;
    }
    summary
}

fn is_mutation_verification_tool(name: &str) -> bool {
    matches!(name, "write_file" | "patch" | "apply_patch")
}

/// True when perception tool content is typed evidence (018 P6 first principles).
///
/// Law: only [`StructuredBrowserResult`] fields count — never prose heuristics
/// (loader/spinner/beautiful/ERR_* substrings). Unstructured results are not evidence.
pub fn visual_perception_evidence_ok(_tool_name: &str, content: &str) -> bool {
    let Some(parsed) = edgecrab_tools::parse_structured_browser_result(content) else {
        return false;
    };
    // 019: ContentClass must be Ok — HttpErrorPage / ChromeError never count.
    if !parsed.content_class.is_evidence() || parsed.is_chrome_error || !parsed.ok {
        return false;
    }
    match parsed.tool.as_str() {
        "browser_snapshot" => parsed.node_count.unwrap_or(0) > 0,
        "browser_vision" | "browser_navigate" => true,
        // Other perception tools must still emit the structured envelope.
        _ => true,
    }
}

/// Visual-UX sessions with repeated `browser_navigate` failures and zero successful loads (games003).
fn visual_browser_navigate_exhausted(messages: &[Message]) -> bool {
    let mut failed = 0usize;
    let mut ok = 0usize;
    for msg in messages {
        if msg.role != Role::Tool || msg.name.as_deref() != Some("browser_navigate") {
            continue;
        }
        let content = msg.text_content();
        match edgecrab_tools::structured_browser_nav_succeeded(&content) {
            Some(true) => ok += 1,
            Some(false) => failed += 1,
            None => {
                if parse_tool_error_payload(&content).is_some() {
                    failed += 1;
                }
                // Unstructured navigate prose does not count as success.
            }
        }
    }
    failed >= 3 && ok == 0
}

/// Visual-UX sessions that wrote multiple report-like markdown files without perception (HA-43).
fn markdown_theater_without_perception(messages: &[Message]) -> bool {
    let mut report_writes = 0usize;
    let mut perception_ok = false;
    for msg in messages {
        if msg.role != Role::Tool {
            continue;
        }
        let Some(name) = msg.name.as_deref() else {
            continue;
        };
        if matches!(
            name,
            "browser_snapshot"
                | "browser_vision"
                | "vision"
                | "analyze_image"
                | "vision_analyze"
                | "capture_screenshot"
        ) && parse_tool_error_payload(&msg.text_content()).is_none()
            && visual_perception_evidence_ok(name, &msg.text_content())
        {
            perception_ok = true;
        }
        if name != "write_file" {
            continue;
        }
        let content = msg.text_content();
        if parse_tool_error_payload(&content).is_some() {
            continue;
        }
        if let Some(path) = write_file_path_from_tool_result(&content)
            && is_verify_theater_basename(&path)
        {
            report_writes += 1;
        }
    }
    report_writes >= 3 && !perception_ok
}

/// Typed `path` from write_file tool result JSON.
fn write_file_path_from_tool_result(tool_result: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(tool_result)
        .ok()
        .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(str::to_string))
}

/// Theater filenames — basename stem only (never scan file body prose).
pub(crate) fn is_verify_theater_basename(path: &str) -> bool {
    let name = std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "verification.md"
            | "delivery.md"
            | "checklist.md"
            | "evidence.md"
            | "readme.md"
            | "report.txt"
            | "report.md"
            | "final_report.md"
            | "final_report.txt"
    ) || name.ends_with("_report.md")
        || name.ends_with("_report.txt")
        || (name.starts_with("final_report") && (name.ends_with(".md") || name.ends_with(".txt")))
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
    use edgecrab_tools::{
        HarnessAdvisorySignals, HarnessBuildInput, MutationTurnState, build_harness_snapshot,
    };
    use edgecrab_types::Message;
    use std::fs;
    use tempfile::TempDir;

    fn base_ctx<'a>(
        final_response: &'a str,
        messages: &'a [Message],
        harness: HarnessSnapshot,
    ) -> CompletionContext<'a> {
        CompletionContext {
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
            verification_strict: false,
            evidence: Default::default(),        }
    }

    #[test]
    fn failed_web_search_is_not_reported_complete() {
        let err = serde_json::json!({
            "type": "tool_error",
            "category": "execution",
            "code": "execution_failed",
            "code_num": 1006,
            "error": "Web search via ddgs failed: bot-challenge",
            "retryable": true,
            "suppress_retry": false
        })
        .to_string();
        let messages = vec![Message::tool_result("tc_1", "web_search", &err)];
        let outcome = assess_completion(&base_ctx(
            "I'm sorry, I cannot provide that information.",
            &messages,
            HarnessSnapshot::default(),
        ));
        assert_eq!(outcome.state, CompletionDecision::Incomplete);
        assert!(!outcome.is_success());
    }

    #[test]
    fn budget_exhausted_is_never_reported_complete() {
        let ctx = CompletionContext {
            final_response: "I ran out of budget.",
            messages: &[],
            interrupted: false,
            budget_exhausted: true,
            invalid_tool_budget_exhausted: false,
            pending_approval: false,
            pending_clarification: false,
            active_todos: 0,
            blocked_todos: 0,
            child_runs_in_flight: 0,
            harness: HarnessSnapshot::default(),
            verification_strict: false,
            evidence: Default::default(),        };

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
            invalid_tool_budget_exhausted: false,
            pending_approval: false,
            pending_clarification: false,
            active_todos: 2,
            blocked_todos: 0,
            child_runs_in_flight: 0,
            harness: HarnessSnapshot::default(),
            verification_strict: false,
            evidence: Default::default(),        };

        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::Incomplete);
    }

    #[test]
    fn clarify_marker_maps_to_needs_user_input() {
        let msg = Message::assistant("[CLARIFY] Which file should I edit?");
        let messages = vec![msg];
        let outcome = assess_completion(&base_ctx(
            "[CLARIFY] Which file should I edit?",
            &messages,
            HarnessSnapshot::default(),
        ));
        assert_eq!(outcome.state, CompletionDecision::NeedsUserInput);
    }

    #[test]
    fn blocked_todos_map_to_blocked() {
        let ctx = CompletionContext {
            final_response: "Need approval.",
            messages: &[],
            interrupted: false,
            budget_exhausted: false,
            invalid_tool_budget_exhausted: false,
            pending_approval: false,
            pending_clarification: false,
            active_todos: 0,
            blocked_todos: 1,
            child_runs_in_flight: 0,
            harness: HarnessSnapshot::default(),
            verification_strict: false,
            evidence: Default::default(),        };

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
            invalid_tool_budget_exhausted: false,
            pending_approval: true,
            pending_clarification: false,
            active_todos: 0,
            blocked_todos: 0,
            child_runs_in_flight: 0,
            harness: HarnessSnapshot::default(),
            verification_strict: false,
            evidence: Default::default(),        };

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
            invalid_tool_budget_exhausted: false,
            pending_approval: false,
            pending_clarification: true,
            active_todos: 0,
            blocked_todos: 0,
            child_runs_in_flight: 0,
            harness: HarnessSnapshot::default(),
            verification_strict: false,
            evidence: Default::default(),        };

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
        let outcome =
            assess_completion(&base_ctx("All set.", &messages, HarnessSnapshot::default()));
        assert_eq!(outcome.state, CompletionDecision::Completed);
        assert!(outcome.verification.evidence_present);
    }

    #[test]
    fn report_task_status_summary_alone_is_not_evidence() {
        let report = serde_json::json!({
            "status": "completed",
            "summary": "everything looks great",
            "evidence": [],
            "remaining_steps": []
        })
        .to_string();
        let messages = vec![
            Message::user("make demo/games003 polished UX"),
            Message::tool_result("tc_1", "report_task_status", &report),
        ];
        let summary = collect_verification_summary(&messages, TaskClass::VisualUx, true);
        assert!(
            !summary.evidence_present,
            "summary prose must not populate evidence: {:?}",
            summary.evidence
        );
    }

    #[test]
    fn approval_prose_alone_does_not_block() {
        let outcome = assess_completion(&base_ctx(
            "Approval required — reply /approve to continue.",
            &[],
            HarnessSnapshot::default(),
        ));
        assert_ne!(outcome.state, CompletionDecision::Blocked);
        assert_ne!(outcome.exit_reason, ExitReason::AwaitingApproval);
    }

    #[test]
    fn failed_terminal_does_not_count_as_verification_evidence() {
        let messages = vec![
            Message::user("run cargo test"),
            Message::tool_result(
                "t1",
                "terminal",
                "[terminal_result status=error backend=local cwd=/tmp exit_code=1]\n\
                 cargo test\nFAILED",
            ),
        ];
        let summary = collect_verification_summary(&messages, TaskClass::CodeChange, false);
        assert!(summary.required);
        assert!(
            !summary.evidence_present,
            "failed terminal must not populate evidence: {:?}",
            summary.evidence
        );
    }

    #[test]
    fn successful_terminal_counts_as_verification_evidence() {
        let messages = vec![Message::tool_result(
            "t1",
            "terminal",
            "[terminal_result status=success backend=local cwd=/tmp exit_code=0]\n\
             cargo test\nok",
        )];
        let summary = collect_verification_summary(&messages, TaskClass::CodeChange, false);
        assert!(summary.evidence_present);
    }

    #[test]
    fn deferred_work_prose_no_longer_blocks_completion() {
        let messages = vec![Message::tool_result(
            "tc_1",
            "write_file",
            r#"{"ok":true,"bytes":12,"lines":1,"path":"game2.txt"}"#,
        )];
        let outcome = assess_completion(&base_ctx(
            "I see the issue. Let me try writing the file directly without creating directories first.",
            &messages,
            HarnessSnapshot::default(),
        ));
        assert_eq!(outcome.state, CompletionDecision::Completed);
    }

    #[test]
    fn final_answer_after_tool_activity_can_still_complete() {
        let messages = vec![Message::tool_result(
            "tc_1",
            "write_file",
            r#"{"ok":true,"bytes":12,"lines":1,"path":"./game2/index.html"}"#,
        )];
        let outcome = assess_completion(&base_ctx(
            "The file is in place and the task is complete.",
            &messages,
            HarnessSnapshot::default(),
        ));
        assert_eq!(outcome.state, CompletionDecision::Completed);
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
        let outcome = assess_completion(&base_ctx(
            "Almost done.",
            &messages,
            HarnessSnapshot::default(),
        ));
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
        let outcome = assess_completion(&base_ctx("Done.", &messages, HarnessSnapshot::default()));
        assert_eq!(outcome.state, CompletionDecision::Incomplete);
    }

    #[test]
    fn harness_oracle_failure_blocks_completion() {
        let dir = TempDir::new().expect("tempdir");
        fs::write(dir.path().join("bad.js"), "const x = ;\n").expect("write");
        let turn = MutationTurnState::new();
        turn.push_success(edgecrab_tools::MutationRecord {
            path: "bad.js".into(),
            kind: edgecrab_tools::MutationKind::Modify,
            lines_added: 1,
            lines_removed: 0,
        });
        let harness = build_harness_snapshot(HarnessBuildInput {
            messages: &[],
            mutation_turn: &turn,
            cwd: dir.path(),
            post_mutation_oracles: true,
            advisory: HarnessAdvisorySignals::default(),
            unanswered_tool_calls: 0,
        });
        let ok = serde_json::json!({"ok": true, "replacements": 1}).to_string();
        let messages = vec![Message::tool_result("t1", "patch", &ok)];
        let outcome = assess_completion(&base_ctx("All done!", &messages, harness));
        assert_eq!(outcome.state, CompletionDecision::Incomplete);
        assert!(outcome.user_summary.contains("post-mutation gate failed"));
    }

    #[test]
    fn ha30_strict_visual_without_preview_evidence_needs_verification() {
        let messages = vec![
            Message::user("make demo/games003 UI more polished"),
            Message::tool_result("t1", "terminal", "Syntax OK"),
        ];
        let ctx = CompletionContext {
            final_response: "Looks great!",
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
            evidence: Default::default(),        };
        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::NeedsVerification);
    }

    #[test]
    fn ha30_browser_navigate_failures_block_visual_completion() {
        let err = serde_json::json!({
            "type": "tool_error",
            "category": "security",
            "code": "ssrf_blocked",
            "code_num": 1005,
            "error": "SSRF blocked",
            "retryable": false,
            "suppress_retry": true
        })
        .to_string();
        let messages = vec![
            Message::user("make demo/games003/index.html beautiful UX"),
            Message::tool_result("n1", "browser_navigate", &err),
            Message::tool_result("n2", "browser_navigate", &err),
            Message::tool_result("n3", "browser_navigate", &err),
        ];
        let ctx = CompletionContext {
            final_response: "Done — game looks great!",
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
            evidence: Default::default(),        };
        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::NeedsVerification);
        assert!(outcome.user_summary.contains("browser navigation"));
    }

    #[test]
    fn ha43_markdown_theater_blocks_visual_completion() {
        let messages = vec![
            Message::user("create beautiful 3D race game demo/race_gamey/index.html"),
            Message::tool_result(
                "w0",
                "write_file",
                r#"{"ok":true,"path":"demo/race_gamey/index.html"}"#,
            ),
            Message::tool_result("w1", "write_file", r#"{"path":"VERIFICATION.md"}"#),
            Message::tool_result("w2", "write_file", r#"{"path":"DELIVERY.md"}"#),
            Message::tool_result("w3", "write_file", r#"{"path":"FINAL_REPORT.txt"}"#),
        ];
        let ctx = CompletionContext {
            final_response: "Game delivered with full verification docs.",
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
            evidence: Default::default(),        };
        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::NeedsVerification);
        assert!(outcome.user_summary.contains("markdown"));
    }

    #[test]
    fn invalid_tool_budget_exhausted_is_failed_not_completed() {
        let outcome = assess_completion(&CompletionContext {
            final_response: "Model generated invalid tool call: quick_stock_quote",
            messages: &[],
            interrupted: false,
            budget_exhausted: false,
            invalid_tool_budget_exhausted: true,
            pending_approval: false,
            pending_clarification: false,
            active_todos: 0,
            blocked_todos: 0,
            child_runs_in_flight: 0,
            harness: HarnessSnapshot::default(),
            verification_strict: false,
            evidence: Default::default(),        });
        assert_eq!(outcome.state, CompletionDecision::Failed);
        assert_eq!(outcome.exit_reason, ExitReason::InvalidToolBudget);
        assert!(outcome.user_summary.contains("quick_stock_quote"));
        assert!(!outcome.is_success());
    }

    #[test]
    fn closed_unknown_tool_results_do_not_trip_unanswered_gate() {
        // Structural stand-in for strike-3 PartialAbort after tool-call closure:
        // assistant tool_call + matching tool result → unanswered == 0.
        let messages = vec![
            Message::assistant_with_tool_calls(
                "",
                vec![edgecrab_types::ToolCall {
                    id: "c1".into(),
                    r#type: "function".into(),
                    function: edgecrab_types::FunctionCall {
                        name: "quick_stock_quote".into(),
                        arguments: r#"{"symbol":"MSFT"}"#.into(),
                    },
                    thought_signature: None,
                }],
            ),
            Message::tool_result(
                "c1",
                "quick_stock_quote",
                r#"{"type":"tool_error","code":"tool_not_found","error":"Tool 'quick_stock_quote' does not exist."}"#,
            ),
        ];
        assert_eq!(
            crate::turn_completion::count_unanswered_tool_calls(&messages),
            0
        );
        let harness = build_harness_snapshot(HarnessBuildInput {
            messages: &messages,
            mutation_turn: &MutationTurnState::new(),
            cwd: std::path::Path::new("."),
            post_mutation_oracles: false,
            advisory: HarnessAdvisorySignals::default(),
            unanswered_tool_calls: 0,
        });
        assert!(!harness.blocks_completion());
        let outcome = assess_completion(&CompletionContext {
            final_response: "Model generated invalid tool call: quick_stock_quote",
            messages: &messages,
            interrupted: false,
            budget_exhausted: false,
            invalid_tool_budget_exhausted: true,
            pending_approval: false,
            pending_clarification: false,
            active_todos: 0,
            blocked_todos: 0,
            child_runs_in_flight: 0,
            harness,
            verification_strict: false,
            evidence: Default::default(),        });
        assert_eq!(outcome.state, CompletionDecision::Failed);
        assert_eq!(outcome.exit_reason, ExitReason::InvalidToolBudget);
    }

    #[test]
    fn ha51_unanswered_tool_calls_incomplete() {
        let messages = vec![Message::assistant_with_tool_calls(
            "",
            vec![edgecrab_types::ToolCall {
                id: "t1".into(),
                r#type: "function".into(),
                function: edgecrab_types::FunctionCall {
                    name: "terminal".into(),
                    arguments: "{}".into(),
                },
                thought_signature: None,
            }],
        )];
        let harness = build_harness_snapshot(HarnessBuildInput {
            messages: &messages,
            mutation_turn: &MutationTurnState::new(),
            cwd: std::path::Path::new("."),
            post_mutation_oracles: false,
            advisory: HarnessAdvisorySignals::default(),
            unanswered_tool_calls: 1,
        });
        let outcome = assess_completion(&base_ctx("Done.", &messages, harness));
        assert_eq!(outcome.state, CompletionDecision::Incomplete);
    }

    #[test]
    fn terminal_patch_error_blocks_even_with_cheerful_final_text() {
        let err = serde_json::json!({
            "type": "tool_error",
            "category": "arguments",
            "code": "invalid_arguments",
            "code_num": 1002,
            "error": "Invalid arguments for patch",
            "retryable": false,
            "suppress_retry": true
        })
        .to_string();
        let messages = vec![Message::tool_result("t1", "patch", &err)];
        let harness = build_harness_snapshot(HarnessBuildInput {
            messages: &messages,
            mutation_turn: &MutationTurnState::new(),
            cwd: std::path::Path::new("."),
            post_mutation_oracles: false,
            advisory: HarnessAdvisorySignals::default(),
            unanswered_tool_calls: 0,
        });
        let outcome = assess_completion(&base_ctx("Done!", &messages, harness));
        assert_eq!(outcome.state, CompletionDecision::Incomplete);
    }

    #[test]
    fn navigate_alone_does_not_satisfy_visual_ux() {
        let messages = vec![
            Message::user("Write a beautiful 3D Chess UX in ./demo/game005/index.html"),
            Message::tool_result(
                "n1",
                "browser_navigate",
                "Navigated to: http://127.0.0.1:8000/index.html\nTitle: 3D Chess",
            ),
        ];
        let ctx = CompletionContext {
            final_response: "Done — WebGL unsupported in preview.",
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
            evidence: Default::default(),        };
        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::NeedsVerification);
    }

    #[test]
    fn chrome_error_not_visual_evidence() {
        let chrome = edgecrab_tools::StructuredBrowserResult::vision_ok(
            "chrome-error://chromewebdata/",
            "looks fine",
            true,
        )
        .to_tool_result_text();
        assert!(!visual_perception_evidence_ok("browser_vision", &chrome));

        // Unstructured prose is never evidence (no loader-keyword law).
        assert!(!visual_perception_evidence_ok(
            "browser_vision",
            "The page is not currently displaying a game; blank canvas only."
        ));
        assert!(!visual_perception_evidence_ok(
            "browser_snapshot",
            "Board with 32 chess pieces visible; white to move."
        ));

        let good = edgecrab_tools::StructuredBrowserResult::snapshot_ok(
            "http://127.0.0.1:8000/",
            "Board with 32 chess pieces; white to move. @e1 @e2",
            Some(8),
        )
        .to_tool_result_text();
        assert!(visual_perception_evidence_ok("browser_snapshot", &good));
    }

    #[test]
    fn visual_perception_snapshot_satisfies_completion() {
        let snap = edgecrab_tools::StructuredBrowserResult::snapshot_ok(
            "http://127.0.0.1:8000/",
            "Chess board rendered with pieces; UI overlay visible. @e1 button @e2 canvas",
            Some(12),
        )
        .to_tool_result_text();
        let messages = vec![
            Message::user("make demo/game005/index.html beautiful UX"),
            Message::tool_result("s1", "browser_snapshot", &snap),
        ];
        let ctx = CompletionContext {
            final_response: "Game looks good in browser.",
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
            evidence: Default::default(),        };
        let outcome = assess_completion(&ctx);
        assert_eq!(outcome.state, CompletionDecision::Completed);
    }
}
