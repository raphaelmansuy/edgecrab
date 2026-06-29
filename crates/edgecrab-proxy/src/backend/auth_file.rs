//! Hermes-format `auth.json` load/save (shared by static read and Nous OAuth refresh).

use std::path::{Path, PathBuf};

use edgecrab_core::config::edgecrab_home;
use serde_json::{Map, Value};

use crate::error::ProxyError;

use super::auth_lock::with_auth_store_lock;
use super::auth_store::provider_state_from_doc;

/// Default auth store: `~/.edgecrab/auth.json` (created by `edgecrab auth add`).
pub fn default_auth_path() -> PathBuf {
    edgecrab_home().join("auth.json")
}

fn hermes_auth_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".hermes").join("auth.json"))
}

fn doc_has_provider(doc: &Value, provider: &str) -> bool {
    if super::auth_store::provider_state_from_doc(doc, provider).is_some() {
        return true;
    }
    !read_credential_pool_entries(doc, provider).is_empty()
}

/// Resolve which auth.json holds `provider` — EdgeCrab first, Hermes only when populated.
pub fn auth_path_for_provider(provider: &str) -> PathBuf {
    let edgecrab = default_auth_path();
    if edgecrab.is_file()
        && load_auth_doc(&edgecrab)
            .ok()
            .is_some_and(|doc| doc_has_provider(&doc, provider))
    {
        return edgecrab;
    }
    if let Some(hermes) = hermes_auth_path().filter(|p| p.is_file())
        && load_auth_doc(&hermes)
            .ok()
            .is_some_and(|doc| doc_has_provider(&doc, provider))
    {
        return hermes;
    }
    edgecrab
}

pub fn load_auth_doc(path: &Path) -> Result<Value, ProxyError> {
    with_auth_store_lock(path, || {
        let raw = std::fs::read_to_string(path).map_err(|e| {
            ProxyError::UpstreamAuth(format!("cannot read auth file {}: {e}", path.display()))
        })?;
        serde_json::from_str(&raw).map_err(|e| {
            ProxyError::UpstreamAuth(format!("invalid JSON in {}: {e}", path.display()))
        })
    })
}

pub fn read_provider_state(path: &Path, provider: &str) -> Result<Value, ProxyError> {
    let doc = load_auth_doc(path)?;
    provider_state_from_doc(&doc, provider).ok_or_else(|| {
        ProxyError::UpstreamAuth(format!(
            "provider '{provider}' not found in {}",
            path.display()
        ))
    })
}

/// Persist the full auth document (under file lock).
pub fn write_auth_doc(path: &Path, doc: &Value) -> Result<(), ProxyError> {
    with_auth_store_lock(path, || {
        let bytes = serde_json::to_vec_pretty(doc)
            .map_err(|e| ProxyError::UpstreamAuth(format!("serialize auth store: {e}")))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ProxyError::UpstreamAuth(format!("create auth dir {}: {e}", parent.display()))
            })?;
        }
        std::fs::write(path, bytes).map_err(|e| {
            ProxyError::UpstreamAuth(format!("write auth file {}: {e}", path.display()))
        })
    })
}

/// Remove a provider entry from the auth document.
pub fn remove_provider_state(path: &Path, provider: &str) -> Result<(), ProxyError> {
    with_auth_store_lock(path, || {
        if !path.exists() {
            return Ok(());
        }
        let raw = std::fs::read_to_string(path).map_err(|e| {
            ProxyError::UpstreamAuth(format!("cannot read auth file {}: {e}", path.display()))
        })?;
        let mut doc: Value = serde_json::from_str(&raw).map_err(|e| {
            ProxyError::UpstreamAuth(format!("invalid JSON in {}: {e}", path.display()))
        })?;
        if let Some(providers) = doc.get_mut("providers").and_then(|v| v.as_object_mut()) {
            providers.remove(provider);
        }
        if let Some(pool) = doc
            .get_mut("credential_pool")
            .and_then(|v| v.as_object_mut())
        {
            pool.remove(provider);
        }
        write_auth_doc(path, &doc)
    })
}

