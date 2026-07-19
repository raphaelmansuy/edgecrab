//! Display state machine + status-bar badge helpers (extracted from `app.rs`).

use std::time::Instant;

use ratatui::style::{Color, Modifier, Style};

/// Display state machine for the spinner/status area.
#[derive(Clone, Debug)]
pub enum DisplayState {
    Idle,
    AwaitingFirstToken {
        frame: usize,
        started: Instant,
    },
    Thinking {
        frame: usize,
        started: Instant,
    },
    Streaming {
        token_count: u64,
        /// Accumulated character count for word-count estimation in the status bar.
        chars_written: u64,
        /// Most-recently detected markdown heading (level 1 or 2) in the stream.
        current_section: Option<String>,
        started: Instant,
    },
    #[allow(dead_code)]
    ToolExec {
        tool_call_id: String,
        name: String,
        args_json: String,
        detail: Option<String>,
        frame: usize,
        started: Instant,
    },
    /// Background I/O (e.g. model discovery). Does not block user input.
    BgOp {
        label: String,
        frame: usize,
        started: Instant,
    },
    WaitingForClarify,
    WaitingForApproval {
        command: String,
        full_command: String,
        /// Terminal command vs preview-loopback grant (spec 021).
        kind: edgecrab_tools::registry::ApprovalKind,
        selected: usize,
        show_full: bool,
        scroll_offset: u16,
    },
    SecretCapture {
        var_name: String,
        prompt: String,
        is_sudo: bool,
        buffer: String,
    },
    ValueCapture {
        title: String,
        prompt: String,
        placeholder: String,
        masked: bool,
        buffer: String,
        action: ValueCaptureAction,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueCaptureAction {
    BindAddress,
    HomeChannel(String),
    AllowedUsers(String),
    PrimaryField(String),
    ProfileCreate,
    ProfileRename(String),
    ProfileDeleteConfirm(String),
    ProfileAlias(String),
    ProfileExport(String),
    ProfileImport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoicePresenceState {
    Recording { elapsed_secs: u64, continuous: bool },
    Speaking,
    Listening,
}

const VOICE_LISTEN_FRAMES: &[&str] = &[".  ", ".. ", "...", " ..", "  .", "   "];
const VOICE_RECORD_FRAMES: &[&str] = &["*  ", "** ", "***", " **", "  *", "   "];
const VOICE_PLAYBACK_FRAMES: &[&str] = &[">  ", ">> ", ">>>", " >>", "  >", "   "];

/// Frame count shared by voice presence meters (used by the main loop tick).
pub fn voice_presence_frame_count() -> usize {
    VOICE_RECORD_FRAMES.len()
}

pub fn context_usage_ratio(tokens: u64, context_window: Option<u64>) -> Option<f64> {
    context_window
        .filter(|&cw| cw > 0)
        .map(|cw| (tokens as f64 / cw as f64).clamp(0.0, 1.0))
}

pub fn format_voice_presence_badge(state: VoicePresenceState, frame_idx: usize) -> String {
    match state {
        VoicePresenceState::Recording {
            elapsed_secs,
            continuous,
        } => {
            let meter = VOICE_RECORD_FRAMES[frame_idx % VOICE_RECORD_FRAMES.len()];
            let label = if continuous { "TALK" } else { "REC" };
            format!(" {label} {meter} {elapsed_secs}s ")
        }
        VoicePresenceState::Speaking => {
            let meter = VOICE_PLAYBACK_FRAMES[frame_idx % VOICE_PLAYBACK_FRAMES.len()];
            format!(" SPEAK {meter} ")
        }
        VoicePresenceState::Listening => {
            let meter = VOICE_LISTEN_FRAMES[frame_idx % VOICE_LISTEN_FRAMES.len()];
            format!(" LISTEN {meter} ")
        }
    }
}

pub fn run_outcome_badge_style(outcome: &edgecrab_types::RunOutcome) -> Style {
    match outcome.state {
        edgecrab_types::CompletionDecision::Completed => Style::default()
            .fg(Color::Rgb(12, 28, 20))
            .bg(Color::Rgb(108, 220, 155))
            .add_modifier(Modifier::BOLD),
        edgecrab_types::CompletionDecision::NeedsUserInput
        | edgecrab_types::CompletionDecision::Blocked
        | edgecrab_types::CompletionDecision::NeedsVerification => Style::default()
            .fg(Color::Rgb(36, 24, 10))
            .bg(Color::Rgb(255, 204, 92))
            .add_modifier(Modifier::BOLD),
        edgecrab_types::CompletionDecision::BudgetExhausted
        | edgecrab_types::CompletionDecision::Incomplete => Style::default()
            .fg(Color::Rgb(40, 22, 8))
            .bg(Color::Rgb(255, 170, 90))
            .add_modifier(Modifier::BOLD),
        edgecrab_types::CompletionDecision::Interrupted
        | edgecrab_types::CompletionDecision::Failed => Style::default()
            .fg(Color::Rgb(38, 12, 12))
            .bg(Color::Rgb(255, 120, 120))
            .add_modifier(Modifier::BOLD),
    }
}

pub fn goal_status_chip_style(status: edgecrab_core::GoalStatus) -> Style {
    match status {
        edgecrab_core::GoalStatus::Active => Style::default()
            .fg(Color::Rgb(180, 230, 255))
            .add_modifier(Modifier::BOLD),
        edgecrab_core::GoalStatus::Paused => Style::default()
            .fg(Color::Rgb(255, 210, 120))
            .add_modifier(Modifier::BOLD),
        edgecrab_core::GoalStatus::Done => Style::default()
            .fg(Color::Rgb(140, 255, 180))
            .add_modifier(Modifier::BOLD),
        edgecrab_core::GoalStatus::Cleared => Style::default().fg(Color::Rgb(140, 140, 160)),
    }
}

pub fn goal_flash_badge_style(flash: &str) -> Style {
    if flash.contains("continuing") {
        Style::default()
            .fg(Color::Rgb(200, 230, 255))
            .add_modifier(Modifier::BOLD)
    } else if flash.contains("complete") {
        Style::default()
            .fg(Color::Rgb(140, 255, 180))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Rgb(255, 210, 120))
            .add_modifier(Modifier::BOLD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_usage_ratio_clamps_at_one_hundred_percent() {
        assert_eq!(context_usage_ratio(210_000, Some(200_000)), Some(1.0));
        assert_eq!(context_usage_ratio(100_000, Some(200_000)), Some(0.5));
        assert_eq!(context_usage_ratio(100, Some(0)), None);
        assert_eq!(context_usage_ratio(100, None), None);
    }

    #[test]
    fn voice_presence_badges_non_empty() {
        let recording = format_voice_presence_badge(
            VoicePresenceState::Recording {
                elapsed_secs: 3,
                continuous: false,
            },
            0,
        );
        let speaking = format_voice_presence_badge(VoicePresenceState::Speaking, 2);
        let listening = format_voice_presence_badge(VoicePresenceState::Listening, 2);
        assert!(recording.contains("REC"));
        assert!(speaking.contains("SPEAK"));
        assert!(listening.contains("LISTEN"));
    }
}
