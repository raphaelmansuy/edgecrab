//! Path traversal prevention — jail check.
//!
//! Ensures resolved paths stay within an allowed root directory.
//! Prevents `../../../etc/passwd` style attacks.

use std::path::{Path, PathBuf};

use edgecrab_types::AgentError;

/// Resolve and validate a path against a jail directory.
///
/// Returns the canonicalized path only if it stays within `jail`.
/// Rejects symlink escapes by canonicalizing first.
pub fn resolve_safe_path(path: &str, jail: &Path) -> Result<PathBuf, AgentError> {
    // Canonicalize jail first to handle symlink roots (e.g. macOS /var → /private/var)
    let canon_jail = jail.canonicalize().map_err(|_| {
        AgentError::Security(format!("Jail directory not accessible: {}", jail.display()))
    })?;

    let candidate = canon_jail.join(path);

    let resolved = candidate
        .canonicalize()
        .map_err(|_| AgentError::Security(format!("Path traversal blocked: {path}")))?;

    if !resolved.starts_with(&canon_jail) {
        return Err(AgentError::Security(format!(
            "Path traversal blocked: {path} resolves outside jail"
        )));
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn allows_valid_path() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello").expect("write");

        let result = resolve_safe_path("test.txt", dir.path());
        assert!(result.is_ok());
        assert_eq!(
            result.expect("ok"),
            file_path.canonicalize().expect("canon")
        );
    }

    #[test]
    fn blocks_traversal() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let result = resolve_safe_path("../../../etc/passwd", dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn blocks_nonexistent_path() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let result = resolve_safe_path("nonexistent/file.txt", dir.path());
        assert!(result.is_err());
    }
}
