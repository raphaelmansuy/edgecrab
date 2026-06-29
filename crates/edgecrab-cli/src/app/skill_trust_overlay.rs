//! Skill guard trust overlay — render + input (exceeds Hermes ScanPanel + file inspector).

use super::*;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

use edgecrab_tools::tools::skills_hub::{BundleFilePreview, InstallScanPreview};

use crate::skill_trust_overlay::{
    SkillTrustFilesFocus, SkillTrustKeyContext, SkillTrustOverlayAction, SkillTrustPane,
    skill_trust_action_count, skill_trust_action_labels,
};

use super::remote_skill_guard::{skill_trust_severity_style, skill_trust_verdict_palette};

pub struct SkillTrustRenderState<'a> {
    pub preview: &'a InstallScanPreview,
    pub review_only: bool,
    pub pane: SkillTrustPane,
    pub files_focus: SkillTrustFilesFocus,
    pub selected_action: usize,
    pub findings_scroll: usize,
    pub selected_file: usize,
    pub file_content_scroll: usize,
    pub jump_line: Option<usize>,
}

pub fn render_skill_trust_overlay(frame: &mut Frame, area: Rect, state: SkillTrustRenderState<'_>) {
    frame.render_widget(Clear, area);

    let (accent, verdict_label, border) = skill_trust_verdict_palette(&state.preview.verdict);
    let needs_trust = state.preview.needs_trust;

    let popup_w = area.width.saturating_sub(2).min(100);
    let popup_h = area.height.saturating_sub(1).min(42);
    let x = area.x + (area.width.saturating_sub(popup_w)) / 2;
    let y = area.y + (area.height.saturating_sub(popup_h)) / 2;
    let popup = Rect::new(x, y, popup_w, popup_h);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(if state.preview.already_trusted { 2 } else { 1 }),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(popup);

    let title = format!(
        " Skill Guard · {} ",
        unicode_truncate(
            &state.preview.skill_name,
            (popup_w as usize).saturating_sub(18)
        )
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border))
            .title(Span::styled(
                title,
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ))
            .style(Style::default().bg(Color::Rgb(16, 18, 24))),
        popup,
    );

    let header = vec![
        Line::from(vec![
            Span::styled(
                verdict_label,
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("[{}]", state.preview.trust_level),
                Style::default().fg(Color::Rgb(130, 150, 170)),
            ),
            Span::raw("  "),
            Span::styled(
                format!(
                    "{} finding(s) · {} file(s)",
                    state.preview.finding_count,
                    state.preview.files.len()
                ),
                Style::default().fg(Color::Rgb(160, 170, 190)),
            ),
        ]),
        Line::from(Span::styled(
            unicode_truncate(&state.preview.identifier, popup_w as usize - 4),
            Style::default().fg(Color::Rgb(110, 220, 210)),
        )),
    ];
    frame.render_widget(Paragraph::new(header), chunks[0]);

    let hash_short = state
        .preview
        .content_hash
        .strip_prefix("sha256:")
        .unwrap_or(&state.preview.content_hash);
    let hash_short = if hash_short.len() > 16 {
        format!("{}…", &hash_short[..16])
    } else {
        hash_short.to_string()
    };
    let mut meta_lines = vec![Line::from(vec![
        Span::styled("Hash ", Style::default().fg(Color::Rgb(100, 110, 130))),
        Span::styled(hash_short, Style::default().fg(Color::Rgb(130, 150, 170))),
    ])];
    if state.preview.already_trusted {
        meta_lines.push(Line::from(Span::styled(
            "✓ Prior hash-bound trust on file",
            Style::default().fg(Color::Rgb(80, 220, 140)),
        )));
    }
    frame.render_widget(Paragraph::new(meta_lines), chunks[1]);

    let mut tally: Vec<Span> = Vec::new();
    for (count, sev) in [
        (state.preview.critical_count, "critical"),
        (state.preview.high_count, "high"),
        (state.preview.medium_count, "medium"),
        (state.preview.low_count, "low"),
    ] {
        if count > 0 {
            if !tally.is_empty() {
                tally.push(Span::raw(" "));
            }
            tally.push(Span::styled(
                format!(" {count} {sev} "),
                skill_trust_severity_style(sev),
            ));
        }
    }
    if tally.is_empty() {
        tally.push(Span::styled(
            " No risky patterns detected ",
            Style::default().fg(Color::Rgb(80, 220, 140)),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(tally)), chunks[2]);

    let policy_style = if state.preview.needs_trust {
        Style::default().fg(Color::Rgb(255, 180, 120))
    } else if state.preview.needs_force {
        Style::default().fg(Color::Rgb(255, 220, 140))
    } else {
        Style::default().fg(Color::Rgb(140, 160, 180))
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            unicode_truncate(&state.preview.policy_reason, popup_w as usize - 4),
            policy_style,
        ))
        .wrap(Wrap { trim: true }),
        chunks[3],
    );

    let tab_findings_style = if state.pane == SkillTrustPane::Findings {
        Style::default()
            .fg(Color::Rgb(110, 220, 210))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(100, 110, 130))
    };
    let tab_files_style = if state.pane == SkillTrustPane::Files {
        Style::default()
            .fg(Color::Rgb(110, 220, 210))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Rgb(100, 110, 130))
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("  Findings  ", tab_findings_style),
            Span::raw("│ "),
            Span::styled(
                format!(" Files ({}) ", state.preview.files.len()),
                tab_files_style,
            ),
            Span::raw("  Tab switch · f view file at finding"),
        ])),
        chunks[4],
    );

    match state.pane {
        SkillTrustPane::Findings => {
            render_findings_pane(frame, chunks[5], &state);
        }
        SkillTrustPane::Files => {
            render_files_pane(frame, chunks[5], &state);
        }
    }

    let selected_action = state.selected_action;

    let labels = skill_trust_action_labels(needs_trust, state.review_only);
    let action_count = skill_trust_action_count(needs_trust, state.review_only);
    let mut action_spans = Vec::new();
    for (i, label) in labels.iter().take(action_count).enumerate() {
        if i > 0 {
            action_spans.push(Span::raw("   "));
        }
        let selected = i == selected_action.min(action_count.saturating_sub(1));
        action_spans.push(Span::styled(
            format!("[{}{}] ", i + 1, if selected { "●" } else { " " }),
            if selected {
                Style::default()
                    .fg(Color::Rgb(110, 220, 210))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(100, 110, 130))
            },
        ));
        action_spans.push(Span::styled(
            (*label).to_string(),
            if selected {
                Style::default().fg(Color::Rgb(230, 240, 250))
            } else {
                Style::default().fg(Color::Rgb(160, 170, 190))
            },
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(action_spans)).alignment(Alignment::Center),
        chunks[6],
    );

    let hint = match state.pane {
        SkillTrustPane::Findings => {
            " ←/→ actions · j/k findings · f file · Tab files · Enter confirm · Esc cancel"
        }
        SkillTrustPane::Files => {
            " j/k browse · h/l focus · Tab findings · Enter confirm · Esc cancel"
        }
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            hint,
            Style::default().fg(Color::Rgb(90, 100, 120)),
        ))
        .alignment(Alignment::Center),
        chunks[7],
    );
}

