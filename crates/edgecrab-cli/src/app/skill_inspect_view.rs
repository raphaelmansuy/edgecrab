//! Pure Inspect dossier model — SKILL.md-first capability preview (019 016).
//!
//! No HTTP. Built from catalog metadata + optional [`InstallScanPreview`] cache.

use edgecrab_tools::tools::skills_hub::{BundleFilePreview, InstallScanPreview};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Catalog fields needed for the Inspect header (from RemoteSkillEntry).
#[derive(Debug, Clone)]
pub struct SkillInspectCatalog {
    pub name: String,
    pub identifier: String,
    pub description: String,
    pub source_label: String,
    pub origin: String,
    pub trust_level: String,
    pub tags: Vec<String>,
    pub url: Option<String>,
    pub repo: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectPreviewState {
    Loading,
    Ready,
    Failed(String),
    Missing,
}

#[derive(Debug, Clone)]
pub struct SkillInspectModel {
    pub catalog: SkillInspectCatalog,
    pub preview_state: InspectPreviewState,
    pub verdict: Option<String>,
    pub hash_short: Option<String>,
    pub file_count: usize,
    pub finding_count: usize,
    pub frontmatter_name: Option<String>,
    pub frontmatter_description: Option<String>,
    pub skill_md_excerpt: Vec<String>,
    pub capability_bullets: Vec<String>,
    pub files_ordered: Vec<(String, usize)>,
    pub has_skill_md: bool,
}

impl SkillInspectModel {
    pub fn from_catalog_and_preview(
        catalog: SkillInspectCatalog,
        preview: Option<&InstallScanPreview>,
        loading: bool,
        error: Option<&str>,
    ) -> Self {
        if loading {
            return Self {
                catalog,
                preview_state: InspectPreviewState::Loading,
                verdict: None,
                hash_short: None,
                file_count: 0,
                finding_count: 0,
                frontmatter_name: None,
                frontmatter_description: None,
                skill_md_excerpt: Vec::new(),
                capability_bullets: Vec::new(),
                files_ordered: Vec::new(),
                has_skill_md: false,
            };
        }
        if let Some(err) = error {
            return Self {
                catalog,
                preview_state: InspectPreviewState::Failed(err.to_string()),
                verdict: None,
                hash_short: None,
                file_count: 0,
                finding_count: 0,
                frontmatter_name: None,
                frontmatter_description: None,
                skill_md_excerpt: Vec::new(),
                capability_bullets: Vec::new(),
                files_ordered: Vec::new(),
                has_skill_md: false,
            };
        }
        let Some(preview) = preview else {
            return Self {
                catalog,
                preview_state: InspectPreviewState::Missing,
                verdict: None,
                hash_short: None,
                file_count: 0,
                finding_count: 0,
                frontmatter_name: None,
                frontmatter_description: None,
                skill_md_excerpt: Vec::new(),
                capability_bullets: Vec::new(),
                files_ordered: Vec::new(),
                has_skill_md: false,
            };
        };

        let files_ordered = order_files_skill_md_first(&preview.files);
        let skill_md = preview
            .files
            .iter()
            .find(|f| f.path.eq_ignore_ascii_case("SKILL.md") || f.path.ends_with("/SKILL.md"));
        let has_skill_md = skill_md.is_some();
        let (fm_name, fm_desc, body) =
            skill_md
                .map(|f| parse_skill_md(&f.content))
                .unwrap_or((None, None, String::new()));
        let excerpt = excerpt_lines(&body, 36);
        let mut bullets = capability_bullets_from_body(&body);
        bullets.extend(capability_bullets_from_paths(
            &files_ordered
                .iter()
                .map(|(p, _)| p.as_str())
                .collect::<Vec<_>>(),
        ));
        bullets.truncate(8);

        Self {
            catalog,
            preview_state: InspectPreviewState::Ready,
            verdict: Some(preview.verdict.clone()),
            hash_short: Some(short_hash(&preview.content_hash)),
            file_count: preview.files.len(),
            finding_count: preview.finding_count,
            frontmatter_name: fm_name,
            frontmatter_description: fm_desc,
            skill_md_excerpt: excerpt,
            capability_bullets: bullets,
            files_ordered,
            has_skill_md,
        }
    }

