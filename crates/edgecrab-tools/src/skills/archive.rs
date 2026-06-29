//! Skill archive / restore — Hermes-parity `.archive/` layout (never auto-delete).

use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::tools::skills::find_skill_dir_public;
use crate::tools::skills_hub::hub_installed_skill_names;
use crate::tools::skills_sync::is_bundled_skill;

use super::protected::is_protected_builtin;
use super::slug::slugify;
use super::usage;

fn parse_skill_name_from_md(content: &str, fallback: &str) -> String {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return fallback.to_string();
    }
    let after = &trimmed[3..];
    let Some(end) = after.find("\n---") else {
        return fallback.to_string();
    };
    for line in after[..end].lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("name:") {
            let name = rest.trim().trim_matches(['\'', '"']);
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    fallback.to_string()
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("mkdir failed: {e}"))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("readdir failed: {e}"))? {
        let entry = entry.map_err(|e| format!("entry failed: {e}"))?;
        let path = entry.path();
        let dest_path = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &dest_path)?;
        } else {
            std::fs::copy(&path, &dest_path).map_err(|e| format!("copy failed: {e}"))?;
        }
    }
    Ok(())
}

fn move_dir(src: &Path, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        return Err(format!("destination already exists: {}", dest.display()));
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir failed: {e}"))?;
    }
    match std::fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_dir_recursive(src, dest)?;
            std::fs::remove_dir_all(src).map_err(|e| format!("cleanup failed: {e}"))?;
            Ok(())
        }
    }
}

fn matches_skill_query(dir: &Path, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return false;
    }
    let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if dir_name.eq_ignore_ascii_case(query) || slugify(query) == dir_name {
        return true;
    }
    let skill_md = dir.join("SKILL.md");
    if skill_md.is_file()
        && let Ok(content) = std::fs::read_to_string(&skill_md)
    {
        let name = parse_skill_name_from_md(&content, dir_name);
        return name.eq_ignore_ascii_case(query);
    }
    false
}

fn find_archived_dir(home: &Path, skill_query: &str) -> Option<PathBuf> {
    let root = archive_root(home);
    if !root.is_dir() {
        return None;
    }
    let mut timestamped: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(&root).ok()?.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if matches_skill_query(&path, skill_query) {
            let dir_name = path.file_name()?.to_str()?;
            if dir_name.contains('-') && dir_name.starts_with(&format!("{}-", slugify(skill_query)))
            {
                timestamped.push(path);
            } else {
                return Some(path);
            }
        }
    }
    timestamped.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    timestamped.into_iter().next()
}

pub fn archive_root(home: &Path) -> PathBuf {
    home.join("skills").join(".archive")
}

