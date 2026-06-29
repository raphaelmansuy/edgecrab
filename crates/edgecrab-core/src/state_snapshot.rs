//! Quick state snapshots — Hermes `hermes_cli/backup.py` parity.
//!
//! Lightweight copy of critical `~/.edgecrab/` files under `state-snapshots/`
//! for `/snapshot create|restore|list|prune`. Distinct from full `edgecrab backup`
//! tar.gz (which excludes sessions by default).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::config::edgecrab_home;

const SNAPSHOTS_DIR: &str = "state-snapshots";
const DEFAULT_KEEP: usize = 20;
pub const PRE_UPDATE_SNAPSHOT_LABEL: &str = "pre-update";

/// Critical state paths relative to `~/.edgecrab/` (files or directories).
const QUICK_STATE_PATHS: &[&str] = &[
    "config.yaml",
    "sessions.db",
    "auth.json",
    ".anthropic_oauth.json",
    "cron/jobs.json",
    "command_allowlist.json",
    "pairing",
    "skills/.hub/lock.json",
    "skills/.hub/taps.json",
    "skills/.hub/guard_approvals.json",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub id: String,
    pub timestamp: String,
    #[serde(default)]
    pub label: Option<String>,
    pub file_count: usize,
    pub total_size: u64,
    pub files: BTreeMap<String, u64>,
}

fn snapshot_root(home: Option<&Path>) -> PathBuf {
    home_path(home).join(SNAPSHOTS_DIR)
}

fn home_path(home: Option<&Path>) -> PathBuf {
    home.map(Path::to_path_buf).unwrap_or_else(edgecrab_home)
}

fn safe_copy_db(src: &Path, dst: &Path) -> bool {
    fs::copy(src, dst).is_ok()
}

fn copy_state_entry(home: &Path, snap_dir: &Path, rel: &str, manifest: &mut BTreeMap<String, u64>) {
    let src = home.join(rel);
    if !src.exists() {
        return;
    }

    if src.is_dir() {
        let Ok(walk) = fs::read_dir(&src) else {
            return;
        };
        for entry in walk.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Ok(rel_path) = path.strip_prefix(home) else {
                continue;
            };
            let rel_posix = rel_path.to_string_lossy().replace('\\', "/");
            let dst = snap_dir.join(rel_path);
            if let Some(parent) = dst.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if fs::copy(&path, &dst).is_ok()
                && let Ok(meta) = dst.metadata()
            {
                manifest.insert(rel_posix, meta.len());
            }
        }
        return;
    }

    if !src.is_file() {
        return;
    }

    let dst = snap_dir.join(rel);
    if let Some(parent) = dst.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let copied = if src.extension().is_some_and(|e| e == "db") {
        safe_copy_db(&src, &dst)
    } else {
        fs::copy(&src, &dst).is_ok()
    };

    if copied && let Ok(meta) = dst.metadata() {
        manifest.insert(rel.to_string(), meta.len());
    }
}

/// Create a quick state snapshot. Returns snapshot id or None if nothing copied.
pub fn create_quick_snapshot(label: Option<&str>, home: Option<&Path>) -> io::Result<Option<String>> {
    let id = create_quick_snapshot_inner(label, home)?;
    if id.is_some() {
        prune_quick_snapshots(DEFAULT_KEEP, home)?;
    }
    Ok(id)
}

/// Pre-update safety snapshot — Hermes `create_quick_snapshot(label="pre-update", keep=1)`.
///
/// Creates a labeled snapshot and prunes older pre-update snapshots only (other snapshots
/// are untouched).
pub fn create_pre_update_snapshot(keep: usize, home: Option<&Path>) -> io::Result<Option<String>> {
    let id = create_quick_snapshot_inner(Some(PRE_UPDATE_SNAPSHOT_LABEL), home)?;
    if id.is_some() {
        prune_labeled_snapshots(PRE_UPDATE_SNAPSHOT_LABEL, keep, home)?;
    }
    Ok(id)
}