fn render_findings_pane(frame: &mut Frame, area: Rect, state: &SkillTrustRenderState<'_>) {
    let findings = &state.preview.findings;
    let max_visible = area.height.saturating_sub(2) as usize;
    let scroll = state.findings_scroll.min(findings.len().saturating_sub(1));

    let items: Vec<ListItem> = if findings.is_empty() {
        vec![ListItem::new(Span::styled(
            "  (no individual findings)",
            Style::default().fg(Color::Rgb(100, 110, 130)),
        ))]
    } else {
        findings
            .iter()
            .enumerate()
            .skip(scroll)
            .take(max_visible)
            .map(|(idx, f)| {
                let is_cursor = idx == scroll;
                let bg = if is_cursor {
                    Color::Rgb(28, 36, 48)
                } else {
                    Color::Rgb(16, 18, 24)
                };
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(
                            format!(" {:<8}", f.severity),
                            skill_trust_severity_style(&f.severity).bg(bg),
                        ),
                        Span::styled(
                            format!("{:<14} ", f.category),
                            Style::default().fg(Color::Rgb(150, 160, 180)).bg(bg),
                        ),
                        Span::styled(
                            format!("{}:{} ", f.file, f.line),
                            Style::default().fg(Color::Rgb(100, 120, 140)).bg(bg),
                        ),
                        Span::styled(
                            unicode_truncate(&f.description, 40),
                            Style::default().fg(Color::Rgb(210, 220, 230)).bg(bg),
                        ),
                    ]),
                    Line::from(Span::styled(
                        format!("      › {}", unicode_truncate(&f.matched_text, 72)),
                        Style::default().fg(Color::Rgb(130, 140, 160)).bg(bg),
                    )),
                ])
            })
            .collect()
    };

    let title = if findings.len() > max_visible {
        format!(" Findings ({}/{}) ", scroll + 1, findings.len())
    } else {
        " Findings ".into()
    };
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(50, 60, 80)))
                .title(title),
        ),
        area,
    );
}

