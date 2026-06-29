//! Remote skill browser — proactive guard scan on selection (Hermes ScanPanel parity).

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use edgecrab_tools::tools::skills_hub::InstallScanPreview;

use super::*;

#[derive(Default, Clone)]
pub(super) struct RemoteSkillGuardCache {
    pub for_identifier: Option<String>,
    pub inflight: Option<String>,
    pub preview: Option<InstallScanPreview>,
    pub error: Option<String>,
}

impl App {
    pub(super) fn clear_remote_skill_guard_cache(&mut self) {
        self.remote_skill_guard = RemoteSkillGuardCache::default();
    }

    pub(super) fn schedule_remote_skill_guard_preview(&mut self) {
        if !self.remote_skill_browser.selector.active {
            return;
        }
        let Some(entry) = self.remote_skill_browser.selector.current() else {
            self.clear_remote_skill_guard_cache();
            return;
        };
        let identifier = entry.identifier.clone();
        if self.remote_skill_guard.inflight.as_deref() == Some(identifier.as_str()) {
            return;
        }
        if self.remote_skill_guard.for_identifier.as_deref() == Some(identifier.as_str())
            && self.remote_skill_guard.preview.is_some()
        {
            return;
        }

        if self.remote_skill_guard.for_identifier.as_deref() != Some(identifier.as_str()) {
            self.remote_skill_guard.preview = None;
            self.remote_skill_guard.error = None;
            self.remote_skill_guard.for_identifier = None;
        }

        self.remote_skill_guard.inflight = Some(identifier.clone());
        self.remote_skill_guard.error = None;
        self.needs_redraw = true;

        let tx = self.response_tx.clone();
        self.rt_handle.spawn(async move {
            let optional_dir = edgecrab_tools::tools::skills_sync::optional_skills_dir();
            match edgecrab_tools::tools::skills_hub::preview_install_scan(
                &identifier,
                optional_dir.as_deref(),
            )
            .await
            {
                Ok(preview) => {
                    let _ = tx.send(AgentResponse::RemoteSkillGuardPreviewReady {
                        identifier,
                        preview: Box::new(preview),
                    });
                }
                Err(error) => {
                    let _ =
                        tx.send(AgentResponse::RemoteSkillGuardPreviewFailed { identifier, error });
                }
            }
        });
    }

    pub(super) fn apply_remote_skill_guard_preview_ready(
        &mut self,
        identifier: String,
        preview: InstallScanPreview,
    ) {
        if self.remote_skill_guard.inflight.as_deref() != Some(identifier.as_str()) {
            return;
        }
        if self
            .remote_skill_browser
            .selector
            .current()
            .is_none_or(|e| e.identifier != identifier)
        {
            return;
        }
        self.remote_skill_guard.inflight = None;
        self.remote_skill_guard.for_identifier = Some(identifier);
        self.remote_skill_guard.preview = Some(preview);
        self.remote_skill_guard.error = None;
        self.needs_redraw = true;
    }

    pub(super) fn apply_remote_skill_guard_preview_failed(
        &mut self,
        identifier: String,
        error: String,
    ) {
        if self.remote_skill_guard.inflight.as_deref() != Some(identifier.as_str()) {
            return;
        }
        if self
            .remote_skill_browser
            .selector
            .current()
            .is_none_or(|e| e.identifier != identifier)
        {
            return;
        }
        self.remote_skill_guard.inflight = None;
        self.remote_skill_guard.for_identifier = Some(identifier);
        self.remote_skill_guard.preview = None;
        self.remote_skill_guard.error = Some(error);
        self.needs_redraw = true;
    }

