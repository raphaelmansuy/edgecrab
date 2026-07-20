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
    ActivityNotice, ActivityTone, FOCUS_TOOL_BODY_LINES, SHELF_BG_TAIL_CHARS, SHELF_MAX_TOOL_ROWS,
    SHELF_MAX_TOOL_ROWS_FULL, ShelfPhase, TurnActivityState, detail_has_evidence,
    focus_tool_body_lines,
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
    /// Session edit ledger caption (`files N  +X −Y`), when present.
    pub edit_ledger_caption: Option<&'a str>,
    /// Truncated last-N-lines thinking body from StreamPresentation (multi-line peek).
    pub thinking_truncated: Option<&'a str>,
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
        // Signal priority: tools evidence → subagents → thinking → activity → tokens.
        append_tool_lines(&mut lines, state, details, verbose_tools, &palette);
        append_subagent_lines(&mut lines, state, details, &palette);
        let show_thinking = !tools_section_has_evidence(state, details)
            || (details.section_render(ShelfSection::Thinking) == SectionRender::Full
                && !matches!(
                    state.phase,
                    ShelfPhase::ToolExec | ShelfPhase::GeneratingTool
                ));
        if show_thinking {
            append_thinking_lines(
                &mut lines,
                state,
                details,
                spin,
                &palette,
                params.thinking_truncated,
            );
        }
        append_activity_lines(&mut lines, state, details, &palette);
        if (lines.len() as u16) < MAX_SHELF_LINES {
            append_tokens_footer(&mut lines, state, &palette, params.edit_ledger_caption);
        }
        // Hard cap — prefer keeping tool evidence (already first).
        if lines.len() > MAX_SHELF_LINES as usize {
            lines.truncate(MAX_SHELF_LINES as usize);
        }
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
    n += tool_line_count(state, details, verbose_tools);
    n += subagent_line_count(state, details);
    let skip_thinking = tools_section_has_evidence(state, details)
        && matches!(
            state.phase,
            ShelfPhase::ToolExec | ShelfPhase::GeneratingTool
        );
    if !skip_thinking {
        n += thinking_line_count(state, details);
    }
    n += activity_line_count(state, details);
    n += tokens_footer_line_count(state);
    n.min(MAX_SHELF_LINES)
}

fn tools_section_has_evidence(state: &TurnActivityState, details: &ShelfDetailsState) -> bool {
    if details.section_render(ShelfSection::Tools) == SectionRender::Skip {
        return false;
    }
    state
        .primary_focus_tool()
        .is_some_and(|t| detail_has_evidence(t.detail.as_deref()))
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
        .is_some_and(|s| !s.trim().is_empty())
        || state
            .reasoning_truncated
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty());
    details.effective_thinking_render(has_snippet)
}

fn thinking_body_lines(truncated: Option<&str>) -> Vec<&str> {
    truncated
        .unwrap_or("")
        .lines()
        .filter(|l| !l.trim().is_empty() && *l != "…")
        .collect()
}

fn thinking_line_count(state: &TurnActivityState, details: &ShelfDetailsState) -> u16 {
    if thinking_suppressed_by_tools(state, details) {
        return 0;
    }
    match thinking_render_mode(state, details) {
        SectionRender::Skip => 0,
        SectionRender::Summary => {
            if state.reasoning_snippet.is_some() || state.reasoning_truncated.is_some() {
                1
            } else {
                0
            }
        }
        SectionRender::Full => {
            let body = thinking_body_lines(state.reasoning_truncated.as_deref());
            if body.is_empty() {
                u16::from(state.reasoning_snippet.is_some())
            } else {
                // Header + body lines, leave room for tokens footer.
                (1 + body.len() as u16).min(MAX_SHELF_LINES.saturating_sub(1))
            }
        }
    }
}

fn thinking_suppressed_by_tools(state: &TurnActivityState, details: &ShelfDetailsState) -> bool {
    if state.generating_tool.is_some() || state.tools.values().any(|t| !t.finished) {
        return details.section_render(ShelfSection::Tools) != SectionRender::Skip;
    }
    false
}

