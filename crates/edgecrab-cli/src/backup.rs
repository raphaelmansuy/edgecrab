//! # Backup & Import — `edgecrab backup` / `edgecrab import`
//!
//! Creates and restores tar.gz archives of `~/.edgecrab/` state.
//! Credentials (`.env`, `mcp-tokens/`) are always excluded for security.
//! Path traversal and symlink attacks are blocked during import.

use std::path::{Path, Component};

use anyhow::{Context, Result, bail};
use tracing::{info, warn};

/// Files and directories excluded from backup archives.
const BACKUP_EXCLUDE: &[&str] = &[
    ".env",
    "mcp-tokens",
    "sessions.db",
    "sessions.db-wal",
    "sessions.db-shm",
    "processes.json",
    "profiles",
    "logs",
];

/// Create a backup archive of the EdgeCrab home directory.
pub fn create_backup(
    output_path: Option<&Path>,
    include_sessions: bool,
) -> Result<std::path::PathBuf> {
    let home = edgecrab_core::edgecrab_home();
    if !home.exists() {
        bail!("EdgeCrab home directory does not exist: {}", home.display());
    }

    let out = match output_path {
        Some(p) => p.to_path_buf(),
        None => {
            let date = chrono::Local::now().format("%Y-%m-%d");
            std::env::current_dir()?.join(format!("edgecrab-backup-{date}.tar.gz"))
        }
    };

    let file = std::fs::File::create(&out)
        .with_context(|| format!("Cannot create archive: {}", out.display()))?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);

    let mut file_count = 0u64;
    walk_and_add(&home, &home, &mut archive, include_sessions, &mut file_count)?;

    archive.finish()?;
    info!(path = %out.display(), files = file_count, "Backup archive created");
    Ok(out)
}

fn walk_and_add(
    root: &Path,
    dir: &Path,
    archive: &mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>,
    include_sessions: bool,
    file_count: &mut u64,
) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("Cannot read directory: {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path);

        let name = rel
            .components()
            .next()
            .and_then(|c| match c {
                Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .unwrap_or("");

        // Check exclusions
        if should_exclude(name, include_sessions) {
            continue;
        }

        // Skip symlinks
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            warn!(path = %path.display(), "Skipping symlink in backup");
            continue;
        }

        let archive_path = Path::new("edgecrab").join(rel);

        if meta.is_dir() {
            archive.append_dir(&archive_path, &path)?;
            walk_and_add(root, &path, archive, include_sessions, file_count)?;
        } else if meta.is_file() {
            archive
                .append_path_with_name(&path, &archive_path)
                .with_context(|| format!("Failed to add: {}", path.display()))?;
            *file_count += 1;
        }
    }
    Ok(())
}

fn should_exclude(name: &str, include_sessions: bool) -> bool {
    for excl in BACKUP_EXCLUDE {
        if name == *excl {
            // Allow sessions.db if explicitly included
            if name.starts_with("sessions.db") && include_sessions {
                return false;
            }
            return true;
        }
    }
    false
}

