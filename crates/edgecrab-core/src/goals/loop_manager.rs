//! Ralph-loop orchestration — judge + auto-continuation decisions.

use std::sync::Arc;

use edgecrab_types::AgentError;
use edgequake_llm::LLMProvider;

use crate::config::{GoalJudgeConfig, GoalsConfig};
use crate::goal_judge::{
    DEFAULT_MAX_CONSECUTIVE_PARSE_FAILURES, resolve_goal_judge_provider_and_model, run_goal_judge,
};
use crate::goals::{GoalState, GoalStatus, GoalStore};

pub const CONTINUATION_PROMPT_TEMPLATE: &str = "\
[Continuing toward your standing goal]\n\
Goal: {goal}\n\
{contract_line}\n\
Continue working toward this goal. Take the next concrete step. \
If you believe the goal is complete, state so explicitly and stop. \
If you are blocked and need input from the user, say so clearly and stop.";

pub const CONTINUATION_PROMPT_WITH_SUBGOALS_TEMPLATE: &str = "\
[Continuing toward your standing goal]\n\
Goal: {goal}\n\
{contract_line}\n\
Additional criteria the user added mid-loop:\n\
{subgoals_block}\n\n\
Continue working toward the goal AND all additional criteria. Take \
the next concrete step. If you believe the goal and every \
additional criterion are complete, state so explicitly and stop. \
If you are blocked and need input from the user, say so clearly \
and stop.";

/// Prefix shared by synthetic Ralph-loop continuation user messages.
pub const GOAL_CONTINUATION_PREFIX: &str = "[Continuing toward your standing goal]\nGoal:";

/// Return true for synthetic Ralph-loop continuation prompts.
pub fn is_goal_continuation_text(text: &str) -> bool {
    text.starts_with(GOAL_CONTINUATION_PREFIX)
}

/// Return true if the text looks like a slash command.
pub fn looks_like_slash_command(text: &str) -> bool {
    text.trim_start().starts_with('/')
}

/// True when the queue contains a non-slash user payload (Hermes queue peek).
pub fn prompt_queue_has_real_user_message(queue: &[String]) -> bool {
    queue
        .iter()
        .any(|item| !looks_like_slash_command(item) && !is_goal_continuation_text(item))
}

/// Remove synthetic goal continuations from a FIFO prompt queue.
pub fn drain_goal_continuations_from_queue(queue: &mut Vec<String>) -> usize {
    let before = queue.len();
    queue.retain(|item| !is_goal_continuation_text(item));
    before - queue.len()
}

/// Compact status-bar chip for the active Ralph loop (TUI / gateway surfaces).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalStatusChip {
    pub label: String,
    pub status: GoalStatus,
}

/// Build a one-line status chip from persisted goal state.
pub fn compact_status_chip(state: &GoalState) -> Option<GoalStatusChip> {
    if state.is_empty() || state.status == GoalStatus::Cleared {
        return None;
    }
    let goal = state
        .goal_text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())?;
    let short = crate::safe_truncate(goal, 22);
    let turns = format!("{}/{}", state.turns_used, state.max_turns);
    let sub = if state.subgoals.is_empty() {
        String::new()
    } else {
        format!(" · {} sub", state.subgoals.len())
    };

    let (label, status) = match state.status {
        GoalStatus::Active => (format!("⊙ {turns}{sub} · {short}"), GoalStatus::Active),
        GoalStatus::Paused => (format!("⏸ {turns}{sub} · {short}"), GoalStatus::Paused),
        GoalStatus::Done => (format!("✓ goal done · {short}"), GoalStatus::Done),
        GoalStatus::Cleared => return None,
    };
    Some(GoalStatusChip { label, status })
}

/// Map a post-turn judge decision to a short flash string for the status bar.
pub fn goal_flash_from_decision(decision: &GoalContinuationDecision) -> Option<String> {
    if decision.verdict == "done" || decision.status == GoalStatus::Done {
        return Some("✓ goal complete".into());
    }
    if decision.status == GoalStatus::Paused {
        return Some("⏸ goal paused".into());
    }
    if decision.should_continue {
        return Some("↻ goal continuing".into());
    }
    None
}