fn render_files_pane(frame: &mut Frame, area: Rect, state: &SkillTrustRenderState<'_>) {
    let files = &state.preview.files;
    if files.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "  No files in bundle",
                Style::default().fg(Color::Rgb(100, 110, 130)),
            ))
            .block(Block::default().borders(Borders::ALL).title(" Files ")),
            area,
        );
        return;
    }

    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(34), Constraint::Percentage(66)])
        .split(area);

    let selected = state.selected_file.min(files.len().saturating_sub(1));
    let max_list = split[0].height.saturating_sub(2) as usize;
    let list_scroll = if files.len() <= max_list {
        0
    } else {
        selected
            .saturating_sub(max_list / 2)
            .min(files.len().saturating_sub(max_list))
    };

    let list_items: Vec<ListItem> = files
        .iter()
        .enumerate()
        .skip(list_scroll)
        .take(max_list)
        .map(|(idx, file)| {
            let is_sel = idx == selected;
            let is_focus = is_sel && state.files_focus == SkillTrustFilesFocus::List;
            let bg = if is_focus {
                Color::Rgb(28, 44, 48)
            } else if is_sel {
                Color::Rgb(22, 30, 36)
            } else {
                Color::Rgb(16, 18, 24)
            };
            let flag = if file.finding_lines.is_empty() {
                "   "
            } else {
                " ⚠ "
            };
            ListItem::new(Line::from(vec![
                Span::styled(flag, Style::default().fg(Color::Rgb(255, 191, 0)).bg(bg)),
                Span::styled(
                    unicode_truncate(&file.path, (split[0].width as usize).saturating_sub(6)),
                    Style::default()
                        .fg(if is_sel {
                            Color::Rgb(110, 220, 210)
                        } else {
                            Color::Rgb(180, 190, 200)
                        })
                        .bg(bg),
                ),
            ]))
        })
        .collect();

    let list_border = if state.files_focus == SkillTrustFilesFocus::List {
        Color::Rgb(110, 220, 210)
    } else {
        Color::Rgb(50, 60, 80)
    };
    frame.render_widget(
        List::new(list_items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(list_border))
                .title(format!(" Files ({}/{}) ", selected + 1, files.len())),
        ),
        split[0],
    );

    let file = &files[selected];
    render_file_content(frame, split[1], file, state);
}

