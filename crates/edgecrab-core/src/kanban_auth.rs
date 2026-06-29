//! Kanban dashboard API token — Hermes session-token subset for `/api/kanban/*`.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use edgecrab_types::AgentError;
use subtle::ConstantTimeEq;

use crate::config::KanbanConfig;
use crate::edgecrab_home;

/// Default token file when `kanban.api_token_path` is unset.
pub fn default_kanban_token_path() -> PathBuf {
    edgecrab_home().join("kanban-token")
}

/// Resolved token path from config.
pub fn resolved_kanban_token_path(cfg: &KanbanConfig) -> PathBuf {
    cfg.api_token_path
        .clone()
        .unwrap_or_else(default_kanban_token_path)
}

/// Load token from disk; `None` when file missing and auth not required.
pub fn load_kanban_api_token(cfg: &KanbanConfig) -> Result<Option<String>, AgentError> {
    if !cfg.require_api_auth {
        return Ok(None);
    }
    let path = resolved_kanban_token_path(cfg);
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path).map_err(AgentError::Io)?;
    let token = raw.trim().to_string();
    if token.is_empty() {
        return Err(AgentError::Validation(format!(
            "kanban API token file is empty: {}",
            path.display()
        )));
    }
    Ok(Some(token))
}

/// Create token file when missing (`chmod 0600` on Unix).
pub fn write_kanban_api_token(path: &Path, token: Option<&str>) -> Result<String, AgentError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(AgentError::Io)?;
    }
    let value = match token {
        Some(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => uuid::Uuid::new_v4().to_string(),
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(AgentError::Io)?;
    file.write_all(value.as_bytes()).map_err(AgentError::Io)?;
    file.write_all(b"\n").map_err(AgentError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(AgentError::Io)?;
    }
    Ok(value)
}

/// Ensure token exists when auth is required.
pub fn ensure_kanban_api_token(cfg: &KanbanConfig) -> Result<Option<String>, AgentError> {
    if !cfg.require_api_auth {
        return Ok(None);
    }
    let path = resolved_kanban_token_path(cfg);
    if path.exists() {
        return load_kanban_api_token(cfg);
    }
    let token = write_kanban_api_token(&path, None)?;
    Ok(Some(token))
}

fn is_loopback_host(host: &str) -> bool {
    host == "127.0.0.1" || host == "::1" || host.eq_ignore_ascii_case("localhost")
}

/// Timing-safe token check (Bearer, `X-Kanban-Token`, or query `token=`).
pub fn check_kanban_token(
    cfg: &KanbanConfig,
    gateway_bind_host: &str,
    bearer: Option<&str>,
    header_token: Option<&str>,
    query_token: Option<&str>,
) -> Result<(), &'static str> {
    let expected = match load_kanban_api_token(cfg) {
        Ok(v) => v,
        Err(_) => return Err("kanban token misconfigured"),
    };
    let Some(expected) = expected else {
        return Ok(());
    };
    if cfg.allow_localhost_without_auth && is_loopback_host(gateway_bind_host) {
        return Ok(());
    }
    let provided = bearer
        .or(header_token)
        .or(query_token)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(provided) = provided else {
        return Err("missing kanban API token");
    };
    if !bool::from(provided.as_bytes().ct_eq(expected.as_bytes())) {
        return Err("invalid kanban API token");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn token_round_trip() {
        let dir = TempDir::new().expect("tmpdir");
        let path = dir.path().join("kanban-token");
        let token = write_kanban_api_token(&path, Some("secret")).expect("write");
        assert_eq!(token, "secret");
        let cfg = KanbanConfig {
            require_api_auth: true,
            api_token_path: Some(path),
            allow_localhost_without_auth: false,
            ..Default::default()
        };
        assert_eq!(
            load_kanban_api_token(&cfg).expect("load"),
            Some("secret".into())
        );
    }

    #[test]
    fn localhost_bypass_when_enabled() {
        let dir = TempDir::new().expect("tmpdir");
        let path = dir.path().join("kanban-token");
        write_kanban_api_token(&path, Some("secret")).expect("write");
        let cfg = KanbanConfig {
            require_api_auth: true,
            api_token_path: Some(path),
            allow_localhost_without_auth: true,
            ..Default::default()
        };
        assert!(check_kanban_token(&cfg, "127.0.0.1", None, None, None).is_ok());
        assert!(check_kanban_token(&cfg, "0.0.0.0", None, None, None).is_err());
        assert!(check_kanban_token(&cfg, "0.0.0.0", Some("secret"), None, None).is_ok());
    }
}
