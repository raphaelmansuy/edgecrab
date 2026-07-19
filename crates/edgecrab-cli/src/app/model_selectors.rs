//! Model picker overlays (/model, vision, image, MoA experts).

use super::*;

impl App {
    pub(super) fn render_model_like_selector(
        &self,
        frame: &mut Frame,
        area: Rect,
        selector: &FuzzySelector<ModelEntry>,
        detail_surface: DetailSurface,
        chrome: SelectorChrome<'_>,
    ) {
        frame.render_widget(Clear, area);

        let chunks = Self::browser_overlay_chunks(area);
        let body = Self::browser_body_chunks(chunks[1]);
        self.render_browser_header(
            frame,
            chunks[0],
            &selector.query,
            BrowserChrome {
                title: chrome.title,
                placeholder: chrome.placeholder,
                icon: "◈",
                icon_color: Color::Cyan,
                border_color: Color::Cyan,
            },
        );

        let max_visible = body[0].height as usize;
        let filtered = &selector.filtered;
        let selected = selector.selected;
        let scroll_start = Self::browser_scroll_start(selected, max_visible);

        let items: Vec<ListItem> = filtered
            .iter()
            .skip(scroll_start)
            .take(max_visible)
            .enumerate()
            .map(|(vis_idx, &model_idx)| {
                let entry = &selector.items[model_idx];
                let (display, provider) = (&entry.display, &entry.provider);
                let is_selected = vis_idx + scroll_start == selected;
                let bg = if is_selected {
                    Color::Rgb(50, 50, 70)
                } else {
                    Color::Rgb(20, 20, 28)
                };
                let style = if is_selected {
                    Style::default()
                        .bg(bg)
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(200, 200, 200))
                };
                let provider_style = if is_selected {
                    Style::default().bg(bg).fg(Color::Rgb(120, 120, 150))
                } else {
                    Style::default().fg(Color::Rgb(80, 80, 100))
                };
                let mut spans = vec![
                    selector_marker(is_selected, Color::Cyan, Some(bg)),
                    Span::styled(format!("  {:<12}", provider), provider_style),
                    Span::styled(display.clone(), style),
                ];
                if !entry.detail.is_empty() && entry.detail != entry.model_name {
                    let detail_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(160, 160, 180))
                    } else {
                        Style::default().fg(Color::Rgb(110, 110, 130))
                    };
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(entry.detail.clone(), detail_style));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        let model_count = filtered.len();
        let list = List::new(items).style(Style::default().bg(Color::Rgb(20, 20, 28)));
        frame.render_widget(list, body[0]);

