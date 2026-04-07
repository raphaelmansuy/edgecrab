//! # checkpoint — Shadow-git filesystem snapshots for rollback
//!
//! Stores numbered checkpoints in a shadow git repository so the user can
//! do `/rollback N` to restore prior state without polluting the project's own git history.
//!
//! ```text
//!   Shadow repo: ~/.edgecrab/checkpoints/<sha256_of_cwd>/
//!
//!   checkpoint create "before refactor"
//!       ├── sync tracked CWD files → shadow repo
//!       └── git commit -m "before refactor"
//!
//!   checkpoint list
//!       └── git log (1 = newest)
//!
//!   checkpoint restore N
//!       └── copy files from commit N back to CWD
//!
//!   checkpoint diff N
//!       └── show changed files between checkpoint N and current CWD
//!
//!   checkpoint restore_file N <file>
//!       └── restore single file from checkpoint N
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use edgecrab_types::{ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Compute SHA-256 of a string and return the first 16 hex chars.
fn path_hash(p: &Path) -> String {
    let mut h = Sha256::new();
    h.update(p.to_string_lossy().as_bytes());
    format!("{:x}", h.finalize())[..16].to_string()
}

/// Shadow git repo directory for a given working directory.
fn shadow_dir(edgecrab_home: &Path, cwd: &Path) -> PathBuf {
    edgecrab_home.join("checkpoints").join(path_hash(cwd))
}

/// Ensure the shadow git repo exists and is initialised.
fn ensure_shadow_repo(shadow: &Path) -> Result<(), ToolError> {
    std::fs::create_dir_all(shadow).map_err(|e| ToolError::ExecutionFailed {
        tool: "checkpoint".into(),
        message: format!("Failed to create shadow dir: {e}"),
    })?;

    if !shadow.join(".git").exists() {
        let ok = Command::new("git")
            .arg("init")
            .arg("-q")
            .current_dir(shadow)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return Err(ToolError::ExecutionFailed {
                tool: "checkpoint".into(),
                message: "Failed to git-init shadow repo".into(),
            });
        }
        // Set a stable identity so commits succeed in any environment
        for (k, v) in &[("user.email", "edgecrab@local"), ("user.name", "edgecrab")] {
            let _ = Command::new("git")
                .args(["config", k, v])
                .current_dir(shadow)
                .status();
        }
    }
    Ok(())
}

/// Copy tracked files from `cwd` to `shadow_dir`, preserving relative paths.
fn sync_to_shadow(cwd: &Path, shadow: &Path) -> Result<usize, ToolError> {
    let files = collect_files(cwd);

    for rel in &files {
        let src = cwd.join(rel);
        let dst = shadow.join(rel);
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::copy(&src, &dst).map_err(|e| ToolError::ExecutionFailed {
            tool: "checkpoint".into(),
            message: format!("Failed to sync {}: {e}", rel.display()),
        })?;
    }
    Ok(files.len())
}

/// Collect files to snapshot (git-tracked files, or recursive fallback).
fn collect_files(cwd: &Path) -> Vec<PathBuf> {
    if let Ok(out) = Command::new("git")
        .arg("ls-files")
        .current_dir(cwd)
        .output()
    {
        if out.status.success() {
            return String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(PathBuf::from)
                .filter(|p| cwd.join(p).is_file())
                .take(2000)
                .collect();
        }
    }
    let mut v = Vec::new();
    collect_recursive(cwd, cwd, 0, 4, &mut v);
    v.truncate(2000);
    v
}

fn collect_recursive(dir: &Path, root: &Path, depth: usize, max: usize, out: &mut Vec<PathBuf>) {
    if depth >= max {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        if path.is_file() {
            // Store relative path from the root so sync_to_shadow computes correct src/dst
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_path_buf());
            }
        } else if path.is_dir() {
            collect_recursive(&path, root, depth + 1, max, out);
        }
    }
}

/// Run a git command inside the shadow repo, returning trimmed stdout.
fn git(args: &[&str], shadow: &Path) -> Result<String, ToolError> {
    let out = Command::new("git")
        .args(args)
        .current_dir(shadow)
        .output()
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "checkpoint".into(),
            message: format!("git {}: {e}", args.join(" ")),
        })?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Return `(hash, subject, date)` tuples for each commit, newest first.
