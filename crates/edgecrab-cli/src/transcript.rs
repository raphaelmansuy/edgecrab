//! Transcript rendering — scrollable output area (extracted from `app.rs`).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::display_state::DisplayState;
use crate::markdown_render;
use crate::status_chrome::{TerminalGlyphProfile, compact_spinner_frame};
use crate::theme::palette as P;
use crate::transcript_heights::TranscriptHeightCache;

/// A single line in the output area with a semantic role.
#[derive(Clone)]
pub struct OutputLine {
    pub text: String,
    pub role: OutputRole,
    /// Pre-built ratatui spans (for tool-done lines with emoji).
    pub prebuilt_spans: Option<Vec<Span<'static>>>,
    /// Full tool result body for Ctrl+Shift+T expand (terminal / execute_code).
    pub expandable_body: Option<String>,
    /// Whether [`expandable_body`] is shown instead of the collapsed spans.
    pub expanded: bool,
    pub(crate) collapsed_prebuilt_spans: Option<Vec<Span<'static>>>,
    pub(crate) rendered: Option<Vec<Line<'static>>>,
    pub(crate) plain_rendered: Option<Vec<Line<'static>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OutputRole {
    Assistant,
    Tool,
    System,
    Reasoning,
    Error,
    User,
}

impl OutputLine {
    pub(crate) fn new_text(text: impl Into<String>, role: OutputRole) -> Self {
        Self {
            text: text.into(),
            role,
            prebuilt_spans: None,
            expandable_body: None,
            expanded: false,
            collapsed_prebuilt_spans: None,
            rendered: None,
            plain_rendered: None,
        }
    }

    pub(crate) fn new_spans(spans: Vec<Span<'static>>, role: OutputRole) -> Self {
        Self {
            text: String::new(),
            role,
            prebuilt_spans: Some(spans),
            expandable_body: None,
            expanded: false,
            collapsed_prebuilt_spans: None,
            rendered: None,
            plain_rendered: None,
        }
    }

    pub(crate) fn invalidate_render_cache(&mut self) {
        self.rendered = None;
        self.plain_rendered = None;
    }

    pub(crate) fn attach_expandable_body(&mut self, body: String) {
        let body = crate::transcript_heights::truncate_verbose_trail(&body);
        if body.is_empty() || body.lines().count() <= 3 {
            return;
        }
        self.expandable_body = Some(body);
    }

    pub fn toggle_expand(&mut self) -> bool {
        let Some(body) = self.expandable_body.clone() else {
            return false;
        };
        self.expanded = !self.expanded;
        if self.expanded {
            self.collapsed_prebuilt_spans = self.prebuilt_spans.take();
            self.text = body;
            self.prebuilt_spans = None;
        } else {
            self.text.clear();
            self.prebuilt_spans = self.collapsed_prebuilt_spans.take();
        }
        self.invalidate_render_cache();
        true
    }
}

/// Scroll metrics updated during transcript render.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TranscriptScrollMetrics {
    pub scroll_offset: u16,
    pub output_visual_rows: u16,
    pub output_area_height: u16,
    pub at_bottom: bool,
}

pub fn paging_scroll_hint(scroll: u16, ascii: bool, paging_key: &str) -> String {
    if ascii {
        format!(" scroll:{scroll} ^G=end {paging_key} ")
    } else {
        format!(" ↑{scroll}  ^G=end  {paging_key} ")
    }
}

pub struct TranscriptRenderParams<'a> {
    pub output: &'a mut [OutputLine],
    pub transcript_heights: &'a mut TranscriptHeightCache,
    pub rich_transcript: bool,
    pub display_state: &'a DisplayState,
    pub reasoning_line: Option<usize>,
    pub terminal_glyph_profile: TerminalGlyphProfile,
    pub show_output_scrollbar: bool,
    pub paging_key_hint: &'a str,
    /// Optional detail for blocking non-streaming LLM waits (LM Studio shelf notice).
    pub llm_wait_label: Option<&'a str>,
}