/// Merge provider state into the document and persist atomically (best-effort).
pub fn write_provider_state(path: &Path, provider: &str, state: &Value) -> Result<(), ProxyError> {
    with_auth_store_lock(path, || {
        let mut doc = if path.exists() {
            let raw = std::fs::read_to_string(path).map_err(|e| {
                ProxyError::UpstreamAuth(format!("cannot read auth file {}: {e}", path.display()))
            })?;
            serde_json::from_str(&raw).map_err(|e| {
                ProxyError::UpstreamAuth(format!("invalid JSON in {}: {e}", path.display()))
            })?
        } else {
            Value::Object(Map::new())
        };
        let providers = doc.as_object_mut().ok_or_else(|| {
            ProxyError::UpstreamAuth("auth store root must be a JSON object".into())
        })?;
        let map = providers
            .entry("providers")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(obj) = map.as_object_mut() {
            obj.insert(provider.to_string(), state.clone());
        } else {
            return Err(ProxyError::UpstreamAuth(
                "auth store providers field must be an object".into(),
            ));
        }
        write_auth_doc(path, &doc)
    })
}

/// Hermes `credential_pool.<provider>` entries (list of dicts).
pub fn read_credential_pool_entries(doc: &Value, provider: &str) -> Vec<Value> {
    doc.get("credential_pool")
        .and_then(|p| p.get(provider))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter(|e| e.is_object()).cloned().collect())
        .unwrap_or_default()
}

/// Bearer + base URL from a pool row or provider singleton (`tokens`, `access_token`, `agent_key`).
pub fn bearer_and_base_from_entry(entry: &Value, fallback_base: &str) -> Option<(String, String)> {
    let bearer = entry
        .get("access_token")
        .or_else(|| entry.get("runtime_api_key"))
        .or_else(|| entry.get("agent_key"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            entry
                .get("tokens")
                .and_then(|t| t.get("access_token"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })?;
    let base = entry
        .get("base_url")
        .or_else(|| entry.get("runtime_base_url"))
        .and_then(|v| v.as_str())
        .map(|s| s.trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback_base.trim_end_matches('/').to_string());
    Some((bearer, base))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_pool_and_tokens_access() {
        let doc = serde_json::json!({
            "credential_pool": {
                "xai-oauth": [{"access_token": "pool-tok", "base_url": "https://api.x.ai/v1"}]
            },
            "providers": {
                "xai-oauth": {
                    "tokens": { "access_token": "singleton-tok" }
                }
            }
        });
        let pool = read_credential_pool_entries(&doc, "xai-oauth");
        assert_eq!(pool.len(), 1);
        let (b, _) =
            bearer_and_base_from_entry(&pool[0], "https://fallback/v1").expect("pool entry bearer");
        assert_eq!(b, "pool-tok");
        let state = provider_state_from_doc(&doc, "xai-oauth").expect("provider state");
        let (b2, _) =
            bearer_and_base_from_entry(&state, "https://fallback/v1").expect("state bearer");
        assert_eq!(b2, "singleton-tok");
    }

    #[test]
    fn auth_path_for_provider_prefers_edgecrab_when_both_have_provider() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ec = dir.path().join("edgecrab");
        let hermes_dir = dir.path().join(".hermes");
        std::fs::create_dir_all(&ec).expect("mkdir ec");
        std::fs::create_dir_all(&hermes_dir).expect("mkdir hermes");
        let doc = serde_json::json!({
            "providers": {
                "xai-oauth": { "tokens": { "access_token": "tok" } }
            }
        });
        std::fs::write(ec.join("auth.json"), doc.to_string()).expect("write ec");
        std::fs::write(hermes_dir.join("auth.json"), doc.to_string()).expect("write hm");
        // SAFETY: test-only env isolation.
        unsafe {
            std::env::set_var("EDGECRAB_HOME", ec.to_str().expect("utf8"));
            std::env::set_var("HOME", dir.path().to_str().expect("utf8"));
        }
        assert_eq!(
            auth_path_for_provider("xai-oauth"),
            ec.join("auth.json")
        );
    }

    #[test]
    fn auth_path_for_provider_returns_edgecrab_when_neither_has_provider() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ec = dir.path().join("edgecrab");
        let hermes_dir = dir.path().join(".hermes");
        std::fs::create_dir_all(&ec).expect("mkdir ec");
        std::fs::create_dir_all(&hermes_dir).expect("mkdir hermes");
        std::fs::write(hermes_dir.join("auth.json"), r#"{"providers":{}}"#).expect("write hm");
        // SAFETY: test-only env isolation.
        unsafe {
            std::env::set_var("EDGECRAB_HOME", ec.to_str().expect("utf8"));
            std::env::set_var("HOME", dir.path().to_str().expect("utf8"));
        }
        assert_eq!(
            auth_path_for_provider("xai-oauth"),
            ec.join("auth.json")
        );
    }
}