fn create_quick_snapshot_inner(label: Option<&str>, home: Option<&Path>) -> io::Result<Option<String>> {
    let home = home_path(home);
    let root = snapshot_root(Some(home.as_path()));
    fs::create_dir_all(&root)?;

    let ts = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let snap_id = match label.filter(|l| !l.trim().is_empty()) {
        Some(l) => format!("{ts}-{}", slugify_label(l)),
        None => ts.clone(),
    };
    let snap_dir = root.join(&snap_id);
    fs::create_dir_all(&snap_dir)?;

    let mut manifest = BTreeMap::new();
    for rel in QUICK_STATE_PATHS {
        copy_state_entry(&home, &snap_dir, rel, &mut manifest);
    }

    if manifest.is_empty() {
        let _ = fs::remove_dir_all(&snap_dir);
        return Ok(None);
    }

    let total_size: u64 = manifest.values().sum();
    let meta = SnapshotManifest {
        id: snap_id.clone(),
        timestamp: ts,
        label: label.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string),
        file_count: manifest.len(),
        total_size,
        files: manifest,
    };
    let manifest_path = snap_dir.join("manifest.json");
    let json = serde_json::to_string_pretty(&meta).map_err(io::Error::other)?;
    fs::write(&manifest_path, json + "\n")?;

    Ok(Some(snap_id))
}

fn snapshot_matches_label(manifest: &SnapshotManifest, label_slug: &str) -> bool {
    manifest
        .label
        .as_deref()
        .map(slugify_label)
        .is_some_and(|slug| slug == label_slug)
        || manifest.id.ends_with(&format!("-{label_slug}"))
}

/// Prune snapshots matching a label slug, keeping the newest `keep` entries.
pub fn prune_labeled_snapshots(label: &str, keep: usize, home: Option<&Path>) -> io::Result<usize> {
    let slug = slugify_label(label);
    let snaps = list_quick_snapshots(usize::MAX, home);
    let matching: Vec<_> = snaps
        .into_iter()
        .filter(|s| snapshot_matches_label(s, &slug))
        .collect();
    let mut deleted = 0usize;
    for snap in matching.into_iter().skip(keep) {
        let dir = snapshot_root(home).join(&snap.id);
        if fs::remove_dir_all(&dir).is_ok() {
            deleted += 1;
        }
    }
    Ok(deleted)
}

pub fn list_quick_snapshots(limit: usize, home: Option<&Path>) -> Vec<SnapshotManifest> {
    let root = snapshot_root(home);
    if !root.is_dir() {
        return Vec::new();
    }

    let mut dirs: Vec<_> = fs::read_dir(&root)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .collect();
    dirs.sort_by_key(|e| e.file_name());
    dirs.reverse();

    let mut out = Vec::new();
    for entry in dirs.into_iter().take(limit) {
        let manifest_path = entry.path().join("manifest.json");
        if manifest_path.is_file()
            && let Ok(text) = fs::read_to_string(&manifest_path)
            && let Ok(parsed) = serde_json::from_str::<SnapshotManifest>(&text)
        {
            out.push(parsed);
        } else {
            out.push(SnapshotManifest {
                id: entry.file_name().to_string_lossy().into_owned(),
                timestamp: String::new(),
                label: None,
                file_count: 0,
                total_size: 0,
                files: BTreeMap::new(),
            });
        }
    }
    out
}

