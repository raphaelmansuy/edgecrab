//! Tool-turn dispatch trackers and guardrail wiring (spec 015 P2.1 extraction).
//!
//! Groups per-turn harness state that was previously scattered across
//! `conversation.rs` — single owner for failure escalation, dedup, advisories,
//! and Hermes-style tool-loop guardrails.

use crate::config::HarnessConfig;
use crate::harness_advisory::HarnessTurnAdvisory;
use crate::harness_loop_policy::resolve_guardrail_config;
use edgecrab_tools::tool_loop_guardrails::ToolLoopGuardrailController;
use edgecrab_types::Message;

/// Detects duplicate tool+args calls across consecutive turns (FP11).
#[derive(Debug, Default)]
pub struct DuplicateToolCallDetector {
    prev_turn: std::collections::HashMap<(String, u64), String>,
    current_turn: std::collections::HashMap<(String, u64), String>,
}

impl DuplicateToolCallDetector {
    pub fn new() -> Self {
        Self::default()
    }

    fn hash_args(args: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        args.hash(&mut hasher);
        hasher.finish()
    }

    pub fn check_duplicate(&self, name: &str, args: &str) -> Option<&str> {
        let key = (name.to_string(), Self::hash_args(args));
        self.prev_turn.get(&key).map(|s| s.as_str())
    }

    pub fn record(&mut self, name: &str, args: &str, result: &str) {
        let key = (name.to_string(), Self::hash_args(args));
        self.current_turn.insert(key, result.to_string());
    }

    pub fn end_turn(&mut self) {
        std::mem::swap(&mut self.prev_turn, &mut self.current_turn);
        self.current_turn.clear();
    }
}

/// Tracks consecutive tool failures for escalation guidance.
#[derive(Debug)]
pub struct ConsecutiveFailureTracker {
    pub count: u32,
    max_before_escalation: u32,
    last_errors: Vec<String>,
}

impl ConsecutiveFailureTracker {
    pub fn new(max: u32) -> Self {
        Self {
            count: 0,
            max_before_escalation: max,
            last_errors: Vec::new(),
        }
    }

    pub fn record_failure(&mut self, error_summary: &str) -> bool {
        self.count += 1;
        self.last_errors.push(error_summary.to_string());
        if self.last_errors.len() > 5 {
            self.last_errors.remove(0);
        }
        self.count >= self.max_before_escalation
    }

    pub fn record_success(&mut self) {
        self.count = 0;
        self.last_errors.clear();
    }

    pub fn should_escalate(&self) -> bool {
        self.count >= self.max_before_escalation
    }

    pub fn escalation_message(&self) -> String {
        let recent = self
            .last_errors
            .iter()
            .map(|e| format!("  - {e}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "⚠ {count} consecutive tool calls have failed. Recent errors:\n{recent}\n\n\
             Please stop retrying with similar arguments. Instead:\n\
             1. Re-read the error messages carefully.\n\
             2. Consider a completely different approach or tool.\n\
             3. If you are stuck, ask the user for guidance.",
            count = self.count
        )
    }
}

/// Per-turn harness trackers bundled for `process_response` (ISP / SOLID).
#[derive(Debug)]
pub struct TurnDispatchTrackers {
    pub failure: ConsecutiveFailureTracker,
    pub dedup: DuplicateToolCallDetector,
    pub harness_advisory: HarnessTurnAdvisory,
    pub tool_guardrail: ToolLoopGuardrailController,
    /// Set when guardrail halt steer was injected this tool turn (HA-46).
    pub guardrail_halt: bool,
}

impl TurnDispatchTrackers {
    pub fn new(failure_threshold: u32) -> Self {
        Self::with_harness(failure_threshold, &HarnessConfig::default())
    }

    pub fn with_harness(failure_threshold: u32, harness: &HarnessConfig) -> Self {
        Self {
            failure: ConsecutiveFailureTracker::new(failure_threshold),
            dedup: DuplicateToolCallDetector::new(),
            harness_advisory: HarnessTurnAdvisory::new(),
            tool_guardrail: ToolLoopGuardrailController::new(resolve_guardrail_config(harness)),
            guardrail_halt: false,
        }
    }

    pub fn reset_guardrail_turn(&mut self) {
        self.tool_guardrail.reset_for_turn();
    }
}

pub fn apply_guardrail_result(
    guardrail: &mut ToolLoopGuardrailController,
    tool_name: &str,
    args_json: &str,
    tool_result: &str,
    is_error: bool,
) -> String {
    let decision = guardrail.after_call(tool_name, args_json, tool_result, Some(is_error));
    edgecrab_tools::tool_loop_guardrails::append_guardrail_guidance(tool_result, &decision)
}

pub fn guardrail_before_dispatch(
    guardrail: &ToolLoopGuardrailController,
    tool_name: &str,
    args_json: &str,
) -> Option<String> {
    let decision = guardrail.before_call(tool_name, args_json);
    if decision.allows_execution() {
        None
    } else {
        Some(edgecrab_tools::tool_loop_guardrails::guardrail_block_result(&decision))
    }
}

/// Bundled refs for pre-dispatch guardrail checks at tool dispatch sites.
pub struct TurnDispatchTrackersView<'a> {
    pub harness_advisory: &'a HarnessTurnAdvisory,
    pub tool_guardrail: &'a ToolLoopGuardrailController,
}