fn log_entries(shadow: &Path) -> Vec<(String, String, String)> {
    let raw = git(
        &["log", "--format=%H\x1f%s\x1f%ar", "--max-count=200"],
        shadow,
    )
    .unwrap_or_default();

    raw.lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let mut parts = l.splitn(3, '\x1f');
            let hash = parts.next().unwrap_or("").to_string();
            let subj = parts.next().unwrap_or("").to_string();
            let date = parts.next().unwrap_or("").to_string();
            (hash, subj, date)
        })
        .collect()
}

/// Get the commit hash for checkpoint number N (1=newest).
fn hash_for_n(shadow: &Path, n: u32) -> Option<String> {
    let entries = log_entries(shadow);
    entries
        .into_iter()
        .nth((n as usize).saturating_sub(1))
        .map(|(h, _, _)| h)
}

// ── Public helper for other tools ────────────────────────────────────────────

/// Auto-create a checkpoint before a mutation if checkpoints are enabled.
///
/// Called by `file_write`, `file_patch`, and `terminal` tools.  Silently
/// ignores errors — a missing checkpoint must never block a tool action.
pub fn ensure_checkpoint(ctx: &ToolContext, reason: &str) {
    if !ctx.config.checkpoints_enabled {
        return;
    }
    let shadow = shadow_dir(&ctx.config.edgecrab_home, &ctx.cwd);
    if let Err(e) = ensure_shadow_repo(&shadow) {
        tracing::debug!("checkpoint: ensure_shadow_repo failed: {e}");
        return;
    }
    if let Err(e) = sync_to_shadow(&ctx.cwd, &shadow) {
        tracing::debug!("checkpoint: sync_to_shadow failed: {e}");
        return;
    }
    let _ = git(&["add", "-A"], &shadow);
    // Only commit if there are staged changes
    let status = git(&["status", "--porcelain"], &shadow).unwrap_or_default();
    if status.is_empty() {
        return;
    }
    let _ = git(&["commit", "-m", reason, "--quiet"], &shadow);

    // Prune old checkpoints if over the limit
    let max = ctx.config.checkpoints_max_snapshots as usize;
    let entries = log_entries(&shadow);
    if entries.len() > max {
        // Keep only the newest `max` commits by rebasing onto the first commit we
        // want to keep as an orphan start (best-effort, skip on any error)
        if let Some((oldest_keep, _, _)) = entries.get(max - 1) {
            let _ = git(
                &[
                    "rebase",
                    "--onto",
                    oldest_keep,
                    &format!("{oldest_keep}~1"),
                    "HEAD",
                ],
                &shadow,
            );
        }
    }
}

// ── Tool actions ──────────────────────────────────────────────────────────────

fn action_create(ctx: &ToolContext, reason: &str) -> Result<String, ToolError> {
    let shadow = shadow_dir(&ctx.config.edgecrab_home, &ctx.cwd);
    ensure_shadow_repo(&shadow)?;
    let count = sync_to_shadow(&ctx.cwd, &shadow)?;
    git(&["add", "-A"], &shadow)?;
    let status = git(&["status", "--porcelain"], &shadow)?;
    if status.is_empty() {
        return Ok(format!(
            "Checkpoint created (no file changes, {count} files tracked)."
        ));
    }
    git(&["commit", "-m", reason, "--quiet"], &shadow)?;
    let n = log_entries(&shadow).len();
    Ok(format!(
        "Checkpoint #{n} '{reason}' created ({count} files tracked)."
    ))
}

fn action_list(ctx: &ToolContext) -> Result<String, ToolError> {
    let shadow = shadow_dir(&ctx.config.edgecrab_home, &ctx.cwd);
    if !shadow.join(".git").exists() {
        return Ok("No checkpoints found.".into());
    }
    let entries = log_entries(&shadow);
    if entries.is_empty() {
        return Ok("No checkpoints found.".into());
    }
    let mut out = format!("Checkpoints ({}):\n", entries.len());
    for (i, (hash, subj, date)) in entries.iter().enumerate() {
        out.push_str(&format!(
            "  {:>3}  {}  {}  ({})\n",
            i + 1,
            crate::safe_truncate(hash, 8),
            subj,
            date
        ));
    }
    Ok(out)
}

