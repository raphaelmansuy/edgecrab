//! Skills Marketplace FSM + install theatre (019 W2 + polish).
//!
//! Pure keymap/state helpers live here; App owns async I/O and reuses Skill Guard
//! for the Gate stage.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

/// Accent shared with remote-skills browser chrome (DRY vs one-off themes).
pub const MARKETPLACE_ACCENT: Color = Color::Rgb(110, 220, 210);
pub const MARKETPLACE_WARN: Color = Color::Rgb(255, 191, 0);
pub const MARKETPLACE_BG: Color = Color::Rgb(16, 18, 24);

/// Provider filter cycle — derived from hub catalog SoT (+ registry keys).
pub fn marketplace_provider_filters() -> &'static [&'static str] {
    edgecrab_tools::tools::skills_hub::marketplace_provider_filters()
}

/// Peer import-from aliases — DRY with hub [`IMPORT_FROM_PEERS`].
pub const IMPORT_FROM_PEERS: &[&str] = edgecrab_tools::tools::skills_hub::IMPORT_FROM_PEERS;

/// Install pipeline stages — DRY with hub [`InstallStage`](edgecrab_tools::tools::skills_hub::InstallStage).
pub use edgecrab_tools::tools::skills_hub::InstallStage;

/// High-level marketplace mode (owns installed + remote + import).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MarketplaceMode {
    /// Local installed skills (marketplace home).
    #[default]
    BrowseInstalled,
    /// Remote search / browse.
    SearchRemote,
    /// Viewing selected remote skill dossier (SKILL.md-first).
    Inspect {
        identifier: String,
        preview_scroll: u16,
    },
    /// Peer import-from picker.
    ImportFrom {
        selected: usize,
    },
    /// Marketplace source/provider picker (browse by source).
    SourcePick {
        selected: usize,
    },
    /// Staged install in progress.
    Installing {
        identifier: String,
        stage: InstallStage,
    },
    /// Safe path: confirm before commit (orientation already shown).
    ConfirmSafe {
        identifier: String,
        name: String,
    },
    /// Guard overlay owns input; marketplace remembers return query.
    GuardReview {
        identifier: String,
        preserved_query: String,
    },
    Done {
        name: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarketplaceAction {
    Noop,
    GoSearchRemote,
    GoBrowseInstalled,
    Refresh,
    CycleProviderFilter,
    PrevProviderFilter,
    JumpSource(usize),
    OpenImportFrom,
    OpenSourcePick,
    ImportPeer(usize),
    ImportFromMoveUp,
    ImportFromMoveDown,
    SourcePickMoveUp,
    SourcePickMoveDown,
    SelectSource(usize),
    ToggleHelp,
    InspectSelected,
    /// From SearchRemote: enter Inspect first if not yet inspected, else install.
    RequestInstall,
    StartInstall,
    OpenEvidence,
    RetryPreview,
    ScrollInspectUp,
    ScrollInspectDown,
    ConfirmSafeInstall,
    Back,
    Close,
    DismissDone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketplaceKeyContext {
    pub mode: MarketplaceModeKind,
    pub has_selection: bool,
    pub help_visible: bool,
    pub import_count: usize,
    pub import_selected: usize,
    /// True when SearchRemote query is empty (browse mode — digits jump sources).
    pub query_empty: bool,
    pub source_count: usize,
    pub source_selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketplaceModeKind {
    BrowseInstalled,
    SearchRemote,
    Inspect,
    ImportFrom,
    SourcePick,
    Installing,
    ConfirmSafe,
    GuardReview,
    Done,
    Error,
}

impl MarketplaceMode {
    pub fn kind(&self) -> MarketplaceModeKind {
        match self {
            Self::BrowseInstalled => MarketplaceModeKind::BrowseInstalled,
            Self::SearchRemote => MarketplaceModeKind::SearchRemote,
            Self::Inspect { .. } => MarketplaceModeKind::Inspect,
            Self::ImportFrom { .. } => MarketplaceModeKind::ImportFrom,
            Self::SourcePick { .. } => MarketplaceModeKind::SourcePick,
            Self::Installing { .. } => MarketplaceModeKind::Installing,
            Self::ConfirmSafe { .. } => MarketplaceModeKind::ConfirmSafe,
            Self::GuardReview { .. } => MarketplaceModeKind::GuardReview,
            Self::Done { .. } => MarketplaceModeKind::Done,
            Self::Error { .. } => MarketplaceModeKind::Error,
        }
    }
}

/// Human label for a marketplace provider filter key.
pub fn provider_filter_label(key: &str) -> String {
    match key.to_ascii_lowercase().as_str() {
        "all" => "All".into(),
        "openai" => "OpenAI".into(),
        "anthropic" => "Anthropic".into(),
        "huggingface" => "HF".into(),
        "nvidia" => "NVIDIA".into(),
        "gstack" => "gstack".into(),
        "voltagent" => "VoltAgent".into(),
        "minimax" => "MiniMax".into(),
        "clawhub" => "ClawHub".into(),
        "skills-sh" => "skills.sh".into(),
        other => other.to_string(),
    }
}

/// Cycle provider filter string; returns next filter label (`all` clears → None).
pub fn next_provider_filter(current: Option<&str>) -> Option<&'static str> {
    step_provider_filter(current, 1)
}

/// Previous provider filter (`all` clears → None).
pub fn prev_provider_filter(current: Option<&str>) -> Option<&'static str> {
    step_provider_filter(current, -1)
}

fn step_provider_filter(current: Option<&str>, delta: isize) -> Option<&'static str> {
    let cur = current.unwrap_or("all");
    let filters = marketplace_provider_filters();
    let idx = filters
        .iter()
        .position(|p| (*p).eq_ignore_ascii_case(cur))
        .unwrap_or(0);
    let len = filters.len() as isize;
    let next_idx = ((idx as isize + delta).rem_euclid(len)) as usize;
    let next = filters[next_idx];
    if next == "all" { None } else { Some(next) }
}

