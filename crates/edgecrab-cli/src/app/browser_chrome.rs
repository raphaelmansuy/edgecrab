//! Shared browser overlay layout + detail pane chrome (DRY across pickers).

use super::*;

impl App {
    pub(super) fn browser_overlay_chunks(area: Rect) -> std::rc::Rc<[Rect]> {
        crate::overlay_layout::browser_overlay_chunks(area)
    }

    pub(super) fn browser_body_chunks(area: Rect) -> std::rc::Rc<[Rect]> {
        crate::overlay_layout::browser_body_chunks(area)
    }

    pub(super) fn browser_list_visible_rows(area: Rect, bordered: bool) -> usize {
        crate::overlay_layout::browser_list_visible_rows(area, bordered)
    }

    pub(super) fn browser_scroll_start(selected: usize, max_visible: usize) -> usize {
        crate::overlay_layout::browser_scroll_start(selected, max_visible)
    }

    pub(super) fn browser_virtual_window(
        selected: usize,
        total: usize,
        max_visible: usize,
    ) -> (usize, usize) {
        crate::overlay_layout::browser_virtual_window(selected, total, max_visible)
    }

    pub(super) fn browser_virtual_range_label(
        scroll_start: usize,
        visible: usize,
        total: usize,
    ) -> String {
        crate::overlay_layout::browser_virtual_range_label(scroll_start, visible, total)
    }

