# ADR-0608: `edgecrab backup` & `edgecrab import`

| Field       | Value                                                   |
|-------------|---------------------------------------------------------|
| Status      | Implemented                                             |
| Date        | 2026-04-14                                              |
| Implements  | hermes-agent PR #7997                                   |
| Crate       | `edgecrab-cli`                                          |
| File        | `crates/edgecrab-cli/src/backup.rs` (NEW)               |

---

## 1. Context

Users need to backup and restore their EdgeCrab configuration, sessions,
skills, and memory before major changes or when migrating between machines.
hermes-agent v0.9.0 ships `hermes backup` / `hermes import` via
`hermes_cli/profiles.py:export_profile()` and `import_profile()`.

EdgeCrab state lives in `~/.edgecrab/`:

```
~/.edgecrab/
  config.yaml            # settings
  .env                   # API keys (EXCLUDED from backup)
  sessions.db            # SQLite session store
  memories/              # MEMORY.md, USER.md
  skills/                # installed skills
  plugins/               # installed plugins
  models.yaml            # user model overrides
  skin.yaml              # theme customization
  mcp-tokens/            # MCP server tokens (EXCLUDED from backup)
```

---

## 2. First Principles

| Principle       | Application                                              |
|-----------------|----------------------------------------------------------|
| **SRP**         | Backup/import logic isolated in `backup.rs`              |
| **Secure by Default** | Credentials EXCLUDED from backup archives         |
| **DRY**         | Reuse tar/gz crate, path validation from security        |
| **Code is Law** | hermes-agent `profiles.py:L770-960` as reference         |

---

## 3. Architecture

```
+-------------------------------------------------------------------+
|                     edgecrab backup                                |
|                                                                    |
|  edgecrab backup [--output <path>]                                 |
|    |                                                               |
|    +-- 1. Resolve EDGECRAB_HOME (~/.edgecrab/)                     |
|    +-- 2. Walk directory, collect files                            |
|    +-- 3. EXCLUDE: .env, mcp-tokens/, sessions.db, processes.json  |
|    +-- 4. Create .tar.gz archive                                   |
|    +-- 5. Write to output path (default: edgecrab-backup-<date>)   |
|                                                                    |
|  edgecrab import <archive> [--name <profile>]                      |
|    |                                                               |
|    +-- 1. Validate archive (tar.gz)                                |
|    +-- 2. Security: path traversal check on every member           |
|    +-- 3. Security: reject symlinks, absolute paths, ..            |
|    +-- 4. Extract to target directory                              |
|    +-- 5. Warn user about missing credentials                      |
+-------------------------------------------------------------------+
```

---

## 4. Data Model

### 4.1 Backup Exclusions

```rust
/// Files and directories excluded from backup archives.
/// Matches hermes-agent's export_profile() exclusion list.
const BACKUP_EXCLUDE: &[&str] = &[
    ".env",              // API keys / secrets
    "mcp-tokens",        // MCP Bearer tokens (chmod 0o600)
    "sessions.db",       // SQLite session store (large, non-portable)
    "sessions.db-wal",   // WAL file
    "sessions.db-shm",   // Shared memory file
    "processes.json",    // Transient process state
    "profiles",          // Sub-profiles (backup individually)
];
```

### 4.2 CLI Subcommands

```
USAGE:
    edgecrab backup [OPTIONS]
    edgecrab import <ARCHIVE> [OPTIONS]

BACKUP OPTIONS:
    --output <PATH>     Output file path (default: ./edgecrab-backup-YYYY-MM-DD.tar.gz)
    --include-sessions  Include sessions.db in backup (default: excluded)

IMPORT OPTIONS:
    --target <PATH>     Target directory (default: ~/.edgecrab/)
    --dry-run           Show what would be extracted without writing
    --force             Overwrite existing files (default: skip conflicts)
```

### 4.3 Archive Format

```
edgecrab-backup-2026-04-14.tar.gz
  edgecrab/
    config.yaml
    models.yaml
    skin.yaml
    memories/
      MEMORY.md
      USER.md
    skills/
      my-skill/
        SKILL.md
    plugins/
      my-plugin/
        manifest.yaml
```

---

## 5. Security: Safe Archive Extraction

### 5.1 Path Traversal Protection

```rust
/// Validate tar member path against traversal attacks.
/// Source: hermes-agent profiles.py:_safe_extract_profile_archive()
fn validate_tar_member(member_name: &str) -> Result<(), BackupError> {
    let path = Path::new(member_name);

    // Reject absolute paths
    if path.is_absolute() {
        return Err(BackupError::PathTraversal("absolute path"));
    }

    // Reject .. components
    for component in path.components() {
        if matches!(component, std::path::Component::ParentDir) {
            return Err(BackupError::PathTraversal("parent directory reference"));
        }
    }

    // Reject Windows drive letters (cross-platform safety)
    if member_name.len() >= 2 && member_name.as_bytes()[1] == b':' {
        return Err(BackupError::PathTraversal("Windows drive letter"));
    }

    // Reject symlinks (checked at extraction time)
    Ok(())
}
```