        let mut detail_lines = Vec::new();
        if let Some(entry) = selector.current() {
            detail_lines.push(Line::from(Span::styled(
                entry.display.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(vec![
                Span::styled("Provider: ", Style::default().fg(Color::Rgb(145, 170, 170))),
                Span::raw(entry.provider.clone()),
            ]));
            detail_lines.push(Line::from(vec![
                Span::styled("Model: ", Style::default().fg(Color::Rgb(145, 170, 170))),
                Span::raw(entry.model_name.clone()),
            ]));
            if !entry.detail.is_empty() {
                detail_lines.push(Line::from(vec![
                    Span::styled(
                        "Inventory: ",
                        Style::default().fg(Color::Rgb(145, 170, 170)),
                    ),
                    Span::raw(entry.detail.clone()),
                ]));
            }
            detail_lines.push(Line::from(""));
            if entry.provider == "super-grok" {
                detail_lines.push(Line::from(Span::styled(
                    "SuperGrok · subscription OAuth — flagship: super-grok/grok-4.5",
                    Style::default().fg(Color::Rgb(255, 200, 120)),
                )));
                if entry.detail.contains("sign-in") {
                    detail_lines.push(Line::from(Span::styled(
                        "Not signed in — Enter still works: EdgeCrab opens /login grok, then switches for you.",
                        Style::default().fg(Color::Rgb(255, 160, 120)),
                    )));
                } else {
                    detail_lines.push(Line::from(Span::styled(
                        "Signed in — Enter switches immediately (no restart).",
                        Style::default().fg(Color::Rgb(140, 220, 160)),
                    )));
                }
                detail_lines.push(Line::from(""));
            } else if entry.provider == "xai" {
                detail_lines.push(Line::from(Span::styled(
                    "xAI API key (XAI_API_KEY) · OAuth fallback if key unset. Flagship: xai/grok-4.5.",
                    Style::default().fg(Color::Rgb(160, 180, 200)),
                )));
                detail_lines.push(Line::from(""));
            }
            detail_lines.push(Line::from(
                "Use Enter to switch immediately. Plain typing keeps refining the list without triggering actions.",
            ));
        } else if selector.query.trim().is_empty() {
            detail_lines.push(Line::from(Span::styled(
                chrome.title.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Browse models with fuzzy search over provider, model name, and discovery source.",
            ));
            if let Some(note) = chrome.status_note {
                detail_lines.push(Line::from(""));
                detail_lines.push(Line::from(note.to_string()));
            }
        } else {
            detail_lines.push(Line::from("No models matched this query."));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Try a broader provider name or a shorter model fragment.",
            ));
        }
        if self.detail_fullscreen_active(detail_surface) {
            self.render_fullscreen_browser_detail(
                frame,
                area,
                FullscreenBrowserChrome {
                    query: &selector.query,
                    header: BrowserChrome {
                        title: chrome.title,
                        placeholder: chrome.placeholder,
                        icon: "◈",
                        icon_color: Color::Cyan,
                        border_color: Color::Cyan,
                    },
                    detail: ScrollableDetailChrome {
                        title: "Details",
                        border_color: Color::Cyan,
                        focused: true,
                        requested_scroll: self.detail_fullscreen_scroll(detail_surface),
                    },
                    help: Line::from(vec![
                        Span::styled(" ↑↓ ", Style::default().fg(Color::Cyan)),
                        Span::styled("change item  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("type ", Style::default().fg(Color::Cyan)),
                        Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
                        self.paging_key_help_span(Color::Cyan),
                        Span::styled("scroll detail  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Enter ", Style::default().fg(Color::Cyan)),
                        Span::styled("select  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Z ", Style::default().fg(Color::Cyan)),
                        Span::styled("split view  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Esc ", Style::default().fg(Color::Cyan)),
                        Span::styled("close  ", Style::default().fg(Color::DarkGray)),
                    ]),
                },
                detail_lines,
            );
            return;
        }
        self.render_standard_split_detail(
            frame,
            body[1],
            detail_surface,
            Color::Cyan,
            detail_lines,
        );

        let mut help_spans = vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Cyan)),
            Span::styled("browse  ", Style::default().fg(Color::DarkGray)),
            Span::styled("type ", Style::default().fg(Color::Cyan)),
            Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
        ];
        help_spans.extend(self.focus_pane_help_spans(Color::Cyan));
        help_spans.extend(self.page_or_scroll_help_spans(Color::Cyan));
        help_spans.extend([
            Span::styled("Enter ", Style::default().fg(Color::Cyan)),
            Span::styled("select  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Z ", Style::default().fg(Color::Cyan)),
            Span::styled("detail  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Cyan)),
            Span::styled("cancel  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "{model_count} {} · {} pane",
                    chrome.count_label,
                    self.simple_split_focus_label(detail_surface)
                ),
                Style::default().fg(Color::Rgb(80, 80, 100)),
            ),
            Span::styled(
                chrome
                    .status_note
                    .map(|note| format!("  {note}"))
                    .unwrap_or_default(),
                Style::default().fg(Color::Yellow),
            ),
        ]);
        if detail_surface == DetailSurface::ModelSelector {
            help_spans.push(Span::styled(
                disconnect_help_suffix(),
                Style::default().fg(Color::Rgb(110, 110, 130)),
            ));
        }
        let help = Paragraph::new(Line::from(help_spans));
        frame.render_widget(help, chunks[2]);
    }

    /// Render the full-screen model selector overlay.
    pub(super) fn render_model_selector(&self, frame: &mut Frame, area: Rect) {
        if let ModelPickerStage::ExpensiveConfirm {
            ref model,
            ref message,
            ..
        } = self.model_selector_stage
        {
            render_expensive_confirm(frame, area, model, message, &self.theme);
            return;
        }
        if let ModelPickerStage::DisconnectConfirm { ref provider } = self.model_selector_stage {
            render_disconnect_confirm(frame, area, provider, &self.theme);
            return;
        }
        let base_title = match self.model_selector_target {
            ModelSelectorTarget::Primary => "Select Model",
            ModelSelectorTarget::Cheap => "Select Cheap Model",
            ModelSelectorTarget::MoaAggregator => "Select MoA Aggregator",
        };
        let title = if self.model_selector_refresh_in_flight {
            format!("{base_title} · refreshing live inventory")
        } else {
            base_title.to_string()
        };
        let placeholder = if self.model_selector_refresh_in_flight {
            "Type to filter models... live discovery updates in place (Esc to cancel)"
        } else {
            "Type to filter models... (Esc to cancel)"
        };
        self.render_model_like_selector(
            frame,
            area,
            &self.model_selector,
            DetailSurface::ModelSelector,
            SelectorChrome {
                title: &title,
                placeholder,
                count_label: "models",
                status_note: model_selector_status_hint(
                    &self.model_selector,
                    self.model_selector_refresh_in_flight,
                    &self.model_name,
                )
                .as_deref(),
            },
        );
    }

    /// Render the full-screen vision-model selector overlay.
    pub(super) fn render_vision_model_selector(&self, frame: &mut Frame, area: Rect) {
        self.render_model_like_selector(
            frame,
            area,
            &self.vision_model_selector,
            DetailSurface::VisionModelSelector,
            SelectorChrome {
                title: "Select Vision Model",
                placeholder: "Type to filter vision backends... (Esc to cancel)",
                count_label: "options",
                status_note: None,
            },
        );
    }

    pub(super) fn render_image_model_selector(&self, frame: &mut Frame, area: Rect) {
        self.render_model_like_selector(
            frame,
            area,
            &self.image_model_selector,
            DetailSurface::ImageModelSelector,
            SelectorChrome {
                title: "Select Image Model",
                placeholder: "Type to filter image-generation backends... (Esc to cancel)",
                count_label: "image backends",
                status_note: None,
            },
        );
    }

    pub(super) fn render_moa_reference_selector(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let (title, placeholder, action_hint, count_hint) = match self.moa_reference_selector_mode {
            MoaReferenceSelectorMode::EditRoster => (
                " Select MoA Experts ",
                "Type to filter expert models…",
                "Space toggle  ",
                format!("{} selected", self.moa_reference_selected.len()),
            ),
            MoaReferenceSelectorMode::AddExpert => (
                " Add MoA Expert ",
                "Type to find an expert to add…",
                "Enter add  ",
                format!("{} configured", self.moa_reference_selected.len()),
            ),
            MoaReferenceSelectorMode::RemoveExpert => (
                " Remove MoA Expert ",
                "Type to find a configured expert…",
                "Enter remove  ",
                format!("{} configured", self.moa_reference_selected.len()),
            ),
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area);

        let search_text = if self.moa_reference_selector.query.is_empty() {
            placeholder.to_string()
        } else {
            self.moa_reference_selector.query.clone()
        };
        let search_style = if self.moa_reference_selector.query.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let search = Paragraph::new(Line::from(vec![
            Span::styled("  🧠 ", Style::default().fg(Color::Rgb(130, 210, 255))),
            Span::styled(search_text, search_style),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(130, 210, 255)))
                .title(title),
        );
        frame.render_widget(search, chunks[0]);

        let filtered = &self.moa_reference_selector.filtered;
        let selected = self.moa_reference_selector.selected;
        let max_visible = chunks[1].height as usize;
        let scroll_start = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };

        let items: Vec<ListItem> = filtered
            .iter()
            .skip(scroll_start)
            .take(max_visible)
            .enumerate()
            .map(|(vis_idx, &entry_idx)| {
                let entry = &self.moa_reference_selector.items[entry_idx];
                let is_selected = vis_idx + scroll_start == selected;
                let is_checked = self.moa_reference_selected.contains(&entry.display);
                let bg = if is_selected {
                    Color::Rgb(22, 36, 44)
                } else {
                    Color::Rgb(18, 22, 28)
                };
                let prefix = match self.moa_reference_selector_mode {
                    MoaReferenceSelectorMode::EditRoster => {
                        format!("  [{}] ", if is_checked { "x" } else { " " })
                    }
                    MoaReferenceSelectorMode::AddExpert => "  [+] ".to_string(),
                    MoaReferenceSelectorMode::RemoveExpert => "  [-] ".to_string(),
                };
                let prefix_color = match self.moa_reference_selector_mode {
                    MoaReferenceSelectorMode::EditRoster => {
                        if is_checked {
                            Color::Green
                        } else {
                            Color::DarkGray
                        }
                    }
                    MoaReferenceSelectorMode::AddExpert => Color::Rgb(120, 220, 160),
                    MoaReferenceSelectorMode::RemoveExpert => Color::Rgb(255, 130, 130),
                };
                ListItem::new(Line::from(vec![
                    selector_marker(is_selected, Color::Rgb(130, 210, 255), Some(bg)),
                    Span::styled(prefix, Style::default().bg(bg).fg(prefix_color)),
                    Span::styled(
                        unicode_pad_right(&entry.display, 38),
                        Style::default().bg(bg).fg(if is_selected {
                            Color::Rgb(130, 210, 255)
                        } else {
                            Color::Rgb(220, 232, 240)
                        }),
                    ),
                    Span::styled(
                        unicode_trunc(&entry.detail, 44),
                        Style::default().bg(bg).fg(Color::Rgb(125, 140, 150)),
                    ),
                ]))
            })
            .collect();
        frame.render_widget(
            List::new(items).style(Style::default().bg(Color::Rgb(18, 22, 28))),
            chunks[1],
        );

        let help = Paragraph::new(Line::from(vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Cyan)),
            Span::styled("navigate  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                if self.moa_reference_selector_mode == MoaReferenceSelectorMode::EditRoster {
                    "Space "
                } else {
                    "Enter "
                },
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(action_hint, Style::default().fg(Color::DarkGray)),
            Span::styled(
                if self.moa_reference_selector_mode == MoaReferenceSelectorMode::EditRoster {
                    "Enter "
                } else {
                    ""
                },
                Style::default().fg(Color::Cyan),
            ),
            Span::styled(
                if self.moa_reference_selector_mode == MoaReferenceSelectorMode::EditRoster {
                    "save  "
                } else {
                    ""
                },
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled("Esc ", Style::default().fg(Color::Cyan)),
            Span::styled("cancel  ", Style::default().fg(Color::DarkGray)),
            Span::styled(count_hint, Style::default().fg(Color::Rgb(80, 80, 100))),
        ]));
        frame.render_widget(help, chunks[2]);
    }
}