    /// Provenance one-liner for the header.
    pub fn provenance_line(&self) -> String {
        let mut parts = Vec::new();
        if let Some(url) = &self.catalog.url {
            if !url.is_empty() {
                parts.push(url.clone());
            }
        } else if let Some(repo) = &self.catalog.repo {
            let mut p = repo.clone();
            if let Some(path) = &self.catalog.path
                && !path.is_empty()
            {
                p.push('/');
                p.push_str(path);
            }
            parts.push(p);
        } else if !self.catalog.origin.is_empty() {
            parts.push(self.catalog.origin.clone());
        }
        parts.join(" · ")
    }
}

pub fn order_files_skill_md_first(files: &[BundleFilePreview]) -> Vec<(String, usize)> {
    let mut items: Vec<(String, usize)> = files
        .iter()
        .map(|f| (f.path.clone(), f.line_count))
        .collect();
    items.sort_by(|a, b| {
        let a_skill = a.0.eq_ignore_ascii_case("SKILL.md") || a.0.ends_with("/SKILL.md");
        let b_skill = b.0.eq_ignore_ascii_case("SKILL.md") || b.0.ends_with("/SKILL.md");
        match (a_skill, b_skill) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.0.cmp(&b.0),
        }
    });
    items
}

fn short_hash(hash: &str) -> String {
    let h = hash.trim_start_matches("sha256:");
    if h.len() <= 12 {
        hash.to_string()
    } else {
        format!("{}…", &h[..12])
    }
}

/// Parse optional YAML frontmatter + body.
pub fn parse_skill_md(content: &str) -> (Option<String>, Option<String>, String) {
    let content = content.trim_start_matches('\u{feff}');
    if !content.starts_with("---") {
        return (None, None, content.to_string());
    }
    let rest = content.strip_prefix("---").unwrap_or(content);
    let Some(end) = rest.find("\n---") else {
        return (None, None, content.to_string());
    };
    let fm = &rest[..end];
    let body = rest[end + 4..].trim_start_matches('\n').to_string();
    let mut name = None;
    let mut description = None;
    for line in fm.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("name:") {
            name = Some(unquote(v.trim()));
        } else if let Some(v) = line.strip_prefix("description:") {
            description = Some(unquote(v.trim()));
        }
    }
    (name, description, body)
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn excerpt_lines(body: &str, max_lines: usize) -> Vec<String> {
    body.lines()
        .take(max_lines)
        .map(|l| l.to_string())
        .collect()
}

pub fn capability_bullets_from_body(body: &str) -> Vec<String> {
    let mut bullets = Vec::new();
    let mut h1 = None;
    let mut h2: Vec<String> = Vec::new();
    let mut langs: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if !in_fence {
                let lang = trimmed.trim_start_matches('`').trim();
                if !lang.is_empty() && !langs.iter().any(|l| l == lang) {
                    langs.push(lang.to_string());
                }
                in_fence = true;
            } else {
                in_fence = false;
            }
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            if h1.is_none() {
                h1 = Some(rest.trim().to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("## ")
            && h2.len() < 5
        {
            h2.push(rest.trim().to_string());
        }
    }
    if let Some(h) = h1 {
        bullets.push(format!("Teaches: {h}"));
    }
    for section in h2 {
        bullets.push(format!("Section: {section}"));
    }
    if !langs.is_empty() {
        bullets.push(format!("May include code: {}", langs.join(", ")));
    }
    bullets
}

pub fn capability_bullets_from_paths(paths: &[&str]) -> Vec<String> {
    let mut bullets = Vec::new();
    let mut scripts = false;
    let mut py = false;
    let mut makefile = false;
    for p in paths {
        let lower = p.to_ascii_lowercase();
        if lower.contains("scripts/") || lower.starts_with("scripts/") {
            scripts = true;
        }
        if lower.ends_with(".py") {
            py = true;
        }
        if lower.ends_with("makefile") || lower.ends_with("/makefile") {
            makefile = true;
        }
    }
    if scripts {
        bullets.push("Contains scripts/ directory".into());
    }
    if py {
        bullets.push("Includes Python files".into());
    }
    if makefile {
        bullets.push("Includes a Makefile".into());
    }
    bullets
}

/// Map search notices to marketplace empty-state CTAs (source-aware).
pub fn marketplace_notice_cta(notice: &str) -> Option<&'static str> {
    let lower = notice.to_ascii_lowercase();
    let skills_sh = lower.contains("skills.sh") || lower.contains("skills-sh");
    let githubish = lower.contains("github")
        || lower.contains("api.github.com")
        || lower.contains("raw.githubusercontent");
    let rate_limited = lower.contains("rate limit")
        || lower.contains("rate-limit")
        || lower.contains("rate_limit")
        || lower.contains("ratelimited")
        || lower.contains("429");
    let authish = lower.contains("403") || lower.contains("401");

    if skills_sh && (rate_limited || authish) {
        return Some(
            "skills.sh rate-limited — wait a minute, press r to retry, or type ≥2 chars to search.",
        );
    }
    if (githubish || (!skills_sh && (rate_limited || authish)))
        && (rate_limited || authish || lower.contains("forbidden"))
    {
        return Some("Set GITHUB_TOKEN or GH_TOKEN, then press r to retry.");
    }
    if lower.contains("timeout") || lower.contains("timed out") {
        return Some("Source timed out — press r to retry or change source with [ ].");
    }
    if lower.contains("offline") || lower.contains("network") || lower.contains("dns") {
        return Some("Network issue — check connectivity, then press r.");
    }
    None
}