### 5.2 Symlink Blocking

```rust
// During extraction:
for entry in archive.entries()? {
    let entry = entry?;
    match entry.header().entry_type() {
        tar::EntryType::Regular | tar::EntryType::Directory => {
            // OK — extract
        }
        tar::EntryType::Symlink | tar::EntryType::Link => {
            tracing::warn!(path = ?entry.path(), "Skipping symlink in backup archive");
            continue;  // Skip, don't extract
        }
        _ => {
            tracing::warn!(path = ?entry.path(), "Skipping unknown entry type");
            continue;
        }
    }
}
```

---

## 6. Edge Cases & Roadblocks

| #  | Edge Case                              | Remediation                                         | Source                          |
|----|----------------------------------------|------------------------------------------------------|---------------------------------|
| 1  | Target dir already exists (import)     | `--force` to overwrite; default: skip conflicts      | `profiles.py:import_profile()`  |
| 2  | Archive contains `.env`                | Skip during extraction + warn user                   | Security policy                 |
| 3  | Archive from different version         | Warn about potential config format differences        | New — version check             |
| 4  | Symlink escape in archive              | Reject all symlinks during extraction                | `profiles.py:_safe_extract()`   |
| 5  | Corrupted tar.gz                       | `flate2` and `tar` crate handle gracefully           | Standard error handling         |
| 6  | Disk full during extraction            | Atomic: extract to temp dir first, then rename       | New — crash safety              |
| 7  | Large sessions.db in archive           | `--include-sessions` opt-in only                     | `profiles.py:export_profile()`  |
| 8  | mcp-tokens with chmod 0o600            | Always excluded from both backup and import          | Security: credential isolation  |
| 9  | Relative path in --output              | Resolve to absolute before creating archive          | Standard path handling          |
| 10 | Import as "default" would overwrite    | Block import when target is `~/.edgecrab/` directly  | `profiles.py:import_profile()`  |

---

## 7. Implementation Plan

### 7.1 Files to Create

| File                                     | Purpose                               |
|------------------------------------------|---------------------------------------|
| `crates/edgecrab-cli/src/backup.rs`      | Backup/import subcommand handlers     |

### 7.2 Files to Modify

| File                                     | Change                                |
|------------------------------------------|---------------------------------------|
| `crates/edgecrab-cli/src/main.rs`        | Add `backup` and `import` subcommands |
| `Cargo.toml` (edgecrab-cli)             | Add `tar`, `flate2` dependencies      |

### 7.3 Dependencies

```toml
[dependencies]
tar = "0.4"          # tar archive reading/writing
flate2 = "1"         # gzip compression/decompression
```

### 7.4 Test Matrix

| Test                              | Validates                                      |
|-----------------------------------|-------------------------------------------------|
| `test_backup_creates_archive`     | Archive created with correct structure          |
| `test_backup_excludes_env`        | `.env` not in archive                           |
| `test_backup_excludes_tokens`     | `mcp-tokens/` not in archive                   |
| `test_backup_excludes_sessions`   | `sessions.db` excluded by default              |
| `test_backup_includes_sessions`   | `sessions.db` included with `--include-sessions`|
| `test_import_basic`               | Archive extracted to target directory           |
| `test_import_path_traversal`      | `../../../etc/passwd` in archive rejected       |
| `test_import_absolute_path`       | `/etc/shadow` in archive rejected               |
| `test_import_symlink_rejected`    | Symlink entries skipped                         |
| `test_import_windows_drive`       | `C:\Users\...` path rejected                   |
| `test_import_conflict_skip`       | Existing files skipped without `--force`        |
| `test_import_conflict_force`      | Existing files overwritten with `--force`       |
| `test_import_dry_run`             | No files written in dry-run mode               |
| `test_backup_restore_roundtrip`   | backup → import → files match                  |

---

## 8. Acceptance Criteria

- [ ] `edgecrab backup` creates a `.tar.gz` archive of `~/.edgecrab/`
- [ ] Credentials excluded: `.env`, `mcp-tokens/`, API keys
- [ ] `edgecrab import` extracts archive with path traversal protection
- [ ] Symlinks in archive rejected (not extracted)
- [ ] `--dry-run` shows what would be extracted without writing
- [ ] `--force` flag for overwriting existing files
- [ ] `--include-sessions` opt-in for session database
- [ ] Atomic extraction: temp dir → rename
- [ ] Version warning when config format differs
- [ ] All tests pass: `cargo test -p edgecrab-cli -- backup`