fn render_file_content(
    frame: &mut Frame,
    area: Rect,
    file: &BundleFilePreview,
    state: &SkillTrustRenderState<'_>,
) {
    let content_border = if state.files_focus == SkillTrustFilesFocus::Content {
        Color::Rgb(110, 220, 210)
    } else {
        Color::Rgb(50, 60, 80)
    };
    let title = if file.truncated {
        format!(" {} (truncated) ", file.path)
    } else {
        format!(" {} ", file.path)
    };

    let lines: Vec<&str> = file.content.lines().collect();
    let max_visible = area.height.saturating_sub(2) as usize;

    let jump = state.jump_line.unwrap_or(1);
    let auto_scroll = state
        .jump_line
        .map(|line| line.saturating_sub(1))
        .unwrap_or(state.file_content_scroll);
    let scroll = auto_scroll.min(lines.len().saturating_sub(1));

    let finding_set: std::collections::HashSet<usize> =
        file.finding_lines.iter().copied().collect();

    let content_lines: Vec<Line> = if lines.is_empty() {
        vec![Line::from(Span::styled(
            "(empty file)",
            Style::default().fg(Color::Rgb(100, 110, 130)),
        ))]
    } else {
        lines
            .iter()
            .enumerate()
            .skip(scroll)
            .take(max_visible)
            .map(|(idx, line)| {
                let line_no = idx + 1;
                let is_finding = finding_set.contains(&line_no);
                let is_jump = state.jump_line == Some(line_no);
                let bg = if is_jump {
                    Color::Rgb(60, 40, 30)
                } else if is_finding {
                    Color::Rgb(40, 28, 28)
                } else {
                    Color::Rgb(16, 18, 24)
                };
                let num_style = Style::default().fg(Color::Rgb(80, 90, 110)).bg(bg);
                let text_style = if is_finding {
                    Style::default().fg(Color::Rgb(255, 180, 120)).bg(bg)
                } else {
                    Style::default().fg(Color::Rgb(200, 210, 220)).bg(bg)
                };
                Line::from(vec![
                    Span::styled(format!("{:>4} ", line_no), num_style),
                    Span::styled(*line, text_style),
                ])
            })
            .collect()
    };

    frame.render_widget(
        Paragraph::new(content_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(content_border))
                .title(title),
        ),
        area,
    );

    let _ = jump;
}

fn unicode_truncate(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    if max_chars <= 1 {
        return "…".into();
    }
    format!(
        "{}…",
        s.chars()
            .take(max_chars.saturating_sub(1))
            .collect::<String>()
    )
}

impl App {
    pub(super) fn open_skill_guard_review(&mut self, identifier: &str) {
        if identifier.is_empty() {
            self.push_output("Usage: /skills review <identifier>", OutputRole::System);
            return;
        }
        if self.skill_trust_prompt.is_some() {
            self.push_output("Skill guard overlay is already open.", OutputRole::System);
            return;
        }

        self.push_output(
            format!("Reviewing `{identifier}` — fetch + scan…"),
            OutputRole::System,
        );
        let tx = self.response_tx.clone();
        let id = identifier.to_string();
        let skills_dir = edgecrab_core::edgecrab_home().join("skills");
        let review_only = skills_dir.join(identifier).is_dir()
            && !edgecrab_tools::tools::skills_hub::is_remote_skill_identifier(identifier);
        self.rt_handle.spawn(async move {
            let optional_dir = edgecrab_tools::tools::skills_sync::optional_skills_dir();
            match edgecrab_tools::tools::skills_hub::preview_skill_scan(
                &id,
                &skills_dir,
                optional_dir.as_deref(),
            )
            .await
            {
                Ok(preview) => {
                    let entry = RemoteSkillEntry {
                        name: preview.skill_name.clone(),
                        identifier: preview.identifier.clone(),
                        description: preview.policy_reason.clone(),
                        source_label: preview.source.clone(),
                        origin: preview.source.clone(),
                        trust_level: preview.trust_level.clone(),
                        tags: Vec::new(),
                        search_text: String::new(),
                        installed_name: if review_only {
                            Some(preview.skill_name.clone())
                        } else {
                            None
                        },
                        action: RemoteSkillAction::Install,
                    };
                    let _ = tx.send(AgentResponse::RemoteSkillGuardPrompt {
                        entry,
                        preview: Box::new(preview),
                        review_only,
                    });
                }
                Err(error) => {
                    let _ = tx.send(AgentResponse::RemoteSkillActionFailed {
                        action_label: "review".into(),
                        identifier: id,
                        error,
                    });
                }
            }
        });
    }