pub fn render_transcript_rich(
    frame: &mut Frame,
    area: Rect,
    params: &mut TranscriptRenderParams<'_>,
    metrics: &mut TranscriptScrollMetrics,
) {
    if !params.rich_transcript {
        render_transcript_compact(frame, area, params, metrics);
        return;
    }

    // ── Pass 1: ensure every OutputLine has a cached render ──────
    for (idx, ol) in params.output.iter_mut().enumerate() {
        let _ = params.transcript_heights.get_or_insert(
            idx as u64,
            area.width.saturating_sub(4),
            &ol.text,
        );
        if ol.rendered.is_none() {
            let rendered = if let Some(ref spans) = ol.prebuilt_spans {
                // Pre-built spans (tool-done lines with emoji) — use directly.
                // Ratatui measures each Span's display width via unicode-width,
                // so emoji and wide characters align correctly.
                vec![Line::from(spans.clone())]
            } else if ol.role == OutputRole::Assistant {
                markdown_render::render_markdown(&ol.text)
            } else {
                let style = match ol.role {
                    OutputRole::Assistant => unreachable!(),
                    OutputRole::Tool => Style::default()
                        .fg(Color::Rgb(255, 191, 0))
                        .add_modifier(Modifier::DIM),
                    OutputRole::System => Style::default()
                        .fg(Color::Rgb(140, 140, 150))
                        .add_modifier(Modifier::ITALIC),
                    OutputRole::Reasoning => Style::default()
                        .fg(Color::Rgb(170, 170, 190))
                        .add_modifier(Modifier::ITALIC | Modifier::DIM),
                    OutputRole::Error => Style::default().fg(Color::Rgb(239, 83, 80)),
                    OutputRole::User => Style::default().fg(Color::Rgb(255, 248, 220)),
                };
                ol.text
                    .lines()
                    .map(|l| Line::from(Span::styled(l.to_string(), style)))
                    .collect()
            };
            ol.rendered = Some(rendered);
        }
    }

    // ── Pass 2: build visual lines with role bars + turn separators ─
    // Each message gets a 2-char left accent: coloured "▎ " for most roles,
    // "· " (dimmed dot) for system messages. User messages get a thin
    // horizontal rule injected before them (except the very first).
    let sep_style = Style::default()
        .fg(Color::Rgb(45, 45, 58))
        .add_modifier(Modifier::DIM);
    // Dynamic separator width: fill the content column minus bar + scrollbar
    let sep_width = (area.width.saturating_sub(4) as usize).max(10);

    let mut lines: Vec<Line<'static>> = Vec::new();
    for (idx, ol) in params.output.iter().enumerate() {
        // Turn separator: thin rule before each user message that follows
        // at least one other message (marks start of a new conversation turn).
        if ol.role == OutputRole::User && idx > 0 {
            // Blank line + subtle separator rule
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled("─".repeat(sep_width), sep_style),
            ]));
            lines.push(Line::from(""));
        }

        // Role bar: the 2-char left accent column
        let (bar, bar_style): (&'static str, Style) = match ol.role {
            OutputRole::User => ("▎ ", Style::default().fg(Color::Rgb(255, 248, 220))),
            OutputRole::Assistant => ("▎ ", Style::default().fg(Color::Rgb(77, 208, 225))),
            OutputRole::Tool => ("▎ ", Style::default().fg(Color::Rgb(255, 191, 0))),
            OutputRole::Error => ("▎ ", Style::default().fg(Color::Rgb(239, 83, 80))),
            OutputRole::System => (". ", Style::default().fg(Color::Rgb(60, 60, 72))),
            OutputRole::Reasoning => ("~ ", Style::default().fg(Color::Rgb(95, 95, 115))),
        };

        // Prepend bar to every rendered sub-line
        for rendered_line in ol.rendered.as_ref().unwrap() {
            let mut spans: Vec<Span<'static>> = vec![Span::styled(bar, bar_style)];
            spans.extend(rendered_line.spans.clone());
            lines.push(Line::from(spans));
        }
    }

    // ── Ghost waiting line (FP45) ─────────────────────────────────
    // During AwaitingFirstToken and Thinking (when no reasoning output is
    // yet visible), inject a dim pulsing line at the bottom of the content
    // area. This puts the waiting indicator AT THE USER'S FOCAL POINT
    // (bottom of the conversation) rather than only in the peripheral
    // status bar. The ghost line disappears naturally once real tokens arrive.
    match params.display_state {
            DisplayState::AwaitingFirstToken { frame, started } => {
                let spinner = compact_spinner_frame(*frame, params.terminal_glyph_profile);
                let elapsed = started.elapsed().as_secs();
                let ghost_text: String = if let Some(label) = params.llm_wait_label {
                    if elapsed > 10 {
                        format!(
                            "  {spinner}  {}  {elapsed}s  (^C to stop)",
                            edgecrab_core::safe_truncate(label, 64)
                        )
                    } else if elapsed > 3 {
                        format!(
                            "  {spinner}  {}  {elapsed}s",
                            edgecrab_core::safe_truncate(label, 64)
                        )
                    } else {
                        format!(
                            "  {spinner}  {}",
                            edgecrab_core::safe_truncate(label, 64)
                        )
                    }
                } else if elapsed > 10 {
                    format!("  {spinner}  awaiting model response\u{2026}  {elapsed}s  (^C to stop)")
                } else if elapsed > 3 {
                    format!("  {spinner}  awaiting model response\u{2026}  {elapsed}s")
                } else {
                    format!("  {spinner}  awaiting model response\u{2026}")
                };
                lines.push(Line::from(vec![
                    Span::styled(
                        "\u{258e} ",
                        Style::default()
                            .fg(P::GUTTER_BAR)
                            .add_modifier(Modifier::DIM), // decorative glyph — DIM OK
                    ),
                    Span::styled(
                        ghost_text,
                        Style::default()
                            // WCAG AA: P::TERTIARY_COOL Rgb(125,138,162) CR=6.0:1.
                            // DIM removed — this is semantic text the user must read.
                            .fg(P::TERTIARY_COOL)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ]));
            }
            DisplayState::Thinking { frame, started }
                // Only show when reasoning is not already streaming in the output
                // area (show_reasoning=true with tokens arriving). When
                // reasoning_line is Some, the user already sees live reasoning
                // text — adding a ghost line would duplicate the signal.
                if params.reasoning_line.is_none() => {
                    let spinner = compact_spinner_frame(*frame, params.terminal_glyph_profile);
                    let elapsed = started.elapsed().as_secs();
                    let ghost_text: String = if elapsed > 3 {
                        format!("  {spinner}  thinking\u{2026}  {elapsed}s")
                    } else {
                        format!("  {spinner}  thinking\u{2026}")
                    };
                    lines.push(Line::from(vec![
                        Span::styled(
                            "\u{258e} ",
                            Style::default()
                                .fg(P::GUTTER_BAR)
                                .add_modifier(Modifier::DIM), // decorative glyph — DIM OK
                        ),
                        Span::styled(
                            ghost_text,
                            Style::default()
                                // WCAG AA: P::SECONDARY_WARM CR=9.2:1. DIM removed.
                                .fg(P::SECONDARY_WARM)
                                .add_modifier(Modifier::ITALIC),
                        ),
                    ]));
                }
            _ => {}
        }

    // ── Scroll math ───────────────────────────────────────────────
    //
    // Scrollbar is on the LEFT (1 col).  Content starts at x+1.
    // WHY left: the content's natural reading edge is the right margin;
    // placing the scroll indicator on the left avoids it competing with
    // text flow and emoji that may appear near the right edge.
    //   area.x  ← scrollbar (1 col)
    //   area.x+1 .. area.right()  ← text content (width − 1 cols)
    //
    // content_width: used for word-wrap row count estimation.
    // Subtract 4 = 1 (scrollbar) + 1 (gap) + 2 (role bar "▎ ").
    let content_width = area.width.saturating_sub(4) as usize;

    let visual_rows: u16 = if content_width == 0 {
        lines.len() as u16
    } else {
        lines
            .iter()
            .map(|l| {
                let w = l.width();
                if w == 0 {
                    1u16
                } else {
                    w.div_ceil(content_width) as u16
                }
            })
            .sum()
    };

    let visible_height = area.height;
    let max_scroll = visual_rows.saturating_sub(visible_height);
    if metrics.scroll_offset > max_scroll {
        metrics.scroll_offset = max_scroll;
    }
    let scroll = metrics.scroll_offset;

    metrics.output_visual_rows = visual_rows;
    metrics.output_area_height = visible_height;
    metrics.at_bottom = scroll == 0;

    let top_row = visual_rows.saturating_sub(visible_height + scroll);

    // ── Render: scrollbar LEFT, 1-col gap, then content ──────────
    let scrollbar_area = Rect {
        x: area.x,
        y: area.y,
        width: 1,
        height: area.height,
    };
    // Content column: skip 1 col (scrollbar) + 1 col (breathing gap).
    let content_area = Rect {
        x: area.x + 2,
        y: area.y,
        width: area.width.saturating_sub(2),
        height: area.height,
    };

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .scroll((top_row, 0));
    frame.render_widget(paragraph, content_area);

    if visual_rows > visible_height {
        let scrollbar_pos = max_scroll.saturating_sub(scroll) as usize;
        let mut scrollbar_state = ScrollbarState::new(max_scroll as usize).position(scrollbar_pos);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalLeft)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("│"))
                .thumb_symbol("█"),
            scrollbar_area,
            &mut scrollbar_state,
        );
    }

    // "Scrolled ↑" hint — anchored to right edge of the content area
    // (not the scrollbar edge) so it stays readable.
    if scroll > 0 {
        let hint = format!(" ↑{}  ^G=end  ↕scroll  {} ", scroll, params.paging_key_hint);
        let hint_len = hint
            .len()
            .min(content_area.width.saturating_sub(1) as usize);
        let hint_x = content_area.x + content_area.width.saturating_sub(hint_len as u16);
        let hint_area = Rect::new(hint_x, area.y, hint_len as u16, 1);
        frame.render_widget(
            Paragraph::new(Span::styled(
                hint,
                Style::default()
                    .fg(Color::Rgb(255, 210, 50))
                    .bg(Color::Rgb(30, 30, 38))
                    .add_modifier(Modifier::BOLD),
            )),
            hint_area,
        );
    }
}

