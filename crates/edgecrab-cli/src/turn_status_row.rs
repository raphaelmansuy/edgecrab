//! Turn status row — Grok-style single-line activity chrome (026 Wave A).
//!
//! Layout: `⠧ Running terminal…  12s              Exec 1 · Read 2  [stop]`
//! Hidden when idle (0 height). Sits between activity shelf and the separator.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::status_chrome::{TerminalGlyphProfile, compact_spinner_frame};
use crate::stream_presentation::{ChromePhaseHint, StreamPresentation, format_elapsed_compact};
use crate::theme::Theme;

/// Height of the turn-status row (0 when idle).
pub fn turn_status_height(presentation: &StreamPresentation, is_processing: bool) -> u16 {
    if is_processing && presentation.turn_status_visible() {
        1
    } else {
        0
    }
}

pub struct TurnStatusParams<'a> {
    pub presentation: &'a StreamPresentation,
    pub theme: &'a Theme,
    pub spinner_frame: usize,
    pub glyphs: TerminalGlyphProfile,
    pub animate: bool,
    pub is_processing: bool,
    pub tool_generating: bool,
    pub token_hint: Option<&'a str>,
    pub show_stop_affordance: bool,
}

/// Render the turn-status row into `area` (height must be 1).
pub fn render_turn_status(frame: &mut Frame, area: Rect, params: &TurnStatusParams<'_>) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    if !params.is_processing || !params.presentation.turn_status_visible() {
        return;
    }

    let hint = params
        .presentation
        .chrome_phase_hint(params.tool_generating);
    let label = params
        .presentation
        .phase_activity_label(params.tool_generating);
    let accent = match hint {
        ChromePhaseHint::ToolExec | ChromePhaseHint::GeneratingTool => Color::Rgb(255, 200, 80),
        ChromePhaseHint::Thinking => Color::Rgb(180, 160, 220),
        ChromePhaseHint::Streaming => Color::Rgb(77, 208, 225),
        ChromePhaseHint::AwaitingFirstToken => Color::Rgb(160, 160, 170),
        ChromePhaseHint::Idle => Color::DarkGray,
    };

    let spin = if params.animate {
        compact_spinner_frame(params.spinner_frame, params.glyphs)
    } else {
        "·"
    };

    let elapsed = params
        .presentation
        .turn_started
        .map(|t| format_elapsed_compact(t.elapsed()))
        .unwrap_or_default();

    let usage = params.presentation.tool_usage_caption();
    let ledger = params.presentation.edit_ledger.caption();

    let mut left_parts = vec![format!("{spin} {label}")];
    if !elapsed.is_empty() {
        left_parts.push(elapsed);
    }
    let left = left_parts.join("  ");

    let mut right_parts: Vec<String> = Vec::new();
    if let Some(tok) = params.token_hint.filter(|s| !s.is_empty()) {
        right_parts.push(tok.to_string());
    }
    if let Some(u) = usage {
        right_parts.push(u);
    }
    if let Some(l) = ledger {
        right_parts.push(l);
    }
    if params.show_stop_affordance {
        right_parts.push("[stop]".into());
    }
    let right = right_parts.join("  ");

    let dim = params
        .theme
        .shelf_dim
        .fg
        .unwrap_or(Color::Rgb(120, 120, 130));

    // Fit left + gap + right into width.
    let width = area.width as usize;
    let right_w = right.width();
    let left_budget = width.saturating_sub(right_w.saturating_add(1));
    let left_display = if left.width() > left_budget {
        truncate_to_width(&left, left_budget)
    } else {
        left
    };
    let gap = width
        .saturating_sub(left_display.width())
        .saturating_sub(right_w);
    let pad = " ".repeat(gap);

    let line = Line::from(vec![
        Span::styled(
            left_display,
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ),
        Span::raw(pad),
        Span::styled(right, Style::default().fg(dim).add_modifier(Modifier::DIM)),
    ]);

    frame.render_widget(Paragraph::new(line).alignment(Alignment::Left), area);
}

fn truncate_to_width(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.width() <= max {
        return s.to_string();
    }
    if max <= 1 {
        return "…".into();
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw + 1 > max {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream_presentation::StreamPresentation;

    #[test]
    fn idle_height_is_zero() {
        let p = StreamPresentation::new();
        assert_eq!(turn_status_height(&p, false), 0);
        assert_eq!(turn_status_height(&p, true), 0);
    }

    #[test]
    fn thinking_shows_row() {
        let mut p = StreamPresentation::new();
        p.on_reasoning("plan");
        assert_eq!(turn_status_height(&p, true), 1);
        assert!(p.phase_activity_label(false).contains("Thinking"));
    }
}
