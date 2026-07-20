//! Test harness: `StreamEvent` → shelf activity (Hermes `turnController` unit tests).

use std::time::Instant;

use edgecrab_core::StreamEvent;

use crate::stream_bridge::{
    apply_activity_notice, apply_llm_wait_progress, apply_reasoning_delta, apply_subagent_detail,
    apply_subagent_finish, apply_subagent_start, apply_subagent_tool, apply_tool_done,
    apply_tool_exec, apply_tool_generating, apply_tool_progress, maybe_agents_nudge,
};
use crate::stream_presentation::StreamPresentation;
use crate::tool_display::extract_tool_preview;
use crate::turn_activity::{ActivityTone, TurnActivityState};
use edgecrab_tools::tool_progress_tail::format_tail_from_text;

/// Test harness: `StreamEvent` → shelf activity (Hermes `turnController` unit tests).
#[allow(dead_code)] // consumed by `#[cfg(test)]` module in this binary crate
#[derive(Debug)]
pub struct TurnStreamHarness {
    pub activity: TurnActivityState,
    pub presentation: StreamPresentation,
    pub agents_nudged: bool,
    progress_seq: u64,
}

#[allow(dead_code)]
impl TurnStreamHarness {
    pub fn new() -> Self {
        Self {
            activity: TurnActivityState::new(true),
            presentation: StreamPresentation::new(),
            agents_nudged: false,
            progress_seq: 0,
        }
    }

    pub fn apply(&mut self, event: StreamEvent, now: Instant) {
        match event {
            StreamEvent::Reasoning(text) => {
                self.presentation.on_reasoning(&text);
                apply_reasoning_delta(&mut self.activity, &self.presentation, &text);
                crate::stream_bridge::sync_shelf_phase_from_presentation(
                    &mut self.activity,
                    &self.presentation,
                );
            }
            StreamEvent::ToolGenerating {
                tool_call_id,
                name,
                partial_args,
            } => {
                apply_tool_generating(&mut self.activity, tool_call_id, name, partial_args);
                crate::stream_bridge::sync_shelf_phase_from_presentation(
                    &mut self.activity,
                    &self.presentation,
                );
            }
            StreamEvent::ToolExec {
                tool_call_id,
                name,
                args_json,
            } => {
                let _ = self
                    .presentation
                    .on_tool_exec(tool_call_id.clone(), name.clone());
                self.activity.clear_thinking_peek();
                self.progress_seq += 1;
                let preview = extract_tool_preview(&name, &args_json);
                apply_tool_exec(
                    &mut self.activity,
                    tool_call_id,
                    name,
                    args_json,
                    preview,
                    self.progress_seq,
                );
                crate::stream_bridge::sync_shelf_phase_from_presentation(
                    &mut self.activity,
                    &self.presentation,
                );
            }
            StreamEvent::ToolProgress {
                tool_call_id,
                message,
                ..
            } => {
                self.progress_seq += 1;
                let detail = if message.contains('\n') {
                    format_tail_from_text(&message)
                } else {
                    message
                };
                self.presentation.on_tool_progress(&tool_call_id, &detail);
                apply_tool_progress(
                    &mut self.activity,
                    &tool_call_id,
                    detail,
                    self.progress_seq,
                    now,
                );
                crate::stream_bridge::sync_shelf_phase_from_presentation(
                    &mut self.activity,
                    &self.presentation,
                );
            }
            StreamEvent::ToolDone { tool_call_id, .. } => {
                let _ = self.presentation.on_tool_done(&tool_call_id);
                apply_tool_done(&mut self.activity, &tool_call_id);
                crate::stream_bridge::sync_shelf_phase_from_presentation(
                    &mut self.activity,
                    &self.presentation,
                );
            }
            StreamEvent::Token(_) => {
                let _ = self.presentation.on_token();
                crate::stream_bridge::sync_shelf_phase_from_presentation(
                    &mut self.activity,
                    &self.presentation,
                );
            }
            StreamEvent::SubAgentStart {
                task_index,
                task_count,
                goal,
                depth,
                agent_id,
                parent_id: _,
            } => {
                apply_subagent_start(
                    &mut self.activity,
                    task_index,
                    task_count,
                    goal,
                    depth,
                    agent_id,
                    None,
                );
                maybe_agents_nudge(&mut self.activity, &mut self.agents_nudged);
            }
            StreamEvent::SubAgentReasoning {
                task_index, text, ..
            } => {
                apply_subagent_detail(&mut self.activity, task_index, text);
            }
            StreamEvent::SubAgentToolExec {
                task_index,
                name,
                args_json,
                ..
            } => {
                let label = extract_tool_preview(&name, &args_json);
                apply_subagent_tool(
                    &mut self.activity,
                    task_index,
                    &name,
                    format!("{name}  {label}"),
                );
            }
            StreamEvent::SubAgentFinish { task_index, .. } => {
                apply_subagent_finish(&mut self.activity, task_index);
            }
            StreamEvent::ActivityNotice(text) => {
                apply_activity_notice(&mut self.activity, text, ActivityTone::Info);
            }
            StreamEvent::LlmWaitProgress {
                provider,
                elapsed_secs,
                has_tools,
                prompt_tokens_estimated,
                context_length,
                prefill_pct,
                api_iteration,
                native_streaming,
            } => {
                apply_llm_wait_progress(
                    &mut self.activity,
                    &provider,
                    elapsed_secs,
                    has_tools,
                    edgecrab_tools::tool_progress_tail::LlmWaitContext {
                        prompt_tokens_estimated,
                        context_length,
                        prefill_pct,
                        api_iteration,
                        native_streaming,
                    },
                );
            }
            StreamEvent::BackgroundProcessTail {
                process_id,
                command_preview,
                tail,
            } => {
                self.activity.on_bg_tail(process_id, command_preview, tail);
            }
            _ => {}
        }
    }

