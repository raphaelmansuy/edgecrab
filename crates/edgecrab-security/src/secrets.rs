//! Secret resolution chain — env / keychain / vault backends (gap 032).
//!
//! Call sites depend on [`SecretResolver`], not on `std::env` directly.
//! Unconfigured installs behave identically to today: env backend only.

use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;

/// Ordered secret lookup. First hit wins.
#[derive(Clone)]
pub struct SecretResolver {
    backends: Vec<Arc<dyn SecretBackend>>,
}

impl Default for SecretResolver {
    fn default() -> Self {
        Self::env_only()
    }
}

impl SecretResolver {
    /// Env / `.env` only — backwards-compatible default.
    pub fn env_only() -> Self {
        Self {
            backends: vec![Arc::new(EnvSecretBackend)],
        }
    }

    pub fn with_backends(backends: Vec<Arc<dyn SecretBackend>>) -> Self {
        Self { backends }
    }

    /// Resolve a secret by conventional env name (`ANTHROPIC_API_KEY`, …).
    pub fn resolve(&self, name: &str) -> Option<SecretString> {
        for backend in &self.backends {
            if let Some(value) = backend.get(name) {
                let exposed = value.expose_secret();
                if !exposed.trim().is_empty() {
                    return Some(value);
                }
            }
        }
        None
    }

    /// Convenience: resolve and return plain String (callers must not log it).
    pub fn resolve_string(&self, name: &str) -> Option<String> {
        self.resolve(name).map(|s| s.expose_secret().to_string())
    }

    /// Store via the first writable backend (keychain / file). Env backend is read-only.
    pub fn set(&self, name: &str, value: &str) -> Result<(), SecretError> {
        for backend in &self.backends {
            if backend.is_writable() {
                return backend.set(name, value);
            }
        }
        Err(SecretError::NoWritableBackend)
    }

    pub fn list_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for backend in &self.backends {
            for n in backend.list_names() {
                if !names.iter().any(|existing| existing == &n) {
                    names.push(n);
                }
            }
        }
        names.sort();
        names
    }
}

/// Pluggable secret store.
pub trait SecretBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn get(&self, key: &str) -> Option<SecretString>;
    fn set(&self, _key: &str, _value: &str) -> Result<(), SecretError> {
        Err(SecretError::ReadOnly)
    }
    fn is_writable(&self) -> bool {
        false
    }
    fn list_names(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Process environment (and dotenv if already loaded by the CLI).
pub struct EnvSecretBackend;

impl SecretBackend for EnvSecretBackend {
    fn name(&self) -> &'static str {
        "env"
    }

    fn get(&self, key: &str) -> Option<SecretString> {
        std::env::var(key).ok().map(SecretString::from)
    }
}

/// File-backed store under `~/.edgecrab/secrets/<name>` (mode 0o600).
pub struct FileSecretBackend {
    root: std::path::PathBuf,
}

impl FileSecretBackend {
    pub fn new(root: std::path::PathBuf) -> Self {
        Self { root }
    }

    fn path_for(&self, key: &str) -> Option<std::path::PathBuf> {
        if key.is_empty()
            || key.contains('/')
            || key.contains('\\')
            || key.contains("..")
            || !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return None;
        }
        Some(self.root.join(key))
    }
}

impl SecretBackend for FileSecretBackend {
    fn name(&self) -> &'static str {
        "file"
    }

    fn get(&self, key: &str) -> Option<SecretString> {
        let path = self.path_for(key)?;
        let bytes = std::fs::read_to_string(path).ok()?;
        let trimmed = bytes.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(SecretString::from(trimmed.to_string()))
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        let path = self
            .path_for(key)
            .ok_or(SecretError::InvalidKey(key.to_string()))?;
        std::fs::create_dir_all(&self.root).map_err(SecretError::Io)?;
        std::fs::write(&path, value).map_err(SecretError::Io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&path, perms).map_err(SecretError::Io)?;
        }
        Ok(())
    }

    fn is_writable(&self) -> bool {
        true
    }

    fn list_names(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                e.file_type().ok()?.is_file().then_some(name)
            })
            .collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("secret backend is read-only")]
    ReadOnly,
    #[error("no writable secret backend configured")]
    NoWritableBackend,
    #[error("invalid secret key: {0}")]
    InvalidKey(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Build the default resolver: file store (writable) then env fallback.
pub fn default_resolver(edgecrab_home: &std::path::Path) -> SecretResolver {
    SecretResolver::with_backends(vec![
        Arc::new(FileSecretBackend::new(edgecrab_home.join("secrets"))),
        Arc::new(EnvSecretBackend),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn env_backend_resolves() {
        unsafe { std::env::set_var("EDGECRAB_TEST_SECRET_032", "from-env") };
        let r = SecretResolver::env_only();
        assert_eq!(
            r.resolve_string("EDGECRAB_TEST_SECRET_032").as_deref(),
            Some("from-env")
        );
        unsafe { std::env::remove_var("EDGECRAB_TEST_SECRET_032") };
    }

    #[test]
    fn file_backend_wins_over_env() {
        let dir = TempDir::new().expect("tmpdir");
        let file = FileSecretBackend::new(dir.path().join("secrets"));
        file.set("MY_KEY", "from-file").expect("set");
        unsafe { std::env::set_var("MY_KEY", "from-env") };
        let r = SecretResolver::with_backends(vec![Arc::new(file), Arc::new(EnvSecretBackend)]);
        assert_eq!(r.resolve_string("MY_KEY").as_deref(), Some("from-file"));
        unsafe { std::env::remove_var("MY_KEY") };
    }

    #[test]
    fn rejects_path_traversal_keys() {
        let dir = TempDir::new().expect("tmpdir");
        let file = FileSecretBackend::new(dir.path().to_path_buf());
        assert!(matches!(
            file.set("../etc/passwd", "x"),
            Err(SecretError::InvalidKey(_))
        ));
    }
}