/// Visual-storm block + tool-loop guardrails before dispatch.
///
/// Thin re-export — ownership lives in [`crate::turn_dispatch_policy`].
#[deprecated(note = "use turn_dispatch_policy::pre_dispatch_decision")]
pub fn guardrail_before_dispatch_checked(
    trackers: &TurnDispatchTrackersView<'_>,
    messages: &[Message],
    tool_name: &str,
    args_json: &str,
) -> Option<String> {
    crate::turn_dispatch_policy::pre_dispatch_decision(
        trackers,
        messages,
        tool_name,
        args_json,
        "",
    )
}

/// Like [`guardrail_before_dispatch_checked`] with session id for port-shopping halt.
///
/// Thin re-export — ownership lives in [`crate::turn_dispatch_policy`].
#[deprecated(note = "use turn_dispatch_policy::pre_dispatch_decision")]
pub fn guardrail_before_dispatch_checked_with_session(
    trackers: &TurnDispatchTrackersView<'_>,
    messages: &[Message],
    tool_name: &str,
    args_json: &str,
    session_id: &str,
) -> Option<String> {
    crate::turn_dispatch_policy::pre_dispatch_decision(
        trackers,
        messages,
        tool_name,
        args_json,
        session_id,
    )
}

/// Post-tool-turn harness finalization (spec 015 P2.1 — single owner for advisories + budget).
pub struct ToolTurnFinalizeParams<'a> {
    pub messages: &'a mut Vec<edgecrab_types::Message>,
    pub tool_turn_start: usize,
    pub tool_names: &'a [&'a str],
    pub browser_navigate_results: &'a [&'a str],
    pub known_dev_ports: &'a [u16],
    pub result_turn_budget_chars: usize,
    pub spill_config: edgecrab_tools::artifact_spill::SpillConfig,
    pub session_id: &'a str,
    pub cwd: &'a std::path::Path,
    pub spill_seq: &'a crate::tool_result_spill::SpillSequence,
    pub max_write_payload_bytes: usize,
    pub provider: Option<&'a dyn edgequake_llm::LLMProvider>,
    pub argument_loop_blocked: bool,
    pub blocked_tool_names: Vec<String>,
}

pub async fn finalize_tool_turn(
    trackers: &mut TurnDispatchTrackers,
    params: ToolTurnFinalizeParams<'_>,
) {
    crate::harness_advisory::apply_harness_advisories(
        &mut trackers.harness_advisory,
        params.messages,
        params.tool_names,
        params.browser_navigate_results,
        params.known_dev_ports,
    );

    if params.result_turn_budget_chars > 0 {
        let spilled = crate::tool_result_spill::enforce_turn_budget(
            &mut params.messages[params.tool_turn_start..],
            params.result_turn_budget_chars,
            &params.spill_config,
            params.session_id,
            params.cwd,
            params.spill_seq,
        );
        if spilled > 0 {
            tracing::info!(
                spilled,
                turn_budget = params.result_turn_budget_chars,
                "per-turn tool result budget enforced"
            );
        }
    }

    if params.argument_loop_blocked {
        let recovery = edgecrab_tools::mutation_turn_policy::continuation_user_message(
            edgecrab_tools::mutation_turn_policy::ContinuationFailureClass::InvalidToolArguments,
            &params.blocked_tool_names,
            params.max_write_payload_bytes,
            params.provider,
        );
        params
            .messages
            .push(edgecrab_types::Message::user(&recovery));
    }

    if let Some(halt) =
        crate::harness_loop_policy::consume_guardrail_halt_message(&mut trackers.tool_guardrail)
    {
        trackers.guardrail_halt = true;
        params.messages.push(edgecrab_types::Message::user(&halt));
    }
}

