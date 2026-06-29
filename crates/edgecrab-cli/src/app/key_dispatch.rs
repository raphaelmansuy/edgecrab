//! Key event dispatch — extracted from `app.rs` (Hermes `useInputHandlers` parity).

use super::*;

impl App {
    pub fn handle_key_event(&mut self, key: event::KeyEvent) {
        // Only process key press events, ignore release events (prevents double-fire on Windows)
        if key.kind == KeyEventKind::Release {
            return;
        }
        let raw_key = key;
        let key = normalize_fallback_paging_key(normalize_terminal_control_key(key));

        self.needs_redraw = true;

        // Modal overlays must run before global shortcuts and the main chat textarea.
        if self.grok_auth.active {
            self.handle_grok_auth_key(key);
            return;
        }

        if matches!(
            self.model_selector_stage,
            ModelPickerStage::ExpensiveConfirm { .. }
        ) {
            match handle_picker_keys(&self.model_selector_stage, key) {
                ModelPickerKeyAction::ConfirmExpensive => {
                    if let ModelPickerStage::ExpensiveConfirm { model, intent, .. } =
                        self.model_selector_stage.clone()
                    {
                        self.model_selector_stage.reset();
                        self.model_selector.active = false;
                        self.apply_model_switch_intent(model, intent);
                    }
                }
                ModelPickerKeyAction::CancelExpensive => {
                    self.model_selector_stage.reset();
                }
                _ => {}
            }
            return;
        }

        if key_matches_binding(&key, &self.voice_push_to_talk_key) {
            self.toggle_voice_recording(true);
            return;
        }

        // Global shortcuts first — these work regardless of any overlay
        match (key.modifiers, key.code) {
            // Ctrl+C — clear input → cancel agent → exit  (standard readline behaviour)
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                let text = self.textarea_text();
                if !text.is_empty() {
                    // Non-empty input: clear it (like ^C at a shell prompt)
                    self.textarea_clear();
                    self.completion.active = false;
                    self.history_pos = self.input_history.len();
                    self.push_output("^C", OutputRole::System);
                } else if self.voice_recording.is_some() {
                    self.abort_voice_recording("^C  (voice recording cancelled)");
                } else if self.is_processing {
                    self.cancel_active_request();
                    self.push_output("^C  (cancelled)", OutputRole::System);
                } else {
                    // Nothing to do: exit
                    self.should_exit = true;
                }
                return;
            }
            // Ctrl+D — exit (EOF signal, identical to shell behaviour)
            (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
                let text = self.textarea_text();
                if text.is_empty() {
                    self.should_exit = true;
                }
                // Non-empty: let textarea handle delete-char (standard readline)
                return;
            }
            // Ctrl+L — clear screen (standard shell shortcut)
            (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
                self.clear_output();
                return;
            }
            // Ctrl+Shift+V — paste clipboard image (or text) into conversation.
            // Ctrl+V (without Shift) arrives as a bracketed-paste Event::Paste, so
            // this shortcut gives explicit access to the arboard clipboard reader
            // which can capture raw images (screenshots, browser copies, etc.).
            (m, KeyCode::Char('v'))
                if m.contains(KeyModifiers::CONTROL) && m.contains(KeyModifiers::SHIFT) =>
            {
                self.handle_paste_clipboard();
                return;
            }
            // Ctrl+Shift+T — expand/collapse the most recent expandable tool result.
            (m, KeyCode::Char('t'))
                if m.contains(KeyModifiers::CONTROL) && m.contains(KeyModifiers::SHIFT) =>
            {
                self.toggle_tool_result_expand();
                return;
            }
            // F6 — toggle mouse capture mode for copy/select ergonomics.
            (_, KeyCode::F(6)) => {
                self.toggle_mouse_capture_mode();
                return;
            }
            // Ctrl+M — alternate toggle for mouse capture mode.
            (KeyModifiers::CONTROL, KeyCode::Char('m')) => {
                self.toggle_mouse_capture_mode();
                return;
            }
            // Ctrl+U — clear current input line (standard readline shortcut)
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => {
                self.textarea_clear();
                self.completion.active = false;
                return;
            }
            // Ctrl+G — scroll output to very bottom (jump back to live view)
            (KeyModifiers::CONTROL, KeyCode::Char('g')) => {
                self.scroll_offset = 0;
                self.at_bottom = true;
                self.needs_redraw = true;
                return;
            }
            // Ctrl+K — kill text from cursor to end of line (readline standard)
            (KeyModifiers::CONTROL, KeyCode::Char('k')) => {
                self.textarea.delete_line_by_end();
                self.needs_redraw = true;
                return;
            }
            // Ctrl+A — move cursor to beginning of line (readline standard)
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => {
                self.textarea.move_cursor(CursorMove::Head);
                self.needs_redraw = true;
                return;
            }
            // Ctrl+E — move cursor to end of line (readline standard)
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => {
                self.textarea.move_cursor(CursorMove::End);
                self.needs_redraw = true;
                return;
            }
            // Ctrl+Home — scroll output to very top
            (KeyModifiers::CONTROL, KeyCode::Home) => {
                let max_scroll = self
                    .output_visual_rows
                    .saturating_sub(self.output_area_height);
                self.scroll_offset = max_scroll;
                self.at_bottom = false;
                return;
            }
            // Ctrl+End — scroll output to very bottom
            (KeyModifiers::CONTROL, KeyCode::End) => {
                self.scroll_offset = 0;
                self.at_bottom = true;
                return;
            }
            // Shift+Up — scroll output up one line (doesn't conflict with history navigation)
            (KeyModifiers::SHIFT, KeyCode::Up) => {
                self.scroll_output(5);
                return;
            }
            // Shift+Down — scroll output down one line
            (KeyModifiers::SHIFT, KeyCode::Down) => {
                self.scroll_output(-5);
                return;
            }
            // Alt+Up — scroll output up (works in multi-line input mode)
            (KeyModifiers::ALT, KeyCode::Up) => {
                self.scroll_output(5);
                return;
            }
            // Alt+Down — scroll output down
            (KeyModifiers::ALT, KeyCode::Down) => {
                self.scroll_output(-5);
                return;
            }
            // F1 — show help overlay
            (_, KeyCode::F(1)) => {
                self.process_input("/help");
                return;
            }
            // F2 — open model selector
            (_, KeyCode::F(2)) => {
                self.refresh_model_selector_catalog();
                return;
            }
            // F3 — open skill browser (same experience as F2 for models)
            (_, KeyCode::F(3)) => {
                self.refresh_skills_list();
                self.open_skill_selector();
                return;
            }
            // F7 — open dedicated vision-model selector
            (_, KeyCode::F(7)) => {
                self.open_vision_model_selector();
                return;
            }
            // F4 — open session browser overlay
            (_, KeyCode::F(4)) => {
                self.open_session_browser();
                return;
            }
            // F5 — retry last message
            (_, KeyCode::F(5)) => {
                self.process_input("/retry");
                return;
            }
            // F10 - cycle tool progress mode
            (_, KeyCode::F(10)) => {
                self.process_input("/verbose");
                return;
            }
            _ => {}
        }

        if let Some(intent) = paging_intent_for_key(raw_key).or_else(|| paging_intent_for_key(key))
            && self.handle_paging_intent(intent)
        {
            return;
        }

        // Approval overlay active — intercept all keys for choice navigation
        if matches!(self.display_state, DisplayState::WaitingForApproval { .. }) {
            self.handle_approval_key(key);
            return;
        }

        // Secret capture overlay active — intercept all keys for masked input
        if matches!(self.display_state, DisplayState::SecretCapture { .. }) {
            self.handle_secret_capture_key(key);
            return;
        }

        if matches!(self.display_state, DisplayState::ValueCapture { .. }) {
            self.handle_value_capture_key(key);
            return;
        }

        if self.web_setup.active {
            let action = self.web_setup.handle_key(key);
            match action {
                crate::web_setup_tui::WebSetupAction::Close => {
                    self.web_setup.close();
                    self.needs_redraw = true;
                }
                crate::web_setup_tui::WebSetupAction::Redraw => self.needs_redraw = true,
                crate::web_setup_tui::WebSetupAction::ChainSaved => {
                    if let Some(agent) = &self.agent {
                        self.rt_handle.block_on(agent.reload_web_search_from_disk());
                    }
                    self.needs_redraw = true;
                }
                crate::web_setup_tui::WebSetupAction::None => {}
            }
            return;
        }

        if self.proxy_setup.active {
            let action = self.proxy_setup.handle_key(key);
            match action {
                crate::proxy_setup_tui::ProxySetupAction::Close => {
                    self.proxy_setup.close();
                    self.needs_redraw = true;
                }
                crate::proxy_setup_tui::ProxySetupAction::Redraw
                | crate::proxy_setup_tui::ProxySetupAction::ConfigSaved => {
                    self.needs_redraw = true;
                }
                crate::proxy_setup_tui::ProxySetupAction::RunOAuthLogin(target) => {
                    if crate::auth_cmd::is_grok_auth_target(target) {
                        self.open_grok_auth_overlay(crate::grok_auth_tui::GrokAuthScreen::Start);
                        self.proxy_setup.toast = Some(
                            "Grok sign-in — follow the overlay (paste code from x.ai).".into(),
                        );
                    } else {
                        self.run_login_target_with_terminal_handoff(target, false, false);
                        self.proxy_setup.toast =
                            Some("OAuth sign-in finished — check ✓ on the preset.".into());
                    }
                    self.needs_redraw = true;
                }
                crate::proxy_setup_tui::ProxySetupAction::None => {}
            }
            return;
        }

        if self.document_overlay.is_some() {
            self.handle_document_overlay_key(key);
            return;
        }

        // ── Steering overlay active — intercept all keys ──────────────────
        if self.steering_overlay_active {
            self.handle_steering_overlay_key(key);
            return;
        }

        // Ctrl+S — open steering overlay (when agent is running) or send steer
        // as a new message if idle (EC-04).
        if matches!(
            (key.modifiers, key.code),
            (KeyModifiers::CONTROL, KeyCode::Char('s'))
        ) && !matches!(
            self.editor_mode,
            InputEditorMode::ComposeInsert | InputEditorMode::ComposeNormal
        ) {
            self.open_steering_overlay();
            return;
        }

        // Gateway Diagnostics overlay — full-screen scrollable report
        if self.agents_overlay.is_active() {
            let rows = self.agents_delegate_rows();
            let row_count = rows.len();
            let turn_count = self.spawn_history.turn_count();
            let live_mode = self.agents_overlay.history_index == 0;
            let selected = rows
                .get(self.agents_overlay.cursor)
                .map(|row| row.agent_id.as_str());
            let action = handle_agents_overlay_key(
                &mut self.agents_overlay,
                key,
                row_count,
                turn_count,
                selected,
                live_mode,
            );
            match action {
                AgentsOverlayAction::None
                | AgentsOverlayAction::Close
                | AgentsOverlayAction::Refresh
                | AgentsOverlayAction::ToggleDiff
                | AgentsOverlayAction::HistoryChanged => {}
                AgentsOverlayAction::SendStopSteer => self.send_stop_steer_from_agents_overlay(),
                AgentsOverlayAction::InterruptSubagent(id) => {
                    self.interrupt_subagents_from_agents_overlay(&[id]);
                }
                AgentsOverlayAction::InterruptSubtree(root) => {
                    let ids = crate::subagent_tree::descendant_agent_ids(&rows, &root);
                    self.interrupt_subagents_from_agents_overlay(&ids);
                }
                AgentsOverlayAction::ToggleSpawnPause => {
                    self.toggle_spawn_pause_from_agents_overlay();
                }
            }
            self.needs_redraw = true;
            return;
        }

        if self.process_tail_panel.is_active() {
            match key.code {
                KeyCode::Esc => {
                    self.process_tail_panel.close();
                    self.needs_redraw = true;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.process_tail_panel.scroll_offset =
                        self.process_tail_panel.scroll_offset.saturating_sub(1);
                    self.needs_redraw = true;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.process_tail_panel.scroll_offset =
                        self.process_tail_panel.scroll_offset.saturating_add(1);
                    self.needs_redraw = true;
                }
                _ => {}
            }
            return;
        }

