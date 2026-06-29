//! Credential file resolution for skill setup (Hermes `required_credential_files` parity).
//!
//! Paths are relative to `~/.edgecrab/` (not the skill directory). Absolute paths and
//! `..` traversal are rejected — same security model as hermes-agent `credential_files.py`.

use std::path::{Component, Path};

use edgecrab_security::path_jail::resolve_safe_path;

/// Declared in SKILL.md frontmatter (`required_credential_files`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialFileSpec {
    pub path: String,
    pub description: Option<String>,
}

/// A credential file declared by the skill but absent on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCredentialRequirement {
    pub path: String,
    pub description: Option<String>,
}

/// Return credential files declared by the skill that are missing under `home`.
pub fn missing_credential_files(
    home: &Path,
    specs: &[CredentialFileSpec],
) -> Vec<SkillCredentialRequirement> {
    specs
        .iter()
        .filter(|spec| !spec.path.trim().is_empty() && !credential_file_present(home, &spec.path))
        .map(|spec| SkillCredentialRequirement {
            path: spec.path.clone(),
            description: spec.description.clone(),
        })
        .collect()
}

/// True when `rel` resolves to an existing regular file inside `home`.
pub fn credential_file_present(home: &Path, rel: &str) -> bool {
    let rel = rel.trim();
    if rel.is_empty() || Path::new(rel).is_absolute() {
        return false;
    }
    if !path_stays_in_jail(home, rel) {
        return false;
    }
    if let Ok(resolved) = resolve_safe_path(rel, home) {
        return resolved.is_file();
    }
    home.join(rel).is_file()
}

fn path_stays_in_jail(home: &Path, rel: &str) -> bool {
    let canon_home = match home.canonicalize() {
        Ok(h) => h,
        Err(_) => home.to_path_buf(),
    };
    let mut candidate = canon_home.clone();
    for component in Path::new(rel).components() {
        match component {
            Component::Normal(part) => candidate = candidate.join(part),
            Component::CurDir => {}
            Component::ParentDir => return false,
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    candidate.starts_with(&canon_home)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn present_when_file_exists_in_home() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("token.json"), "{}").expect("write");
        let specs = vec![CredentialFileSpec {
            path: "token.json".into(),
            description: None,
        }];
        assert!(missing_credential_files(dir.path(), &specs).is_empty());
    }

    #[test]
    fn missing_when_file_absent() {
        let dir = TempDir::new().expect("tmpdir");
        let specs = vec![CredentialFileSpec {
            path: "token.json".into(),
            description: Some("OAuth token".into()),
        }];
        let missing = missing_credential_files(dir.path(), &specs);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].path, "token.json");
    }

    #[test]
    fn rejects_traversal_paths() {
        let dir = TempDir::new().expect("tmpdir");
        let specs = vec![CredentialFileSpec {
            path: "../secret".into(),
            description: None,
        }];
        let missing = missing_credential_files(dir.path(), &specs);
        assert_eq!(missing.len(), 1);
    }

    #[test]
    fn rejects_absolute_paths() {
        let dir = TempDir::new().expect("tmpdir");
        let specs = vec![CredentialFileSpec {
            path: "/etc/passwd".into(),
            description: None,
        }];
        assert_eq!(missing_credential_files(dir.path(), &specs).len(), 1);
    }
}
