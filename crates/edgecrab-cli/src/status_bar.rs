//! Status bar rendering — extracted from `app.rs` (Phase 0 modularization).

use std::collections::HashMap;
use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::display_state::{
    DisplayState, VoicePresenceState, context_usage_ratio, format_voice_presence_badge,
    goal_flash_badge_style, goal_status_chip_style, run_outcome_badge_style,
};
use crate::spawn_hud::{
    SpawnHudCaps, SpawnHudSeverity, format_spawn_hud, format_spawn_pause_chip, metrics_from_turn,
    spawn_hud_severity,
};
use crate::status_chrome::{
    TerminalGlyphProfile, compact_spinner_frame, format_elapsed_hint, format_thinking_status,
    format_token_count, format_waiting_first_token_status, shelf_generating_status_span,
    summarize_tools_for_status, wait_urgency_color, words_estimate,
};
use crate::status_indicator::StatusIndicatorStyle;
use crate::status_summaries::{
    ActiveSubagentStatus, BackgroundTaskStatus, format_background_status_summary,
    format_subagent_status_summary,
};
use crate::theme::Theme;
use crate::tool_display::{
    DisplayWidths, build_context_gauge, tool_action_verb, tool_category, tool_icon,
    tool_status_preview_width,
};
use crate::turn_activity::{ShelfPhase, TurnActivityState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusBarEditorMode {
    Normal,
    ComposeInsert,
    ComposeNormal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusBarUiProfile {
    Standard,
    ReducedMotion,
    BasicCompat,
}

pub struct StatusBarDocumentChip {
    pub icon: String,
    pub title: String,
    pub accent: Color,
}

/// Precomputed render inputs — keeps `status_bar` free of `App` coupling.
pub struct StatusBarRenderParams<'a> {
    pub compact: bool,
    pub theme: &'a Theme,
    pub turn_activity: &'a TurnActivityState,
    pub shelf_spinner_frame: usize,
    pub terminal_glyph_profile: TerminalGlyphProfile,
    pub status_indicator: StatusIndicatorStyle,
    pub spawn_hud_caps: SpawnHudCaps,
    pub display_state: &'a DisplayState,
    pub thinking_verb_idx: usize,
    pub kaomoji_frame_idx: usize,
    pub last_terminal_width: u16,
    pub goal_flash_status: Option<&'a str>,
    pub last_run_outcome: Option<&'a edgecrab_types::RunOutcome>,
    pub document_overlay: Option<StatusBarDocumentChip>,
    pub goal_status_chip: Option<&'a edgecrab_core::GoalStatusChip>,
    pub model_name: &'a str,
    pub context_window: Option<u64>,
    pub total_tokens: u64,
    pub session_cost: f64,
    pub voice_presence: Option<VoicePresenceState>,
    pub voice_presence_frame_idx: usize,
    pub active_subagents: &'a HashMap<usize, ActiveSubagentStatus>,
    pub background_tasks_active: &'a HashMap<String, BackgroundTaskStatus>,
    pub pending_steer_count: usize,
    pub steer_applied_at: Option<Instant>,
    pub shadow_judge_enabled: bool,
    pub shadow_judge_intervention_at: Option<Instant>,
    pub shadow_judge_intervention_confidence: Option<f32>,
    pub shadow_judge_intervention_text: Option<&'a str>,
    pub turn_count: usize,
    pub scroll_offset: u16,
    pub paging_key_hint: &'a str,
    pub mouse_capture_enabled: bool,
    pub clarify_pending: bool,
    pub is_processing: bool,
    pub editor_mode: StatusBarEditorMode,
    pub compose_normal_hint: &'a str,
    pub active_skills: &'a [String],
    pub voice_push_to_talk_key: &'a str,
    pub inline_compose_hint: &'a str,
    pub remote_terminal_session: bool,
    pub terminal_ui_profile: StatusBarUiProfile,
}

fn unicode_trunc(s: &str, max_cols: usize) -> String {
    let w = s.width();
    if w <= max_cols {
        return s.to_string();
    }
    let budget = max_cols.saturating_sub(3);
    let mut out = String::new();
    let mut used = 0usize;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        if used + cw > budget {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push_str("...");
    out
}

pub fn render_status_bar(frame: &mut Frame, area: Rect, params: &StatusBarRenderParams) {
    if params.compact {
        render_compact_status_bar_inner(frame, area, params);
        return;
    }

    let mut left_spans = Vec::new();

    // ── Brand badge ─────────────────────────────────────────────
    // A small copper "EC" badge anchors the left side of the status bar.
    left_spans.push(Span::styled(
        " EC ",
        Style::default()
            .fg(Color::Rgb(205, 127, 50))
            .add_modifier(Modifier::BOLD),
    ));
    left_spans.push(Span::styled(
        "│",
        Style::default().fg(Color::Rgb(50, 50, 65)),
    ));
    left_spans.push(Span::styled(
        format!(" v{} ", crate::banner::VERSION),
        Style::default().fg(Color::Rgb(120, 130, 150)),
    ));
    left_spans.push(Span::styled(
        "│",
        Style::default().fg(Color::Rgb(50, 50, 65)),
    ));

    // ── Spinner / state indicator ────────────────────────────────
    if let Some(span) = shelf_generating_status_span(
        params.turn_activity,
        params.shelf_spinner_frame,
        params.terminal_glyph_profile,
    ) {
        left_spans.push(span);
    } else {
        match params.display_state {
            DisplayState::AwaitingFirstToken { frame: f, started } => {
                let elapsed_secs = started.elapsed().as_secs();
                let long_label = params
                    .turn_activity
                    .llm_wait_compact_label()
                    .unwrap_or("waiting for first token");
                let msg = format_waiting_first_token_status(
                    params.theme,
                    params.status_indicator,
                    params.terminal_glyph_profile,
                    *f,
                    params.thinking_verb_idx,
                    params.kaomoji_frame_idx,
                    elapsed_secs,
                    long_label,
                );
                // FP46: urgency color ramp — amber (normal) → orange (slow) → red (stall)
                let color = wait_urgency_color(elapsed_secs);
                left_spans.push(Span::styled(format!(" {msg} "), Style::default().fg(color)));
            }
            DisplayState::Thinking { frame: f, started } => {
                let elapsed_secs = started.elapsed().as_secs();
                let msg = format_thinking_status(
                    params.theme,
                    params.status_indicator,
                    params.terminal_glyph_profile,
                    *f,
                    params.thinking_verb_idx,
                    params.kaomoji_frame_idx,
                    elapsed_secs,
                );
                // FP46: same urgency ramp for extended reasoning waits
                let color = wait_urgency_color(elapsed_secs);
                left_spans.push(Span::styled(format!(" {msg} "), Style::default().fg(color)));
            }
            DisplayState::Streaming {
                token_count,
                chars_written,
                current_section,
                started,
            } => {
                let elapsed = started.elapsed().as_secs_f64();
                // Only show rate once enough tokens and time have elapsed to
                // produce a meaningful estimate — avoids "1t/s" flicker on start.
                let rate_str = if elapsed > 1.0 && *token_count > 5 {
                    let rate = *token_count as f64 / elapsed;
                    format!("  {rate:.0}t/s")
                } else {
                    String::new()
                };
                // Word estimate: ~4.5 chars per word, bucketed to nearest 10 for stability.
                let words = words_estimate(*chars_written);
                let word_str = if words > 0 {
                    format!(" ~{words}w ")
                } else {
                    String::new()
                };
                // Show elapsed time after 5s of streaming so user knows how long they've waited.
                let elapsed_str = format_elapsed_hint(started.elapsed(), 5);
                // Show current section heading if we detected one.
                let section_str = current_section
                    .as_deref()
                    .map(|s| format!(" │ {s}"))
                    .unwrap_or_default();
                left_spans.push(Span::styled(
                    format!(" ▶{word_str}{section_str}{rate_str}{elapsed_str} "),
                    Style::default().fg(Color::Rgb(100, 230, 100)),
                ));
            }
            DisplayState::ToolExec {
                name,
                args_json,
                detail,
                frame: f,
                started,
                ..
            } => {
                let spinner = compact_spinner_frame(*f, params.terminal_glyph_profile);
                let summary = summarize_tools_for_status(params.turn_activity);
                let elapsed_secs = summary
                    .as_ref()
                    .map(|active| active.elapsed_secs)
                    .unwrap_or_else(|| started.elapsed().as_secs());
                // Show elapsed time after 3 s (mirrors Thinking state behaviour).
                // Show the stop hint after 10 s for long-running tools.
                let time_part = if elapsed_secs >= 3 {
                    format!(" {elapsed_secs}s")
                } else {
                    String::new()
                };
                let stop_hint = if elapsed_secs >= 10 { "  ^C=stop" } else { "" };
                let content = if let Some(active) = summary {
                    format!(
                        " {spinner} {} {} {}{time_part}{stop_hint} ",
                        active.verb, active.icon, active.detail
                    )
                } else {
                    let icon = tool_icon(name);
                    let verb = tool_action_verb(name);
                    let preview = detail
                        .as_deref()
                        .filter(|detail| !detail.trim().is_empty())
                        .map(|detail| edgecrab_core::safe_truncate(detail.trim(), 60).to_string())
                        .unwrap_or_else(|| {
                            // Use width-adaptive preview — status bar has more room on wide terminals.
                            let widths = DisplayWidths::from_terminal_width(
                                params.last_terminal_width as usize,
                            );
                            tool_status_preview_width(name, args_json, widths.status_preview)
                        });
                    format!(" {spinner} {verb} {icon} {preview}{time_part}{stop_hint} ")
                };
                // FP48: Use semantic category color so the status bar matches the output area.
                // FP50: Escalate color for slow (>=5s) and stalled (>=15s) tool calls.
                let category_name = params
                    .turn_activity
                    .latest_active_tool()
                    .map(|(_, row)| row.name.as_str())
                    .unwrap_or(name.as_str());
                let base_color = tool_category(category_name).name_color();
                let bar_color = if elapsed_secs >= 15 {
                    Color::Rgb(255, 140, 50) // orange: stalled / very slow
                } else if elapsed_secs >= 5 {
                    Color::Rgb(255, 200, 80) // amber: slow
                } else {
                    base_color
                };
                left_spans.push(Span::styled(content, Style::default().fg(bar_color)));
            }
            DisplayState::BgOp {
                label,
                frame: f,
                started,
            } => {
                let spinner = compact_spinner_frame(*f, params.terminal_glyph_profile);
                let elapsed = started.elapsed().as_secs();
                let msg = if elapsed > 3 {
                    format!(" {spinner} {label} {elapsed}s ")
                } else {
                    format!(" {spinner} {label} ")
                };
                left_spans.push(Span::styled(
                    msg,
                    Style::default().fg(Color::Rgb(180, 180, 255)),
                ));
            }
            DisplayState::Idle => {
                if let Some(flash) = params.goal_flash_status {
                    left_spans.push(Span::styled(
                        format!(" {flash} "),
                        goal_flash_badge_style(flash),
                    ));
                } else if let Some(outcome) = params.last_run_outcome {
                    left_spans.push(Span::styled(
                        format!(
                            " {} {} ",
                            outcome.state.emoji(),
                            outcome.state.compact_label()
                        ),
                        run_outcome_badge_style(outcome),
                    ));
                } else {
                    left_spans.push(Span::raw(" "));
                }
            }
            DisplayState::WaitingForClarify => {
                // Agent is paused waiting for a user reply to a clarifying question.
                // Show a distinct amber label so the user knows input is expected.
                left_spans.push(Span::styled(
                    " ❓ Waiting for reply ",
                    Style::default()
                        .fg(Color::Rgb(255, 220, 80))
                        .add_modifier(Modifier::BOLD),
                ));
            }
            DisplayState::WaitingForApproval { command, .. } => {
                // Agent is waiting for a risk-graduated approval from the user.
                let short = if command.len() > 30 {
                    format!("{}…", edgecrab_core::safe_truncate(command, 27))
                } else {
                    command.clone()
                };
                left_spans.push(Span::styled(
                    format!(" ⚠  Approve: {short} "),
                    Style::default()
                        .fg(Color::Rgb(255, 140, 0))
                        .add_modifier(Modifier::BOLD),
                ));
            }
            DisplayState::SecretCapture {
                var_name, is_sudo, ..
            } => {
                // Agent is waiting for a secret value from the user.
                let label = if *is_sudo {
                    format!(" 🔒 sudo: {var_name} ")
                } else {
                    format!(" 🔑 secret: {var_name} ")
                };
                left_spans.push(Span::styled(
                    label,
                    Style::default()
                        .fg(Color::Rgb(255, 80, 80))
                        .add_modifier(Modifier::BOLD),
                ));
            }
            DisplayState::ValueCapture { title, .. } => {
                left_spans.push(Span::styled(
                    format!(" ⛵ Editing: {title} "),
                    Style::default()
                        .fg(Color::Rgb(120, 220, 200))
                        .add_modifier(Modifier::BOLD),
                ));
            }
        }
    }

    if let Some(metrics) = metrics_from_turn(params.turn_activity) {
        let hud = format_spawn_hud(&metrics, &params.spawn_hud_caps);
        let hud_color = match spawn_hud_severity(&metrics, &params.spawn_hud_caps) {
            SpawnHudSeverity::Error => Color::Rgb(239, 83, 80),
            SpawnHudSeverity::Warn => Color::Rgb(255, 200, 80),
            SpawnHudSeverity::Muted => Color::Rgb(120, 130, 150),
        };
        left_spans.push(Span::styled(hud, Style::default().fg(hud_color)));
    }

    if let Some(pause) = format_spawn_pause_chip() {
        left_spans.push(Span::styled(
            pause,
            Style::default()
                .fg(Color::Rgb(255, 200, 80))
                .add_modifier(Modifier::BOLD),
        ));
    }

    left_spans.push(Span::styled(
        "│",
        Style::default().fg(Color::Rgb(50, 50, 65)),
    ));

    if let Some(overlay) = params.document_overlay.as_ref() {
        left_spans.push(Span::styled(
            format!(
                " {} {} ",
                overlay.icon.as_str(),
                unicode_trunc(overlay.title.as_str(), 28)
            ),
            Style::default()
                .fg(overlay.accent)
                .add_modifier(Modifier::BOLD),
        ));
        left_spans.push(Span::styled(
            "│",
            Style::default().fg(Color::Rgb(50, 50, 65)),
        ));
    }

    if let Some(chip) = params.goal_status_chip {
        left_spans.push(Span::styled(
            format!(" {} ", chip.label),
            goal_status_chip_style(chip.status),
        ));
        left_spans.push(Span::styled(
            "│",
            Style::default().fg(Color::Rgb(50, 50, 65)),
        ));
    }

    // Model name
    left_spans.push(Span::styled(
        format!(" {} ", params.model_name),
        params.theme.status_bar_model,
    ));

    // Token count with color threshold.
    // When context window is known, show a watermark: `12.4k / 200k (7%)`.
    // Color: green → yellow → red at 50% / 80% of context window.
    let ctx_pct = params
        .context_window
        .and_then(|cw| context_usage_ratio(params.total_tokens, Some(cw)));
    let token_style = if ctx_pct.is_some_and(|p| p > 0.80) || params.total_tokens > 100_000 {
        Style::default().fg(Color::Red)
    } else if ctx_pct.is_some_and(|p| p > 0.50) || params.total_tokens > 50_000 {
        Style::default().fg(Color::Yellow)
    } else {
        params.theme.status_bar_tokens
    };
    let token_display = if let (Some(cw), Some(pct)) = (params.context_window, ctx_pct) {
        format!(
            " {}/{} ({:.0}%)",
            format_token_count(params.total_tokens),
            format_token_count(cw),
            pct * 100.0
        )
    } else {
        format!(" {}", format_token_count(params.total_tokens))
    };
    left_spans.push(Span::styled(token_display, token_style));

    // Cost with color threshold
    let cost_style = if params.session_cost >= 1.0 {
        Style::default().fg(Color::Red)
    } else if params.session_cost >= 0.10 {
        Style::default().fg(Color::Yellow)
    } else {
        params.theme.status_bar_cost
    };
    left_spans.push(Span::styled(
        format!(" ${:.4}", params.session_cost),
        cost_style,
    ));

    // ── Context pressure gauge ───────────────────────────────────
    if params.context_window.is_some() {
        left_spans.push(Span::styled(
            " │ ",
            Style::default().fg(Color::Rgb(50, 50, 65)),
        ));
        let gauge_spans =
            build_context_gauge(params.total_tokens, params.context_window.unwrap_or(0));
        left_spans.extend(gauge_spans);
    }

    if let Some(presence) = params.voice_presence {
        left_spans.push(Span::styled(
            " │ ",
            Style::default().fg(Color::Rgb(50, 50, 65)),
        ));
        let style = match presence {
            VoicePresenceState::Recording { .. } => Style::default()
                .fg(Color::Rgb(30, 20, 20))
                .bg(Color::Rgb(240, 110, 90))
                .add_modifier(Modifier::BOLD),
            VoicePresenceState::Speaking => Style::default()
                .fg(Color::Rgb(10, 24, 38))
                .bg(Color::Rgb(120, 210, 255))
                .add_modifier(Modifier::BOLD),
            VoicePresenceState::Listening => Style::default()
                .fg(Color::Rgb(18, 32, 26))
                .bg(Color::Rgb(120, 225, 165))
                .add_modifier(Modifier::BOLD),
        };
        left_spans.push(Span::styled(
            format_voice_presence_badge(presence, params.voice_presence_frame_idx),
            style,
        ));
    }
    if !params.active_subagents.is_empty() {
        left_spans.push(Span::styled(
            " │ ",
            Style::default().fg(Color::Rgb(50, 50, 65)),
        ));
        left_spans.push(Span::styled(
            format!(" DG {} ", params.active_subagents.len()),
            Style::default()
                .fg(Color::Rgb(10, 24, 38))
                .bg(Color::Rgb(95, 170, 255))
                .add_modifier(Modifier::BOLD),
        ));
        if let Some(summary) = format_subagent_status_summary(params.active_subagents) {
            left_spans.push(Span::styled(
                format!(" {summary} "),
                Style::default()
                    .fg(Color::Rgb(165, 205, 245))
                    .add_modifier(Modifier::DIM),
            ));
        }
    }
    if !params.background_tasks_active.is_empty() {
        left_spans.push(Span::styled(
            " │ ",
            Style::default().fg(Color::Rgb(50, 50, 65)),
        ));
        left_spans.push(Span::styled(
            format!(" BG {} ", params.background_tasks_active.len()),
            Style::default()
                .fg(Color::Rgb(20, 20, 28))
                .bg(Color::Rgb(110, 180, 255))
                .add_modifier(Modifier::BOLD),
        ));
        if let Some(summary) = format_background_status_summary(params.background_tasks_active) {
            left_spans.push(Span::styled(
                format!(" {summary} "),
                Style::default()
                    .fg(Color::Rgb(180, 220, 255))
                    .add_modifier(Modifier::DIM),
            ));
        }
    }

    // ── Steering indicator ────────────────────────────────────────────
    // Show pending count in amber, or a brief "applied" flash in green
    // (fades after 3 seconds).
    if params.pending_steer_count > 0 {
        left_spans.push(Span::styled(
            " │ ",
            Style::default().fg(Color::Rgb(50, 50, 65)),
        ));
        left_spans.push(Span::styled(
            format!(" ⛵ {} pending ", params.pending_steer_count),
            Style::default()
                .fg(Color::Rgb(20, 20, 28))
                .bg(Color::Rgb(255, 190, 50))
                .add_modifier(Modifier::BOLD),
        ));
    } else if params
        .steer_applied_at
        .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(4))
    {
        left_spans.push(Span::styled(
            " │ ",
            Style::default().fg(Color::Rgb(50, 50, 65)),
        ));
        left_spans.push(Span::styled(
            " ⛵ applied ",
            Style::default()
                .fg(Color::Rgb(18, 32, 26))
                .bg(Color::Rgb(100, 215, 140))
                .add_modifier(Modifier::BOLD),
        ));
    }

    // ── Shadow Judge indicator ────────────────────────────────────────
    // Show a compact " SJ " badge when the completion oracle is active.
    if params.shadow_judge_enabled {
        left_spans.push(Span::styled(
            " │ ",
            Style::default().fg(Color::Rgb(50, 50, 65)),
        ));
        left_spans.push(Span::styled(
            " SJ ",
            Style::default()
                .fg(Color::Rgb(18, 32, 26))
                .bg(Color::Rgb(130, 200, 255))
                .add_modifier(Modifier::BOLD),
        ));
    }
    if params
        .shadow_judge_intervention_at
        .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(10))
    {
        let confidence = params
            .shadow_judge_intervention_confidence
            .map(|c| (c * 100.0).clamp(0.0, 100.0));
        let reason = params
            .shadow_judge_intervention_text
            .map(|text| edgecrab_core::safe_truncate(text, 42).to_string())
            .unwrap_or_else(|| "continuation requested".to_string());
        left_spans.push(Span::styled(
            " │ ",
            Style::default().fg(Color::Rgb(50, 50, 65)),
        ));
        left_spans.push(Span::styled(
            if let Some(conf) = confidence {
                format!(" SJ veto {conf:.0}%: {reason} ")
            } else {
                format!(" SJ veto: {reason} ")
            },
            Style::default()
                .fg(Color::Rgb(30, 22, 8))
                .bg(Color::Rgb(255, 200, 90))
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Right side: keyboard hints + turn counter
    let mut right_spans = Vec::new();
    if params.turn_count > 0 {
        right_spans.push(Span::styled(
            format!(" turn {} ", params.turn_count),
            Style::default().fg(Color::Rgb(80, 90, 110)),
        ));
        right_spans.push(Span::styled(
            "│",
            Style::default().fg(Color::Rgb(50, 50, 65)),
        ));
    }
    if params.scroll_offset > 0 {
        right_spans.push(Span::styled(
            format!(" ↑SCROLLED  ^G=↓  ↕scroll  {} ", params.paging_key_hint),
            Style::default()
                .fg(Color::Rgb(255, 210, 50))
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        // ── Mode pill ─────────────────────────────────────────────────────
        // Always visible so the user knows the active mode and the key to
        // switch.  SCROLL (green) = mouse capture on, wheel scrolls output.
        //           SELECT (amber) = mouse capture off, native drag=copy.
        if params.mouse_capture_enabled {
            right_spans.push(Span::styled(
                " SCROLL ",
                Style::default()
                    .fg(Color::Rgb(20, 20, 28))
                    .bg(Color::Rgb(60, 185, 105))
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            right_spans.push(Span::styled(
                " SELECT ",
                Style::default()
                    .fg(Color::Rgb(20, 20, 28))
                    .bg(Color::Rgb(255, 200, 50))
                    .add_modifier(Modifier::BOLD),
            ));
        }
        // ── State-specific hints ──────────────────────────────────────────
        if !params.mouse_capture_enabled {
            right_spans.push(Span::styled(
                " drag=copy  F6=scroll  Tab=complete  ^C=cancel ",
                Style::default()
                    .fg(Color::Rgb(255, 210, 50))
                    .add_modifier(Modifier::BOLD),
            ));
        } else if params.clarify_pending {
            // Agent is awaiting a reply — emphasise the prompt so users know
            // their next Enter submits an answer, not a new conversation turn.
            right_spans.push(Span::styled(
                " ↵=send reply  ^C=cancel  ↕scroll ",
                Style::default()
                    .fg(Color::Rgb(255, 220, 80))
                    .add_modifier(Modifier::BOLD),
            ));
        } else if params.is_processing {
            right_spans.push(Span::styled(
                " ^C=cancel  ^S=steer  ↕scroll ",
                Style::default().fg(Color::Rgb(70, 75, 95)),
            ));
        } else if matches!(params.editor_mode, StatusBarEditorMode::ComposeInsert) {
            right_spans.push(Span::styled(
                " COMPOSE ",
                Style::default()
                    .fg(Color::Rgb(20, 20, 28))
                    .bg(Color::Rgb(90, 200, 150))
                    .add_modifier(Modifier::BOLD),
            ));
            right_spans.push(Span::styled(
                " INSERT  ↵=newline  ^S=send  Esc=normal ",
                Style::default().fg(Color::Rgb(90, 210, 170)),
            ));
        } else if matches!(params.editor_mode, StatusBarEditorMode::ComposeNormal) {
            right_spans.push(Span::styled(
                " COMPOSE ",
                Style::default()
                    .fg(Color::Rgb(20, 20, 28))
                    .bg(Color::Rgb(255, 191, 0))
                    .add_modifier(Modifier::BOLD),
            ));
            right_spans.push(Span::styled(
                params.compose_normal_hint,
                Style::default().fg(Color::Rgb(255, 210, 80)),
            ));
        } else if !params.active_skills.is_empty() {
            // Show active skill names so the user knows which skills are loaded.
            // Typing /skill_name again deactivates; /skills opens the browser.
            let names = params.active_skills.join(" + ");
            right_spans.push(Span::styled(
                format!(" 📚 {names}  F6=select  /skill off  ^C=cancel "),
                Style::default()
                    .fg(Color::Rgb(100, 210, 120))
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            let voice_hint = if params.voice_push_to_talk_key.is_empty() {
                String::new()
            } else {
                format!(" {}=voice ", params.voice_push_to_talk_key.to_uppercase())
            };
            right_spans.push(Span::styled(
                    format!(
                        " F6=select  F1=help  {}  F2=model  F3=skills  F7=vision{} Tab=complete  ^C=cancel ",
                        params.inline_compose_hint,
                        voice_hint
                    ),
                    Style::default().fg(Color::Rgb(70, 75, 95)),
                ));
        }
    }

    // Build two-sided status bar
    let right_line = Line::from(right_spans);
    let right_text = right_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>();
    // WHY .width() not .len(): multi-byte Unicode chars (↑↓↕ = 3 bytes, 📚 = 4 bytes)
    // inflate .len() past the terminal column count, causing right_area.right() to
    // exceed the ratatui buffer bounds → panic. UnicodeWidthStr gives display cols.
    let right_width = (right_text.width() as u16).min(area.width);

    let left_area = Rect {
        width: area.width.saturating_sub(right_width),
        ..area
    };
    let right_area = Rect {
        x: area.x + area.width.saturating_sub(right_width),
        width: right_width,
        ..area
    };

    let status =
        Paragraph::new(Line::from(left_spans)).style(Style::default().bg(Color::Rgb(30, 30, 38)));
    frame.render_widget(status, left_area);

    let right_status = Paragraph::new(right_line)
        .style(Style::default().bg(Color::Rgb(30, 30, 38)))
        .alignment(Alignment::Right);
    frame.render_widget(right_status, right_area);
}

fn render_compact_status_bar_inner(frame: &mut Frame, area: Rect, params: &StatusBarRenderParams) {
    let glyphs = params.terminal_glyph_profile;
    let divider = " | ";
    let state = if params.turn_activity.enabled
        && matches!(params.turn_activity.phase, ShelfPhase::GeneratingTool)
    {
        params
            .turn_activity
            .tool_summary()
            .map(|s| {
                format!(
                    "{} prep {}",
                    compact_spinner_frame(params.shelf_spinner_frame, glyphs),
                    s.primary_name.replace('_', " ")
                )
            })
            .unwrap_or_else(|| "prep tool".into())
    } else {
        match params.display_state {
            DisplayState::Idle => params
                .goal_flash_status
                .map(|s| s.to_string())
                .or_else(|| {
                    params.last_run_outcome.map(|outcome| {
                        format!(
                            "{} {}",
                            outcome.state.emoji(),
                            outcome.state.compact_label()
                        )
                    })
                })
                .unwrap_or_else(|| "idle".to_string()),
            DisplayState::AwaitingFirstToken { frame, started } => format!(
                "{} wait {}s",
                compact_spinner_frame(*frame, glyphs),
                started.elapsed().as_secs()
            ),
            DisplayState::Thinking { frame, started } => format!(
                "{} think {}s",
                compact_spinner_frame(*frame, glyphs),
                started.elapsed().as_secs()
            ),
            DisplayState::Streaming {
                token_count,
                chars_written,
                started,
                ..
            } => {
                let secs = started.elapsed().as_secs().max(1);
                let words = words_estimate(*chars_written);
                if words > 0 {
                    format!("reply ~{}w {}t/s", words, token_count / secs)
                } else {
                    format!("reply {}tok {}t/s", token_count, token_count / secs)
                }
            }
            DisplayState::ToolExec { frame, started, .. } => format!(
                "{} tool {}s",
                compact_spinner_frame(*frame, glyphs),
                started.elapsed().as_secs()
            ),
            DisplayState::BgOp { frame, label, .. } => format!(
                "{} {}",
                compact_spinner_frame(*frame, glyphs),
                edgecrab_core::safe_truncate(label, 18)
            ),
            DisplayState::WaitingForClarify => "reply needed".into(),
            DisplayState::WaitingForApproval { .. } => "approval needed".into(),
            DisplayState::SecretCapture { .. } => "secret input".into(),
            DisplayState::ValueCapture { .. } => "editing".into(),
        }
    };
    let token_display = if let (Some(cw), Some(pct)) = (
        params.context_window,
        context_usage_ratio(params.total_tokens, params.context_window),
    ) {
        format!(
            "{}/{} {:.0}%",
            format_token_count(params.total_tokens),
            format_token_count(cw),
            pct * 100.0
        )
    } else {
        format_token_count(params.total_tokens)
    };
    let mode = if params.mouse_capture_enabled {
        "scroll"
    } else {
        "select"
    };
    let transport = if params.remote_terminal_session {
        "ssh"
    } else {
        "local"
    };
    let profile = match params.terminal_ui_profile {
        StatusBarUiProfile::Standard => "std",
        StatusBarUiProfile::ReducedMotion => "calm",
        StatusBarUiProfile::BasicCompat => "compat",
    };
    let right = format!(
        "t{}{}{}{}",
        params.turn_count,
        divider,
        mode,
        if params.remote_terminal_session {
            " ssh"
        } else {
            ""
        }
    );
    let goal_part = params
        .goal_status_chip
        .map(|chip| format!("{divider}{}", chip.label))
        .unwrap_or_default();
    let left = format!(
        "{}{}{}{}{}{}{}${:.4}{}{}{}{}{}{}",
        state,
        goal_part,
        divider,
        edgecrab_core::safe_truncate(params.model_name, 18),
        divider,
        token_display,
        divider,
        params.session_cost,
        divider,
        transport,
        divider,
        profile,
        if params.shadow_judge_enabled {
            " | SJ"
        } else {
            ""
        },
        if params
            .shadow_judge_intervention_at
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(10))
        {
            " | SJ veto"
        } else {
            ""
        },
    );
    let right_width = right.width().min(area.width as usize) as u16;
    let left_area = Rect {
        width: area.width.saturating_sub(right_width),
        ..area
    };
    let right_area = Rect {
        x: area.right().saturating_sub(right_width),
        width: right_width,
        ..area
    };
    let bg = Style::default().bg(Color::Rgb(30, 30, 38));
    frame.render_widget(
        Paragraph::new(Span::styled(
            edgecrab_core::safe_truncate(&left, left_area.width as usize),
            bg.fg(Color::Rgb(200, 205, 215)),
        ))
        .style(bg),
        left_area,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            edgecrab_core::safe_truncate(&right, right_area.width as usize),
            bg.fg(Color::Rgb(120, 130, 150)),
        ))
        .style(bg)
        .alignment(Alignment::Right),
        right_area,
    );
}
