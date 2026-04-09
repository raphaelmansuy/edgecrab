use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::PluginError;

const MANIFEST_NAME: &str = ".bundled_manifest";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundledSyncStatus {
    New,
    Unchanged,
    Customized,
    DeletedByUser,
    RemovedFromBundled,
}

#[derive(Debug, Default)]
pub struct BundledSyncReport {
    pub results: BTreeMap<String, BundledSyncStatus>,
}

pub fn bundled_skills_sync(
    bundled_dir: &Path,
    install_dir: &Path,
) -> Result<BundledSyncReport, PluginError> {
    std::fs::create_dir_all(install_dir)?;

    let manifest_path = install_dir.join(MANIFEST_NAME);
    let mut existing_manifest = read_manifest(&manifest_path)?;
    let bundled = discover_bundled_skills(bundled_dir)?;
    let mut report = BundledSyncReport::default();
    let mut next_manifest = BTreeMap::new();

    for (name, bundled_path) in &bundled {
        let bundled_contents = std::fs::read_to_string(bundled_path)?;
        let bundled_hash = hex_hash(&bundled_contents);
        let destination = install_dir.join(name).join("SKILL.md");

        let status = if destination.exists() {
            let current_hash = hex_hash(&std::fs::read_to_string(&destination)?);
            match existing_manifest.remove(name) {
                Some(previous_hash) if previous_hash == current_hash => {
                    if let Some(parent) = destination.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&destination, &bundled_contents)?;
                    BundledSyncStatus::Unchanged
                }
                Some(_) => BundledSyncStatus::Customized,
                None => BundledSyncStatus::Customized,
            }
        } else if existing_manifest.contains_key(name) {
            existing_manifest.remove(name);
            BundledSyncStatus::DeletedByUser
        } else {
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&destination, &bundled_contents)?;
            BundledSyncStatus::New
        };

        next_manifest.insert(name.clone(), bundled_hash);
        report.results.insert(name.clone(), status);
    }

    for removed in existing_manifest.keys() {
        report
            .results
            .insert(removed.clone(), BundledSyncStatus::RemovedFromBundled);
    }

    write_manifest(&manifest_path, &next_manifest)?;
    Ok(report)
}

fn discover_bundled_skills(root: &Path) -> Result<BTreeMap<String, PathBuf>, PluginError> {
    let mut found = BTreeMap::new();
    if !root.exists() {
        return Ok(found);
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let skill_file = path.join("SKILL.md");
        if skill_file.is_file() {
            found.insert(name.to_string(), skill_file);
        }
    }
    Ok(found)
}

fn read_manifest(path: &Path) -> Result<BTreeMap<String, String>, PluginError> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let content = std::fs::read_to_string(path)?;
    let mut entries = BTreeMap::new();
    for line in content.lines() {
        let Some((name, hash)) = line.split_once(':') else {
            continue;
        };
        entries.insert(name.trim().to_string(), hash.trim().to_string());
    }
    Ok(entries)
}

fn write_manifest(path: &Path, entries: &BTreeMap<String, String>) -> Result<(), PluginError> {
    let mut lines = Vec::with_capacity(entries.len());
    for (name, hash) in entries {
        lines.push(format!("{name}:{hash}"));
    }
    std::fs::write(path, lines.join("\n"))?;
    Ok(())
}

fn hex_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_sync_covers_new_unchanged_customized_deleted_and_removed_cases() {
        let temp = tempfile::tempdir().expect("tempdir");
        let bundled_dir = temp.path().join("bundled");
        let install_dir = temp.path().join("install");
        std::fs::create_dir_all(&bundled_dir).expect("bundled dir");
        std::fs::create_dir_all(&install_dir).expect("install dir");

        write_skill(&bundled_dir.join("new-skill"), "new-skill", "New");
        write_skill(&bundled_dir.join("unchanged"), "unchanged", "Unchanged");
        write_skill(&bundled_dir.join("customized"), "customized", "Bundled");
        write_skill(&bundled_dir.join("deleted"), "deleted", "Deleted");

        write_skill(&install_dir.join("unchanged"), "unchanged", "Old");
        write_skill(&install_dir.join("customized"), "customized", "Customized");
        write_skill(&install_dir.join("removed"), "removed", "Removed");

        let mut old_manifest = BTreeMap::new();
        old_manifest.insert("unchanged".into(), hex_hash(&skill_doc("unchanged", "Old")));
        old_manifest.insert(
            "customized".into(),
            hex_hash(&skill_doc("customized", "Bundled")),
        );
        old_manifest.insert("deleted".into(), hex_hash(&skill_doc("deleted", "Deleted")));
        old_manifest.insert("removed".into(), hex_hash(&skill_doc("removed", "Removed")));
        write_manifest(&install_dir.join(MANIFEST_NAME), &old_manifest).expect("manifest");

        let report = bundled_skills_sync(&bundled_dir, &install_dir).expect("sync");

        assert_eq!(report.results["new-skill"], BundledSyncStatus::New);
        assert_eq!(report.results["unchanged"], BundledSyncStatus::Unchanged);
        assert_eq!(report.results["customized"], BundledSyncStatus::Customized);
        assert_eq!(report.results["deleted"], BundledSyncStatus::DeletedByUser);
        assert_eq!(
            report.results["removed"],
            BundledSyncStatus::RemovedFromBundled
        );
    }

    fn write_skill(dir: &Path, name: &str, description: &str) {
        std::fs::create_dir_all(dir).expect("skill dir");
        std::fs::write(dir.join("SKILL.md"), skill_doc(name, description)).expect("skill write");
    }

    fn skill_doc(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n\n# {description}\n\nBody.\n")
    }
}