/// Forward background process watch events to the progress sink (HA-26).
pub fn forward_process_watch_event(
    event: edgecrab_tools::process_table::WatchEvent,
    ev_tx: &tokio::sync::mpsc::UnboundedSender<crate::agent::StreamEvent>,
) {
    use edgecrab_tools::process_table::WatchEventType;
    match event.event_type {
        WatchEventType::TailPreview => {
            let command_preview = crate::safe_truncate(&event.command, 80).to_string();
            crate::progress_sink::emit_optional(
                Some(ev_tx),
                crate::agent::StreamEvent::BackgroundProcessTail {
                    process_id: event.process_id,
                    command_preview,
                    tail: event.matched_output,
                },
            );
        }
        WatchEventType::Exited => {
            crate::progress_sink::emit_optional(
                Some(ev_tx),
                crate::agent::StreamEvent::BackgroundProcessFinished {
                    process_id: event.process_id,
                    exit_code: event.exit_code,
                },
            );
        }
        _ => {
            let notice = edgecrab_tools::process_table::format_watch_activity_notice(&event);
            crate::progress_sink::emit_activity(Some(ev_tx), notice);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HarnessConfig;

    #[test]
    fn consecutive_failure_tracker_escalates_after_threshold() {
        let mut tracker = ConsecutiveFailureTracker::new(3);
        assert!(!tracker.record_failure("err1"));
        assert!(!tracker.record_failure("err2"));
        assert!(tracker.record_failure("err3"));
        assert!(tracker.escalation_message().contains("consecutive"));
    }

    #[test]
    fn consecutive_failure_tracker_resets_on_success() {
        let mut tracker = ConsecutiveFailureTracker::new(3);
        tracker.record_failure("err1");
        tracker.record_failure("err2");
        tracker.record_success();
        assert_eq!(tracker.count, 0);
    }

    #[test]
    fn duplicate_detector_finds_prev_turn_match() {
        let mut tracker = DuplicateToolCallDetector::new();
        tracker.record("read_file", r#"{"path":"a.rs"}"#, "content");
        tracker.end_turn();
        assert!(
            tracker
                .check_duplicate("read_file", r#"{"path":"a.rs"}"#)
                .is_some()
        );
    }

    #[tokio::test]
    async fn ha46_halt_sets_guardrail_halt_flag() {
        let cfg = edgecrab_tools::tool_loop_guardrails::ToolLoopGuardrailConfig {
            hard_stop_enabled: true,
            same_tool_failure_halt_after: 2,
            ..edgecrab_tools::tool_loop_guardrails::ToolLoopGuardrailConfig::default()
        };
        let mut trackers = TurnDispatchTrackers::with_harness(3, &HarnessConfig::default());
        trackers.tool_guardrail =
            edgecrab_tools::tool_loop_guardrails::ToolLoopGuardrailController::new(cfg);
        trackers
            .tool_guardrail
            .after_call("terminal", "{}", "err1", Some(true));
        trackers
            .tool_guardrail
            .after_call("terminal", r#"{"cmd":"ls"}"#, "err2", Some(true));
        let mut messages = Vec::new();
        finalize_tool_turn(
            &mut trackers,
            ToolTurnFinalizeParams {
                messages: &mut messages,
                tool_turn_start: 0,
                tool_names: &["terminal"],
                browser_navigate_results: &[],
                known_dev_ports: &[],
                result_turn_budget_chars: 0,
                spill_config: edgecrab_tools::artifact_spill::SpillConfig::default(),
                session_id: "s",
                cwd: std::path::Path::new("."),
                spill_seq: &crate::tool_result_spill::SpillSequence::new(),
                max_write_payload_bytes: 8000,
                provider: None,
                argument_loop_blocked: false,
                blocked_tool_names: vec![],
            },
        )
        .await;
        assert!(trackers.guardrail_halt);
        assert!(
            messages
                .iter()
                .any(|m| m.text_content().contains("[harness]"))
        );
    }

    #[test]
    fn spill_blind_write_block_before_dispatch() {
        let messages = vec![
            edgecrab_types::Message::user("read big file"),
            edgecrab_types::Message::tool_result(
                "r1",
                "read_file",
                "[tool_result_spill] artifact=.edgecrab/artifacts/s1/read_001.md next_read=read_file",
            ),
        ];
        let advisory = crate::harness_advisory::HarnessTurnAdvisory::new();
        let guardrail = edgecrab_tools::tool_loop_guardrails::ToolLoopGuardrailController::new(
            edgecrab_tools::tool_loop_guardrails::ToolLoopGuardrailConfig::default(),
        );
        let trackers = TurnDispatchTrackersView {
            harness_advisory: &advisory,
            tool_guardrail: &guardrail,
        };
        #[allow(deprecated)]
        let blocked = guardrail_before_dispatch_checked(
            &trackers,
            &messages,
            "write_file",
            r#"{"path":"out.rs","content":"x"}"#,
        );
        assert!(
            blocked
                .as_deref()
                .is_some_and(|b| b.contains("spill_blind") || b.contains("spilled")),
            "got {blocked:?}"
        );
    }

    #[test]
    fn ha26_forward_process_watch_emits_activity_notice() {
        use edgecrab_tools::process_table::{WatchEvent, WatchEventType};
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        forward_process_watch_event(
            WatchEvent {
                process_id: "bg-1".into(),
                command: "python3 -m http.server 8000".into(),
                pattern: "Serving HTTP".into(),
                matched_output: "Serving HTTP on :: port 8000".into(),
                suppressed_count: 0,
                event_type: WatchEventType::Match,
                exit_code: None,
            },
            &tx,
        );
        let event = rx.try_recv().expect("activity notice");
        assert!(matches!(
            event,
            crate::agent::StreamEvent::ActivityNotice(_)
        ));
    }
}
