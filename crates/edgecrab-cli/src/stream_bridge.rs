//! Pure turn-activity updates from agent stream events (Hermes `turnController` parity).
//!
//! Keeps shelf state mutations testable without the `app.rs` event loop.

use std::time::Instant;

use crate::turn_activity::{ActivityTone, TurnActivityState};

/// Hermes `THINKING_COT_MAX` — reasoning snippet budget on the shelf.
pub const THINKING_COT_MAX: usize = 160;

/// Detect markdown heading landmarks in streaming tokens (status bar section hint).
pub fn extract_streaming_section(token: &str, current_section: &mut Option<String>) {
    let candidates: &[&str] = &["\n### ", "\n## ", "\n# ", "### ", "## ", "# "];
    for marker in candidates {
        if let Some(pos) = token.find(marker) {
            let heading_start = pos + marker.len();
            let valid_start =
                pos == 0 || token.as_bytes().get(pos.saturating_sub(1)).copied() == Some(b'\n');
            if !valid_start {
                continue;
            }
            let rest = &token[heading_start..];
            let heading: String = rest
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .chars()
                .take(30)
                .collect();
            if !heading.is_empty() {
                *current_section = Some(heading);
                return;
            }
        }
    }
}

pub fn apply_tool_generating(
    state: &mut TurnActivityState,
    tool_call_id: String,
    name: String,
    partial_args: String,
) {
    state.on_generating(tool_call_id, name, partial_args);
}

pub fn clear_tool_generating(state: &mut TurnActivityState) {
    state.clear_tool_generating();
}

pub fn apply_tool_exec(
    state: &mut TurnActivityState,
    tool_call_id: String,
    name: String,
    args_json: String,
    preview: String,
    seq: u64,
) {
    state.on_tool_exec(tool_call_id, name, args_json, preview, seq);
}

pub fn apply_tool_progress(
    state: &mut TurnActivityState,
    tool_call_id: &str,
    detail: String,
    seq: u64,
    now: Instant,
) {
    state.on_tool_progress(tool_call_id, detail, seq, now);
}

pub fn apply_tool_done(state: &mut TurnActivityState, tool_call_id: &str) {
    state.on_tool_done(tool_call_id);
}

pub fn apply_llm_wait_progress(
    state: &mut TurnActivityState,
    provider: &str,
    elapsed_secs: u64,
    has_tools: bool,
    ctx: edgecrab_tools::tool_progress_tail::LlmWaitContext,
) {
    state.on_llm_wait_progress(provider, elapsed_secs, has_tools, ctx);
}

pub fn apply_reasoning_delta(state: &mut TurnActivityState, text: &str) {
    state.on_reasoning(text);
}

pub fn apply_activity_notice(state: &mut TurnActivityState, text: String, tone: ActivityTone) {
    state.push_activity(text, tone);
}

pub fn apply_subagent_start(
    state: &mut TurnActivityState,
    task_index: usize,
    task_count: usize,
    goal: String,
    depth: u32,
    agent_id: String,
    parent_id: Option<String>,
) {
    state.on_subagent_start(task_index, task_count, goal, depth, agent_id, parent_id);
}

pub fn apply_subagent_detail(state: &mut TurnActivityState, task_index: usize, detail: String) {
    state.on_subagent_detail(task_index, detail);
}

pub fn apply_subagent_tool(
    state: &mut TurnActivityState,
    task_index: usize,
    name: &str,
    tool_label: String,
) {
    state.on_subagent_tool(task_index, name, tool_label);
}

pub fn apply_subagent_finish(state: &mut TurnActivityState, task_index: usize) {
    state.on_subagent_finish(task_index);
}

/// Hermes `maybeNudgeAgents` — once per turn when the first delegate starts.
pub fn maybe_agents_nudge(state: &mut TurnActivityState, nudged: &mut bool) {
    if *nudged || state.subagents.is_empty() {
        return;
    }
    *nudged = true;
    apply_activity_notice(
        state,
        "subagents working · /agents to monitor".into(),
        ActivityTone::Info,
    );
}