    pub fn reasoning_snippet(&self) -> Option<&str> {
        self.activity.reasoning_snippet.as_deref()
    }

    /// Sole CoT buffer (presentation) — activity no longer duplicates full text.
    pub fn reasoning_full(&self) -> &str {
        self.presentation.thinking_full()
    }
}

impl Default for TurnStreamHarness {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn_activity::ShelfPhase;

    #[test]
    fn terminal_build_turn_lifecycle() {
        let mut h = TurnStreamHarness::new();
        let now = Instant::now();

        h.apply(
            StreamEvent::ToolExec {
                tool_call_id: "tc1".into(),
                name: "terminal".into(),
                args_json: r#"{"command":"cargo build"}"#.into(),
            },
            now,
        );
        assert!(matches!(h.activity.phase, ShelfPhase::ToolExec));

        let stdout = (1..=6)
            .map(|n| format!("line{n}"))
            .chain(["   Compiling edgecrab v0.9.0".into()])
            .collect::<Vec<_>>()
            .join("\n");
        h.apply(
            StreamEvent::ToolProgress {
                tool_call_id: "tc1".into(),
                name: "terminal".into(),
                message: stdout,
            },
            now,
        );
        let detail = h
            .activity
            .tool_row("tc1")
            .and_then(|r| r.detail.as_deref())
            .unwrap_or("");
        assert!(detail.contains("Compiling edgecrab"));
        assert!(!detail.contains("line1"));

        h.apply(
            StreamEvent::ToolDone {
                tool_call_id: "tc1".into(),
                name: "terminal".into(),
                args_json: r#"{"command":"cargo build"}"#.into(),
                result_preview: Some("ok".into()),
                duration_ms: 1200,
                is_error: false,
            },
            now,
        );
        assert!(!h.activity.contains_tool("tc1"));
        assert!(matches!(h.activity.phase, ShelfPhase::AwaitingFirstToken));
    }

    #[test]
    fn delegation_nudge_and_subagent_tools() {
        let mut h = TurnStreamHarness::new();
        let now = Instant::now();

        h.apply(
            StreamEvent::SubAgentStart {
                task_index: 0,
                task_count: 1,
                goal: "audit auth".into(),
                depth: 1,
                agent_id: "sa-0".into(),
                parent_id: None,
            },
            now,
        );
        assert!(h.agents_nudged);
        assert!(
            h.activity
                .activity_feed
                .iter()
                .any(|n| n.text.contains("/agents"))
        );

        h.apply(
            StreamEvent::SubAgentToolExec {
                task_index: 0,
                task_count: 1,
                name: "file_read".into(),
                args_json: r#"{"path":"auth.rs"}"#.into(),
            },
            now,
        );
        assert_eq!(h.activity.subagent_tool_total(), 1);

        h.apply(
            StreamEvent::SubAgentFinish {
                task_index: 0,
                task_count: 1,
                status: "done".into(),
                duration_ms: 500,
                summary: "ok".into(),
                api_calls: 2,
                model: None,
            },
            now,
        );
        assert!(h.activity.subagents.is_empty());
    }

    #[test]
    fn reasoning_delta_respects_cot_budget() {
        let mut h = TurnStreamHarness::new();
        h.apply(StreamEvent::Reasoning("x".repeat(400)), Instant::now());
        // Full buffer keeps entire stream; shelf snippet is tail-capped.
        assert_eq!(h.reasoning_full().len(), 400);
        assert!(
            h.reasoning_snippet()
                .map(|s| s.chars().count())
                .unwrap_or(0)
                <= crate::stream_bridge::THINKING_COT_MAX + 1 // optional … prefix
        );
    }

