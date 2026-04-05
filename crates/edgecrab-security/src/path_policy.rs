//! Unified workspace path policy for file and context operations.
//!
//! WHY a separate policy type: `cwd`, configured allow-roots, internal trusted
//! roots, and deny prefixes were previously enforced in different places with
//! different rules. This module makes the effective path boundary explicit and
//! reusable across tools and context expansion.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

/// Structured path-policy failures that callers can map into their own error type.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathPolicyError {
    #[error("Cannot resolve path '{0}'")]
    NotFound(String),
    #[error("{0}")]
    PermissionDenied(String),
    #[error("{0}")]
    InvalidRoot(String),
}

/// Effective workspace path policy for one session.
#[derive(Debug, Clone)]
pub struct PathPolicy {
    workspace_root: PathBuf,
    allowed_roots: Vec<PathBuf>,
    denied_roots: Vec<PathBuf>,
}

impl PathPolicy {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root,
            allowed_roots: Vec::new(),
            denied_roots: Vec::new(),
        }
    }

    pub fn with_allowed_roots(mut self, allowed_roots: Vec<PathBuf>) -> Self {
        self.allowed_roots = allowed_roots;
        self
    }

    pub fn with_denied_roots(mut self, denied_roots: Vec<PathBuf>) -> Self {
        self.denied_roots = denied_roots;
        self
    }

    pub fn resolve_read_path(
        &self,
        path: &Path,
        extra_roots: &[&Path],
    ) -> Result<PathBuf, PathPolicyError> {
        let workspace_root = self.canonical_workspace_root()?;
        let allowed_roots = self.canonical_allowed_roots(&workspace_root, extra_roots)?;
        let denied_roots = self.canonical_denied_roots(&workspace_root)?;
        let candidate = resolve_candidate(path, &workspace_root);

        if !path.is_absolute() {
            let normalized = normalize_path(&candidate);
            if !normalized.starts_with(&workspace_root) {
                return Err(PathPolicyError::PermissionDenied(
                    "Path traversal detected: relative path escapes the workspace root".into(),
                ));
            }
        }

        let resolved = candidate
            .canonicalize()
            .map_err(|_| PathPolicyError::NotFound(path.display().to_string()))?;

        self.ensure_allowed(&resolved, &allowed_roots)?;
        self.ensure_not_denied(&resolved, &denied_roots)?;
        Ok(resolved)
    }

    pub fn resolve_write_path(
        &self,
        path: &Path,
        create_dirs: bool,
    ) -> Result<PathBuf, PathPolicyError> {
        let workspace_root = self.canonical_workspace_root()?;
        let allowed_roots = self.canonical_allowed_roots(&workspace_root, &[])?;
        let denied_roots = self.canonical_denied_roots(&workspace_root)?;
        let candidate = resolve_candidate(path, &workspace_root);
        let normalized = normalize_path(&candidate);

        if !path.is_absolute() && !normalized.starts_with(&workspace_root) {
            return Err(PathPolicyError::PermissionDenied(
                "Path traversal detected: relative path escapes the workspace root".into(),
            ));
        }

        let parent = normalized.parent().ok_or_else(|| {
            PathPolicyError::InvalidRoot("Invalid path: no parent directory".into())
        })?;

        if create_dirs {
            std::fs::create_dir_all(parent).map_err(|e| {
                PathPolicyError::InvalidRoot(format!(
                    "Cannot create parent directories for '{}': {e}",
                    path.display()
                ))
            })?;
        }

        let resolved_parent = parent.canonicalize().map_err(|e| {
            PathPolicyError::InvalidRoot(format!(
                "Cannot resolve parent '{}': {e}",
                parent.display()
            ))
        })?;

        self.ensure_allowed(&resolved_parent, &allowed_roots)?;
        self.ensure_not_denied(&resolved_parent, &denied_roots)?;
        Ok(normalized)
    }

    fn canonical_workspace_root(&self) -> Result<PathBuf, PathPolicyError> {
        self.workspace_root.canonicalize().map_err(|e| {
            PathPolicyError::InvalidRoot(format!(
                "Cannot resolve workspace root '{}': {e}",
                self.workspace_root.display()
            ))
        })
    }

    fn canonical_allowed_roots(
        &self,
        workspace_root: &Path,
        extra_roots: &[&Path],
    ) -> Result<Vec<PathBuf>, PathPolicyError> {
        let mut roots = Vec::new();
        let mut seen = BTreeSet::new();

        for root in std::iter::once(workspace_root.to_path_buf())
            .chain(
                self.allowed_roots
                    .iter()
                    .map(|root| resolve_root(root, workspace_root)),
            )
            .chain(extra_roots.iter().map(|root| (*root).to_path_buf()))
        {
            let canonical = root.canonicalize().map_err(|e| {
                PathPolicyError::InvalidRoot(format!(
                    "Cannot resolve allowed root '{}': {e}",
                    root.display()
                ))
            })?;
            if seen.insert(canonical.clone()) {
                roots.push(canonical);
            }
        }

        Ok(roots)
    }

    fn canonical_denied_roots(
        &self,
        workspace_root: &Path,
    ) -> Result<Vec<PathBuf>, PathPolicyError> {
        let mut denied = Vec::new();
        let mut seen = BTreeSet::new();

        for root in &self.denied_roots {
            let resolved = resolve_root(root, workspace_root);
            let canonical = resolved.canonicalize().map_err(|e| {
                PathPolicyError::InvalidRoot(format!(
                    "Cannot resolve denied root '{}': {e}",
                    resolved.display()
                ))
            })?;
            if seen.insert(canonical.clone()) {
                denied.push(canonical);
            }
        }

        Ok(denied)
    }

    fn ensure_allowed(
        &self,
        resolved_path: &Path,
        allowed_roots: &[PathBuf],
    ) -> Result<(), PathPolicyError> {
        if allowed_roots
            .iter()
            .any(|root| resolved_path.starts_with(root))
        {
            return Ok(());
        }

        Err(PathPolicyError::PermissionDenied(format!(
            "Path '{}' is outside the allowed roots",
            resolved_path.display()
        )))
    }

    fn ensure_not_denied(
        &self,
        resolved_path: &Path,
        denied_roots: &[PathBuf],
    ) -> Result<(), PathPolicyError> {
        if let Some(denied_root) = denied_roots
            .iter()
            .find(|root| resolved_path.starts_with(root))
        {
            return Err(PathPolicyError::PermissionDenied(format!(
                "Path '{}' is blocked by security.path_restrictions via '{}'",
                resolved_path.display(),
                denied_root.display()
            )));
        }

        Ok(())
    }
}

