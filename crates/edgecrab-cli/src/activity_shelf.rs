//! Activity shelf renderer — live turn state between transcript and status bar.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use edgecrab_core::safe_truncate;

use crate::shelf_details::{SectionRender, ShelfDetailsState, ShelfSection};
use crate::shelf_visual::{
    elapsed_heat, fmt_duration, format_recent_tools, format_tokens_label, format_tokens_total,
    heat_color, section_chevron, sparkline,
};
use crate::theme::Theme;
use crate::tool_display::{tool_icon, tool_status_preview};
use crate::turn_activity::{
    ActivityNotice, ActivityTone, SHELF_BG_TAIL_CHARS, SHELF_MAX_TOOL_ROWS,
    SHELF_MAX_TOOL_ROWS_FULL, ShelfPhase, TurnActivityState,
};

const MAX_SHELF_LINES: u16 = 8;

fn shelf_tool_row_cap(render: SectionRender) -> usize {
    match render {
        SectionRender::Full => SHELF_MAX_TOOL_ROWS_FULL,
        _ => SHELF_MAX_TOOL_ROWS,
    }
}

/// Render parameters — keeps the shelf renderer under the clippy arg limit.
pub struct ShelfRenderParams<'a> {
    pub state: &'a TurnActivityState,
    pub details: &'a ShelfDetailsState,
    pub theme: &'a Theme,
    pub compact: bool,
    pub spinner_frame: usize,
    pub animate: bool,
    pub verbose_tools: bool,
}

/// Resolved shelf colors from theme (avoids repeating color args).
struct ShelfPalette {
    accent: Color,
    dim: Color,
    warn: Color,
    hot: Color,
    border: Color,
}

impl ShelfPalette {
    fn from_theme(theme: &Theme) -> Self {
        Self {
            accent: theme.shelf_accent.fg.unwrap_or(Color::Rgb(205, 175, 50)),
            dim: theme.shelf_dim.fg.unwrap_or(Color::DarkGray),
            warn: theme.shelf_hint.fg.unwrap_or(Color::Yellow),
            hot: theme.output_error.fg.unwrap_or(Color::Red),
            border: theme.shelf_border,
        }
    }
}

pub fn estimate_shelf_lines(
    state: &TurnActivityState,
    details: &ShelfDetailsState,
    compact: bool,
    verbose_tools: bool,
    is_processing: bool,
) -> u16 {
    if !state.visible(is_processing) {
        return 0;
    }
    if compact {
        return 1.max(state.minimum_shelf_lines(is_processing));
    }
    let lines = count_section_lines(state, details, verbose_tools);
    lines
        .max(state.minimum_shelf_lines(is_processing))
        .min(MAX_SHELF_LINES)
}

pub fn render_activity_shelf(frame: &mut Frame, area: Rect, params: &ShelfRenderParams<'_>) {
    let state = params.state;
    let details = params.details;
    let theme = params.theme;
    let compact = params.compact;
    let spinner_frame = params.spinner_frame;
    let animate = params.animate;
    let verbose_tools = params.verbose_tools;

    if area.height == 0 || area.width == 0 {
        return;
    }

    let palette = ShelfPalette::from_theme(theme);
    let accent = palette.accent;
    let dim = palette.dim;
    let warn = palette.warn;
    let error = palette.hot;
    let border = palette.border;

    let mut lines: Vec<Line> = Vec::new();
    let spin = crate::tui_spinner::shelf_spinner_glyph(state, spinner_frame, animate);

    if compact {
        if let Some(summary) = compact_summary(state) {
            lines.push(Line::from(vec![
                Span::styled(format!("{spin} "), Style::default().fg(accent)),
                Span::styled(summary, Style::default().fg(dim)),
            ]));
        }
    } else {
        append_thinking_lines(&mut lines, state, details, spin, &palette);
        append_activity_lines(&mut lines, state, details, &palette);
        append_tool_lines(&mut lines, state, details, verbose_tools, &palette);
        append_subagent_lines(&mut lines, state, details, &palette);
        append_tokens_footer(&mut lines, state, &palette);
    }

    if lines.is_empty() {
        if let Some(caption) = state.live_caption() {
            append_live_backstop(&mut lines, &caption, spin, accent, dim);
        } else if details.all_sections_hidden() {
            append_quiet_mode_backstop(&mut lines, state, warn, error);
        }
    }

    if lines.is_empty() {
        return;
    }

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(border))
        .title(Span::styled(
            " live ",
            Style::default().fg(dim).add_modifier(Modifier::DIM),
        ));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn count_section_lines(
    state: &TurnActivityState,
    details: &ShelfDetailsState,
    verbose_tools: bool,
) -> u16 {
    let mut n = 0u16;
    n += thinking_line_count(state, details);
    n += activity_line_count(state, details);
    n += tool_line_count(state, details, verbose_tools);
    n += subagent_line_count(state, details);
    n += tokens_footer_line_count(state);
    n
}