pub fn restore_quick_snapshot(snapshot_id: &str, home: Option<&Path>) -> io::Result<usize> {
    let home = home_path(home);
    let snap_dir = snapshot_root(Some(home.as_path())).join(snapshot_id);
    if !snap_dir.is_dir() {
        return Ok(0);
    }

    let manifest_path = snap_dir.join("manifest.json");
    let text = fs::read_to_string(&manifest_path)?;
    let meta: SnapshotManifest = serde_json::from_str(&text).map_err(io::Error::other)?;

    let mut restored = 0usize;
    for rel in meta.files.keys() {
        let src = snap_dir.join(rel);
        if !src.is_file() {
            continue;
        }
        let dst = home.join(rel);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }

        let ok = if dst.extension().is_some_and(|e| e == "db") {
            let tmp = dst.with_extension("db.snap_restore");
            let copied = safe_copy_db(&src, &tmp);
            if copied {
                let _ = fs::remove_file(&dst);
                fs::rename(&tmp, &dst).is_ok()
            } else {
                false
            }
        } else {
            fs::copy(&src, &dst).is_ok()
        };

        if ok {
            restored += 1;
        }
    }
    Ok(restored)
}

pub fn prune_quick_snapshots(keep: usize, home: Option<&Path>) -> io::Result<usize> {
    let root = snapshot_root(home);
    if !root.is_dir() {
        return Ok(0);
    }

    let mut dirs: Vec<_> = fs::read_dir(&root)?
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    dirs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

    let mut deleted = 0usize;
    for dir in dirs.into_iter().skip(keep) {
        if fs::remove_dir_all(&dir).is_ok() {
            deleted += 1;
        }
    }
    Ok(deleted)
}

fn slugify_label(label: &str) -> String {
    label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else if c.is_whitespace() {
                '-'
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(48)
        .collect()
}

fn format_size(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{} KB", n / 1024)
    } else {
        format!("{:.1} MB", n as f64 / 1024.0 / 1024.0)
    }
}

fn resolve_snapshot_id<'a>(snaps: &'a [SnapshotManifest], token: &str) -> Option<&'a str> {
    if let Ok(idx) = token.parse::<usize>()
        && idx >= 1
        && idx <= snaps.len()
    {
        return Some(&snaps[idx - 1].id);
    }
    snaps
        .iter()
        .find(|s| s.id == token || s.id.starts_with(token))
        .map(|s| s.id.as_str())
}

