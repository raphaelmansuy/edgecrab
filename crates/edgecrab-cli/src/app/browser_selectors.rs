//! Browser-style picker overlays (MCP, profiles, skills, gateway, config).

use super::*;

impl App {
    pub(super) fn render_mcp_selector(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let chunks = Self::browser_overlay_chunks(area);
        let body = Self::browser_body_chunks(chunks[1]);
        self.render_browser_header(
            frame,
            chunks[0],
            &self.mcp_selector.query,
            BrowserChrome {
                title: "MCP Browser",
                placeholder: "Search configured MCP servers and the official catalog.",
                icon: "⛓",
                icon_color: Color::Rgb(110, 220, 210),
                border_color: Color::Rgb(110, 220, 210),
            },
        );

        let max_visible = body[0].height as usize;
        let filtered = &self.mcp_selector.filtered;
        let selected = self.mcp_selector.selected;
        let scroll_start = Self::browser_scroll_start(selected, max_visible);

        let items: Vec<ListItem> = if filtered.is_empty() {
            let empty_text = if self.mcp_selector.query.trim().is_empty() {
                "  No configured servers or catalog entries are available."
            } else {
                "  No MCP entries matched this query."
            };
            vec![ListItem::new(Line::from(Span::styled(
                empty_text.to_string(),
                Style::default().fg(Color::Rgb(120, 120, 135)),
            )))]
        } else {
            filtered
                .iter()
                .skip(scroll_start)
                .take(max_visible)
                .enumerate()
                .map(|(vis_idx, &preset_idx)| {
                    let entry = &self.mcp_selector.items[preset_idx];
                    let is_selected = vis_idx + scroll_start == selected;
                    let bg = if is_selected {
                        Color::Rgb(24, 36, 44)
                    } else {
                        Color::Rgb(18, 24, 26)
                    };
                    let row_style = if is_selected {
                        Style::default()
                            .bg(bg)
                            .fg(Color::Rgb(110, 220, 210))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(210, 220, 220))
                    };
                    let source_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(145, 170, 170))
                    } else {
                        Style::default().fg(Color::Rgb(100, 120, 120))
                    };
                    let action_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(210, 240, 175))
                    } else {
                        Style::default().fg(Color::Rgb(135, 165, 110))
                    };
                    let detail_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(160, 180, 180))
                    } else {
                        Style::default().fg(Color::Rgb(120, 140, 140))
                    };
                    ListItem::new(Line::from(vec![
                        selector_marker(is_selected, Color::Rgb(110, 220, 210), Some(bg)),
                        Span::styled(format!("  {:<12}", entry.source), source_style),
                        Span::styled(format!("{:<9}", entry.action_label), action_style),
                        Span::styled(unicode_trunc(&entry.display_title(), 38), row_style),
                        Span::raw("  "),
                        Span::styled(unicode_trunc(&entry.detail, 38), detail_style),
                    ]))
                })
                .collect()
        };

        frame.render_widget(
            List::new(items).style(Style::default().bg(Color::Rgb(18, 24, 26))),
            body[0],
        );

        let mut detail_lines = Vec::new();
        if let Some(entry) = self.mcp_selector.current() {
            detail_lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", entry.source),
                    Style::default()
                        .fg(Color::Rgb(110, 220, 210))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(entry.title.clone()),
            ]));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(vec![
                Span::styled(
                    "Default action: ",
                    Style::default().fg(Color::Rgb(145, 170, 170)),
                ),
                Span::raw(entry.action_label.clone()),
            ]));
            detail_lines.push(Line::from(""));
            for line in entry.detail_view.lines() {
                detail_lines.push(Line::from(line.to_string()));
            }
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(entry.detail_actions_line()));
        } else if self.mcp_selector.query.trim().is_empty() {
            detail_lines.push(Line::from(Span::styled(
                "MCP Browser",
                Style::default()
                    .fg(Color::Rgb(110, 220, 210))
                    .add_modifier(Modifier::BOLD),
            )));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from("Browse two sources in one place:"));
            detail_lines.push(Line::from(
                "- configured MCP servers from your local config",
            ));
            detail_lines.push(Line::from(
                "- the cached official MCP catalog with installable presets",
            ));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Use fuzzy search to jump by server name, package, tags, transport, env vars, or docs source.",
            ));
        } else {
            detail_lines.push(Line::from("No results for the current query."));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Try broader terms like github, browser, database, time, filesystem, oauth, or http.",
            ));
        }

        if self.detail_fullscreen_active(DetailSurface::McpSelector) {
            self.render_fullscreen_browser_detail(
                frame,
                area,
                FullscreenBrowserChrome {
                    query: &self.mcp_selector.query,
                    header: BrowserChrome {
                        title: "MCP Browser",
                        placeholder: "Search configured MCP servers and the official catalog.",
                        icon: "⛓",
                        icon_color: Color::Rgb(110, 220, 210),
                        border_color: Color::Rgb(110, 220, 210),
                    },
                    detail: ScrollableDetailChrome {
                        title: "Details",
                        border_color: Color::Rgb(110, 220, 210),
                        focused: true,
                        requested_scroll: self.detail_fullscreen_scroll(DetailSurface::McpSelector),
                    },
                    help: Line::from(vec![
                        Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("change item  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("type ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
                        self.paging_key_help_span(Color::Rgb(110, 220, 210)),
                        Span::styled("scroll detail  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Space ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("toggle  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Enter ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("default action  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Z ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("split view  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Esc ", Style::default().fg(Color::Rgb(110, 220, 210))),
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
            DetailSurface::McpSelector,
            Color::Rgb(110, 220, 210),
            detail_lines,
        );

        let mut help_spans = vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("browse  ", Style::default().fg(Color::DarkGray)),
            Span::styled("type ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
        ];
        help_spans.extend(self.focus_pane_help_spans(Color::Rgb(110, 220, 210)));
        help_spans.extend(self.page_or_scroll_help_spans(Color::Rgb(110, 220, 210)));
        help_spans.extend([
            Span::styled("Enter ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("default action  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Space ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("toggle active  ", Style::default().fg(Color::DarkGray)),
            Span::styled("I ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("install  ", Style::default().fg(Color::DarkGray)),
            Span::styled("T ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("test  ", Style::default().fg(Color::DarkGray)),
            Span::styled("C ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("check  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Z ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("detail  ", Style::default().fg(Color::DarkGray)),
            Span::styled("V ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("view  ", Style::default().fg(Color::DarkGray)),
            Span::styled("D ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("remove  ", Style::default().fg(Color::DarkGray)),
            Span::styled("R ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("refresh  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("cancel  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "{} entries · {} pane",
                    filtered.len(),
                    self.simple_split_focus_label(DetailSurface::McpSelector)
                ),
                Style::default().fg(Color::Rgb(100, 120, 120)),
            ),
        ]);
        let help = Paragraph::new(Line::from(help_spans));
        frame.render_widget(help, chunks[2]);
    }

    pub(super) fn render_remote_mcp_selector(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let chunks = Self::browser_overlay_chunks(area);
        let body = Self::browser_body_chunks(chunks[1]);

        let browser = &self.remote_mcp_browser;
        let query = browser.current_query();
        self.render_browser_header(
            frame,
            chunks[0],
            &browser.selector.query,
            BrowserChrome {
                title: if browser.inflight_request_id.is_some() {
                    "Remote MCP · Searching…"
                } else {
                    "Remote MCP"
                },
                placeholder: "Type to search official MCP sources and the official MCP Registry",
                icon: "⛓",
                icon_color: Color::Rgb(90, 190, 220),
                border_color: if browser.inflight_request_id.is_some() {
                    Color::Rgb(110, 220, 210)
                } else {
                    Color::Rgb(90, 190, 220)
                },
            },
        );

        let filtered = &browser.selector.filtered;
        let selected = browser.selector.selected;
        let max_visible = body[0].height as usize;
        let scroll_start = Self::browser_scroll_start(selected, max_visible);

        let items: Vec<ListItem> = if filtered.is_empty() {
            let empty_text = if query.is_empty() {
                "  Start typing to search official MCP sources."
            } else if browser.inflight_request_id.is_some() {
                "  Searching official MCP sources…"
            } else {
                "  No MCP servers matched this query."
            };
            vec![ListItem::new(Line::from(Span::styled(
                empty_text.to_string(),
                Style::default().fg(Color::Rgb(120, 120, 135)),
            )))]
        } else {
            filtered
                .iter()
                .skip(scroll_start)
                .take(max_visible)
                .enumerate()
                .map(|(vis_idx, &entry_idx)| {
                    let entry = &browser.selector.items[entry_idx];
                    let is_selected = vis_idx + scroll_start == selected;
                    let bg = if is_selected {
                        Color::Rgb(24, 36, 44)
                    } else {
                        Color::Rgb(18, 24, 26)
                    };
                    let source_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(145, 170, 170))
                    } else {
                        Style::default().fg(Color::Rgb(100, 120, 120))
                    };
                    let action_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(210, 240, 175))
                    } else {
                        Style::default().fg(Color::Rgb(135, 165, 110))
                    };
                    let main_style = if is_selected {
                        Style::default()
                            .bg(bg)
                            .fg(Color::Rgb(110, 220, 210))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(210, 220, 220))
                    };
                    let desc_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(160, 180, 180))
                    } else {
                        Style::default().fg(Color::Rgb(120, 140, 140))
                    };
                    ListItem::new(Line::from(vec![
                        selector_marker(is_selected, Color::Rgb(110, 220, 210), Some(bg)),
                        Span::styled(format!("  {:<12}", entry.source_label), source_style),
                        Span::styled(format!("{:<8}", entry.action().label()), action_style),
                        Span::styled(unicode_trunc(&entry.id, 40), main_style),
                        Span::raw("  "),
                        Span::styled(unicode_trunc(&entry.description, 34), desc_style),
                    ]))
                })
                .collect()
        };

        frame.render_widget(
            List::new(items).style(Style::default().bg(Color::Rgb(18, 24, 26))),
            body[0],
        );

        let mut detail_lines = Vec::new();
        if let Some(entry) = browser.selector.current() {
            detail_lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", entry.source_label),
                    Style::default()
                        .fg(Color::Rgb(110, 220, 210))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(entry.id.clone()),
            ]));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(entry.description.clone()));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(vec![
                Span::styled("Origin: ", Style::default().fg(Color::Rgb(145, 170, 170))),
                Span::raw(entry.origin.clone()),
            ]));
            detail_lines.push(Line::from(vec![
                Span::styled("Action: ", Style::default().fg(Color::Rgb(145, 170, 170))),
                Span::raw(match entry.action() {
                    RemoteMcpAction::Install => "Default action: install",
                    RemoteMcpAction::View => "Default action: view",
                }),
            ]));
            if let Some(transport) = &entry.transport {
                detail_lines.push(Line::from(vec![
                    Span::styled(
                        "Transport: ",
                        Style::default().fg(Color::Rgb(145, 170, 170)),
                    ),
                    Span::raw(transport.clone()),
                ]));
            }
            if let Some(crate::mcp_catalog::McpInstallPlan::Http {
                required_headers, ..
            }) = &entry.install
                && !required_headers.is_empty()
            {
                detail_lines.push(Line::from(vec![
                    Span::styled("Auth: ", Style::default().fg(Color::Rgb(145, 170, 170))),
                    Span::raw(format!(
                        "manual header setup required: {}",
                        required_headers.join(", ")
                    )),
                ]));
            }
            if !entry.tags.is_empty() {
                detail_lines.push(Line::from(vec![
                    Span::styled("Tags: ", Style::default().fg(Color::Rgb(145, 170, 170))),
                    Span::raw(entry.tags.join(", ")),
                ]));
            }
            if entry.install.is_none() {
                detail_lines.push(Line::from(""));
                detail_lines.push(Line::from(Span::styled(
                    "This entry is searchable but not auto-installable with the current EdgeCrab MCP transport support.",
                    Style::default().fg(Color::Rgb(255, 180, 120)),
                )));
            }
        } else if query.is_empty() {
            detail_lines.push(Line::from(Span::styled(
                "Official Sources",
                Style::default()
                    .fg(Color::Rgb(110, 220, 210))
                    .add_modifier(Modifier::BOLD),
            )));
            detail_lines.push(Line::from("- MCP Reference"));
            detail_lines.push(Line::from("- Official Apps"));
            detail_lines.push(Line::from("- Archived"));
            detail_lines.push(Line::from("- MCP Registry"));
        } else if browser.inflight_request_id.is_some() {
            detail_lines.push(Line::from("Searching official MCP sources…"));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "The selector stays responsive while live registry results are fetched in the background.",
            ));
        } else {
            detail_lines.push(Line::from("No results for the current query."));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Try a broader term like github, database, browser, time, or auth.",
            ));
        }

        if !browser.notices.is_empty() {
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(Span::styled(
                "Source Notes",
                Style::default()
                    .fg(Color::Rgb(255, 191, 0))
                    .add_modifier(Modifier::BOLD),
            )));
            for notice in &browser.notices {
                detail_lines.push(Line::from(format!("- {notice}")));
            }
        }

        if self.detail_fullscreen_active(DetailSurface::RemoteMcpBrowser) {
            self.render_fullscreen_browser_detail(
                frame,
                area,
                FullscreenBrowserChrome {
                    query: &browser.selector.query,
                    header: BrowserChrome {
                        title: if browser.inflight_request_id.is_some() {
                            "Remote MCP · Searching…"
                        } else {
                            "Remote MCP"
                        },
                        placeholder:
                            "Type to search official MCP sources and the official MCP Registry",
                        icon: "⛓",
                        icon_color: Color::Rgb(90, 190, 220),
                        border_color: if browser.inflight_request_id.is_some() {
                            Color::Rgb(110, 220, 210)
                        } else {
                            Color::Rgb(90, 190, 220)
                        },
                    },
                    detail: ScrollableDetailChrome {
                        title: "Details",
                        border_color: Color::Rgb(90, 190, 220),
                        focused: true,
                        requested_scroll: self
                            .detail_fullscreen_scroll(DetailSurface::RemoteMcpBrowser),
                    },
                    help: Line::from(vec![
                        Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("change item  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("type ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
                        self.paging_key_help_span(Color::Rgb(110, 220, 210)),
                        Span::styled("scroll detail  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Enter ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("default action  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Z ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("split view  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Esc ", Style::default().fg(Color::Rgb(110, 220, 210))),
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
            DetailSurface::RemoteMcpBrowser,
            Color::Rgb(90, 190, 220),
            detail_lines,
        );

        let status_text = if browser.inflight_request_id.is_some() {
            "searching"
        } else if !query.is_empty() && filtered.is_empty() {
            "no matches"
        } else {
            "matches"
        };
        let mut help_spans = vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("browse  ", Style::default().fg(Color::DarkGray)),
            Span::styled("type ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
        ];
        help_spans.extend(self.focus_pane_help_spans(Color::Rgb(110, 220, 210)));
        help_spans.extend(self.page_or_scroll_help_spans(Color::Rgb(110, 220, 210)));
        help_spans.extend([
            Span::styled("Enter ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("default action  ", Style::default().fg(Color::DarkGray)),
            Span::styled("I ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("install  ", Style::default().fg(Color::DarkGray)),
            Span::styled("V ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("view  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Z ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("detail  ", Style::default().fg(Color::DarkGray)),
            Span::styled("R ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("refresh  ", Style::default().fg(Color::DarkGray)),
            Span::styled("L ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("local browser  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("cancel  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "{} {} · {} pane",
                    filtered.len(),
                    status_text,
                    self.simple_split_focus_label(DetailSurface::RemoteMcpBrowser)
                ),
                Style::default().fg(Color::Rgb(100, 120, 120)),
            ),
        ]);
        let help = Paragraph::new(Line::from(help_spans));
        frame.render_widget(help, chunks[2]);
    }

    pub(super) fn profile_browser_title(&self) -> &'static str {
        match self.profile_detail_mode {
            ProfileDetailMode::Summary => "Browse Profiles",
            ProfileDetailMode::Config => "Browse Profiles · Config",
            ProfileDetailMode::Soul => "Browse Profiles · SOUL",
            ProfileDetailMode::Memory => "Browse Profiles · Memory",
            ProfileDetailMode::Tools => "Browse Profiles · Tools",
            ProfileDetailMode::Help => "Browse Profiles · Help",
        }
    }

    pub(super) fn profile_help_text(&self) -> String {
        [
            "Profiles",
            "",
            "Enter  switch to the selected profile",
            "V      summary view",
            "C      config.yaml view",
            "S      SOUL.md view",
            "M      memory files view",
            "T      tool policy view",
            "A      alias editor",
            "E      export editor",
            "D      delete confirmation",
            "N      create editor",
            "I      import editor",
            "O      rename editor",
            "Tab / Right  next detail tab",
            "Shift-Tab / Left  previous detail tab",
            "Home / End  jump to first or last match",
            "PgUp / PgDn  jump through matches in split view",
            "Z      toggle split/full detail",
            "Esc    close overlay",
            "",
            "Slash command entry points:",
            "/profile",
            "/profile show <name>",
            "/profile config <name>",
            "/profile soul <name>",
            "/profile memory <name>",
            "/profile tools <name>",
            "/profile create ...",
            "/profile rename ...",
            "/profile delete ...",
            "/profile alias ...",
            "/profile export ...",
            "/profile import ...",
        ]
        .join("\n")
    }

    pub(super) fn render_profile_detail_text(&self, entry: Option<&ProfileEntry>) -> String {
        let Some(entry) = entry else {
            return match self.profile_detail_mode {
                ProfileDetailMode::Help => self.profile_help_text(),
                _ => "No profiles matched this query.".into(),
            };
        };

        let manager = ProfileManager::new();
        match self.profile_detail_mode {
            ProfileDetailMode::Summary => {
                format!("{}\n\n{}", entry.detail_view, entry.detail_actions_line())
            }
            ProfileDetailMode::Config => manager
                .render_config(&entry.name)
                .unwrap_or_else(|err| format!("profile config: {err}")),
            ProfileDetailMode::Soul => manager
                .render_soul(&entry.name)
                .unwrap_or_else(|err| format!("profile soul: {err}")),
            ProfileDetailMode::Memory => manager
                .render_memory(&entry.name)
                .unwrap_or_else(|err| format!("profile memory: {err}")),
            ProfileDetailMode::Tools => manager
                .render_tools_report(&entry.name)
                .unwrap_or_else(|err| format!("profile tools: {err}")),
            ProfileDetailMode::Help => {
                format!(
                    "{}\n\nCurrent selection: {}",
                    self.profile_help_text(),
                    entry.name
                )
            }
        }
    }

    /// Render the full-screen skill browser overlay.
    ///
    /// UX mirrors `render_model_selector` — same search-box + list + help-line
    /// layout — so users get a consistent experience between `/model` and `/skills`.
    pub(super) fn render_profile_selector(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let chunks = Self::browser_overlay_chunks(area);
        let body = Self::browser_body_chunks(chunks[1]);
        self.render_browser_header(
            frame,
            chunks[0],
            &self.profile_selector.query,
            BrowserChrome {
                title: self.profile_browser_title(),
                placeholder: "Search profiles by name, model, path, or status.",
                icon: "👤",
                icon_color: Color::Rgb(120, 205, 255),
                border_color: Color::Rgb(120, 205, 255),
            },
        );

        let max_visible = body[0].height as usize;
        let filtered = &self.profile_selector.filtered;
        let selected = self.profile_selector.selected;
        let scroll_start = Self::browser_scroll_start(selected, max_visible);

        let items: Vec<ListItem> = filtered
            .iter()
            .skip(scroll_start)
            .take(max_visible)
            .enumerate()
            .map(|(vis_idx, &profile_idx)| {
                let entry = &self.profile_selector.items[profile_idx];
                let is_selected = vis_idx + scroll_start == selected;
                let bg = if is_selected {
                    Color::Rgb(18, 36, 48)
                } else {
                    Color::Rgb(20, 20, 28)
                };
                ListItem::new(Line::from(vec![
                    selector_marker(is_selected, Color::Rgb(120, 205, 255), Some(bg)),
                    Span::styled(
                        format!("  {:<7}", entry.status_label()),
                        if is_selected {
                            Style::default().bg(bg).fg(Color::Rgb(165, 225, 255))
                        } else {
                            Style::default().fg(Color::Rgb(90, 130, 155))
                        },
                    ),
                    Span::styled(
                        format!("{:<8}", entry.kind_label()),
                        if is_selected {
                            Style::default().bg(bg).fg(Color::Rgb(110, 170, 210))
                        } else {
                            Style::default().fg(Color::Rgb(70, 100, 120))
                        },
                    ),
                    Span::styled(
                        unicode_trunc(&entry.display_title(), 24),
                        if is_selected {
                            Style::default()
                                .bg(bg)
                                .fg(Color::Rgb(180, 235, 255))
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::Rgb(135, 200, 225))
                        },
                    ),
                    Span::raw("  "),
                    Span::styled(
                        unicode_trunc(&entry.detail, 48),
                        if is_selected {
                            Style::default().bg(bg).fg(Color::Rgb(135, 180, 205))
                        } else {
                            Style::default().fg(Color::Rgb(85, 110, 130))
                        },
                    ),
                ]))
            })
            .collect();
        frame.render_widget(
            List::new(items).style(Style::default().bg(Color::Rgb(20, 20, 28))),
            body[0],
        );

        let mut detail_lines = Vec::new();
        if let Some(entry) = self.profile_selector.current() {
            detail_lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", entry.status_label().to_uppercase()),
                    Style::default()
                        .fg(Color::Rgb(120, 205, 255))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(entry.display_title()),
                Span::styled(
                    format!("  [{}]", self.profile_detail_mode.title()),
                    Style::default().fg(Color::Rgb(90, 150, 180)),
                ),
            ]));
        } else {
            detail_lines.push(Line::from(Span::styled(
                format!("Detail Mode: {}", self.profile_detail_mode.title()),
                Style::default()
                    .fg(Color::Rgb(120, 205, 255))
                    .add_modifier(Modifier::BOLD),
            )));
        }
        detail_lines.push(Line::from(""));
        for line in self
            .render_profile_detail_text(self.profile_selector.current())
            .lines()
        {
            detail_lines.push(Line::from(line.to_string()));
        }

        if self.detail_fullscreen_active(DetailSurface::ProfileSelector) {
            self.render_fullscreen_browser_detail(
                frame,
                area,
                FullscreenBrowserChrome {
                    query: &self.profile_selector.query,
                    header: BrowserChrome {
                        title: self.profile_browser_title(),
                        placeholder: "Search profiles by name, model, path, or status.",
                        icon: "👤",
                        icon_color: Color::Rgb(120, 205, 255),
                        border_color: Color::Rgb(120, 205, 255),
                    },
                    detail: ScrollableDetailChrome {
                        title: "Details",
                        border_color: Color::Rgb(120, 205, 255),
                        focused: true,
                        requested_scroll: self
                            .detail_fullscreen_scroll(DetailSurface::ProfileSelector),
                    },
                    help: Line::from(vec![
                        Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(120, 205, 255))),
                        Span::styled("change item  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("type ", Style::default().fg(Color::Rgb(120, 205, 255))),
                        Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
                        self.paging_key_help_span(Color::Rgb(120, 205, 255)),
                        Span::styled("scroll detail  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Enter ", Style::default().fg(Color::Rgb(120, 205, 255))),
                        Span::styled("switch profile  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("C ", Style::default().fg(Color::Rgb(120, 205, 255))),
                        Span::styled("config  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("S ", Style::default().fg(Color::Rgb(120, 205, 255))),
                        Span::styled("SOUL  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("M ", Style::default().fg(Color::Rgb(120, 205, 255))),
                        Span::styled("memory  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("T ", Style::default().fg(Color::Rgb(120, 205, 255))),
                        Span::styled("tools  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("V ", Style::default().fg(Color::Rgb(120, 205, 255))),
                        Span::styled("view summary  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("A/E ", Style::default().fg(Color::Rgb(120, 205, 255))),
                        Span::styled("alias/export  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("D/N/I/O ", Style::default().fg(Color::Rgb(120, 205, 255))),
                        Span::styled(
                            "delete/new/import/rename  ",
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled("Tab/←→ ", Style::default().fg(Color::Rgb(120, 205, 255))),
                        Span::styled("cycle views  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Home/End ", Style::default().fg(Color::Rgb(120, 205, 255))),
                        Span::styled("jump  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Z ", Style::default().fg(Color::Rgb(120, 205, 255))),
                        Span::styled("split view  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Esc ", Style::default().fg(Color::Rgb(120, 205, 255))),
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
            DetailSurface::ProfileSelector,
            Color::Rgb(120, 205, 255),
            detail_lines,
        );

        let help = Paragraph::new(Line::from(vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(120, 205, 255))),
            Span::styled("browse  ", Style::default().fg(Color::DarkGray)),
            Span::styled("type ", Style::default().fg(Color::Rgb(120, 205, 255))),
            Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
            self.paging_key_help_span(Color::Rgb(120, 205, 255)),
            Span::styled("jump list  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter ", Style::default().fg(Color::Rgb(120, 205, 255))),
            Span::styled("switch  ", Style::default().fg(Color::DarkGray)),
            Span::styled("C/S/M/T ", Style::default().fg(Color::Rgb(120, 205, 255))),
            Span::styled("inspect  ", Style::default().fg(Color::DarkGray)),
            Span::styled("V ", Style::default().fg(Color::Rgb(120, 205, 255))),
            Span::styled("summary  ", Style::default().fg(Color::DarkGray)),
            Span::styled("A/E ", Style::default().fg(Color::Rgb(120, 205, 255))),
            Span::styled("alias/export  ", Style::default().fg(Color::DarkGray)),
            Span::styled("D/N/I/O ", Style::default().fg(Color::Rgb(120, 205, 255))),
            Span::styled("manage  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Tab/←→ ", Style::default().fg(Color::Rgb(120, 205, 255))),
            Span::styled("views  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Home/End ", Style::default().fg(Color::Rgb(120, 205, 255))),
            Span::styled("jump  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Z ", Style::default().fg(Color::Rgb(120, 205, 255))),
            Span::styled("detail  ", Style::default().fg(Color::DarkGray)),
            Span::styled("R ", Style::default().fg(Color::Rgb(120, 205, 255))),
            Span::styled("refresh  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Rgb(120, 205, 255))),
            Span::styled("close  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} profile(s)", filtered.len()),
                Style::default().fg(Color::Rgb(80, 100, 120)),
            ),
        ]));
        frame.render_widget(help, chunks[2]);
    }

    pub(super) fn render_skill_selector(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let chunks = Self::browser_overlay_chunks(area);
        let body = Self::browser_body_chunks(chunks[1]);
        self.render_browser_header(
            frame,
            chunks[0],
            &self.skill_selector.query,
            BrowserChrome {
                title: "Browse Skills",
                placeholder: "Search local skills by name, category, path, preview, or support files.",
                icon: "📚",
                icon_color: Color::Rgb(255, 191, 0),
                border_color: Color::Rgb(255, 191, 0),
            },
        );

        let max_visible = body[0].height as usize;
        let filtered = &self.skill_selector.filtered;
        let selected = self.skill_selector.selected;
        let scroll_start = Self::browser_scroll_start(selected, max_visible);

        let items: Vec<ListItem> = filtered
            .iter()
            .skip(scroll_start)
            .take(max_visible)
            .enumerate()
            .map(|(vis_idx, &skill_idx)| {
                let entry = &self.skill_selector.items[skill_idx];
                let is_selected = vis_idx + scroll_start == selected;

                let bg = if is_selected {
                    Color::Rgb(40, 35, 15)
                } else {
                    Color::Rgb(20, 20, 28)
                };
                let state_style = if is_selected {
                    Style::default().bg(bg).fg(Color::Rgb(150, 140, 90))
                } else {
                    Style::default().fg(Color::Rgb(90, 80, 45))
                };
                let name_style = if is_selected {
                    Style::default()
                        .bg(bg)
                        .fg(Color::Rgb(255, 215, 0))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(220, 200, 100))
                };
                let desc_style = if is_selected {
                    Style::default().bg(bg).fg(Color::Rgb(160, 150, 90))
                } else {
                    Style::default().fg(Color::Rgb(100, 95, 55))
                };

                ListItem::new(Line::from(vec![
                    selector_marker(is_selected, Color::Rgb(255, 191, 0), Some(bg)),
                    Span::styled(
                        format!("  {:<7}", if entry.active { "active" } else { "ready" }),
                        state_style,
                    ),
                    Span::styled(
                        format!("{:<7}", entry.kind_label()),
                        if is_selected {
                            Style::default().bg(bg).fg(Color::Rgb(120, 110, 60))
                        } else {
                            Style::default().fg(Color::Rgb(80, 75, 40))
                        },
                    ),
                    Span::styled(unicode_trunc(&entry.display_title(), 34), name_style),
                    Span::raw("  "),
                    Span::styled(unicode_trunc(&entry.list_detail(), 42), desc_style),
                ]))
            })
            .collect();

        let skill_count = filtered.len();
        let list = List::new(items).style(Style::default().bg(Color::Rgb(20, 20, 28)));
        frame.render_widget(list, body[0]);

        let mut detail_lines = Vec::new();
        if let Some(entry) = self.skill_selector.current() {
            detail_lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", if entry.active { "ACTIVE" } else { "READY" }),
                    Style::default()
                        .fg(Color::Rgb(255, 191, 0))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(entry.display_title()),
            ]));
            detail_lines.push(Line::from(""));
            for line in entry.detail_view.lines() {
                detail_lines.push(Line::from(line.to_string()));
            }
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(entry.detail_actions_line()));
        } else if self.skill_selector.query.trim().is_empty() {
            detail_lines.push(Line::from(Span::styled(
                "Local Skills",
                Style::default()
                    .fg(Color::Rgb(255, 191, 0))
                    .add_modifier(Modifier::BOLD),
            )));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Browse installed skills with fuzzy search across names, categories, previews, and supporting files.",
            ));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Space toggles whether a skill is injected into your next prompt.",
            ));
            detail_lines.push(Line::from(
                "Enter inserts `/skill-name` into the composer if you want the explicit slash flow instead.",
            ));
        } else {
            detail_lines.push(Line::from("No local skills matched this query."));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Try a broader term, a category name, or press R to search remote sources.",
            ));
        }
        if self.detail_fullscreen_active(DetailSurface::SkillSelector) {
            self.render_fullscreen_browser_detail(
                frame,
                area,
                FullscreenBrowserChrome {
                    query: &self.skill_selector.query,
                    header: BrowserChrome {
                        title: "Browse Skills",
                        placeholder:
                            "Search local skills by name, category, path, preview, or support files.",
                        icon: "📚",
                        icon_color: Color::Rgb(255, 191, 0),
                        border_color: Color::Rgb(255, 191, 0),
                    },
                    detail: ScrollableDetailChrome {
                        title: "Details",
                        border_color: Color::Rgb(255, 191, 0),
                        focused: true,
                        requested_scroll: self
                            .detail_fullscreen_scroll(DetailSurface::SkillSelector),
                    },
                    help: Line::from(vec![
                        Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(255, 191, 0))),
                        Span::styled("change item  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("type ", Style::default().fg(Color::Rgb(255, 191, 0))),
                        Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
                        self.paging_key_help_span(Color::Rgb(255, 191, 0)),
                        Span::styled("scroll detail  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Space ", Style::default().fg(Color::Rgb(255, 191, 0))),
                        Span::styled("toggle active  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Enter ", Style::default().fg(Color::Rgb(255, 191, 0))),
                        Span::styled("insert /skill  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Z ", Style::default().fg(Color::Rgb(255, 191, 0))),
                        Span::styled("split view  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Esc ", Style::default().fg(Color::Rgb(255, 191, 0))),
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
            DetailSurface::SkillSelector,
            Color::Rgb(255, 191, 0),
            detail_lines,
        );

        let mut help_spans = vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(255, 191, 0))),
            Span::styled("browse  ", Style::default().fg(Color::DarkGray)),
            Span::styled("type ", Style::default().fg(Color::Rgb(255, 191, 0))),
            Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
        ];
        help_spans.extend(self.focus_pane_help_spans(Color::Rgb(255, 191, 0)));
        help_spans.extend(self.page_or_scroll_help_spans(Color::Rgb(255, 191, 0)));
        help_spans.extend([
            Span::styled("Space ", Style::default().fg(Color::Rgb(255, 191, 0))),
            Span::styled("toggle active  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter ", Style::default().fg(Color::Rgb(255, 191, 0))),
            Span::styled("insert /skill-name  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Z ", Style::default().fg(Color::Rgb(255, 191, 0))),
            Span::styled("detail  ", Style::default().fg(Color::DarkGray)),
            Span::styled("R ", Style::default().fg(Color::Rgb(255, 191, 0))),
            Span::styled("remote search  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Rgb(255, 191, 0))),
            Span::styled("cancel  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "{skill_count} skill(s) · {} pane",
                    self.simple_split_focus_label(DetailSurface::SkillSelector)
                ),
                Style::default().fg(Color::Rgb(80, 75, 40)),
            ),
        ]);
        let help = Paragraph::new(Line::from(help_spans));
        frame.render_widget(help, chunks[2]);
    }

    pub(super) fn render_remote_skill_selector(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let chunks = Self::browser_overlay_chunks(area);
        let body = Self::browser_body_chunks(chunks[1]);

        let browser = &self.remote_skill_browser;
        let query = browser.current_query();
        self.render_browser_header(
            frame,
            chunks[0],
            &browser.selector.query,
            BrowserChrome {
                title: if browser.inflight_request_id.is_some() {
                    "Remote Skills · Searching…"
                } else {
                    "Remote Skills"
                },
                placeholder: "Type to search remote skills from EdgeCrab, Hermes, OpenAI, Anthropic, skills.sh, or a well-known URL",
                icon: "🌐",
                icon_color: Color::Rgb(110, 220, 210),
                border_color: if browser.inflight_request_id.is_some() {
                    Color::Rgb(110, 220, 210)
                } else {
                    Color::Rgb(255, 191, 0)
                },
            },
        );

        let filtered = &browser.selector.filtered;
        let selected = browser.selector.selected;
        let max_visible = body[0].height as usize;
        let scroll_start = Self::browser_scroll_start(selected, max_visible);

        let items: Vec<ListItem> = if filtered.is_empty() {
            let empty_text = if query.is_empty() {
                "  Start typing to search remote skills."
            } else if browser.inflight_request_id.is_some() {
                "  Searching remote sources…"
            } else {
                "  No remote skills matched this query."
            };
            vec![ListItem::new(Line::from(Span::styled(
                empty_text.to_string(),
                Style::default().fg(Color::Rgb(120, 120, 135)),
            )))]
        } else {
            filtered
                .iter()
                .skip(scroll_start)
                .take(max_visible)
                .enumerate()
                .map(|(vis_idx, &entry_idx)| {
                    let entry = &browser.selector.items[entry_idx];
                    let is_selected = vis_idx + scroll_start == selected;
                    let bg = if is_selected {
                        Color::Rgb(24, 40, 44)
                    } else {
                        Color::Rgb(18, 24, 26)
                    };
                    let source_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(145, 170, 170))
                    } else {
                        Style::default().fg(Color::Rgb(100, 120, 120))
                    };
                    let action_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(210, 240, 175))
                    } else {
                        Style::default().fg(Color::Rgb(135, 165, 110))
                    };
                    let main_style = if is_selected {
                        Style::default()
                            .bg(bg)
                            .fg(Color::Rgb(110, 220, 210))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(210, 220, 220))
                    };
                    let desc_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(160, 180, 180))
                    } else {
                        Style::default().fg(Color::Rgb(120, 140, 140))
                    };
                    let mut spans = vec![
                        selector_marker(is_selected, Color::Rgb(110, 220, 210), Some(bg)),
                        Span::styled(format!("  {:<11}", entry.source_label), source_style),
                        Span::styled(format!("{:<8}", entry.action.label()), action_style),
                    ];
                    if is_selected && let Some(guard) = &self.remote_skill_guard.preview {
                        if self.remote_skill_guard.for_identifier.as_deref()
                            == Some(entry.identifier.as_str())
                        {
                            let (chip, chip_color) =
                                super::remote_skill_guard::guard_verdict_chip(&guard.verdict);
                            spans.push(Span::styled(
                                format!("{chip} "),
                                Style::default().bg(bg).fg(chip_color),
                            ));
                        } else if self.remote_skill_guard.inflight.as_deref()
                            == Some(entry.identifier.as_str())
                        {
                            spans.push(Span::styled(
                                "… ",
                                Style::default().bg(bg).fg(Color::Rgb(110, 220, 210)),
                            ));
                        }
                    }
                    spans.push(Span::styled(
                        unicode_trunc(&entry.identifier, 40),
                        main_style,
                    ));
                    spans.push(Span::raw("  "));
                    spans.push(Span::styled(
                        unicode_trunc(&entry.description, 32),
                        desc_style,
                    ));
                    ListItem::new(Line::from(spans))
                })
                .collect()
        };

        frame.render_widget(
            List::new(items).style(Style::default().bg(Color::Rgb(18, 24, 26))),
            body[0],
        );

        let mut detail_lines = Vec::new();
        if let Some(entry) = browser.selector.current() {
            let status_line = match entry.action {
                RemoteSkillAction::Install => "Default action: install".to_string(),
                RemoteSkillAction::Update => format!(
                    "Default action: update ({})",
                    entry.installed_name.as_deref().unwrap_or(&entry.name)
                ),
                RemoteSkillAction::Replace => {
                    "Default action: replace existing local skill".to_string()
                }
            };
            detail_lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", entry.source_label),
                    Style::default()
                        .fg(Color::Rgb(110, 220, 210))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("[{}]", entry.trust_level),
                    Style::default().fg(Color::Rgb(160, 180, 180)),
                ),
            ]));
            detail_lines.push(Line::from(entry.identifier.clone()));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(entry.description.clone()));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(vec![
                Span::styled("Origin: ", Style::default().fg(Color::Rgb(145, 170, 170))),
                Span::raw(entry.origin.clone()),
            ]));
            detail_lines.push(Line::from(vec![
                Span::styled("Action: ", Style::default().fg(Color::Rgb(145, 170, 170))),
                Span::raw(status_line),
            ]));
            if !entry.tags.is_empty() {
                detail_lines.push(Line::from(vec![
                    Span::styled("Tags: ", Style::default().fg(Color::Rgb(145, 170, 170))),
                    Span::raw(entry.tags.join(", ")),
                ]));
            }
            if entry.action == RemoteSkillAction::Replace {
                detail_lines.push(Line::from(""));
                detail_lines.push(Line::from(Span::styled(
                    "Warning: this source would replace an existing local skill with the same name.",
                    Style::default().fg(Color::Rgb(255, 180, 120)),
                )));
            }
            self.append_remote_skill_guard_detail(&mut detail_lines);
        } else if query.is_empty() {
            detail_lines.push(Line::from(Span::styled(
                "Curated Sources",
                Style::default()
                    .fg(Color::Rgb(110, 220, 210))
                    .add_modifier(Modifier::BOLD),
            )));
            for source in edgecrab_tools::tools::skills_hub::curated_source_summaries() {
                detail_lines.push(Line::from(format!(
                    "- {} [{}]",
                    source.label, source.trust_level
                )));
            }
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Paste an https:// URL to search a .well-known skills endpoint too.",
            ));
        } else if browser.inflight_request_id.is_some() {
            detail_lines.push(Line::from("Searching remote sources…"));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "You can keep typing while results refresh. Slow or failing sources are reported here without blocking the UI.",
            ));
        } else {
            detail_lines.push(Line::from("No results for the current query."));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Try a broader term, a source name like 'edgecrab', or a full https:// URL for well-known skill discovery.",
            ));
        }

        if !browser.notices.is_empty() {
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(Span::styled(
                "Source Notes",
                Style::default()
                    .fg(Color::Rgb(255, 191, 0))
                    .add_modifier(Modifier::BOLD),
            )));
            for notice in &browser.notices {
                detail_lines.push(Line::from(format!("- {notice}")));
            }
        }

        if let Some(action) = &browser.action_in_flight {
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(Span::styled(
                format!("Running: {action}"),
                Style::default().fg(Color::Rgb(210, 240, 175)),
            )));
        }

        if self.detail_fullscreen_active(DetailSurface::RemoteSkillBrowser) {
            self.render_fullscreen_browser_detail(
                frame,
                area,
                FullscreenBrowserChrome {
                    query: &browser.selector.query,
                    header: BrowserChrome {
                        title: if browser.inflight_request_id.is_some() {
                            "Remote Skills · Searching…"
                        } else {
                            "Remote Skills"
                        },
                        placeholder: "Type to search remote skills from EdgeCrab, Hermes, OpenAI, Anthropic, skills.sh, or a well-known URL",
                        icon: "🌐",
                        icon_color: Color::Rgb(110, 220, 210),
                        border_color: if browser.inflight_request_id.is_some() {
                            Color::Rgb(110, 220, 210)
                        } else {
                            Color::Rgb(255, 191, 0)
                        },
                    },
                    detail: ScrollableDetailChrome {
                        title: "Details",
                        border_color: Color::Rgb(255, 191, 0),
                        focused: true,
                        requested_scroll: self
                            .detail_fullscreen_scroll(DetailSurface::RemoteSkillBrowser),
                    },
                    help: Line::from(vec![
                        Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("change item  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("type ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
                        self.paging_key_help_span(Color::Rgb(110, 220, 210)),
                        Span::styled("scroll detail  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Enter ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("default action  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Z ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("split view  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Esc ", Style::default().fg(Color::Rgb(110, 220, 210))),
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
            DetailSurface::RemoteSkillBrowser,
            Color::Rgb(255, 191, 0),
            detail_lines,
        );

        let mut help_spans = vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("browse  ", Style::default().fg(Color::DarkGray)),
            Span::styled("type ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
        ];
        help_spans.extend(self.focus_pane_help_spans(Color::Rgb(110, 220, 210)));
        help_spans.extend(self.page_or_scroll_help_spans(Color::Rgb(110, 220, 210)));
        help_spans.extend([
            Span::styled("Enter ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("default action  ", Style::default().fg(Color::DarkGray)),
            Span::styled("I ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("install/update  ", Style::default().fg(Color::DarkGray)),
            Span::styled("U ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("force update  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Z ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("detail  ", Style::default().fg(Color::DarkGray)),
            Span::styled("R ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("refresh  ", Style::default().fg(Color::DarkGray)),
            Span::styled("S ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("guard scan  ", Style::default().fg(Color::DarkGray)),
            Span::styled("L ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("local browser  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("cancel  ", Style::default().fg(Color::DarkGray)),
        ]);
        let status_text = if browser.inflight_request_id.is_some() {
            "searching"
        } else if !query.is_empty() && filtered.is_empty() {
            "no matches"
        } else {
            "matches"
        };
        help_spans.push(Span::styled(
            format!(
                "{} {} · {} pane",
                filtered.len(),
                status_text,
                self.simple_split_focus_label(DetailSurface::RemoteSkillBrowser)
            ),
            Style::default().fg(Color::Rgb(100, 120, 120)),
        ));
        let help = Paragraph::new(Line::from(help_spans));
        frame.render_widget(help, chunks[2]);
    }

    pub(super) fn render_remote_plugin_selector(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let chunks = Self::browser_overlay_chunks(area);
        let body = Self::browser_body_chunks(chunks[1]);

        let browser = &self.remote_plugin_browser;
        let query = browser.current_query();
        let title = if let Some(source) = browser.source_filter.as_deref() {
            if browser.inflight_request_id.is_some() {
                format!("Remote Plugins · {source} · Searching…")
            } else {
                format!("Remote Plugins · {source}")
            }
        } else if browser.inflight_request_id.is_some() {
            "Remote Plugins · Searching…".to_string()
        } else {
            "Remote Plugins".to_string()
        };
        self.render_browser_header(
            frame,
            chunks[0],
            &browser.selector.query,
            BrowserChrome {
                title: &title,
                placeholder:
                    "Type to search official and configured plugin registries, or use /plugins search --source <name> <query>",
                icon: "🔌",
                icon_color: Color::Rgb(210, 190, 110),
                border_color: if browser.inflight_request_id.is_some() {
                    Color::Rgb(210, 190, 110)
                } else {
                    Color::Rgb(255, 191, 0)
                },
            },
        );

        let filtered = &browser.selector.filtered;
        let selected = browser.selector.selected;
        let max_visible = body[0].height as usize;
        let scroll_start = Self::browser_scroll_start(selected, max_visible);

        let items: Vec<ListItem> = if filtered.is_empty() {
            let empty_text = if query.is_empty() {
                "  Start typing to search remote plugins."
            } else if browser.inflight_request_id.is_some() {
                "  Searching remote plugin registries…"
            } else {
                "  No remote plugins matched this query."
            };
            vec![ListItem::new(Line::from(Span::styled(
                empty_text.to_string(),
                Style::default().fg(Color::Rgb(120, 120, 135)),
            )))]
        } else {
            filtered
                .iter()
                .skip(scroll_start)
                .take(max_visible)
                .enumerate()
                .map(|(vis_idx, &entry_idx)| {
                    let entry = &browser.selector.items[entry_idx];
                    let is_selected = vis_idx + scroll_start == selected;
                    let bg = if is_selected {
                        Color::Rgb(42, 34, 18)
                    } else {
                        Color::Rgb(24, 22, 16)
                    };
                    let source_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(205, 190, 140))
                    } else {
                        Style::default().fg(Color::Rgb(155, 140, 105))
                    };
                    let action_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(210, 240, 175))
                    } else {
                        Style::default().fg(Color::Rgb(135, 165, 110))
                    };
                    let kind_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(150, 165, 205))
                    } else {
                        Style::default().fg(Color::Rgb(110, 125, 160))
                    };
                    let main_style = if is_selected {
                        Style::default()
                            .bg(bg)
                            .fg(Color::Rgb(255, 236, 175))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(220, 220, 210))
                    };
                    let desc_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(185, 180, 165))
                    } else {
                        Style::default().fg(Color::Rgb(140, 135, 125))
                    };
                    ListItem::new(Line::from(vec![
                        selector_marker(is_selected, Color::Rgb(255, 210, 110), Some(bg)),
                        Span::styled(format!("  {:<11}", entry.source_label), source_style),
                        Span::styled(format!("{:<8}", entry.action.label()), action_style),
                        Span::styled(unicode_pad_right(&entry.kind, 8), kind_style),
                        Span::raw("  "),
                        Span::styled(unicode_trunc(&entry.identifier, 40), main_style),
                        Span::raw("  "),
                        Span::styled(unicode_trunc(&entry.description, 28), desc_style),
                    ]))
                })
                .collect()
        };

        frame.render_widget(
            List::new(items).style(Style::default().bg(Color::Rgb(24, 22, 16))),
            body[0],
        );

        let mut detail_lines = Vec::new();
        if let Some(entry) = browser.selector.current() {
            let status_line = match entry.action {
                RemotePluginAction::Install => "Default action: install".to_string(),
                RemotePluginAction::Update => format!(
                    "Default action: update ({})",
                    entry.installed_name.as_deref().unwrap_or(&entry.name)
                ),
                RemotePluginAction::Replace => {
                    "Default action: replace existing local plugin".to_string()
                }
            };
            detail_lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", entry.source_label),
                    Style::default()
                        .fg(Color::Rgb(255, 236, 175))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("[{}]", entry.trust_level),
                    Style::default().fg(Color::Rgb(185, 180, 165)),
                ),
            ]));
            detail_lines.push(Line::from(entry.identifier.clone()));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(entry.description.clone()));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(vec![
                Span::styled("Kind: ", Style::default().fg(Color::Rgb(205, 190, 140))),
                Span::raw(entry.kind.clone()),
            ]));
            detail_lines.push(Line::from(vec![
                Span::styled("Origin: ", Style::default().fg(Color::Rgb(205, 190, 140))),
                Span::raw(entry.origin.clone()),
            ]));
            detail_lines.push(Line::from(vec![
                Span::styled("Action: ", Style::default().fg(Color::Rgb(205, 190, 140))),
                Span::raw(status_line),
            ]));
            if !entry.requires_env.is_empty() {
                detail_lines.push(Line::from(vec![
                    Span::styled(
                        "Requires env: ",
                        Style::default().fg(Color::Rgb(205, 190, 140)),
                    ),
                    Span::raw(entry.requires_env.join(", ")),
                ]));
            }
            if !entry.tags.is_empty() {
                detail_lines.push(Line::from(vec![
                    Span::styled("Tags: ", Style::default().fg(Color::Rgb(205, 190, 140))),
                    Span::raw(entry.tags.join(", ")),
                ]));
            }
            if entry.action == RemotePluginAction::Replace {
                detail_lines.push(Line::from(""));
                detail_lines.push(Line::from(Span::styled(
                    "Warning: this source would replace an existing local plugin with the same name.",
                    Style::default().fg(Color::Rgb(255, 180, 120)),
                )));
            }
        } else if query.is_empty() {
            detail_lines.push(Line::from(Span::styled(
                "Registry Sources",
                Style::default()
                    .fg(Color::Rgb(255, 236, 175))
                    .add_modifier(Modifier::BOLD),
            )));
            let config = self.load_runtime_config();
            for source in edgecrab_plugins::hub_source_summaries(&config.plugins) {
                detail_lines.push(Line::from(format!(
                    "- {} [{}] — {}",
                    source.label,
                    format!("{:?}", source.trust_level).to_ascii_lowercase(),
                    source.description
                )));
            }
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Use /plugins search --source hermes <query> to constrain the browser to one registry family.",
            ));
        } else if browser.inflight_request_id.is_some() {
            detail_lines.push(Line::from("Searching remote plugin registries…"));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "You can keep typing while results refresh. Source failures are surfaced as notes instead of blocking the browser.",
            ));
        } else {
            detail_lines.push(Line::from("No results for the current query."));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Try a broader term, a source name like 'edgecrab' or 'hermes', or use /plugins search --source <name> <query>.",
            ));
        }

        if !browser.notices.is_empty() {
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(Span::styled(
                "Source Notes",
                Style::default()
                    .fg(Color::Rgb(255, 191, 0))
                    .add_modifier(Modifier::BOLD),
            )));
            for notice in &browser.notices {
                detail_lines.push(Line::from(format!("- {notice}")));
            }
        }

        if let Some(action) = &browser.action_in_flight {
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(Span::styled(
                format!("Running: {action}"),
                Style::default().fg(Color::Rgb(210, 240, 175)),
            )));
        }

        if self.detail_fullscreen_active(DetailSurface::RemotePluginBrowser) {
            self.render_fullscreen_browser_detail(
                frame,
                area,
                FullscreenBrowserChrome {
                    query: &browser.selector.query,
                    header: BrowserChrome {
                        title: &title,
                        placeholder:
                            "Type to search official and configured plugin registries, or use /plugins search --source <name> <query>",
                        icon: "🔌",
                        icon_color: Color::Rgb(255, 210, 110),
                        border_color: if browser.inflight_request_id.is_some() {
                            Color::Rgb(255, 210, 110)
                        } else {
                            Color::Rgb(255, 191, 0)
                        },
                    },
                    detail: ScrollableDetailChrome {
                        title: "Details",
                        border_color: Color::Rgb(255, 191, 0),
                        focused: true,
                        requested_scroll: self
                            .detail_fullscreen_scroll(DetailSurface::RemotePluginBrowser),
                    },
                    help: Line::from(vec![
                        Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(255, 210, 110))),
                        Span::styled("change item  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("type ", Style::default().fg(Color::Rgb(255, 210, 110))),
                        Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
                        self.paging_key_help_span(Color::Rgb(255, 210, 110)),
                        Span::styled("scroll detail  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Enter ", Style::default().fg(Color::Rgb(255, 210, 110))),
                        Span::styled("default action  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Z ", Style::default().fg(Color::Rgb(255, 210, 110))),
                        Span::styled("split view  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Esc ", Style::default().fg(Color::Rgb(255, 210, 110))),
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
            DetailSurface::RemotePluginBrowser,
            Color::Rgb(255, 191, 0),
            detail_lines,
        );

        let mut help_spans = vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(255, 210, 110))),
            Span::styled("browse  ", Style::default().fg(Color::DarkGray)),
            Span::styled("type ", Style::default().fg(Color::Rgb(255, 210, 110))),
            Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
        ];
        help_spans.extend(self.focus_pane_help_spans(Color::Rgb(255, 210, 110)));
        help_spans.extend(self.page_or_scroll_help_spans(Color::Rgb(255, 210, 110)));
        help_spans.extend([
            Span::styled("Enter ", Style::default().fg(Color::Rgb(255, 210, 110))),
            Span::styled("default action  ", Style::default().fg(Color::DarkGray)),
            Span::styled("I ", Style::default().fg(Color::Rgb(255, 210, 110))),
            Span::styled("install/update  ", Style::default().fg(Color::DarkGray)),
            Span::styled("U ", Style::default().fg(Color::Rgb(255, 210, 110))),
            Span::styled("update  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Z ", Style::default().fg(Color::Rgb(255, 210, 110))),
            Span::styled("detail  ", Style::default().fg(Color::DarkGray)),
            Span::styled("R ", Style::default().fg(Color::Rgb(255, 210, 110))),
            Span::styled("refresh  ", Style::default().fg(Color::DarkGray)),
            Span::styled("L ", Style::default().fg(Color::Rgb(255, 210, 110))),
            Span::styled("local browser  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Rgb(255, 210, 110))),
            Span::styled("cancel  ", Style::default().fg(Color::DarkGray)),
        ]);
        let status_text = if browser.inflight_request_id.is_some() {
            "searching"
        } else if !query.is_empty() && filtered.is_empty() {
            "no matches"
        } else {
            "matches"
        };
        help_spans.push(Span::styled(
            format!(
                "{} {} · {} pane",
                filtered.len(),
                status_text,
                self.simple_split_focus_label(DetailSurface::RemotePluginBrowser)
            ),
            Style::default().fg(Color::Rgb(155, 140, 105)),
        ));
        let help = Paragraph::new(Line::from(help_spans));
        frame.render_widget(help, chunks[2]);
    }

    pub(super) fn render_tool_manager(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area);
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(chunks[1]);

        let tabs = [
            ToolManagerScope::All,
            ToolManagerScope::Toolsets,
            ToolManagerScope::Tools,
        ]
        .into_iter()
        .map(|scope| {
            let style = if scope == self.tool_manager_scope {
                Style::default()
                    .fg(Color::Rgb(255, 238, 170))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(120, 132, 146))
            };
            Span::styled(format!("[{}] ", scope.title()), style)
        })
        .collect::<Vec<_>>();

        let search_text = if self.tool_manager.query.is_empty() {
            "Search tools, toolsets, descriptions, or tags".to_string()
        } else {
            self.tool_manager.query.clone()
        };
        let search_style = if self.tool_manager.query.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let mut search_spans = vec![
            Span::styled("  🧰 ", Style::default().fg(Color::Rgb(140, 220, 210))),
            Span::styled(search_text, search_style),
            Span::raw("   "),
        ];
        search_spans.extend(tabs);
        let search = Paragraph::new(Line::from(search_spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(110, 220, 210)))
                .title(" Tool Manager "),
        );
        frame.render_widget(search, chunks[0]);

        let filtered = &self.tool_manager.filtered;
        let selected = self.tool_manager.selected;
        let max_visible = body[0].height as usize;
        let scroll_start = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };

        let items: Vec<ListItem> = if filtered.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "  No tools matched the current filter.",
                Style::default().fg(Color::Rgb(120, 120, 135)),
            )))]
        } else {
            filtered
                .iter()
                .skip(scroll_start)
                .take(max_visible)
                .enumerate()
                .map(|(vis_idx, &entry_idx)| {
                    let entry = &self.tool_manager.items[entry_idx];
                    let is_selected = vis_idx + scroll_start == selected;
                    let bg = if is_selected {
                        Color::Rgb(20, 42, 46)
                    } else {
                        Color::Rgb(16, 22, 28)
                    };
                    let check_style = if is_selected {
                        Style::default()
                            .bg(bg)
                            .fg(Color::Rgb(210, 240, 175))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(145, 185, 120))
                    };
                    let kind_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(150, 180, 188))
                    } else {
                        Style::default().fg(Color::Rgb(95, 115, 125))
                    };
                    let name_style = if is_selected {
                        Style::default()
                            .bg(bg)
                            .fg(Color::Rgb(110, 220, 210))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(210, 220, 220))
                    };
                    let detail_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(170, 190, 190))
                    } else {
                        Style::default().fg(Color::Rgb(118, 138, 138))
                    };

                    ListItem::new(Line::from(vec![
                        selector_marker(is_selected, Color::Rgb(110, 220, 210), Some(bg)),
                        Span::styled(format!("  {}", entry.check_state.glyph()), check_style),
                        Span::raw("  "),
                        Span::styled(unicode_pad_right(&entry.tag, 8), kind_style),
                        Span::styled(
                            unicode_pad_right(&format!("{} {}", entry.emoji, entry.name), 30),
                            name_style,
                        ),
                        Span::raw("  "),
                        Span::styled(unicode_trunc(&entry.detail, 36), detail_style),
                    ]))
                })
                .collect()
        };

        frame.render_widget(
            List::new(items).style(Style::default().bg(Color::Rgb(16, 22, 28))),
            body[0],
        );

        let mut detail_lines = Vec::new();
        if let Some(entry) = self.tool_manager.current() {
            let policy_text = match entry.policy_source {
                ToolPolicySource::Default => "inherits default policy",
                ToolPolicySource::ExplicitEnable => "forced on by explicit override",
                ToolPolicySource::ExplicitDisable => "forced off by explicit override",
            };
            let runtime_text = if entry.exposed {
                "visible to the model right now"
            } else if !entry.startup_available {
                "hidden because the tool is unavailable at startup"
            } else if !entry.check_allowed {
                "hidden by runtime gating in this session"
            } else {
                "hidden by current policy"
            };

            detail_lines.push(Line::from(Span::styled(
                format!("{} {}", entry.emoji, entry.name),
                Style::default()
                    .fg(Color::Rgb(110, 220, 210))
                    .add_modifier(Modifier::BOLD),
            )));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(format!("Kind: {}", entry.tag)));
            detail_lines.push(Line::from(format!("Policy: {policy_text}")));
            detail_lines.push(Line::from(format!("Runtime: {runtime_text}")));

            match entry.kind {
                ToolManagerItemKind::Toolset => {
                    detail_lines.push(Line::from(format!(
                        "Coverage: {}/{} selected · {}/{} exposed",
                        entry.selected_tools,
                        entry.total_tools,
                        entry.exposed_tools,
                        entry.total_tools
                    )));
                    if !entry.description.is_empty() {
                        detail_lines.push(Line::from(""));
                        detail_lines.push(Line::from("Included tools:"));
                        for tool in entry.description.split(", ") {
                            detail_lines.push(Line::from(format!("  • {tool}")));
                        }
                    }
                }
                ToolManagerItemKind::Tool => {
                    detail_lines.push(Line::from(format!("Toolset: {}", entry.toolset)));
                    if entry.dynamic {
                        detail_lines.push(Line::from("Origin: dynamic runtime tool"));
                    }
                    if !entry.aliases.is_empty() {
                        detail_lines
                            .push(Line::from(format!("Aliases: {}", entry.aliases.join(", "))));
                    }
                    detail_lines.push(Line::from(""));
                    for line in entry.description.lines() {
                        detail_lines.push(Line::from(line.to_string()));
                    }
                }
            }
        }

        if self.detail_fullscreen_active(DetailSurface::ToolManager) {
            self.render_fullscreen_browser_detail(
                frame,
                area,
                FullscreenBrowserChrome {
                    query: &self.tool_manager.query,
                    header: BrowserChrome {
                        title: "Tool Manager",
                        placeholder: "Search tools, toolsets, descriptions, or tags",
                        icon: "🧰",
                        icon_color: Color::Rgb(140, 220, 210),
                        border_color: Color::Rgb(110, 220, 210),
                    },
                    detail: ScrollableDetailChrome {
                        title: "Details",
                        border_color: Color::Rgb(110, 220, 210),
                        focused: true,
                        requested_scroll: self.detail_fullscreen_scroll(DetailSurface::ToolManager),
                    },
                    help: Line::from(vec![
                        Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("change item  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("type ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
                        self.paging_key_help_span(Color::Rgb(110, 220, 210)),
                        Span::styled("scroll detail  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Space ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("toggle  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("←→ ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("scope  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Z ", Style::default().fg(Color::Rgb(110, 220, 210))),
                        Span::styled("split view  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Esc ", Style::default().fg(Color::Rgb(110, 220, 210))),
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
            DetailSurface::ToolManager,
            Color::Rgb(110, 220, 210),
            detail_lines,
        );

        let footer_note = self.tool_manager_status_note.as_deref().unwrap_or(
            "Space toggles. Tab focuses panes. Left/Right changes scope. R restores defaults.",
        );
        let footer_summary = format!(
            "{} · {} scope · {} pane",
            footer_note,
            self.tool_manager_scope.title(),
            self.simple_split_focus_label(DetailSurface::ToolManager)
        );
        let mut help_spans = vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("browse  ", Style::default().fg(Color::DarkGray)),
        ];
        help_spans.extend(self.focus_pane_help_spans(Color::Rgb(110, 220, 210)));
        help_spans.extend(self.page_or_scroll_help_spans(Color::Rgb(110, 220, 210)));
        help_spans.extend([
            Span::styled("Space ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("toggle  ", Style::default().fg(Color::DarkGray)),
            Span::styled("←→ ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("scope  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Z ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("detail  ", Style::default().fg(Color::DarkGray)),
            Span::styled("R ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("reset  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Rgb(110, 220, 210))),
            Span::styled("close  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                edgecrab_core::safe_truncate(&footer_summary, 62),
                Style::default().fg(Color::Rgb(95, 115, 125)),
            ),
        ]);
        let help = Paragraph::new(Line::from(help_spans));
        frame.render_widget(help, chunks[2]);
    }

    pub(super) fn render_plugin_toggle(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let chunks = Self::browser_overlay_chunks(area);
        let body = Self::browser_body_chunks(chunks[1]);
        let title = format!("Browse Plugins [{}]", self.plugin_toggle_scope.title());
        self.render_browser_header(
            frame,
            chunks[0],
            &self.plugin_toggle.query,
            BrowserChrome {
                title: &title,
                placeholder:
                    "Search installed plugins by name, tool, CLI command, status, source, or trust.",
                icon: "🔌",
                icon_color: Color::Rgb(210, 190, 110),
                border_color: Color::Rgb(210, 190, 110),
            },
        );

        let filtered = &self.plugin_toggle.filtered;
        let selected = self.plugin_toggle.selected;
        let max_visible = body[0].height as usize;
        let scroll_start = Self::browser_scroll_start(selected, max_visible);

        let items: Vec<ListItem> = if filtered.is_empty() {
            let empty_text = if self.plugin_toggle.items.is_empty() {
                "  No local plugins installed yet."
            } else if self.plugin_toggle.query.trim().is_empty() {
                "  No plugins available in this scope."
            } else {
                "  No plugins matched the current filter."
            };
            vec![ListItem::new(Line::from(Span::styled(
                empty_text.to_string(),
                Style::default().fg(Color::Rgb(120, 120, 135)),
            )))]
        } else {
            filtered
                .iter()
                .skip(scroll_start)
                .take(max_visible)
                .enumerate()
                .map(|(vis_idx, &entry_idx)| {
                    let entry = &self.plugin_toggle.items[entry_idx];
                    let is_selected = vis_idx + scroll_start == selected;
                    let bg = if is_selected {
                        Color::Rgb(46, 38, 18)
                    } else {
                        Color::Rgb(22, 22, 18)
                    };
                    let state_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(210, 240, 175))
                    } else {
                        Style::default().fg(Color::Rgb(150, 180, 120))
                    };
                    let pending_style = if entry.has_pending_change() {
                        if is_selected {
                            Style::default().bg(bg).fg(Color::Rgb(255, 195, 120))
                        } else {
                            Style::default().fg(Color::Rgb(210, 150, 90))
                        }
                    } else if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(120, 110, 90))
                    } else {
                        Style::default().fg(Color::Rgb(90, 85, 70))
                    };
                    let kind_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(150, 165, 205))
                    } else {
                        Style::default().fg(Color::Rgb(110, 125, 160))
                    };
                    let name_style = if is_selected {
                        Style::default()
                            .bg(bg)
                            .fg(Color::Rgb(255, 236, 175))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(220, 220, 210))
                    };
                    let trust_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(205, 190, 140))
                    } else {
                        Style::default().fg(Color::Rgb(155, 140, 105))
                    };
                    let detail_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(160, 160, 145))
                    } else {
                        Style::default().fg(Color::Rgb(120, 120, 110))
                    };
                    ListItem::new(Line::from(vec![
                        selector_marker(is_selected, Color::Rgb(210, 190, 110), Some(bg)),
                        Span::styled(format!("  {:<8}", entry.runtime_status), state_style),
                        Span::raw("  "),
                        Span::styled(
                            format!(
                                "{:<7}",
                                if entry.has_pending_change() {
                                    "staged"
                                } else {
                                    ""
                                }
                            ),
                            pending_style,
                        ),
                        Span::raw("  "),
                        Span::styled(unicode_pad_right(&entry.kind, 8), kind_style),
                        Span::styled(unicode_trunc(&entry.display_name, 24), name_style),
                        Span::raw("  "),
                        Span::styled(unicode_pad_right(&entry.trust_level, 10), trust_style),
                        Span::raw("  "),
                        Span::styled(
                            unicode_trunc(
                                &format!(
                                    "{} {} tool{}",
                                    entry.check_state.glyph(),
                                    entry.tool_count,
                                    if entry.tool_count == 1 { "" } else { "s" }
                                ),
                                18,
                            ),
                            detail_style,
                        ),
                    ]))
                })
                .collect()
        };
        frame.render_widget(
            List::new(items).style(Style::default().bg(Color::Rgb(22, 22, 18))),
            body[0],
        );

        let mut detail_lines = Vec::new();
        if let Some(entry) = self.plugin_toggle.current() {
            detail_lines.push(Line::from(Span::styled(
                entry.display_name.clone(),
                Style::default()
                    .fg(Color::Rgb(255, 236, 175))
                    .add_modifier(Modifier::BOLD),
            )));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(vec![
                Span::styled(
                    "Desired state: ",
                    Style::default().fg(Color::Rgb(205, 190, 140)),
                ),
                Span::raw(entry.state_label()),
            ]));
            detail_lines.push(Line::from(vec![
                Span::styled(
                    "Runtime status: ",
                    Style::default().fg(Color::Rgb(205, 190, 140)),
                ),
                Span::raw(entry.runtime_status.clone()),
            ]));
            detail_lines.push(Line::from(vec![
                Span::styled("Scope: ", Style::default().fg(Color::Rgb(205, 190, 140))),
                Span::raw(self.plugin_toggle_scope.title()),
            ]));
            detail_lines.push(Line::from(vec![
                Span::styled("Kind: ", Style::default().fg(Color::Rgb(205, 190, 140))),
                Span::raw(entry.kind.clone()),
            ]));
            detail_lines.push(Line::from(vec![
                Span::styled("Trust: ", Style::default().fg(Color::Rgb(205, 190, 140))),
                Span::raw(entry.trust_level.clone()),
            ]));
            detail_lines.push(Line::from(vec![
                Span::styled("Version: ", Style::default().fg(Color::Rgb(205, 190, 140))),
                Span::raw(entry.version.clone()),
            ]));
            detail_lines.push(Line::from(vec![
                Span::styled("Source: ", Style::default().fg(Color::Rgb(205, 190, 140))),
                Span::raw(entry.source.clone()),
            ]));
            if let Some(install_source) = entry.install_source.as_deref() {
                detail_lines.push(Line::from(vec![
                    Span::styled(
                        "Install source: ",
                        Style::default().fg(Color::Rgb(205, 190, 140)),
                    ),
                    Span::raw(install_source.to_string()),
                ]));
            }
            detail_lines.push(Line::from(vec![
                Span::styled("Tools: ", Style::default().fg(Color::Rgb(205, 190, 140))),
                Span::raw(if entry.tools.is_empty() {
                    "none".into()
                } else {
                    entry.tools.join(", ")
                }),
            ]));
            if !entry.cli_commands.is_empty() {
                detail_lines.push(Line::from(vec![
                    Span::styled("CLI: ", Style::default().fg(Color::Rgb(205, 190, 140))),
                    Span::raw(entry.cli_commands.join(", ")),
                ]));
            }
            if let Some(compatibility) = entry.compatibility.as_deref() {
                detail_lines.push(Line::from(vec![
                    Span::styled(
                        "Compatibility: ",
                        Style::default().fg(Color::Rgb(205, 190, 140)),
                    ),
                    Span::raw(compatibility.to_string()),
                ]));
            }
            if !entry.related_skills.is_empty() {
                detail_lines.push(Line::from(vec![
                    Span::styled(
                        "Related skills: ",
                        Style::default().fg(Color::Rgb(205, 190, 140)),
                    ),
                    Span::raw(entry.related_skills.join(", ")),
                ]));
            }
            detail_lines.push(Line::from(vec![
                Span::styled(
                    "Estimated prompt cost: ",
                    Style::default().fg(Color::Rgb(205, 190, 140)),
                ),
                Span::raw(format!("~{} tokens", entry.estimated_tokens)),
            ]));
            if !entry.missing_env.is_empty() {
                detail_lines.push(Line::from(vec![
                    Span::styled(
                        "Missing env: ",
                        Style::default().fg(Color::Rgb(255, 180, 120)),
                    ),
                    Span::raw(entry.missing_env.join(", ")),
                ]));
            }
            if entry.has_pending_change() {
                detail_lines.push(Line::from(""));
                detail_lines.push(Line::from(Span::styled(
                    "This plugin has staged changes that are not saved yet.",
                    Style::default().fg(Color::Rgb(255, 195, 120)),
                )));
            }
            detail_lines.push(Line::from(""));
            for line in entry.description.lines() {
                detail_lines.push(Line::from(line.to_string()));
            }
        } else if self.plugin_toggle.items.is_empty() {
            detail_lines.push(Line::from(Span::styled(
                "No Local Plugins",
                Style::default()
                    .fg(Color::Rgb(255, 236, 175))
                    .add_modifier(Modifier::BOLD),
            )));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Install a local Hermes plugin with `edgecrab plugins install ./path`, or press R to search remote plugin registries.",
            ));
        } else if self.plugin_toggle.query.trim().is_empty() {
            detail_lines.push(Line::from(Span::styled(
                "Installed Plugins",
                Style::default()
                    .fg(Color::Rgb(255, 236, 175))
                    .add_modifier(Modifier::BOLD),
            )));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Browse installed plugins by status, tool surface, trust, and source.",
            ));
            detail_lines.push(Line::from(
                "Space stages enable or disable changes. Enter saves them for the active scope.",
            ));
            detail_lines.push(Line::from(
                "Tab switches between the list and detail panes. Use Left and Right to change the active policy scope.",
            ));
        } else {
            detail_lines.push(Line::from("No plugins matched the current query."));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "Try a broader term, a tool name, a trust level, or press R to search remote sources.",
            ));
        }

        if self.detail_fullscreen_active(DetailSurface::PluginToggle) {
            self.render_fullscreen_browser_detail(
                frame,
                area,
                FullscreenBrowserChrome {
                    query: &self.plugin_toggle.query,
                    header: BrowserChrome {
                        title: &title,
                        placeholder:
                            "Search installed plugins by name, tool, CLI command, status, source, or trust.",
                        icon: "🔌",
                        icon_color: Color::Rgb(210, 190, 110),
                        border_color: Color::Rgb(210, 190, 110),
                    },
                    detail: ScrollableDetailChrome {
                        title: "Details",
                        border_color: Color::Rgb(210, 190, 110),
                        focused: true,
                        requested_scroll: self
                            .detail_fullscreen_scroll(DetailSurface::PluginToggle),
                    },
                    help: Line::from(vec![
                        Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(210, 190, 110))),
                        Span::styled("change item  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("type ", Style::default().fg(Color::Rgb(210, 190, 110))),
                        Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
                        self.paging_key_help_span(Color::Rgb(210, 190, 110)),
                        Span::styled("scroll detail  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Space ", Style::default().fg(Color::Rgb(210, 190, 110))),
                        Span::styled("stage change  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Enter ", Style::default().fg(Color::Rgb(210, 190, 110))),
                        Span::styled("save  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("←→ ", Style::default().fg(Color::Rgb(210, 190, 110))),
                        Span::styled("scope  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("R ", Style::default().fg(Color::Rgb(210, 190, 110))),
                        Span::styled("remote search  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Z ", Style::default().fg(Color::Rgb(210, 190, 110))),
                        Span::styled("split view  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Esc ", Style::default().fg(Color::Rgb(210, 190, 110))),
                        Span::styled("close", Style::default().fg(Color::DarkGray)),
                    ]),
                },
                detail_lines,
            );
            return;
        }
        self.render_standard_split_detail(
            frame,
            body[1],
            DetailSurface::PluginToggle,
            Color::Rgb(210, 190, 110),
            detail_lines,
        );

        let footer_note = self
            .plugin_toggle_status_note
            .clone()
            .unwrap_or_else(|| plugin_toggle_status_line(&self.plugin_toggle.items));
        let footer_summary = format!(
            "{} · {} scope · {} pane",
            footer_note,
            self.plugin_toggle_scope.title(),
            self.simple_split_focus_label(DetailSurface::PluginToggle)
        );
        let mut help_spans = vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(210, 190, 110))),
            Span::styled("browse  ", Style::default().fg(Color::DarkGray)),
            Span::styled("type ", Style::default().fg(Color::Rgb(210, 190, 110))),
            Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
        ];
        help_spans.extend(self.focus_pane_help_spans(Color::Rgb(210, 190, 110)));
        help_spans.extend(self.page_or_scroll_help_spans(Color::Rgb(210, 190, 110)));
        help_spans.extend([
            Span::styled("Space ", Style::default().fg(Color::Rgb(210, 190, 110))),
            Span::styled("stage change  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter ", Style::default().fg(Color::Rgb(210, 190, 110))),
            Span::styled("save  ", Style::default().fg(Color::DarkGray)),
            Span::styled("←→ ", Style::default().fg(Color::Rgb(210, 190, 110))),
            Span::styled("scope  ", Style::default().fg(Color::DarkGray)),
            Span::styled("R ", Style::default().fg(Color::Rgb(210, 190, 110))),
            Span::styled("remote search  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Z ", Style::default().fg(Color::Rgb(210, 190, 110))),
            Span::styled("detail  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Rgb(210, 190, 110))),
            Span::styled("close  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                edgecrab_core::safe_truncate(&footer_summary, 62),
                Style::default().fg(Color::Rgb(120, 120, 110)),
            ),
        ]);
        let help = Paragraph::new(Line::from(help_spans));
        frame.render_widget(help, chunks[2]);
    }

    pub(super) fn build_config_detail_lines(
        &self,
        entry: Option<&ConfigEntry>,
    ) -> Vec<Line<'static>> {
        let mut detail_lines = Vec::new();
        if let Some(entry) = entry {
            detail_lines.push(Line::from(Span::styled(
                entry.title.clone(),
                Style::default()
                    .fg(Color::Rgb(130, 210, 255))
                    .add_modifier(Modifier::BOLD),
            )));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(entry.detail.clone()));
            detail_lines.push(Line::from(""));
            let detail_body = match entry.action {
                ConfigAction::ShowSummary => self.render_config_summary(),
                ConfigAction::ShowPaths => self.render_config_paths(),
                ConfigAction::ShowWorktree => self.render_worktree_report(),
                ConfigAction::OpenTools => {
                    "Press Enter to open the live tool manager. Use Tab to move between the list and detail panes, Left and Right to change scope, Space to toggle toolsets or individual tools, and R to restore defaults.".into()
                }
                ConfigAction::OpenGatewayBrowser => {
                    "Press Enter to open the gateway control browser. From there you can toggle platforms, edit bind settings, change allowlists, update home channels, and restart the gateway runtime without leaving the TUI.".into()
                }
                ConfigAction::ShowGatewayHomes => {
                    let config = self.load_runtime_config();
                    self.render_gateway_home_channel_summary(&config)
                }
                ConfigAction::ShowVoice => format!(
                    "Voice readback is {}.\nRun `/voice status` for recorder, provider, and push-to-talk details.",
                    if self.voice_mode_enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                ),
                ConfigAction::ShowUpdateStatus => {
                    "Runs the local git-based update check and prints ahead/behind guidance.".into()
                }
                ConfigAction::OpenModel => "Press Enter to open the model selector overlay.".into(),
                ConfigAction::OpenCheapModel => {
                    "Press Enter to open the cheap-model selector. Selecting a model enables smart routing for obviously simple turns.".into()
                }
                ConfigAction::ToggleMoa => {
                    "Press Enter to enable or disable the moa tool while keeping the saved aggregator and expert roster.".into()
                }
                ConfigAction::OpenVisionModel => {
                    "Press Enter to open the dedicated vision-model selector.".into()
                }
                ConfigAction::OpenImageModel => {
                    "Press Enter to open the image-model selector.".into()
                }
                ConfigAction::OpenMoaAggregator => {
                    "Press Enter to pick the default aggregator model used by the moa tool.".into()
                }
                ConfigAction::OpenMoaReferences => {
                    "Press Enter to edit the full default MoA expert roster. Use Space to toggle experts and Enter to save.".into()
                }
                ConfigAction::AddMoaExpert => {
                    "Press Enter to choose one model to add to the saved MoA expert roster.".into()
                }
                ConfigAction::RemoveMoaExpert => {
                    "Press Enter to choose one configured expert to remove from the saved MoA roster.".into()
                }
                ConfigAction::ToggleStreaming => {
                    "Press Enter to toggle live token streaming.".into()
                }
                ConfigAction::ToggleReasoning => {
                    "Press Enter to toggle visible reasoning output.".into()
                }
                ConfigAction::ToggleStatusBar => {
                    "Press Enter to show or hide the status bar.".into()
                }
                ConfigAction::OpenLogs => {
                    "Press Enter to open the local log browser. Inside it you can inspect file tails, drill into individual entries, and save the default log level for future launches.".into()
                }
                ConfigAction::OpenSkins => {
                    "Press Enter to browse installed skins and apply one live.".into()
                }
            };
            for line in detail_body.lines() {
                detail_lines.push(Line::from(line.to_string()));
            }
        }
        detail_lines
    }

    pub(super) fn render_config_selector(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area);
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(chunks[1]);

        let search_text = if self.config_selector.query.is_empty() {
            "Type to filter settings and controls…".to_string()
        } else {
            self.config_selector.query.clone()
        };
        let search_style = if self.config_selector.query.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let search = Paragraph::new(Line::from(vec![
            Span::styled("  ⚙ ", Style::default().fg(Color::Rgb(130, 210, 255))),
            Span::styled(search_text, search_style),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(130, 210, 255)))
                .title(" Config Center  [/config] "),
        );
        frame.render_widget(search, chunks[0]);

        let filtered = &self.config_selector.filtered;
        let selected = self.config_selector.selected;
        let max_visible = body[0].height as usize;
        let scroll_start = if selected >= max_visible {
            selected - max_visible + 1
        } else {
            0
        };

        let items: Vec<ListItem> = if filtered.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "  No settings matched the current filter.",
                Style::default().fg(Color::Rgb(120, 120, 135)),
            )))]
        } else {
            filtered
                .iter()
                .skip(scroll_start)
                .take(max_visible)
                .enumerate()
                .map(|(vis_idx, &entry_idx)| {
                    let entry = &self.config_selector.items[entry_idx];
                    let is_selected = vis_idx + scroll_start == selected;
                    let bg = if is_selected {
                        Color::Rgb(22, 36, 44)
                    } else {
                        Color::Rgb(18, 22, 28)
                    };
                    let tag_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(150, 180, 200))
                    } else {
                        Style::default().fg(Color::Rgb(105, 125, 140))
                    };
                    let title_style = if is_selected {
                        Style::default()
                            .bg(bg)
                            .fg(Color::Rgb(130, 210, 255))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Rgb(220, 232, 240))
                    };
                    let detail_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(172, 190, 204))
                    } else {
                        Style::default().fg(Color::Rgb(125, 140, 150))
                    };
                    ListItem::new(Line::from(vec![
                        selector_marker(is_selected, Color::Rgb(130, 210, 255), Some(bg)),
                        Span::styled(format!("  {:<9}", entry.tag), tag_style),
                        Span::styled(unicode_pad_right(&entry.title, 28), title_style),
                        Span::raw("  "),
                        Span::styled(unicode_trunc(&entry.detail, 54), detail_style),
                    ]))
                })
                .collect()
        };
        frame.render_widget(
            List::new(items).style(Style::default().bg(Color::Rgb(18, 22, 28))),
            body[0],
        );

        let detail_lines = self.build_config_detail_lines(self.config_selector.current());
        if self.detail_fullscreen_active(DetailSurface::ConfigSelector) {
            self.render_fullscreen_browser_detail(
                frame,
                area,
                FullscreenBrowserChrome {
                    query: &self.config_selector.query,
                    header: BrowserChrome {
                        title: "Config Center",
                        placeholder: "Type to filter settings and controls…",
                        icon: "⚙",
                        icon_color: Color::Rgb(130, 210, 255),
                        border_color: Color::Rgb(130, 210, 255),
                    },
                    detail: ScrollableDetailChrome {
                        title: "Details",
                        border_color: Color::Rgb(130, 210, 255),
                        focused: true,
                        requested_scroll: self
                            .detail_fullscreen_scroll(DetailSurface::ConfigSelector),
                    },
                    help: Line::from(vec![
                        Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(130, 210, 255))),
                        Span::styled("change item  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("type ", Style::default().fg(Color::Rgb(130, 210, 255))),
                        Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
                        self.paging_key_help_span(Color::Rgb(130, 210, 255)),
                        Span::styled("scroll detail  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Enter ", Style::default().fg(Color::Rgb(130, 210, 255))),
                        Span::styled("run action  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Z ", Style::default().fg(Color::Rgb(130, 210, 255))),
                        Span::styled("split view  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Esc ", Style::default().fg(Color::Rgb(130, 210, 255))),
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
            DetailSurface::ConfigSelector,
            Color::Rgb(130, 210, 255),
            detail_lines,
        );

        let mut help_spans = vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(130, 210, 255))),
            Span::styled("browse  ", Style::default().fg(Color::DarkGray)),
        ];
        help_spans.extend(self.focus_pane_help_spans(Color::Rgb(130, 210, 255)));
        help_spans.extend(self.page_or_scroll_help_spans(Color::Rgb(130, 210, 255)));
        help_spans.extend([
            Span::styled("Enter ", Style::default().fg(Color::Rgb(130, 210, 255))),
            Span::styled("run action  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Z ", Style::default().fg(Color::Rgb(130, 210, 255))),
            Span::styled("detail  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Rgb(130, 210, 255))),
            Span::styled("cancel  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "{} item(s) · {} pane",
                    filtered.len(),
                    self.simple_split_focus_label(DetailSurface::ConfigSelector)
                ),
                Style::default().fg(Color::Rgb(100, 120, 130)),
            ),
        ]);
        let help = Paragraph::new(Line::from(help_spans));
        frame.render_widget(help, chunks[2]);
    }

    pub(super) fn render_gateway_browser(&self, frame: &mut Frame, area: Rect) {
        frame.render_widget(Clear, area);

        let chunks = Self::browser_overlay_chunks(area);
        let body = Self::browser_body_chunks(chunks[1]);
        self.render_browser_header(
            frame,
            chunks[0],
            &self.gateway_browser.query,
            BrowserChrome {
                title: "Gateway Control",
                placeholder:
                    "Search platforms by name, state, delivery mode, or missing setup fields.",
                icon: "⛵",
                icon_color: Color::Rgb(120, 220, 200),
                border_color: Color::Rgb(120, 220, 200),
            },
        );

        let filtered = &self.gateway_browser.filtered;
        let selected = self.gateway_browser.selected;
        let max_visible = Self::browser_list_visible_rows(body[0], false);
        let scroll_start = Self::browser_scroll_start(selected, max_visible);

        let items: Vec<ListItem> = if filtered.is_empty() {
            vec![ListItem::new(Line::from(Span::styled(
                "  No gateway platforms matched this filter.".to_string(),
                Style::default().fg(Color::Rgb(120, 120, 135)),
            )))]
        } else {
            filtered
                .iter()
                .skip(scroll_start)
                .take(max_visible)
                .enumerate()
                .map(|(vis_idx, &entry_idx)| {
                    let entry = &self.gateway_browser.items[entry_idx];
                    let is_selected = vis_idx + scroll_start == selected;
                    let bg = if is_selected {
                        Color::Rgb(18, 42, 42)
                    } else {
                        Color::Rgb(18, 22, 28)
                    };
                    let accent = match entry.diagnostic.state {
                        crate::gateway_catalog::PlatformState::Ready => Color::Rgb(120, 220, 160),
                        crate::gateway_catalog::PlatformState::Available => {
                            Color::Rgb(170, 210, 120)
                        }
                        crate::gateway_catalog::PlatformState::Incomplete => {
                            Color::Rgb(255, 180, 110)
                        }
                        crate::gateway_catalog::PlatformState::NotConfigured => {
                            Color::Rgb(120, 140, 150)
                        }
                    };
                    let tag_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(155, 185, 175))
                    } else {
                        Style::default().fg(Color::Rgb(105, 125, 118))
                    };
                    let title_style = if is_selected {
                        Style::default()
                            .bg(bg)
                            .fg(accent)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(accent)
                    };
                    let detail_style = if is_selected {
                        Style::default().bg(bg).fg(Color::Rgb(175, 195, 188))
                    } else {
                        Style::default().fg(Color::Rgb(125, 140, 150))
                    };

                    ListItem::new(Line::from(vec![
                        selector_marker(is_selected, accent, Some(bg)),
                        Span::styled(
                            format!("  {:<16}", entry.diagnostic.state.label()),
                            tag_style,
                        ),
                        Span::styled(unicode_pad_right(entry.diagnostic.name, 12), title_style),
                        Span::raw("  "),
                        Span::styled(unicode_trunc(&entry.summary, 64), detail_style),
                    ]))
                })
                .collect()
        };
        frame.render_widget(
            List::new(items).style(Style::default().bg(Color::Rgb(18, 22, 28))),
            body[0],
        );

        let mut detail_lines = Vec::new();
        if let Some(entry) = self.gateway_browser.current() {
            for line in entry.detail_view.lines() {
                detail_lines.push(Line::from(line.to_string()));
            }
        } else {
            detail_lines.push(Line::from(Span::styled(
                "Gateway Control",
                Style::default()
                    .fg(Color::Rgb(120, 220, 200))
                    .add_modifier(Modifier::BOLD),
            )));
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(
                "This browser turns `/platforms` into an operator cockpit instead of a text dump.",
            ));
            detail_lines.push(Line::from(
                "Use Enter for the next setup field, Space to toggle enablement, and B to edit the gateway bind address without leaving the TUI.",
            ));
        }

        if self.detail_fullscreen_active(DetailSurface::GatewayBrowser) {
            self.render_fullscreen_browser_detail(
                frame,
                area,
                FullscreenBrowserChrome {
                    query: &self.gateway_browser.query,
                    header: BrowserChrome {
                        title: "Gateway Control",
                        placeholder:
                            "Search platforms by name, state, delivery mode, or missing setup fields.",
                        icon: "⛵",
                        icon_color: Color::Rgb(120, 220, 200),
                        border_color: Color::Rgb(120, 220, 200),
                    },
                    detail: ScrollableDetailChrome {
                        title: "Details",
                        border_color: Color::Rgb(120, 220, 200),
                        focused: true,
                        requested_scroll: self
                            .detail_fullscreen_scroll(DetailSurface::GatewayBrowser),
                    },
                    help: Line::from(vec![
                        Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(120, 220, 200))),
                        Span::styled("change item  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("type ", Style::default().fg(Color::Rgb(120, 220, 200))),
                        Span::styled("filter  ", Style::default().fg(Color::DarkGray)),
                        self.paging_key_help_span(Color::Rgb(120, 220, 200)),
                        Span::styled("scroll detail  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Enter ", Style::default().fg(Color::Rgb(120, 220, 200))),
                        Span::styled("edit setup  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Space ", Style::default().fg(Color::Rgb(120, 220, 200))),
                        Span::styled("toggle  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("X ", Style::default().fg(Color::Rgb(120, 220, 200))),
                        Span::styled("restart  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Z ", Style::default().fg(Color::Rgb(120, 220, 200))),
                        Span::styled("split view  ", Style::default().fg(Color::DarkGray)),
                        Span::styled("Esc ", Style::default().fg(Color::Rgb(120, 220, 200))),
                        Span::styled("close  ", Style::default().fg(Color::DarkGray)),
                    ]),
                },
                detail_lines,
            );
            return;
        }

        self.render_scrollable_browser_detail(
            frame,
            body[1],
            ScrollableDetailChrome {
                title: "Details",
                border_color: Color::Rgb(120, 220, 200),
                focused: self.gateway_browser_pane.focus == SplitPaneFocus::Detail,
                requested_scroll: self.gateway_browser_pane.scroll,
            },
            detail_lines,
        );

        let help = Paragraph::new(Line::from(vec![
            Span::styled(" ↑↓ ", Style::default().fg(Color::Rgb(120, 220, 200))),
            Span::styled("browse  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter ", Style::default().fg(Color::Rgb(120, 220, 200))),
            Span::styled("edit key field  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Space ", Style::default().fg(Color::Rgb(120, 220, 200))),
            Span::styled("toggle  ", Style::default().fg(Color::DarkGray)),
            Span::styled("A ", Style::default().fg(Color::Rgb(120, 220, 200))),
            Span::styled("allowlist  ", Style::default().fg(Color::DarkGray)),
            Span::styled("H ", Style::default().fg(Color::Rgb(120, 220, 200))),
            Span::styled("home  ", Style::default().fg(Color::DarkGray)),
            Span::styled("B ", Style::default().fg(Color::Rgb(120, 220, 200))),
            Span::styled("bind  ", Style::default().fg(Color::DarkGray)),
            Span::styled("R ", Style::default().fg(Color::Rgb(120, 220, 200))),
            Span::styled("refresh  ", Style::default().fg(Color::DarkGray)),
            Span::styled("X ", Style::default().fg(Color::Rgb(120, 220, 200))),
            Span::styled("restart  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Z ", Style::default().fg(Color::Rgb(120, 220, 200))),
            Span::styled("detail  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Rgb(120, 220, 200))),
            Span::styled("close  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} platform(s)", filtered.len()),
                Style::default().fg(Color::Rgb(95, 120, 112)),
            ),
        ]));
        frame.render_widget(help, chunks[2]);
    }
}