    pub(super) fn append_remote_skill_guard_detail(&self, detail_lines: &mut Vec<Line>) {
        let Some(entry) = self.remote_skill_browser.selector.current() else {
            return;
        };
        let guard = &self.remote_skill_guard;
        if guard.for_identifier.as_deref() != Some(entry.identifier.as_str()) {
            if guard.inflight.as_deref() == Some(entry.identifier.as_str()) {
                detail_lines.push(Line::from(""));
                detail_lines.push(Line::from(Span::styled(
                    "Skill Guard: fetching and scanning…",
                    Style::default().fg(Color::Rgb(110, 220, 210)),
                )));
            }
            return;
        }

        detail_lines.push(Line::from(""));
        detail_lines.push(Line::from(Span::styled(
            "Skill Guard",
            Style::default()
                .fg(Color::Rgb(255, 191, 0))
                .add_modifier(Modifier::BOLD),
        )));

        if let Some(error) = &guard.error {
            detail_lines.push(Line::from(Span::styled(
                format!("Scan failed: {error}"),
                Style::default().fg(Color::Rgb(255, 120, 120)),
            )));
            detail_lines.push(Line::from(Span::styled(
                "Press S to retry · Enter to install (scan runs again)",
                Style::default().fg(Color::Rgb(100, 110, 130)),
            )));
            return;
        }

        let Some(preview) = &guard.preview else {
            detail_lines.push(Line::from(Span::styled(
                "Scanning…",
                Style::default().fg(Color::Rgb(110, 220, 210)),
            )));
            return;
        };

        let (accent, verdict_label, _) = skill_trust_verdict_palette(&preview.verdict);
        let policy_style = if preview.allowed {
            Style::default().fg(Color::Rgb(80, 220, 140))
        } else if preview.needs_trust || preview.needs_force {
            Style::default().fg(Color::Rgb(255, 191, 0))
        } else {
            Style::default().fg(Color::Rgb(255, 120, 120))
        };
        let policy_label = if preview.allowed {
            "Install allowed"
        } else if preview.needs_trust {
            "Needs trust approval"
        } else if preview.needs_force {
            "Needs --force"
        } else {
            "Blocked"
        };

        detail_lines.push(Line::from(vec![
            Span::styled(
                verdict_label,
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{} finding(s)", preview.finding_count),
                Style::default().fg(Color::Rgb(160, 170, 190)),
            ),
            Span::raw("  "),
            Span::styled(policy_label, policy_style),
        ]));

        let mut sev_spans: Vec<Span> = Vec::new();
        for (count, sev) in [
            (preview.critical_count, "critical"),
            (preview.high_count, "high"),
            (preview.medium_count, "medium"),
            (preview.low_count, "low"),
        ] {
            if count > 0 {
                if !sev_spans.is_empty() {
                    sev_spans.push(Span::raw(" "));
                }
                sev_spans.push(Span::styled(
                    format!(" {count} {sev} "),
                    skill_trust_severity_style(sev),
                ));
            }
        }
        if sev_spans.is_empty() && preview.finding_count == 0 {
            sev_spans.push(Span::styled(
                " No risky patterns ",
                Style::default().fg(Color::Rgb(80, 220, 140)),
            ));
        }
        if !sev_spans.is_empty() {
            detail_lines.push(Line::from(sev_spans));
        }

        if preview.already_trusted {
            detail_lines.push(Line::from(Span::styled(
                "✓ Hash-bound trust on file — install proceeds without re-approval",
                Style::default().fg(Color::Rgb(80, 220, 140)),
            )));
        }

        let hash_short = preview
            .content_hash
            .strip_prefix("sha256:")
            .unwrap_or(&preview.content_hash);
        let hash_short = if hash_short.len() > 12 {
            format!("{}…", &hash_short[..12])
        } else {
            hash_short.to_string()
        };
        detail_lines.push(Line::from(vec![
            Span::styled("Hash: ", Style::default().fg(Color::Rgb(100, 110, 130))),
            Span::styled(hash_short, Style::default().fg(Color::Rgb(130, 150, 170))),
        ]));

        detail_lines.push(Line::from(Span::styled(
            preview.policy_reason.clone(),
            Style::default().fg(Color::Rgb(140, 150, 170)),
        )));

        if !preview.findings.is_empty() {
            detail_lines.push(Line::from(""));
            for f in preview.findings.iter().take(6) {
                detail_lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {:<8}", f.severity),
                        skill_trust_severity_style(&f.severity),
                    ),
                    Span::styled(
                        format!("{:<12} ", f.category),
                        Style::default().fg(Color::Rgb(150, 160, 180)),
                    ),
                    Span::raw(format!("{}:{} ", f.file, f.line)),
                    Span::raw(truncate_chars(&f.description, 52)),
                ]));
            }
            if preview.findings.len() > 6 {
                detail_lines.push(Line::from(Span::styled(
                    format!(
                        "  … {} more (Enter opens full guard review)",
                        preview.findings.len() - 6
                    ),
                    Style::default().fg(Color::Rgb(100, 110, 130)),
                )));
            }
        }

        if preview.needs_trust {
            detail_lines.push(Line::from(Span::styled(
                "Enter → full guard overlay · /skills trust <id> to pre-approve",
                Style::default().fg(Color::Rgb(255, 180, 120)),
            )));
        }
    }
}

pub(super) fn guard_verdict_chip(verdict: &str) -> (&'static str, Color) {
    match verdict {
        "safe" => ("✓", Color::Rgb(80, 220, 140)),
        "dangerous" => ("⛔", Color::Rgb(255, 90, 90)),
        _ => ("⚠", Color::Rgb(255, 191, 0)),
    }
}

pub(super) fn skill_trust_verdict_palette(verdict: &str) -> (Color, &'static str, Color) {
    match verdict {
        "safe" => (Color::Rgb(80, 220, 140), "✓ SAFE", Color::Rgb(40, 90, 60)),
        "dangerous" => (
            Color::Rgb(255, 90, 90),
            "⛔ DANGEROUS",
            Color::Rgb(120, 30, 30),
        ),
        _ => (
            Color::Rgb(255, 191, 0),
            "⚠ CAUTION",
            Color::Rgb(120, 90, 20),
        ),
    }
}

pub(super) fn skill_trust_severity_style(sev: &str) -> Style {
    match sev {
        "critical" => Style::default()
            .fg(Color::Rgb(255, 80, 80))
            .add_modifier(Modifier::BOLD),
        "high" => Style::default().fg(Color::Rgb(255, 140, 80)),
        "medium" => Style::default().fg(Color::Rgb(255, 191, 0)),
        _ => Style::default().fg(Color::Rgb(140, 160, 180)),
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!(
        "{}…",
        s.chars().take(max.saturating_sub(1)).collect::<String>()
    )
}
