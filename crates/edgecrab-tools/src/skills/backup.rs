//! Pre-curator snapshots + rollback (Hermes `.curator_backups/` parity).

use std::fs::File;
use std::path::{Path, PathBuf};

use chrono::Utc;
use flate2::Compression;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use tar::{Builder, EntryType};

const EXCLUDE_TOP_LEVEL: &[&str] = &[".curator_backups", ".hub"];
const CRON_JOBS_FILENAME: &str = "cron-jobs.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CronBackupInfo {
    pub backed_up: bool,
    #[serde(default)]
    pub jobs_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub id: String,
    pub reason: String,
    pub created_at: String,
    pub archive_path: String,
    pub file_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cron_jobs: Option<CronBackupInfo>,
}

pub fn backups_root(home: &Path) -> PathBuf {
    home.join("skills").join(".curator_backups")
}

fn skills_root(home: &Path) -> PathBuf {
    home.join("skills")
}

fn should_include(rel: &Path) -> bool {
    let Some(first) = rel.components().next() else {
        return false;
    };
    let name = first.as_os_str().to_string_lossy();
    !EXCLUDE_TOP_LEVEL.contains(&name.as_ref())
}

/// Tar.gz snapshot of `~/.edgecrab/skills/` (excludes `.hub`, `.curator_backups`).
pub fn snapshot_skills(home: &Path, reason: &str) -> Result<SnapshotManifest, String> {
    let skills = skills_root(home);
    if !skills.is_dir() {
        return Err("skills directory missing".into());
    }
    let base_id = Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string();
    let mut id = base_id.clone();
    let mut counter = 1u32;
    while backups_root(home).join(&id).exists() {
        id = format!("{base_id}-{counter:02}");
        counter += 1;
    }
    let dest_dir = backups_root(home).join(&id);
    std::fs::create_dir_all(&dest_dir).map_err(|e| format!("mkdir backup: {e}"))?;
    let archive_path = dest_dir.join("skills.tar.gz");

    let file = File::create(&archive_path).map_err(|e| format!("create archive: {e}"))?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut builder = Builder::new(enc);

    let mut file_count = 0u64;
    for entry in walkdir_light(&skills)? {
        let rel = entry
            .strip_prefix(&skills)
            .map_err(|e| format!("path: {e}"))?;
        if !should_include(rel) {
            continue;
        }
        let meta = entry
            .metadata()
            .map_err(|e| format!("metadata {}: {e}", entry.display()))?;
        if meta.is_dir() {
            builder
                .append_dir(rel, &entry)
                .map_err(|e| format!("tar dir {}: {e}", entry.display()))?;
        } else if meta.is_file() {
            builder
                .append_path_with_name(&entry, rel)
                .map_err(|e| format!("tar file {}: {e}", entry.display()))?;
            file_count += 1;
        }
    }
    builder.finish().map_err(|e| format!("finish tar: {e}"))?;

    let cron_info = backup_cron_jobs_into(&dest_dir, home);

    let manifest = SnapshotManifest {
        id: id.clone(),
        reason: reason.to_string(),
        created_at: Utc::now().to_rfc3339(),
        archive_path: archive_path.display().to_string(),
        file_count,
        cron_jobs: Some(cron_info),
    };
    let manifest_path = dest_dir.join("manifest.json");
    let text = serde_json::to_string_pretty(&manifest).map_err(|e| format!("json: {e}"))?;
    std::fs::write(&manifest_path, text).map_err(|e| format!("write manifest: {e}"))?;
    Ok(manifest)
}

fn walkdir_light(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries =
            std::fs::read_dir(&dir).map_err(|e| format!("readdir {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("entry: {e}"))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path.clone());
            }
            out.push(path);
        }
    }
    Ok(out)
}