fn activity_line_count(state: &TurnActivityState, details: &ShelfDetailsState) -> u16 {
    let notices: Vec<_> = visible_notices(state, details).collect();
    match details.section_render(ShelfSection::Activity) {
        SectionRender::Skip => notices
            .iter()
            .filter(|n| matches!(n.tone, ActivityTone::Warn | ActivityTone::Error))
            .count() as u16,
        SectionRender::Summary => u16::from(!notices.is_empty()),
        SectionRender::Full => notices.len() as u16,
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
            let body_lines = state
                .primary_focus_tool()
                .map(|t| focus_tool_body_lines(t.detail.as_deref()).len() as u16)
                .unwrap_or(0);
            1 + drafting
                + tool_rows
                + body_lines
                + verbose_extra
                + overflow_line
                + if bg > 0 { 1 } else { 0 }
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

fn append_tokens_footer(
    lines: &mut Vec<Line>,
    state: &TurnActivityState,
    palette: &ShelfPalette,
    edit_ledger_caption: Option<&str>,
) {
    let tokens = format_tokens_total(state.thinking_token_est, state.tool_token_acc);
    let mut parts: Vec<String> = Vec::new();
    if let Some(total) = tokens {
        parts.push(total);
    }
    if let Some(ledger) = edit_ledger_caption.filter(|s| !s.is_empty()) {
        parts.push(ledger.to_string());
    }
    if parts.is_empty() {
        return;
    }
    lines.push(Line::from(vec![Span::styled(
        format!("  {}", parts.join("  ·  ")),
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
    thinking_truncated: Option<&str>,
) {
    if thinking_suppressed_by_tools(state, details) {
        return;
    }
    let trunc = thinking_truncated.or(state.reasoning_truncated.as_deref());
    let body = thinking_body_lines(trunc);
    let mode = thinking_render_mode(state, details);
    match mode {
        SectionRender::Skip => {}
        SectionRender::Summary => {
            let peek = body
                .last()
                .copied()
                .unwrap_or_else(|| state.reasoning_snippet.as_deref().unwrap_or("thinking…"));
            let mut spans = vec![
                Span::styled(section_chevron(false), Style::default().fg(palette.dim)),
                Span::styled(
                    format!("thinking · {}", safe_truncate(peek, 56)),
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
            if body.is_empty() {
                // Fallback single-line peek when truncated body not ready.
                let Some(content) = thinking_content(state, details) else {
                    return;
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("{spin} "), Style::default().fg(palette.accent)),
                    Span::styled(
                        content,
                        Style::default()
                            .fg(palette.dim)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ]));
                return;
            }
            let mut header = vec![
                Span::styled(format!("{spin} "), Style::default().fg(palette.accent)),
                Span::styled(
                    "Thinking…".to_string(),
                    Style::default()
                        .fg(palette.dim)
                        .add_modifier(Modifier::ITALIC),
                ),
            ];
            if let Some(label) = format_tokens_label(state.thinking_token_est) {
                header.push(Span::styled(
                    format!("  {label}"),
                    Style::default().fg(palette.dim).add_modifier(Modifier::DIM),
                ));
            }
            lines.push(Line::from(header));
            let budget = (MAX_SHELF_LINES as usize)
                .saturating_sub(lines.len())
                .saturating_sub(1); // leave room for tokens footer
            let start = body.len().saturating_sub(budget);
            if start > 0 {
                lines.push(Line::from(Span::styled(
                    "  …".to_string(),
                    Style::default().fg(palette.dim).add_modifier(Modifier::DIM),
                )));
            }
            for line in &body[start..] {
                if lines.len() as u16 >= MAX_SHELF_LINES.saturating_sub(1) {
                    break;
                }
                lines.push(Line::from(Span::styled(
                    format!("  {}", safe_truncate(line, 96)),
                    Style::default()
                        .fg(palette.dim)
                        .add_modifier(Modifier::ITALIC | Modifier::DIM),
                )));
            }
        }
    }
}

fn thinking_content(state: &TurnActivityState, details: &ShelfDetailsState) -> Option<String> {
    // During tool work the tools section owns the live signal — do not mirror
    // captions / provider heartbeats into the thinking band.
    if state.generating_tool.is_some() || state.tools.values().any(|t| !t.finished) {
        if details.section_render(ShelfSection::Tools) != SectionRender::Skip {
            return None;
        }
        return state.live_caption();
    }
    // Between tools: prefer calm phase caption over long provider SSE dumps.
    // Active wait detail (llm_wait_label) only when the model is actually blocking.
    if matches!(state.phase, ShelfPhase::AwaitingFirstToken) {
        if let Some(label) = state.llm_wait_label() {
            let elapsed = state.phase_started.elapsed().as_secs();
            // Compact: first segment before em-dash / long Copilot appendix.
            let short = label.split(" — ").next().unwrap_or(label);
            return Some(format!(
                "{} ({elapsed}s)",
                edgecrab_core::safe_truncate(short, 56)
            ));
        }
        return state.phase_line();
    }
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
    state.phase_line()
}

fn append_activity_lines(
    lines: &mut Vec<Line>,
    state: &TurnActivityState,
    details: &ShelfDetailsState,
    palette: &ShelfPalette,
) {
    if (lines.len() as u16) >= MAX_SHELF_LINES {
        return;
    }
    let render = details.section_render(ShelfSection::Activity);
    let has_evidence = tools_section_has_evidence(state, details);
    let mut shown = 0usize;
    for notice in visible_notices(state, details) {
        // Demote charms / LLM-wait noise when tool stdout is already on screen.
        if has_evidence && is_secondary_activity_notice(&notice.text) {
            continue;
        }
        let style = match notice.tone {
            ActivityTone::Info => Style::default().fg(palette.dim).add_modifier(Modifier::DIM),
            ActivityTone::Warn => Style::default().fg(palette.dim).add_modifier(Modifier::DIM),
            ActivityTone::Error => Style::default().fg(palette.hot),
        };
        let prefix = match render {
            SectionRender::Summary => "▸ ",
            _ => "  ↳ ",
        };
        let text = notice.text.trim_start_matches('↳').trim_start();
        lines.push(Line::from(vec![Span::styled(
            format!("{prefix}{text}"),
            style,
        )]));
        shown += 1;
        if matches!(render, SectionRender::Summary) || (has_evidence && shown >= 1) {
            break;
        }
        if (lines.len() as u16) >= MAX_SHELF_LINES {
            break;
        }
    }
}

fn is_secondary_activity_notice(text: &str) -> bool {
    let t = text.trim_start_matches('↳').trim_start();
    TurnActivityState::is_llm_wait_shelf_line(t)
        || t.contains("still cooking")
        || t.contains("polishing edges")
        || t.contains("asking the void")
        || t.starts_with("still working")
        || t.starts_with("still drafting")
}

fn visible_notices<'a>(
    state: &'a TurnActivityState,
    details: &ShelfDetailsState,
) -> Box<dyn Iterator<Item = &'a ActivityNotice> + 'a> {
    // Newest first — Summary must show the latest signal, not the oldest.
    // Filter expired here so render stays correct even between expire ticks.
    let now = std::time::Instant::now();
    match details.section_render(ShelfSection::Activity) {
        SectionRender::Skip => Box::new(state.activity_feed.iter().rev().filter(move |n| {
            !n.is_expired(now) && matches!(n.tone, ActivityTone::Warn | ActivityTone::Error)
        })),
        SectionRender::Summary => Box::new(
            state
                .activity_feed
                .iter()
                .rev()
                .filter(move |n| !n.is_expired(now))
                .take(1),
        ),
        SectionRender::Full => Box::new(
            state
                .activity_feed
                .iter()
                .rev()
                .filter(move |n| !n.is_expired(now)),
        ),
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
            let primary_id = state.primary_focus_tool().map(|t| t.tool_call_id.as_str());
            let shown: Vec<_> = active.into_iter().take(cap).collect();
            let active_count = shown.len();
            for (i, tool) in shown.into_iter().enumerate() {
                let is_last_header = i + 1 == active_count && total_active <= cap;
                let is_primary = primary_id == Some(tool.tool_call_id.as_str());
                let body = if is_primary {
                    focus_tool_body_lines(tool.detail.as_deref())
                } else {
                    Vec::new()
                };
                let prefix = if is_last_header && body.is_empty() {
                    "  └─ "
                } else {
                    "  ├─ "
                };
                push_tool_header(lines, tool, prefix.to_string(), palette);
                if verbose_tools {
                    push_verbose_args_line(lines, tool, palette.dim);
                }
                if !body.is_empty() {
                    push_tool_body_lines(lines, &body, is_last_header, palette);
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

fn push_tool_header(
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
    let elapsed_secs = tool.started_at.elapsed().as_secs();
    let heat = elapsed_heat(elapsed_secs);
    let elapsed_style =
        Style::default().fg(heat_color(heat, palette.dim, palette.warn, palette.hot));
    let elapsed_suffix = if elapsed_secs > 0 {
        format!(" · {}", fmt_duration(elapsed_secs))
    } else {
        String::new()
    };
    // Header carries command/path only — stdout lives in the focus body lines.
    let preview_show = if detail_has_evidence(tool.detail.as_deref()) {
        safe_truncate(&preview, 56).to_string()
    } else {
        let detail = tool
            .detail
            .as_deref()
            .filter(|d| !d.trim().is_empty())
            .map(crate::turn_activity::last_detail_line)
            .unwrap_or("…");
        format!("{preview} · {detail}")
    };
    lines.push(Line::from(vec![
        Span::styled(prefix, Style::default().fg(palette.border)),
        Span::styled(
            format!("{icon} {}  ", tool.name),
            Style::default().fg(palette.accent),
        ),
        Span::styled(preview_show, Style::default().fg(palette.dim)),
        Span::styled(elapsed_suffix, elapsed_style),
    ]));
}

fn push_tool_body_lines(
    lines: &mut Vec<Line>,
    body: &[&str],
    is_last_tool: bool,
    palette: &ShelfPalette,
) {
    let max = FOCUS_TOOL_BODY_LINES.min(body.len());
    for (i, line) in body.iter().take(max).enumerate() {
        let branch = if is_last_tool && i + 1 == max {
            "  └─ "
        } else {
            "  │  "
        };
        lines.push(Line::from(vec![
            Span::styled(branch, Style::default().fg(palette.border)),
            Span::styled(
                safe_truncate(line, 72).to_string(),
                Style::default().fg(palette.dim),
            ),
        ]));
    }
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
    fn thinking_multiline_counts_body_lines() {
        let mut state = TurnActivityState::new(true);
        let details = ShelfDetailsState::default();
        state.sync_thinking_from_presentation(
            Some("…tail".into()),
            Some("line1\nline2\nline3\nline4".into()),
            "line4",
        );
        let n = thinking_line_count(&state, &details);
        assert!(
            n >= 4,
            "expected header + body lines while Thinking, got {n}"
        );
    }

    #[test]
    fn thinking_suppressed_while_tool_runs() {
        let mut state = TurnActivityState::new(true);
        let details = ShelfDetailsState::default();
        state.sync_thinking_from_presentation(
            Some("plan".into()),
            Some("plan the edit".into()),
            "plan",
        );
        state.on_tool_exec(
            "t1".into(),
            "terminal".into(),
            "{}".into(),
            "cargo test".into(),
            1,
        );
        assert_eq!(thinking_line_count(&state, &details), 0);
        let mut lines = Vec::new();
        append_thinking_lines(
            &mut lines,
            &state,
            &details,
            "⠋",
            &ShelfPalette {
                accent: Color::Yellow,
                dim: Color::DarkGray,
                warn: Color::Yellow,
                hot: Color::Red,
                border: Color::DarkGray,
            },
            Some("plan the edit"),
        );
        assert!(lines.is_empty(), "tool focus must suppress CoT peek");
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

    #[test]
    fn focus_pane_counts_multiline_body() {
        let mut state = TurnActivityState::new(true);
        let mut details = ShelfDetailsState::default();
        details.handle_command("tools expanded");
        state.on_tool_exec(
            "t1".into(),
            "terminal".into(),
            "{}".into(),
            "npm install".into(),
            1,
        );
        state.on_tool_progress(
            "t1",
            "warn one\nwarn two\nadded packages".into(),
            2,
            std::time::Instant::now(),
        );
        let n = tool_line_count(&state, &details, false);
        // header + 1 tool header + 3 body lines
        assert!(n >= 5, "expected focus body lines in count, got {n}");
    }

    #[test]
    fn parallel_tools_only_primary_gets_body_budget() {
        let mut state = TurnActivityState::new(true);
        let mut details = ShelfDetailsState::default();
        details.handle_command("tools expanded");
        state.on_tool_exec(
            "t1".into(),
            "terminal".into(),
            "{}".into(),
            "build".into(),
            1,
        );
        if let Some(row) = state.tools.get_mut("t1") {
            row.started_at = std::time::Instant::now() - std::time::Duration::from_secs(20);
        }
        state.on_tool_exec(
            "t2".into(),
            "file_read".into(),
            "{}".into(),
            "a.rs".into(),
            2,
        );
        state.on_tool_progress(
            "t1",
            "line1\nline2\nline3".into(),
            3,
            std::time::Instant::now(),
        );
        state.on_tool_progress("t2", "only-one".into(), 4, std::time::Instant::now());
        let n = tool_line_count(&state, &details, false);
        // header + 2 tool headers + 3 body (primary only) — not +1 for secondary
        assert!(
            (5..=7).contains(&n),
            "expected ~6 lines (header+2 tools+3 body), got {n}"
        );
        assert_eq!(
            state.primary_focus_tool().map(|t| t.name.as_str()),
            Some("terminal")
        );
    }

    #[test]
    fn secondary_activity_notices_detected() {
        assert!(is_secondary_activity_notice("still cooking… — terminal"));
        assert!(is_secondary_activity_notice(
            "vscode-copilot: iter 17 streaming"
        ));
        assert!(!is_secondary_activity_notice("gateway exited"));
    }

    #[test]
    fn visible_notices_newest_first() {
        let mut state = TurnActivityState::new(true);
        let mut details = ShelfDetailsState::default();
        details.handle_command("activity expanded");
        state.push_activity("older tip".into(), ActivityTone::Info);
        state.push_activity("newer tip".into(), ActivityTone::Info);
        let first = visible_notices(&state, &details)
            .next()
            .map(|n| n.text.as_str());
        assert_eq!(first, Some("newer tip"));
    }
}
