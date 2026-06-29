//! Portable hub configuration export/import — exceeds Hermes with schema versioning + hashes.

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::guard_approvals::{GuardApproval, read_guard_approvals, record_guard_approval};
use super::{InstallGate, install_identifier, read_lock, read_taps, tap_from_snapshot_value};

const SNAPSHOT_FORMAT_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HubSnapshot {
    #[serde(default = "default_format_version")]
    format_version: u32,
    #[serde(default)]
    edgecrab_version: String,
    #[serde(default)]
    hermes_version: Option<String>,
    #[serde(default)]
    exported_at: String,
    #[serde(default)]
    skills: Vec<HubSnapshotSkill>,
    #[serde(default)]
    taps: Vec<serde_json::Value>,
    /// Hash-bound dangerous-skill trust approvals (EdgeCrab v2+).
    #[serde(default)]
    guard_approvals: HashMap<String, GuardApproval>,
}

fn default_format_version() -> u32 {
    SNAPSHOT_FORMAT_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HubSnapshotSkill {
    name: String,
    #[serde(default)]
    source: String,
    identifier: String,
    #[serde(default)]
    content_hash: String,
}

/// Export hub lock + taps to JSON (Hermes `snapshot export` parity + content hashes).
pub fn export_hub_snapshot(output_path: &str) -> Result<String, String> {
    let lock = read_lock();
    let taps = read_taps();

    let mut skills: Vec<HubSnapshotSkill> = lock
        .into_iter()
        .map(|(name, entry)| HubSnapshotSkill {
            name,
            source: entry.source,
            identifier: entry.identifier,
            content_hash: entry.content_hash,
        })
        .collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));

    let tap_values: Vec<serde_json::Value> = taps
        .iter()
        .map(|t| serde_json::to_value(t).unwrap_or_default())
        .collect();

    let snapshot = HubSnapshot {
        format_version: SNAPSHOT_FORMAT_VERSION,
        edgecrab_version: env!("CARGO_PKG_VERSION").into(),
        hermes_version: None,
        exported_at: Utc::now().to_rfc3339(),
        skills,
        taps: tap_values,
        guard_approvals: read_guard_approvals(),
    };

    let payload = serde_json::to_string_pretty(&snapshot)
        .map_err(|e| format!("serialize failed: {e}"))?
        + "\n";

    if output_path == "-" {
        return Ok(payload);
    }

    std::fs::write(output_path, &payload).map_err(|e| format!("write {}: {e}", output_path))?;
    Ok(format!(
        "Snapshot exported: {output_path}\n{} skill(s), {} tap(s), {} trust approval(s)",
        snapshot.skills.len(),
        snapshot.taps.len(),
        snapshot.guard_approvals.len()
    ))
}

/// Import taps + re-install hub skills from snapshot JSON.
pub async fn import_hub_snapshot(
    input_path: &str,
    force: bool,
    skills_dir: &Path,
    optional_dir: Option<&Path>,
) -> Result<String, String> {
    let content =
        std::fs::read_to_string(input_path).map_err(|e| format!("read {}: {e}", input_path))?;
    let snapshot: HubSnapshot =
        serde_json::from_str(&content).map_err(|e| format!("invalid snapshot JSON: {e}"))?;

    if snapshot.format_version > SNAPSHOT_FORMAT_VERSION {
        return Err(format!(
            "Unsupported snapshot format version {} (max {SNAPSHOT_FORMAT_VERSION})",
            snapshot.format_version
        ));
    }

    let mut out = String::new();
    if snapshot.hermes_version.is_some() {
        out.push_str("Importing Hermes-format snapshot (compatible).\n");
    }

    let mut taps_restored = 0usize;
    for raw in &snapshot.taps {
        if let Some(tap) = tap_from_snapshot_value(raw)
            && super::add_tap_if_missing(&tap)
        {
            taps_restored += 1;
        }
    }
    if taps_restored > 0 {
        out.push_str(&format!("Restored {taps_restored} tap(s).\n"));
    }

    let mut approvals_restored = 0usize;
    for approval in snapshot.guard_approvals.values() {
        if record_guard_approval(
            &approval.identifier,
            &approval.skill_name,
            &approval.content_hash,
            &approval.verdict,
            approval.finding_count,
        )
        .is_ok()
        {
            approvals_restored += 1;
        }
    }
    if approvals_restored > 0 {
        out.push_str(&format!("Restored {approvals_restored} trust approval(s).\n"));
    }

    if snapshot.skills.is_empty() {
        out.push_str("No skills in snapshot to install.");
        return Ok(out);
    }

    out.push_str(&format!(
        "Importing {} skill(s) from snapshot…\n\n",
        snapshot.skills.len()
    ));

    let mut ok = 0usize;
    let mut failed = 0usize;
    for entry in &snapshot.skills {
        if entry.identifier.is_empty() {
            out.push_str(&format!("⚠ Skipping '{}' — no identifier\n", entry.name));
            failed += 1;
            continue;
        }
        out.push_str(&format!("--- {} ---\n", entry.name));
        match install_identifier(
            &entry.identifier,
            skills_dir,
            optional_dir,
            InstallGate {
                force,
                trust: force,
            },
        )
        .await
        {
            Ok(outcome) => {
                out.push_str(&format!("{}\n\n", outcome.message));
                ok += 1;
            }
            Err(e) => {
                out.push_str(&format!("Install failed: {e}\n\n"));
                failed += 1;
            }
        }
    }

    out.push_str(&format!(
        "Snapshot import complete: {ok} installed, {failed} failed/skipped."
    ));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEdgecrabHome;
    use tempfile::TempDir;

    #[test]
    fn export_roundtrip_empty_lock() {
        let home = TestEdgecrabHome::new();
        let _ = home;
        let out = TempDir::new().unwrap();
        let path = out.path().join("snap.json");

        let msg = export_hub_snapshot(path.to_str().unwrap()).expect("export");
        assert!(msg.contains("0 skill"));

        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: HubSnapshot = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.format_version, SNAPSHOT_FORMAT_VERSION);
        assert!(parsed.edgecrab_version.contains('.'));
    }

    #[test]
    fn export_includes_guard_approvals_v2() {
        let home = TestEdgecrabHome::new();
        let _ = home;
        super::super::guard_approvals::record_guard_approval(
            "skills.sh:demo/test",
            "test",
            "sha256:abc",
            "dangerous",
            3,
        )
        .expect("record");

        let out = TempDir::new().unwrap();
        let path = out.path().join("snap.json");
        let msg = export_hub_snapshot(path.to_str().unwrap()).expect("export");
        assert!(msg.contains("trust approval"));

        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: HubSnapshot = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.format_version, SNAPSHOT_FORMAT_VERSION);
        assert_eq!(parsed.guard_approvals.len(), 1);
    }

    #[test]
    fn accepts_hermes_snapshot_shape() {
        let json = r#"{
            "hermes_version": "0.1.0",
            "exported_at": "2026-01-01T00:00:00Z",
            "skills": [{"name": "foo", "identifier": "owner/repo/path", "source": "hub"}],
            "taps": [{"repo": "acme/skills", "path": "skills/"}]
        }"#;
        let parsed: HubSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.skills.len(), 1);
        assert_eq!(parsed.skills[0].identifier, "owner/repo/path");
        let tap = tap_from_snapshot_value(&parsed.taps[0]).expect("tap");
        assert!(tap.url.contains("acme/skills"));
    }
}
