//! Publish a local skill to GitHub (PR flow) or ClawHub (Hermes `do_publish` parity).

use std::path::{Path, PathBuf};
use std::process::Command;

use super::skills_guard;
use super::{SkillBundle, build_local_skill_bundle, hub_client};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishTarget {
    Github,
    Clawhub,
}

impl PublishTarget {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "github" | "gh" => Some(Self::Github),
            "clawhub" | "claw" => Some(Self::Clawhub),
            _ => None,
        }
    }
}

/// Publish `skill_path` (directory with SKILL.md) to `target`.
pub async fn publish_skill(
    skill_path: &Path,
    target: PublishTarget,
    repo: Option<&str>,
) -> Result<String, String> {
    let path = resolve_skill_path(skill_path)?;
    let bundle = build_local_skill_bundle(&path, None)?;
    let scan = skills_guard::scan_skill(&path, &bundle.source, &bundle.trust_level);
    if scan.verdict == skills_guard::Verdict::Dangerous {
        return Err(format!(
            "Refusing to publish: Skill Guard verdict Dangerous.\n{}",
            skills_guard::format_scan_report(&scan)
        ));
    }

    match target {
        PublishTarget::Github => publish_github(&path, &bundle, repo, &scan),
        PublishTarget::Clawhub => publish_clawhub(&path, &bundle).await,
    }
}

fn resolve_skill_path(skill_path: &Path) -> Result<PathBuf, String> {
    let path = if skill_path.is_absolute() {
        skill_path.to_path_buf()
    } else {
        let home_skills = crate::config_ref::resolve_edgecrab_home().join("skills");
        let candidate = home_skills.join(skill_path);
        if candidate.join("SKILL.md").is_file() {
            candidate
        } else {
            skill_path.to_path_buf()
        }
    };
    if !path.join("SKILL.md").is_file() {
        return Err(format!("No SKILL.md found at {}", path.display()));
    }
    Ok(path)
}

fn publish_github(
    path: &Path,
    bundle: &SkillBundle,
    repo: Option<&str>,
    scan: &skills_guard::ScanResult,
) -> Result<String, String> {
    let repo = repo
        .map(str::to_string)
        .or_else(|| std::env::var("EDGECRAB_SKILLS_PUBLISH_REPO").ok())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            "GitHub publish requires --repo owner/repo (or EDGECRAB_SKILLS_PUBLISH_REPO)".to_string()
        })?;

    let branch = format!(
        "edgecrab-skill-{}-{}",
        bundle.name,
        chrono::Utc::now().format("%Y%m%d%H%M%S")
    );
    let title = format!("Add skill: {}", bundle.name);
    let report = skills_guard::format_scan_report(scan);

    if Command::new("gh").arg("--version").output().is_ok() {
        return Ok(format!(
            "Ready to open a PR with gh:\n\
               1. Fork/clone {repo}\n\
               2. Copy {} into the skills tree\n\
               3. gh pr create --repo {repo} --title {title:?} --body \"Published via edgecrab skills publish\"\n\
             Suggested branch: {branch}\n\
             Skill Guard:\n{report}\n\
             (Automated PR push is intentionally interactive — review before submitting.)",
            path.display(),
        ));
    }

    Ok(format!(
        "Scan passed for '{}'. Install GitHub CLI (`gh`) or open a PR manually to {repo}.\n\
         Skill path: {}\n{report}",
        bundle.name,
        path.display(),
    ))
}

async fn publish_clawhub(path: &Path, bundle: &SkillBundle) -> Result<String, String> {
    let token = std::env::var("CLAWHUB_TOKEN")
        .or_else(|_| std::env::var("EDGECRAB_CLAWHUB_TOKEN"))
        .map_err(|_| {
            "ClawHub publish requires CLAWHUB_TOKEN (or EDGECRAB_CLAWHUB_TOKEN)".to_string()
        })?;
    let skill_md = std::fs::read_to_string(path.join("SKILL.md"))
        .map_err(|e| format!("read SKILL.md: {e}"))?;

    let client = hub_client()?;
    let url = "https://clawhub.ai/api/v1/skills";
    super::ensure_safe_url(url)?;
    let resp = client
        .post(url)
        .bearer_auth(token.trim())
        .json(&serde_json::json!({
            "name": bundle.name,
            "skill_md": skill_md,
            "source": "edgecrab-publish",
        }))
        .send()
        .await
        .map_err(|e| format!("ClawHub publish request failed: {e}"))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if status.is_success() {
        Ok(format!(
            "Published '{}' to ClawHub (HTTP {status}).\n{body}",
            bundle.name
        ))
    } else {
        Err(format!(
            "ClawHub publish failed (HTTP {status}).\n{body}\n\
             Tip: verify CLAWHUB_TOKEN and that the skill passes community guidelines."
        ))
    }
}
