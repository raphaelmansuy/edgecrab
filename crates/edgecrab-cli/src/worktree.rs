use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::Context;

const WORKTREE_DIR_NAME: &str = ".worktrees";
const WORKTREE_GITIGNORE_ENTRY: &str = ".worktrees/";
const WORKTREE_NAME_PREFIX: &str = "edgecrab-";
const WORKTREE_BRANCH_PREFIX: &str = "edgecrab/edgecrab-";
const DEFAULT_STALE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorktreeInfo {
    pub(crate) path: PathBuf,
    pub(crate) branch: String,
    pub(crate) repo_root: PathBuf,
}

pub(crate) struct ActiveWorktree {
    info: WorktreeInfo,
    original_cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorktreeRuntimeStatus {
    pub(crate) launch_default_enabled: bool,
    pub(crate) cwd: PathBuf,
    pub(crate) repo_root: Option<PathBuf>,
    pub(crate) branch: Option<String>,
    pub(crate) git_dir: Option<PathBuf>,
    pub(crate) common_dir: Option<PathBuf>,
    pub(crate) linked_worktree: bool,
    pub(crate) detection_error: Option<String>,
}

impl ActiveWorktree {
    pub(crate) fn activate() -> anyhow::Result<Self> {
        let original_cwd =
            std::env::current_dir().context("failed to resolve current directory")?;
        let repo_root = git_repo_root_from(&original_cwd)?;

        prune_stale_worktrees(&repo_root, DEFAULT_STALE_MAX_AGE);
        let info = setup_worktree_in_repo(&repo_root)?;

        std::env::set_current_dir(&info.path)
            .with_context(|| format!("failed to cd into worktree {}", info.path.display()))?;

        eprintln!("🌿 Running in isolated worktree: {}", info.path.display());
        eprintln!("   Branch: {}", info.branch);

        Ok(Self { info, original_cwd })
    }

    pub(crate) fn system_prompt_note(&self) -> String {
        format!(
            "[System note: You are working in an isolated git worktree at {}. Your branch is `{}`. \
Changes here do not affect the main working tree or other agents. Remember to commit and push your changes, and create a PR if appropriate. The original repo is at {}.]",
            self.info.path.display(),
            self.info.branch,
            self.info.repo_root.display()
        )
    }
}

impl WorktreeRuntimeStatus {
    pub(crate) fn current_checkout_label(&self) -> &'static str {
        if self.linked_worktree {
            "isolated linked worktree"
        } else if self.repo_root.is_some() {
            "primary checkout"
        } else {
            "not a git checkout"
        }
    }

    pub(crate) fn render_report(&self) -> String {
        let repo_root = self
            .repo_root
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "(not in a git repository)".into());
        let branch = self.branch.as_deref().unwrap_or("(none)");
        let git_dir = self
            .git_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "(unavailable)".into());
        let common_dir = self
            .common_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "(unavailable)".into());

        let mut text = format!(
            "Git worktree status:\n\
             Launch default:   {}\n\
             Current checkout: {}\n\
             Current cwd:      {}\n\
             Repo root:        {}\n\
             Branch:           {}\n\
             Git dir:          {}\n\
             Common git dir:   {}\n",
            if self.launch_default_enabled {
                "enabled"
            } else {
                "disabled"
            },
            self.current_checkout_label(),
            self.cwd.display(),
            repo_root,
            branch,
            git_dir,
            common_dir,
        );

        if let Some(error) = &self.detection_error {
            text.push_str(&format!("\nDetection note: {error}\n"));
        }

        text.push_str(
            "\nCommands:\n\
             - /worktree            open this overlay\n\
             - /worktree on         enable isolated worktrees for future launches\n\
             - /worktree off        disable the default for future launches\n\
             - /worktree toggle     flip the saved default\n\
             - edgecrab -w ...      force a one-off isolated launch\n",
        );

        if self.launch_default_enabled && !self.linked_worktree {
            text.push_str(
                "\nCurrent process note: the saved default only affects future launches. This live session stays in its current checkout.\n",
            );
        }

        text
    }
}

impl Drop for ActiveWorktree {
    fn drop(&mut self) {
        if let Err(error) = std::env::set_current_dir(&self.original_cwd) {
            eprintln!(
                "⚠ Failed to restore cwd '{}' before worktree cleanup: {error}",
                self.original_cwd.display()
            );
        }

        if let Err(error) = cleanup_worktree(&self.info) {
            eprintln!(
                "⚠ Failed to clean worktree '{}': {error}",
                self.info.path.display()
            );
        }
    }
}

