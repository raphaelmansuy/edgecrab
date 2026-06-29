//! Shared file-backed pending write store for memory and skills subsystems.
//!
//! Hermes parity: `tools/write_approval.py` stages autonomous writes under
//! `<home>/pending/{memory,skills}/<id>.json`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const SUBSYSTEM_MEMORY: &str = "memory";
pub const SUBSYSTEM_SKILLS: &str = "skills";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingWriteRecord {
    pub id: String,
    pub subsystem: String,
    pub action: String,
    pub summary: String,
    pub origin: String,
    pub created_at: f64,
    pub payload: serde_json::Value,
}

fn pending_dir(home: &Path, subsystem: &str) -> PathBuf {
    home.join("pending").join(subsystem)
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub fn stage_write(
    home: &Path,
    subsystem: &str,
    payload: serde_json::Value,
    summary: &str,
    origin: &str,
    action: &str,
) -> PendingWriteRecord {
    let id = Uuid::new_v4().simple().to_string()[..8].to_string();
    let record = PendingWriteRecord {
        id: id.clone(),
        subsystem: subsystem.to_string(),
        action: action.to_string(),
        summary: summary.trim().to_string(),
        origin: if origin.is_empty() {
            "foreground".into()
        } else {
            origin.to_string()
        },
        created_at: now_ts(),
        payload,
    };
    let dir = pending_dir(home, subsystem);
    if std::fs::create_dir_all(&dir).is_ok() {
        let path = dir.join(format!("{id}.json"));
        let tmp = dir.join(format!("{id}.json.tmp"));
        if serde_json::to_string_pretty(&record)
            .ok()
            .and_then(|text| std::fs::write(&tmp, text).ok())
            .is_some()
        {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
    record
}

pub fn list_pending(home: &Path, subsystem: &str) -> Vec<PendingWriteRecord> {
    let dir = pending_dir(home, subsystem);
    let mut records = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Ok(text) = std::fs::read_to_string(&path)
                && let Ok(record) = serde_json::from_str::<PendingWriteRecord>(&text)
            {
                records.push(record);
            }
        }
    }
    records.sort_by(|a, b| {
        a.created_at
            .partial_cmp(&b.created_at)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    records
}

pub fn get_pending(home: &Path, subsystem: &str, id: &str) -> Option<PendingWriteRecord> {
    let path = pending_dir(home, subsystem).join(format!("{id}.json"));
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn discard_pending(home: &Path, subsystem: &str, id: &str) -> bool {
    let path = pending_dir(home, subsystem).join(format!("{id}.json"));
    if path.exists() {
        std::fs::remove_file(path).is_ok()
    } else {
        false
    }
}

pub fn format_pending_list(home: &Path, subsystem: &str) -> String {
    let records = list_pending(home, subsystem);
    if records.is_empty() {
        return format!("No pending {subsystem} writes.");
    }
    let mut lines = vec![format!("Pending {subsystem} writes ({}):", records.len())];
    for r in records {
        let tag = if r.origin == "background_review" {
            " [auto]"
        } else {
            ""
        };
        lines.push(format!("  {}{tag}  {}", r.id, r.summary));
    }
    lines.push(String::new());
    lines.push(format!(
        "Apply: /{subsystem} approve <id>   Reject: /{subsystem} reject <id>"
    ));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stage_list_discard_roundtrip() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let payload = json!({"action": "add", "content": "hello"});
        let record = stage_write(
            dir.path(),
            SUBSYSTEM_MEMORY,
            payload,
            "add entry",
            "foreground",
            "add",
        );
        assert_eq!(list_pending(dir.path(), SUBSYSTEM_MEMORY).len(), 1);
        assert!(get_pending(dir.path(), SUBSYSTEM_MEMORY, &record.id).is_some());
        assert!(discard_pending(dir.path(), SUBSYSTEM_MEMORY, &record.id));
        assert!(list_pending(dir.path(), SUBSYSTEM_MEMORY).is_empty());
    }
}