        if self.details_panel.is_active() {
            let action = details_panel::handle_details_panel_key(
                &mut self.details_panel,
                key,
                &mut self.shelf_details,
            );
            match action {
                DetailsPanelAction::None | DetailsPanelAction::Close => {}
                DetailsPanelAction::Changed => {
                    if let Err(e) = persist_shelf_details(&self.shelf_details) {
                        self.push_output(
                            format!("(warning: failed to persist /details: {e})"),
                            OutputRole::System,
                        );
                    }
                }
            }
            self.needs_redraw = true;
            return;
        }

        if self.diagnose_panel.active {
            self.handle_diagnose_panel_key(key);
            return;
        }

        // Reasoning picker overlay active — intercept all keys
        if self.reasoning_selector_active {
            match key.code {
                KeyCode::Esc => {
                    self.reasoning_selector_active = false;
                    self.needs_redraw = true;
                }
                KeyCode::Up | KeyCode::BackTab => {
                    if self.reasoning_selector_cursor > 0 {
                        self.reasoning_selector_cursor -= 1;
                    } else {
                        self.reasoning_selector_cursor = 4;
                    }
                    self.needs_redraw = true;
                }
                KeyCode::Down | KeyCode::Tab => {
                    self.reasoning_selector_cursor = (self.reasoning_selector_cursor + 1) % 5;
                    self.needs_redraw = true;
                }
                KeyCode::Enter => {
                    let arg =
                        ["low", "medium", "high", "show", "hide"][self.reasoning_selector_cursor];
                    self.reasoning_selector_active = false;
                    self.handle_set_reasoning(arg.to_string());
                    self.needs_redraw = true;
                }
                _ => {}
            }
            return;
        }

        // Personality picker overlay active — intercept all keys
        if self.personality_selector_active {
            match key.code {
                KeyCode::Esc => {
                    self.personality_selector_active = false;
                    self.needs_redraw = true;
                }
                KeyCode::Up | KeyCode::BackTab => {
                    let n = self.personality_selector_entries.len();
                    if n > 0 {
                        if self.personality_selector_cursor > 0 {
                            self.personality_selector_cursor -= 1;
                        } else {
                            self.personality_selector_cursor = n.saturating_sub(1);
                        }
                    }
                    self.needs_redraw = true;
                }
                KeyCode::Down | KeyCode::Tab => {
                    let n = self.personality_selector_entries.len();
                    if n > 0 {
                        self.personality_selector_cursor =
                            (self.personality_selector_cursor + 1) % n;
                    }
                    self.needs_redraw = true;
                }
                KeyCode::Enter => {
                    if let Some((name, _)) = self
                        .personality_selector_entries
                        .get(self.personality_selector_cursor)
                        .cloned()
                    {
                        self.personality_selector_active = false;
                        self.handle_switch_personality(name);
                        self.needs_redraw = true;
                    }
                }
                _ => {}
            }
            return;
        }

        // Stream picker overlay active — intercept all keys
        if self.stream_selector_active {
            match key.code {
                KeyCode::Esc => {
                    self.stream_selector_active = false;
                    self.needs_redraw = true;
                }
                KeyCode::Up | KeyCode::BackTab | KeyCode::Down | KeyCode::Tab => {
                    self.stream_selector_cursor = 1 - self.stream_selector_cursor;
                    self.needs_redraw = true;
                }
                KeyCode::Enter => {
                    let arg = if self.stream_selector_cursor == 0 {
                        "on"
                    } else {
                        "off"
                    };
                    self.stream_selector_active = false;
                    self.handle_set_streaming(arg.to_string());
                    self.needs_redraw = true;
                }
                _ => {}
            }
            return;
        }

        // Status bar picker overlay active — intercept all keys
        if self.statusbar_selector_active {
            match key.code {
                KeyCode::Esc => {
                    self.statusbar_selector_active = false;
                    self.needs_redraw = true;
                }
                KeyCode::Up | KeyCode::BackTab | KeyCode::Down | KeyCode::Tab => {
                    self.statusbar_selector_cursor = 1 - self.statusbar_selector_cursor;
                    self.needs_redraw = true;
                }
                KeyCode::Enter => {
                    let arg = if self.statusbar_selector_cursor == 0 {
                        "on"
                    } else {
                        "off"
                    };
                    self.statusbar_selector_active = false;
                    self.handle_status_bar_command(arg.to_string());
                    self.needs_redraw = true;
                }
                _ => {}
            }
            return;
        }

        // Shadow Judge picker overlay active — intercept all keys
        if self.shadow_judge_selector_active {
            match key.code {
                KeyCode::Esc => {
                    self.shadow_judge_selector_active = false;
                    self.needs_redraw = true;
                }
                KeyCode::Up | KeyCode::BackTab | KeyCode::Down | KeyCode::Tab => {
                    self.shadow_judge_selector_cursor = 1 - self.shadow_judge_selector_cursor;
                    self.needs_redraw = true;
                }
                KeyCode::Enter => {
                    let arg = if self.shadow_judge_selector_cursor == 0 {
                        "on"
                    } else {
                        "off"
                    };
                    self.shadow_judge_selector_active = false;
                    self.handle_set_shadow_judge(arg.to_string());
                    self.needs_redraw = true;
                }
                _ => {}
            }
            return;
        }

        // Verbose / tool-progress picker overlay active — intercept all keys
        if self.verbose_selector_active {
            match key.code {
                KeyCode::Esc => {
                    self.verbose_selector_active = false;
                    self.needs_redraw = true;
                }
                KeyCode::Up | KeyCode::BackTab => {
                    if self.verbose_selector_cursor > 0 {
                        self.verbose_selector_cursor -= 1;
                    } else {
                        self.verbose_selector_cursor = 3;
                    }
                    self.needs_redraw = true;
                }
                KeyCode::Down | KeyCode::Tab => {
                    self.verbose_selector_cursor = (self.verbose_selector_cursor + 1) % 4;
                    self.needs_redraw = true;
                }
                KeyCode::Enter => {
                    let mode = [
                        ToolProgressMode::Off,
                        ToolProgressMode::New,
                        ToolProgressMode::All,
                        ToolProgressMode::Verbose,
                    ][self.verbose_selector_cursor];
                    let msg = self.set_tool_progress_mode_explicit(mode);
                    self.verbose_selector_active = false;
                    self.push_output(msg, OutputRole::System);
                    self.needs_redraw = true;
                }
                _ => {}
            }
            return;
        }