    #[test]
    fn super_grok_thinking_accumulates_then_tool_finalizes() {
        let mut h = TurnStreamHarness::new();
        let now = Instant::now();
        h.apply(StreamEvent::Reasoning("Step 1: inspect. ".into()), now);
        h.apply(StreamEvent::Reasoning("Step 2: write file.".into()), now);
        assert!(h.reasoning_full().contains("Step 1"));
        assert!(h.reasoning_full().contains("Step 2"));
        assert!(matches!(h.activity.phase, ShelfPhase::Thinking));
        h.apply(
            StreamEvent::ToolExec {
                tool_call_id: "tc1".into(),
                name: "write_file".into(),
                args_json: r#"{"path":"a.rs"}"#.into(),
            },
            now,
        );
        // Thinking cleared for next phase; tool tracked.
        assert!(h.presentation.thinking_full().is_empty());
        assert!(h.activity.reasoning_snippet.is_none());
        assert!(h.activity.contains_tool("tc1"));
        h.apply(
            StreamEvent::ToolProgress {
                tool_call_id: "tc1".into(),
                name: "write_file".into(),
                message: "writing…".into(),
            },
            now,
        );
        assert!(
            h.presentation
                .tools
                .get("tc1")
                .is_some_and(|b| b.body.contains("writing"))
        );
        h.apply(
            StreamEvent::ToolDone {
                tool_call_id: "tc1".into(),
                name: "write_file".into(),
                args_json: "{}".into(),
                result_preview: Some("ok".into()),
                duration_ms: 12,
                is_error: false,
            },
            now,
        );
        assert!(!h.activity.contains_tool("tc1"));
    }

    #[test]
    fn parallel_tool_exec_all_tracked() {
        let mut h = TurnStreamHarness::new();
        let now = Instant::now();
        for (id, name) in [("a", "file_read"), ("b", "file_search"), ("c", "terminal")] {
            h.apply(
                StreamEvent::ToolExec {
                    tool_call_id: id.into(),
                    name: name.into(),
                    args_json: "{}".into(),
                },
                now,
            );
        }
        assert_eq!(h.activity.sorted_active_tools().count(), 3);
    }

    #[test]
    fn generating_args_stream_updates_preview() {
        let mut h = TurnStreamHarness::new();
        h.apply(
            StreamEvent::ToolGenerating {
                tool_call_id: "tc1".into(),
                name: "terminal".into(),
                partial_args: r#"{"command":"cargo ""#.into(),
            },
            Instant::now(),
        );
        h.apply(
            StreamEvent::ToolGenerating {
                tool_call_id: "tc1".into(),
                name: "terminal".into(),
                partial_args: r#"{"command":"cargo test --workspace"}"#.into(),
            },
            Instant::now(),
        );
        let preview = h.activity.generating_preview.as_deref().unwrap_or("");
        assert!(preview.contains("cargo test"));
    }

    #[test]
    fn ha03_tool_exec_caches_path_for_done_resolution() {
        let mut h = TurnStreamHarness::new();
        let now = Instant::now();
        let args = r#"{"path":"demo/games003/index.html","limit":120}"#;
        h.apply(
            StreamEvent::ToolExec {
                tool_call_id: "tc-ha03".into(),
                name: "read_file".into(),
                args_json: args.into(),
            },
            now,
        );
        let resolved = h.activity.resolve_args_at_done("tc-ha03", "");
        let preview = crate::tool_display::extract_tool_preview("read_file", &resolved);
        assert!(preview.contains("games003"));
        assert!(!preview.contains('?'));
    }

    #[test]
    fn tool_generating_then_exec_clears_draft() {
        let mut h = TurnStreamHarness::new();
        let now = Instant::now();
        h.apply(
            StreamEvent::ToolGenerating {
                tool_call_id: "tc1".into(),
                name: "terminal".into(),
                partial_args: "{}".into(),
            },
            now,
        );
        h.apply(
            StreamEvent::ToolExec {
                tool_call_id: "tc1".into(),
                name: "terminal".into(),
                args_json: r#"{"command":"cargo build"}"#.into(),
            },
            now,
        );
        assert!(h.activity.generating_tool.is_none());
        assert!(h.activity.contains_tool("tc1"));
    }

    #[test]
    fn turn_phase_drives_shelf_chrome() {
        use crate::stream_presentation::{ChromePhaseHint, TurnPhase};
        let mut h = TurnStreamHarness::new();
        let now = Instant::now();
        h.apply(StreamEvent::Reasoning("plan".into()), now);
        assert!(matches!(h.presentation.phase, TurnPhase::Thinking));
        assert_eq!(
            h.presentation.chrome_phase_hint(false),
            ChromePhaseHint::Thinking
        );
        assert!(matches!(h.activity.phase, ShelfPhase::Thinking));

        h.apply(
            StreamEvent::ToolExec {
                tool_call_id: "tc1".into(),
                name: "terminal".into(),
                args_json: r#"{"command":"ls"}"#.into(),
            },
            now,
        );
        assert!(matches!(h.presentation.phase, TurnPhase::Tool { .. }));
        assert!(matches!(h.activity.phase, ShelfPhase::ToolExec));

        h.apply(
            StreamEvent::ToolDone {
                tool_call_id: "tc1".into(),
                name: "terminal".into(),
                args_json: "{}".into(),
                result_preview: Some("ok".into()),
                duration_ms: 10,
                is_error: false,
            },
            now,
        );
        assert_eq!(
            h.presentation.chrome_phase_hint(false),
            ChromePhaseHint::AwaitingFirstToken
        );

        h.apply(StreamEvent::Token("hello".into()), now);
        assert!(matches!(h.presentation.phase, TurnPhase::Responding));
        assert!(matches!(h.activity.phase, ShelfPhase::Streaming));
        assert!(
            h.presentation.worked_for_label().is_some() || h.presentation.turn_started.is_some()
        );
    }
}
