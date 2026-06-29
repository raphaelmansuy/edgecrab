//! Explicit user approvals for dangerous hub skills — hash-bound, audited.
//!
//! First principles: default deny on `dangerous`; `--force` overrides `caution` only;
//! dangerous requires `/skills trust` (review + record) or install with `--trust`.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::config_ref::resolve_edgecrab_home;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardApproval {
    pub identifier: String,
    pub skill_name: String,
    pub content_hash: String,
    pub verdict: String,
    pub finding_count: usize,
    pub approved_at: String,
}

fn approvals_path() -> PathBuf {
    resolve_edgecrab_home()
        .join("skills")
        .join(".hub")
        .join("guard_approvals.json")
}

pub fn read_guard_approvals() -> HashMap<String, GuardApproval> {
    let path = approvals_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => HashMap::new(),
    }
}

fn write_guard_approvals(map: &HashMap<String, GuardApproval>) -> Result<(), String> {
    let path = approvals_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create hub dir: {e}"))?;
    }
    let json =
        serde_json::to_string_pretty(map).map_err(|e| format!("serialize approvals: {e}"))?;
    std::fs::write(&path, json + "\n").map_err(|e| format!("write approvals: {e}"))
}

/// Normalized lookup key (lowercase identifier).
fn approval_key(identifier: &str) -> String {
    identifier.trim().to_lowercase()
}

/// True when user explicitly approved this exact upstream bundle hash.
pub fn is_dangerous_approved(identifier: &str, content_hash: &str) -> bool {
    let key = approval_key(identifier);
    read_guard_approvals()
        .get(&key)
        .is_some_and(|a| a.content_hash == content_hash)
}

pub fn record_guard_approval(
    identifier: &str,
    skill_name: &str,
    content_hash: &str,
    verdict: &str,
    finding_count: usize,
) -> Result<(), String> {
    let key = approval_key(identifier);
    let mut map = read_guard_approvals();
    map.insert(
        key,
        GuardApproval {
            identifier: identifier.to_string(),
            skill_name: skill_name.to_string(),
            content_hash: content_hash.to_string(),
            verdict: verdict.to_string(),
            finding_count,
            approved_at: Utc::now().to_rfc3339(),
        },
    );
    write_guard_approvals(&map)
}

pub fn revoke_guard_approval(identifier: &str) -> bool {
    let key = approval_key(identifier);
    let mut map = read_guard_approvals();
    if map.remove(&key).is_some() {
        let _ = write_guard_approvals(&map);
        true
    } else {
        false
    }
}

pub fn format_guard_approvals_list() -> String {
    let mut entries: Vec<_> = read_guard_approvals().into_values().collect();
    if entries.is_empty() {
        return "No dangerous-skill trust approvals on file.\n\
                After a blocked install, review the scan report then:\n\
                /skills trust <identifier>"
            .into();
    }
    entries.sort_by(|a, b| a.identifier.cmp(&b.identifier));
    let mut out = format!("Dangerous-skill trust approvals ({}):\n\n", entries.len());
    for entry in entries {
        out.push_str(&format!(
            "  {}\n    identifier: {}\n    hash: {}\n    verdict: {} ({} findings) @ {}\n\n",
            entry.skill_name,
            entry.identifier,
            entry.content_hash,
            entry.verdict,
            entry.finding_count,
            entry.approved_at
        ));
    }
    out.push_str("Revoke: /skills untrust <identifier>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEdgecrabHome;

    #[test]
    fn approval_roundtrip_and_hash_binding() {
        let home = TestEdgecrabHome::new();
        let _ = home;

        record_guard_approval("skills.sh:acme/evil", "evil", "sha256:abc", "dangerous", 15)
            .expect("record");

        assert!(is_dangerous_approved("skills.sh:acme/evil", "sha256:abc"));
        assert!(!is_dangerous_approved(
            "skills.sh:acme/evil",
            "sha256:changed"
        ));
        assert!(revoke_guard_approval("skills.sh:acme/evil"));
        assert!(!is_dangerous_approved("skills.sh:acme/evil", "sha256:abc"));
    }
}