fn tokens_footer_line_count(state: &TurnActivityState) -> u16 {
    if format_tokens_total(state.thinking_token_est, state.tool_token_acc).is_some() {
        1
    } else {
        0
    }
}

fn thinking_render_mode(state: &TurnActivityState, details: &ShelfDetailsState) -> SectionRender {
    let has_snippet = state
        .reasoning_snippet
        .as_ref()
        .is_some_and(|s| !s.trim().is_empty());
    details.effective_thinking_render(has_snippet)
}

fn thinking_line_count(state: &TurnActivityState, details: &ShelfDetailsState) -> u16 {
    if thinking_content(state, details).is_none() {
        return 0;
    }
    match thinking_render_mode(state, details) {
        SectionRender::Skip => 0,
        SectionRender::Summary | SectionRender::Full => 1,
    }
}

fn activity_line_count(state: &TurnActivityState, details: &ShelfDetailsState) -> u16 {
    let notices = visible_notices(state, details);
    match details.section_render(ShelfSection::Activity) {
        SectionRender::Skip => notices
            .filter(|n| matches!(n.tone, ActivityTone::Warn | ActivityTone::Error))
            .count() as u16,
        SectionRender::Summary => notices.take(1).count() as u16,
        SectionRender::Full => notices.count() as u16,
    }
}

fn tool_line_count(
    state: &TurnActivityState,
    details: &ShelfDetailsState,
    verbose_tools: bool,
) -> u16 {
    let active = state.sorted_active_tools().count();
    let bg = state.bg_processes.values().filter(|b| !b.finished).count();
    if active == 0 && bg == 0 {
        return 0;
    }
    match details.section_render(ShelfSection::Tools) {
        SectionRender::Skip => 0,
        SectionRender::Summary => 1,
        SectionRender::Full => {
            let cap = shelf_tool_row_cap(SectionRender::Full);
            let shown = active.min(cap);
            let overflow = active.saturating_sub(cap);
            let tool_rows = shown as u16;
            let verbose_extra = if verbose_tools { tool_rows } else { 0 };
            let drafting = u16::from(state.generating_tool.is_some());
            let overflow_line = u16::from(overflow > 0);
            1 + drafting + tool_rows + verbose_extra + overflow_line + if bg > 0 { 1 } else { 0 }
        }
    }
}

fn subagent_line_count(state: &TurnActivityState, details: &ShelfDetailsState) -> u16 {
    let count = state.subagents.len();
    if count == 0 {
        return 0;
    }
    match details.section_render(ShelfSection::Subagents) {
        SectionRender::Skip => 0,
        SectionRender::Summary => 1,
        SectionRender::Full => {
            let rows = count.min(3) as u16 + 1;
            let tail_rows = state
                .subagents
                .values()
                .filter(|s| s.recent_tools.len() >= 2)
                .take(3)
                .count() as u16;
            rows + tail_rows
        }
    }
}

fn append_live_backstop(
    lines: &mut Vec<Line>,
    caption: &str,
    spin: &str,
    accent: Color,
    dim: Color,
) {
    lines.push(Line::from(vec![
        Span::styled(format!("{spin} "), Style::default().fg(accent)),
        Span::styled(
            safe_truncate(caption, 72).to_string(),
            Style::default().fg(dim),
        ),
    ]));
}

fn append_quiet_mode_backstop(
    lines: &mut Vec<Line>,
    state: &TurnActivityState,
    warn: Color,
    error: Color,
) {
    for notice in state
        .activity_feed
        .iter()
        .filter(|n| matches!(n.tone, ActivityTone::Warn | ActivityTone::Error))
        .take(2)
    {
        let (style, prefix) = match notice.tone {
            ActivityTone::Error => (Style::default().fg(error), "✗ "),
            _ => (Style::default().fg(warn), "! "),
        };
        lines.push(Line::from(vec![Span::styled(
            format!("{prefix}{}", notice.text),
            style,
        )]));
    }
}

