//! Command approval overlay — render + input (Hermes `ApprovalPrompt` parity).

use super::*;

impl App {
    pub(super) fn apply_approval_choice(&mut self, choice: edgecrab_core::ApprovalChoice) {
        let full_command =
            if let DisplayState::WaitingForApproval { full_command, .. } = &self.display_state {
                Some(full_command.clone())
            } else {
                None
            };

        if matches!(
            choice,
            edgecrab_core::ApprovalChoice::Session | edgecrab_core::ApprovalChoice::Always
        ) && let Some(full_command) = full_command.as_deref()
        {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            full_command.hash(&mut hasher);
            self.session_approvals
                .insert(format!("{:x}", hasher.finish()));
        }

        if let Some(tx) = self.approval_pending_tx.take() {
            let is_deny = choice == edgecrab_core::ApprovalChoice::Deny;
            let _ = tx.send(choice);
            self.display_state = if is_deny {
                DisplayState::Idle
            } else {
                DisplayState::AwaitingFirstToken {
                    frame: 0,
                    started: Instant::now(),
                }
            };
            self.needs_redraw = true;
        }
    }

    pub(super) fn handle_approval_choice_command(&mut self, choice: edgecrab_core::ApprovalChoice) {
        if self.approval_pending_tx.is_some() {
            let is_preview = matches!(
                self.display_state,
                DisplayState::WaitingForApproval {
                    kind: edgecrab_tools::registry::ApprovalKind::PreviewLoopback,
                    ..
                }
            );
            let text = match (&choice, is_preview) {
                (edgecrab_core::ApprovalChoice::Once, true) => {
                    "Allowed browser preview access once."
                }
                (edgecrab_core::ApprovalChoice::Session, true) => {
                    "Allowed browser preview access for this session."
                }
                (edgecrab_core::ApprovalChoice::Always, true) => {
                    "Enabled security.preview permanently (persisted)."
                }
                (edgecrab_core::ApprovalChoice::Deny, true) => {
                    "Denied browser preview access."
                }
                (edgecrab_core::ApprovalChoice::Once, false) => "Approved current command once.",
                (edgecrab_core::ApprovalChoice::Session, false) => {
                    "Approved current command for the rest of this session."
                }
                (edgecrab_core::ApprovalChoice::Always, false) => {
                    "Approved current command permanently."
                }
                (edgecrab_core::ApprovalChoice::Deny, false) => "Denied current command.",
            };
            self.apply_approval_choice(choice);
            self.push_output(text, OutputRole::System);
            return;
        }

        if choice == edgecrab_core::ApprovalChoice::Deny && self.clarify_pending_tx.is_some() {
            let tx = self.clarify_pending_tx.take();
            self.flush_abandoned_clarify("cancelled");
            if let Some(tx) = tx {
                let _ = tx.send(String::new());
            }
            self.display_state = DisplayState::AwaitingFirstToken {
                frame: 0,
                started: Instant::now(),
            };
            self.turn_activity.set_phase(ShelfPhase::AwaitingFirstToken);
            self.needs_redraw = true;
            return;
        }

        self.push_output(
            "No pending approval prompt. Use /deny only when EdgeCrab is explicitly waiting for approval or clarification.",
            OutputRole::System,
        );
    }