fn action_restore(ctx: &ToolContext, n: u32) -> Result<String, ToolError> {
    let shadow = shadow_dir(&ctx.config.edgecrab_home, &ctx.cwd);
    let hash = hash_for_n(&shadow, n).ok_or_else(|| ToolError::ExecutionFailed {
        tool: "checkpoint".into(),
        message: format!("Checkpoint #{n} not found"),
    })?;

    // List all files in that commit
    let tree_out = git(&["ls-tree", "-r", "--name-only", &hash], &shadow)?;
    let mut restored = 0usize;
    for rel in tree_out.lines().filter(|l| !l.is_empty()) {
        let content = git(&["show", &format!("{hash}:{rel}")], &shadow)?;
        let dst = ctx.cwd.join(rel);
        if let Some(p) = dst.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        std::fs::write(&dst, content.as_bytes()).map_err(|e| ToolError::ExecutionFailed {
            tool: "checkpoint".into(),
            message: format!("Failed to restore {rel}: {e}"),
        })?;
        restored += 1;
    }
    Ok(format!(
        "Checkpoint #{n} restored ({restored} files written)."
    ))
}

fn action_diff(ctx: &ToolContext, n: u32) -> Result<String, ToolError> {
    let shadow = shadow_dir(&ctx.config.edgecrab_home, &ctx.cwd);
    let hash = hash_for_n(&shadow, n).ok_or_else(|| ToolError::ExecutionFailed {
        tool: "checkpoint".into(),
        message: format!("Checkpoint #{n} not found"),
    })?;

    // Compare files in that commit vs current CWD
    let tree_out = git(&["ls-tree", "-r", "--name-only", &hash], &shadow)?;
    let mut modified = Vec::new();
    let mut deleted = Vec::new();
    let mut added = Vec::new();

    let tracked: std::collections::HashSet<String> = tree_out
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();

    for rel in &tracked {
        let full = ctx.cwd.join(rel);
        if full.exists() {
            let current = std::fs::read_to_string(&full).unwrap_or_default();
            let saved = git(&["show", &format!("{hash}:{rel}")], &shadow).unwrap_or_default();
            if current != saved {
                modified.push(rel.clone());
            }
        } else {
            deleted.push(rel.clone());
        }
    }

    // Find files that exist now but weren't in the checkpoint
    for file in collect_files(&ctx.cwd) {
        let rel = file.to_string_lossy().to_string();
        if !tracked.contains(&rel) {
            added.push(rel);
        }
    }

    if added.is_empty() && modified.is_empty() && deleted.is_empty() {
        return Ok(format!("Checkpoint #{n}: no changes vs current state."));
    }
    let mut out = format!("Diff vs checkpoint #{n}:\n");
    for f in &added {
        out.push_str(&format!("  + {f}  (added)\n"));
    }
    for f in &modified {
        out.push_str(&format!("  ~ {f}  (modified)\n"));
    }
    for f in &deleted {
        out.push_str(&format!("  - {f}  (deleted)\n"));
    }
    out.push_str(&format!(
        "\n{} added, {} modified, {} deleted",
        added.len(),
        modified.len(),
        deleted.len()
    ));
    Ok(out)
}

fn action_restore_file(ctx: &ToolContext, n: u32, file: &str) -> Result<String, ToolError> {
    let shadow = shadow_dir(&ctx.config.edgecrab_home, &ctx.cwd);
    let hash = hash_for_n(&shadow, n).ok_or_else(|| ToolError::ExecutionFailed {
        tool: "checkpoint".into(),
        message: format!("Checkpoint #{n} not found"),
    })?;
    let content = git(&["show", &format!("{hash}:{file}")], &shadow).map_err(|_| {
        ToolError::ExecutionFailed {
            tool: "checkpoint".into(),
            message: format!("File '{file}' not found in checkpoint #{n}"),
        }
    })?;
    let dst = ctx.cwd.join(file);
    if let Some(p) = dst.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    std::fs::write(&dst, content.as_bytes()).map_err(|e| ToolError::ExecutionFailed {
        tool: "checkpoint".into(),
        message: format!("Failed to write {file}: {e}"),
    })?;
    Ok(format!("File '{file}' restored from checkpoint #{n}."))
}