/// Render dossier lines (capability first, trust teaser last).
pub fn render_inspect_dossier_lines(model: &SkillInspectModel, scroll: u16) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let label = Style::default().fg(Color::Rgb(145, 170, 170));
    let accent = Style::default()
        .fg(Color::Rgb(110, 220, 210))
        .add_modifier(Modifier::BOLD);
    let warn = Style::default().fg(Color::Rgb(255, 191, 0));

    lines.push(Line::from(vec![
        Span::styled(format!("Inspect · {} ", model.catalog.name), accent),
        Span::styled(
            format!("{} ", model.catalog.source_label),
            Style::default().fg(Color::Rgb(110, 220, 210)),
        ),
        Span::styled(
            format!("[{}]", model.catalog.trust_level),
            Style::default().fg(Color::Rgb(160, 180, 180)),
        ),
    ]));
    lines.push(Line::from(model.catalog.identifier.clone()));

    let prov = model.provenance_line();
    if !prov.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Provenance: ", label),
            Span::raw(prov),
        ]));
    }

    let mut meta_bits = Vec::new();
    if model.file_count > 0 {
        meta_bits.push(format!("{} files", model.file_count));
    }
    if let Some(h) = &model.hash_short {
        meta_bits.push(format!("hash {h}"));
    }
    if let Some(v) = &model.verdict {
        meta_bits.push(format!("verdict {}", v.to_ascii_uppercase()));
    }
    if !meta_bits.is_empty() {
        lines.push(Line::from(meta_bits.join(" · ")));
    }
    lines.push(Line::from(""));

    lines.push(Line::from(Span::styled("WHAT IT CLAIMS", accent)));
    match &model.preview_state {
        InspectPreviewState::Loading => {
            lines.push(Line::from(Span::styled(
                "Fetching skill body…",
                Style::default().fg(Color::Rgb(110, 220, 210)),
            )));
            if !model.catalog.description.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(model.catalog.description.clone()));
            }
        }
        InspectPreviewState::Failed(err) => {
            lines.push(Line::from(Span::styled(
                format!("Scan failed: {err}"),
                Style::default().fg(Color::Rgb(255, 120, 120)),
            )));
            lines.push(Line::from(Span::styled(
                "Press s to retry preview scan.",
                warn,
            )));
            if !model.catalog.description.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(model.catalog.description.clone()));
            }
        }
        InspectPreviewState::Missing => {
            lines.push(Line::from("Preview not loaded yet."));
            if !model.catalog.description.is_empty() {
                lines.push(Line::from(model.catalog.description.clone()));
            }
        }
        InspectPreviewState::Ready => {
            if let Some(n) = &model.frontmatter_name {
                lines.push(Line::from(vec![
                    Span::styled("name: ", label),
                    Span::raw(n.clone()),
                ]));
            }
            if let Some(d) = &model.frontmatter_description {
                lines.push(Line::from(vec![
                    Span::styled("description: ", label),
                    Span::raw(d.clone()),
                ]));
            } else if !model.catalog.description.is_empty() {
                lines.push(Line::from(model.catalog.description.clone()));
            }
            if !model.has_skill_md {
                lines.push(Line::from(Span::styled(
                    "No SKILL.md — treat as incomplete bundle.",
                    warn,
                )));
            } else if model.skill_md_excerpt.is_empty() {
                lines.push(Line::from("(SKILL.md is empty)"));
            } else {
                lines.push(Line::from(""));
                for line in &model.skill_md_excerpt {
                    lines.push(Line::from(line.clone()));
                }
            }
        }
    }

    if matches!(model.preview_state, InspectPreviewState::Ready)
        && !model.capability_bullets.is_empty()
    {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "CAPABILITIES (claims / contents)",
            accent,
        )));
        for b in &model.capability_bullets {
            lines.push(Line::from(format!("· {b}")));
        }
    }

    if matches!(model.preview_state, InspectPreviewState::Ready) && !model.files_ordered.is_empty()
    {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("FILES", accent)));
        for (path, nlines) in model.files_ordered.iter().take(24) {
            lines.push(Line::from(format!("  {path}  ({nlines} lines)")));
        }
        if model.files_ordered.len() > 24 {
            lines.push(Line::from(format!(
                "  … {} more",
                model.files_ordered.len() - 24
            )));
        }
    }

    if matches!(model.preview_state, InspectPreviewState::Ready) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("TRUST TEASER", accent)));
        if let Some(v) = &model.verdict {
            lines.push(Line::from(format!(
                "Verdict: {} · {} finding(s)",
                v.to_ascii_uppercase(),
                model.finding_count
            )));
        }
        lines.push(Line::from(Span::styled(
            "Press e for full Skill Guard evidence (findings + files).",
            label,
        )));
    }

    if !model.catalog.tags.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Tags: ", label),
            Span::raw(model.catalog.tags.join(", ")),
        ]));
    }

    let scroll = scroll as usize;
    if scroll >= lines.len() {
        Vec::new()
    } else {
        lines.into_iter().skip(scroll).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_tools::tools::skills_hub::{BundleFilePreview, InstallScanPreview};

    fn fixture_preview() -> InstallScanPreview {
        InstallScanPreview {
            skill_name: "demo".into(),
            identifier: "openai/skills/demo".into(),
            source: "openai".into(),
            trust_level: "trusted".into(),
            verdict: "safe".into(),
            content_hash: "sha256:abcdef0123456789ffff".into(),
            finding_count: 0,
            critical_count: 0,
            high_count: 0,
            medium_count: 0,
            low_count: 0,
            allowed: true,
            needs_trust: false,
            needs_force: false,
            already_trusted: false,
            policy_reason: "ok".into(),
            findings: Vec::new(),
            files: vec![
                BundleFilePreview {
                    path: "scripts/run.py".into(),
                    size_bytes: 10,
                    line_count: 2,
                    finding_lines: Vec::new(),
                    truncated: false,
                    content: "print(1)\n".into(),
                },
                BundleFilePreview {
                    path: "SKILL.md".into(),
                    size_bytes: 100,
                    line_count: 12,
                    finding_lines: Vec::new(),
                    truncated: false,
                    content: "---\nname: Demo Skill\ndescription: Helps with demos\n---\n# Demo Skill\n\n## Setup\n\nDo setup.\n\n## Usage\n\n```bash\necho hi\n```\n".into(),
                },
            ],
        }
    }

    #[test]
    fn skill_md_first_ordering() {
        let p = fixture_preview();
        let ordered = order_files_skill_md_first(&p.files);
        assert_eq!(ordered[0].0, "SKILL.md");
    }

    #[test]
    fn parse_frontmatter_and_capabilities() {
        let p = fixture_preview();
        let catalog = SkillInspectCatalog {
            name: "demo".into(),
            identifier: "openai/skills/demo".into(),
            description: "catalog blurb".into(),
            source_label: "OpenAI".into(),
            origin: "https://github.com/openai/skills".into(),
            trust_level: "trusted".into(),
            tags: vec!["demo".into()],
            url: None,
            repo: Some("openai/skills".into()),
            path: Some("skills/.curated/demo".into()),
        };
        let model = SkillInspectModel::from_catalog_and_preview(catalog, Some(&p), false, None);
        assert_eq!(model.frontmatter_name.as_deref(), Some("Demo Skill"));
        assert!(
            model
                .skill_md_excerpt
                .iter()
                .any(|l| l.contains("Demo Skill"))
        );
        assert!(
            model
                .capability_bullets
                .iter()
                .any(|b| b.contains("Setup") || b.contains("bash") || b.contains("Python"))
        );
        assert_eq!(model.verdict.as_deref(), Some("safe"));
        let lines = render_inspect_dossier_lines(&model, 0);
        assert!(
            lines
                .iter()
                .any(|l| l.to_string().contains("WHAT IT CLAIMS"))
        );
    }

    #[test]
    fn notice_cta_token() {
        let gh = marketplace_notice_cta("GitHub: rate limit exceeded").expect("cta");
        assert!(gh.contains("GITHUB_TOKEN"));
        assert!(marketplace_notice_cta("source timed out").is_some());
        assert!(marketplace_notice_cta("all good").is_none());
    }

    #[test]
    fn notice_cta_skills_sh_429_does_not_suggest_github_token() {
        let cta = marketplace_notice_cta(
            "skills.sh: skills.sh returned HTTP 429 Too Many Requests: rate_limit_exceeded",
        )
        .expect("cta");
        assert!(cta.contains("skills.sh"));
        assert!(
            !cta.contains("GITHUB_TOKEN"),
            "skills.sh rate limits must not suggest GitHub tokens: {cta}"
        );
        let cached = marketplace_notice_cta(
            "skills.sh rate-limited — showing 40 cached skills. Press r to retry later.",
        )
        .expect("cta");
        assert!(!cached.contains("GITHUB_TOKEN"));
    }
}