fn user_skills_root(home: &Path) -> PathBuf {
    home.join("skills")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveEligibility {
    Eligible,
    Pinned,
    Protected,
    Bundled,
    HubInstalled,
    NotFound,
}

pub fn check_archive_eligibility(
    home: &Path,
    skill_name: &str,
    prune_builtins: bool,
) -> ArchiveEligibility {
    let name = skill_name.trim();
    if name.is_empty() {
        return ArchiveEligibility::NotFound;
    }
    if usage::is_pinned(home, name) {
        return ArchiveEligibility::Pinned;
    }
    if is_protected_builtin(name) {
        return ArchiveEligibility::Protected;
    }
    if hub_installed_skill_names(home).contains(name) {
        return ArchiveEligibility::HubInstalled;
    }
    if is_bundled_skill(home, name) && !prune_builtins {
        return ArchiveEligibility::Bundled;
    }
    if find_skill_dir_public(&user_skills_root(home), name).is_some() {
        ArchiveEligibility::Eligible
    } else {
        ArchiveEligibility::NotFound
    }
}

/// Move an agent-created skill to `~/.edgecrab/skills/.archive/`.
pub fn archive_skill(
    home: &Path,
    skill_name: &str,
    prune_builtins: bool,
) -> Result<String, String> {
    let name = skill_name.trim();
    match check_archive_eligibility(home, name, prune_builtins) {
        ArchiveEligibility::Pinned => {
            return Err(format!(
                "skill '{name}' is pinned — run /skills unpin {name} first"
            ));
        }
        ArchiveEligibility::Protected => {
            return Err(format!(
                "skill '{name}' is a protected built-in — never archived by curator"
            ));
        }
        ArchiveEligibility::Bundled => {
            return Err(format!(
                "skill '{name}' is bundled/synced — never archive (use /skills disable or platform filter)"
            ));
        }
        ArchiveEligibility::HubInstalled => {
            return Err(format!(
                "skill '{name}' is hub-installed — remove via /skills hub instead"
            ));
        }
        ArchiveEligibility::NotFound => {
            return Err(format!("skill '{name}' not found in active skills tree"));
        }
        ArchiveEligibility::Eligible => {}
    }

    let skill_dir = find_skill_dir_public(&user_skills_root(home), name)
        .ok_or_else(|| format!("skill '{name}' not found"))?;
    let root = archive_root(home);
    std::fs::create_dir_all(&root).map_err(|e| format!("failed to create archive dir: {e}"))?;

    let base_name = skill_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(name);
    let mut dest = root.join(base_name);
    if dest.exists() {
        let stamp = Utc::now().format("%Y%m%d%H%M%S");
        dest = root.join(format!("{base_name}-{stamp}"));
    }

    move_dir(&skill_dir, &dest)?;
    usage::set_archived(home, name, true);
    Ok(format!("Archived '{name}' to {}", dest.display()))
}

/// Restore a skill from `.archive/` to the active tree (flat layout).
pub fn restore_skill(home: &Path, skill_name: &str) -> Result<String, String> {
    let name = skill_name.trim();
    if hub_installed_skill_names(home).contains(name) {
        return Err(format!(
            "skill '{name}' is hub-installed — restore would shadow upstream version"
        ));
    }
    if is_bundled_skill(home, name)
        && find_skill_dir_public(&user_skills_root(home), name).is_some()
    {
        return Err(format!(
            "skill '{name}' is bundled and already present — restore would shadow upstream version"
        ));
    }

    let Some(src) = find_archived_dir(home, name) else {
        return Err(format!("skill '{name}' not found in archive"));
    };

    let dest_leaf = src.file_name().and_then(|n| n.to_str()).unwrap_or(name);
    let dest = user_skills_root(home).join(dest_leaf);
    if dest.exists() {
        return Err(format!("destination already exists: {}", dest.display()));
    }

    move_dir(&src, &dest)?;
    usage::set_archived(home, name, false);
    Ok(format!("Restored '{name}' to {}", dest.display()))
}

/// List skill directory names under `.archive/`.
pub fn list_archived(home: &Path) -> Vec<String> {
    let root = archive_root(home);
    if !root.is_dir() {
        return Vec::new();
    }
    let mut names: Vec<String> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.is_dir() {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map(str::to_string)
            } else {
                None
            }
        })
        .collect();
    names.sort();
    names
}

pub fn format_archived_list(home: &Path) -> String {
    let names = list_archived(home);
    if names.is_empty() {
        return "No archived skills.".into();
    }
    let mut lines = vec![format!("Archived skills ({}):", names.len())];
    for name in names {
        lines.push(format!("  {name}  (/curator restore {name})"));
    }
    lines.join("\n")
}

#[derive(Debug, Clone)]
pub struct PruneCandidate {
    pub name: String,
    pub slug: String,
    pub idle_days: i64,
}

#[derive(Debug, Clone, Default)]
pub struct PruneReport {
    pub dry_run: bool,
    pub archived: Vec<String>,
    pub skipped_pinned: Vec<String>,
    pub skipped_protected: Vec<String>,
    pub errors: Vec<String>,
}

