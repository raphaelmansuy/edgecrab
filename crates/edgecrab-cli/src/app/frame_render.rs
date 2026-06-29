//! Main frame layout: transcript, shelf, status bar, overlay stack.

use super::*;

impl App {
    /// Render the full application frame.
    pub fn render(&mut self, frame: &mut Frame) {
        // Cache terminal width for event handlers that build tool display spans
        self.last_terminal_width = frame.area().width;

        let textarea_height = self.input_area_height_for_area(frame.area());
        let shelf_height = self.shelf_area_height();
        let queue_height = crate::queued_messages::panel_height(self.prompt_queue.len());
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1), // output area
                Constraint::Length(shelf_height),
                Constraint::Length(1), // separator
                Constraint::Length(if self.show_status_bar { 1 } else { 0 }), // status bar
                Constraint::Length(queue_height),
                Constraint::Length(textarea_height), // input area (dynamic height)
            ])
            .split(frame.area());

        self.render_output(frame, chunks[0]);
        if shelf_height > 0 {
            render_activity_shelf(
                frame,
                chunks[1],
                &ShelfRenderParams {
                    state: &self.turn_activity,
                    details: &self.shelf_details,
                    theme: &self.theme,
                    compact: self.shelf_compact_mode(),
                    spinner_frame: self.shelf_spinner_frame(),
                    animate: self.animate_status_indicators,
                    verbose_tools: self.tool_progress_mode == ToolProgressMode::Verbose,
                },
            );
        }
        // Thin horizontal separator between output/shelf and status
        let sep = Paragraph::new(Line::from("─".repeat(chunks[2].width as usize)))
            .style(Style::default().fg(Color::Rgb(60, 60, 70)));
        frame.render_widget(sep, chunks[2]);
        if self.show_status_bar {
            self.render_status_bar(frame, chunks[3]);
        }
        if queue_height > 0 {
            crate::queued_messages::render_queued_messages(
                frame,
                chunks[4],
                &self.prompt_queue,
                self.queue_edit_idx,
                &self.theme,
            );
        }
        if !self.grok_auth.active {
            self.render_input(frame, chunks[5]);
        }

        // Model selector overlay (full screen)
        if self.model_selector.active {
            self.render_model_selector(frame, frame.area());
        }

        if let ModelPickerStage::ExpensiveConfirm {
            ref model,
            ref message,
            ..
        } = self.model_selector_stage
        {
            render_expensive_confirm(frame, frame.area(), model, message, &self.theme);
        }

        // Vision-model selector overlay (full screen)
        if self.vision_model_selector.active {
            self.render_vision_model_selector(frame, frame.area());
        }

        // Image-model selector overlay (full screen)
        if self.image_model_selector.active {
            self.render_image_model_selector(frame, frame.area());
        }

        if self.moa_reference_selector.active {
            self.render_moa_reference_selector(frame, frame.area());
        }

        // MCP selector overlay (full screen, same family as /model)
        if self.mcp_selector.active {
            self.render_mcp_selector(frame, frame.area());
        }

        if self.remote_mcp_browser.selector.active {
            self.render_remote_mcp_selector(frame, frame.area());
        }

        if self.profile_selector.active {
            self.render_profile_selector(frame, frame.area());
        }

        // Skill selector overlay (full screen, takes precedence over model selector)
        if self.skill_selector.active {
            self.render_skill_selector(frame, frame.area());
        }

        if self.tool_manager.active {
            self.render_tool_manager(frame, frame.area());
        }

        if self.plugin_toggle.active {
            self.render_plugin_toggle(frame, frame.area());
        }

        if self.remote_plugin_browser.selector.active {
            self.render_remote_plugin_selector(frame, frame.area());
        }

        if self.remote_skill_browser.selector.active {
            self.render_remote_skill_selector(frame, frame.area());
        }

        if self.config_selector.active {
            self.render_config_selector(frame, frame.area());
        }

        if self.gateway_browser.active {
            self.render_gateway_browser(frame, frame.area());
        }

        if self.diagnose_panel.active {
            self.render_diagnose_panel(frame, frame.area());
        }

        if self.process_tail_panel.is_active() {
            self.render_process_tail_panel(frame, frame.area());
        }

        if self.agents_overlay.is_active() {
            self.render_agents_overlay_panel(frame, frame.area());
        }

        if self.log_browser.active {
            self.render_log_browser(frame, frame.area());
        }

        // Session browser overlay (full screen, same precedence as skill browser)
        if self.session_browser.active {
            self.render_session_browser(frame, frame.area());
        }

        if self.log_inspector.active() {
            self.render_log_inspector(frame, frame.area());
        }

        if self.session_inspector.active() {
            self.render_session_inspector(frame, frame.area());
        }

        // Skin browser overlay (full screen, same precedence as session browser)
        if self.skin_browser.active {
            self.render_skin_browser(frame, frame.area());
        }

        // Verbose / tool-progress picker overlay (compact centered popup)
        if self.verbose_selector_active {
            self.render_verbose_selector(frame, frame.area());
        }

        // Reasoning picker overlay (compact centered popup)
        if self.reasoning_selector_active {
            self.render_reasoning_selector(frame, frame.area());
        }

        if self.details_panel.is_active() {
            details_panel::render_details_panel(
                frame,
                frame.area(),
                &self.details_panel,
                &self.shelf_details,
                &self.theme,
            );
        }

        // Personality picker overlay (compact centered popup)
        if self.personality_selector_active {
            self.render_personality_selector(frame, frame.area());
        }

        if self.document_overlay.is_some() {
            self.render_document_overlay(frame, frame.area());
        }

        if self.web_setup.active {
            self.render_web_setup_tui(frame, frame.area());
        }

        if self.proxy_setup.active {
            self.render_proxy_setup_tui(frame, frame.area());
        }

        if self.grok_auth.active {
            self.render_grok_auth_tui(frame, frame.area());
        }

        // Stream picker overlay (compact centered popup)
        if self.stream_selector_active {
            self.render_stream_selector(frame, frame.area());
        }

        // Status bar picker overlay (compact centered popup)
        if self.statusbar_selector_active {
            self.render_statusbar_selector(frame, frame.area());
        }

        // Shadow Judge picker overlay (compact centered popup)
        if self.shadow_judge_selector_active {
            self.render_shadow_judge_selector(frame, frame.area());
        }

        // Steering overlay (compact floating panel — lower screen half)
        if self.steering_overlay_active {
            self.render_steering_overlay(frame, frame.area());
        }

        // Skill guard trust overlay (high precedence)
        if self.skill_trust_prompt.is_some() {
            self.render_skill_trust_overlay(frame, frame.area());
        }

        // Approval overlay (full screen, highest precedence)
        if matches!(self.display_state, DisplayState::WaitingForApproval { .. }) {
            self.render_approval_overlay(frame, frame.area());
        }

        // Secret capture overlay (full screen, highest precedence — masks the secret)
        if matches!(self.display_state, DisplayState::SecretCapture { .. }) {
            self.render_secret_capture_overlay(frame, frame.area());
        }
        if matches!(self.display_state, DisplayState::ValueCapture { .. }) {
            self.render_value_capture_overlay(frame, frame.area());
        }
    }

    /// Render the scrollable output area with markdown formatting and a scrollbar.
    pub(super) fn render_output(&mut self, frame: &mut Frame, area: Rect) {
        let paging_key_hint = self.paging_key_hint_label();
        let mut metrics = TranscriptScrollMetrics {
            scroll_offset: self.scroll_offset,
            output_visual_rows: self.output_visual_rows,
            output_area_height: self.output_area_height,
            at_bottom: self.at_bottom,
        };
        let mut params = TranscriptRenderParams {
            output: &mut self.output,
            transcript_heights: &mut self.transcript_heights,
            rich_transcript: self.rich_transcript,
            display_state: &self.display_state,
            reasoning_line: self.reasoning_line,
            terminal_glyph_profile: self.terminal_glyph_profile,
            show_output_scrollbar: self.show_output_scrollbar,
            paging_key_hint,
            llm_wait_label: self.turn_activity.llm_wait_label(),
        };
        if self.rich_transcript {
            render_transcript_rich(frame, area, &mut params, &mut metrics);
        } else {
            render_transcript_compact(frame, area, &mut params, &mut metrics);
        }
        self.scroll_offset = metrics.scroll_offset;
        self.output_visual_rows = metrics.output_visual_rows;
        self.output_area_height = metrics.output_area_height;
        self.at_bottom = metrics.at_bottom;
    }

    /// Render the status bar with spinner and color-coded metrics.
    pub(super) fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let editor_mode = match self.editor_mode {
            InputEditorMode::ComposeInsert => StatusBarEditorMode::ComposeInsert,
            InputEditorMode::ComposeNormal => StatusBarEditorMode::ComposeNormal,
            _ => StatusBarEditorMode::Normal,
        };
        let terminal_ui_profile = match self.terminal_ui_profile {
            TerminalUiProfile::Standard => StatusBarUiProfile::Standard,
            TerminalUiProfile::ReducedMotion => StatusBarUiProfile::ReducedMotion,
            TerminalUiProfile::BasicCompat => StatusBarUiProfile::BasicCompat,
        };
        let document_overlay =
            self.document_overlay
                .as_ref()
                .map(|overlay| StatusBarDocumentChip {
                    icon: overlay.icon.clone(),
                    title: overlay.title.clone(),
                    accent: overlay.accent,
                });
        let paging_key_hint = self.paging_key_hint_label();
        let compose_normal_hint = self.compose_normal_hint();
        let inline_compose_hint = self.inline_compose_hint();
        let params = StatusBarRenderParams {
            compact: self.compact_status_bar,
            theme: &self.theme,
            turn_activity: &self.turn_activity,
            shelf_spinner_frame: self.shelf_spinner_frame(),
            terminal_glyph_profile: self.terminal_glyph_profile,
            status_indicator: self.status_indicator,
            spawn_hud_caps: crate::spawn_hud::SpawnHudCaps::from_config(
                edgecrab_core::AppConfig::load()
                    .map(|c| c.delegation.max_subagents)
                    .unwrap_or(3),
            ),
            display_state: &self.display_state,
            thinking_verb_idx: self.thinking_verb_idx,
            kaomoji_frame_idx: self.kaomoji_frame_idx,
            last_terminal_width: self.last_terminal_width,
            goal_flash_status: self.goal_flash_status.as_deref(),
            last_run_outcome: self.last_run_outcome.as_ref(),
            document_overlay,
            goal_status_chip: self.goal_status_chip.as_ref(),
            model_name: &self.model_name,
            context_window: self.context_window,
            total_tokens: self.total_tokens,
            session_cost: self.session_cost,
            voice_presence: self.voice_presence_state(),
            voice_presence_frame_idx: self.voice_presence_frame_idx,
            active_subagents: &self.active_subagents,
            background_tasks_active: &self.background_tasks_active,
            pending_steer_count: self.pending_steer_count,
            steer_applied_at: self.steer_applied_at,
            shadow_judge_enabled: self.shadow_judge_enabled,
            shadow_judge_intervention_at: self.shadow_judge_intervention_at,
            shadow_judge_intervention_confidence: self.shadow_judge_intervention_confidence,
            shadow_judge_intervention_text: self.shadow_judge_intervention_text.as_deref(),
            turn_count: self.turn_count,
            scroll_offset: self.scroll_offset,
            paging_key_hint,
            mouse_capture_enabled: self.mouse_capture_enabled,
            clarify_pending: self.clarify_pending_tx.is_some(),
            is_processing: self.is_processing,
            editor_mode,
            compose_normal_hint: &compose_normal_hint,
            active_skills: &self.active_skills,
            voice_push_to_talk_key: &self.voice_push_to_talk_key,
            inline_compose_hint,
            remote_terminal_session: self.remote_terminal_session,
            terminal_ui_profile,
        };
        render_status_bar_widget(frame, area, &params);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ── Process tail overlay (`/tail`) ─────────────────────────────────────────
    // ─────────────────────────────────────────────────────────────────────────

    pub(super) fn render_process_tail_panel(&self, frame: &mut Frame, area: Rect) {
        let accent = self
            .theme
            .status_bar_model
            .fg
            .unwrap_or(Color::Rgb(205, 175, 50));
        let dim = self.theme.output_system.fg.unwrap_or(Color::DarkGray);
        crate::process_tail_panel::render_process_tail_panel(
            frame,
            area,
            &self.process_tail_panel,
            accent,
            dim,
        );
    }
}