pub fn render_transcript_compact(
    frame: &mut Frame,
    area: Rect,
    params: &mut TranscriptRenderParams<'_>,
    metrics: &mut TranscriptScrollMetrics,
) {
    let glyphs = params.terminal_glyph_profile;
    let flatten_spans = |spans: &[Span<'static>]| -> String {
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    };
    let line_style = |role: OutputRole| match role {
        OutputRole::Assistant => Style::default().fg(Color::Rgb(220, 228, 235)),
        OutputRole::Tool => Style::default().fg(Color::Rgb(235, 200, 120)),
        OutputRole::System => Style::default()
            .fg(Color::Rgb(155, 165, 175))
            .add_modifier(Modifier::DIM),
        OutputRole::Reasoning => Style::default()
            .fg(Color::Rgb(145, 150, 170))
            .add_modifier(Modifier::DIM),
        OutputRole::Error => Style::default().fg(Color::Rgb(239, 83, 80)),
        OutputRole::User => Style::default().fg(Color::Rgb(255, 248, 220)),
    };
    let prefix_for = |role: OutputRole, ascii: bool| match (role, ascii) {
        (OutputRole::Tool, true) => "[tool] ",
        (OutputRole::Tool, false) => "› ",
        (OutputRole::System, true) => "[note] ",
        (OutputRole::System, false) => "· ",
        (OutputRole::Reasoning, true) => "[think] ",
        (OutputRole::Reasoning, false) => "~ ",
        (OutputRole::Error, true) => "[error] ",
        (OutputRole::Error, false) => "! ",
        _ => "",
    };

    for ol in params.output.iter_mut() {
        if ol.plain_rendered.is_none() {
            let style = line_style(ol.role);
            let prefix = prefix_for(ol.role, matches!(glyphs, TerminalGlyphProfile::Ascii));
            let prefix_width = prefix.width();
            let text = ol
                .prebuilt_spans
                .as_ref()
                .map(|spans| flatten_spans(spans))
                .unwrap_or_else(|| ol.text.clone());
            let source_lines = if text.is_empty() {
                vec![String::new()]
            } else {
                text.lines().map(str::to_string).collect::<Vec<_>>()
            };
            let mut rendered_lines = Vec::with_capacity(source_lines.len());
            for (idx, content) in source_lines.into_iter().enumerate() {
                let leader = if idx == 0 {
                    prefix.to_string()
                } else if prefix_width == 0 {
                    String::new()
                } else {
                    " ".repeat(prefix_width)
                };
                rendered_lines.push(Line::from(vec![
                    Span::styled(leader, style),
                    Span::styled(content, style),
                ]));
            }
            ol.plain_rendered = Some(rendered_lines);
        }
    }

    let mut lines: Vec<Line<'static>> = Vec::new();
    for ol in params.output.iter() {
        if let Some(rendered) = &ol.plain_rendered {
            lines.extend(rendered.clone());
        }
    }

    // ── Ghost waiting line (FP45) compact variant ─────────────────
    match params.display_state {
        DisplayState::AwaitingFirstToken { frame, started } => {
            let spinner = compact_spinner_frame(*frame, glyphs);
            let elapsed = started.elapsed().as_secs();
            let ghost: String = if let Some(label) = params.llm_wait_label {
                format!(
                    "  {spinner}  {}  {elapsed}s",
                    edgecrab_core::safe_truncate(label, 48)
                )
            } else if elapsed > 3 {
                format!("  {spinner}  awaiting\u{2026}  {elapsed}s")
            } else {
                format!("  {spinner}  awaiting\u{2026}")
            };
            lines.push(Line::from(Span::styled(
                ghost,
                Style::default()
                    // WCAG AA: P::TERTIARY_COOL CR=6.0:1. DIM removed.
                    .fg(P::TERTIARY_COOL)
                    .add_modifier(Modifier::ITALIC),
            )));
        }
        DisplayState::Thinking { frame, started } if params.reasoning_line.is_none() => {
            let spinner = compact_spinner_frame(*frame, glyphs);
            let elapsed = started.elapsed().as_secs();
            let ghost: String = if elapsed > 3 {
                format!("  {spinner}  thinking\u{2026}  {elapsed}s")
            } else {
                format!("  {spinner}  thinking\u{2026}")
            };
            lines.push(Line::from(Span::styled(
                ghost,
                Style::default()
                    // WCAG AA: P::SECONDARY_WARM CR=9.2:1 (thinking = higher priority signal).
                    // DIM removed.
                    .fg(P::SECONDARY_WARM)
                    .add_modifier(Modifier::ITALIC),
            )));
        }
        _ => {}
    }

    let content_width = area.width.max(1) as usize;
    let visual_rows: u16 = lines
        .iter()
        .map(|line| {
            let width = line.width();
            if width == 0 {
                1
            } else {
                width.div_ceil(content_width) as u16
            }
        })
        .sum();
    let visible_height = area.height;
    let max_scroll = visual_rows.saturating_sub(visible_height);
    if metrics.scroll_offset > max_scroll {
        metrics.scroll_offset = max_scroll;
    }
    let scroll = metrics.scroll_offset;
    metrics.output_visual_rows = visual_rows;
    metrics.output_area_height = visible_height;
    metrics.at_bottom = scroll == 0;

    let top_row = visual_rows.saturating_sub(visible_height + scroll);
    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .scroll((top_row, 0));
    frame.render_widget(paragraph, area);

    if params.show_output_scrollbar && visual_rows > visible_height && area.width > 1 {
        let scrollbar_area = Rect {
            x: area.right().saturating_sub(1),
            y: area.y,
            width: 1,
            height: area.height,
        };
        let scrollbar_pos = max_scroll.saturating_sub(scroll) as usize;
        let mut scrollbar_state = ScrollbarState::new(max_scroll as usize).position(scrollbar_pos);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .track_symbol(Some("|"))
                .thumb_symbol("#"),
            scrollbar_area,
            &mut scrollbar_state,
        );
    }

    if scroll > 0 && area.width > 12 {
        let hint = paging_scroll_hint(
            scroll,
            matches!(glyphs, TerminalGlyphProfile::Ascii),
            params.paging_key_hint,
        );
        let hint_width = hint.width().min(area.width as usize) as u16;
        let hint_area = Rect::new(
            area.right().saturating_sub(hint_width),
            area.y,
            hint_width,
            1,
        );
        frame.render_widget(
            Paragraph::new(Span::styled(
                hint,
                Style::default()
                    .fg(Color::Rgb(240, 210, 120))
                    .bg(Color::Rgb(30, 30, 38))
                    .add_modifier(Modifier::BOLD),
            )),
            hint_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paging_scroll_hint_ascii_and_unicode() {
        assert!(paging_scroll_hint(3, true, "PgUp").contains("scroll:3"));
        assert!(paging_scroll_hint(3, false, "PgUp").contains("↑3"));
    }

    #[test]
    fn output_line_toggle_expand_roundtrip() {
        let mut line = OutputLine::new_spans(vec![Span::raw("collapsed")], OutputRole::Tool);
        line.attach_expandable_body("a\nb\nc\nd".into());
        assert!(line.toggle_expand());
        assert!(line.expanded);
        assert!(line.toggle_expand());
        assert!(!line.expanded);
    }
}