fn append_tokens_footer(lines: &mut Vec<Line>, state: &TurnActivityState, palette: &ShelfPalette) {
    let Some(total) = format_tokens_total(state.thinking_token_est, state.tool_token_acc) else {
        return;
    };
    lines.push(Line::from(vec![Span::styled(
        format!("  {total}"),
        Style::default()
            .fg(palette.dim)
            .add_modifier(Modifier::DIM | Modifier::ITALIC),
    )]));
}

fn append_thinking_lines(
    lines: &mut Vec<Line>,
    state: &TurnActivityState,
    details: &ShelfDetailsState,
    spin: &str,
    palette: &ShelfPalette,
) {
    let Some(content) = thinking_content(state, details) else {
        return;
    };
    match thinking_render_mode(state, details) {
        SectionRender::Skip => {}
        SectionRender::Summary => {
            let mut spans = vec![
                Span::styled(section_chevron(false), Style::default().fg(palette.dim)),
                Span::styled(
                    safe_truncate(&content, 56).to_string(),
                    Style::default()
                        .fg(palette.dim)
                        .add_modifier(Modifier::ITALIC),
                ),
            ];
            if let Some(label) = format_tokens_label(state.thinking_token_est) {
                spans.push(Span::styled(
                    format!("  {label}"),
                    Style::default().fg(palette.dim).add_modifier(Modifier::DIM),
                ));
            }
            lines.push(Line::from(spans));
        }
        SectionRender::Full => {
            let mut spans = vec![
                Span::styled(format!("{spin} "), Style::default().fg(palette.accent)),
                Span::styled(
                    content,
                    Style::default()
                        .fg(palette.dim)
                        .add_modifier(Modifier::ITALIC),
                ),
            ];
            if let Some(label) = format_tokens_label(state.thinking_token_est) {
                spans.push(Span::styled(
                    format!("  {label}"),
                    Style::default().fg(palette.dim).add_modifier(Modifier::DIM),
                ));
            }
            lines.push(Line::from(spans));
        }
    }
}

fn thinking_content(state: &TurnActivityState, details: &ShelfDetailsState) -> Option<String> {
    if let Some(label) = state.llm_wait_label() {
        let elapsed = state.phase_started.elapsed().as_secs();
        return Some(format!(
            "{} ({elapsed}s)",
            edgecrab_core::safe_truncate(label, 72)
        ));
    }
    let render = thinking_render_mode(state, details);
    if render != SectionRender::Skip
        && let Some(snippet) = state
            .reasoning_snippet
            .as_ref()
            .filter(|s| !s.trim().is_empty())
    {
        return Some(format!("thinking · {snippet}"));
    }
    match state.phase {
        ShelfPhase::ToolExec | ShelfPhase::GeneratingTool => {
            if details.section_render(ShelfSection::Tools) != SectionRender::Skip {
                return None;
            }
            state.live_caption()
        }
        _ => state.phase_line(),
    }
}

fn append_activity_lines(
    lines: &mut Vec<Line>,
    state: &TurnActivityState,
    details: &ShelfDetailsState,
    palette: &ShelfPalette,
) {
    let render = details.section_render(ShelfSection::Activity);
    for notice in visible_notices(state, details) {
        let style = match notice.tone {
            ActivityTone::Info => Style::default().fg(palette.dim),
            ActivityTone::Warn => Style::default().fg(palette.warn),
            ActivityTone::Error => Style::default().fg(palette.hot),
        };
        let prefix = match render {
            SectionRender::Summary => "▸ ",
            _ => "  ↳ ",
        };
        lines.push(Line::from(vec![Span::styled(
            format!("{prefix}{}", notice.text),
            style,
        )]));
        if matches!(render, SectionRender::Summary) {
            break;
        }
    }
}

fn visible_notices<'a>(
    state: &'a TurnActivityState,
    details: &ShelfDetailsState,
) -> Box<dyn Iterator<Item = &'a ActivityNotice> + 'a> {
    match details.section_render(ShelfSection::Activity) {
        SectionRender::Skip => Box::new(
            state
                .activity_feed
                .iter()
                .filter(|n| matches!(n.tone, ActivityTone::Warn | ActivityTone::Error)),
        ),
        SectionRender::Summary => Box::new(state.activity_feed.iter().take(1)),
        SectionRender::Full => Box::new(state.activity_feed.iter()),
    }
}