        // Model selector overlay active — intercept all keys
        if self.model_selector.active {
            if matches!(
                self.model_selector_stage,
                ModelPickerStage::DisconnectConfirm { .. }
            ) {
                match handle_disconnect_keys(&self.model_selector_stage, key) {
                    ModelPickerKeyAction::ConfirmDisconnect => {
                        if let ModelPickerStage::DisconnectConfirm { provider } =
                            self.model_selector_stage.clone()
                        {
                            match execute_disconnect(&provider) {
                                Ok(msg) => {
                                    self.push_output(msg, OutputRole::System);
                                    self.refresh_model_selector_catalog();
                                }
                                Err(err) => self.push_output(err, OutputRole::Error),
                            }
                        }
                        self.model_selector_stage.reset();
                    }
                    ModelPickerKeyAction::CancelDisconnect => {
                        self.model_selector_stage.reset();
                    }
                    ModelPickerKeyAction::None
                    | ModelPickerKeyAction::RequestDisconnect
                    | ModelPickerKeyAction::ConfirmExpensive
                    | ModelPickerKeyAction::CancelExpensive => {}
                }
                self.needs_redraw = true;
                return;
            }
            if matches!(
                handle_disconnect_keys(&ModelPickerStage::Browse, key),
                ModelPickerKeyAction::RequestDisconnect
            ) {
                let provider = self
                    .model_selector
                    .current()
                    .and_then(|entry| browse_disconnect_provider(Some(&entry.provider)));
                if let Some(provider) = provider {
                    self.model_selector_stage = ModelPickerStage::DisconnectConfirm { provider };
                } else {
                    self.push_output(
                        "No managed credentials to disconnect for this provider.",
                        OutputRole::System,
                    );
                }
                self.needs_redraw = true;
                return;
            }
            match key.code {
                KeyCode::Esc if !self.close_detail_fullscreen(DetailSurface::ModelSelector) => {
                    self.model_selector.active = false;
                    self.model_selector_stage.reset();
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::ModelSelector,
                        self.split_detail_scroll(DetailSurface::ModelSelector),
                    );
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::ModelSelector) =>
                {
                    self.toggle_simple_split_focus(DetailSurface::ModelSelector);
                }
                KeyCode::Enter => {
                    if let Some(model) = self.model_selector.current().map(|e| e.display.clone()) {
                        let intent = match self.model_selector_target {
                            ModelSelectorTarget::Primary => ModelSwitchIntent::Primary,
                            ModelSelectorTarget::Cheap => ModelSwitchIntent::Cheap,
                            ModelSelectorTarget::MoaAggregator => ModelSwitchIntent::MoaAggregator,
                        };
                        self.model_selector.active = false;
                        self.close_detail_fullscreen(DetailSurface::ModelSelector);
                        self.try_model_switch(model, intent);
                    }
                }
                KeyCode::Up => {
                    if self.simple_split_detail_focused(DetailSurface::ModelSelector) {
                        self.scroll_split_detail_lines(DetailSurface::ModelSelector, -1);
                    } else {
                        self.model_selector.move_up();
                        self.reset_split_detail_scroll(DetailSurface::ModelSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ModelSelector);
                    }
                }
                KeyCode::Down => {
                    if self.simple_split_detail_focused(DetailSurface::ModelSelector) {
                        self.scroll_split_detail_lines(DetailSurface::ModelSelector, 1);
                    } else {
                        self.model_selector.move_down();
                        self.reset_split_detail_scroll(DetailSurface::ModelSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ModelSelector);
                    }
                }
                KeyCode::PageUp if self.detail_fullscreen_active(DetailSurface::ModelSelector) => {
                    self.page_up_detail_fullscreen(DetailSurface::ModelSelector);
                }
                KeyCode::PageDown
                    if self.detail_fullscreen_active(DetailSurface::ModelSelector) =>
                {
                    self.page_down_detail_fullscreen(DetailSurface::ModelSelector);
                }
                KeyCode::PageUp => self
                    .apply_simple_selector_paging(DetailSurface::ModelSelector, PagingIntent::Up),
                KeyCode::PageDown => self
                    .apply_simple_selector_paging(DetailSurface::ModelSelector, PagingIntent::Down),
                KeyCode::Home => {
                    if self.simple_split_detail_focused(DetailSurface::ModelSelector) {
                        self.set_split_detail_scroll_to_edge(DetailSurface::ModelSelector, false);
                    } else {
                        self.model_selector.selected = 0;
                        self.reset_split_detail_scroll(DetailSurface::ModelSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ModelSelector);
                    }
                }
                KeyCode::End => {
                    if self.simple_split_detail_focused(DetailSurface::ModelSelector) {
                        self.set_split_detail_scroll_to_edge(DetailSurface::ModelSelector, true);
                    } else {
                        self.model_selector.selected =
                            self.model_selector.filtered.len().saturating_sub(1);
                        self.reset_split_detail_scroll(DetailSurface::ModelSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ModelSelector);
                    }
                }
                KeyCode::Backspace => {
                    self.model_selector.pop_char();
                    self.reset_split_detail_scroll(DetailSurface::ModelSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ModelSelector);
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.model_selector.push_char(c);
                    self.reset_split_detail_scroll(DetailSurface::ModelSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ModelSelector);
                }
                _ => {}
            }
            return;
        }

        if self.gateway_browser.active {
            match key.code {
                KeyCode::Esc if !self.close_detail_fullscreen(DetailSurface::GatewayBrowser) => {
                    self.gateway_browser.active = false;
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::GatewayBrowser,
                        self.gateway_browser_pane.scroll,
                    );
                }
                KeyCode::Enter => {
                    self.open_gateway_primary_editor();
                }
                KeyCode::Char(' ') => {
                    self.toggle_selected_gateway_platform();
                    self.reset_detail_fullscreen_scroll(DetailSurface::GatewayBrowser);
                }
                _ if selector_action_key(&key, 'a') => {
                    self.open_gateway_allowlist_editor();
                }
                _ if selector_action_key(&key, 'h') => {
                    self.open_gateway_home_channel_editor();
                }
                _ if selector_action_key(&key, 'b') => {
                    self.open_gateway_bind_editor();
                }
                _ if selector_action_key(&key, 'r') => {
                    self.refresh_gateway_browser();
                }
                _ if selector_action_key(&key, 'x') => {
                    self.handle_gateway_control("restart".into());
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::GatewayBrowser) =>
                {
                    self.gateway_browser_pane.focus = self.gateway_browser_pane.focus.toggle();
                }
                KeyCode::Up => {
                    self.gateway_browser.move_up();
                    self.gateway_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::GatewayBrowser);
                }
                KeyCode::Down => {
                    self.gateway_browser.move_down();
                    self.gateway_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::GatewayBrowser);
                }
                KeyCode::PageUp if self.detail_fullscreen_active(DetailSurface::GatewayBrowser) => {
                    self.page_up_detail_fullscreen(DetailSurface::GatewayBrowser);
                }
                KeyCode::PageUp => {
                    if self.gateway_browser_pane.focus == SplitPaneFocus::Detail {
                        self.gateway_browser_pane.page_up(8);
                    } else {
                        self.gateway_browser.page_up();
                        self.gateway_browser_pane.reset_scroll();
                        self.reset_detail_fullscreen_scroll(DetailSurface::GatewayBrowser);
                    }
                }
                KeyCode::PageDown
                    if self.detail_fullscreen_active(DetailSurface::GatewayBrowser) =>
                {
                    self.page_down_detail_fullscreen(DetailSurface::GatewayBrowser);
                }
                KeyCode::PageDown => {
                    if self.gateway_browser_pane.focus == SplitPaneFocus::Detail {
                        self.gateway_browser_pane.page_down(8);
                    } else {
                        self.gateway_browser.page_down();
                        self.gateway_browser_pane.reset_scroll();
                        self.reset_detail_fullscreen_scroll(DetailSurface::GatewayBrowser);
                    }
                }
                KeyCode::Home => {
                    self.gateway_browser.selected = 0;
                    self.gateway_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::GatewayBrowser);
                }
                KeyCode::End => {
                    self.gateway_browser.selected =
                        self.gateway_browser.filtered.len().saturating_sub(1);
                    self.gateway_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::GatewayBrowser);
                }
                KeyCode::Backspace => {
                    self.gateway_browser.pop_char();
                    self.gateway_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::GatewayBrowser);
                }
                KeyCode::Char(c) if selector_search_char(&key) == Some(c) => {
                    self.gateway_browser.push_char(c);
                    self.gateway_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::GatewayBrowser);
                }
                _ => {}
            }
            self.needs_redraw = true;
            return;
        }

        if self.moa_reference_selector.active {
            match key.code {
                KeyCode::Esc => {
                    self.moa_reference_selector.active = false;
                }
                KeyCode::Enter => {
                    self.moa_reference_selector.active = false;
                    match self.moa_reference_selector_mode {
                        MoaReferenceSelectorMode::EditRoster => {
                            let selected: Vec<String> =
                                self.moa_reference_selected.iter().cloned().collect();
                            self.handle_save_moa_reference_selection(selected);
                        }
                        MoaReferenceSelectorMode::AddExpert => {
                            if let Some(model) = self
                                .moa_reference_selector
                                .current()
                                .map(|entry| entry.display.clone())
                            {
                                self.handle_add_moa_reference(model);
                            }
                        }
                        MoaReferenceSelectorMode::RemoveExpert => {
                            if let Some(model) = self
                                .moa_reference_selector
                                .current()
                                .map(|entry| entry.display.clone())
                            {
                                self.handle_remove_moa_reference(model);
                            }
                        }
                    }
                }
                KeyCode::Char(' ')
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && self.moa_reference_selector_mode
                            == MoaReferenceSelectorMode::EditRoster =>
                {
                    if let Some(model) = self
                        .moa_reference_selector
                        .current()
                        .map(|entry| entry.display.clone())
                    {
                        if !self.moa_reference_selected.insert(model.clone()) {
                            self.moa_reference_selected.remove(&model);
                        }
                        self.needs_redraw = true;
                    }
                }
                KeyCode::Tab => self.moa_reference_selector.move_down(),
                KeyCode::BackTab => self.moa_reference_selector.move_up(),
                KeyCode::Up => self.moa_reference_selector.move_up(),
                KeyCode::Down => self.moa_reference_selector.move_down(),
                KeyCode::PageUp => self.moa_reference_selector.page_up(),
                KeyCode::PageDown => self.moa_reference_selector.page_down(),
                KeyCode::Backspace => self.moa_reference_selector.pop_char(),
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.moa_reference_selector.push_char(c);
                }
                _ => {}
            }
            return;
        }

        // Vision-model selector overlay active — same navigation as /model.
        if self.vision_model_selector.active {
            match key.code {
                KeyCode::Esc
                    if !self.close_detail_fullscreen(DetailSurface::VisionModelSelector) =>
                {
                    self.vision_model_selector.active = false;
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::VisionModelSelector,
                        self.split_detail_scroll(DetailSurface::VisionModelSelector),
                    );
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::VisionModelSelector) =>
                {
                    self.toggle_simple_split_focus(DetailSurface::VisionModelSelector);
                }
                KeyCode::Enter => {
                    if let Some(model) = self
                        .vision_model_selector
                        .current()
                        .map(|entry| entry.display.clone())
                    {
                        self.vision_model_selector.active = false;
                        self.close_detail_fullscreen(DetailSurface::VisionModelSelector);
                        self.handle_set_vision_model(model);
                    }
                }
                KeyCode::Up => {
                    if self.simple_split_detail_focused(DetailSurface::VisionModelSelector) {
                        self.scroll_split_detail_lines(DetailSurface::VisionModelSelector, -1);
                    } else {
                        self.vision_model_selector.move_up();
                        self.reset_split_detail_scroll(DetailSurface::VisionModelSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::VisionModelSelector);
                    }
                }
                KeyCode::Down => {
                    if self.simple_split_detail_focused(DetailSurface::VisionModelSelector) {
                        self.scroll_split_detail_lines(DetailSurface::VisionModelSelector, 1);
                    } else {
                        self.vision_model_selector.move_down();
                        self.reset_split_detail_scroll(DetailSurface::VisionModelSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::VisionModelSelector);
                    }
                }
                KeyCode::PageUp
                    if self.detail_fullscreen_active(DetailSurface::VisionModelSelector) =>
                {
                    self.page_up_detail_fullscreen(DetailSurface::VisionModelSelector);
                }
                KeyCode::PageDown
                    if self.detail_fullscreen_active(DetailSurface::VisionModelSelector) =>
                {
                    self.page_down_detail_fullscreen(DetailSurface::VisionModelSelector);
                }
                KeyCode::PageUp => self.apply_simple_selector_paging(
                    DetailSurface::VisionModelSelector,
                    PagingIntent::Up,
                ),
                KeyCode::PageDown => self.apply_simple_selector_paging(
                    DetailSurface::VisionModelSelector,
                    PagingIntent::Down,
                ),
                KeyCode::Home => {
                    if self.simple_split_detail_focused(DetailSurface::VisionModelSelector) {
                        self.set_split_detail_scroll_to_edge(
                            DetailSurface::VisionModelSelector,
                            false,
                        );
                    } else {
                        self.vision_model_selector.selected = 0;
                        self.reset_split_detail_scroll(DetailSurface::VisionModelSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::VisionModelSelector);
                    }
                }
                KeyCode::End => {
                    if self.simple_split_detail_focused(DetailSurface::VisionModelSelector) {
                        self.set_split_detail_scroll_to_edge(
                            DetailSurface::VisionModelSelector,
                            true,
                        );
                    } else {
                        self.vision_model_selector.selected =
                            self.vision_model_selector.filtered.len().saturating_sub(1);
                        self.reset_split_detail_scroll(DetailSurface::VisionModelSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::VisionModelSelector);
                    }
                }
                KeyCode::Backspace => {
                    self.vision_model_selector.pop_char();
                    self.reset_split_detail_scroll(DetailSurface::VisionModelSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::VisionModelSelector);
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.vision_model_selector.push_char(c);
                    self.reset_split_detail_scroll(DetailSurface::VisionModelSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::VisionModelSelector);
                }
                _ => {}
            }
            return;
        }

        if self.image_model_selector.active {
            match key.code {
                KeyCode::Esc
                    if !self.close_detail_fullscreen(DetailSurface::ImageModelSelector) =>
                {
                    self.image_model_selector.active = false;
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::ImageModelSelector,
                        self.split_detail_scroll(DetailSurface::ImageModelSelector),
                    );
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::ImageModelSelector) =>
                {
                    self.toggle_simple_split_focus(DetailSurface::ImageModelSelector);
                }
                KeyCode::Enter => {
                    if let Some(model) = self
                        .image_model_selector
                        .current()
                        .map(|entry| entry.display.clone())
                    {
                        self.image_model_selector.active = false;
                        self.close_detail_fullscreen(DetailSurface::ImageModelSelector);
                        self.handle_set_image_model(model);
                    }
                }
                KeyCode::Up => {
                    if self.simple_split_detail_focused(DetailSurface::ImageModelSelector) {
                        self.scroll_split_detail_lines(DetailSurface::ImageModelSelector, -1);
                    } else {
                        self.image_model_selector.move_up();
                        self.reset_split_detail_scroll(DetailSurface::ImageModelSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ImageModelSelector);
                    }
                }
                KeyCode::Down => {
                    if self.simple_split_detail_focused(DetailSurface::ImageModelSelector) {
                        self.scroll_split_detail_lines(DetailSurface::ImageModelSelector, 1);
                    } else {
                        self.image_model_selector.move_down();
                        self.reset_split_detail_scroll(DetailSurface::ImageModelSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ImageModelSelector);
                    }
                }
                KeyCode::PageUp
                    if self.detail_fullscreen_active(DetailSurface::ImageModelSelector) =>
                {
                    self.page_up_detail_fullscreen(DetailSurface::ImageModelSelector);
                }
                KeyCode::PageDown
                    if self.detail_fullscreen_active(DetailSurface::ImageModelSelector) =>
                {
                    self.page_down_detail_fullscreen(DetailSurface::ImageModelSelector);
                }
                KeyCode::PageUp => self.apply_simple_selector_paging(
                    DetailSurface::ImageModelSelector,
                    PagingIntent::Up,
                ),
                KeyCode::PageDown => self.apply_simple_selector_paging(
                    DetailSurface::ImageModelSelector,
                    PagingIntent::Down,
                ),
                KeyCode::Home => {
                    if self.simple_split_detail_focused(DetailSurface::ImageModelSelector) {
                        self.set_split_detail_scroll_to_edge(
                            DetailSurface::ImageModelSelector,
                            false,
                        );
                    } else {
                        self.image_model_selector.selected = 0;
                        self.reset_split_detail_scroll(DetailSurface::ImageModelSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ImageModelSelector);
                    }
                }
                KeyCode::End => {
                    if self.simple_split_detail_focused(DetailSurface::ImageModelSelector) {
                        self.set_split_detail_scroll_to_edge(
                            DetailSurface::ImageModelSelector,
                            true,
                        );
                    } else {
                        self.image_model_selector.selected =
                            self.image_model_selector.filtered.len().saturating_sub(1);
                        self.reset_split_detail_scroll(DetailSurface::ImageModelSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ImageModelSelector);
                    }
                }
                KeyCode::Backspace => {
                    self.image_model_selector.pop_char();
                    self.reset_split_detail_scroll(DetailSurface::ImageModelSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ImageModelSelector);
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.image_model_selector.push_char(c);
                    self.reset_split_detail_scroll(DetailSurface::ImageModelSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ImageModelSelector);
                }
                _ => {}
            }
            return;
        }

        // MCP selector overlay active — mirrors /model UX while keeping
        // installs controlled. Catalog-only entries open detail view instead.
        if self.mcp_selector.active {
            match key.code {
                KeyCode::Esc if !self.close_detail_fullscreen(DetailSurface::McpSelector) => {
                    self.mcp_selector.active = false;
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::McpSelector,
                        self.split_detail_scroll(DetailSurface::McpSelector),
                    );
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::McpSelector) =>
                {
                    self.toggle_simple_split_focus(DetailSurface::McpSelector);
                }
                _ if selector_action_key(&key, 'r') => {
                    let query = self.mcp_selector.query.clone();
                    self.open_mcp_selector(Some(&query), true);
                }
                KeyCode::Enter => {
                    if let Some(entry) = self.mcp_selector.current() {
                        let command = entry.default_command();
                        self.mcp_selector.active = false;
                        self.close_detail_fullscreen(DetailSurface::McpSelector);
                        self.handle_mcp_command(command);
                    }
                }
                KeyCode::Delete => {
                    if let Some(entry) = self.mcp_selector.current()
                        && let Some(command) = entry.remove_command()
                    {
                        self.mcp_selector.active = false;
                        self.handle_mcp_command(command);
                    }
                }
                KeyCode::Up => {
                    if self.simple_split_detail_focused(DetailSurface::McpSelector) {
                        self.scroll_split_detail_lines(DetailSurface::McpSelector, -1);
                    } else {
                        self.mcp_selector.move_up();
                        self.reset_split_detail_scroll(DetailSurface::McpSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::McpSelector);
                    }
                }
                KeyCode::Down => {
                    if self.simple_split_detail_focused(DetailSurface::McpSelector) {
                        self.scroll_split_detail_lines(DetailSurface::McpSelector, 1);
                    } else {
                        self.mcp_selector.move_down();
                        self.reset_split_detail_scroll(DetailSurface::McpSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::McpSelector);
                    }
                }
                KeyCode::PageUp if self.detail_fullscreen_active(DetailSurface::McpSelector) => {
                    self.page_up_detail_fullscreen(DetailSurface::McpSelector);
                }
                KeyCode::PageDown if self.detail_fullscreen_active(DetailSurface::McpSelector) => {
                    self.page_down_detail_fullscreen(DetailSurface::McpSelector);
                }
                KeyCode::PageUp => {
                    self.apply_simple_selector_paging(DetailSurface::McpSelector, PagingIntent::Up)
                }
                KeyCode::PageDown => self
                    .apply_simple_selector_paging(DetailSurface::McpSelector, PagingIntent::Down),
                KeyCode::Home => {
                    if self.simple_split_detail_focused(DetailSurface::McpSelector) {
                        self.set_split_detail_scroll_to_edge(DetailSurface::McpSelector, false);
                    } else {
                        self.mcp_selector.selected = 0;
                        self.reset_split_detail_scroll(DetailSurface::McpSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::McpSelector);
                    }
                }
                KeyCode::End => {
                    if self.simple_split_detail_focused(DetailSurface::McpSelector) {
                        self.set_split_detail_scroll_to_edge(DetailSurface::McpSelector, true);
                    } else {
                        self.mcp_selector.selected =
                            self.mcp_selector.filtered.len().saturating_sub(1);
                        self.reset_split_detail_scroll(DetailSurface::McpSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::McpSelector);
                    }
                }
                KeyCode::Backspace => {
                    self.mcp_selector.pop_char();
                    self.reset_split_detail_scroll(DetailSurface::McpSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::McpSelector);
                }
                KeyCode::Char(' ') => {
                    if let Some(entry) = self.mcp_selector.current()
                        && let Some(command) = entry.toggle_command()
                    {
                        let query = self.mcp_selector.query.clone();
                        self.handle_mcp_command(command);
                        self.open_mcp_selector(Some(&query), false);
                    }
                }
                _ if selector_action_key(&key, 'v') => {
                    if let Some(entry) = self.mcp_selector.current() {
                        let command = entry.view_command();
                        self.mcp_selector.active = false;
                        self.handle_mcp_command(command);
                    }
                }
                _ if selector_action_key(&key, 'i') => {
                    if let Some(entry) = self.mcp_selector.current()
                        && let Some(command) = entry.install_command()
                    {
                        self.mcp_selector.active = false;
                        self.handle_mcp_command(command);
                    }
                }
                _ if selector_action_key(&key, 't') => {
                    if let Some(entry) = self.mcp_selector.current()
                        && entry.kind == McpEntryKind::ConfiguredServer
                    {
                        let command = entry.test_command();
                        self.mcp_selector.active = false;
                        self.handle_mcp_command(command);
                    }
                }
                _ if selector_action_key(&key, 'c') => {
                    if let Some(entry) = self.mcp_selector.current()
                        && entry.kind == McpEntryKind::ConfiguredServer
                    {
                        let command = entry.doctor_command();
                        self.mcp_selector.active = false;
                        self.handle_mcp_command(command);
                    }
                }
                _ if selector_action_key(&key, 'd') => {
                    if let Some(entry) = self.mcp_selector.current()
                        && let Some(command) = entry.remove_command()
                    {
                        self.mcp_selector.active = false;
                        self.handle_mcp_command(command);
                    }
                }
                KeyCode::Char(c) if selector_search_char(&key) == Some(c) => {
                    self.mcp_selector.push_char(c);
                    self.reset_split_detail_scroll(DetailSurface::McpSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::McpSelector);
                }
                _ => {}
            }
            return;
        }

        if self.remote_mcp_browser.selector.active {
            match key.code {
                KeyCode::Esc if !self.close_detail_fullscreen(DetailSurface::RemoteMcpBrowser) => {
                    self.remote_mcp_browser.selector.active = false;
                    self.needs_redraw = true;
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::RemoteMcpBrowser,
                        self.split_detail_scroll(DetailSurface::RemoteMcpBrowser),
                    );
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::RemoteMcpBrowser) =>
                {
                    self.toggle_simple_split_focus(DetailSurface::RemoteMcpBrowser);
                }
                KeyCode::Enter => {
                    if let Some(entry) = self.remote_mcp_browser.selector.current().cloned() {
                        match entry.action() {
                            RemoteMcpAction::Install => self.install_remote_mcp_entry(&entry),
                            RemoteMcpAction::View => self.view_remote_mcp_entry(&entry),
                        }
                    }
                }
                _ if selector_action_key(&key, 'i') => {
                    if let Some(entry) = self.remote_mcp_browser.selector.current().cloned() {
                        if entry.install.is_some() {
                            self.install_remote_mcp_entry(&entry);
                        } else {
                            self.push_output(
                                "This remote MCP entry is view-only. Use Enter or V to inspect it.",
                                OutputRole::System,
                            );
                        }
                    }
                }
                _ if selector_action_key(&key, 'v') => {
                    if let Some(entry) = self.remote_mcp_browser.selector.current().cloned() {
                        self.view_remote_mcp_entry(&entry);
                    }
                }
                _ if selector_action_key(&key, 'r') => {
                    self.schedule_remote_mcp_search(true);
                }
                _ if selector_action_key(&key, 'l') => {
                    self.remote_mcp_browser.selector.active = false;
                    self.close_detail_fullscreen(DetailSurface::RemoteMcpBrowser);
                    self.open_mcp_selector(None, false);
                }
                KeyCode::Up => {
                    if self.simple_split_detail_focused(DetailSurface::RemoteMcpBrowser) {
                        self.scroll_split_detail_lines(DetailSurface::RemoteMcpBrowser, -1);
                    } else {
                        self.remote_mcp_browser.selector.move_up();
                        self.reset_split_detail_scroll(DetailSurface::RemoteMcpBrowser);
                        self.reset_detail_fullscreen_scroll(DetailSurface::RemoteMcpBrowser);
                    }
                }
                KeyCode::Down => {
                    if self.simple_split_detail_focused(DetailSurface::RemoteMcpBrowser) {
                        self.scroll_split_detail_lines(DetailSurface::RemoteMcpBrowser, 1);
                    } else {
                        self.remote_mcp_browser.selector.move_down();
                        self.reset_split_detail_scroll(DetailSurface::RemoteMcpBrowser);
                        self.reset_detail_fullscreen_scroll(DetailSurface::RemoteMcpBrowser);
                    }
                }
                KeyCode::PageUp
                    if self.detail_fullscreen_active(DetailSurface::RemoteMcpBrowser) =>
                {
                    self.page_up_detail_fullscreen(DetailSurface::RemoteMcpBrowser);
                }
                KeyCode::PageDown
                    if self.detail_fullscreen_active(DetailSurface::RemoteMcpBrowser) =>
                {
                    self.page_down_detail_fullscreen(DetailSurface::RemoteMcpBrowser);
                }
                KeyCode::PageUp => self.apply_simple_selector_paging(
                    DetailSurface::RemoteMcpBrowser,
                    PagingIntent::Up,
                ),
                KeyCode::PageDown => self.apply_simple_selector_paging(
                    DetailSurface::RemoteMcpBrowser,
                    PagingIntent::Down,
                ),
                KeyCode::Home => {
                    if self.simple_split_detail_focused(DetailSurface::RemoteMcpBrowser) {
                        self.set_split_detail_scroll_to_edge(
                            DetailSurface::RemoteMcpBrowser,
                            false,
                        );
                    } else {
                        self.remote_mcp_browser.selector.selected = 0;
                        self.reset_split_detail_scroll(DetailSurface::RemoteMcpBrowser);
                        self.reset_detail_fullscreen_scroll(DetailSurface::RemoteMcpBrowser);
                    }
                }
                KeyCode::End => {
                    if self.simple_split_detail_focused(DetailSurface::RemoteMcpBrowser) {
                        self.set_split_detail_scroll_to_edge(DetailSurface::RemoteMcpBrowser, true);
                    } else {
                        self.remote_mcp_browser.selector.selected = self
                            .remote_mcp_browser
                            .selector
                            .filtered
                            .len()
                            .saturating_sub(1);
                        self.reset_split_detail_scroll(DetailSurface::RemoteMcpBrowser);
                        self.reset_detail_fullscreen_scroll(DetailSurface::RemoteMcpBrowser);
                    }
                }
                KeyCode::Backspace => {
                    self.remote_mcp_browser.selector.pop_char();
                    self.reset_split_detail_scroll(DetailSurface::RemoteMcpBrowser);
                    self.reset_detail_fullscreen_scroll(DetailSurface::RemoteMcpBrowser);
                    self.schedule_remote_mcp_search(false);
                }
                KeyCode::Char(c) if selector_search_char(&key) == Some(c) => {
                    self.remote_mcp_browser.selector.push_char(c);
                    self.reset_split_detail_scroll(DetailSurface::RemoteMcpBrowser);
                    self.reset_detail_fullscreen_scroll(DetailSurface::RemoteMcpBrowser);
                    self.schedule_remote_mcp_search(false);
                }
                _ => {}
            }
            return;
        }

        // Skill guard trust overlay (full screen — before remote browser)
        if self.skill_trust_prompt.is_some() {
            self.handle_skill_trust_key(key);
            return;
        }

        if self.remote_skill_browser.selector.active {
            match key.code {
                KeyCode::Esc
                    if !self.close_detail_fullscreen(DetailSurface::RemoteSkillBrowser) =>
                {
                    self.remote_skill_browser.selector.active = false;
                    self.remote_skill_browser.action_in_flight = None;
                    self.clear_remote_skill_guard_cache();
                    self.needs_redraw = true;
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::RemoteSkillBrowser,
                        self.split_detail_scroll(DetailSurface::RemoteSkillBrowser),
                    );
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::RemoteSkillBrowser) =>
                {
                    self.toggle_simple_split_focus(DetailSurface::RemoteSkillBrowser);
                }
                KeyCode::Enter => {
                    if let Some(entry) = self.remote_skill_browser.selector.current().cloned() {
                        self.run_remote_skill_action(entry);
                    }
                }
                _ if selector_action_key(&key, 'i') => {
                    if let Some(entry) = self.remote_skill_browser.selector.current().cloned() {
                        self.run_remote_skill_action(entry);
                    }
                }
                _ if selector_action_key(&key, 'u') => {
                    if let Some(mut entry) = self.remote_skill_browser.selector.current().cloned() {
                        if entry.installed_name.is_some() {
                            entry.action = RemoteSkillAction::Update;
                            self.run_remote_skill_action(entry);
                        } else {
                            self.push_output(
                                "This remote skill is not hub-installed yet. Use Enter or I to install it.",
                                OutputRole::System,
                            );
                        }
                    }
                }
                _ if selector_action_key(&key, 'r') => {
                    self.schedule_remote_skill_search(true);
                }
                _ if selector_action_key(&key, 'l') => {
                    self.remote_skill_browser.selector.active = false;
                    self.close_detail_fullscreen(DetailSurface::RemoteSkillBrowser);
                    self.refresh_skills_list();
                    self.open_skill_selector();
                    self.needs_redraw = true;
                }
                _ if selector_action_key(&key, 's') => {
                    self.clear_remote_skill_guard_cache();
                    self.schedule_remote_skill_guard_preview();
                }
                KeyCode::Up => {
                    if self.simple_split_detail_focused(DetailSurface::RemoteSkillBrowser) {
                        self.scroll_split_detail_lines(DetailSurface::RemoteSkillBrowser, -1);
                    } else {
                        self.remote_skill_browser.selector.move_up();
                        self.reset_split_detail_scroll(DetailSurface::RemoteSkillBrowser);
                        self.reset_detail_fullscreen_scroll(DetailSurface::RemoteSkillBrowser);
                        self.schedule_remote_skill_guard_preview();
                    }
                }
                KeyCode::Down => {
                    if self.simple_split_detail_focused(DetailSurface::RemoteSkillBrowser) {
                        self.scroll_split_detail_lines(DetailSurface::RemoteSkillBrowser, 1);
                    } else {
                        self.remote_skill_browser.selector.move_down();
                        self.reset_split_detail_scroll(DetailSurface::RemoteSkillBrowser);
                        self.reset_detail_fullscreen_scroll(DetailSurface::RemoteSkillBrowser);
                        self.schedule_remote_skill_guard_preview();
                    }
                }
                KeyCode::PageUp
                    if self.detail_fullscreen_active(DetailSurface::RemoteSkillBrowser) =>
                {
                    self.page_up_detail_fullscreen(DetailSurface::RemoteSkillBrowser);
                }
                KeyCode::PageDown
                    if self.detail_fullscreen_active(DetailSurface::RemoteSkillBrowser) =>
                {
                    self.page_down_detail_fullscreen(DetailSurface::RemoteSkillBrowser);
                }
                KeyCode::PageUp => self.apply_simple_selector_paging(
                    DetailSurface::RemoteSkillBrowser,
                    PagingIntent::Up,
                ),
                KeyCode::PageDown => self.apply_simple_selector_paging(
                    DetailSurface::RemoteSkillBrowser,
                    PagingIntent::Down,
                ),
                KeyCode::Home => {
                    if self.simple_split_detail_focused(DetailSurface::RemoteSkillBrowser) {
                        self.set_split_detail_scroll_to_edge(
                            DetailSurface::RemoteSkillBrowser,
                            false,
                        );
                    } else {
                        self.remote_skill_browser.selector.selected = 0;
                        self.reset_split_detail_scroll(DetailSurface::RemoteSkillBrowser);
                        self.reset_detail_fullscreen_scroll(DetailSurface::RemoteSkillBrowser);
                        self.schedule_remote_skill_guard_preview();
                    }
                }
                KeyCode::End => {
                    if self.simple_split_detail_focused(DetailSurface::RemoteSkillBrowser) {
                        self.set_split_detail_scroll_to_edge(
                            DetailSurface::RemoteSkillBrowser,
                            true,
                        );
                    } else {
                        self.remote_skill_browser.selector.selected = self
                            .remote_skill_browser
                            .selector
                            .filtered
                            .len()
                            .saturating_sub(1);
                        self.reset_split_detail_scroll(DetailSurface::RemoteSkillBrowser);
                        self.reset_detail_fullscreen_scroll(DetailSurface::RemoteSkillBrowser);
                        self.schedule_remote_skill_guard_preview();
                    }
                }
                KeyCode::Backspace => {
                    self.remote_skill_browser.selector.pop_char();
                    self.reset_split_detail_scroll(DetailSurface::RemoteSkillBrowser);
                    self.reset_detail_fullscreen_scroll(DetailSurface::RemoteSkillBrowser);
                    self.schedule_remote_skill_search(false);
                }
                KeyCode::Char(c) if selector_search_char(&key) == Some(c) => {
                    self.remote_skill_browser.selector.push_char(c);
                    self.reset_split_detail_scroll(DetailSurface::RemoteSkillBrowser);
                    self.reset_detail_fullscreen_scroll(DetailSurface::RemoteSkillBrowser);
                    self.schedule_remote_skill_search(false);
                }
                _ => {}
            }
            return;
        }

        if self.remote_plugin_browser.selector.active {
            match key.code {
                KeyCode::Esc
                    if !self.close_detail_fullscreen(DetailSurface::RemotePluginBrowser) =>
                {
                    self.remote_plugin_browser.selector.active = false;
                    self.remote_plugin_browser.action_in_flight = None;
                    self.needs_redraw = true;
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::RemotePluginBrowser,
                        self.split_detail_scroll(DetailSurface::RemotePluginBrowser),
                    );
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::RemotePluginBrowser) =>
                {
                    self.toggle_simple_split_focus(DetailSurface::RemotePluginBrowser);
                }
                KeyCode::Enter => {
                    if let Some(entry) = self.remote_plugin_browser.selector.current().cloned() {
                        self.run_remote_plugin_action(entry);
                    }
                }
                _ if selector_action_key(&key, 'i') => {
                    if let Some(entry) = self.remote_plugin_browser.selector.current().cloned() {
                        self.run_remote_plugin_action(entry);
                    }
                }
                _ if selector_action_key(&key, 'u') => {
                    if let Some(mut entry) = self.remote_plugin_browser.selector.current().cloned()
                    {
                        if entry.installed_name.is_some() {
                            entry.action = RemotePluginAction::Update;
                            self.run_remote_plugin_action(entry);
                        } else {
                            self.push_output(
                                "This remote plugin is not hub-installed yet. Use Enter or I to install it.",
                                OutputRole::System,
                            );
                        }
                    }
                }
                _ if selector_action_key(&key, 'r') => {
                    self.schedule_remote_plugin_search(true);
                }
                _ if selector_action_key(&key, 'l') => {
                    let scope = self.plugin_toggle_scope.clone();
                    self.remote_plugin_browser.selector.active = false;
                    self.close_detail_fullscreen(DetailSurface::RemotePluginBrowser);
                    self.open_plugin_toggle(scope);
                    self.needs_redraw = true;
                }
                KeyCode::Up => {
                    if self.simple_split_detail_focused(DetailSurface::RemotePluginBrowser) {
                        self.scroll_split_detail_lines(DetailSurface::RemotePluginBrowser, -1);
                    } else {
                        self.remote_plugin_browser.selector.move_up();
                        self.reset_split_detail_scroll(DetailSurface::RemotePluginBrowser);
                        self.reset_detail_fullscreen_scroll(DetailSurface::RemotePluginBrowser);
                    }
                }
                KeyCode::Down => {
                    if self.simple_split_detail_focused(DetailSurface::RemotePluginBrowser) {
                        self.scroll_split_detail_lines(DetailSurface::RemotePluginBrowser, 1);
                    } else {
                        self.remote_plugin_browser.selector.move_down();
                        self.reset_split_detail_scroll(DetailSurface::RemotePluginBrowser);
                        self.reset_detail_fullscreen_scroll(DetailSurface::RemotePluginBrowser);
                    }
                }
                KeyCode::PageUp
                    if self.detail_fullscreen_active(DetailSurface::RemotePluginBrowser) =>
                {
                    self.page_up_detail_fullscreen(DetailSurface::RemotePluginBrowser);
                }
                KeyCode::PageDown
                    if self.detail_fullscreen_active(DetailSurface::RemotePluginBrowser) =>
                {
                    self.page_down_detail_fullscreen(DetailSurface::RemotePluginBrowser);
                }
                KeyCode::PageUp => self.apply_simple_selector_paging(
                    DetailSurface::RemotePluginBrowser,
                    PagingIntent::Up,
                ),
                KeyCode::PageDown => self.apply_simple_selector_paging(
                    DetailSurface::RemotePluginBrowser,
                    PagingIntent::Down,
                ),
                KeyCode::Home => {
                    if self.simple_split_detail_focused(DetailSurface::RemotePluginBrowser) {
                        self.set_split_detail_scroll_to_edge(
                            DetailSurface::RemotePluginBrowser,
                            false,
                        );
                    } else {
                        self.remote_plugin_browser.selector.selected = 0;
                        self.reset_split_detail_scroll(DetailSurface::RemotePluginBrowser);
                        self.reset_detail_fullscreen_scroll(DetailSurface::RemotePluginBrowser);
                    }
                }
                KeyCode::End => {
                    if self.simple_split_detail_focused(DetailSurface::RemotePluginBrowser) {
                        self.set_split_detail_scroll_to_edge(
                            DetailSurface::RemotePluginBrowser,
                            true,
                        );
                    } else {
                        self.remote_plugin_browser.selector.selected = self
                            .remote_plugin_browser
                            .selector
                            .filtered
                            .len()
                            .saturating_sub(1);
                        self.reset_split_detail_scroll(DetailSurface::RemotePluginBrowser);
                        self.reset_detail_fullscreen_scroll(DetailSurface::RemotePluginBrowser);
                    }
                }
                KeyCode::Backspace => {
                    self.remote_plugin_browser.selector.pop_char();
                    self.reset_split_detail_scroll(DetailSurface::RemotePluginBrowser);
                    self.reset_detail_fullscreen_scroll(DetailSurface::RemotePluginBrowser);
                    self.schedule_remote_plugin_search(false);
                }
                KeyCode::Char(c) if selector_search_char(&key) == Some(c) => {
                    self.remote_plugin_browser.selector.push_char(c);
                    self.reset_split_detail_scroll(DetailSurface::RemotePluginBrowser);
                    self.reset_detail_fullscreen_scroll(DetailSurface::RemotePluginBrowser);
                    self.schedule_remote_plugin_search(false);
                }
                _ => {}
            }
            return;
        }

        if self.profile_selector.active {
            match key.code {
                KeyCode::Esc if !self.close_detail_fullscreen(DetailSurface::ProfileSelector) => {
                    self.profile_selector.active = false;
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::ProfileSelector,
                        self.split_detail_scroll(DetailSurface::ProfileSelector),
                    );
                }
                KeyCode::Enter => {
                    if let Some(name) = self
                        .profile_selector
                        .current()
                        .map(|entry| entry.name.clone())
                    {
                        self.handle_profile_switch(name);
                    }
                }
                _ if selector_action_key(&key, 'v') => {
                    self.set_profile_detail_mode(ProfileDetailMode::Summary);
                }
                _ if selector_action_key(&key, 'c') => {
                    self.set_profile_detail_mode(ProfileDetailMode::Config);
                }
                _ if selector_action_key(&key, 's') => {
                    self.set_profile_detail_mode(ProfileDetailMode::Soul);
                }
                _ if selector_action_key(&key, 'm') => {
                    self.set_profile_detail_mode(ProfileDetailMode::Memory);
                }
                _ if selector_action_key(&key, 't') => {
                    self.set_profile_detail_mode(ProfileDetailMode::Tools);
                }
                _ if selector_action_key(&key, 'h') || matches!(key.code, KeyCode::Char('?')) => {
                    self.set_profile_detail_mode(ProfileDetailMode::Help);
                }
                _ if selector_action_key(&key, 'a') => {
                    if let Some(name) = self
                        .profile_selector
                        .current()
                        .map(|entry| entry.name.clone())
                    {
                        self.open_profile_alias_editor(name);
                    }
                }
                _ if selector_action_key(&key, 'e') => {
                    if let Some(name) = self
                        .profile_selector
                        .current()
                        .map(|entry| entry.name.clone())
                    {
                        self.open_profile_export_editor(name);
                    }
                }
                _ if selector_action_key(&key, 'd') || selector_action_key(&key, 'x') => {
                    if let Some(name) = self
                        .profile_selector
                        .current()
                        .map(|entry| entry.name.clone())
                    {
                        self.open_profile_delete_editor(name);
                    }
                }
                _ if selector_action_key(&key, 'n') => {
                    self.open_profile_create_editor();
                }
                _ if selector_action_key(&key, 'i') => {
                    self.open_profile_import_editor();
                }
                _ if selector_action_key(&key, 'o') => {
                    if let Some(name) = self
                        .profile_selector
                        .current()
                        .map(|entry| entry.name.clone())
                    {
                        self.open_profile_rename_editor(name);
                    }
                }
                KeyCode::Home => {
                    self.profile_selector.selected = 0;
                    self.reset_split_detail_scroll(DetailSurface::ProfileSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ProfileSelector);
                }
                KeyCode::End => {
                    self.profile_selector.selected =
                        self.profile_selector.filtered.len().saturating_sub(1);
                    self.reset_split_detail_scroll(DetailSurface::ProfileSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ProfileSelector);
                }
                KeyCode::Left | KeyCode::BackTab => {
                    self.cycle_profile_detail_mode(false);
                }
                KeyCode::Right | KeyCode::Tab => {
                    self.cycle_profile_detail_mode(true);
                }
                KeyCode::Up => {
                    self.profile_selector.move_up();
                    self.reset_split_detail_scroll(DetailSurface::ProfileSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ProfileSelector);
                }
                KeyCode::Down => {
                    self.profile_selector.move_down();
                    self.reset_split_detail_scroll(DetailSurface::ProfileSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ProfileSelector);
                }
                KeyCode::PageUp
                    if self.detail_fullscreen_active(DetailSurface::ProfileSelector) =>
                {
                    self.page_up_detail_fullscreen(DetailSurface::ProfileSelector);
                }
                KeyCode::PageDown
                    if self.detail_fullscreen_active(DetailSurface::ProfileSelector) =>
                {
                    self.page_down_detail_fullscreen(DetailSurface::ProfileSelector);
                }
                KeyCode::PageUp => self
                    .apply_simple_selector_paging(DetailSurface::ProfileSelector, PagingIntent::Up),
                KeyCode::PageDown => self.apply_simple_selector_paging(
                    DetailSurface::ProfileSelector,
                    PagingIntent::Down,
                ),
                _ if selector_action_key(&key, 'r') => {
                    self.refresh_profiles_list();
                }
                KeyCode::Backspace => {
                    self.profile_selector.pop_char();
                    self.reset_split_detail_scroll(DetailSurface::ProfileSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ProfileSelector);
                }
                KeyCode::Char(c) if selector_search_char(&key) == Some(c) => {
                    self.profile_selector.push_char(c);
                    self.reset_split_detail_scroll(DetailSurface::ProfileSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ProfileSelector);
                }
                _ => {}
            }
            return;
        }

        // Skill selector overlay active — same key scheme as model selector
        if self.skill_selector.active {
            match key.code {
                KeyCode::Esc if !self.close_detail_fullscreen(DetailSurface::SkillSelector) => {
                    self.skill_selector.active = false;
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::SkillSelector,
                        self.split_detail_scroll(DetailSurface::SkillSelector),
                    );
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::SkillSelector) =>
                {
                    self.toggle_simple_split_focus(DetailSurface::SkillSelector);
                }
                KeyCode::Char(' ') => {
                    if let Some(name) = self
                        .skill_selector
                        .current()
                        .map(|entry| entry.name.clone())
                    {
                        self.set_skill_activation(
                            &name,
                            !self.active_skills.iter().any(|skill| skill == &name),
                        );
                    }
                }
                KeyCode::Enter => {
                    if let Some(entry) = self.skill_selector.current() {
                        let skill_name = format!("/{} ", entry.name);
                        self.skill_selector.active = false;
                        self.close_detail_fullscreen(DetailSurface::SkillSelector);
                        self.textarea_set_text(&skill_name);
                        self.needs_redraw = true;
                    }
                }
                KeyCode::Up => {
                    if self.simple_split_detail_focused(DetailSurface::SkillSelector) {
                        self.scroll_split_detail_lines(DetailSurface::SkillSelector, -1);
                    } else {
                        self.skill_selector.move_up();
                        self.reset_split_detail_scroll(DetailSurface::SkillSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::SkillSelector);
                    }
                }
                KeyCode::Down => {
                    if self.simple_split_detail_focused(DetailSurface::SkillSelector) {
                        self.scroll_split_detail_lines(DetailSurface::SkillSelector, 1);
                    } else {
                        self.skill_selector.move_down();
                        self.reset_split_detail_scroll(DetailSurface::SkillSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::SkillSelector);
                    }
                }
                KeyCode::PageUp if self.detail_fullscreen_active(DetailSurface::SkillSelector) => {
                    self.page_up_detail_fullscreen(DetailSurface::SkillSelector);
                }
                KeyCode::PageDown
                    if self.detail_fullscreen_active(DetailSurface::SkillSelector) =>
                {
                    self.page_down_detail_fullscreen(DetailSurface::SkillSelector);
                }
                KeyCode::PageUp => self
                    .apply_simple_selector_paging(DetailSurface::SkillSelector, PagingIntent::Up),
                KeyCode::PageDown => self
                    .apply_simple_selector_paging(DetailSurface::SkillSelector, PagingIntent::Down),
                KeyCode::Home => {
                    if self.simple_split_detail_focused(DetailSurface::SkillSelector) {
                        self.set_split_detail_scroll_to_edge(DetailSurface::SkillSelector, false);
                    } else {
                        self.skill_selector.selected = 0;
                        self.reset_split_detail_scroll(DetailSurface::SkillSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::SkillSelector);
                    }
                }
                KeyCode::End => {
                    if self.simple_split_detail_focused(DetailSurface::SkillSelector) {
                        self.set_split_detail_scroll_to_edge(DetailSurface::SkillSelector, true);
                    } else {
                        self.skill_selector.selected =
                            self.skill_selector.filtered.len().saturating_sub(1);
                        self.reset_split_detail_scroll(DetailSurface::SkillSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::SkillSelector);
                    }
                }
                _ if selector_action_key(&key, 'r') => {
                    self.open_remote_skill_selector(None);
                }
                KeyCode::Backspace => {
                    self.skill_selector.pop_char();
                    self.reset_split_detail_scroll(DetailSurface::SkillSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::SkillSelector);
                }
                KeyCode::Char(c) if selector_search_char(&key) == Some(c) => {
                    self.skill_selector.push_char(c);
                    self.reset_split_detail_scroll(DetailSurface::SkillSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::SkillSelector);
                }
                _ => {}
            }
            return;
        }

        if self.tool_manager.active {
            match key.code {
                KeyCode::Esc if !self.close_detail_fullscreen(DetailSurface::ToolManager) => {
                    self.tool_manager.active = false;
                }
                KeyCode::Left => {
                    self.tool_manager_scope = self.tool_manager_scope.previous();
                    let _ = self.refresh_tool_manager_entries();
                    self.reset_split_detail_scroll(DetailSurface::ToolManager);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ToolManager);
                }
                KeyCode::Right => {
                    self.tool_manager_scope = self.tool_manager_scope.next();
                    let _ = self.refresh_tool_manager_entries();
                    self.reset_split_detail_scroll(DetailSurface::ToolManager);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ToolManager);
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::ToolManager) =>
                {
                    self.toggle_simple_split_focus(DetailSurface::ToolManager);
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::ToolManager,
                        self.split_detail_scroll(DetailSurface::ToolManager),
                    );
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    self.toggle_tool_manager_selected();
                }
                KeyCode::Up => {
                    if self.simple_split_detail_focused(DetailSurface::ToolManager) {
                        self.scroll_split_detail_lines(DetailSurface::ToolManager, -1);
                    } else {
                        self.tool_manager.move_up();
                        self.reset_split_detail_scroll(DetailSurface::ToolManager);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ToolManager);
                    }
                }
                KeyCode::Down => {
                    if self.simple_split_detail_focused(DetailSurface::ToolManager) {
                        self.scroll_split_detail_lines(DetailSurface::ToolManager, 1);
                    } else {
                        self.tool_manager.move_down();
                        self.reset_split_detail_scroll(DetailSurface::ToolManager);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ToolManager);
                    }
                }
                KeyCode::PageUp if self.detail_fullscreen_active(DetailSurface::ToolManager) => {
                    self.page_up_detail_fullscreen(DetailSurface::ToolManager);
                }
                KeyCode::PageDown if self.detail_fullscreen_active(DetailSurface::ToolManager) => {
                    self.page_down_detail_fullscreen(DetailSurface::ToolManager);
                }
                KeyCode::PageUp => {
                    self.apply_simple_selector_paging(DetailSurface::ToolManager, PagingIntent::Up)
                }
                KeyCode::PageDown => self
                    .apply_simple_selector_paging(DetailSurface::ToolManager, PagingIntent::Down),
                KeyCode::Home => {
                    if self.simple_split_detail_focused(DetailSurface::ToolManager) {
                        self.set_split_detail_scroll_to_edge(DetailSurface::ToolManager, false);
                    } else {
                        self.tool_manager.selected = 0;
                        self.reset_split_detail_scroll(DetailSurface::ToolManager);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ToolManager);
                    }
                }
                KeyCode::End => {
                    if self.simple_split_detail_focused(DetailSurface::ToolManager) {
                        self.set_split_detail_scroll_to_edge(DetailSurface::ToolManager, true);
                    } else {
                        self.tool_manager.selected =
                            self.tool_manager.filtered.len().saturating_sub(1);
                        self.reset_split_detail_scroll(DetailSurface::ToolManager);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ToolManager);
                    }
                }
                _ if selector_action_key(&key, 'r') => {
                    self.reset_tool_manager_policy();
                }
                KeyCode::Backspace => {
                    self.tool_manager.pop_char();
                    self.reset_split_detail_scroll(DetailSurface::ToolManager);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ToolManager);
                }
                KeyCode::Char(c) if selector_search_char(&key) == Some(c) => {
                    self.tool_manager.push_char(c);
                    self.reset_split_detail_scroll(DetailSurface::ToolManager);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ToolManager);
                }
                _ => {}
            }
            return;
        }

        if self.plugin_toggle.active {
            match key.code {
                KeyCode::Esc if !self.close_detail_fullscreen(DetailSurface::PluginToggle) => {
                    self.plugin_toggle.active = false;
                }
                KeyCode::Left => {
                    self.plugin_toggle_scope = self.previous_plugin_toggle_scope();
                    let _ = self.refresh_plugin_toggle_entries();
                    self.reset_split_detail_scroll(DetailSurface::PluginToggle);
                    self.reset_detail_fullscreen_scroll(DetailSurface::PluginToggle);
                }
                KeyCode::Right => {
                    self.plugin_toggle_scope = self.next_plugin_toggle_scope();
                    let _ = self.refresh_plugin_toggle_entries();
                    self.reset_split_detail_scroll(DetailSurface::PluginToggle);
                    self.reset_detail_fullscreen_scroll(DetailSurface::PluginToggle);
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::PluginToggle) =>
                {
                    self.toggle_simple_split_focus(DetailSurface::PluginToggle);
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::PluginToggle,
                        self.split_detail_scroll(DetailSurface::PluginToggle),
                    );
                }
                KeyCode::Enter => {
                    self.confirm_plugin_toggle();
                }
                KeyCode::Char(' ') => {
                    self.toggle_plugin_selected();
                }
                KeyCode::Up => {
                    if self.simple_split_detail_focused(DetailSurface::PluginToggle) {
                        self.scroll_split_detail_lines(DetailSurface::PluginToggle, -1);
                    } else {
                        self.plugin_toggle.move_up();
                        self.reset_split_detail_scroll(DetailSurface::PluginToggle);
                        self.reset_detail_fullscreen_scroll(DetailSurface::PluginToggle);
                    }
                }
                KeyCode::Down => {
                    if self.simple_split_detail_focused(DetailSurface::PluginToggle) {
                        self.scroll_split_detail_lines(DetailSurface::PluginToggle, 1);
                    } else {
                        self.plugin_toggle.move_down();
                        self.reset_split_detail_scroll(DetailSurface::PluginToggle);
                        self.reset_detail_fullscreen_scroll(DetailSurface::PluginToggle);
                    }
                }
                KeyCode::PageUp if self.detail_fullscreen_active(DetailSurface::PluginToggle) => {
                    self.page_up_detail_fullscreen(DetailSurface::PluginToggle);
                }
                KeyCode::PageDown if self.detail_fullscreen_active(DetailSurface::PluginToggle) => {
                    self.page_down_detail_fullscreen(DetailSurface::PluginToggle);
                }
                KeyCode::PageUp => {
                    self.apply_simple_selector_paging(DetailSurface::PluginToggle, PagingIntent::Up)
                }
                KeyCode::PageDown => self
                    .apply_simple_selector_paging(DetailSurface::PluginToggle, PagingIntent::Down),
                KeyCode::Home => {
                    if self.simple_split_detail_focused(DetailSurface::PluginToggle) {
                        self.set_split_detail_scroll_to_edge(DetailSurface::PluginToggle, false);
                    } else {
                        self.plugin_toggle.selected = 0;
                        self.reset_split_detail_scroll(DetailSurface::PluginToggle);
                        self.reset_detail_fullscreen_scroll(DetailSurface::PluginToggle);
                    }
                }
                KeyCode::End => {
                    if self.simple_split_detail_focused(DetailSurface::PluginToggle) {
                        self.set_split_detail_scroll_to_edge(DetailSurface::PluginToggle, true);
                    } else {
                        self.plugin_toggle.selected =
                            self.plugin_toggle.filtered.len().saturating_sub(1);
                        self.reset_split_detail_scroll(DetailSurface::PluginToggle);
                        self.reset_detail_fullscreen_scroll(DetailSurface::PluginToggle);
                    }
                }
                _ if selector_action_key(&key, 'r') => {
                    self.open_remote_plugin_selector(None, None);
                }
                KeyCode::Backspace => {
                    self.plugin_toggle.pop_char();
                    self.reset_split_detail_scroll(DetailSurface::PluginToggle);
                    self.reset_detail_fullscreen_scroll(DetailSurface::PluginToggle);
                }
                KeyCode::Char(c) if selector_search_char(&key) == Some(c) => {
                    self.plugin_toggle.push_char(c);
                    self.reset_split_detail_scroll(DetailSurface::PluginToggle);
                    self.reset_detail_fullscreen_scroll(DetailSurface::PluginToggle);
                }
                _ => {}
            }
            return;
        }

        if self.config_selector.active {
            match key.code {
                KeyCode::Esc if !self.close_detail_fullscreen(DetailSurface::ConfigSelector) => {
                    self.config_selector.active = false;
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::ConfigSelector,
                        self.split_detail_scroll(DetailSurface::ConfigSelector),
                    );
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::ConfigSelector) =>
                {
                    self.toggle_simple_split_focus(DetailSurface::ConfigSelector);
                }
                KeyCode::Enter => {
                    if let Some(entry) = self.config_selector.current() {
                        let action = entry.action;
                        self.config_selector.active = false;
                        self.close_detail_fullscreen(DetailSurface::ConfigSelector);
                        self.handle_config_selector_action(action);
                    }
                }
                KeyCode::Up => {
                    if self.simple_split_detail_focused(DetailSurface::ConfigSelector) {
                        self.scroll_split_detail_lines(DetailSurface::ConfigSelector, -1);
                    } else {
                        self.config_selector.move_up();
                        self.reset_split_detail_scroll(DetailSurface::ConfigSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ConfigSelector);
                    }
                }
                KeyCode::Down => {
                    if self.simple_split_detail_focused(DetailSurface::ConfigSelector) {
                        self.scroll_split_detail_lines(DetailSurface::ConfigSelector, 1);
                    } else {
                        self.config_selector.move_down();
                        self.reset_split_detail_scroll(DetailSurface::ConfigSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ConfigSelector);
                    }
                }
                KeyCode::PageUp if self.detail_fullscreen_active(DetailSurface::ConfigSelector) => {
                    self.page_up_detail_fullscreen(DetailSurface::ConfigSelector);
                }
                KeyCode::PageDown
                    if self.detail_fullscreen_active(DetailSurface::ConfigSelector) =>
                {
                    self.page_down_detail_fullscreen(DetailSurface::ConfigSelector);
                }
                KeyCode::PageUp => self
                    .apply_simple_selector_paging(DetailSurface::ConfigSelector, PagingIntent::Up),
                KeyCode::PageDown => self.apply_simple_selector_paging(
                    DetailSurface::ConfigSelector,
                    PagingIntent::Down,
                ),
                KeyCode::Home => {
                    if self.simple_split_detail_focused(DetailSurface::ConfigSelector) {
                        self.set_split_detail_scroll_to_edge(DetailSurface::ConfigSelector, false);
                    } else {
                        self.config_selector.selected = 0;
                        self.reset_split_detail_scroll(DetailSurface::ConfigSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ConfigSelector);
                    }
                }
                KeyCode::End => {
                    if self.simple_split_detail_focused(DetailSurface::ConfigSelector) {
                        self.set_split_detail_scroll_to_edge(DetailSurface::ConfigSelector, true);
                    } else {
                        self.config_selector.selected =
                            self.config_selector.filtered.len().saturating_sub(1);
                        self.reset_split_detail_scroll(DetailSurface::ConfigSelector);
                        self.reset_detail_fullscreen_scroll(DetailSurface::ConfigSelector);
                    }
                }
                KeyCode::Backspace => {
                    self.config_selector.pop_char();
                    self.reset_split_detail_scroll(DetailSurface::ConfigSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ConfigSelector);
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.config_selector.push_char(c);
                    self.reset_split_detail_scroll(DetailSurface::ConfigSelector);
                    self.reset_detail_fullscreen_scroll(DetailSurface::ConfigSelector);
                }
                _ => {}
            }
            return;
        }

        // Skin browser overlay active — select with Enter to hot-reload skin
        if self.skin_browser.active {
            match key.code {
                KeyCode::Esc => {
                    self.skin_browser.active = false;
                }
                KeyCode::Enter => {
                    if let Some(entry) = self.skin_browser.current() {
                        let name = entry.name.clone();
                        self.skin_browser.active = false;
                        self.handle_switch_skin(name);
                    }
                }
                KeyCode::Up => self.skin_browser.move_up(),
                KeyCode::Down => self.skin_browser.move_down(),
                KeyCode::Tab => self.skin_browser.move_down(),
                KeyCode::BackTab => self.skin_browser.move_up(),
                KeyCode::PageUp => self.skin_browser.page_up(),
                KeyCode::PageDown => self.skin_browser.page_down(),
                KeyCode::Backspace => self.skin_browser.pop_char(),
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.skin_browser.push_char(c);
                }
                _ => {}
            }
            return;
        }

        if self.session_inspector.active() {
            match key.code {
                KeyCode::Esc if !self.close_detail_fullscreen(DetailSurface::SessionInspector) => {
                    self.close_session_inspector();
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::SessionInspector,
                        self.session_inspector.pane.scroll,
                    );
                }
                _ if selector_action_key(&key, 'b') => {
                    self.close_session_inspector();
                }
                _ if selector_action_key(&key, 'r') => {
                    if let Some(session) = self.session_inspector.session.as_ref() {
                        if session.is_live {
                            self.push_output(
                                "The current session is already active.",
                                OutputRole::System,
                            );
                        } else {
                            let session_id = session.id.clone();
                            self.session_inspector.close();
                            self.session_browser.active = false;
                            self.close_detail_fullscreen(DetailSurface::SessionInspector);
                            self.handle_resume_session(Some(session_id));
                        }
                    }
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::SessionInspector) =>
                {
                    self.session_inspector.pane.focus = self.session_inspector.pane.focus.toggle();
                }
                KeyCode::Up => {
                    self.session_inspector.selector.move_up();
                    self.session_inspector.pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::SessionInspector);
                }
                KeyCode::Down => {
                    self.session_inspector.selector.move_down();
                    self.session_inspector.pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::SessionInspector);
                }
                KeyCode::PageUp
                    if self.detail_fullscreen_active(DetailSurface::SessionInspector) =>
                {
                    self.page_up_detail_fullscreen(DetailSurface::SessionInspector);
                }
                KeyCode::PageUp => {
                    if self.session_inspector.pane.focus == SplitPaneFocus::Detail {
                        self.session_inspector.pane.page_up(8);
                    } else {
                        self.session_inspector.selector.page_up();
                        self.session_inspector.pane.reset_scroll();
                        self.reset_detail_fullscreen_scroll(DetailSurface::SessionInspector);
                    }
                }
                KeyCode::PageDown
                    if self.detail_fullscreen_active(DetailSurface::SessionInspector) =>
                {
                    self.page_down_detail_fullscreen(DetailSurface::SessionInspector);
                }
                KeyCode::PageDown => {
                    if self.session_inspector.pane.focus == SplitPaneFocus::Detail {
                        self.session_inspector.pane.page_down(8);
                    } else {
                        self.session_inspector.selector.page_down();
                        self.session_inspector.pane.reset_scroll();
                        self.reset_detail_fullscreen_scroll(DetailSurface::SessionInspector);
                    }
                }
                KeyCode::Home => {
                    self.session_inspector.selector.selected = 0;
                    self.session_inspector.pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::SessionInspector);
                }
                KeyCode::End => {
                    self.session_inspector.selector.selected = self
                        .session_inspector
                        .selector
                        .filtered
                        .len()
                        .saturating_sub(1);
                    self.session_inspector.pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::SessionInspector);
                }
                KeyCode::Backspace => {
                    self.session_inspector.selector.pop_char();
                    self.session_inspector.pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::SessionInspector);
                }
                KeyCode::Char(c) if selector_search_char(&key) == Some(c) => {
                    self.session_inspector.selector.push_char(c);
                    self.session_inspector.pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::SessionInspector);
                }
                _ => {}
            }
            self.needs_redraw = true;
            return;
        }

        if self.log_inspector.active() {
            match key.code {
                KeyCode::Esc if !self.close_detail_fullscreen(DetailSurface::LogInspector) => {
                    self.close_log_inspector();
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::LogInspector,
                        self.log_inspector.pane.scroll,
                    );
                }
                _ if selector_action_key(&key, 'b') => {
                    self.close_log_inspector();
                }
                _ if selector_action_key(&key, 'r') => {
                    self.refresh_log_inspector();
                }
                _ if selector_action_key(&key, 'f') => {
                    self.toggle_log_follow();
                }
                KeyCode::Char(_) if log_level_shortcut(&key).is_some() => {
                    if let Some(level) = log_level_shortcut(&key) {
                        self.apply_log_level(level);
                    }
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::LogInspector) =>
                {
                    self.log_inspector.pane.focus = self.log_inspector.pane.focus.toggle();
                }
                KeyCode::Up => {
                    self.log_inspector.selector.move_up();
                    self.log_inspector.pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::LogInspector);
                }
                KeyCode::Down => {
                    self.log_inspector.selector.move_down();
                    self.log_inspector.pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::LogInspector);
                }
                KeyCode::PageUp if self.detail_fullscreen_active(DetailSurface::LogInspector) => {
                    self.page_up_detail_fullscreen(DetailSurface::LogInspector);
                }
                KeyCode::PageUp => {
                    if self.log_inspector.pane.focus == SplitPaneFocus::Detail {
                        self.log_inspector.pane.page_up(8);
                    } else {
                        self.log_inspector.selector.page_up();
                        self.log_inspector.pane.reset_scroll();
                        self.reset_detail_fullscreen_scroll(DetailSurface::LogInspector);
                    }
                }
                KeyCode::PageDown if self.detail_fullscreen_active(DetailSurface::LogInspector) => {
                    self.page_down_detail_fullscreen(DetailSurface::LogInspector);
                }
                KeyCode::PageDown => {
                    if self.log_inspector.pane.focus == SplitPaneFocus::Detail {
                        self.log_inspector.pane.page_down(8);
                    } else {
                        self.log_inspector.selector.page_down();
                        self.log_inspector.pane.reset_scroll();
                        self.reset_detail_fullscreen_scroll(DetailSurface::LogInspector);
                    }
                }
                KeyCode::Home => {
                    self.log_inspector.selector.selected = 0;
                    self.log_inspector.pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::LogInspector);
                }
                KeyCode::End => {
                    self.log_inspector.selector.selected =
                        self.log_inspector.selector.filtered.len().saturating_sub(1);
                    self.log_inspector.pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::LogInspector);
                }
                KeyCode::Backspace => {
                    self.log_inspector.selector.pop_char();
                    self.log_inspector.pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::LogInspector);
                }
                KeyCode::Char(c) if selector_search_char(&key) == Some(c) => {
                    self.log_inspector.selector.push_char(c);
                    self.log_inspector.pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::LogInspector);
                }
                _ => {}
            }
            self.needs_redraw = true;
            return;
        }

        if self.log_browser.active {
            match key.code {
                KeyCode::Esc if !self.close_detail_fullscreen(DetailSurface::LogBrowser) => {
                    self.log_browser.active = false;
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::LogBrowser,
                        self.log_browser_pane.scroll,
                    );
                }
                _ if selector_action_key(&key, 'r') => {
                    self.refresh_log_browser();
                    self.log_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::LogBrowser);
                }
                _ if selector_action_key(&key, 'f') => {
                    self.toggle_log_follow();
                }
                KeyCode::Char(_) if log_level_shortcut(&key).is_some() => {
                    if let Some(level) = log_level_shortcut(&key) {
                        self.apply_log_level(level);
                    }
                    self.log_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::LogBrowser);
                }
                KeyCode::Enter => {
                    if let Some(entry) = self.log_browser.current().cloned() {
                        self.open_log_inspector(entry);
                    }
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::LogBrowser) =>
                {
                    self.log_browser_pane.focus = self.log_browser_pane.focus.toggle();
                }
                KeyCode::Up => {
                    self.log_browser.move_up();
                    self.log_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::LogBrowser);
                }
                KeyCode::Down => {
                    self.log_browser.move_down();
                    self.log_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::LogBrowser);
                }
                KeyCode::PageUp if self.detail_fullscreen_active(DetailSurface::LogBrowser) => {
                    self.page_up_detail_fullscreen(DetailSurface::LogBrowser);
                }
                KeyCode::PageUp => {
                    if self.log_browser_pane.focus == SplitPaneFocus::Detail {
                        self.log_browser_pane.page_up(8);
                    } else {
                        self.log_browser.page_up();
                        self.log_browser_pane.reset_scroll();
                        self.reset_detail_fullscreen_scroll(DetailSurface::LogBrowser);
                    }
                }
                KeyCode::PageDown if self.detail_fullscreen_active(DetailSurface::LogBrowser) => {
                    self.page_down_detail_fullscreen(DetailSurface::LogBrowser);
                }
                KeyCode::PageDown => {
                    if self.log_browser_pane.focus == SplitPaneFocus::Detail {
                        self.log_browser_pane.page_down(8);
                    } else {
                        self.log_browser.page_down();
                        self.log_browser_pane.reset_scroll();
                        self.reset_detail_fullscreen_scroll(DetailSurface::LogBrowser);
                    }
                }
                KeyCode::Home => {
                    self.log_browser.selected = 0;
                    self.log_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::LogBrowser);
                }
                KeyCode::End => {
                    self.log_browser.selected = self.log_browser.filtered.len().saturating_sub(1);
                    self.log_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::LogBrowser);
                }
                KeyCode::Backspace => {
                    self.log_browser.pop_char();
                    self.log_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::LogBrowser);
                }
                KeyCode::Char(c) if selector_search_char(&key) == Some(c) => {
                    self.log_browser.push_char(c);
                    self.log_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::LogBrowser);
                }
                _ => {}
            }
            self.needs_redraw = true;
            return;
        }

        // Session browser overlay active — search-first, explicit uppercase actions
        if self.session_browser.active {
            match key.code {
                KeyCode::Esc if !self.close_detail_fullscreen(DetailSurface::SessionBrowser) => {
                    self.session_browser.active = false;
                }
                _ if selector_action_key(&key, 'z') => {
                    self.toggle_detail_fullscreen(
                        DetailSurface::SessionBrowser,
                        self.session_browser_pane.scroll,
                    );
                }
                KeyCode::Enter => {
                    if let Some(entry) = self.session_browser.current().cloned() {
                        self.close_detail_fullscreen(DetailSurface::SessionBrowser);
                        self.open_session_inspector(entry);
                    }
                }
                _ if selector_action_key(&key, 'r') => {
                    if let Some(entry) = self.session_browser.current() {
                        let session_id = entry.id.clone();
                        self.session_browser.active = false;
                        self.close_detail_fullscreen(DetailSurface::SessionBrowser);
                        self.handle_resume_session(Some(session_id));
                    }
                }
                _ if selector_action_key(&key, 'd') => {
                    if let Some(session_id) =
                        self.session_browser.current().map(|entry| entry.id.clone())
                    {
                        self.delete_session_from_browser(&session_id);
                        self.reset_detail_fullscreen_scroll(DetailSurface::SessionBrowser);
                    }
                }
                KeyCode::Tab | KeyCode::BackTab
                    if !self.detail_fullscreen_active(DetailSurface::SessionBrowser) =>
                {
                    self.session_browser_pane.focus = self.session_browser_pane.focus.toggle();
                }
                KeyCode::Up => {
                    self.session_browser.move_up();
                    self.session_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::SessionBrowser);
                }
                KeyCode::Down => {
                    self.session_browser.move_down();
                    self.session_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::SessionBrowser);
                }
                KeyCode::PageUp if self.detail_fullscreen_active(DetailSurface::SessionBrowser) => {
                    self.page_up_detail_fullscreen(DetailSurface::SessionBrowser);
                }
                KeyCode::PageUp => {
                    if self.session_browser_pane.focus == SplitPaneFocus::Detail {
                        self.session_browser_pane.page_up(8);
                    } else {
                        self.session_browser.page_up();
                        self.session_browser_pane.reset_scroll();
                        self.reset_detail_fullscreen_scroll(DetailSurface::SessionBrowser);
                    }
                }
                KeyCode::PageDown
                    if self.detail_fullscreen_active(DetailSurface::SessionBrowser) =>
                {
                    self.page_down_detail_fullscreen(DetailSurface::SessionBrowser);
                }
                KeyCode::PageDown => {
                    if self.session_browser_pane.focus == SplitPaneFocus::Detail {
                        self.session_browser_pane.page_down(8);
                    } else {
                        self.session_browser.page_down();
                        self.session_browser_pane.reset_scroll();
                        self.reset_detail_fullscreen_scroll(DetailSurface::SessionBrowser);
                    }
                }
                KeyCode::Home => {
                    self.session_browser.selected = 0;
                    self.session_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::SessionBrowser);
                }
                KeyCode::End => {
                    self.session_browser.selected =
                        self.session_browser.filtered.len().saturating_sub(1);
                    self.session_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::SessionBrowser);
                }
                KeyCode::Backspace => {
                    self.session_browser.pop_char();
                    self.session_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::SessionBrowser);
                    self.refresh_session_browser();
                }
                KeyCode::Char(c) if selector_search_char(&key) == Some(c) => {
                    self.session_browser.push_char(c);
                    self.session_browser_pane.reset_scroll();
                    self.reset_detail_fullscreen_scroll(DetailSurface::SessionBrowser);
                    self.refresh_session_browser();
                }
                _ => {}
            }
            self.needs_redraw = true;
            return;
        }

        // Completion overlay active — intercept Tab, Enter, Escape, arrows
        if self.completion.active {
            match key.code {
                KeyCode::Tab => {
                    if !self.completion.candidates.is_empty() {
                        self.completion.selected =
                            (self.completion.selected + 1) % self.completion.candidates.len();
                    }
                    return;
                }
                KeyCode::BackTab => {
                    if !self.completion.candidates.is_empty() {
                        self.completion.selected = if self.completion.selected == 0 {
                            self.completion.candidates.len() - 1
                        } else {
                            self.completion.selected - 1
                        };
                    }
                    return;
                }
                KeyCode::Up => {
                    if !self.completion.candidates.is_empty() {
                        self.completion.selected = if self.completion.selected == 0 {
                            self.completion.candidates.len() - 1
                        } else {
                            self.completion.selected - 1
                        };
                    }
                    return;
                }
                KeyCode::Down => {
                    if !self.completion.candidates.is_empty() {
                        self.completion.selected =
                            (self.completion.selected + 1) % self.completion.candidates.len();
                    }
                    return;
                }
                KeyCode::PageUp => {
                    if !self.completion.candidates.is_empty() {
                        self.completion.selected = self.completion.selected.saturating_sub(8);
                    }
                    return;
                }
                KeyCode::PageDown => {
                    if !self.completion.candidates.is_empty() {
                        let last = self.completion.candidates.len() - 1;
                        self.completion.selected = (self.completion.selected + 8).min(last);
                    }
                    return;
                }
                KeyCode::Home => {
                    if !self.completion.candidates.is_empty() {
                        self.completion.selected = 0;
                    }
                    return;
                }
                KeyCode::End => {
                    if !self.completion.candidates.is_empty() {
                        self.completion.selected = self.completion.candidates.len() - 1;
                    }
                    return;
                }
                KeyCode::Enter => {
                    self.accept_completion();
                    return;
                }
                KeyCode::Esc => {
                    self.completion.active = false;
                    return;
                }
                _ => {
                    // Any other key deactivates completion and falls through
                    self.completion.active = false;
                }
            }
        }

        match (key.modifiers, key.code) {
            // Page Up/Down — scroll output by viewport height
            (_, KeyCode::PageUp) if self.editor_mode.is_compose() => {
                self.textarea.scroll(Scrolling::PageUp);
                self.needs_redraw = true;
                return;
            }
            (_, KeyCode::PageDown) if self.editor_mode.is_compose() => {
                self.textarea.scroll(Scrolling::PageDown);
                self.needs_redraw = true;
                return;
            }
            (_, KeyCode::PageUp) => {
                let page = self.output_area_height.max(3).saturating_sub(2);
                self.scroll_output(page as i32);
                return;
            }
            (_, KeyCode::PageDown) => {
                let page = self.output_area_height.max(3).saturating_sub(2);
                self.scroll_output(-(page as i32));
                return;
            }
            _ => {}
        }

        if self.try_handle_queue_edit_key(key) {
            return;
        }

        match self.editor_mode {
            InputEditorMode::Inline => self.handle_inline_input_key(key),
            InputEditorMode::ComposeInsert => self.handle_compose_insert_key(key),
            InputEditorMode::ComposeNormal => self.handle_compose_normal_key(key),
        }
    }
}