pub(crate) fn git_repo_root_from(cwd: &Path) -> anyhow::Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .context("failed to execute git rev-parse")?;

    if !output.status.success() {
        anyhow::bail!("--worktree requires being inside a git repository");
    }

    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        anyhow::bail!("git rev-parse returned an empty repository root");
    }

    Ok(PathBuf::from(root))
}

pub(crate) fn inspect_runtime(cwd: &Path, launch_default_enabled: bool) -> WorktreeRuntimeStatus {
    let cwd = cwd.to_path_buf();
    let repo_root = git_repo_root_from(&cwd).ok();
    let branch = git_stdout(&cwd, &["branch", "--show-current"]);
    let git_dir = git_path_stdout(&cwd, &["rev-parse", "--git-dir"]);
    let common_dir = git_path_stdout(&cwd, &["rev-parse", "--git-common-dir"]);

    let linked_worktree = match (git_dir.as_ref(), common_dir.as_ref()) {
        (Some(git_dir), Some(common_dir)) => {
            canonical_if_possible(git_dir) != canonical_if_possible(common_dir)
        }
        _ => false,
    };
    let detection_error = if repo_root.is_none() {
        Some("EdgeCrab is not currently running inside a git repository.".into())
    } else {
        None
    };

    WorktreeRuntimeStatus {
        launch_default_enabled,
        cwd,
        repo_root,
        branch,
        git_dir,
        common_dir,
        linked_worktree,
        detection_error,
    }
}

pub(crate) fn setup_worktree_in_repo(repo_root: &Path) -> anyhow::Result<WorktreeInfo> {
    ensure_worktrees_ignored(repo_root);

    let short_id = uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(8)
        .collect::<String>();
    let worktree_name = format!("{WORKTREE_NAME_PREFIX}{short_id}");
    let branch = format!("{WORKTREE_BRANCH_PREFIX}{short_id}");
    let worktrees_dir = repo_root.join(WORKTREE_DIR_NAME);
    std::fs::create_dir_all(&worktrees_dir).with_context(|| {
        format!(
            "failed to create {} in {}",
            WORKTREE_DIR_NAME,
            repo_root.display()
        )
    })?;
    let worktree_path = worktrees_dir.join(&worktree_name);

    let output = Command::new("git")
        .args(["worktree", "add"])
        .arg(&worktree_path)
        .args(["-b", &branch, "HEAD"])
        .current_dir(repo_root)
        .output()
        .context("failed to execute git worktree add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("git worktree add failed: {stderr}");
    }

    let info = WorktreeInfo {
        path: worktree_path,
        branch,
        repo_root: repo_root.to_path_buf(),
    };

    if let Err(error) = copy_worktree_includes(&info) {
        tracing::warn!(?error, "failed to apply .worktreeinclude entries");
    }

    Ok(info)
}

pub(crate) fn cleanup_worktree(info: &WorktreeInfo) -> anyhow::Result<()> {
    if !info.path.exists() {
        return Ok(());
    }

    if has_unpushed_commits(&info.path) {
        eprintln!(
            "⚠ Worktree has unpushed commits, keeping: {}",
            info.path.display()
        );
        eprintln!(
            "  To clean up manually: git worktree remove --force {}",
            info.path.display()
        );
        return Ok(());
    }

    let remove_output = Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(&info.path)
        .current_dir(&info.repo_root)
        .output()
        .context("failed to execute git worktree remove")?;

    if !remove_output.status.success() {
        let stderr = String::from_utf8_lossy(&remove_output.stderr)
            .trim()
            .to_string();
        anyhow::bail!("git worktree remove failed: {stderr}");
    }

    let branch_output = Command::new("git")
        .args(["branch", "-D", &info.branch])
        .current_dir(&info.repo_root)
        .output()
        .context("failed to execute git branch -D")?;

    if !branch_output.status.success() {
        let stderr = String::from_utf8_lossy(&branch_output.stderr)
            .trim()
            .to_string();
        anyhow::bail!("git branch -D failed: {stderr}");
    }

    eprintln!("✓ Worktree cleaned up: {}", info.path.display());
    Ok(())
}

