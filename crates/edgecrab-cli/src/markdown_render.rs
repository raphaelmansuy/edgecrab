//! # markdown_render — Lightweight Markdown → ratatui Text converter
//!
//! Converts raw markdown text into styled ratatui `Line` objects for display
//! in the output area. Handles: headers, bold, italic, inline code, code blocks
//! (with `│` prefix), lists, blockquotes, and horizontal rules.
//!
//! No external crate — just line-by-line + inline regex substitution.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::theme::palette as P;

/// Render a markdown string into a vector of styled ratatui Lines.
pub fn render_markdown(text: &str) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut in_code_block = false;

    for raw_line in text.lines() {
        if raw_line.starts_with("```") {
            if !in_code_block {
                // Opening fence — extract language hint (e.g. "rust", "python")
                in_code_block = true;
                let lang = raw_line.trim_start_matches('`').trim();
                lines.push(Line::from(""));
                if !lang.is_empty() {
                    // Language badge: ─── rust ──────
                    // Rgb(205,150,60) base CR=8.0:1 passes AA even with DIM; kept as-is.
                    let badge_style = Style::default()
                        .fg(Color::Rgb(205, 150, 60))
                        .add_modifier(Modifier::DIM);
                    let badge_text = format!("  ─── {lang} ");
                    lines.push(Line::from(Span::styled(badge_text, badge_style)));
                }
            } else {
                // Closing fence
                in_code_block = false;
                lines.push(Line::from(""));
            }
            continue;
        }

        if in_code_block {
            // Code block lines: vertical bar prefix indicates code extent.
            // WCAG AA: code body white text unchanged. Bar upgraded to TERTIARY_WARM
            // (Rgb(128,138,152) CR=5.9:1); DIM removed from structural glyph.
            let code_style = Style::default()
                .fg(Color::Rgb(200, 200, 200))
                .add_modifier(Modifier::DIM);
            // WCAG AA: TERTIARY_WARM — code block bar glyph "\u2502" is structural.
            let bar_style = Style::default().fg(P::TERTIARY_WARM);
            lines.push(Line::from(vec![
                Span::styled("  │ ", bar_style),
                Span::styled(raw_line.to_string(), code_style),
            ]));
            continue;
        }

        // Horizontal rule — purely decorative (WCAG SC 1.4.3 exempts decoration).
        if raw_line.trim() == "---" || raw_line.trim() == "***" || raw_line.trim() == "___" {
            let rule = "─".repeat(60);
            lines.push(Line::from(Span::styled(
                rule,
                // Rgb(60,60,70) — decorative; DIM is acceptable for decoration.
                Style::default().fg(P::SEP_LINE).add_modifier(Modifier::DIM),
            )));
            continue;
        }

        // Headers
        if let Some(rest) = raw_line.strip_prefix("### ") {
            lines.push(Line::from(Span::styled(
                format!("   {rest}"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(rest) = raw_line.strip_prefix("## ") {
            lines.push(Line::from(Span::styled(
                format!("  {rest}"),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }
        if let Some(rest) = raw_line.strip_prefix("# ") {
            lines.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        // Blockquote
        if let Some(rest) = raw_line.strip_prefix("> ") {
            // WCAG AA: TERTIARY_COOL for blockquote bar glyph; DIM removed.
            let bar_style = Style::default().fg(P::TERTIARY_COOL);
            let text_style = Style::default()
                .fg(Color::Rgb(180, 180, 180))
                .add_modifier(Modifier::ITALIC);
            lines.push(Line::from(vec![
                Span::styled("  ▎ ", bar_style),
                Span::styled(rest.to_string(), text_style),
            ]));
            continue;
        }

        // List items
        if let Some(rest) = raw_line.strip_prefix("- ") {
            lines.push(render_inline_spans(&format!("  • {rest}")));
            continue;
        }
        if let Some(rest) = raw_line.strip_prefix("* ") {
            lines.push(render_inline_spans(&format!("  • {rest}")));
            continue;
        }
        // Numbered lists — keep as-is with inline formatting
        if raw_line.len() > 2
            && raw_line.chars().next().is_some_and(|c| c.is_ascii_digit())
            && raw_line.contains(". ")
        {
            lines.push(render_inline_spans(&format!("  {raw_line}")));
            continue;
        }

        // Regular text with inline formatting
        lines.push(render_inline_spans(raw_line));
    }

    lines
}

/// Render inline markdown formatting: **bold**, *italic*, `inline code`.
fn render_inline_spans(text: &str) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        // Try bold **text**
        if let Some(start) = remaining.find("**") {
            if start > 0 {
                spans.push(Span::raw(remaining[..start].to_string()));
            }
            let after_start = &remaining[start + 2..];
            if let Some(end) = after_start.find("**") {
                spans.push(Span::styled(
                    after_start[..end].to_string(),
                    Style::default().add_modifier(Modifier::BOLD),
                ));
                remaining = &after_start[end + 2..];
                continue;
            } else {
                // No closing **, treat as literal
                spans.push(Span::raw(remaining[..start + 2].to_string()));
                remaining = &remaining[start + 2..];
                continue;
            }
        }

        // Try inline code `text`
        if let Some(start) = remaining.find('`') {
            if start > 0 {
                spans.push(Span::raw(remaining[..start].to_string()));
            }
            let after_start = &remaining[start + 1..];
            if let Some(end) = after_start.find('`') {
                spans.push(Span::styled(
                    after_start[..end].to_string(),
                    Style::default().fg(Color::Yellow),
                ));
                remaining = &after_start[end + 1..];
                continue;
            } else {
                spans.push(Span::raw(remaining[..start + 1].to_string()));
                remaining = &remaining[start + 1..];
                continue;
            }
        }

        // Try italic *text* (single asterisk, not double)
        if let Some(start) = remaining.find('*') {
            if start > 0 {
                spans.push(Span::raw(remaining[..start].to_string()));
            }
            let after_start = &remaining[start + 1..];
            if let Some(end) = after_start.find('*') {
                spans.push(Span::styled(
                    after_start[..end].to_string(),
                    Style::default().add_modifier(Modifier::ITALIC),
                ));
                remaining = &after_start[end + 1..];
                continue;
            } else {
                spans.push(Span::raw(remaining[..start + 1].to_string()));
                remaining = &remaining[start + 1..];
                continue;
            }
        }

        // No more inline markers — push rest as-is
        spans.push(Span::raw(remaining.to_string()));
        break;
    }

    if spans.is_empty() {
        Line::from("")
    } else {
        Line::from(spans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_block_renders_with_bar_prefix() {
        let md = "before\n```rust\nfn main() {}\n```\nafter";
        let lines = render_markdown(md);
        // Find the code line
        let code_line = lines
            .iter()
            .find(|l| l.spans.iter().any(|s| s.content.contains("fn main")));
        assert!(code_line.is_some(), "should find code line");
        let code_line = code_line.unwrap();
        assert!(
            code_line
                .spans
                .first()
                .is_some_and(|s| s.content.contains('│')),
            "code line should start with │"
        );
    }

    #[test]
    fn header_renders_cyan_bold() {
        let lines = render_markdown("# Hello World");
        assert_eq!(lines.len(), 1);
        let span = &lines[0].spans[0];
        assert!(span.content.contains("Hello World"));
        assert_eq!(span.style.fg, Some(Color::Cyan));
    }

    #[test]
    fn bold_text_renders() {
        let line = render_inline_spans("this is **bold** text");
        let bold_span = line.spans.iter().find(|s| s.content == "bold");
        assert!(bold_span.is_some());
        assert!(
            bold_span
                .unwrap()
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn inline_code_renders_yellow() {
        let line = render_inline_spans("use the `command` here");
        let code_span = line.spans.iter().find(|s| s.content == "command");
        assert!(code_span.is_some());
        assert_eq!(code_span.unwrap().style.fg, Some(Color::Yellow));
    }

    #[test]
    fn blockquote_renders_with_bar() {
        let lines = render_markdown("> some quote");
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0]
                .spans
                .first()
                .is_some_and(|s| s.content.contains('▎'))
        );
    }

    #[test]
    fn list_item_renders_with_bullet() {
        let lines = render_markdown("- item one\n- item two");
        assert_eq!(lines.len(), 2);
        assert!(lines[0].spans.iter().any(|s| s.content.contains('•')));
    }

    #[test]
    fn horizontal_rule_renders_dashes() {
        let lines = render_markdown("---");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans[0].content.contains('─'));
    }
}