// ── Tool handler ──────────────────────────────────────────────────────────────

pub struct CheckpointTool;

#[derive(Deserialize)]
struct CheckpointArgs {
    action: String,
    /// Reason/label for "create"; also accepted as "name" for compatibility.
    reason: Option<String>,
    /// Backward-compat alias for "reason"
    name: Option<String>,
    /// Checkpoint number (1 = newest) for restore / diff / restore_file.
    n: Option<u32>,
    /// File path for restore_file action.
    file: Option<String>,
}

#[async_trait]
impl ToolHandler for CheckpointTool {
    fn name(&self) -> &'static str {
        "checkpoint"
    }

    fn toolset(&self) -> &'static str {
        "core"
    }

    fn emoji(&self) -> &'static str {
        "💾"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "checkpoint".into(),
            description: concat!(
                "Create, list, restore, or diff shadow-git filesystem checkpoints for rollback. ",
                "Use 'create' before risky changes, 'list' to see numbered history, ",
                "'restore' to roll back to checkpoint N (1=newest), ",
                "'diff' to preview changes, and 'restore_file' to recover a single file."
            )
            .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "list", "restore", "diff", "restore_file"],
                        "description": "Checkpoint action"
                    },
                    "reason": {
                        "type": "string",
                        "description": "Reason / label for 'create'"
                    },
                    "n": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Checkpoint number (1=newest) for restore/diff/restore_file"
                    },
                    "file": {
                        "type": "string",
                        "description": "Relative file path for restore_file action"
                    }
                },
                "required": ["action"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: CheckpointArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "checkpoint".into(),
                message: format!("Invalid checkpoint args: {e}"),
            })?;

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        // Accept "name" as backward-compat alias for "reason"
        let reason = args.reason.or(args.name);

        match args.action.as_str() {
            "create" => {
                let r = reason.unwrap_or_else(|| "manual checkpoint".to_string());
                action_create(ctx, &r)
            }
            "list" => action_list(ctx),
            "restore" => {
                let n = args.n.ok_or_else(|| ToolError::InvalidArgs {
                    tool: "checkpoint".into(),
                    message: "'n' (checkpoint number) is required for restore".into(),
                })?;
                action_restore(ctx, n)
            }
            "diff" => {
                let n = args.n.ok_or_else(|| ToolError::InvalidArgs {
                    tool: "checkpoint".into(),
                    message: "'n' (checkpoint number) is required for diff".into(),
                })?;
                action_diff(ctx, n)
            }
            "restore_file" => {
                let n = args.n.ok_or_else(|| ToolError::InvalidArgs {
                    tool: "checkpoint".into(),
                    message: "'n' is required for restore_file".into(),
                })?;
                let file = args.file.ok_or_else(|| ToolError::InvalidArgs {
                    tool: "checkpoint".into(),
                    message: "'file' is required for restore_file".into(),
                })?;
                action_restore_file(ctx, n, &file)
            }
            other => Err(ToolError::InvalidArgs {
                tool: "checkpoint".into(),
                message: format!(
                    "Unknown action: '{}'. Use: create, list, restore, diff, restore_file",
                    other
                ),
            }),
        }
    }
}

