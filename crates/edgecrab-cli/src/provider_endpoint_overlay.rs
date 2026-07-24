//! Exceptional TUI for per-provider base URL overrides.
//!
//! Features:
//! - Browse all configurable providers with live URL + source badge
//! - Edit / clear / reset-to-default
//! - Async-friendly probe status (caller runs probe and feeds result)
//! - Keyboard-first: ↑↓ · Enter edit · r reset · p probe · / filter · Esc

use edgecrab_core::provider_endpoints::{
    self, EndpointSource, PROVIDER_ENDPOINT_SPECS, ProviderEndpointConfig, ProviderEndpointSpec,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use std::collections::HashMap;

use crate::overlay_layout::{picker_three_layout, picker_two_cols, popup_rect};
use crate::picker_chrome::selector_marker;

const ACCENT: Color = Color::Rgb(120, 200, 255);
const ACCENT_DIM: Color = Color::Rgb(70, 130, 180);
const GOOD: Color = Color::Rgb(100, 210, 140);
const WARN: Color = Color::Rgb(230, 180, 80);
const BAD: Color = Color::Rgb(230, 100, 100);
const MUTED: Color = Color::Rgb(90, 105, 125);
const BG_SEL: Color = Color::Rgb(22, 38, 55);
const BG: Color = Color::Rgb(12, 16, 22);
const WHITE: Color = Color::Rgb(220, 230, 240);

#[derive(Debug, Clone)]
pub enum EndpointOverlayPhase {
    Browse,
    Edit {
        provider_id: String,
        buffer: String,
        error: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct ProviderEndpointOverlay {
    pub phase: EndpointOverlayPhase,
    pub cursor: usize,
    /// Optional case-insensitive substring filter.
    pub filter: String,
    pub filter_active: bool,
    /// Last probe message per provider id.
    pub probe: HashMap<String, ProbeStatus>,
    /// When true, show local-only providers first (default true for Mac power users).
    pub locals_first: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeStatus {
    Idle,
    Pending,
    Ok(String),
    Err(String),
}

impl Default for ProviderEndpointOverlay {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderEndpointOverlay {
    pub fn new() -> Self {
        Self {
            phase: EndpointOverlayPhase::Browse,
            cursor: 0,
            filter: String::new(),
            filter_active: false,
            probe: HashMap::new(),
            locals_first: true,
        }
    }

    pub fn filtered_specs(&self) -> Vec<&'static ProviderEndpointSpec> {
        let mut specs: Vec<_> = PROVIDER_ENDPOINT_SPECS.iter().collect();
        if self.locals_first {
            specs.sort_by_key(|s| (!s.local, s.label));
        } else {
            specs.sort_by_key(|s| s.label);
        }
        let q = self.filter.trim().to_ascii_lowercase();
        if q.is_empty() {
            return specs;
        }
        specs
            .into_iter()
            .filter(|s| {
                s.id.contains(&q)
                    || s.label.to_ascii_lowercase().contains(&q)
                    || s.description.to_ascii_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn selected_spec(&self) -> Option<&'static ProviderEndpointSpec> {
        let specs = self.filtered_specs();
        specs.get(self.cursor).copied()
    }

    pub fn clamp_cursor(&mut self) {
        let n = self.filtered_specs().len();
        if n == 0 {
            self.cursor = 0;
            return;
        }
        if self.cursor >= n {
            self.cursor = n - 1;
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let n = self.filtered_specs().len() as isize;
        if n == 0 {
            return;
        }
        let next = (self.cursor as isize + delta).rem_euclid(n) as usize;
        self.cursor = next;
    }

    pub fn begin_edit(&mut self, config_url: Option<&str>) {
        let Some(spec) = self.selected_spec() else {
            return;
        };
        let buffer = config_url.map(str::to_string).unwrap_or_else(|| {
            provider_endpoints::effective_base_url(spec.id)
                .map(|(u, _)| u)
                .unwrap_or_else(|| spec.default_base_url.to_string())
        });
        self.phase = EndpointOverlayPhase::Edit {
            provider_id: spec.id.to_string(),
            buffer,
            error: None,
        };
    }

    pub fn cancel_edit(&mut self) {
        self.phase = EndpointOverlayPhase::Browse;
    }
}

/// Build display rows using config map.
pub fn row_url_and_source(
    spec: &ProviderEndpointSpec,
    config_map: &HashMap<String, ProviderEndpointConfig>,
) -> (String, EndpointSource) {
    provider_endpoints::resolve_endpoint(spec.id, config_map)
        .unwrap_or_else(|| (spec.default_base_url.to_string(), EndpointSource::Default))
}

pub fn render_endpoint_overlay(
    frame: &mut Frame,
    area: Rect,
    state: &ProviderEndpointOverlay,
    config_map: &HashMap<String, ProviderEndpointConfig>,
) {
    let popup = popup_rect(area, 92, 26);
    frame.render_widget(Clear, popup);

    match &state.phase {
        EndpointOverlayPhase::Browse => render_browse(frame, popup, state, config_map),
        EndpointOverlayPhase::Edit {
            provider_id,
            buffer,
            error,
        } => render_edit(frame, popup, provider_id, buffer, error.as_deref()),
    }
}

fn render_browse(
    frame: &mut Frame,
    popup: Rect,
    state: &ProviderEndpointOverlay,
    config_map: &HashMap<String, ProviderEndpointConfig>,
) {
    let chunks = picker_three_layout(popup);
    let body = picker_two_cols(chunks[1], 52);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  ⬡  ", Style::default().fg(ACCENT)),
        Span::styled(
            "Provider Endpoints",
            Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            "base URL overrides for every provider",
            Style::default().fg(MUTED),
        ),
        if state.filter_active || !state.filter.is_empty() {
            Span::styled(
                format!("  filter: {}", state.filter),
                Style::default().fg(WARN),
            )
        } else {
            Span::raw("")
        },
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ACCENT_DIM))
            .title(" /endpoint ")
            .style(Style::default().bg(BG)),
    );
    frame.render_widget(header, chunks[0]);

    let specs = state.filtered_specs();
    let items: Vec<ListItem> = specs
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let is_cursor = i == state.cursor;
            let (url, src) = row_url_and_source(spec, config_map);
            let bg = if is_cursor { BG_SEL } else { BG };
            let marker = selector_marker(is_cursor, ACCENT, Some(bg));
            let local_badge = if spec.local {
                Span::styled(" local ", Style::default().fg(GOOD).bg(bg))
            } else {
                Span::styled(" cloud ", Style::default().fg(MUTED).bg(bg))
            };
            let src_color = match src {
                EndpointSource::Config => WARN,
                EndpointSource::Env => ACCENT,
                EndpointSource::Default => MUTED,
            };
            let probe = state
                .probe
                .get(spec.id)
                .cloned()
                .unwrap_or(ProbeStatus::Idle);
            let probe_span = match probe {
                ProbeStatus::Idle => Span::styled(" · ", Style::default().fg(MUTED).bg(bg)),
                ProbeStatus::Pending => Span::styled(" … ", Style::default().fg(WARN).bg(bg)),
                ProbeStatus::Ok(_) => Span::styled(" ✓ ", Style::default().fg(GOOD).bg(bg)),
                ProbeStatus::Err(_) => Span::styled(" ✗ ", Style::default().fg(BAD).bg(bg)),
            };
            let url_short = if url.chars().count() > 36 {
                format!("{}…", url.chars().take(35).collect::<String>())
            } else {
                url
            };
            ListItem::new(Line::from(vec![
                marker,
                Span::styled(
                    format!("{:<10}", spec.label),
                    Style::default()
                        .fg(if is_cursor { ACCENT } else { WHITE })
                        .bg(bg)
                        .add_modifier(if is_cursor {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                local_badge,
                probe_span,
                Span::styled(
                    format!("[{}]", src.label()),
                    Style::default().fg(src_color).bg(bg),
                ),
                Span::raw(" "),
                Span::styled(url_short, Style::default().fg(MUTED).bg(bg)),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ACCENT_DIM))
            .title(" providers ")
            .style(Style::default().bg(BG)),
    );
    frame.render_widget(list, body[0]);

    // Detail pane
    let detail = if let Some(spec) = specs.get(state.cursor) {
        let (url, src) = row_url_and_source(spec, config_map);
        let probe = state
            .probe
            .get(spec.id)
            .cloned()
            .unwrap_or(ProbeStatus::Idle);
        let probe_line = match probe {
            ProbeStatus::Idle => "Probe: not run (press p)".to_string(),
            ProbeStatus::Pending => "Probe: checking /v1/models …".to_string(),
            ProbeStatus::Ok(msg) => format!("Probe: {msg}"),
            ProbeStatus::Err(msg) => format!("Probe failed: {msg}"),
        };
        let lines = vec![
            Line::from(Span::styled(
                spec.label,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(spec.description, Style::default().fg(MUTED))),
            Line::from(""),
            Line::from(vec![
                Span::styled("id     ", Style::default().fg(MUTED)),
                Span::styled(spec.id, Style::default().fg(WHITE)),
            ]),
            Line::from(vec![
                Span::styled("url    ", Style::default().fg(MUTED)),
                Span::styled(url, Style::default().fg(WHITE)),
            ]),
            Line::from(vec![
                Span::styled("source ", Style::default().fg(MUTED)),
                Span::styled(src.label(), Style::default().fg(WARN)),
            ]),
            Line::from(vec![
                Span::styled("default ", Style::default().fg(MUTED)),
                Span::styled(spec.default_base_url, Style::default().fg(MUTED)),
            ]),
            Line::from(vec![
                Span::styled("env    ", Style::default().fg(MUTED)),
                Span::styled(spec.env_keys.join(" · "), Style::default().fg(MUTED)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                probe_line,
                Style::default().fg(match state.probe.get(spec.id) {
                    Some(ProbeStatus::Ok(_)) => GOOD,
                    Some(ProbeStatus::Err(_)) => BAD,
                    Some(ProbeStatus::Pending) => WARN,
                    _ => MUTED,
                }),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Persisted under provider_endpoints.<id>.base_url",
                Style::default().fg(MUTED),
            )),
            Line::from(Span::styled(
                "in ~/.edgecrab/config.yaml — applied to process env.",
                Style::default().fg(MUTED),
            )),
        ];
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ACCENT_DIM))
                .title(" detail ")
                .style(Style::default().bg(BG)),
        )
    } else {
        Paragraph::new("No providers match filter.").block(
            Block::default()
                .borders(Borders::ALL)
                .title(" detail ")
                .style(Style::default().bg(BG)),
        )
    };
    frame.render_widget(detail, body[1]);

    let help = Paragraph::new(Line::from(vec![
        Span::styled(" ↑↓ ", Style::default().fg(ACCENT)),
        Span::styled("nav  ", Style::default().fg(MUTED)),
        Span::styled("Enter ", Style::default().fg(ACCENT)),
        Span::styled("edit  ", Style::default().fg(MUTED)),
        Span::styled("r ", Style::default().fg(ACCENT)),
        Span::styled("reset  ", Style::default().fg(MUTED)),
        Span::styled("p ", Style::default().fg(ACCENT)),
        Span::styled("probe  ", Style::default().fg(MUTED)),
        Span::styled("/ ", Style::default().fg(ACCENT)),
        Span::styled("filter  ", Style::default().fg(MUTED)),
        Span::styled("Esc ", Style::default().fg(ACCENT)),
        Span::styled("close", Style::default().fg(MUTED)),
    ]))
    .style(Style::default().bg(BG));
    frame.render_widget(help, chunks[2]);
}

fn render_edit(
    frame: &mut Frame,
    popup: Rect,
    provider_id: &str,
    buffer: &str,
    error: Option<&str>,
) {
    let spec = provider_endpoints::endpoint_spec(provider_id);
    let label = spec.map(|s| s.label).unwrap_or(provider_id);
    let default = spec.map(|s| s.default_base_url).unwrap_or("");

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Length(4),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(popup);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  ✎  ", Style::default().fg(ACCENT)),
        Span::styled(
            format!("Edit base URL · {label}"),
            Style::default().fg(WHITE).add_modifier(Modifier::BOLD),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ACCENT))
            .title(format!(" {provider_id} "))
            .style(Style::default().bg(BG)),
    );
    frame.render_widget(header, chunks[0]);

    let input = Paragraph::new(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(buffer, Style::default().fg(WHITE)),
        Span::styled("█", Style::default().fg(ACCENT)),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ACCENT))
            .title(" base URL ")
            .style(Style::default().bg(BG)),
    );
    frame.render_widget(input, chunks[1]);

    let mut hints = vec![
        Line::from(Span::styled(
            format!("Default: {default}"),
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            "Enter empty / type clear / default → remove override",
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            "Must be http:// or https:// · trailing /v1 allowed",
            Style::default().fg(MUTED),
        )),
    ];
    if let Some(err) = error {
        hints.insert(
            0,
            Line::from(Span::styled(
                format!("✗ {err}"),
                Style::default().fg(BAD).add_modifier(Modifier::BOLD),
            )),
        );
    }
    frame.render_widget(
        Paragraph::new(hints).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ACCENT_DIM))
                .style(Style::default().bg(BG)),
        ),
        chunks[2],
    );

    let help = Paragraph::new(Line::from(vec![
        Span::styled(" Enter ", Style::default().fg(ACCENT)),
        Span::styled("save  ", Style::default().fg(MUTED)),
        Span::styled("Esc ", Style::default().fg(ACCENT)),
        Span::styled("cancel", Style::default().fg(MUTED)),
    ]))
    .style(Style::default().bg(BG));
    frame.render_widget(help, chunks[4]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_omlx() {
        let mut s = ProviderEndpointOverlay::new();
        s.filter = "omlx".into();
        let specs = s.filtered_specs();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].id, "omlx");
    }

    #[test]
    fn locals_first_orders() {
        let s = ProviderEndpointOverlay::new();
        let specs = s.filtered_specs();
        assert!(specs[0].local);
    }
}