pub(crate) fn prune_stale_worktrees(repo_root: &Path, max_age: Duration) {
    let now = SystemTime::now();
    let soft_cutoff = now.checked_sub(max_age).unwrap_or(SystemTime::UNIX_EPOCH);
    let hard_cutoff = now
        .checked_sub(max_age.saturating_mul(3))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let worktrees_dir = repo_root.join(WORKTREE_DIR_NAME);

    if let Ok(entries) = std::fs::read_dir(&worktrees_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let file_name = path.file_name().and_then(OsStr::to_str).unwrap_or_default();
            if !path.is_dir() || !file_name.starts_with(WORKTREE_NAME_PREFIX) {
                continue;
            }

            let mtime = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);

            if mtime > soft_cutoff {
                continue;
            }

            let force = mtime <= hard_cutoff;
            if !force && has_unpushed_commits(&path) {
                continue;
            }

            let branch = git_current_branch(&path).unwrap_or_default();
            let _ = Command::new("git")
                .args(["worktree", "remove", "--force"])
                .arg(&path)
                .current_dir(repo_root)
                .output();
            if !branch.is_empty() {
                let _ = Command::new("git")
                    .args(["branch", "-D", &branch])
                    .current_dir(repo_root)
                    .output();
            }
        }
    }

    prune_orphaned_branches(repo_root);
}

fn prune_orphaned_branches(repo_root: &Path) {
    let branch_list = Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(repo_root)
        .output();
    let Ok(output) = branch_list else {
        return;
    };
    if !output.status.success() {
        return;
    }

    let active_branches = active_worktree_branches(repo_root);
    for branch in String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|branch| branch.starts_with(WORKTREE_BRANCH_PREFIX))
    {
        if active_branches.contains(branch) {
            continue;
        }
        let _ = Command::new("git")
            .args(["branch", "-D", branch])
            .current_dir(repo_root)
            .output();
    }
}

fn active_worktree_branches(repo_root: &Path) -> std::collections::BTreeSet<String> {
    let mut branches = std::collections::BTreeSet::new();
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_root)
        .output();
    let Ok(output) = output else {
        return branches;
    };
    if !output.status.success() {
        return branches;
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(branch) = line.strip_prefix("branch refs/heads/") {
            branches.insert(branch.trim().to_string());
        }
    }

    branches
}

fn git_current_branch(repo_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

fn git_stdout(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

fn git_path_stdout(cwd: &Path, args: &[&str]) -> Option<PathBuf> {
    let value = git_stdout(cwd, args)?;
    let path = PathBuf::from(value);
    Some(if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    })
}

fn canonical_if_possible(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn has_unpushed_commits(repo_root: &Path) -> bool {
    let output = Command::new("git")
        .args(["log", "--oneline", "HEAD", "--not", "--remotes"])
        .current_dir(repo_root)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            !String::from_utf8_lossy(&output.stdout).trim().is_empty()
        }
        _ => true,
    }
}

fn ensure_worktrees_ignored(repo_root: &Path) {
    let gitignore = repo_root.join(".gitignore");
    let existing = std::fs::read_to_string(&gitignore).unwrap_or_default();
    if existing
        .lines()
        .any(|line| line.trim() == WORKTREE_GITIGNORE_ENTRY)
    {
        return;
    }

    let addition = if existing.is_empty() || existing.ends_with('\n') {
        format!("{WORKTREE_GITIGNORE_ENTRY}\n")
    } else {
        format!("\n{WORKTREE_GITIGNORE_ENTRY}\n")
    };

    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&gitignore)
        .and_then(|mut file| std::io::Write::write_all(&mut file, addition.as_bytes()));
}

fn copy_worktree_includes(info: &WorktreeInfo) -> anyhow::Result<()> {
    let include_file = info.repo_root.join(".worktreeinclude");
    if !include_file.exists() {
        return Ok(());
    }

    let repo_root = info.repo_root.canonicalize().with_context(|| {
        format!(
            "failed to canonicalize repository root '{}'",
            info.repo_root.display()
        )
    })?;
    let worktree_root = info
        .path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize worktree '{}'", info.path.display()))?;
    let contents = std::fs::read_to_string(&include_file).with_context(|| {
        format!(
            "failed to read .worktreeinclude from '{}'",
            include_file.display()
        )
    })?;

    for raw_line in contents.lines() {
        let entry = raw_line.trim();
        if entry.is_empty() || entry.starts_with('#') {
            continue;
        }

        let relative = match sanitize_relative_include(entry) {
            Some(path) => path,
            None => {
                tracing::warn!(entry = entry, "skipping invalid .worktreeinclude entry");
                continue;
            }
        };

        let src = repo_root.join(&relative);
        if !src.exists() {
            continue;
        }
        let src_resolved = match src.canonicalize() {
            Ok(path) => path,
            Err(_) => continue,
        };
        if !src_resolved.starts_with(&repo_root) {
            tracing::warn!(
                entry = entry,
                "skipping .worktreeinclude entry outside repo root"
            );
            continue;
        }

        let dst = match resolve_destination_path(&worktree_root, &relative) {
            Some(path) => path,
            None => {
                tracing::warn!(
                    entry = entry,
                    "skipping .worktreeinclude entry that escapes worktree"
                );
                continue;
            }
        };

        if src.is_file() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src, &dst).with_context(|| {
                format!(
                    "failed to copy include file '{}' to '{}'",
                    src.display(),
                    dst.display()
                )
            })?;
            continue;
        }

        if src.is_dir() && !dst.exists() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            link_or_copy_dir(&src_resolved, &dst)?;
        }
    }

    Ok(())
}