    fn skill_trust_key_context(&self) -> SkillTrustKeyContext {
        let Some(state) = &self.skill_trust_prompt else {
            return SkillTrustKeyContext::default();
        };
        SkillTrustKeyContext {
            pane: state.pane,
            needs_trust: state.preview.needs_trust,
            review_only: state.review_only,
        }
    }

    pub(super) fn handle_skill_trust_key(&mut self, key: crossterm::event::KeyEvent) {
        let ctx = self.skill_trust_key_context();
        if self.skill_trust_prompt.is_none() {
            return;
        }
        let action_count = skill_trust_action_count(ctx.needs_trust, ctx.review_only);

        match crate::skill_trust_overlay::map_skill_trust_key(key.code, key.modifiers, ctx) {
            SkillTrustOverlayAction::SelectPrevAction => {
                if let Some(state) = self.skill_trust_prompt.as_mut()
                    && state.selected_action > 0
                {
                    state.selected_action -= 1;
                }
            }
            SkillTrustOverlayAction::SelectNextAction => {
                if let Some(state) = self.skill_trust_prompt.as_mut()
                    && state.selected_action + 1 < action_count
                {
                    state.selected_action += 1;
                }
            }
            SkillTrustOverlayAction::ScrollUp => {
                if let Some(state) = self.skill_trust_prompt.as_mut() {
                    match state.pane {
                        SkillTrustPane::Findings => {
                            state.findings_scroll = state.findings_scroll.saturating_sub(1);
                        }
                        SkillTrustPane::Files => match state.files_focus {
                            SkillTrustFilesFocus::List => {
                                if state.selected_file > 0 {
                                    state.selected_file -= 1;
                                    state.jump_line = None;
                                }
                            }
                            SkillTrustFilesFocus::Content => {
                                state.file_content_scroll =
                                    state.file_content_scroll.saturating_sub(1);
                                state.jump_line = None;
                            }
                        },
                    }
                }
            }
            SkillTrustOverlayAction::ScrollDown => {
                if let Some(state) = self.skill_trust_prompt.as_mut() {
                    match state.pane {
                        SkillTrustPane::Findings => {
                            let max = state.preview.findings.len().saturating_sub(1);
                            if state.findings_scroll < max {
                                state.findings_scroll += 1;
                            }
                        }
                        SkillTrustPane::Files => {
                            let file_count = state.preview.files.len();
                            match state.files_focus {
                                SkillTrustFilesFocus::List => {
                                    if state.selected_file + 1 < file_count {
                                        state.selected_file += 1;
                                        state.jump_line = None;
                                    }
                                }
                                SkillTrustFilesFocus::Content => {
                                    state.file_content_scroll += 1;
                                    state.jump_line = None;
                                }
                            }
                        }
                    }
                }
            }
            SkillTrustOverlayAction::TogglePane => {
                if let Some(state) = self.skill_trust_prompt.as_mut() {
                    state.pane = match state.pane {
                        SkillTrustPane::Findings => SkillTrustPane::Files,
                        SkillTrustPane::Files => SkillTrustPane::Findings,
                    };
                    state.jump_line = None;
                }
            }
            SkillTrustOverlayAction::ToggleFilesFocus => {
                if let Some(state) = self.skill_trust_prompt.as_mut()
                    && state.pane == SkillTrustPane::Files
                {
                    state.files_focus = match state.files_focus {
                        SkillTrustFilesFocus::List => SkillTrustFilesFocus::Content,
                        SkillTrustFilesFocus::Content => SkillTrustFilesFocus::List,
                    };
                    state.jump_line = None;
                }
            }
            SkillTrustOverlayAction::JumpToFindingFile => {
                self.skill_trust_jump_to_finding_file();
            }
            SkillTrustOverlayAction::Confirm => {
                let choice = self
                    .skill_trust_prompt
                    .as_ref()
                    .map(|s| s.selected_action)
                    .unwrap_or(0);
                self.apply_skill_trust_choice(choice);
                return;
            }
            SkillTrustOverlayAction::Cancel => {
                self.skill_trust_prompt = None;
                self.remote_skill_browser.action_in_flight = None;
            }
            SkillTrustOverlayAction::Choose(index) => {
                if index < action_count {
                    self.apply_skill_trust_choice(index);
                    return;
                }
            }
            SkillTrustOverlayAction::Noop => {}
        }
        self.needs_redraw = true;
    }