pub fn list_snapshots(home: &Path) -> Vec<SnapshotManifest> {
    let root = backups_root(home);
    if !root.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        let manifest_path = entry.path().join("manifest.json");
        if manifest_path.is_file()
            && let Ok(text) = std::fs::read_to_string(&manifest_path)
            && let Ok(m) = serde_json::from_str::<SnapshotManifest>(&text)
        {
            out.push(m);
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    out
}

pub fn format_backup_list(home: &Path) -> String {
    let snaps = list_snapshots(home);
    if snaps.is_empty() {
        return "No curator backups. Snapshots are taken before mutating /curator prune runs."
            .into();
    }
    let mut lines = vec![format!("Curator backups ({}):", snaps.len())];
    for s in snaps {
        let cron_note = s
            .cron_jobs
            .as_ref()
            .filter(|c| c.backed_up)
            .map(|c| format!(", cron: {} jobs", c.jobs_count))
            .unwrap_or_default();
        lines.push(format!(
            "  {} — {} ({} files{}) [{}]",
            s.id, s.created_at, s.file_count, cron_note, s.reason
        ));
    }
    lines.push(String::new());
    lines.push("Rollback: /curator rollback <id>".into());
    lines.join("\n")
}

pub fn prune_old_backups(home: &Path, keep: u32) {
    let keep = keep.max(1) as usize;
    let snaps = list_snapshots(home);
    for s in snaps.into_iter().skip(keep) {
        let dir = backups_root(home).join(&s.id);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Restore skills tree from a snapshot (current tree saved as pre-rollback snapshot first).
pub fn rollback_snapshot(home: &Path, snapshot_id: &str) -> Result<String, String> {
    let id = snapshot_id.trim();
    let snap_dir = backups_root(home).join(id);
    let archive = snap_dir.join("skills.tar.gz");
    if !archive.is_file() {
        return Err(format!("backup '{id}' not found"));
    }

    let _ = snapshot_skills(home, "pre-rollback");

    let skills = skills_root(home);
    // Move aside mutable top-level entries (except backup/hub dirs).
    let aside = backups_root(home).join(format!("aside-{}", Utc::now().format("%Y%m%d%H%M%S")));
    std::fs::create_dir_all(&aside).map_err(|e| format!("aside mkdir: {e}"))?;
    if skills.is_dir() {
        for entry in std::fs::read_dir(&skills).map_err(|e| format!("readdir: {e}"))? {
            let entry = entry.map_err(|e| format!("entry: {e}"))?;
            let name = entry.file_name().to_string_lossy().to_string();
            if EXCLUDE_TOP_LEVEL.contains(&name.as_str()) {
                continue;
            }
            let src = entry.path();
            let dst = aside.join(&name);
            std::fs::rename(&src, &dst).or_else(|_| {
                copy_dir_recursive(&src, &dst)?;
                if src.is_dir() {
                    std::fs::remove_dir_all(&src)
                } else {
                    std::fs::remove_file(&src)
                }
                .map_err(|e| format!("cleanup {}: {e}", src.display()))
            })?;
        }
    }

    extract_tar_gz(&archive, &skills)?;
    let cron_summary = restore_cron_skill_links(&snap_dir, home);
    Ok(format!(
        "Rolled back skills from backup '{id}'{cron_summary}"
    ))
}

fn cron_jobs_file(home: &Path) -> PathBuf {
    home.join("cron").join("jobs.json")
}

/// Copy live `cron/jobs.json` into snapshot dir (additive; never fails snapshot).
fn backup_cron_jobs_into(dest: &Path, home: &Path) -> CronBackupInfo {
    let src = cron_jobs_file(home);
    if !src.is_file() {
        return CronBackupInfo {
            backed_up: false,
            jobs_count: 0,
            reason: Some("no cron/jobs.json present".into()),
        };
    }
    let text = match std::fs::read_to_string(&src) {
        Ok(t) => t,
        Err(e) => {
            return CronBackupInfo {
                backed_up: false,
                jobs_count: 0,
                reason: Some(format!("read cron/jobs.json: {e}")),
            };
        }
    };
    let jobs_count = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| {
            if let Some(arr) = v.get("jobs").and_then(|j| j.as_array()) {
                Some(arr.len() as u32)
            } else {
                v.as_array().map(|arr| arr.len() as u32)
            }
        })
        .unwrap_or(0);
    let dst = dest.join(CRON_JOBS_FILENAME);
    if std::fs::write(&dst, text).is_err() {
        return CronBackupInfo {
            backed_up: false,
            jobs_count: 0,
            reason: Some("failed to write cron-jobs.json into snapshot".into()),
        };
    }
    CronBackupInfo {
        backed_up: true,
        jobs_count,
        reason: None,
    }
}

#[derive(Debug, Default)]
struct CronRestoreReport {
    restored: u32,
    unchanged: u32,
    skipped_missing: u32,
    error: Option<String>,
}

/// Surgical restore of cron job `skills` fields from snapshot (Hermes parity).
fn restore_cron_skill_links(snapshot_dir: &Path, home: &Path) -> String {
    let backup_file = snapshot_dir.join(CRON_JOBS_FILENAME);
    if !backup_file.is_file() {
        return String::new();
    }
    match restore_cron_skill_links_inner(&backup_file, home) {
        Ok(report) => format_cron_restore_summary(&report),
        Err(e) => format!("; cron links: error — {e}"),
    }
}

fn format_cron_restore_summary(report: &CronRestoreReport) -> String {
    if report.error.is_some() {
        return format!(
            "; cron links: error — {}",
            report.error.as_deref().unwrap_or("")
        );
    }
    if report.restored == 0 && report.skipped_missing == 0 && report.unchanged == 0 {
        return String::new();
    }
    let mut parts = Vec::new();
    if report.restored > 0 {
        parts.push(format!("{} restored", report.restored));
    }
    if report.unchanged > 0 {
        parts.push(format!("{} unchanged", report.unchanged));
    }
    if report.skipped_missing > 0 {
        parts.push(format!(
            "{} skipped (deleted since snapshot)",
            report.skipped_missing
        ));
    }
    format!("; cron links: {}", parts.join(", "))
}

fn restore_cron_skill_links_inner(
    backup_file: &Path,
    home: &Path,
) -> Result<CronRestoreReport, String> {
    let text =
        std::fs::read_to_string(backup_file).map_err(|e| format!("load backed-up jobs: {e}"))?;
    let parsed: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("parse backed-up jobs: {e}"))?;
    let backup_jobs = parsed
        .get("jobs")
        .and_then(|v| v.as_array())
        .or_else(|| parsed.as_array())
        .ok_or_else(|| "backed-up cron-jobs.json has no jobs list".to_string())?
        .clone();

    let mut backup_by_id: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for job in &backup_jobs {
        let Some(id) = job
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        backup_by_id.insert(id.to_string(), normalize_skills_field(job));
    }
    if backup_by_id.is_empty() {
        return Ok(CronRestoreReport::default());
    }

    let cron_path = cron_jobs_file(home);
    if !cron_path.is_file() {
        return Ok(CronRestoreReport {
            error: Some("live cron/jobs.json missing".into()),
            ..Default::default()
        });
    }

    let live_text =
        std::fs::read_to_string(&cron_path).map_err(|e| format!("read live cron: {e}"))?;
    let mut live_parsed: serde_json::Value =
        serde_json::from_str(&live_text).map_err(|e| format!("parse live cron: {e}"))?;
    let live_jobs = live_parsed
        .get_mut("jobs")
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| "live cron/jobs.json has no jobs list".to_string())?;

    let mut report = CronRestoreReport::default();
    let mut live_ids = std::collections::HashSet::new();

    for job in live_jobs.iter_mut() {
        let Some(id) = job
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        live_ids.insert(id.to_string());
        let Some(backup_skills) = backup_by_id.get(id) else {
            continue;
        };
        let current = normalize_skills_field(job);
        if &current == backup_skills {
            report.unchanged += 1;
            continue;
        }
        if backup_skills.is_empty() {
            job.as_object_mut().map(|m| m.remove("skills"));
        } else {
            job["skills"] = serde_json::json!(backup_skills);
        }
        job.as_object_mut().map(|m| m.remove("skill"));
        report.restored += 1;
    }

    for id in backup_by_id.keys() {
        if !live_ids.contains(id) {
            report.skipped_missing += 1;
        }
    }

    if report.restored > 0 {
        let out = serde_json::to_string_pretty(&live_parsed)
            .map_err(|e| format!("serialize cron: {e}"))?;
        let tmp = cron_path.with_extension("json.tmp");
        std::fs::write(&tmp, &out).map_err(|e| format!("write cron tmp: {e}"))?;
        std::fs::rename(&tmp, &cron_path).map_err(|e| format!("rename cron: {e}"))?;
    }
    Ok(report)
}

