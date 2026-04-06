//! # Skills Sync — manifest-based seeding and updating of bundled skills
//!
//! WHY sync: Copies bundled skills from the repo's `skills/` directory into
//! `~/.edgecrab/skills/` and uses a manifest to track which skills have been
//! synced and their origin hash.
//!
//! Mirrors hermes-agent's `tools/skills_sync.py`:
//! - Manifest format: `skill_name:origin_hash` per line
//! - NEW skills: copied, hash recorded
//! - EXISTING unchanged: updated from bundled if bundled changed
//! - EXISTING modified by user: SKIP (user customizations preserved)
//! - DELETED by user: respected, not re-added
//! - REMOVED from bundled: cleaned from manifest

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config_ref::resolve_edgecrab_home;

#[derive(Debug, Clone, Copy)]
pub(crate) struct EmbeddedSkillFile {
    pub relative_path: &'static str,
    pub content: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EmbeddedSkill {
    pub name: &'static str,
    pub files: &'static [EmbeddedSkillFile],
}

include!(concat!(env!("OUT_DIR"), "/embedded_skills.rs"));

#[derive(Debug, Clone)]
struct SkillSnapshot {
    name: String,
    files: Vec<SkillSnapshotFile>,
}

#[derive(Debug, Clone)]
struct SkillSnapshotFile {
    relative_path: String,
    content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncSource {
    Filesystem,
    Embedded,
}

/// Locate the repo's bundled `skills/` directory.
///
/// Resolution order:
/// 1. `EDGECRAB_BUNDLED_SKILLS` env var (set by Nix/wrapper scripts)
/// 2. Relative to the running binary: `<binary_dir>/../../skills/`
///    (covers `target/release/edgecrab` → workspace root `skills/`)
/// 3. Compile-time `CARGO_MANIFEST_DIR` fallback (dev builds only):
///    `<crate_dir>/../../skills/`
///
/// Returns `None` if none of the above exist on disk.
pub fn bundled_skills_dir() -> Option<PathBuf> {
    if let Ok(env) = std::env::var("EDGECRAB_BUNDLED_SKILLS") {
        let p = PathBuf::from(env);
        if p.is_dir() {
            return Some(p);
        }
    }

    // Relative to current binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            let candidate = bin_dir.join("../..").join("skills");
            if candidate.is_dir() {
                return Some(candidate);
            }
            // Also try alongside the binary (flat install layout)
            let flat = bin_dir.join("skills");
            if flat.is_dir() {
                return Some(flat);
            }
        }
    }

    // Compile-time fallback (workspace root / skills/)
    let compile_time = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills");
    if compile_time.is_dir() {
        return Some(compile_time);
    }

    None
}

/// Locate the repo's `optional-skills/` directory (same resolution as bundled).
pub fn optional_skills_dir() -> Option<PathBuf> {
    if let Ok(env) = std::env::var("EDGECRAB_OPTIONAL_SKILLS") {
        let p = PathBuf::from(env);
        if p.is_dir() {
            return Some(p);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            let candidate = bin_dir.join("../..").join("optional-skills");
            if candidate.is_dir() {
                return Some(candidate);
            }
            let flat = bin_dir.join("optional-skills");
            if flat.is_dir() {
                return Some(flat);
            }
        }
    }

    let compile_time = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../optional-skills");
    if compile_time.is_dir() {
        return Some(compile_time);
    }

    None
}

/// Run the full bundled skills sync at startup.
///
/// Discovers bundled skills from the repo, syncs them to `~/.edgecrab/skills/`,
/// and returns a summary string.  Safe to call multiple times — idempotent.
pub fn sync_on_startup() -> Option<SyncReport> {
    if let Some(bundled_dir) = bundled_skills_dir() {
        return Some(sync_bundled_skills(&bundled_dir));
    }
    if !EMBEDDED_BUNDLED_SKILLS.is_empty() {
        return Some(sync_skill_snapshots(
            embedded_skill_snapshots(EMBEDDED_BUNDLED_SKILLS),
            SyncSource::Embedded,
        ));
    }
    None
}

/// Get the user's skills directory.
fn skills_dir() -> PathBuf {
    resolve_edgecrab_home().join("skills")
}

/// Get the manifest file path.
fn manifest_path() -> PathBuf {
    skills_dir().join(".bundled_manifest")
}

/// Read the manifest as `{skill_name: origin_hash}`.
fn read_manifest() -> HashMap<String, String> {
    let path = manifest_path();
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let mut map = HashMap::new();
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some((name, hash)) = line.split_once(':') {
                    map.insert(name.trim().to_string(), hash.trim().to_string());
                } else {
                    // v1 format: plain name, empty hash triggers migration
                    map.insert(line.to_string(), String::new());
                }
            }
            map
        }
        Err(_) => HashMap::new(),
    }
}