inventory::submit!(&CheckpointTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEdgecrabHome;

    #[test]
    fn tool_metadata() {
        let tool = CheckpointTool;
        assert_eq!(tool.name(), "checkpoint");
        assert_eq!(tool.toolset(), "core");
        assert!(tool.is_available());
    }

    #[tokio::test]
    async fn create_and_list_checkpoint() {
        // Only run if git is available
        if std::process::Command::new("git")
            .arg("--version")
            .status()
            .is_err()
        {
            return;
        }
        let tmp = tempfile::TempDir::new().expect("tmp");
        let edgecrab_home = tempfile::TempDir::new().expect("edgecrab home");
        let mut ctx = ToolContext::test_context();
        ctx.cwd = tmp.path().to_path_buf();
        ctx.config.edgecrab_home = edgecrab_home.path().to_path_buf();

        // Need a git repo in the tmp dir for ls-files to work
        let _ = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&ctx.cwd)
            .status();
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "test@test"])
            .current_dir(&ctx.cwd)
            .status();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "test"])
            .current_dir(&ctx.cwd)
            .status();

        let test_file = ctx.cwd.join("test_ckpt.txt");
        std::fs::write(&test_file, "hello checkpoint").expect("write");
        let _ = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&ctx.cwd)
            .status();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "init", "--quiet"])
            .current_dir(&ctx.cwd)
            .status();

        let result = CheckpointTool
            .execute(json!({ "action": "create", "name": "test-ckpt" }), &ctx)
            .await
            .expect("create should succeed");
        assert!(result.contains("created"), "got: {result}");

        let list = CheckpointTool
            .execute(json!({ "action": "list" }), &ctx)
            .await
            .expect("list should succeed");
        assert!(list.contains("test-ckpt"), "got: {list}");
    }

    #[tokio::test]
    async fn diff_no_changes() {
        if std::process::Command::new("git")
            .arg("--version")
            .status()
            .is_err()
        {
            return;
        }
        let tmp = tempfile::TempDir::new().expect("tmp");
        let edgecrab_home = tempfile::TempDir::new().expect("edgecrab home");
        let mut ctx = ToolContext::test_context();
        ctx.cwd = tmp.path().to_path_buf();
        ctx.config.edgecrab_home = edgecrab_home.path().to_path_buf();

        let _ = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&ctx.cwd)
            .status();
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "test@test"])
            .current_dir(&ctx.cwd)
            .status();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "test"])
            .current_dir(&ctx.cwd)
            .status();

        let test_file = ctx.cwd.join("test_diff.txt");
        std::fs::write(&test_file, "diff content").expect("write");
        let _ = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&ctx.cwd)
            .status();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "init", "--quiet"])
            .current_dir(&ctx.cwd)
            .status();

        let create_result = CheckpointTool
            .execute(json!({ "action": "create", "name": "diff-test" }), &ctx)
            .await
            .expect("create");
        assert!(create_result.contains("created"), "got: {create_result}");

        let diff = CheckpointTool
            .execute(json!({ "action": "diff", "n": 1 }), &ctx)
            .await
            .expect("diff should succeed");
        assert!(diff.contains("no changes"), "got: {diff}");
    }

    #[tokio::test]
    async fn invalid_action() {
        let ctx = ToolContext::test_context();
        let result = CheckpointTool
            .execute(json!({ "action": "explode" }), &ctx)
            .await;
        assert!(matches!(result, Err(ToolError::InvalidArgs { .. })));
    }

    #[tokio::test]
    async fn checkpoint_uses_context_home_not_process_env() {
        if std::process::Command::new("git")
            .arg("--version")
            .status()
            .is_err()
        {
            return;
        }

        let tmp = tempfile::TempDir::new().expect("tmp");
        let configured_home = tempfile::TempDir::new().expect("configured home");
        let foreign_home = TestEdgecrabHome::new();

        let mut ctx = ToolContext::test_context();
        ctx.cwd = tmp.path().to_path_buf();
        ctx.config.edgecrab_home = configured_home.path().to_path_buf();

        let _ = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&ctx.cwd)
            .status();
        let _ = std::process::Command::new("git")
            .args(["config", "user.email", "test@test"])
            .current_dir(&ctx.cwd)
            .status();
        let _ = std::process::Command::new("git")
            .args(["config", "user.name", "test"])
            .current_dir(&ctx.cwd)
            .status();

        std::fs::write(ctx.cwd.join("test_ckpt.txt"), "hello checkpoint").expect("write");
        let _ = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&ctx.cwd)
            .status();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "init", "--quiet"])
            .current_dir(&ctx.cwd)
            .status();

        let result = CheckpointTool
            .execute(json!({ "action": "create", "name": "ctx-home" }), &ctx)
            .await
            .expect("create should succeed");
        assert!(result.contains("created"), "got: {result}");

        assert!(
            shadow_dir(configured_home.path(), &ctx.cwd)
                .join(".git")
                .exists(),
            "checkpoint should use ToolContext config home"
        );
        assert!(
            !shadow_dir(foreign_home.path(), &ctx.cwd)
                .join(".git")
                .exists(),
            "checkpoint should ignore unrelated process env overrides"
        );
    }
}