fn resolve_candidate(path: &Path, workspace_root: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}

fn resolve_root(root: &Path, workspace_root: &Path) -> PathBuf {
    if root.is_absolute() {
        root.to_path_buf()
    } else {
        workspace_root.join(root)
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other.as_os_str()),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_root_is_always_allowed() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let file = dir.path().join("main.rs");
        std::fs::write(&file, "fn main() {}").expect("write file");

        let policy = PathPolicy::new(dir.path().to_path_buf());
        let resolved = policy
            .resolve_read_path(Path::new("main.rs"), &[])
            .expect("resolve file");

        assert_eq!(resolved, file.canonicalize().expect("canon file"));
    }

    #[test]
    fn absolute_path_under_extra_allowed_root_is_permitted() {
        let workspace = tempfile::tempdir().expect("workspace");
        let extra = tempfile::tempdir().expect("extra");
        let file = extra.path().join("shared.txt");
        std::fs::write(&file, "shared").expect("write shared");

        let policy = PathPolicy::new(workspace.path().to_path_buf())
            .with_allowed_roots(vec![extra.path().to_path_buf()]);
        let resolved = policy
            .resolve_read_path(&file, &[])
            .expect("resolve extra root file");

        assert_eq!(resolved, file.canonicalize().expect("canon file"));
    }

    #[test]
    fn deny_roots_override_allowed_roots() {
        let workspace = tempfile::tempdir().expect("workspace");
        let secret_dir = workspace.path().join("secrets");
        std::fs::create_dir_all(&secret_dir).expect("create secrets");
        let secret_file = secret_dir.join("token.txt");
        std::fs::write(&secret_file, "secret").expect("write secret");

        let policy = PathPolicy::new(workspace.path().to_path_buf())
            .with_denied_roots(vec![PathBuf::from("secrets")]);

        let err = policy
            .resolve_read_path(Path::new("secrets/token.txt"), &[])
            .expect_err("denylisted path should fail");

        assert!(matches!(err, PathPolicyError::PermissionDenied(_)));
    }

    #[test]
    fn relative_traversal_is_blocked_before_canonical_access() {
        let workspace = tempfile::tempdir().expect("workspace");
        let policy = PathPolicy::new(workspace.path().to_path_buf());

        let err = policy
            .resolve_read_path(Path::new("../../../etc/passwd"), &[])
            .expect_err("relative traversal should fail");

        assert!(matches!(err, PathPolicyError::PermissionDenied(_)));
    }

    #[test]
    fn write_path_can_target_extra_allowed_root() {
        let workspace = tempfile::tempdir().expect("workspace");
        let extra = tempfile::tempdir().expect("extra");
        let policy = PathPolicy::new(workspace.path().to_path_buf())
            .with_allowed_roots(vec![extra.path().to_path_buf()]);

        let target = extra.path().join("out.txt");
        let resolved = policy
            .resolve_write_path(&target, false)
            .expect("write path should be allowed");

        assert_eq!(resolved, target);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_blocked_even_under_allowed_root() {
        use std::os::unix::fs::symlink;

        let workspace = tempfile::tempdir().expect("workspace");
        let outside = tempfile::tempdir().expect("outside");
        let outside_file = outside.path().join("outside.txt");
        std::fs::write(&outside_file, "outside").expect("write outside file");

        let link = workspace.path().join("escape.txt");
        symlink(&outside_file, &link).expect("create symlink");

        let policy = PathPolicy::new(workspace.path().to_path_buf());
        let err = policy
            .resolve_read_path(Path::new("escape.txt"), &[])
            .expect_err("symlink escape should fail");

        assert!(matches!(err, PathPolicyError::PermissionDenied(_)));
    }
}