/// Shared `/snapshot` slash handler (CLI + gateway).
pub fn handle_snapshot_slash(args: &str, home: Option<&Path>) -> String {
    let home = home_path(home);
    let tokens: Vec<&str> = args.split_whitespace().collect();
    let sub = tokens.first().map(|s| s.to_ascii_lowercase());
    let sub = sub.as_deref().unwrap_or("list");

    match sub {
        "list" | "ls" | "" => {
            let snaps = list_quick_snapshots(20, Some(home.as_path()));
            if snaps.is_empty() {
                return "No state snapshots yet.\nCreate one: /snapshot create [label]".into();
            }
            let mut out = format!(
                "State snapshots ({}):\n\n",
                home.join(SNAPSHOTS_DIR).display()
            );
            out.push_str(&format!(
                "  {:>3}  {:<35} {:>5} {:>10} Label\n",
                "#", "ID", "Files", "Size"
            ));
            out.push_str(&format!(
                "  {:>3}  {:<35} {:>5} {:>10} {}\n",
                "─", "─", "─", "─", "─"
            ));
            for (i, s) in snaps.iter().enumerate() {
                let label = s.label.as_deref().unwrap_or("");
                out.push_str(&format!(
                    "  {:>3}  {:<35} {:>5} {:>10} {label}\n",
                    i + 1,
                    s.id,
                    s.file_count,
                    format_size(s.total_size)
                ));
            }
            out.push_str("\nRestore: /snapshot restore <id-or-#>");
            out
        }
        "create" | "save" => {
            let label = if tokens.len() > 1 {
                Some(tokens[1..].join(" "))
            } else {
                None
            };
            match create_quick_snapshot(label.as_deref(), Some(home.as_path())) {
                Ok(Some(id)) => format!("Snapshot created: {id}"),
                Ok(None) => "No state files found to snapshot.".into(),
                Err(e) => format!("Snapshot failed: {e}"),
            }
        }
        "restore" | "rewind" => {
            let Some(token) = tokens.get(1) else {
                let mut msg = "Usage: /snapshot restore <snapshot-id-or-#>".to_string();
                if let Some(first) = list_quick_snapshots(1, Some(home.as_path())).first() {
                    msg.push_str(&format!("\nMost recent: {}", first.id));
                }
                return msg;
            };
            let snaps = list_quick_snapshots(50, Some(home.as_path()));
            let Some(id) = resolve_snapshot_id(&snaps, token) else {
                return format!("Snapshot not found: {token}");
            };
            match restore_quick_snapshot(id, Some(home.as_path())) {
                Ok(0) => format!("Snapshot not found or empty: {id}"),
                Ok(n) => format!(
                    "Restored {n} file(s) from: {id}\n\
                     Restart recommended for sessions.db changes to take effect."
                ),
                Err(e) => format!("Restore failed: {e}"),
            }
        }
        "prune" => {
            let keep = tokens
                .get(1)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(DEFAULT_KEEP);
            match prune_quick_snapshots(keep, Some(home.as_path())) {
                Ok(n) => format!("Pruned {n} old snapshot(s) (keeping {keep})."),
                Err(e) => format!("Prune failed: {e}"),
            }
        }
        other => format!(
            "Unknown subcommand: {other}\n\
             Usage: /snapshot [list|create [label]|restore <id>|prune [N]]"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_home() -> TempDir {
        let dir = TempDir::new().expect("tmpdir");
        fs::write(dir.path().join("config.yaml"), "model: test\n").expect("write");
        fs::create_dir_all(dir.path().join("pairing")).expect("mkdir");
        fs::write(
            dir.path().join("pairing/telegram-approved.json"),
            "{}",
        )
        .expect("write");
        dir
    }

    #[test]
    fn labeled_prune_respects_non_matching_snapshots() {
        let home = setup_home();
        create_quick_snapshot(Some("pre-update"), Some(home.path())).expect("pre");
        create_quick_snapshot(Some("manual-backup"), Some(home.path())).expect("other");
        assert_eq!(list_quick_snapshots(10, Some(home.path())).len(), 2);
        let deleted = prune_labeled_snapshots("pre-update", 0, Some(home.path())).expect("prune");
        assert_eq!(deleted, 1);
        let left = list_quick_snapshots(10, Some(home.path()));
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].label.as_deref(), Some("manual-backup"));
    }

    #[test]
    fn create_pre_update_uses_label() {
        let home = setup_home();
        let id = create_pre_update_snapshot(1, Some(home.path()))
            .expect("create")
            .expect("id");
        assert!(id.contains("pre-update"));
    }

    #[test]
    fn create_list_restore_roundtrip() {
        let home = setup_home();
        let id = create_quick_snapshot(Some("pre-update"), Some(home.path()))
            .expect("create")
            .expect("id");
        assert!(id.contains("pre-update"));

        let snaps = list_quick_snapshots(10, Some(home.path()));
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].file_count, 2);

        fs::write(home.path().join("config.yaml"), "model: changed\n").expect("write");
        let restored = restore_quick_snapshot(&id, Some(home.path())).expect("restore");
        assert!(restored >= 1);
        let content = fs::read_to_string(home.path().join("config.yaml")).expect("read");
        assert_eq!(content, "model: test\n");
    }

    #[test]
    fn slash_handler_list_and_create() {
        let home = setup_home();
        let listed = handle_snapshot_slash("", Some(home.path()));
        assert!(listed.contains("No state snapshots"));

        let created = handle_snapshot_slash("create test", Some(home.path()));
        assert!(created.contains("Snapshot created"));

        let listed2 = handle_snapshot_slash("list", Some(home.path()));
        assert!(listed2.contains("test"));
    }

    #[test]
    fn restore_by_index() {
        let home = setup_home();
        create_quick_snapshot(None, Some(home.path())).expect("create");
        fs::write(home.path().join("config.yaml"), "x\n").expect("write");
        let msg = handle_snapshot_slash("restore 1", Some(home.path()));
        assert!(msg.contains("Restored"));
    }
}