/// Jump to Nth filter (0-based). Returns `None` for `all`.
pub fn provider_filter_at(index: usize) -> Option<&'static str> {
    let filters = marketplace_provider_filters();
    let key = *filters.get(index)?;
    if key == "all" { None } else { Some(key) }
}

/// Pure keymap for marketplace chrome (GuardReview keys stay in skill_trust_overlay).
pub fn map_marketplace_key(
    code: KeyCode,
    modifiers: KeyModifiers,
    ctx: MarketplaceKeyContext,
) -> MarketplaceAction {
    if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
        return MarketplaceAction::Noop;
    }
    if matches!(ctx.mode, MarketplaceModeKind::GuardReview) {
        return MarketplaceAction::Noop;
    }

    match (ctx.mode, code) {
        (_, KeyCode::Char('?')) => MarketplaceAction::ToggleHelp,
        (MarketplaceModeKind::Done | MarketplaceModeKind::Error, KeyCode::Enter | KeyCode::Esc) => {
            MarketplaceAction::DismissDone
        }
        (MarketplaceModeKind::ConfirmSafe, KeyCode::Enter | KeyCode::Char('i')) => {
            MarketplaceAction::ConfirmSafeInstall
        }
        (MarketplaceModeKind::ConfirmSafe, KeyCode::Esc) => MarketplaceAction::Back,
        (MarketplaceModeKind::Installing, KeyCode::Esc) => MarketplaceAction::Back,
        (MarketplaceModeKind::Inspect, KeyCode::Esc) => MarketplaceAction::Back,
        (MarketplaceModeKind::Inspect, KeyCode::Char('i')) if ctx.has_selection => {
            MarketplaceAction::StartInstall
        }
        (MarketplaceModeKind::Inspect, KeyCode::Char('e')) => MarketplaceAction::OpenEvidence,
        (MarketplaceModeKind::Inspect, KeyCode::Char('s')) => MarketplaceAction::RetryPreview,
        (MarketplaceModeKind::Inspect, KeyCode::Up | KeyCode::Char('k')) => {
            MarketplaceAction::ScrollInspectUp
        }
        (MarketplaceModeKind::Inspect, KeyCode::Down | KeyCode::Char('j')) => {
            MarketplaceAction::ScrollInspectDown
        }
        // BrowseInstalled / SearchRemote: lowercase letters type into the filter.
        // Letter actions use UPPERCASE (Shift) — same convention as selector_action_key.
        (MarketplaceModeKind::BrowseInstalled, KeyCode::Char('/') | KeyCode::Char('S')) => {
            MarketplaceAction::GoSearchRemote
        }
        (MarketplaceModeKind::BrowseInstalled, KeyCode::Char('M')) => {
            MarketplaceAction::OpenImportFrom
        }
        (MarketplaceModeKind::BrowseInstalled, KeyCode::Esc) => MarketplaceAction::Close,
        (MarketplaceModeKind::SearchRemote, KeyCode::Enter) if ctx.has_selection => {
            MarketplaceAction::InspectSelected
        }
        (MarketplaceModeKind::SearchRemote, KeyCode::Char('I')) if ctx.has_selection => {
            MarketplaceAction::RequestInstall
        }
        (MarketplaceModeKind::SearchRemote, KeyCode::Char('R')) => MarketplaceAction::Refresh,
        (MarketplaceModeKind::SearchRemote, KeyCode::Char('P') | KeyCode::Char(']')) => {
            MarketplaceAction::CycleProviderFilter
        }
        (MarketplaceModeKind::SearchRemote, KeyCode::Char('[')) => {
            MarketplaceAction::PrevProviderFilter
        }
        (MarketplaceModeKind::SearchRemote, KeyCode::Left) if ctx.query_empty => {
            MarketplaceAction::PrevProviderFilter
        }
        (MarketplaceModeKind::SearchRemote, KeyCode::Right) if ctx.query_empty => {
            MarketplaceAction::CycleProviderFilter
        }
        (MarketplaceModeKind::SearchRemote, KeyCode::Char('S')) => {
            MarketplaceAction::OpenSourcePick
        }
        (MarketplaceModeKind::SearchRemote, KeyCode::Char(c @ '1'..='9')) if ctx.query_empty => {
            let idx = (c as u8 - b'1') as usize;
            if idx < ctx.source_count {
                MarketplaceAction::JumpSource(idx)
            } else {
                MarketplaceAction::Noop
            }
        }
        (MarketplaceModeKind::SearchRemote, KeyCode::Char('M')) => {
            MarketplaceAction::OpenImportFrom
        }
        (MarketplaceModeKind::SearchRemote, KeyCode::Char('L')) => {
            MarketplaceAction::GoBrowseInstalled
        }
        // Already in SearchRemote — `/` is a no-op (search field owns typing).
        (MarketplaceModeKind::SearchRemote, KeyCode::Char('/')) => MarketplaceAction::Noop,
        (MarketplaceModeKind::SearchRemote, KeyCode::Esc) => MarketplaceAction::GoBrowseInstalled,
        (MarketplaceModeKind::ImportFrom, KeyCode::Esc) => MarketplaceAction::Back,
        (MarketplaceModeKind::ImportFrom, KeyCode::Enter) => MarketplaceAction::ImportPeer(
            ctx.import_selected.min(ctx.import_count.saturating_sub(1)),
        ),
        (MarketplaceModeKind::ImportFrom, KeyCode::Up | KeyCode::Char('k')) => {
            MarketplaceAction::ImportFromMoveUp
        }
        (MarketplaceModeKind::ImportFrom, KeyCode::Down | KeyCode::Char('j')) => {
            MarketplaceAction::ImportFromMoveDown
        }
        (MarketplaceModeKind::ImportFrom, KeyCode::Char(c @ '1'..='5')) => {
            let idx = (c as u8 - b'1') as usize;
            if idx < ctx.import_count {
                MarketplaceAction::ImportPeer(idx)
            } else {
                MarketplaceAction::Noop
            }
        }
        (MarketplaceModeKind::SourcePick, KeyCode::Esc) => MarketplaceAction::Back,
        (MarketplaceModeKind::SourcePick, KeyCode::Enter) => MarketplaceAction::SelectSource(
            ctx.source_selected.min(ctx.source_count.saturating_sub(1)),
        ),
        (MarketplaceModeKind::SourcePick, KeyCode::Up | KeyCode::Char('k')) => {
            MarketplaceAction::SourcePickMoveUp
        }
        (MarketplaceModeKind::SourcePick, KeyCode::Down | KeyCode::Char('j')) => {
            MarketplaceAction::SourcePickMoveDown
        }
        (MarketplaceModeKind::SourcePick, KeyCode::Char(c @ '1'..='9')) => {
            let idx = (c as u8 - b'1') as usize;
            if idx < ctx.source_count {
                MarketplaceAction::SelectSource(idx)
            } else {
                MarketplaceAction::Noop
            }
        }
        _ => MarketplaceAction::Noop,
    }
}

