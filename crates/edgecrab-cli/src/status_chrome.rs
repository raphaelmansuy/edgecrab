//! Status bar chrome — spinner labels and urgency colors (extracted from `app.rs`).

use ratatui::style::Color;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use crate::status_indicator::{StatusIndicatorStyle, render_indicator_frame};
use crate::theme::Theme;
use crate::tool_display::{tool_action_verb, tool_icon};
use crate::turn_activity::{ShelfPhase, TurnActivityState};

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const ASCII_SPINNER_FRAMES: &[&str] = &["-", "\\", "|", "/"];
const VERB_DISPLAY_PAD: usize = 13;

/// Format token count for display (e.g. 1234 → "1.2k", 1234567 → "1.2M").
pub fn format_token_count(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        format!("{count}")
    }
}

/// Estimate word count from a character count (status bar stability).
pub fn words_estimate(chars: u64) -> u64 {
    if chars < 20 {
        return 0;
    }
    let raw = (chars as f64 / 4.5) as u64;
    (raw / 10) * 10
}

/// Elapsed-time hint once `threshold_secs` have passed.
pub fn format_elapsed_hint(elapsed: std::time::Duration, threshold_secs: u64) -> String {
    let secs = elapsed.as_secs();
    if secs >= threshold_secs {
        format!("  {}s", secs)
    } else {
        String::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalGlyphProfile {
    Unicode,
    Ascii,
}

/// Status-bar spinner with ASCII fallback for constrained terminals.
pub fn compact_spinner_frame(frame_idx: usize, glyphs: TerminalGlyphProfile) -> &'static str {
    match glyphs {
        TerminalGlyphProfile::Unicode => SPINNER_FRAMES[frame_idx % SPINNER_FRAMES.len()],
        TerminalGlyphProfile::Ascii => ASCII_SPINNER_FRAMES[frame_idx % ASCII_SPINNER_FRAMES.len()],
    }
}

fn unicode_pad_right(s: &str, target_display_cols: usize) -> String {
    let w = s.width();
    if w >= target_display_cols {
        return s.to_string();
    }
    format!("{}{}", s, " ".repeat(target_display_cols - w))
}

fn phase_wings(wings: &[[String; 2]], idx: usize) -> (&str, &str) {
    if wings.is_empty() {
        ("", "")
    } else {
        let wing = &wings[idx % wings.len()];
        (wing[0].as_str(), wing[1].as_str())
    }
}

#[allow(clippy::too_many_arguments)]
fn format_phase_status(
    indicator_style: StatusIndicatorStyle,
    theme: &Theme,
    glyphs: TerminalGlyphProfile,
    spinner_frame: usize,
    face_idx: usize,
    verb: &str,
    elapsed_secs: u64,
    early_label: &str,
    long_label: &str,
) -> String {
    let ind = render_indicator_frame(indicator_style, theme, glyphs, spinner_frame, face_idx);
    let verb_padded = if ind.show_verb {
        unicode_pad_right(verb, VERB_DISPLAY_PAD)
    } else {
        String::new()
    };
    let core = if ind.face.is_empty() {
        if verb_padded.is_empty() {
            ind.leading.clone()
        } else {
            format!("{} {verb_padded}", ind.leading.trim_end())
        }
    } else if ind.leading.is_empty() {
        format!("{} {verb_padded}", ind.face)
    } else {
        format!("{} {} {verb_padded}", ind.leading, ind.face)
    };
    let (left_wing, right_wing) = phase_wings(&theme.spinner_wings, face_idx);
    if elapsed_secs > 20 {
        format!("{left_wing}{core} \u{26a0} {long_label} {elapsed_secs}s  ^C=stop{right_wing}")
    } else if elapsed_secs > 10 {
        format!("{left_wing}{core} {long_label} {elapsed_secs}s  ^C=stop{right_wing}")
    } else if elapsed_secs > 3 {
        format!("{left_wing}{core} {long_label} {elapsed_secs}s{right_wing}")
    } else if elapsed_secs > 1 {
        format!("{left_wing}{core} {early_label}{right_wing}")
    } else {
        format!("{left_wing}{core}{right_wing}")
    }
}

/// Map wait elapsed seconds to urgency color (FP46).
pub fn wait_urgency_color(elapsed_secs: u64) -> Color {
    if elapsed_secs >= 30 {
        Color::Rgb(239, 83, 80)
    } else if elapsed_secs >= 15 {
        Color::Rgb(255, 140, 50)
    } else {
        Color::Rgb(255, 210, 120)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn format_waiting_first_token_status(
    theme: &Theme,
    indicator_style: StatusIndicatorStyle,
    glyphs: TerminalGlyphProfile,
    frame_idx: usize,
    verb_idx: usize,
    face_idx: usize,
    elapsed_secs: u64,
    long_label: &str,
) -> String {
    let verb = if theme.waiting_verbs.is_empty() {
        "awaiting"
    } else {
        &theme.waiting_verbs[verb_idx % theme.waiting_verbs.len()]
    };
    format_phase_status(
        indicator_style,
        theme,
        glyphs,
        frame_idx,
        face_idx,
        verb,
        elapsed_secs,
        "first token",
        long_label,
    )
}

pub fn format_thinking_status(
    theme: &Theme,
    indicator_style: StatusIndicatorStyle,
    glyphs: TerminalGlyphProfile,
    frame_idx: usize,
    verb_idx: usize,
    face_idx: usize,
    elapsed_secs: u64,
) -> String {
    let verb = if theme.thinking_verbs.is_empty() {
        "thinking"
    } else {
        &theme.thinking_verbs[verb_idx % theme.thinking_verbs.len()]
    };
    format_phase_status(
        indicator_style,
        theme,
        glyphs,
        frame_idx,
        face_idx,
        verb,
        elapsed_secs,
        "thinking",
        "thinking",
    )
}

pub struct ActiveToolSummary {
    pub verb: String,
    pub icon: String,
    pub detail: String,
    pub elapsed_secs: u64,
}

/// Status-bar tool line — single source via [`TurnActivityState::tool_summary`].
pub fn summarize_tools_for_status(turn_activity: &TurnActivityState) -> Option<ActiveToolSummary> {
    turn_activity.tool_summary().map(|s| {
        let detail = if s.preparing {
            let preview = if s.detail.trim().is_empty() {
                s.primary_name.replace('_', " ")
            } else {
                edgecrab_core::safe_truncate(s.detail.trim(), 52).to_string()
            };
            if s.elapsed_secs > 0 {
                format!("{preview} · {}s", s.elapsed_secs)
            } else {
                preview
            }
        } else if s.active_count > 1 {
            format!(
                "{} tools · {} +{}",
                s.active_count,
                s.primary_name.replace('_', " "),
                s.active_count - 1
            )
        } else {
            edgecrab_core::safe_truncate(&s.detail, 52).to_string()
        };
        ActiveToolSummary {
            verb: if s.preparing {
                "preparing".into()
            } else if s.active_count > 1 {
                "running".into()
            } else {
                tool_action_verb(&s.primary_name).into()
            },
            icon: tool_icon(&s.primary_name).into(),
            detail,
            elapsed_secs: s.elapsed_secs,
        }
    })
}

/// Shelf-aligned generating-tool status for the status bar.
pub fn shelf_generating_status_span(
    turn_activity: &TurnActivityState,
    spinner_frame: usize,
    glyphs: TerminalGlyphProfile,
) -> Option<Span<'static>> {
    if !turn_activity.enabled || turn_activity.phase != ShelfPhase::GeneratingTool {
        return None;
    }
    let active = summarize_tools_for_status(turn_activity)?;
    let spinner = compact_spinner_frame(spinner_frame, glyphs);
    Some(Span::styled(
        format!(
            " {spinner} {} {} {} ",
            active.verb, active.icon, active.detail
        ),
        ratatui::style::Style::default().fg(Color::Rgb(255, 200, 80)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urgency_ramps_to_red() {
        assert_eq!(wait_urgency_color(5), Color::Rgb(255, 210, 120));
        assert_eq!(wait_urgency_color(20), Color::Rgb(255, 140, 50));
        assert_eq!(wait_urgency_color(40), Color::Rgb(239, 83, 80));
    }

    #[test]
    fn thinking_status_includes_spinner() {
        let theme = Theme::default();
        let msg = format_thinking_status(
            &theme,
            StatusIndicatorStyle::Kaomoji,
            TerminalGlyphProfile::Unicode,
            0,
            0,
            0,
            3,
        );
        assert!(msg.contains("thinking"));
    }

    #[test]
    fn format_token_count_scales() {
        assert_eq!(format_token_count(500), "500");
        assert_eq!(format_token_count(1500), "1.5k");
        assert_eq!(format_token_count(1_500_000), "1.5M");
    }

    #[test]
    fn stall_tier_shows_stop_hint() {
        let theme = Theme::default();
        let msg = format_waiting_first_token_status(
            &theme,
            StatusIndicatorStyle::Kaomoji,
            TerminalGlyphProfile::Unicode,
            0,
            0,
            0,
            25,
            "waiting for first token",
        );
        assert!(msg.contains("^C=stop"));
    }
}