fn normalize_skills_field(job: &serde_json::Value) -> Vec<String> {
    if let Some(arr) = job.get("skills").and_then(|v| v.as_array()) {
        return arr
            .iter()
            .filter_map(|v| v.as_str().map(str::trim).filter(|s| !s.is_empty()))
            .map(str::to_string)
            .collect();
    }
    job.get("skill")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| vec![s.to_string()])
        .unwrap_or_default()
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("mkdir: {e}"))?;
    for entry in walkdir_light(src)? {
        let rel = entry.strip_prefix(src).map_err(|e| format!("rel: {e}"))?;
        let target = dst.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| format!("mkdir: {e}"))?;
        } else if entry.is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
            }
            std::fs::copy(&entry, &target).map_err(|e| format!("copy: {e}"))?;
        }
    }
    Ok(())
}

fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("mkdir dest: {e}"))?;
    let file = File::open(archive).map_err(|e| format!("open archive: {e}"))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    for entry in archive.entries().map_err(|e| format!("entries: {e}"))? {
        let mut entry = entry.map_err(|e| format!("entry: {e}"))?;
        let path = entry
            .path()
            .map_err(|e| format!("path: {e}"))?
            .to_path_buf();
        if path
            .components()
            .any(|c| c.as_os_str() == ".hub" || c.as_os_str() == ".curator_backups")
        {
            continue;
        }
        let out_path = dest.join(&path);
        match entry.header().entry_type() {
            EntryType::Directory => {
                std::fs::create_dir_all(&out_path).map_err(|e| format!("mkdir: {e}"))?;
            }
            EntryType::Regular => {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
                }
                let mut out = File::create(&out_path)
                    .map_err(|e| format!("create {}: {e}", out_path.display()))?;
                std::io::copy(&mut entry, &mut out).map_err(|e| format!("extract: {e}"))?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Snapshot before a mutating curator pass when backups are enabled.
pub fn maybe_snapshot_before_mutate(
    home: &Path,
    enabled: bool,
    keep: u32,
    reason: &str,
) -> Option<String> {
    if !enabled {
        return None;
    }
    match snapshot_skills(home, reason) {
        Ok(m) => {
            prune_old_backups(home, keep);
            Some(format!("Backup {} ({} files)", m.id, m.file_count))
        }
        Err(e) => Some(format!("Backup skipped: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn snapshot_roundtrip_list() {
        let dir = TempDir::new().expect("tmpdir");
        let skills = dir.path().join("skills").join("demo");
        std::fs::create_dir_all(&skills).expect("mkdir");
        std::fs::write(skills.join("SKILL.md"), "---\nname: demo\n---\n").expect("write");
        let m = snapshot_skills(dir.path(), "test").expect("snap");
        assert!(m.file_count >= 1);
        assert_eq!(list_snapshots(dir.path()).len(), 1);
    }

    #[test]
    fn snapshot_includes_cron_when_present() {
        let dir = TempDir::new().expect("tmpdir");
        let skills = dir.path().join("skills").join("demo");
        std::fs::create_dir_all(&skills).expect("mkdir");
        std::fs::write(skills.join("SKILL.md"), "---\nname: demo\n---\n").expect("write");
        let cron_dir = dir.path().join("cron");
        std::fs::create_dir_all(&cron_dir).expect("mkdir cron");
        std::fs::write(
            cron_dir.join("jobs.json"),
            r#"{"jobs":[{"id":"j1","name":"daily","prompt":"go","skills":["demo"],"schedule":{"kind":"interval","minutes":60},"schedule_display":"every 1h","state":"scheduled","enabled":true,"repeat":{"completed":0},"deliver":"local","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","run_count":0}]}"#,
        )
        .expect("write cron");
        let m = snapshot_skills(dir.path(), "test").expect("snap");
        assert!(
            m.cron_jobs
                .as_ref()
                .is_some_and(|c| c.backed_up && c.jobs_count == 1)
        );
        assert!(
            backups_root(dir.path())
                .join(&m.id)
                .join(CRON_JOBS_FILENAME)
                .is_file()
        );
    }

    #[test]
    fn rollback_restores_cron_skill_links() {
        let dir = TempDir::new().expect("tmpdir");
        let skills = dir.path().join("skills").join("demo");
        std::fs::create_dir_all(&skills).expect("mkdir");
        std::fs::write(skills.join("SKILL.md"), "---\nname: demo\n---\n").expect("write");
        let cron_dir = dir.path().join("cron");
        std::fs::create_dir_all(&cron_dir).expect("mkdir cron");
        std::fs::write(
            cron_dir.join("jobs.json"),
            r#"{"jobs":[{"id":"j1","name":"daily","prompt":"go","skills":["demo"],"schedule":{"kind":"interval","minutes":60},"schedule_display":"every 1h","state":"scheduled","enabled":true,"repeat":{"completed":0},"deliver":"local","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","run_count":0}]}"#,
        )
        .expect("write cron");
        let m = snapshot_skills(dir.path(), "test").expect("snap");
        // Mutate live cron to point at a consolidated skill name.
        std::fs::write(
            cron_dir.join("jobs.json"),
            r#"{"jobs":[{"id":"j1","name":"daily","prompt":"go","skills":["umbrella-skill"],"schedule":{"kind":"interval","minutes":60},"schedule_display":"every 1h","state":"scheduled","enabled":true,"repeat":{"completed":0},"deliver":"local","created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z","run_count":0}]}"#,
        )
        .expect("mutate cron");
        let msg = rollback_snapshot(dir.path(), &m.id).expect("rollback");
        assert!(msg.contains("Rolled back"));
        let live: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(cron_dir.join("jobs.json")).expect("read"),
        )
        .expect("parse");
        assert_eq!(live["jobs"][0]["skills"], serde_json::json!(["demo"]));
    }
}