pub fn format_prune_report(report: &PruneReport, archive_after_days: u32) -> String {
    let mode = if report.dry_run {
        "DRY-RUN (no changes)"
    } else {
        "APPLIED"
    };
    let mut lines = vec![format!(
        "Curator prune [{mode}] — archive threshold: {archive_after_days} days idle"
    )];
    if report.archived.is_empty()
        && report.skipped_pinned.is_empty()
        && report.skipped_protected.is_empty()
    {
        lines.push("Nothing to archive.".into());
    }
    if !report.archived.is_empty() {
        let verb = if report.dry_run {
            "Would archive"
        } else {
            "Archived"
        };
        lines.push(format!("{verb} ({}):", report.archived.len()));
        for name in &report.archived {
            lines.push(format!("  {name}"));
        }
    }
    if !report.skipped_pinned.is_empty() {
        lines.push(format!(
            "Skipped pinned ({}): {}",
            report.skipped_pinned.len(),
            report.skipped_pinned.join(", ")
        ));
    }
    if !report.skipped_protected.is_empty() {
        lines.push(format!(
            "Skipped bundled/hub ({}): {}",
            report.skipped_protected.len(),
            report.skipped_protected.join(", ")
        ));
    }
    for err in &report.errors {
        lines.push(format!("Error: {err}"));
    }
    lines.join("\n")
}

/// Archive idle agent-created skills past `archive_after_days` (Hermes auto-transition parity).
pub fn prune_idle_skills(
    home: &Path,
    candidates: &[PruneCandidate],
    dry_run: bool,
    prune_builtins: bool,
) -> PruneReport {
    let mut report = PruneReport {
        dry_run,
        ..Default::default()
    };
    for cand in candidates {
        match check_archive_eligibility(home, &cand.name, prune_builtins) {
            ArchiveEligibility::Pinned => {
                report.skipped_pinned.push(cand.name.clone());
            }
            ArchiveEligibility::Protected
            | ArchiveEligibility::Bundled
            | ArchiveEligibility::HubInstalled => {
                report.skipped_protected.push(cand.name.clone());
            }
            ArchiveEligibility::NotFound => {}
            ArchiveEligibility::Eligible => {
                if dry_run {
                    report.archived.push(cand.name.clone());
                } else {
                    match archive_skill(home, &cand.name, prune_builtins) {
                        Ok(_) => report.archived.push(cand.name.clone()),
                        Err(e) => report.errors.push(format!("{}: {e}", cand.name)),
                    }
                }
            }
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_user_skill(home: &Path, slug: &str, name: &str) {
        let dir = home.join("skills").join(slug);
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test\n---\n"),
        )
        .expect("write");
    }

    #[test]
    fn archive_and_restore_roundtrip() {
        let dir = TempDir::new().expect("tmpdir");
        write_user_skill(dir.path(), "my-skill", "My Skill");
        let msg = archive_skill(dir.path(), "My Skill", false).expect("archive");
        assert!(msg.contains("Archived"));
        assert!(find_skill_dir_public(&dir.path().join("skills"), "My Skill").is_none());
        assert_eq!(list_archived(dir.path()), vec!["my-skill"]);
        restore_skill(dir.path(), "My Skill").expect("restore");
        assert!(find_skill_dir_public(&dir.path().join("skills"), "My Skill").is_some());
    }

    #[test]
    fn refuses_bundled_skill() {
        let dir = TempDir::new().expect("tmpdir");
        write_user_skill(dir.path(), "bundled-one", "Bundled One");
        std::fs::write(
            dir.path().join("skills/.bundled_manifest"),
            "Bundled One:abc123\n",
        )
        .expect("manifest");
        assert_eq!(
            check_archive_eligibility(dir.path(), "Bundled One", false),
            ArchiveEligibility::Bundled
        );
    }

    #[test]
    fn dry_run_prune_lists_without_moving() {
        let dir = TempDir::new().expect("tmpdir");
        write_user_skill(dir.path(), "old", "Old Skill");
        let candidates = vec![PruneCandidate {
            name: "Old Skill".into(),
            slug: "old".into(),
            idle_days: 100,
        }];
        let report = prune_idle_skills(dir.path(), &candidates, true, false);
        assert_eq!(report.archived, vec!["Old Skill"]);
        assert!(find_skill_dir_public(&dir.path().join("skills"), "Old Skill").is_some());
    }

    #[test]
    fn protected_plan_not_archivable() {
        assert_eq!(
            check_archive_eligibility(
                &tempfile::TempDir::new().expect("tmpdir").path(),
                "plan",
                true
            ),
            ArchiveEligibility::Protected
        );
    }
}
