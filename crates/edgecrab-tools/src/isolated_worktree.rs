//! Isolated git worktrees for parallel subagent delegation (Wave 1).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Metadata for a delegate-task isolated worktree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsolatedWorktreeInfo {
    pub path: PathBuf,
    pub branch: String,
    pub repo_root: PathBuf,
    pub task_id: String,
}

/// Create a git worktree under `<repo>/.edgecrab/worktrees/<task_id>/`.
pub fn setup_isolated_worktree(
    repo_root: &Path,
    task_id: &str,
) -> Result<IsolatedWorktreeInfo, String> {
    let safe_id: String = task_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let worktrees_dir = repo_root.join(".edgecrab").join("worktrees");
    std::fs::create_dir_all(&worktrees_dir).map_err(|e| format!("mkdir worktrees: {e}"))?;
    let worktree_path = worktrees_dir.join(&safe_id);
    let branch = format!("edgecrab/delegate-{safe_id}");

    if worktree_path.exists() {
        return Ok(IsolatedWorktreeInfo {
            path: worktree_path,
            branch,
            repo_root: repo_root.to_path_buf(),
            task_id: safe_id,
        });
    }

    let output = Command::new("git")
        .args(["worktree", "add"])
        .arg(&worktree_path)
        .args(["-b", &branch, "HEAD"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("git worktree add failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree add: {stderr}"));
    }

    Ok(IsolatedWorktreeInfo {
        path: worktree_path,
        branch,
        repo_root: repo_root.to_path_buf(),
        task_id: safe_id,
    })
}

/// Build a merge report (diffstat + branch) for the parent agent.
pub fn merge_report(info: &IsolatedWorktreeInfo) -> String {
    let diffstat = Command::new("git")
        .args(["diff", "--stat", "HEAD"])
        .current_dir(&info.path)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "(no changes)".into());

    format!(
        "Isolated worktree merge report:\n\
         Branch:   {}\n\
         Path:     {}\n\
         Repo:     {}\n\
         Diffstat:\n{}\n",
        info.branch,
        info.path.display(),
        info.repo_root.display(),
        diffstat
    )
}

/// Resolve git repo root from cwd, if any.
pub fn git_repo_root_from(cwd: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn init_git_repo(dir: &Path) {
        Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .expect("git init");
        Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir)
            .output()
            .expect("git config email");
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .expect("git config name");
        fs::write(dir.join("README.md"), "hello").expect("write readme");
        Command::new("git")
            .args(["add", "README.md"])
            .current_dir(dir)
            .output()
            .expect("git add");
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir)
            .output()
            .expect("git commit");
    }

    #[test]
    fn setup_worktree_in_temp_repo() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_git_repo(tmp.path());
        let info = setup_isolated_worktree(tmp.path(), "task-1").expect("worktree");
        assert!(info.path.exists());
        assert!(info.branch.contains("delegate-task-1"));
        let report = merge_report(&info);
        assert!(report.contains("Diffstat"));
    }
}