fn sanitize_relative_include(entry: &str) -> Option<PathBuf> {
    let path = Path::new(entry);
    if path.is_absolute() {
        return None;
    }

    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => cleaned.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    if cleaned.as_os_str().is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn resolve_destination_path(root: &Path, relative: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let mut current = root.clone();

    for component in relative.components() {
        let Component::Normal(part) = component else {
            return None;
        };
        let candidate = current.join(part);
        if candidate.exists() {
            let canonical = candidate.canonicalize().ok()?;
            if !canonical.starts_with(&root) {
                return None;
            }
            current = canonical;
        } else {
            current = candidate;
        }
    }

    if current.starts_with(&root) {
        Some(current)
    } else {
        None
    }
}

#[cfg(unix)]
fn link_or_copy_dir(src: &Path, dst: &Path) -> anyhow::Result<()> {
    std::os::unix::fs::symlink(src, dst).with_context(|| {
        format!(
            "failed to symlink include directory '{}' to '{}'",
            src.display(),
            dst.display()
        )
    })?;
    Ok(())
}

#[cfg(not(unix))]
fn link_or_copy_dir(src: &Path, dst: &Path) -> anyhow::Result<()> {
    copy_dir_recursive(src, dst)
}

#[cfg(not(unix))]
fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn run_git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn make_repo_with_remote() -> (TempDir, PathBuf) {
        let temp = TempDir::new().expect("tempdir");
        let repo = temp.path().join("repo");
        let bare = temp.path().join("remote.git");
        std::fs::create_dir_all(&repo).expect("create repo");

        run_git(&repo, &["init", "-b", "main"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "Test User"]);
        std::fs::write(repo.join("README.md"), "# test\n").expect("write readme");
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "initial"]);

        Command::new("git")
            .args(["init", "--bare"])
            .arg(&bare)
            .output()
            .expect("init bare");
        run_git(
            &repo,
            &["remote", "add", "origin", bare.to_str().expect("utf8")],
        );
        run_git(&repo, &["push", "-u", "origin", "main"]);

        (temp, repo)
    }

    fn configure_test_home(root: &Path) {
        let edgecrab_home = root.join(".edgecrab");
        std::fs::create_dir_all(edgecrab_home.join("logs")).expect("create logs dir");
        // SAFETY: tests serialize env mutations through lock_test_env().
        unsafe {
            std::env::set_var("EDGECRAB_HOME", &edgecrab_home);
        }
    }

    fn clear_test_home() {
        // SAFETY: tests serialize env mutations through lock_test_env().
        unsafe {
            std::env::remove_var("EDGECRAB_HOME");
        }
    }

    #[test]
    fn setup_worktree_adds_gitignore_entry_and_branch() {
        let _guard = crate::gateway_catalog::lock_test_env();
        let (temp, repo) = make_repo_with_remote();
        configure_test_home(temp.path());

        let info = setup_worktree_in_repo(&repo).expect("setup worktree");
        assert!(info.path.exists());
        assert!(info.branch.starts_with(WORKTREE_BRANCH_PREFIX));

        let gitignore = std::fs::read_to_string(repo.join(".gitignore")).expect("read gitignore");
        assert!(gitignore.contains(WORKTREE_GITIGNORE_ENTRY));

        cleanup_worktree(&info).expect("cleanup worktree");
        clear_test_home();
    }

    #[test]
    fn cleanup_removes_clean_worktree_and_branch() {
        let _guard = crate::gateway_catalog::lock_test_env();
        let (temp, repo) = make_repo_with_remote();
        configure_test_home(temp.path());
        let info = setup_worktree_in_repo(&repo).expect("setup worktree");

        cleanup_worktree(&info).expect("cleanup worktree");

        assert!(!info.path.exists());
        let branches = run_git(&repo, &["branch", "--format=%(refname:short)"]);
        assert!(!branches.lines().any(|line| line.trim() == info.branch));
        clear_test_home();
    }

    #[test]
    fn cleanup_preserves_worktree_with_unpushed_commit() {
        let _guard = crate::gateway_catalog::lock_test_env();
        let (temp, repo) = make_repo_with_remote();
        configure_test_home(temp.path());
        let info = setup_worktree_in_repo(&repo).expect("setup worktree");

        std::fs::write(info.path.join("new.txt"), "hello").expect("write file");
        run_git(&info.path, &["add", "new.txt"]);
        run_git(&info.path, &["commit", "-m", "worktree change"]);

        cleanup_worktree(&info).expect("cleanup worktree");

        assert!(info.path.exists());
        let branches = run_git(&repo, &["branch", "--format=%(refname:short)"]);
        assert!(branches.lines().any(|line| line.trim() == info.branch));
        clear_test_home();
    }

    #[test]
    fn prune_stale_worktree_removes_old_clean_worktree() {
        let _guard = crate::gateway_catalog::lock_test_env();
        let (temp, repo) = make_repo_with_remote();
        configure_test_home(temp.path());
        let info = setup_worktree_in_repo(&repo).expect("setup worktree");

        prune_stale_worktrees(&repo, Duration::ZERO);

        assert!(!info.path.exists());
        let branches = run_git(&repo, &["branch", "--format=%(refname:short)"]);
        assert!(!branches.lines().any(|line| line.trim() == info.branch));
        clear_test_home();
    }

    #[test]
    fn rejects_parent_directory_file_traversal_in_worktreeinclude() {
        let _guard = crate::gateway_catalog::lock_test_env();
        let (temp, repo) = make_repo_with_remote();
        configure_test_home(temp.path());
        let outside = repo.parent().expect("repo parent").join("secret.txt");
        std::fs::write(&outside, "sensitive").expect("write outside");
        std::fs::write(repo.join(".worktreeinclude"), "../secret.txt\n").expect("write include");

        let info = setup_worktree_in_repo(&repo).expect("setup worktree");

        assert!(!info.path.join("secret.txt").exists());
        cleanup_worktree(&info).expect("cleanup worktree");
        clear_test_home();
    }

    #[test]
    fn rejects_symlink_file_that_resolves_outside_repo() {
        let _guard = crate::gateway_catalog::lock_test_env();
        let (temp, repo) = make_repo_with_remote();
        configure_test_home(temp.path());
        let outside = repo
            .parent()
            .expect("repo parent")
            .join("linked-secret.txt");
        std::fs::write(&outside, "sensitive").expect("write outside");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, repo.join("leak.txt")).expect("symlink");
        #[cfg(not(unix))]
        std::fs::write(repo.join("leak.txt"), "placeholder").expect("write placeholder");
        std::fs::write(repo.join(".worktreeinclude"), "leak.txt\n").expect("write include");

        let info = setup_worktree_in_repo(&repo).expect("setup worktree");

        #[cfg(unix)]
        assert!(!info.path.join("leak.txt").exists());
        cleanup_worktree(&info).expect("cleanup worktree");
        clear_test_home();
    }

    #[test]
    fn allows_valid_file_and_directory_includes() {
        let _guard = crate::gateway_catalog::lock_test_env();
        let (temp, repo) = make_repo_with_remote();
        configure_test_home(temp.path());
        std::fs::write(repo.join(".env"), "TOKEN=test\n").expect("write env");
        let venv_dir = repo.join(".venv").join("lib");
        std::fs::create_dir_all(&venv_dir).expect("create venv");
        std::fs::write(venv_dir.join("marker.txt"), "marker").expect("write marker");
        std::fs::write(repo.join(".worktreeinclude"), ".env\n.venv\n").expect("write include");

        let info = setup_worktree_in_repo(&repo).expect("setup worktree");

        assert_eq!(
            std::fs::read_to_string(info.path.join(".env")).expect("read copied env"),
            "TOKEN=test\n"
        );
        let linked_dir = info.path.join(".venv");
        assert!(linked_dir.exists());
        assert_eq!(
            std::fs::read_to_string(linked_dir.join("lib/marker.txt")).expect("read marker"),
            "marker"
        );
        #[cfg(unix)]
        assert!(linked_dir.is_symlink());

        cleanup_worktree(&info).expect("cleanup worktree");
        clear_test_home();
    }
}