    /// Render a virtualized list: only `items` (already windowed) are drawn, with
    /// a scrollbar reflecting position in the full `total` catalog.
    pub(super) fn render_virtualized_list(
        frame: &mut Frame,
        area: Rect,
        items: Vec<ListItem>,
        scroll_start: usize,
        total: usize,
        bg: Color,
    ) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let max_visible = area.height as usize;
        let needs_scrollbar = total > max_visible && area.width > 1;
        let list_area = if needs_scrollbar {
            Rect {
                x: area.x,
                y: area.y,
                width: area.width.saturating_sub(1),
                height: area.height,
            }
        } else {
            area
        };
        frame.render_widget(
            List::new(items).style(Style::default().bg(bg)),
            list_area,
        );
        if needs_scrollbar {
            let max_scroll = total.saturating_sub(max_visible.max(1));
            let mut scrollbar_state =
                ScrollbarState::new(max_scroll).position(scroll_start.min(max_scroll));
            let scrollbar_area = Rect {
                x: area.right().saturating_sub(1),
                y: area.y,
                width: 1,
                height: area.height,
            };
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .end_symbol(None)
                    .track_symbol(Some("│"))
                    .thumb_symbol("█"),
                scrollbar_area,
                &mut scrollbar_state,
            );
        }
    }

    pub(super) fn best_session_message_selection(
        items: &[SessionMessageEntry],
        matched_role: Option<&str>,
        matched_snippet: Option<&str>,
    ) -> usize {
        let mut best_index = 0usize;
        let mut best_score = 0usize;

        for (index, entry) in items.iter().enumerate() {
            let score = entry.browser_match_score(matched_role, matched_snippet);
            if score > best_score {
                best_score = score;
                best_index = index;
            }
        }

        best_index
    }

    pub(super) fn render_browser_header(
        &self,
        frame: &mut Frame,
        area: Rect,
        query: &str,
        chrome: BrowserChrome<'_>,
    ) {
        let search_text = if query.is_empty() {
            chrome.placeholder.to_string()
        } else {
            query.to_string()
        };
        let search_style = if query.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::White)
        };
        let search = Paragraph::new(Line::from(vec![
            Span::styled(
                format!("  {} ", chrome.icon),
                Style::default().fg(chrome.icon_color),
            ),
            Span::styled(search_text, search_style),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(chrome.border_color))
                .title(format!(" {} ", chrome.title)),
        );
        frame.render_widget(search, area);
    }

    pub(super) fn render_scrollable_browser_detail(
        &self,
        frame: &mut Frame,
        area: Rect,
        chrome: ScrollableDetailChrome<'_>,
        detail_lines: Vec<Line<'static>>,
    ) {
        let border_style = if chrome.focused {
            Style::default()
                .fg(chrome.border_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(60, 80, 84))
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(format!(" {} ", chrome.title));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let visual_rows_for_width = |content_width: usize| -> u16 {
            detail_lines
                .iter()
                .map(|line| {
                    let width = line.width();
                    if width == 0 {
                        1
                    } else {
                        width.div_ceil(content_width.max(1)) as u16
                    }
                })
                .sum()
        };

        let full_width = inner.width.max(1) as usize;
        let mut content_width = full_width;
        let mut visual_rows = visual_rows_for_width(content_width);
        let needs_scrollbar = visual_rows > inner.height;
        if needs_scrollbar && inner.width > 1 {
            content_width = inner.width.saturating_sub(1).max(1) as usize;
            visual_rows = visual_rows_for_width(content_width);
        }
        let max_scroll = visual_rows.saturating_sub(inner.height);
        let scroll = chrome.requested_scroll.min(max_scroll);

        let paragraph = Paragraph::new(Text::from(detail_lines))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));
        let content_area = Rect {
            x: inner.x,
            y: inner.y,
            width: if needs_scrollbar && inner.width > 1 {
                inner.width.saturating_sub(1)
            } else {
                inner.width
            },
            height: inner.height,
        };
        frame.render_widget(paragraph, content_area);

        if needs_scrollbar {
            let mut scrollbar_state =
                ScrollbarState::new(max_scroll as usize).position(scroll as usize);
            let scrollbar_area = Rect {
                x: inner.right().saturating_sub(1),
                y: inner.y,
                width: 1,
                height: inner.height,
            };
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .end_symbol(None)
                    .track_symbol(Some("│"))
                    .thumb_symbol("█"),
                scrollbar_area,
                &mut scrollbar_state,
            );
        }
    }

    pub(super) fn detail_fullscreen_active(&self, surface: DetailSurface) -> bool {
        self.detail_fullscreen
            .is_some_and(|state| state.surface == surface)
    }

    pub(super) fn split_detail_scroll(&self, surface: DetailSurface) -> u16 {
        match surface {
            DetailSurface::GatewayBrowser => self.gateway_browser_pane.scroll,
            DetailSurface::LogBrowser => self.log_browser_pane.scroll,
            DetailSurface::SessionBrowser => self.session_browser_pane.scroll,
            DetailSurface::SessionInspector => self.session_inspector.pane.scroll,
            DetailSurface::LogInspector => self.log_inspector.pane.scroll,
            _ => self.simple_detail_state.scroll_for(surface),
        }
    }

    pub(super) fn set_split_detail_scroll(&mut self, surface: DetailSurface, scroll: u16) {
        match surface {
            DetailSurface::GatewayBrowser => self.gateway_browser_pane.scroll = scroll,
            DetailSurface::LogBrowser => self.log_browser_pane.scroll = scroll,
            DetailSurface::SessionBrowser => self.session_browser_pane.scroll = scroll,
            DetailSurface::SessionInspector => self.session_inspector.pane.scroll = scroll,
            DetailSurface::LogInspector => self.log_inspector.pane.scroll = scroll,
            _ => self.simple_detail_state.set_scroll(surface, scroll),
        }
    }

    pub(super) fn reset_split_detail_scroll(&mut self, surface: DetailSurface) {
        match surface {
            DetailSurface::GatewayBrowser => self.gateway_browser_pane.reset_scroll(),
            DetailSurface::LogBrowser => self.log_browser_pane.reset_scroll(),
            DetailSurface::SessionBrowser => self.session_browser_pane.reset_scroll(),
            DetailSurface::SessionInspector => self.session_inspector.pane.reset_scroll(),
            DetailSurface::LogInspector => self.log_inspector.pane.reset_scroll(),
            _ => self.simple_detail_state.reset(surface),
        }
        self.needs_redraw = true;
    }

    pub(super) fn page_up_split_detail(&mut self, surface: DetailSurface, step: u16) {
        let scroll = self
            .split_detail_scroll(surface)
            .saturating_sub(step.max(1));
        self.set_split_detail_scroll(surface, scroll);
        self.needs_redraw = true;
    }

    pub(super) fn page_down_split_detail(&mut self, surface: DetailSurface, step: u16) {
        let scroll = self
            .split_detail_scroll(surface)
            .saturating_add(step.max(1));
        self.set_split_detail_scroll(surface, scroll);
        self.needs_redraw = true;
    }

    pub(super) fn render_standard_split_detail(
        &self,
        frame: &mut Frame,
        area: Rect,
        surface: DetailSurface,
        border_color: Color,
        detail_lines: Vec<Line<'static>>,
    ) {
        self.render_scrollable_browser_detail(
            frame,
            area,
            ScrollableDetailChrome {
                title: "Details",
                border_color,
                focused: if Self::simple_split_focus_supported(surface) {
                    self.simple_split_focus(surface) == SplitPaneFocus::Detail
                } else {
                    true
                },
                requested_scroll: self.split_detail_scroll(surface),
            },
            detail_lines,
        );
    }

    pub(super) fn toggle_detail_fullscreen(&mut self, surface: DetailSurface, initial_scroll: u16) {
        if self.detail_fullscreen_active(surface) {
            let scroll = self.detail_fullscreen_scroll(surface);
            self.set_split_detail_scroll(surface, scroll);
            self.detail_fullscreen = None;
        } else {
            self.detail_fullscreen = Some(DetailFullscreenState {
                surface,
                scroll: initial_scroll,
            });
        }
        self.needs_redraw = true;
    }

    pub(super) fn close_detail_fullscreen(&mut self, surface: DetailSurface) -> bool {
        if self.detail_fullscreen_active(surface) {
            let scroll = self.detail_fullscreen_scroll(surface);
            self.set_split_detail_scroll(surface, scroll);
            self.detail_fullscreen = None;
            self.needs_redraw = true;
            true
        } else {
            false
        }
    }

    pub(super) fn reset_detail_fullscreen_scroll(&mut self, surface: DetailSurface) {
        if let Some(state) = self.detail_fullscreen.as_mut()
            && state.surface == surface
        {
            state.scroll = 0;
            self.needs_redraw = true;
        }
    }

    pub(super) fn page_up_detail_fullscreen(&mut self, surface: DetailSurface) {
        if let Some(state) = self.detail_fullscreen.as_mut()
            && state.surface == surface
        {
            state.scroll = state.scroll.saturating_sub(8);
            self.needs_redraw = true;
        }
    }

    pub(super) fn page_down_detail_fullscreen(&mut self, surface: DetailSurface) {
        if let Some(state) = self.detail_fullscreen.as_mut()
            && state.surface == surface
        {
            state.scroll = state.scroll.saturating_add(8);
            self.needs_redraw = true;
        }
    }

    pub(super) fn page_simple_selector_list(
        &mut self,
        surface: DetailSurface,
        intent: PagingIntent,
    ) {
        match surface {
            DetailSurface::ModelSelector => match intent {
                PagingIntent::Up => self.model_selector.page_up(),
                PagingIntent::Down => self.model_selector.page_down(),
            },
            DetailSurface::VisionModelSelector => match intent {
                PagingIntent::Up => self.vision_model_selector.page_up(),
                PagingIntent::Down => self.vision_model_selector.page_down(),
            },
            DetailSurface::ImageModelSelector => match intent {
                PagingIntent::Up => self.image_model_selector.page_up(),
                PagingIntent::Down => self.image_model_selector.page_down(),
            },
            DetailSurface::McpSelector => match intent {
                PagingIntent::Up => self.mcp_selector.page_up(),
                PagingIntent::Down => self.mcp_selector.page_down(),
            },
            DetailSurface::RemoteMcpBrowser => match intent {
                PagingIntent::Up => self.remote_mcp_browser.selector.page_up(),
                PagingIntent::Down => self.remote_mcp_browser.selector.page_down(),
            },
            DetailSurface::RemoteSkillBrowser => {
                match intent {
                    PagingIntent::Up => self.remote_skill_browser.selector.page_up(),
                    PagingIntent::Down => self.remote_skill_browser.selector.page_down(),
                }
                self.schedule_remote_skill_guard_preview();
                self.maybe_extend_browse_cache();
            }
            DetailSurface::RemotePluginBrowser => match intent {
                PagingIntent::Up => self.remote_plugin_browser.selector.page_up(),
                PagingIntent::Down => self.remote_plugin_browser.selector.page_down(),
            },
            DetailSurface::ProfileSelector => match intent {
                PagingIntent::Up => self.profile_selector.page_up(),
                PagingIntent::Down => self.profile_selector.page_down(),
            },
            DetailSurface::SkillSelector => match intent {
                PagingIntent::Up => self.skill_selector.page_up(),
                PagingIntent::Down => self.skill_selector.page_down(),
            },
            DetailSurface::ToolManager => match intent {
                PagingIntent::Up => self.tool_manager.page_up(),
                PagingIntent::Down => self.tool_manager.page_down(),
            },
            DetailSurface::PluginToggle => match intent {
                PagingIntent::Up => self.plugin_toggle.page_up(),
                PagingIntent::Down => self.plugin_toggle.page_down(),
            },
            DetailSurface::ConfigSelector => match intent {
                PagingIntent::Up => self.config_selector.page_up(),
                PagingIntent::Down => self.config_selector.page_down(),
            },
            DetailSurface::GatewayBrowser
            | DetailSurface::SessionBrowser
            | DetailSurface::SessionInspector
            | DetailSurface::LogBrowser
            | DetailSurface::LogInspector => return,
        }

        self.reset_split_detail_scroll(surface);
        self.reset_detail_fullscreen_scroll(surface);
        self.needs_redraw = true;
    }

    pub(super) fn apply_simple_selector_paging(
        &mut self,
        surface: DetailSurface,
        intent: PagingIntent,
    ) {
        if self.detail_fullscreen_active(surface) {
            match intent {
                PagingIntent::Up => self.page_up_detail_fullscreen(surface),
                PagingIntent::Down => self.page_down_detail_fullscreen(surface),
            }
        } else if self.simple_split_detail_focused(surface) {
            match intent {
                PagingIntent::Up => self.page_up_split_detail(surface, 8),
                PagingIntent::Down => self.page_down_split_detail(surface, 8),
            }
        } else {
            self.page_simple_selector_list(surface, intent);
        }
    }

    pub(super) fn detail_fullscreen_scroll(&self, surface: DetailSurface) -> u16 {
        self.detail_fullscreen
            .filter(|state| state.surface == surface)
            .map_or(0, |state| state.scroll)
    }

    pub(super) fn render_fullscreen_browser_detail(
        &self,
        frame: &mut Frame,
        area: Rect,
        chrome: FullscreenBrowserChrome<'_>,
        detail_lines: Vec<Line<'static>>,
    ) {
        frame.render_widget(Clear, area);
        let chunks = Self::browser_overlay_chunks(area);
        let header_title = format!("{} · Detail", chrome.header.title);
        self.render_browser_header(
            frame,
            chunks[0],
            chrome.query,
            BrowserChrome {
                title: &header_title,
                placeholder: chrome.header.placeholder,
                icon: chrome.header.icon,
                icon_color: chrome.header.icon_color,
                border_color: chrome.header.border_color,
            },
        );
        self.render_scrollable_browser_detail(frame, chunks[1], chrome.detail, detail_lines);
        frame.render_widget(
            Paragraph::new(self.normalize_paging_help_line(chrome.help)),
            chunks[2],
        );
    }
}