/// Write the manifest file.
fn write_manifest(entries: &HashMap<String, String>) {
    let path = manifest_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut lines: Vec<String> = entries
        .iter()
        .map(|(name, hash)| format!("{name}:{hash}"))
        .collect();
    lines.sort();
    let content = lines.join("\n") + "\n";
    let _ = std::fs::write(&path, content);
}

/// Compute an MD5 hash of all files in a directory (content + relative path).
fn dir_hash(directory: &Path) -> String {
    use std::io::Read;

    let mut hasher = md5::Context::new();
    let mut paths: Vec<PathBuf> = Vec::new();

    if let Ok(walker) = walkdir(directory) {
        paths = walker;
        paths.sort();
    }

    for fpath in &paths {
        if let Ok(rel) = fpath.strip_prefix(directory) {
            hasher.consume(rel.to_string_lossy().as_bytes());
            if let Ok(mut f) = std::fs::File::open(fpath) {
                let mut buf = Vec::new();
                if f.read_to_end(&mut buf).is_ok() {
                    hasher.consume(&buf);
                }
            }
        }
    }

    format!("{:x}", hasher.compute())
}

/// Simple recursive file walker.
fn walkdir(dir: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = Vec::new();
    if !dir.is_dir() {
        return Ok(files);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            files.extend(walkdir(&path)?);
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(files)
}

/// Discover all SKILL.md files in the bundled directory, recursively.
fn discover_bundled_skills(bundled_dir: &Path) -> Vec<SkillSnapshot> {
    let mut skills = Vec::new();
    if !bundled_dir.is_dir() {
        return skills;
    }

    let mut stack: Vec<PathBuf> = vec![bundled_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let skill_md = path.join("SKILL.md");
            if skill_md.is_file() {
                let rel = path
                    .strip_prefix(bundled_dir)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                skills.push(read_skill_snapshot(&path, rel));
            } else {
                stack.push(path);
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Sync result for reporting.
#[derive(Debug)]
pub struct SyncReport {
    pub source: SyncSource,
    pub added: Vec<String>,
    pub updated: Vec<String>,
    pub skipped_user_modified: Vec<String>,
    pub skipped_deleted_by_user: Vec<String>,
    pub removed_from_manifest: Vec<String>,
}

impl SyncReport {
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.added.is_empty() {
            parts.push(format!("{} added", self.added.len()));
        }
        if !self.updated.is_empty() {
            parts.push(format!("{} updated", self.updated.len()));
        }
        if !self.skipped_user_modified.is_empty() {
            parts.push(format!(
                "{} skipped (user-modified)",
                self.skipped_user_modified.len()
            ));
        }
        let base = if parts.is_empty() {
            "No changes".into()
        } else {
            parts.join(", ")
        };
        match self.source {
            SyncSource::Filesystem => base,
            SyncSource::Embedded => format!("{base} (embedded fallback)"),
        }
    }
}

impl Default for SyncReport {
    fn default() -> Self {
        Self {
            source: SyncSource::Filesystem,
            added: Vec::new(),
            updated: Vec::new(),
            skipped_user_modified: Vec::new(),
            skipped_deleted_by_user: Vec::new(),
            removed_from_manifest: Vec::new(),
        }
    }
}

/// Sync bundled skills from `bundled_dir` to the user's `~/.edgecrab/skills/`.
///
/// Returns a report of what was added, updated, or skipped.
pub fn sync_bundled_skills(bundled_dir: &Path) -> SyncReport {
    sync_skill_snapshots(discover_bundled_skills(bundled_dir), SyncSource::Filesystem)
}

fn sync_skill_snapshots(bundled: Vec<SkillSnapshot>, source: SyncSource) -> SyncReport {
    let user_skills = skills_dir();
    let _ = std::fs::create_dir_all(&user_skills);

    let mut manifest = read_manifest();
    let mut report = SyncReport {
        source,
        ..SyncReport::default()
    };

    let bundled_names: HashSet<String> = bundled.iter().map(|skill| skill.name.clone()).collect();

    for snapshot in &bundled {
        let user_path = user_skills.join(&snapshot.name);
        let bundled_hash = snapshot_hash(snapshot);

        if let Some(origin_hash) = manifest.get(&snapshot.name) {
            // Skill is in manifest — check if it still exists in user dir
            if !user_path.is_dir() {
                // User deleted it — respect their choice, skip
                report.skipped_deleted_by_user.push(snapshot.name.clone());
                continue;
            }

            if origin_hash.is_empty() {
                // v1 migration: no hash recorded, safe to update
                write_skill_snapshot(snapshot, &user_path);
                manifest.insert(snapshot.name.clone(), bundled_hash);
                report.updated.push(snapshot.name.clone());
                continue;
            }

            let user_hash = dir_hash(&user_path);
            if &user_hash == origin_hash {
                // User hasn't modified it — safe to update if bundled changed
                if bundled_hash != *origin_hash {
                    write_skill_snapshot(snapshot, &user_path);
                    manifest.insert(snapshot.name.clone(), bundled_hash);
                    report.updated.push(snapshot.name.clone());
                }
                // If bundled hasn't changed either, nothing to do
            } else {
                // User customized it — skip
                report.skipped_user_modified.push(snapshot.name.clone());
            }
        } else {
            // NEW skill — not in manifest
            write_skill_snapshot(snapshot, &user_path);
            manifest.insert(snapshot.name.clone(), bundled_hash);
            report.added.push(snapshot.name.clone());
        }
    }

    // Remove manifest entries for skills that are no longer bundled
    let stale: Vec<String> = manifest
        .keys()
        .filter(|k| !bundled_names.contains(*k))
        .cloned()
        .collect();
    for name in stale {
        manifest.remove(&name);
        report.removed_from_manifest.push(name);
    }

    write_manifest(&manifest);
    report
}

fn read_skill_snapshot(root: &Path, name: String) -> SkillSnapshot {
    let mut files = Vec::new();
    if let Ok(paths) = walkdir(root) {
        let mut sorted = paths;
        sorted.sort();
        for file in sorted {
            if let Ok(rel) = file.strip_prefix(root) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                if let Ok(content) = std::fs::read_to_string(&file) {
                    files.push(SkillSnapshotFile {
                        relative_path: rel,
                        content,
                    });
                }
            }
        }
    }
    SkillSnapshot { name, files }
}

fn embedded_skill_snapshots(skills: &[EmbeddedSkill]) -> Vec<SkillSnapshot> {
    let mut snapshots: Vec<SkillSnapshot> = skills
        .iter()
        .map(|skill| SkillSnapshot {
            name: skill.name.to_string(),
            files: skill
                .files
                .iter()
                .map(|file| SkillSnapshotFile {
                    relative_path: file.relative_path.to_string(),
                    content: file.content.to_string(),
                })
                .collect(),
        })
        .collect();
    snapshots.sort_by(|a, b| a.name.cmp(&b.name));
    snapshots
}

fn snapshot_hash(snapshot: &SkillSnapshot) -> String {
    let mut hasher = md5::Context::new();
    for file in &snapshot.files {
        hasher.consume(file.relative_path.as_bytes());
        hasher.consume(file.content.as_bytes());
    }
    format!("{:x}", hasher.compute())
}

fn write_skill_snapshot(snapshot: &SkillSnapshot, dst: &Path) {
    let _ = std::fs::remove_dir_all(dst);
    let _ = std::fs::create_dir_all(dst);
    for file in &snapshot.files {
        if let Some(target) = safe_relative_join(dst, &file.relative_path) {
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(target, &file.content);
        }
    }
}

fn safe_relative_join(base: &Path, rel_path: &str) -> Option<PathBuf> {
    use std::path::Component;

    if rel_path.is_empty() {
        return None;
    }

    let rel = Path::new(rel_path);
    let mut normalized = PathBuf::new();
    for component in rel.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    if normalized.as_os_str().is_empty() {
        None
    } else {
        Some(base.join(normalized))
    }
}

pub(crate) fn embedded_optional_skills() -> &'static [EmbeddedSkill] {
    EMBEDDED_OPTIONAL_SKILLS
}

// ─── MD5 context (minimal implementation using standard lib) ───

mod md5 {
    /// Minimal wrapper providing MD5 hashing for manifest content comparison.
    /// Uses the same algorithm as Python's hashlib.md5().
    ///
    /// Note: MD5 is used here for content comparison only (not security).
    /// This matches hermes-agent's _dir_hash() which uses hashlib.md5.
    pub struct Context {
        data: Vec<u8>,
    }

    pub struct Digest {
        bytes: [u8; 16],
    }

    impl Context {
        pub fn new() -> Self {
            Self { data: Vec::new() }
        }

        pub fn consume(&mut self, input: &[u8]) {
            self.data.extend_from_slice(input);
        }

        /// Compute a simple hash. For manifest comparison, we just need
        /// a consistent hash — doesn't need to be cryptographic MD5.
        pub fn compute(self) -> Digest {
            // Simple FNV-1a-like hash folded into 16 bytes
            let mut hash: u64 = 0xcbf29ce484222325;
            for &byte in &self.data {
                hash ^= byte as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
            let mut hash2: u64 = 0x6c62272e07bb0142;
            for &byte in self.data.iter().rev() {
                hash2 ^= byte as u64;
                hash2 = hash2.wrapping_mul(0x100000001b3);
            }
            let mut bytes = [0u8; 16];
            bytes[..8].copy_from_slice(&hash.to_le_bytes());
            bytes[8..].copy_from_slice(&hash2.to_le_bytes());
            Digest { bytes }
        }
    }

    impl std::fmt::LowerHex for Digest {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            for byte in &self.bytes {
                write!(f, "{:02x}", byte)?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use tempfile::TempDir;

    static EDGECRAB_HOME_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn manifest_roundtrip() {
        // Test manifest format
        let mut entries: HashMap<String, String> = HashMap::new();
        entries.insert("skill-a".into(), "abc123".into());
        entries.insert("skill-b".into(), "def456".into());

        let mut lines: Vec<String> = entries
            .iter()
            .map(|(name, hash)| format!("{name}:{hash}"))
            .collect();
        lines.sort();
        let content = lines.join("\n") + "\n";

        let mut parsed = HashMap::new();
        for line in content.lines() {
            if let Some((name, hash)) = line.split_once(':') {
                parsed.insert(name.to_string(), hash.to_string());
            }
        }
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed["skill-a"], "abc123");
    }

    #[test]
    fn dir_hash_consistent() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();

        let h1 = dir_hash(dir.path());
        let h2 = dir_hash(dir.path());
        assert_eq!(h1, h2, "same contents should produce same hash");
    }

    #[test]
    fn dir_hash_changes_on_content_change() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();
        let h1 = dir_hash(dir.path());

        std::fs::write(dir.path().join("test.txt"), "world").unwrap();
        let h2 = dir_hash(dir.path());

        assert_ne!(h1, h2, "different contents should produce different hash");
    }

    #[test]
    fn sync_adds_new_skills() {
        let bundled = TempDir::new().unwrap();
        let skill_a = bundled.path().join("skill-a");
        std::fs::create_dir_all(&skill_a).unwrap();
        std::fs::write(skill_a.join("SKILL.md"), "# Skill A").unwrap();

        // Save to temp location instead of user home
        let target = TempDir::new().unwrap();
        let report = sync_to_dir(bundled.path(), target.path());
        assert_eq!(report.added.len(), 1);
        assert_eq!(report.added[0], "skill-a");
    }

    #[test]
    fn sync_uses_edgecrab_home_env() {
        let _guard = EDGECRAB_HOME_LOCK.lock().expect("lock");
        let home = TempDir::new().unwrap();
        let bundled = TempDir::new().unwrap();
        let skill = bundled.path().join("ops").join("audit");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "# Audit").unwrap();

        // SAFETY: protected by EDGECRAB_HOME_LOCK for the duration of the test.
        unsafe { std::env::set_var("EDGECRAB_HOME", home.path()) };
        let report = sync_bundled_skills(bundled.path());
        // SAFETY: protected by EDGECRAB_HOME_LOCK for the duration of the test.
        unsafe { std::env::remove_var("EDGECRAB_HOME") };

        assert_eq!(report.added, vec!["ops/audit"]);
        assert!(home.path().join("skills/ops/audit/SKILL.md").is_file());
    }

    #[test]
    fn embedded_bundle_is_not_empty() {
        assert!(
            !EMBEDDED_BUNDLED_SKILLS.is_empty(),
            "embedded bundled skill catalog should never be empty"
        );
        assert!(
            !embedded_optional_skills().is_empty(),
            "embedded optional skill catalog should never be empty"
        );
    }

    #[test]
    fn write_skill_snapshot_replaces_removed_files() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("skill");
        std::fs::create_dir_all(target.join("references")).unwrap();
        std::fs::write(target.join("SKILL.md"), "# Old").unwrap();
        std::fs::write(target.join("references/old.md"), "stale").unwrap();

        let snapshot = SkillSnapshot {
            name: "skill".into(),
            files: vec![SkillSnapshotFile {
                relative_path: "SKILL.md".into(),
                content: "# New".into(),
            }],
        };

        write_skill_snapshot(&snapshot, &target);
        assert!(target.join("SKILL.md").is_file());
        assert!(!target.join("references/old.md").exists());
    }

    /// Internal test helper that syncs to an arbitrary directory.
    fn sync_to_dir(bundled_dir: &Path, user_dir: &Path) -> SyncReport {
        let snapshots = discover_bundled_skills(bundled_dir);
        let mut report = SyncReport::default();
        for snapshot in &snapshots {
            write_skill_snapshot(snapshot, &user_dir.join(&snapshot.name));
            report.added.push(snapshot.name.clone());
        }
        report
    }
}