    fn skill_trust_jump_to_finding_file(&mut self) {
        let Some(state) = self.skill_trust_prompt.as_mut() else {
            return;
        };
        let scroll = state.findings_scroll;
        let Some(finding) = state.preview.findings.get(scroll) else {
            return;
        };
        let file_idx = state
            .preview
            .files
            .iter()
            .position(|f| f.path == finding.file);
        state.pane = SkillTrustPane::Files;
        state.files_focus = SkillTrustFilesFocus::Content;
        if let Some(idx) = file_idx {
            state.selected_file = idx;
        }
        state.file_content_scroll = finding.line.saturating_sub(1);
        state.jump_line = Some(finding.line);
    }

    fn apply_skill_trust_choice(&mut self, choice: usize) {
        let Some(state) = self.skill_trust_prompt.take() else {
            return;
        };
        let needs_trust = state.preview.needs_trust;
        let review_only = state.review_only;
        let action_count = skill_trust_action_count(needs_trust, review_only);
        let choice = choice.min(action_count.saturating_sub(1));

        if review_only {
            if needs_trust && choice == 0 {
                let preview = state.preview.clone();
                if let Err(e) = edgecrab_tools::tools::skills_hub::record_guard_approval(
                    &preview.identifier,
                    &preview.skill_name,
                    &preview.content_hash,
                    &preview.verdict,
                    preview.finding_count,
                ) {
                    self.push_output(format!("Trust record failed: {e}"), OutputRole::Error);
                } else {
                    self.push_output(
                        format!("Trust recorded for `{}` (hash-bound).", preview.skill_name),
                        OutputRole::System,
                    );
                }
            }
            self.remote_skill_browser.action_in_flight = None;
            self.needs_redraw = true;
            return;
        }

        let cancel_index = if needs_trust { 2 } else { 1 };
        if choice == cancel_index {
            self.remote_skill_browser.action_in_flight = None;
            self.needs_redraw = true;
            return;
        }

        if needs_trust && choice == 1 {
            let preview = state.preview.clone();
            if let Err(e) = edgecrab_tools::tools::skills_hub::record_guard_approval(
                &preview.identifier,
                &preview.skill_name,
                &preview.content_hash,
                &preview.verdict,
                preview.finding_count,
            ) {
                self.push_output(format!("Trust record failed: {e}"), OutputRole::Error);
            } else {
                self.push_output(
                    format!(
                        "Trust recorded for `{}`. Install later with /skills install {}",
                        preview.skill_name, preview.identifier
                    ),
                    OutputRole::System,
                );
            }
            self.remote_skill_browser.action_in_flight = None;
            self.needs_redraw = true;
            return;
        }

        let gate = if needs_trust {
            edgecrab_tools::tools::skills_hub::InstallGate {
                force: false,
                trust: true,
            }
        } else {
            edgecrab_tools::tools::skills_hub::InstallGate {
                force: true,
                trust: false,
            }
        };

        self.begin_remote_skill_install(state.entry, gate);
    }

    pub(super) fn render_skill_trust_overlay(&self, frame: &mut Frame, area: Rect) {
        let Some(state) = &self.skill_trust_prompt else {
            return;
        };
        render_skill_trust_overlay(
            frame,
            area,
            SkillTrustRenderState {
                preview: &state.preview,
                review_only: state.review_only,
                pane: state.pane,
                files_focus: state.files_focus,
                selected_action: state.selected_action,
                findings_scroll: state.findings_scroll,
                selected_file: state.selected_file,
                file_content_scroll: state.file_content_scroll,
                jump_line: state.jump_line,
            },
        );
    }
}