/// Decision returned after a turn completes and the goal judge runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalContinuationDecision {
    pub should_continue: bool,
    pub continuation_prompt: Option<String>,
    pub message: String,
    pub verdict: String,
    pub status: GoalStatus,
}

impl GoalContinuationDecision {
    pub fn inactive() -> Self {
        Self {
            should_continue: false,
            continuation_prompt: None,
            message: String::new(),
            verdict: "inactive".into(),
            status: GoalStatus::Cleared,
        }
    }
}

pub fn status_line(state: &GoalState) -> String {
    let Some(goal) = state
        .goal_text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    else {
        return "No active goal. Set one with /goal <text>.".into();
    };

    let turns = format!("{}/{} turns", state.turns_used, state.max_turns);
    let sub = if state.subgoals.is_empty() {
        String::new()
    } else {
        format!(
            ", {} subgoal{}",
            state.subgoals.len(),
            if state.subgoals.len() == 1 { "" } else { "s" }
        )
    };

    match state.status {
        GoalStatus::Active => format!("⊙ Goal (active, {turns}{sub}): {goal}"),
        GoalStatus::Paused => {
            let extra = state
                .paused_reason
                .as_deref()
                .filter(|r| !r.is_empty())
                .map(|r| format!(" — {r}"))
                .unwrap_or_default();
            format!("⏸ Goal (paused, {turns}{sub}{extra}): {goal}")
        }
        GoalStatus::Done => format!("✓ Goal done ({turns}{sub}): {goal}"),
        GoalStatus::Cleared => "No active goal. Set one with /goal <text>.".into(),
    }
}