fn append_tool_lines(
    lines: &mut Vec<Line>,
    state: &TurnActivityState,
    details: &ShelfDetailsState,
    verbose_tools: bool,
    palette: &ShelfPalette,
) {
    let active: Vec<_> = state.sorted_active_tools().collect();
    let bg_count = state.bg_processes.values().filter(|b| !b.finished).count();
    if active.is_empty() && bg_count == 0 {
        return;
    }

    match details.section_render(ShelfSection::Tools) {
        SectionRender::Skip => {}
        SectionRender::Summary => {
            let primary = active
                .first()
                .map(|t| t.name.as_str())
                .unwrap_or("background");
            let extra = active.len().saturating_sub(1) + bg_count;
            let suffix = if extra > 0 {
                format!(" +{extra}")
            } else {
                String::new()
            };
            let mut spans = vec![
                Span::styled(section_chevron(false), Style::default().fg(palette.dim)),
                Span::styled(
                    format!("{} tool(s) · {primary}{suffix}", active.len().max(bg_count)),
                    Style::default().fg(palette.accent),
                ),
            ];
            if let Some(label) = format_tokens_label(state.tool_token_acc) {
                spans.push(Span::styled(
                    format!("  {label}"),
                    Style::default().fg(palette.dim).add_modifier(Modifier::DIM),
                ));
            }
            lines.push(Line::from(spans));
        }
        SectionRender::Full => {
            let label_suffix = format_tokens_label(state.tool_token_acc)
                .map(|label| format!("  {label}"))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(section_chevron(true), Style::default().fg(palette.accent)),
                Span::styled(
                    format!("tools{label_suffix}"),
                    Style::default()
                        .fg(palette.dim)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            if let Some((_, name)) = &state.generating_tool {
                let preview = state
                    .generating_preview
                    .as_deref()
                    .filter(|p| !p.trim().is_empty())
                    .unwrap_or("…");
                let label = name.replace('_', " ");
                lines.push(Line::from(vec![
                    Span::styled("  ├─ ", Style::default().fg(palette.border)),
                    Span::styled(
                        format!("✎ drafting {label} · {preview}"),
                        Style::default()
                            .fg(palette.dim)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ]));
            }
            let cap = shelf_tool_row_cap(SectionRender::Full);
            let total_active = active.len();
            let shown: Vec<_> = active.into_iter().take(cap).collect();
            let active_count = shown.len();
            for (i, tool) in shown.into_iter().enumerate() {
                let prefix = if i + 1 == active_count && total_active <= cap {
                    "  └─ "
                } else {
                    "  ├─ "
                };
                push_tool_row(lines, tool, prefix.to_string(), palette);
                if verbose_tools {
                    push_verbose_args_line(lines, tool, palette.dim);
                }
            }
            if total_active > cap {
                lines.push(Line::from(vec![
                    Span::styled("  └─ ", Style::default().fg(palette.border)),
                    Span::styled(
                        format!("+{} more tool(s)", total_active - cap),
                        Style::default().fg(palette.dim).add_modifier(Modifier::DIM),
                    ),
                ]));
            }
            for bg in state.bg_processes.values().filter(|b| !b.finished).take(1) {
                let tail = shelf_bg_tail_preview(&bg.tail);
                let tail_part = if tail.is_empty() {
                    String::new()
                } else {
                    format!(" · {tail}")
                };
                lines.push(Line::from(vec![
                    Span::styled("  ├─ ", Style::default().fg(palette.border)),
                    Span::styled("📟 ", Style::default()),
                    Span::styled(
                        format!(
                            "{} · {}{tail_part}  (/tail {})",
                            bg.process_id, bg.command_preview, bg.process_id
                        ),
                        Style::default().fg(palette.dim),
                    ),
                ]));
            }
        }
    }
}

fn push_tool_row(
    lines: &mut Vec<Line>,
    tool: &crate::turn_activity::ShelfToolRow,
    prefix: String,
    palette: &ShelfPalette,
) {
    let icon = tool_icon(&tool.name);
    let preview = if tool.preview.is_empty() {
        tool_status_preview(&tool.name, &tool.args_json)
    } else {
        tool.preview.clone()
    };
    let detail = tool
        .detail
        .as_deref()
        .filter(|d| !d.trim().is_empty())
        .unwrap_or("…");
    let elapsed_secs = tool.started_at.elapsed().as_secs();
    let heat = elapsed_heat(elapsed_secs);
    let elapsed_style =
        Style::default().fg(heat_color(heat, palette.dim, palette.warn, palette.hot));
    let elapsed_suffix = if elapsed_secs > 0 {
        format!(" · {}", fmt_duration(elapsed_secs))
    } else {
        String::new()
    };
    lines.push(Line::from(vec![
        Span::styled(prefix, Style::default().fg(palette.border)),
        Span::styled(
            format!("{icon} {}  ", tool.name),
            Style::default().fg(palette.accent),
        ),
        Span::styled(
            format!("{preview} · {detail}"),
            Style::default().fg(palette.dim),
        ),
        Span::styled(elapsed_suffix, elapsed_style),
    ]));
}

fn push_verbose_args_line(
    lines: &mut Vec<Line>,
    tool: &crate::turn_activity::ShelfToolRow,
    dim: Color,
) {
    let args_line = safe_truncate(tool.args_json.trim(), 72);
    if !args_line.is_empty() && args_line != "{}" {
        lines.push(Line::from(vec![
            Span::styled("      ", Style::default()),
            Span::styled(
                format!("args: {args_line}"),
                Style::default().fg(dim).add_modifier(Modifier::DIM),
            ),
        ]));
    }
}

fn append_subagent_lines(
    lines: &mut Vec<Line>,
    state: &TurnActivityState,
    details: &ShelfDetailsState,
    palette: &ShelfPalette,
) {
    if state.subagents.is_empty() {
        return;
    }
    let mut subs: Vec<_> = state.subagents.values().collect();
    subs.sort_by_key(|s| s.task_index);

    match details.section_render(ShelfSection::Subagents) {
        SectionRender::Skip => {}
        SectionRender::Summary => {
            let tool_total = state.subagent_tool_total();
            let tool_suffix = if tool_total > 0 {
                format!(" · {tool_total} tools")
            } else {
                String::new()
            };
            let spark = if subs.len() >= 2 {
                let counts: Vec<u64> = subs.iter().map(|s| s.tool_count as u64).collect();
                let spark = sparkline(&counts);
                if spark.is_empty() {
                    String::new()
                } else {
                    format!(" {spark}")
                }
            } else {
                String::new()
            };
            lines.push(Line::from(vec![
                Span::styled(section_chevron(false), Style::default().fg(palette.dim)),
                Span::styled(
                    format!(
                        "{} delegate(s) active{tool_suffix}{spark}  (/agents)",
                        subs.len()
                    ),
                    Style::default().fg(palette.dim),
                ),
            ]));
        }
        SectionRender::Full => {
            let spark = if subs.len() >= 2 {
                let counts: Vec<u64> = subs.iter().map(|s| s.tool_count as u64).collect();
                sparkline(&counts)
            } else {
                String::new()
            };
            let header_suffix = if spark.is_empty() {
                String::new()
            } else {
                format!(" · {spark}")
            };
            lines.push(Line::from(vec![
                Span::styled(section_chevron(true), Style::default().fg(palette.accent)),
                Span::styled(
                    format!("agents{header_suffix}  (/agents)"),
                    Style::default()
                        .fg(palette.dim)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            let last = subs.len().min(3);
            for (i, sub) in subs.into_iter().take(3).enumerate() {
                let stem = if i + 1 == last {
                    "     └─ "
                } else {
                    "     ├─ "
                };
                let elapsed_secs = sub.started_at.elapsed().as_secs();
                let heat = elapsed_heat(elapsed_secs);
                let elapsed_style =
                    Style::default().fg(heat_color(heat, palette.dim, palette.warn, palette.hot));
                let elapsed_suffix = if elapsed_secs > 0 {
                    format!(" · {}", fmt_duration(elapsed_secs))
                } else {
                    String::new()
                };
                lines.push(Line::from(vec![
                    Span::styled(stem, Style::default().fg(palette.border)),
                    Span::styled(
                        format!(
                            "[{}/{}] {}{}",
                            sub.task_index + 1,
                            sub.task_count,
                            sub.goal,
                            sub.detail
                                .as_deref()
                                .map(|d| format!(" · {d}"))
                                .unwrap_or_default(),
                        ),
                        Style::default().fg(palette.dim),
                    ),
                    Span::styled(elapsed_suffix, elapsed_style),
                ]));
                let tail = format_recent_tools(&sub.recent_tools, 3);
                if sub.recent_tools.len() >= 2 && !tail.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("           ", Style::default()),
                        Span::styled(
                            tail,
                            Style::default().fg(palette.dim).add_modifier(Modifier::DIM),
                        ),
                    ]));
                }
            }
        }
    }
}

fn shelf_bg_tail_preview(tail: &str) -> String {
    let trimmed = tail.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let lines: Vec<&str> = trimmed.lines().filter(|l| !l.trim().is_empty()).collect();
    let joined = if lines.len() >= 2 {
        format!(
            "{} | {}",
            lines[lines.len() - 2].trim(),
            lines[lines.len() - 1].trim()
        )
    } else {
        lines.last().copied().unwrap_or("").trim().to_string()
    };
    safe_truncate(&joined, SHELF_BG_TAIL_CHARS).to_string()
}

fn compact_summary(state: &TurnActivityState) -> Option<String> {
    state.live_caption()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shelf_details::ShelfDetailsState;
    use crate::turn_activity::TurnActivityState;

    #[test]
    fn live_backstop_when_all_sections_hidden() {
        let mut state = TurnActivityState::new(true);
        let mut details = ShelfDetailsState::default();
        details.handle_command("hidden");
        state.on_tool_exec(
            "t1".into(),
            "file_write".into(),
            "{}".into(),
            "demo/index.html".into(),
            1,
        );
        let lines = estimate_shelf_lines(&state, &details, false, false, true);
        assert!(
            lines >= 1,
            "expected at least one shelf line during tool exec"
        );
    }

    #[test]
    fn compact_summary_prefers_active_tool() {
        let mut state = TurnActivityState::new(true);
        state.on_tool_exec(
            "t1".into(),
            "terminal".into(),
            "{}".into(),
            "cargo build".into(),
            1,
        );
        state.on_tool_progress(
            "t1",
            "Compiling edgecrab".into(),
            2,
            std::time::Instant::now(),
        );
        let s = compact_summary(&state).unwrap();
        assert!(s.contains("terminal"));
        assert!(s.contains("Compiling"));
    }

    #[test]
    fn activity_hidden_still_shows_errors() {
        let mut state = TurnActivityState::new(true);
        let details = ShelfDetailsState::default();
        state.push_activity("gateway exited".into(), ActivityTone::Error);
        let count = activity_line_count(&state, &details);
        assert_eq!(count, 1);
    }

    #[test]
    fn tools_collapsed_is_one_line() {
        let mut state = TurnActivityState::new(true);
        let mut details = ShelfDetailsState::default();
        details.handle_command("tools collapsed");
        state.on_tool_exec(
            "t1".into(),
            "terminal".into(),
            "{}".into(),
            "build".into(),
            1,
        );
        assert_eq!(tool_line_count(&state, &details, false), 1);
    }

    #[test]
    fn tools_expanded_shows_more_than_three_parallel_rows() {
        let mut state = TurnActivityState::new(true);
        let mut details = ShelfDetailsState::default();
        details.handle_command("tools expanded");
        for i in 0..5 {
            state.on_tool_exec(
                format!("t{i}"),
                "file_read".into(),
                "{}".into(),
                format!("path{i}"),
                i as u64 + 1,
            );
        }
        let n = tool_line_count(&state, &details, false);
        assert!(n >= 6, "expected header + 5 tool rows, got {n}");
    }

    #[test]
    fn subagent_tree_counts_header_plus_rows() {
        let mut state = TurnActivityState::new(true);
        let mut details = ShelfDetailsState::default();
        details.handle_command("subagents expanded");
        state.on_subagent_start(0, 2, "audit".into(), 1, "sa-0".into(), None);
        state.on_subagent_start(1, 2, "migrate".into(), 1, "sa-1".into(), None);
        let n = subagent_line_count(&state, &details);
        assert!(n >= 3, "got {n}");
    }

    #[test]
    fn subagent_summary_includes_tool_count() {
        let mut state = TurnActivityState::new(true);
        let details = ShelfDetailsState::default();
        state.on_subagent_start(0, 1, "audit".into(), 1, "sa-0".into(), None);
        state.on_subagent_tool(0, "file_read", "file_read  src/a.rs".into());
        state.on_subagent_tool(0, "terminal", "terminal  cargo test".into());
        assert_eq!(state.subagent_tool_total(), 2);
        assert_eq!(subagent_line_count(&state, &details), 1);
    }
}