/// Default action index for Skill Guard: prefer Cancel over Force/Trust.
pub fn default_skill_trust_selected_action(needs_trust: bool, review_only: bool) -> usize {
    let count = crate::skill_trust_overlay::skill_trust_action_count(needs_trust, review_only);
    count.saturating_sub(1)
}

pub fn marketplace_footer_help(help_expanded: bool, mode: MarketplaceModeKind) -> String {
    if help_expanded {
        return "↑↓ move  Enter inspect dossier  I install (Shift)  e evidence (Inspect)\n\
         [ ] or ←→ source  S pick  1-9 jump (empty query)  P next source  M import  L local  R refresh\n\
         Type lowercase to filter · Shift+letter for actions · Esc back · ? collapse\n\
         Inspect: SKILL.md first · capabilities · files · trust teaser\n\
         Stages: Fetch → Quarantine → Scan → Gate (Skill Guard) → Commit\n\
         Guard: f force (caution)  t trust+install (dangerous)  v jump to finding"
            .into();
    }
    match mode {
        MarketplaceModeKind::BrowseInstalled => {
            "/ or S Skills Hub browse  M import-from  Enter activate  Esc close  ? help".into()
        }
        MarketplaceModeKind::ImportFrom => {
            "↑↓ peer  Enter/1-5 import  Esc back  ? help".into()
        }
        MarketplaceModeKind::SourcePick => {
            "↑↓ source  Enter/1-9 select  Esc back  ? help".into()
        }
        MarketplaceModeKind::Inspect => {
            "↑↓ scroll  i install  e evidence  s retry scan  Esc back  ? help".into()
        }
        MarketplaceModeKind::ConfirmSafe => {
            "Enter or i confirm Safe install  Esc cancel  ? help".into()
        }
        MarketplaceModeKind::SearchRemote => {
            "Skills Hub · type to search  [ ] source  S pick  Enter inspect  I install  R retry  Esc back  ? help"
                .into()
        }
        _ => {
            "Enter inspect  I install  [ ] source  S pick  Esc back  ? help".into()
        }
    }
}