pub fn render_subgoals_list(state: &GoalState) -> String {
    if state.is_empty() {
        return "(no active goal)".into();
    }
    if state.subgoals.is_empty() {
        return "(no subgoals — use /subgoal <text> to add criteria)".into();
    }
    state
        .subgoals
        .iter()
        .enumerate()
        .map(|(idx, sub)| {
            let marker = if sub.done { "[x]" } else { "[ ]" };
            format!("  {}. {marker} {}", idx + 1, sub.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn contract_reminder_line(state: &GoalState) -> String {
    let v = state.contract.verification.trim();
    if v.is_empty() {
        String::new()
    } else {
        format!("Must produce tool evidence (exit_code=0) for: {v}\n")
    }
}

pub fn next_continuation_prompt(state: &GoalState) -> Option<String> {
    if state.status != GoalStatus::Active {
        return None;
    }
    let goal = state.goal_text.as_deref()?.trim();
    if goal.is_empty() {
        return None;
    }
    let contract_line = contract_reminder_line(state);
    if state.subgoals.is_empty() {
        return Some(
            CONTINUATION_PROMPT_TEMPLATE
                .replace("{goal}", goal)
                .replace("{contract_line}", &contract_line),
        );
    }
    let subgoals_block = state
        .subgoals
        .iter()
        .enumerate()
        .map(|(idx, sub)| {
            let marker = if sub.done { "[x]" } else { "[ ]" };
            format!("- {}. {marker} {}", idx + 1, sub.text)
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(
        CONTINUATION_PROMPT_WITH_SUBGOALS_TEMPLATE
            .replace("{goal}", goal)
            .replace("{contract_line}", &contract_line)
            .replace("{subgoals_block}", &subgoals_block),
    )
}

/// Run the goal judge after a turn and update persisted loop state.
///
/// `messages` supplies tool-result contract evidence (code is law).
#[allow(clippy::too_many_arguments)]
pub async fn evaluate_goal_after_turn(
    goal_store: Arc<dyn GoalStore>,
    session_id: &str,
    last_response: &str,
    messages: &[edgecrab_types::Message],
    interrupted: bool,
    _goals_cfg: &GoalsConfig,
    judge_cfg: &GoalJudgeConfig,
    auxiliary_model: Option<&str>,
    main_provider: Arc<dyn LLMProvider>,
    main_model: &str,
) -> Result<GoalContinuationDecision, AgentError> {
    let mut state = goal_store.active(session_id)?;
    if state.is_empty() || state.status != GoalStatus::Active {
        return Ok(GoalContinuationDecision::inactive());
    }

    if interrupted {
        goal_store.pause(session_id, "user-interrupted (Ctrl+C)")?;
        return Ok(GoalContinuationDecision {
            should_continue: false,
            continuation_prompt: None,
            message: "⏸ Goal paused — turn was interrupted. Use /goal resume to continue, or /goal clear to stop.".into(),
            verdict: "skipped".into(),
            status: GoalStatus::Paused,
        });
    }

    if last_response.trim().is_empty() {
        return Ok(GoalContinuationDecision {
            should_continue: false,
            continuation_prompt: None,
            message: String::new(),
            verdict: "skipped".into(),
            status: GoalStatus::Active,
        });
    }

    state.turns_used = state.turns_used.saturating_add(1);

    let goal_text = state.goal_text.clone().unwrap_or_default();
    let (judge_provider, judge_model) = resolve_goal_judge_provider_and_model(
        judge_cfg,
        auxiliary_model,
        main_provider,
        main_model,
    );
    let mut verdict = run_goal_judge(
        &judge_provider,
        &judge_model,
        &goal_text,
        last_response,
        &state,
        judge_cfg,
    )
    .await;

    // Contract law: judge vibes alone cannot mark done — need tool-result evidence.
    if verdict.done && !crate::goal_judge::contract_evidence_in_messages(&state.contract, messages)
    {
        verdict.done = false;
        verdict.reason = format!(
            "contract verification unmet — need tool evidence for: {}",
            state.contract.verification.trim()
        );
    }

    // Wave C: park on wait (do not busy-loop continuations).
    if verdict.wait && !verdict.done {
        state.last_verdict = Some("wait".into());
        state.last_reason = Some(verdict.reason.clone());
        state.consecutive_parse_failures = 0;
        goal_store.save_loop_state(session_id, &state)?;
        return Ok(GoalContinuationDecision {
            should_continue: false,
            continuation_prompt: None,
            message: format!(
                "⏳ Goal parked (waiting): {}. Resume with /goal resume when ready.",
                verdict.reason
            ),
            verdict: "wait".into(),
            status: GoalStatus::Active,
        });
    }

    state.last_verdict = Some(if verdict.done {
        "done".into()
    } else {
        "continue".into()
    });
    state.last_reason = Some(verdict.reason.clone());

    if verdict.parse_failed {
        state.consecutive_parse_failures = state.consecutive_parse_failures.saturating_add(1);
    } else {
        state.consecutive_parse_failures = 0;
    }

    if verdict.done {
        state.status = GoalStatus::Done;
        goal_store.save_loop_state(session_id, &state)?;
        return Ok(GoalContinuationDecision {
            should_continue: false,
            continuation_prompt: None,
            message: format!("✓ Goal achieved: {}", verdict.reason),
            verdict: "done".into(),
            status: GoalStatus::Done,
        });
    }

    if state.consecutive_parse_failures >= DEFAULT_MAX_CONSECUTIVE_PARSE_FAILURES {
        state.status = GoalStatus::Paused;
        state.paused_reason = Some(format!(
            "judge model returned unparseable output {} turns in a row",
            state.consecutive_parse_failures
        ));
        goal_store.save_loop_state(session_id, &state)?;
        return Ok(GoalContinuationDecision {
            should_continue: false,
            continuation_prompt: None,
            message: format!(
                "⏸ Goal paused — the judge model ({} turns) isn't returning the required JSON verdict. \
                 Route the judge to a stricter model in ~/.edgecrab/config.yaml:\n\
                   auxiliary:\n\
                     goal_judge:\n\
                       model: google/gemini-3-flash-preview\n\
                 Then /goal resume to continue.",
                state.consecutive_parse_failures
            ),
            verdict: "continue".into(),
            status: GoalStatus::Paused,
        });
    }

    if state.turns_used >= state.max_turns {
        state.status = GoalStatus::Paused;
        state.paused_reason = Some(format!(
            "turn budget exhausted ({}/{})",
            state.turns_used, state.max_turns
        ));
        goal_store.save_loop_state(session_id, &state)?;
        return Ok(GoalContinuationDecision {
            should_continue: false,
            continuation_prompt: None,
            message: format!(
                "⏸ Goal paused — {}/{} turns used. Use /goal resume to keep going, or /goal clear to stop.",
                state.turns_used, state.max_turns
            ),
            verdict: "continue".into(),
            status: GoalStatus::Paused,
        });
    }

    goal_store.save_loop_state(session_id, &state)?;
    let continuation = next_continuation_prompt(&state);
    Ok(GoalContinuationDecision {
        should_continue: continuation.is_some(),
        continuation_prompt: continuation,
        message: format!(
            "↻ Continuing toward goal ({}/{}): {}",
            state.turns_used, state.max_turns, verdict.reason
        ),
        verdict: "continue".into(),
        status: GoalStatus::Active,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goals::SubGoal;

    #[test]
    fn status_line_formats_active_goal() {
        let state = GoalState {
            goal_text: Some("Ship it".into()),
            status: GoalStatus::Active,
            turns_used: 2,
            max_turns: 20,
            ..Default::default()
        };
        let line = status_line(&state);
        assert!(line.contains("active"));
        assert!(line.contains("Ship it"));
    }

    #[test]
    fn continuation_prompt_includes_subgoals() {
        let state = GoalState {
            goal_text: Some("Build API".into()),
            status: GoalStatus::Active,
            subgoals: vec![SubGoal {
                id: 1,
                text: "write tests".into(),
                done: false,
            }],
            ..Default::default()
        };
        let prompt = next_continuation_prompt(&state).expect("prompt");
        assert!(prompt.contains("write tests"));
        assert!(prompt.contains("Additional criteria"));
    }

    #[test]
    fn is_goal_continuation_text_detects_synthetic_prompt() {
        let prompt = next_continuation_prompt(&GoalState {
            goal_text: Some("Ship".into()),
            status: GoalStatus::Active,
            ..Default::default()
        })
        .expect("prompt");
        assert!(is_goal_continuation_text(&prompt));
        assert!(!is_goal_continuation_text("/goal status"));
    }

    #[test]
    fn prompt_queue_peek_ignores_slash_only_entries() {
        let queue = vec!["/subgoal add tests".into(), "fix the bug".into()];
        assert!(prompt_queue_has_real_user_message(&queue));

        let slash_only = vec!["/goal status".into(), "/subgoal clear".into()];
        assert!(!prompt_queue_has_real_user_message(&slash_only));
    }

    #[test]
    fn drain_goal_continuations_preserves_user_messages() {
        let cont = CONTINUATION_PROMPT_TEMPLATE
            .replace("{goal}", "Ship")
            .replace("{contract_line}", "");
        let mut queue = vec![cont.clone(), "real user msg".into(), cont];
        let removed = drain_goal_continuations_from_queue(&mut queue);
        assert_eq!(removed, 2);
        assert_eq!(queue, vec!["real user msg".to_string()]);
    }

    #[test]
    fn compact_status_chip_formats_active_goal() {
        let chip = compact_status_chip(&GoalState {
            goal_text: Some("Refactor payment module".into()),
            status: GoalStatus::Active,
            turns_used: 3,
            max_turns: 20,
            subgoals: vec![SubGoal {
                id: 1,
                text: "tests".into(),
                done: false,
            }],
            ..Default::default()
        })
        .expect("chip");
        assert!(chip.label.contains("⊙ 3/20"));
        assert!(chip.label.contains("1 sub"));
        assert!(chip.label.contains("Refactor payment"));
    }

    #[test]
    fn goal_flash_from_decision_maps_verdicts() {
        let cont = goal_flash_from_decision(&GoalContinuationDecision {
            should_continue: true,
            continuation_prompt: None,
            message: String::new(),
            verdict: "continue".into(),
            status: GoalStatus::Active,
        });
        assert_eq!(cont.as_deref(), Some("↻ goal continuing"));
    }
}
