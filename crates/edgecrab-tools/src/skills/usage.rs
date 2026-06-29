//! Skill usage telemetry — sidecar `~/.edgecrab/skills/.usage.json`.
//!
//! Best-effort counters for slash invocations and skill_manage patches.
//! Foundation for future curator without mutating SKILL.md frontmatter.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageRecord {
    #[serde(default)]
    pub use_count: u64,
    #[serde(default)]
    pub last_used_at: Option<String>,
    #[serde(default)]
    pub view_count: u64,
    #[serde(default)]
    pub last_viewed_at: Option<String>,
    #[serde(default)]
    pub patch_count: u64,
    #[serde(default)]
    pub last_patched_at: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub archived: bool,
}

type UsageStore = HashMap<String, UsageRecord>;

fn usage_path(home: &Path) -> PathBuf {
    home.join("skills").join(".usage.json")
}

fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

fn read_store(home: &Path) -> UsageStore {
    let path = usage_path(home);
    if !path.is_file() {
        return HashMap::new();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_store(home: &Path, store: &UsageStore) {
    let path = usage_path(home);
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(text) = serde_json::to_string_pretty(store) else {
        return;
    };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

fn mutate<F>(home: &Path, skill_name: &str, mutator: F)
where
    F: FnOnce(&mut UsageRecord),
{
    let key = skill_name.trim();
    if key.is_empty() {
        return;
    }
    let mut store = read_store(home);
    let entry = store.entry(key.to_string()).or_default();
    mutator(entry);
    write_store(home, &store);
}

/// Record an active skill load (slash invocation or explicit preload).
pub fn bump_use(home: &Path, skill_name: &str) {
    mutate(home, skill_name, |rec| {
        rec.use_count = rec.use_count.saturating_add(1);
        rec.last_used_at = Some(now_iso());
        rec.archived = false;
    });
}

/// Record skill_view / preload read.
pub fn bump_view(home: &Path, skill_name: &str) {
    mutate(home, skill_name, |rec| {
        rec.view_count = rec.view_count.saturating_add(1);
        rec.last_viewed_at = Some(now_iso());
    });
}

/// Latest touch across slash invoke, view, or patch.
pub fn last_activity_at(rec: &UsageRecord) -> Option<&str> {
    let mut best: Option<&str> = None;
    for ts in [
        rec.last_used_at.as_deref(),
        rec.last_viewed_at.as_deref(),
        rec.last_patched_at.as_deref(),
    ] {
        let Some(ts) = ts else { continue };
        if best.is_none_or(|b| ts > b) {
            best = Some(ts);
        }
    }
    best
}

pub fn activity_count(rec: &UsageRecord) -> u64 {
    rec.use_count
        .saturating_add(rec.view_count)
        .saturating_add(rec.patch_count)
}

/// Record a skill_manage patch/edit.
pub fn bump_patch(home: &Path, skill_name: &str) {
    mutate(home, skill_name, |rec| {
        rec.patch_count = rec.patch_count.saturating_add(1);
        rec.last_patched_at = Some(now_iso());
    });
}

pub fn set_pinned(home: &Path, skill_name: &str, pinned: bool) -> bool {
    let key = skill_name.trim();
    if key.is_empty() {
        return false;
    }
    mutate(home, skill_name, |rec| rec.pinned = pinned);
    read_store(home)
        .get(key)
        .is_some_and(|r| r.pinned == pinned)
}

pub fn is_pinned(home: &Path, skill_name: &str) -> bool {
    read_store(home)
        .get(skill_name.trim())
        .is_some_and(|r| r.pinned)
}

pub fn set_archived(home: &Path, skill_name: &str, archived: bool) {
    mutate(home, skill_name, |rec| rec.archived = archived);
}

/// Read usage store (for curator stale detection).
pub fn read_store_public(home: &Path) -> HashMap<String, UsageRecord> {
    read_store(home)
}

/// Human-readable summary for `/skills usage`.
pub fn format_usage_summary(home: &Path, limit: usize) -> String {
    let store = read_store(home);
    if store.is_empty() {
        return "No skill usage recorded yet.\n\
                Invoke skills with /skill-name or use skill_view / skill_manage."
            .into();
    }
    let mut rows: Vec<_> = store
        .iter()
        .map(|(name, rec)| {
            (
                name.clone(),
                activity_count(rec),
                rec.use_count,
                rec.view_count,
                last_activity_at(rec).map(str::to_string),
                rec.pinned,
            )
        })
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut lines = vec![format!("Skill usage (top {}):", limit.min(rows.len()))];
    for (name, activity, uses, views, last, pinned) in rows.into_iter().take(limit) {
        let pin_tag = if pinned { " [pinned]" } else { "" };
        let when = last.unwrap_or_else(|| "never".into());
        lines.push(format!(
            "  {name}{pin_tag}: activity={activity} (use={uses}, view={views}, last: {when})"
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn bump_use_persists() {
        let dir = TempDir::new().expect("tmpdir");
        bump_use(dir.path(), "demo");
        bump_use(dir.path(), "demo");
        let store = read_store(dir.path());
        assert_eq!(store.get("demo").expect("demo").use_count, 2);
    }
}