    /// Handle a key event when the approval overlay is active.
    pub(super) fn handle_approval_key(&mut self, key: crossterm::event::KeyEvent) {
        if !matches!(self.display_state, DisplayState::WaitingForApproval { .. }) {
            return;
        }

        match crate::approval_overlay::map_approval_key(key.code, key.modifiers) {
            crate::approval_overlay::ApprovalOverlayAction::SelectPrev => {
                if let DisplayState::WaitingForApproval {
                    ref mut selected, ..
                } = self.display_state
                    && *selected > 0
                {
                    *selected -= 1;
                }
            }
            crate::approval_overlay::ApprovalOverlayAction::SelectNext => {
                if let DisplayState::WaitingForApproval {
                    ref mut selected, ..
                } = self.display_state
                    && *selected + 1 < crate::approval_overlay::APPROVAL_CHOICE_COUNT
                {
                    *selected += 1;
                }
            }
            crate::approval_overlay::ApprovalOverlayAction::ToggleFullView => {
                if let DisplayState::WaitingForApproval {
                    ref mut show_full, ..
                } = self.display_state
                {
                    *show_full = !*show_full;
                }
            }
            crate::approval_overlay::ApprovalOverlayAction::ScrollUp => {
                if let DisplayState::WaitingForApproval {
                    ref mut scroll_offset,
                    ..
                } = self.display_state
                {
                    *scroll_offset = scroll_offset.saturating_add(1);
                }
            }
            crate::approval_overlay::ApprovalOverlayAction::ScrollDown => {
                if let DisplayState::WaitingForApproval {
                    ref mut scroll_offset,
                    ..
                } = self.display_state
                {
                    *scroll_offset = scroll_offset.saturating_sub(1);
                }
            }
            crate::approval_overlay::ApprovalOverlayAction::Confirm => {
                if let DisplayState::WaitingForApproval { selected, .. } = self.display_state {
                    let choice = crate::approval_overlay::approval_choice_at_index(selected);
                    self.apply_approval_choice(choice);
                }
            }
            crate::approval_overlay::ApprovalOverlayAction::Deny => {
                self.apply_approval_choice(edgecrab_core::ApprovalChoice::Deny);
            }
            crate::approval_overlay::ApprovalOverlayAction::Choose(index) => {
                if index < crate::approval_overlay::APPROVAL_CHOICE_COUNT {
                    let choice = crate::approval_overlay::approval_choice_at_index(index);
                    self.apply_approval_choice(choice);
                }
            }
            crate::approval_overlay::ApprovalOverlayAction::Noop => {}
        }

        self.needs_redraw = true;
    }

    pub(super) fn render_approval_overlay(&self, frame: &mut Frame, area: Rect) {
        let (command, full_command, kind, selected, scroll_offset) =
            if let DisplayState::WaitingForApproval {
                ref command,
                ref full_command,
                kind,
                selected,
                scroll_offset,
                ..
            } = self.display_state
            {
                (
                    command.as_str(),
                    full_command.as_str(),
                    kind,
                    selected,
                    scroll_offset,
                )
            } else {
                return;
            };

        frame.render_widget(Clear, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(area);

        let is_preview = matches!(kind, edgecrab_tools::registry::ApprovalKind::PreviewLoopback);
        let cmd_text = if is_preview {
            if full_command.is_empty() {
                format!("Allow browser access to {command} for this session?")
            } else {
                format!("Allow browser access to {full_command} for this session?")
            }
        } else if full_command.is_empty() {
            command.to_string()
        } else {
            full_command.to_string()
        };
        let title = if is_preview {
            " ⚠  Preview access "
        } else {
            " ⚠  Approval required "
        };
        let cmd_lines: Vec<Line> = cmd_text
            .lines()
            .map(|l| {
                Line::from(vec![
                    Span::styled(
                        "  ⚠  ",
                        Style::default()
                            .fg(Color::Rgb(255, 140, 0))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        l.to_string(),
                        Style::default().fg(Color::Rgb(255, 220, 180)),
                    ),
                ])
            })
            .collect();
        let cmd_para = Paragraph::new(cmd_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Rgb(255, 140, 0)))
                    .title(title),
            )
            .wrap(Wrap { trim: false })
            .scroll((scroll_offset, 0));
        frame.render_widget(cmd_para, chunks[0]);

        let mut btn_spans: Vec<Span> = vec![Span::raw("  ")];
        for (i, label) in crate::approval_overlay::APPROVAL_LABELS.iter().enumerate() {
            let is_sel = i == selected;
            let style = if is_sel {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Rgb(255, 140, 0))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(180, 180, 200))
            };
            btn_spans.push(Span::styled(format!(" [{label}] "), style));
            btn_spans.push(Span::raw(" "));
        }

        let buttons = Paragraph::new(Line::from(btn_spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(80, 80, 100))),
        );
        frame.render_widget(buttons, chunks[1]);

        let help = Paragraph::new(Line::from(vec![
            Span::styled(" ← → ", Style::default().fg(Color::Rgb(255, 140, 0))),
            Span::styled("select  ", Style::default().fg(Color::DarkGray)),
            Span::styled("1-4 ", Style::default().fg(Color::Rgb(255, 140, 0))),
            Span::styled("pick  ", Style::default().fg(Color::DarkGray)),
            Span::styled("↑ ↓ ", Style::default().fg(Color::Rgb(255, 140, 0))),
            Span::styled("scroll  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Enter ", Style::default().fg(Color::Rgb(255, 140, 0))),
            Span::styled("confirm  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc ", Style::default().fg(Color::Rgb(255, 140, 0))),
            Span::styled("deny", Style::default().fg(Color::DarkGray)),
        ]));
        frame.render_widget(help, chunks[2]);
    }
}