/// Import (restore) from a backup archive.
pub fn import_backup(
    archive_path: &Path,
    target: Option<&Path>,
    dry_run: bool,
    force: bool,
) -> Result<()> {
    if !archive_path.exists() {
        bail!("Archive not found: {}", archive_path.display());
    }

    let target_dir = target
        .map(|p| p.to_path_buf())
        .unwrap_or_else(edgecrab_core::edgecrab_home);

    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("Cannot open archive: {}", archive_path.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    // Atomic: extract to temp dir first, then copy to target
    let temp_dir = tempfile::Builder::new()
        .prefix(".edgecrab-import-")
        .tempdir_in(
            target_dir
                .parent()
                .unwrap_or(Path::new("/tmp")),
        )
        .with_context(|| "Cannot create temp directory for import")?;
    let temp_path = temp_dir.path().to_path_buf();

    let mut extracted = 0u64;
    let mut skipped = 0u64;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let raw_path = entry.path()?.to_path_buf();
        let raw_str = raw_path.to_string_lossy();

        // Security: validate path
        if let Err(msg) = validate_tar_member(&raw_str) {
            warn!(path = %raw_str, reason = %msg, "Rejected archive member");
            skipped += 1;
            continue;
        }

        // Security: reject symlinks
        match entry.header().entry_type() {
            tar::EntryType::Regular | tar::EntryType::Directory => {}
            tar::EntryType::Symlink | tar::EntryType::Link => {
                warn!(path = %raw_str, "Skipping symlink in archive");
                skipped += 1;
                continue;
            }
            _ => {
                warn!(path = %raw_str, "Skipping unknown entry type");
                skipped += 1;
                continue;
            }
        }

        // Skip credential files if they leaked into the archive
        let first_component = raw_path
            .components()
            .nth(1) // skip the "edgecrab/" prefix
            .and_then(|c| match c {
                Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .unwrap_or("");
        if first_component == ".env" || first_component == "mcp-tokens" {
            warn!(path = %raw_str, "Skipping credential file in archive");
            skipped += 1;
            continue;
        }

        // Strip the "edgecrab/" prefix for extraction
        let stripped = raw_path
            .strip_prefix("edgecrab")
            .unwrap_or(&raw_path);

        let dest = temp_path.join(stripped);

        if dry_run {
            println!("  would extract: {}", stripped.display());
            extracted += 1;
            continue;
        }

        if entry.header().entry_type() == tar::EntryType::Directory {
            std::fs::create_dir_all(&dest)?;
        } else {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&dest)?;
            std::io::copy(&mut entry, &mut out_file)?;
        }
        extracted += 1;
    }

    if dry_run {
        println!("\nDry run: {extracted} files would be extracted, {skipped} skipped");
        return Ok(());
    }

    // Copy from temp to target
    copy_tree(&temp_path, &target_dir, force)?;

    info!(
        extracted,
        skipped,
        target = %target_dir.display(),
        "Import complete"
    );
    println!(
        "Import complete: {extracted} files restored to {}, {skipped} skipped",
        target_dir.display()
    );
    println!("Note: .env and mcp-tokens/ are not included — re-add API keys manually.");

    Ok(())
}

fn copy_tree(src: &Path, dst: &Path, force: bool) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let rel = src_path.strip_prefix(src).unwrap_or(&src_path);
        let dst_path = dst.join(rel);

        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            copy_tree(&src_path, &dst_path, force)?;
        } else {
            if dst_path.exists() && !force {
                info!(path = %dst_path.display(), "Skipping existing file (use --force to overwrite)");
                continue;
            }
            if let Some(parent) = dst_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Validate tar member path against traversal attacks.
fn validate_tar_member(member_name: &str) -> Result<(), &'static str> {
    let path = Path::new(member_name);

    // Reject absolute paths
    if path.is_absolute() {
        return Err("absolute path");
    }

    // Reject .. components
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err("parent directory reference (..)");
        }
    }

    // Reject Windows drive letters
    if member_name.len() >= 2 && member_name.as_bytes()[1] == b':' {
        return Err("Windows drive letter");
    }

    Ok(())
}

/// CLI entry point for `edgecrab backup`.
pub fn run_backup(output: Option<&str>, include_sessions: bool) -> Result<()> {
    let out = output.map(Path::new);
    let path = create_backup(out, include_sessions)?;
    println!("Backup created: {}", path.display());
    Ok(())
}

