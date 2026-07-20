//! Uniform `RenderEntry` → plain text / ratatui lines (026 Wave B).

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::stream_presentation::DisplayMode;

use super::entries::{RenderEntry, RenderEntryKind};

const DEFAULT_TRUNC_LINES: usize = 6;
const DEFAULT_EXPANDED_CAP_LINES: usize = 80;

#[derive(Debug, Clone, Copy)]
pub struct RenderOpts {
    pub trunc_lines: usize,
    pub expanded_cap_lines: usize,
    pub width: u16,
    /// Dim finished collapsed tools (Grok muted_collapsed).
    pub mute_collapsed: bool,
    pub thinking_fg: Color,
    pub tool_fg: Color,
    pub insert_fg: Color,
    pub delete_fg: Color,
    pub error_fg: Color,
    pub dim_fg: Color,
}

impl Default for RenderOpts {
    fn default() -> Self {
        Self {
            trunc_lines: DEFAULT_TRUNC_LINES,
            expanded_cap_lines: DEFAULT_EXPANDED_CAP_LINES,
            width: 80,
            mute_collapsed: true,
            thinking_fg: Color::Rgb(160, 150, 190),
            tool_fg: Color::Rgb(180, 180, 190),
            insert_fg: Color::Rgb(120, 200, 140),
            delete_fg: Color::Rgb(220, 120, 120),
            error_fg: Color::Rgb(239, 83, 80),
            dim_fg: Color::Rgb(110, 110, 120),
        }
    }
}

/// Plain-text projection for tests / golden dumps.
pub fn render_entry_plain(entry: &RenderEntry, opts: RenderOpts) -> String {
    match entry.mode {
        DisplayMode::Collapsed => {
            if entry.header.is_empty() {
                first_line(&entry.body)
            } else {
                entry.header.clone()
            }
        }
        DisplayMode::Truncated => {
            let body = last_n_lines(&entry.body, opts.trunc_lines);
            if entry.header.is_empty() {
                body
            } else if body.is_empty() {
                entry.header.clone()
            } else {
                format!("{}\n{body}", entry.header)
            }
        }
        DisplayMode::Expanded => {
            let body = first_n_lines(&entry.body, opts.expanded_cap_lines);
            if entry.header.is_empty() {
                body
            } else if body.is_empty() {
                entry.header.clone()
            } else {
                format!("{}\n{body}", entry.header)
            }
        }
    }
}

/// Ratatui lines for transcript paint.
pub fn render_entry_lines(entry: &RenderEntry, opts: RenderOpts) -> Vec<Line<'static>> {
    let plain = render_entry_plain(entry, opts);
    let muted = opts.mute_collapsed && entry.is_muted();
    let base_fg = match &entry.kind {
        RenderEntryKind::Thinking { .. } => opts.thinking_fg,
        RenderEntryKind::Tool { .. } if entry.is_error => opts.error_fg,
        RenderEntryKind::Tool { .. } => {
            if muted {
                opts.dim_fg
            } else {
                opts.tool_fg
            }
        }
        RenderEntryKind::Footer => opts.dim_fg,
        RenderEntryKind::User => Color::Rgb(205, 127, 50),
        RenderEntryKind::AgentMessage | RenderEntryKind::VerbGroup { .. } => opts.tool_fg,
    };

    let mut style = Style::default().fg(base_fg);
    if muted {
        style = style.add_modifier(Modifier::DIM);
    }
    if matches!(entry.kind, RenderEntryKind::Thinking { running: true, .. }) {
        style = style.add_modifier(Modifier::ITALIC);
    }

    plain
        .lines()
        .map(|l| {
            // Light craft: mark +/- for edit bodies when expanded/truncated.
            if l.starts_with('+') && !l.starts_with("+++") {
                Line::from(Span::styled(
                    l.to_string(),
                    Style::default().fg(opts.insert_fg),
                ))
            } else if l.starts_with('-') && !l.starts_with("---") {
                Line::from(Span::styled(
                    l.to_string(),
                    Style::default().fg(opts.delete_fg),
                ))
            } else {
                Line::from(Span::styled(l.to_string(), style))
            }
        })
        .collect()
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").to_string()
}

fn last_n_lines(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() <= max {
        return lines.join("\n");
    }
    let start = lines.len() - max;
    format!("…\n{}", lines[start..].join("\n"))
}

fn first_n_lines(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() <= max {
        return s.to_string();
    }
    format!("{}\n…({} more lines)", lines[..max].join("\n"), lines.len() - max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::entries::{CardStatus, RenderEntry};
    use crate::stream_presentation::{DisplayMode, ToolCardKind};
    use std::time::Duration;

    #[test]
    fn truncated_shows_last_n() {
        let body = (1..=10).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
        let e = RenderEntry::tool(crate::presentation::entries::ToolEntryArgs {
            name: "terminal".into(),
            kind: ToolCardKind::Execute,
            status: CardStatus::Running,
            caption: "$ cargo test".into(),
            body,
            mode: DisplayMode::Truncated,
            duration: None,
            is_error: false,
        });
        let plain = render_entry_plain(&e, RenderOpts { trunc_lines: 3, ..Default::default() });
        assert!(plain.contains("line10"));
        assert!(plain.contains('…'));
        assert!(!plain.contains("line1\n"));
    }

    #[test]
    fn collapsed_is_header_only() {
        let e = RenderEntry::tool(crate::presentation::entries::ToolEntryArgs {
            name: "read_file".into(),
            kind: ToolCardKind::Read,
            status: CardStatus::Success,
            caption: "⊙ Read a.rs".into(),
            body: "secret body".into(),
            mode: DisplayMode::Collapsed,
            duration: Some(Duration::from_millis(10)),
            is_error: false,
        });
        assert_eq!(
            render_entry_plain(&e, RenderOpts::default()),
            "⊙ Read a.rs"
        );
    }
}
