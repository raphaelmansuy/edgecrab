//! Phase-specific shelf spinners — Hermes `thinking.tsx` THINK/TOOL parity (without unicode-animations dep).

use crate::turn_activity::{ShelfPhase, TurnActivityState};

const BRAILLE: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const TOOL: [&str; 8] = ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
const DELEGATE: [&str; 4] = ["◐", "◓", "◑", "◒"];
const CLARIFY: [&str; 4] = ["❓", "❔", "❓", "❔"];

pub const STATIC_SPINNER: &str = "◦";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpinnerKind {
    Thinking,
    Tool,
    Delegate,
    Clarify,
    Default,
}

pub fn spinner_kind_for_state(state: &TurnActivityState) -> SpinnerKind {
    if matches!(
        state.phase,
        ShelfPhase::WaitingForClarify | ShelfPhase::WaitingForApproval
    ) {
        return SpinnerKind::Clarify;
    }
    if !state.subagents.is_empty() {
        let active_tools = state.tools.values().any(|row| !row.finished);
        if !active_tools {
            return SpinnerKind::Delegate;
        }
    }
    match state.phase {
        ShelfPhase::ToolExec | ShelfPhase::GeneratingTool | ShelfPhase::BgOp => SpinnerKind::Tool,
        ShelfPhase::Thinking
        | ShelfPhase::AwaitingFirstToken
        | ShelfPhase::Streaming => SpinnerKind::Thinking,
        _ => SpinnerKind::Default,
    }
}

pub fn spinner_glyph(kind: SpinnerKind, frame: usize, animate: bool) -> &'static str {
    if !animate {
        return STATIC_SPINNER;
    }
    match kind {
        SpinnerKind::Thinking | SpinnerKind::Default => BRAILLE[frame % BRAILLE.len()],
        SpinnerKind::Tool => TOOL[frame % TOOL.len()],
        SpinnerKind::Delegate => DELEGATE[frame % DELEGATE.len()],
        SpinnerKind::Clarify => CLARIFY[frame % CLARIFY.len()],
    }
}

pub fn shelf_spinner_glyph(state: &TurnActivityState, frame: usize, animate: bool) -> &'static str {
    spinner_glyph(spinner_kind_for_state(state), frame, animate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_phase_uses_bar_spinner() {
        let mut state = TurnActivityState::new(true);
        state.set_phase(ShelfPhase::ToolExec);
        state.on_tool_exec(
            "t1".into(),
            "terminal".into(),
            "{}".into(),
            "build".into(),
            1,
        );
        assert_eq!(spinner_kind_for_state(&state), SpinnerKind::Tool);
        assert_eq!(spinner_glyph(SpinnerKind::Tool, 0, true), "▁");
    }

    #[test]
    fn delegate_phase_when_subagents_active() {
        let mut state = TurnActivityState::new(true);
        state.on_subagent_start(0, 2, "scan".into(), 1, "sa-0".into(), None);
        assert_eq!(spinner_kind_for_state(&state), SpinnerKind::Delegate);
    }

    #[test]
    fn static_when_animation_off() {
        assert_eq!(
            spinner_glyph(SpinnerKind::Thinking, 3, false),
            STATIC_SPINNER
        );
    }
}