/// CLI entry point for `edgecrab import`.
pub fn run_import(archive: &str, target: Option<&str>, dry_run: bool, force: bool) -> Result<()> {
    let archive_path = Path::new(archive);
    let target_path = target.map(Path::new);
    import_backup(archive_path, target_path, dry_run, force)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Mutex to serialize tests that mutate the EDGECRAB_HOME env var.
    /// Env vars are process-global, so parallel mutation causes races.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn setup_fake_home() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let home = tmp.path();
        std::fs::create_dir_all(home.join("memories")).unwrap();
        std::fs::create_dir_all(home.join("skills")).unwrap();
        std::fs::write(home.join("config.yaml"), "model: test\n").unwrap();
        std::fs::write(home.join("memories/MEMORY.md"), "# Memory\n").unwrap();
        std::fs::write(home.join(".env"), "SECRET_KEY=abc123\n").unwrap();
        std::fs::create_dir_all(home.join("mcp-tokens")).unwrap();
        std::fs::write(home.join("mcp-tokens/server.json"), "{}").unwrap();
        tmp
    }

    #[test]
    fn validate_tar_member_normal_path() {
        assert!(validate_tar_member("edgecrab/config.yaml").is_ok());
    }

    #[test]
    fn validate_tar_member_traversal_rejected() {
        assert!(validate_tar_member("../../../etc/passwd").is_err());
    }

    #[test]
    fn validate_tar_member_absolute_rejected() {
        assert!(validate_tar_member("/etc/shadow").is_err());
    }

    #[test]
    fn validate_tar_member_windows_drive_rejected() {
        assert!(validate_tar_member("C:\\Users\\exploit").is_err());
    }

    #[test]
    fn backup_excludes_env() {
        assert!(should_exclude(".env", false));
    }

    #[test]
    fn backup_excludes_mcp_tokens() {
        assert!(should_exclude("mcp-tokens", false));
    }

    #[test]
    fn backup_excludes_sessions_by_default() {
        assert!(should_exclude("sessions.db", false));
    }

    #[test]
    fn backup_includes_sessions_when_flag_set() {
        assert!(!should_exclude("sessions.db", true));
    }

    #[test]
    fn backup_does_not_exclude_normal_files() {
        assert!(!should_exclude("config.yaml", false));
        assert!(!should_exclude("memories", false));
        assert!(!should_exclude("skills", false));
    }

    /// Roundtrip: backup → import → files match.
    #[test]
    fn backup_restore_roundtrip() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let fake_home = setup_fake_home();
        // Point edgecrab home at the fake dir
        unsafe { std::env::set_var("EDGECRAB_HOME", fake_home.path()) };

        let archive_dir = TempDir::new().unwrap();
        let archive_path = archive_dir.path().join("roundtrip.tar.gz");
        let result = create_backup(Some(&archive_path), false);
        assert!(result.is_ok(), "create_backup failed: {result:?}");
        assert!(archive_path.exists());

        // Import to a fresh target
        let restore_dir = TempDir::new().unwrap();
        let res = import_backup(&archive_path, Some(restore_dir.path()), false, false);
        assert!(res.is_ok(), "import_backup failed: {res:?}");

        // config.yaml and memories should be restored
        assert!(restore_dir.path().join("config.yaml").exists());
        assert!(restore_dir.path().join("memories/MEMORY.md").exists());
        // .env must NOT be restored
        assert!(!restore_dir.path().join(".env").exists());

        unsafe { std::env::remove_var("EDGECRAB_HOME") };
    }

    /// Dry-run import must not write any files.
    #[test]
    fn import_dry_run_no_writes() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let fake_home = setup_fake_home();
        unsafe { std::env::set_var("EDGECRAB_HOME", fake_home.path()) };

        let archive_dir = TempDir::new().unwrap();
        let archive_path = archive_dir.path().join("dryrun.tar.gz");
        create_backup(Some(&archive_path), false).unwrap();

        let restore_dir = TempDir::new().unwrap();
        let res = import_backup(&archive_path, Some(restore_dir.path()), true, false);
        assert!(res.is_ok());

        // Nothing should be written in dry-run mode
        let entries: Vec<_> = std::fs::read_dir(restore_dir.path())
            .unwrap()
            .flatten()
            .collect();
        assert!(entries.is_empty(), "dry-run should not write files");

        unsafe { std::env::remove_var("EDGECRAB_HOME") };
    }

    /// Import without --force skips existing files.
    #[test]
    fn import_conflict_skip_default() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let fake_home = setup_fake_home();
        unsafe { std::env::set_var("EDGECRAB_HOME", fake_home.path()) };

        let archive_dir = TempDir::new().unwrap();
        let archive_path = archive_dir.path().join("conflict.tar.gz");
        create_backup(Some(&archive_path), false).unwrap();

        let restore_dir = TempDir::new().unwrap();
        // Pre-create a file that will conflict
        std::fs::write(restore_dir.path().join("config.yaml"), "original\n").unwrap();

        let res = import_backup(&archive_path, Some(restore_dir.path()), false, false);
        assert!(res.is_ok());

        // Original file should be preserved (not overwritten)
        let content = std::fs::read_to_string(restore_dir.path().join("config.yaml")).unwrap();
        assert_eq!(content, "original\n");

        unsafe { std::env::remove_var("EDGECRAB_HOME") };
    }

    /// Import with --force overwrites existing files.
    #[test]
    fn import_conflict_force_overwrites() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let fake_home = setup_fake_home();
        unsafe { std::env::set_var("EDGECRAB_HOME", fake_home.path()) };

        let archive_dir = TempDir::new().unwrap();
        let archive_path = archive_dir.path().join("force.tar.gz");
        create_backup(Some(&archive_path), false).unwrap();

        let restore_dir = TempDir::new().unwrap();
        std::fs::write(restore_dir.path().join("config.yaml"), "original\n").unwrap();

        let res = import_backup(&archive_path, Some(restore_dir.path()), false, true);
        assert!(res.is_ok());

        // File should be overwritten with archive content
        let content = std::fs::read_to_string(restore_dir.path().join("config.yaml")).unwrap();
        assert_eq!(content, "model: test\n");

        unsafe { std::env::remove_var("EDGECRAB_HOME") };
    }

    /// Symlinks in archive are rejected during import.
    #[test]
    fn import_rejects_symlinks_in_archive() {
        // Build a tar.gz with a symlink entry
        let archive_dir = TempDir::new().unwrap();
        let archive_path = archive_dir.path().join("symlink.tar.gz");

        {
            let file = std::fs::File::create(&archive_path).unwrap();
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut builder = tar::Builder::new(encoder);

            // Add a regular file
            let data = b"hello";
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder
                .append_data(&mut header, "edgecrab/good.txt", &data[..])
                .unwrap();

            // Add a symlink entry
            let mut sym_header = tar::Header::new_gnu();
            sym_header.set_size(0);
            sym_header.set_entry_type(tar::EntryType::Symlink);
            sym_header.set_mode(0o777);
            sym_header.set_cksum();
            builder
                .append_link(&mut sym_header, "edgecrab/evil", "/etc/passwd")
                .unwrap();

            builder.finish().unwrap();
        }

        let restore_dir = TempDir::new().unwrap();
        let res = import_backup(&archive_path, Some(restore_dir.path()), false, false);
        assert!(res.is_ok());

        // Regular file extracted
        assert!(restore_dir.path().join("good.txt").exists());
        // Symlink must NOT be extracted
        assert!(!restore_dir.path().join("evil").exists());
    }
}