/// Truncate reasoning for shelf display (centralized budget).
pub fn truncate_reasoning_snippet(text: &str) -> String {
    edgecrab_core::safe_truncate(text.trim(), THINKING_COT_MAX).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn_activity::{ShelfPhase, TurnActivityState};

    #[test]
    fn extract_streaming_section_detects_headings() {
        let mut section = None;
        extract_streaming_section("\n## Competitive Landscape\nsome text", &mut section);
        assert_eq!(section.as_deref(), Some("Competitive Landscape"));

        let mut s2 = None;
        extract_streaming_section("\n# Introduction\n", &mut s2);
        assert_eq!(s2.as_deref(), Some("Introduction"));

        let mut s3 = None;
        extract_streaming_section("just prose text", &mut s3);
        assert!(s3.is_none());

        let mut s4 = None;
        extract_streaming_section("## Market Analysis\n", &mut s4);
        assert_eq!(s4.as_deref(), Some("Market Analysis"));

        let long_title = format!("\n## {}", "x".repeat(40));
        let mut s5 = None;
        extract_streaming_section(&long_title, &mut s5);
        assert_eq!(s5.as_ref().map(|s| s.chars().count()), Some(30));
    }

    #[test]
    fn extract_streaming_section_preserves_previous_when_no_heading() {
        let mut s3: Option<String> = Some("Previous".to_string());
        extract_streaming_section("just prose text", &mut s3);
        assert_eq!(s3.as_deref(), Some("Previous"));
    }

    #[test]
    fn tool_lifecycle_updates_phase() {
        let mut state = TurnActivityState::new(true);
        apply_tool_exec(
            &mut state,
            "tc1".into(),
            "terminal".into(),
            r#"{"command":"echo hi"}"#.into(),
            "echo hi".into(),
            1,
        );
        assert!(matches!(state.phase, ShelfPhase::ToolExec));
        assert!(state.contains_tool("tc1"));

        apply_tool_progress(&mut state, "tc1", "hello".into(), 2, Instant::now());
        assert_eq!(
            state.tool_row("tc1").and_then(|r| r.detail.as_deref()),
            Some("hello")
        );

        apply_tool_done(&mut state, "tc1");
        assert!(!state.contains_tool("tc1"));
        assert!(matches!(state.phase, ShelfPhase::AwaitingFirstToken));
    }

    #[test]
    fn generating_then_exec_clears_generating() {
        let mut state = TurnActivityState::new(true);
        apply_tool_generating(
            &mut state,
            "tc1".into(),
            "file_read".into(),
            r#"{"path":"a.rs"}"#.into(),
        );
        assert!(matches!(state.phase, ShelfPhase::GeneratingTool));

        apply_tool_exec(
            &mut state,
            "tc1".into(),
            "file_read".into(),
            r#"{"path":"a.rs"}"#.into(),
            "a.rs".into(),
            1,
        );
        assert!(matches!(state.phase, ShelfPhase::ToolExec));
        assert!(state.generating_tool.is_none());
    }

    #[test]
    fn subagent_tracks_tool_churn() {
        let mut state = TurnActivityState::new(true);
        apply_subagent_start(
            &mut state,
            0,
            2,
            "audit auth".into(),
            1,
            "sa-0".into(),
            None,
        );
        apply_subagent_tool(&mut state, 0, "file_read", "file_read  auth.rs".into());
        apply_subagent_tool(&mut state, 0, "terminal", "terminal  cargo test".into());
        assert_eq!(state.subagent_tool_total(), 2);
        let row = state.subagents.get(&0).unwrap();
        assert_eq!(row.tool_count, 2);
        assert_eq!(row.recent_tools, vec!["file read", "terminal"]);

        apply_subagent_finish(&mut state, 0);
        assert!(state.subagents.is_empty());
    }

    #[test]
    fn reasoning_snippet_respects_cot_max() {
        let long = "x".repeat(300);
        let trimmed = truncate_reasoning_snippet(&long);
        assert!(trimmed.chars().count() <= THINKING_COT_MAX);
    }

    #[test]
    fn activity_notice_dedupes_consecutive() {
        let mut state = TurnActivityState::new(true);
        apply_activity_notice(
            &mut state,
            "compressing context…".into(),
            ActivityTone::Info,
        );
        apply_activity_notice(
            &mut state,
            "compressing context…".into(),
            ActivityTone::Info,
        );
        assert_eq!(state.activity_feed.len(), 1);
    }

    #[test]
    fn agents_nudge_fires_once() {
        let mut state = TurnActivityState::new(true);
        let mut nudged = false;
        apply_subagent_start(&mut state, 0, 2, "audit".into(), 1, "sa-0".into(), None);
        maybe_agents_nudge(&mut state, &mut nudged);
        assert!(nudged);
        assert!(
            state
                .activity_feed
                .iter()
                .any(|n| n.text.contains("/agents"))
        );
        apply_subagent_start(&mut state, 1, 2, "other".into(), 1, "sa-1".into(), None);
        let feed_len = state.activity_feed.len();
        maybe_agents_nudge(&mut state, &mut nudged);
        assert_eq!(state.activity_feed.len(), feed_len);
    }

    /// Anti-stuck invariant: shelf detail receives tail-3 from `tool_progress_tail` formatter.
    #[test]
    fn tool_progress_applies_tail_three_detail() {
        use edgecrab_tools::tool_progress_tail::format_tail_from_text;

        let stdout = "line1\nline2\nline3\nline4\n   Compiling edgecrab v0.9.0";
        let tail = format_tail_from_text(stdout);
        assert!(tail.contains("Compiling edgecrab"));
        assert!(!tail.contains("line1"));

        let mut state = TurnActivityState::new(true);
        apply_tool_exec(
            &mut state,
            "tc1".into(),
            "terminal".into(),
            r#"{"command":"cargo build"}"#.into(),
            "cargo build".into(),
            1,
        );
        apply_tool_progress(&mut state, "tc1", tail.clone(), 2, Instant::now());
        assert_eq!(
            state.tool_row("tc1").and_then(|r| r.detail.as_deref()),
            Some(tail.as_str())
        );
    }
}