pub fn render_install_theatre(
    frame: &mut Frame,
    area: Rect,
    identifier: &str,
    stage: InstallStage,
    status_line: Option<&str>,
    stage_elapsed: Option<std::time::Duration>,
) {
    render_marketplace_popup(
        frame,
        area,
        &format!(" Skill Install · {} ", unicode_truncate(identifier, 40)),
        |_inner| {
            let current = stage.index();
            let elapsed_hint = stage_elapsed
                .map(|d| {
                    let secs = d.as_secs();
                    if secs == 0 {
                        String::new()
                    } else {
                        format!(" {secs}s")
                    }
                })
                .unwrap_or_default();
            let mut lines = Vec::new();
            for (i, s) in InstallStage::ALL.iter().enumerate() {
                let (marker, style) = if i < current {
                    ("●", Style::default().fg(Color::Rgb(120, 200, 140)))
                } else if i == current {
                    (
                        "▶",
                        Style::default()
                            .fg(MARKETPLACE_WARN)
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    ("○", Style::default().fg(Color::Rgb(90, 95, 105)))
                };
                let suffix = if i < current {
                    "done".to_string()
                } else if i == current {
                    format!("running…{elapsed_hint}")
                } else {
                    String::new()
                };
                lines.push(Line::from(vec![
                    Span::styled(format!(" {marker} "), style),
                    Span::styled(format!("{:<12}", s.label()), style),
                    Span::styled(suffix, Style::default().fg(Color::Rgb(140, 145, 155))),
                ]));
            }
            if let Some(status) = status_line {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    unicode_truncate(status, 60),
                    Style::default().fg(Color::Rgb(160, 170, 180)),
                )));
            }
            lines.push(Line::from(Span::styled(
                " Esc cancel · Gate uses Skill Guard ",
                Style::default().fg(Color::DarkGray),
            )));
            lines
        },
    );
}

pub fn render_marketplace_banner(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    body: &str,
    accent: Color,
) {
    render_marketplace_popup(frame, area, title, |_| {
        vec![
            Line::from(Span::styled(
                body.to_string(),
                Style::default().fg(Color::Rgb(200, 210, 210)),
            )),
            Line::from(""),
            Line::from(Span::styled(
                " Enter or Esc to continue ",
                Style::default().fg(accent),
            )),
        ]
    });
}

