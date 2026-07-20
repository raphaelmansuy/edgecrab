//! Document, setup wizard, and skin browser overlays.

use super::*;

impl App {
    pub(super) fn render_document_overlay(&self, frame: &mut Frame, area: Rect) {
        let Some(overlay) = self.document_overlay.as_ref() else {
            return;
        };

        frame.render_widget(Clear, area);

        let chunks = Self::browser_overlay_chunks(area);
        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                format!("  {} ", overlay.icon),
                Style::default().fg(overlay.accent),
            ),
            Span::styled(
                if overlay.subtitle.trim().is_empty() {
                    "Read-only report".to_string()
                } else {
                    overlay.subtitle.clone()
                },
                Style::default().fg(Color::White),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(overlay.accent))
                .title(format!(" {} ", overlay.title)),
        );
        frame.render_widget(header, chunks[0]);

        let detail_lines: Vec<Line<'static>> = overlay
            .body
            .lines()
            .map(|line| Line::from(line.to_string()))
            .collect();
        self.render_scrollable_browser_detail(
            frame,
            chunks[1],
            ScrollableDetailChrome {
                title: "Details",
                border_color: overlay.accent,
                focused: true,
                requested_scroll: overlay.scroll,
            },
            detail_lines,
        );

        let help = if overlay.kind == DocumentOverlayKind::Web {
            Paragraph::new(Line::from(vec![
                Span::styled(" ↑↓ ", Style::default().fg(overlay.accent)),
                Span::styled("scroll  ", Style::default().fg(Color::DarkGray)),
                Span::styled("s ", Style::default().fg(overlay.accent)),
                Span::styled("setup  ", Style::default().fg(Color::DarkGray)),
                Span::styled("c ", Style::default().fg(overlay.accent)),
                Span::styled("chain  ", Style::default().fg(Color::DarkGray)),
                Span::styled("d ", Style::default().fg(overlay.accent)),
                Span::styled("doctor  ", Style::default().fg(Color::DarkGray)),
                Span::styled("p ", Style::default().fg(overlay.accent)),
                Span::styled("providers  ", Style::default().fg(Color::DarkGray)),
                Span::styled("h ", Style::default().fg(overlay.accent)),
                Span::styled("help  ", Style::default().fg(Color::DarkGray)),
                Span::styled("r ", Style::default().fg(overlay.accent)),
                Span::styled("refresh  ", Style::default().fg(Color::DarkGray)),
                Span::styled("b ", Style::default().fg(overlay.accent)),
                Span::styled("hub  ", Style::default().fg(Color::DarkGray)),
                Span::styled("Esc ", Style::default().fg(overlay.accent)),
                Span::styled("close", Style::default().fg(Color::DarkGray)),
            ]))
        } else {
            Paragraph::new(Line::from(vec![
                Span::styled(" ↑↓ ", Style::default().fg(overlay.accent)),
                Span::styled("scroll line  ", Style::default().fg(Color::DarkGray)),
                self.paging_key_help_span(overlay.accent),
                Span::styled("scroll page  ", Style::default().fg(Color::DarkGray)),
                Span::styled("Home/End ", Style::default().fg(overlay.accent)),
                Span::styled("jump  ", Style::default().fg(Color::DarkGray)),
                Span::styled("Esc ", Style::default().fg(overlay.accent)),
                Span::styled("close", Style::default().fg(Color::DarkGray)),
            ]))
        };
        frame.render_widget(help, chunks[2]);
    }

    pub(super) fn render_web_setup_tui(&self, frame: &mut Frame, area: Rect) {
        use crate::web_setup_tui::WebSetupScreen;
        use ratatui::widgets::{Block, Borders, List, Paragraph, Wrap};

        frame.render_widget(Clear, area);
        let chunks = picker_three_layout(area);
        let body = picker_two_cols(chunks[1], 38);

        let setup = &self.web_setup;
        let accent = crate::web_command::WEB_ACCENT;

        let title = match setup.screen {
            WebSetupScreen::Configure => " /web — search priority ",
            WebSetupScreen::ConfirmAuto => " Reset to auto? ",
        };

        let header_lines = if setup.screen == WebSetupScreen::Configure {
            let mut lines = vec![setup.status_line(), setup.chain_summary_line()];
            if let Some(w) = setup.override_warning_line() {
                lines.push(w);
            }
            lines
        } else {
            vec![setup.status_line()]
        };
        frame.render_widget(
            Paragraph::new(header_lines).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(180, 130, 50)))
                    .title(title),
            ),
            chunks[0],
        );

        let list_items = if setup.screen == WebSetupScreen::ConfirmAuto {
            vec![ListItem::new(Line::from(Span::styled(
                "  Clear custom chain and use auto (best configured backend)?",
                Style::default().fg(Color::Rgb(255, 210, 150)),
            )))]
        } else {
            setup.build_list_items(accent)
        };

        frame.render_widget(
            List::new(list_items).block(
                Block::default()
                    .borders(Borders::LEFT | Borders::RIGHT)
                    .title(" Providers "),
            ),
            body[0],
        );

        let mut detail = setup.selected_provider_detail();
        if setup.screen == WebSetupScreen::ConfirmAuto {
            detail = vec![
                Line::from("Reset removes your custom priority order."),
                Line::from("EdgeCrab picks the best backends that have credentials."),
                Line::from(""),
                Line::from("y or Enter — confirm   n or Esc — cancel"),
            ];
        }
        if let Some(ref toast) = setup.toast {
            detail.push(Line::from(""));
            detail.push(Line::from(Span::styled(
                toast.clone(),
                Style::default().fg(Color::Rgb(140, 220, 160)),
            )));
        }

        frame.render_widget(
            Paragraph::new(detail).wrap(Wrap { trim: true }).block(
                Block::default()
                    .borders(Borders::LEFT | Borders::RIGHT)
                    .title(" Details "),
            ),
            body[1],
        );

        let help = if setup.screen == WebSetupScreen::ConfirmAuto {
            Line::from(Span::styled(
                " y/Enter confirm · n/Esc cancel ",
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            crate::web_setup_tui::WebSetupTui::help_line()
        };
        frame.render_widget(Paragraph::new(help), chunks[2]);
    }

    pub(super) fn render_proxy_setup_tui(&self, frame: &mut Frame, area: Rect) {
        use crate::proxy_setup_tui::ProxySetupScreen;
        use ratatui::widgets::{Block, Borders, List, Paragraph, Wrap};

        frame.render_widget(Clear, area);
        let chunks = picker_three_layout(area);
        let body = picker_two_cols(chunks[1], 38);

        let setup = &self.proxy_setup;
        let accent = crate::proxy_hub::PROXY_ACCENT;

        let title = match setup.screen {
            ProxySetupScreen::PickPreset => " /proxy — OpenAI bridge (Grok / Nous) ",
            ProxySetupScreen::ConfirmEnable => " Enable upstream? ",
        };

        frame.render_widget(
            Paragraph::new(setup.status_line()).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(accent))
                    .title(title),
            ),
            chunks[0],
        );

        let list_items = if setup.screen == ProxySetupScreen::ConfirmEnable {
            vec![ListItem::new(Line::from(Span::styled(
                "  Confirm enable preset and create proxy token?",
                Style::default().fg(Color::Rgb(180, 220, 255)),
            )))]
        } else {
            setup.build_list_items(accent)
        };

        frame.render_widget(
            List::new(list_items).block(
                Block::default()
                    .borders(Borders::LEFT | Borders::RIGHT)
                    .title(" Presets "),
            ),
            body[0],
        );

        let detail = if setup.screen == ProxySetupScreen::ConfirmEnable {
            setup.confirm_lines()
        } else {
            setup.detail_lines()
        };

        frame.render_widget(
            Paragraph::new(detail).wrap(Wrap { trim: true }).block(
                Block::default()
                    .borders(Borders::LEFT | Borders::RIGHT)
                    .title(" Details "),
            ),
            body[1],
        );

        let help = if setup.screen == ProxySetupScreen::ConfirmEnable {
            crate::proxy_setup_tui::ProxySetupTui::confirm_help_line()
        } else {
            crate::proxy_setup_tui::ProxySetupTui::help_line()
        };
        frame.render_widget(Paragraph::new(help), chunks[2]);
    }

    pub(super) fn render_grok_auth_tui(&self, frame: &mut Frame, area: Rect) {
        use crate::overlay_layout::popup_rect;
        use ratatui::layout::{Constraint, Direction, Layout};
        use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

        // Centered modal — keeps OAuth focused and avoids full-bleed noise.
        let popup_w = area.width.saturating_sub(6).clamp(52, 100);
        let popup_h = area.height.saturating_sub(2).clamp(20, 32);
        let popup = popup_rect(area, popup_w, popup_h);
        frame.render_widget(Clear, area);
        // Dim the rest of the TUI so the modal owns attention.
        frame.render_widget(
            Block::default().style(Style::default().bg(Color::Rgb(12, 14, 18))),
            area,
        );

        let accent = crate::proxy_hub::PROXY_ACCENT;
        // Header (title) · body (progress + instructions) · help bar
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(1),
            ])
            .split(popup);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  🪪  ", Style::default().fg(accent)),
                Span::styled(self.grok_auth.subtitle(), Style::default().fg(Color::White)),
            ]))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(accent))
                    .title(self.grok_auth.title()),
            ),
            chunks[0],
        );

        // Width-aware body: we pre-wrap long OAuth URLs (ratatui wrap alone mid-splits ugly).
        let body_width = chunks[1].width.saturating_sub(2);
        frame.render_widget(
            Paragraph::new(self.grok_auth.body_lines(body_width))
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(accent))
                        .title(" Sign-in "),
                ),
            chunks[1],
        );

        frame.render_widget(
            Paragraph::new(self.grok_auth.help_line()).block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
            chunks[2],
        );
    }

    pub(super) fn render_skin_browser(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let accent = Color::Rgb(255, 150, 80); // warm tangerine
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // search input
                Constraint::Min(1),    // skin list
                Constraint::Length(1), // help line
            ])
            .split(area);

        // ── Search box ───────────────────────────────────────────────
        let search_text = if self.skin_browser.query.is_empty() {
            "Type to filter skins…  (Esc to cancel)".to_string()
        } else {
            self.skin_browser.query.clone()
        };
        let search_style = if self.skin_browser.query.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let search = Paragraph::new(Line::from(vec![
            Span::styled("  🎨 ", Style::default().fg(accent)),
            Span::styled(search_text, search_style),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(accent))
                .title(" Browse Skins  [/skin] "),
        );
        frame.render_widget(search, chunks[0]);

        // ── Skin list ─────────────────────────────────────────────────
        let max_visible = chunks[1].height as usize;
        let filtered = &self.skin_browser.filtered;
        let selected = self.skin_browser.selected;

        let scroll_start = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };

        let name_w = 20usize;

        let items: Vec<ListItem> = filtered
            .iter()
            .skip(scroll_start)
            .take(max_visible)
            .enumerate()
            .map(|(vis_idx, &entry_idx)| {
                let entry = &self.skin_browser.items[entry_idx];
                let is_selected = vis_idx + scroll_start == selected;

                let name_cell = unicode_pad_right(&entry.name, name_w);
                let badge = if entry.is_active { " ✓ active" } else { "" };

                let bg = if is_selected {
                    Color::Rgb(60, 40, 20)
                } else {
                    Color::Reset
                };
                let name_fg = if is_selected {
                    Color::White
                } else {
                    Color::Rgb(220, 180, 100)
                };
                let badge_fg = Color::Rgb(100, 200, 100);

                ListItem::new(Line::from(vec![
                    selector_marker(is_selected, accent, Some(bg)),
                    Span::styled(
                        format!("  {name_cell}"),
                        Style::default().fg(name_fg).bg(bg),
                    ),
                    Span::styled(badge, Style::default().fg(badge_fg).bg(bg)),
                ]))
            })
            .collect();

        let skin_list =
            List::new(items).block(Block::default().borders(Borders::LEFT | Borders::RIGHT));
        frame.render_widget(skin_list, chunks[1]);

        // ── Help line ─────────────────────────────────────────────────
        let help = Paragraph::new(Line::from(vec![
            Span::styled("  ↑↓ ", Style::default().fg(accent)),
            Span::styled("navigate  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter ", Style::default().fg(accent)),
            Span::styled("apply  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(accent)),
            Span::styled("cancel", Style::default().fg(Color::DarkGray)),
        ]));
        frame.render_widget(help, chunks[2]);
    }
}
