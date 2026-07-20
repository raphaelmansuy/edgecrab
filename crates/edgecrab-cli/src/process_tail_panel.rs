//! `/tail` overlay — read-only view of a background process buffer (Hermes `process.list` parity).

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::overlay_layout::popup_rect;

pub const TAIL_PANEL_MAX_CHARS: usize = 4096;

#[derive(Clone, Debug, Default)]
pub struct ProcessTailPanel {
    pub active: bool,
    pub process_id: String,
    pub body: String,
    pub status_line: String,
    pub scroll_offset: u16,
    /// True when showing a foreground tool live buffer (`t=expand`), not `/tail`.
    pub foreground_live: bool,
}

impl ProcessTailPanel {
    pub fn close(&mut self) {
        *self = Self::default();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn set_content(
        &mut self,
        process_id: String,
        body: String,
        status: &str,
        exit_code: Option<i32>,
    ) {
        let trimmed = truncate_tail_body(&body, TAIL_PANEL_MAX_CHARS);
        let exit = exit_code.map(|c| format!(" exit {c}")).unwrap_or_default();
        self.active = true;
        self.foreground_live = false;
        self.process_id = process_id;
        self.body = trimmed;
        self.status_line = format!("{status}{exit}");
        self.scroll_offset = 0;
    }

    /// Open/update the foreground Focus Tool live overlay (spec 020).
    pub fn set_foreground_live(&mut self, tool_name: &str, preview: &str, body: &str) {
        let at_bottom = self.foreground_live && self.active && self.scroll_offset == 0;
        let trimmed = truncate_tail_body(body, TAIL_PANEL_MAX_CHARS);
        self.active = true;
        self.foreground_live = true;
        self.process_id = tool_name.to_string();
        self.body = trimmed;
        self.status_line = if preview.trim().is_empty() {
            "live".into()
        } else {
            format!("live · {preview}")
        };
        if !at_bottom {
            // Fresh open — start at top of the tail window (newest content is at end;
            // render already shows from scroll_offset; keep 0 = show from start of buffer).
            self.scroll_offset = 0;
        }
    }
}

pub fn truncate_tail_body(body: &str, max_chars: usize) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let start = trimmed
        .char_indices()
        .nth(trimmed.chars().count().saturating_sub(max_chars))
        .map(|(i, _)| i)
        .unwrap_or(0);
    format!("…{}", &trimmed[start..])
}

/// Render the `/tail` popup overlay (Hermes `process.list` tail view parity).
pub fn render_process_tail_panel(
    frame: &mut Frame,
    area: Rect,
    panel: &ProcessTailPanel,
    accent: Color,
    dim: Color,
) {
    frame.render_widget(Clear, area);
    let pw = (area.width * 9 / 10).max(20);
    let ph = (area.height * 4 / 5).max(8);
    let popup = popup_rect(area, pw, ph);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent))
        .title(format!(
            " {}: {} — {} ",
            if panel.foreground_live {
                "live tool"
            } else {
                "tail"
            },
            panel.process_id,
            panel.status_line
        ));
    let lines: Vec<&str> = panel.body.lines().collect();
    let scroll = panel.scroll_offset as usize;
    let visible_height = chunks[0].height as usize;
    let visible: String = lines
        .iter()
        .skip(scroll)
        .take(visible_height)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    let paragraph = Paragraph::new(visible)
        .block(block)
        .style(Style::default().fg(dim))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, chunks[0]);
    let help = Paragraph::new(Line::from(vec![
        Span::styled(" ↑↓ ", Style::default().fg(accent)),
        Span::styled("scroll  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Esc ", Style::default().fg(accent)),
        Span::styled("close", Style::default().fg(Color::DarkGray)),
    ]));
    frame.render_widget(help, chunks[1]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_tail() {
        let body = "x".repeat(5000);
        let out = truncate_tail_body(&body, 100);
        assert!(out.starts_with('…'));
        assert!(out.chars().count() <= 101);
    }

    #[test]
    fn foreground_live_sets_flag() {
        let mut panel = ProcessTailPanel::default();
        panel.set_foreground_live("terminal", "npm install", "line1\nline2");
        assert!(panel.active);
        assert!(panel.foreground_live);
        assert_eq!(panel.process_id, "terminal");
        assert!(panel.body.contains("line2"));
        panel.close();
        assert!(!panel.active);
        assert!(!panel.foreground_live);
    }
}