/// Confirm-safe banner with explicit Install / Cancel affordances.
pub fn render_confirm_safe_banner(frame: &mut Frame, area: Rect, name: &str) {
    render_marketplace_popup(frame, area, " Skill Install · Confirm ", |_| {
        vec![
            Line::from(Span::styled(
                format!("Install `{name}` — Skill Guard: Safe."),
                Style::default().fg(Color::Rgb(200, 210, 210)),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    " [Install] ",
                    Style::default()
                        .fg(MARKETPLACE_BG)
                        .bg(MARKETPLACE_ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled(
                    " [Cancel] ",
                    Style::default()
                        .fg(Color::Rgb(200, 210, 210))
                        .bg(Color::Rgb(50, 55, 65)),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                " Enter confirm · Esc cancel · click a button ",
                Style::default().fg(Color::DarkGray),
            )),
        ]
    });
}

/// Hit-test ConfirmSafe button row inside the marketplace popup.
/// Returns `Some(true)` for Install, `Some(false)` for Cancel, `None` if neither.
pub fn confirm_safe_button_hit(area: Rect, col: u16, row: u16) -> Option<bool> {
    let popup = marketplace_popup_rect(area);
    if !rect_contains_cell(popup, col, row) {
        return None;
    }
    // Button row sits ~4 rows below the popup top (title + body + blank).
    let button_row = popup.y.saturating_add(4);
    if row != button_row {
        // Also accept adjacent rows inside the popup for fat-finger clicks.
        if row < popup.y.saturating_add(3) || row > popup.y.saturating_add(5) {
            return None;
        }
    }
    let rel = col.saturating_sub(popup.x.saturating_add(1));
    // Approximate: " [Install] " (~10) + "  " + " [Cancel] " (~10) from left margin.
    if rel < 12 {
        Some(true)
    } else if rel < 28 {
        Some(false)
    } else {
        None
    }
}

/// Geometry of centered marketplace modals (ImportFrom / SourcePick / banners).
/// Shared by render + mouse hit-testing (click-outside dismiss).
pub fn marketplace_popup_rect(area: Rect) -> Rect {
    let popup_w = area.width.saturating_sub(4).min(72);
    let popup_h = 14u16;
    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    Rect::new(x, y, popup_w, popup_h)
}

/// True when `(col, row)` falls inside `rect` (inclusive of edges).
pub fn rect_contains_cell(rect: Rect, col: u16, row: u16) -> bool {
    col >= rect.x
        && row >= rect.y
        && col < rect.x.saturating_add(rect.width)
        && row < rect.y.saturating_add(rect.height)
}

fn render_marketplace_popup(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    lines_fn: impl FnOnce(Rect) -> Vec<Line<'static>>,
) {
    frame.render_widget(Clear, area);

    let popup = marketplace_popup_rect(area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(MARKETPLACE_ACCENT))
        .title(Span::styled(
            title.to_string(),
            Style::default()
                .fg(MARKETPLACE_ACCENT)
                .add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(MARKETPLACE_BG));
    frame.render_widget(block, popup);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Min(1)])
        .split(popup);

    let lines = lines_fn(inner[0]);
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Left), inner[0]);
}

pub fn render_import_from_picker(frame: &mut Frame, area: Rect, selected: usize) {
    render_marketplace_popup(frame, area, " Import skills from peer ", |_| {
        let mut lines = vec![Line::from(Span::styled(
            "Quarantine → scan → gate (same pipeline)",
            Style::default().fg(Color::Rgb(140, 150, 160)),
        ))];
        lines.push(Line::from(""));
        for (i, peer) in IMPORT_FROM_PEERS.iter().enumerate() {
            let marker = if i == selected { "▶" } else { " " };
            let style = if i == selected {
                Style::default()
                    .fg(MARKETPLACE_ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(180, 190, 190))
            };
            lines.push(Line::from(Span::styled(
                format!(" {marker} {}. {peer}", i + 1),
                style,
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Enter import · Esc back ",
            Style::default().fg(Color::DarkGray),
        )));
        lines
    });
}

/// Source/marketplace picker — browse by provider without typing a query.
pub fn render_source_picker(frame: &mut Frame, area: Rect, selected: usize) {
    let filters = marketplace_provider_filters();
    render_marketplace_popup(frame, area, " Browse by source ", |_| {
        let mut lines = vec![Line::from(Span::styled(
            "Select a marketplace — empty query lists skills for that source",
            Style::default().fg(Color::Rgb(140, 150, 160)),
        ))];
        lines.push(Line::from(""));
        for (i, key) in filters.iter().enumerate() {
            let marker = if i == selected { "▶" } else { " " };
            let style = if i == selected {
                Style::default()
                    .fg(MARKETPLACE_ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(180, 190, 190))
            };
            let label = provider_filter_label(key);
            lines.push(Line::from(Span::styled(
                format!(" {marker} {}. {label}", i + 1),
                style,
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Enter select · Esc back ",
            Style::default().fg(Color::DarkGray),
        )));
        lines
    });
}

/// Compact source chip strip for wide terminals.
///
/// When `loading`, the active chip gets a spinner accent so browse fan-out
/// never looks frozen mid chip-jump.
pub fn source_chip_spans(active_filter: Option<&str>, loading: bool) -> Vec<Span<'static>> {
    let active = active_filter.unwrap_or("all");
    let mut spans = vec![Span::styled(
        " Sources ",
        Style::default().fg(Color::Rgb(140, 150, 160)),
    )];
    for key in marketplace_provider_filters() {
        let label = provider_filter_label(key);
        let is_active = key.eq_ignore_ascii_case(active);
        let style = if is_active {
            Style::default()
                .fg(MARKETPLACE_ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(100, 110, 120))
        };
        let text = if is_active && loading {
            format!("[⠋ {label}] ")
        } else if is_active {
            format!("[{label}] ")
        } else {
            format!("{label} ")
        };
        spans.push(Span::styled(text, style));
    }
    spans
}

/// Dim placeholder lines for the detail pane while a catalog fetch is in flight.
pub fn skeleton_detail_lines(filter_label: &str, browse: bool) -> Vec<Line<'static>> {
    let verb = if browse { "Browsing" } else { "Searching" };
    vec![
        Line::from(Span::styled(
            format!("{verb} {filter_label}"),
            Style::default()
                .fg(MARKETPLACE_ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "████████████████░░░░░░░░",
            Style::default().fg(Color::Rgb(45, 52, 62)),
        )),
        Line::from(Span::styled(
            "████████░░░░░░░░░░░░░░░░",
            Style::default().fg(Color::Rgb(40, 46, 56)),
        )),
        Line::from(Span::styled(
            "██████████████░░░░░░░░░░",
            Style::default().fg(Color::Rgb(45, 52, 62)),
        )),
        Line::from(Span::styled(
            "██████░░░░░░░░░░░░░░░░░░",
            Style::default().fg(Color::Rgb(40, 46, 56)),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Esc or click header/footer to leave · list stays interactive",
            Style::default().fg(Color::Rgb(120, 120, 135)),
        )),
    ]
}

fn unicode_truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max.saturating_sub(1) {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    if out.is_empty() {
        s.chars().take(max).collect()
    } else {
        out
    }
}

/// Apply a stage update to marketplace mode.
/// Returns `true` when the stage value changed (caller should reset elapsed timer).
pub fn advance_install_stage(
    mode: &mut MarketplaceMode,
    identifier: &str,
    stage: InstallStage,
) -> bool {
    let changed = match mode {
        MarketplaceMode::Installing {
            identifier: cur_id,
            stage: cur_stage,
        } => cur_id != identifier || *cur_stage != stage,
        _ => true,
    };
    *mode = MarketplaceMode::Installing {
        identifier: identifier.to_string(),
        stage,
    };
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browse_slash_goes_search() {
        assert_eq!(
            map_marketplace_key(
                KeyCode::Char('/'),
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::BrowseInstalled,
                    has_selection: false,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: false,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::GoSearchRemote
        );
    }

    #[test]
    fn browse_shift_s_goes_search_lowercase_s_is_noop() {
        let ctx = MarketplaceKeyContext {
            mode: MarketplaceModeKind::BrowseInstalled,
            has_selection: false,
            help_visible: false,
            import_count: IMPORT_FROM_PEERS.len(),
            import_selected: 0,
            query_empty: false,
            source_count: marketplace_provider_filters().len(),
            source_selected: 0,
        };
        assert_eq!(
            map_marketplace_key(KeyCode::Char('S'), KeyModifiers::NONE, ctx),
            MarketplaceAction::GoSearchRemote
        );
        assert_eq!(
            map_marketplace_key(KeyCode::Char('s'), KeyModifiers::NONE, ctx),
            MarketplaceAction::Noop
        );
    }

    #[test]
    fn browse_installed_footer_mentions_marketplace() {
        let help = marketplace_footer_help(false, MarketplaceModeKind::BrowseInstalled);
        assert!(
            help.contains("Skills Hub")
                || help.contains("search remote")
                || help.contains("/ or S")
        );
    }

    #[test]
    fn search_remote_s_opens_source_pick() {
        assert_eq!(
            map_marketplace_key(
                KeyCode::Char('S'),
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::SearchRemote,
                    has_selection: false,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: true,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::OpenSourcePick
        );
    }

    #[test]
    fn search_remote_brackets_cycle_source() {
        let ctx = MarketplaceKeyContext {
            mode: MarketplaceModeKind::SearchRemote,
            has_selection: true,
            help_visible: false,
            import_count: IMPORT_FROM_PEERS.len(),
            import_selected: 0,
            query_empty: true,
            source_count: marketplace_provider_filters().len(),
            source_selected: 0,
        };
        assert_eq!(
            map_marketplace_key(KeyCode::Char(']'), KeyModifiers::NONE, ctx),
            MarketplaceAction::CycleProviderFilter
        );
        assert_eq!(
            map_marketplace_key(KeyCode::Char('['), KeyModifiers::NONE, ctx),
            MarketplaceAction::PrevProviderFilter
        );
    }

    #[test]
    fn search_remote_digit_jumps_source_when_browsing() {
        assert_eq!(
            map_marketplace_key(
                KeyCode::Char('2'),
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::SearchRemote,
                    has_selection: false,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: true,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::JumpSource(1)
        );
        // Digits do not jump while typing a search query.
        assert_eq!(
            map_marketplace_key(
                KeyCode::Char('2'),
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::SearchRemote,
                    has_selection: false,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: false,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::Noop
        );
    }

    #[test]
    fn source_pick_enter_selects() {
        assert_eq!(
            map_marketplace_key(
                KeyCode::Enter,
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::SourcePick,
                    has_selection: false,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: true,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 2,
                }
            ),
            MarketplaceAction::SelectSource(2)
        );
    }

    #[test]
    fn search_remote_footer_mentions_source_pick() {
        let help = marketplace_footer_help(false, MarketplaceModeKind::SearchRemote);
        assert!(help.contains("source") || help.contains("S pick"));
        assert!(help.contains("inspect"));
    }

    #[test]
    fn prev_provider_filter_wraps() {
        let filters = marketplace_provider_filters();
        assert!(!filters.is_empty());
        // From all, previous wraps to last non-all or None if last is all.
        let prev = prev_provider_filter(None);
        if let Some(last) = filters.last().copied() {
            if last == "all" {
                assert!(prev.is_none());
            } else {
                assert_eq!(prev, Some(last));
            }
        }
    }

    #[test]
    fn search_esc_returns_to_installed() {
        assert_eq!(
            map_marketplace_key(
                KeyCode::Esc,
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::SearchRemote,
                    has_selection: true,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: false,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::GoBrowseInstalled
        );
    }

    #[test]
    fn search_shift_p_cycles_provider_lowercase_p_is_noop() {
        let ctx = MarketplaceKeyContext {
            mode: MarketplaceModeKind::SearchRemote,
            has_selection: false,
            help_visible: false,
            import_count: IMPORT_FROM_PEERS.len(),
            import_selected: 0,
            query_empty: false,
            source_count: marketplace_provider_filters().len(),
            source_selected: 0,
        };
        assert_eq!(
            map_marketplace_key(KeyCode::Char('P'), KeyModifiers::NONE, ctx),
            MarketplaceAction::CycleProviderFilter
        );
        assert_eq!(
            map_marketplace_key(KeyCode::Char('p'), KeyModifiers::NONE, ctx),
            MarketplaceAction::Noop
        );
        assert_eq!(next_provider_filter(None), Some("openai"));
        assert_eq!(next_provider_filter(Some("openai")), Some("anthropic"));
        assert_eq!(next_provider_filter(Some("skills-sh")), None); // wraps to all
    }

    #[test]
    fn search_enter_inspects_when_selected() {
        assert_eq!(
            map_marketplace_key(
                KeyCode::Enter,
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::SearchRemote,
                    has_selection: true,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: false,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::InspectSelected
        );
    }

    #[test]
    fn search_shift_i_requests_install_lowercase_i_is_noop() {
        let ctx = MarketplaceKeyContext {
            mode: MarketplaceModeKind::SearchRemote,
            has_selection: true,
            help_visible: false,
            import_count: IMPORT_FROM_PEERS.len(),
            import_selected: 0,
            query_empty: false,
            source_count: marketplace_provider_filters().len(),
            source_selected: 0,
        };
        assert_eq!(
            map_marketplace_key(KeyCode::Char('I'), KeyModifiers::NONE, ctx),
            MarketplaceAction::RequestInstall
        );
        assert_eq!(
            map_marketplace_key(KeyCode::Char('i'), KeyModifiers::NONE, ctx),
            MarketplaceAction::Noop
        );
    }

    #[test]
    fn inspect_e_opens_evidence() {
        assert_eq!(
            map_marketplace_key(
                KeyCode::Char('e'),
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::Inspect,
                    has_selection: true,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: false,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::OpenEvidence
        );
    }

    #[test]
    fn inspect_scroll_and_retry() {
        assert_eq!(
            map_marketplace_key(
                KeyCode::Down,
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::Inspect,
                    has_selection: true,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: false,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::ScrollInspectDown
        );
        assert_eq!(
            map_marketplace_key(
                KeyCode::Char('s'),
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::Inspect,
                    has_selection: true,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: false,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::RetryPreview
        );
    }

    #[test]
    fn confirm_safe_enter_commits() {
        assert_eq!(
            map_marketplace_key(
                KeyCode::Enter,
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::ConfirmSafe,
                    has_selection: true,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: false,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::ConfirmSafeInstall
        );
        assert_eq!(
            map_marketplace_key(
                KeyCode::Esc,
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::ConfirmSafe,
                    has_selection: true,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: false,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::Back
        );
    }

    #[test]
    fn inspect_footer_mentions_evidence() {
        let help = marketplace_footer_help(false, MarketplaceModeKind::Inspect);
        assert!(help.contains("evidence"));
        assert!(help.contains("install"));
        let search = marketplace_footer_help(false, MarketplaceModeKind::SearchRemote);
        assert!(search.contains("inspect"));
        assert!(!search.to_lowercase().contains("enter install"));
    }

    #[test]
    fn search_enter_noop_without_selection() {
        assert_eq!(
            map_marketplace_key(
                KeyCode::Enter,
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::SearchRemote,
                    has_selection: false,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: false,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::Noop
        );
    }

    #[test]
    fn esc_from_inspect_goes_back() {
        assert_eq!(
            map_marketplace_key(
                KeyCode::Esc,
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::Inspect,
                    has_selection: true,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: false,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::Back
        );
    }

    #[test]
    fn guard_review_keys_are_noop_here() {
        assert_eq!(
            map_marketplace_key(
                KeyCode::Char('t'),
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::GuardReview,
                    has_selection: true,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: false,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::Noop
        );
    }

    #[test]
    fn question_toggles_help() {
        assert_eq!(
            map_marketplace_key(
                KeyCode::Char('?'),
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::SearchRemote,
                    has_selection: false,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: false,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::ToggleHelp
        );
    }

    #[test]
    fn default_trust_action_is_cancel() {
        assert_eq!(default_skill_trust_selected_action(false, false), 1);
        assert_eq!(default_skill_trust_selected_action(true, false), 2);
        assert_eq!(default_skill_trust_selected_action(false, true), 0);
    }

    #[test]
    fn stage_order() {
        assert_eq!(InstallStage::Fetch.index(), 0);
        assert_eq!(InstallStage::Commit.index(), 4);
        assert_eq!(InstallStage::ALL.len(), 5);
    }

    #[test]
    fn marketplace_popup_rect_hit_test() {
        let area = Rect::new(0, 0, 80, 24);
        let popup = marketplace_popup_rect(area);
        assert!(rect_contains_cell(popup, popup.x, popup.y));
        assert!(rect_contains_cell(
            popup,
            popup.x + popup.width / 2,
            popup.y + popup.height / 2
        ));
        assert!(!rect_contains_cell(popup, 0, 0));
        assert!(!rect_contains_cell(popup, area.width - 1, area.height - 1));
    }

    #[test]
    fn import_digit_selects_peer() {
        assert_eq!(
            map_marketplace_key(
                KeyCode::Char('2'),
                KeyModifiers::NONE,
                MarketplaceKeyContext {
                    mode: MarketplaceModeKind::ImportFrom,
                    has_selection: false,
                    help_visible: false,
                    import_count: IMPORT_FROM_PEERS.len(),
                    import_selected: 0,
                    query_empty: false,
                    source_count: marketplace_provider_filters().len(),
                    source_selected: 0,
                }
            ),
            MarketplaceAction::ImportPeer(1)
        );
    }

    #[test]
    fn render_install_theatre_smoke() {
        use ratatui::{Terminal, backend::TestBackend};

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|frame| {
                render_install_theatre(
                    frame,
                    frame.area(),
                    "openai/skill-creator",
                    InstallStage::Scan,
                    Some("scanning quarantine bundle"),
                    Some(std::time::Duration::from_secs(2)),
                );
            })
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let flat: String = buffer
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect();
        assert!(flat.contains("Fetch"));
        assert!(flat.contains("Scan"));
        assert!(flat.contains("Gate"));
        assert!(flat.contains("Commit"));
    }
}
