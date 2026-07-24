//! xAI / Grok agent credentials (spec 020) — single owner for prepare + mode labels.
//!
//! DRY: login stays in `edgecrab-proxy`; this module only **resolves** for agent
//! provider construction and 401 refresh.
//!
//! SuperGrok OAuth experience principles:
//! - `super-grok/*` prefers OAuth when tokens exist (subscription path).
//! - `xai/*` prefers static `XAI_API_KEY` when set (console key path).
//! - Missing credentials always produce a one-shot CTA (`/login grok`).

use anyhow::{Result, anyhow};
use edgecrab_core::oauth::{
    EDGECRAB_XAI_AUTH_MODE_ENV, EDGECRAB_XAI_AUTH_MODE_KEY, EDGECRAB_XAI_AUTH_MODE_OAUTH,
    xai_auth_mode_label,
};

/// Re-export for call sites (agent create_provider).
pub use edgecrab_core::oauth::provider_needs_xai_credentials;

/// Credential resolution preference (catalog provider decides).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XaiAuthPreference {
    /// `xai/*` — env key wins; else SuperGrok OAuth from auth.json.
    PreferKey,
    /// `super-grok/*` — OAuth wins when tokens exist; else key; else clear CTA.
    PreferOauth,
}

/// Resolve preference from a catalog / user provider segment (`super-grok`, `xai`, …).
pub fn xai_auth_preference_for_provider(provider: &str) -> XaiAuthPreference {
    let catalog = edgecrab_core::ModelCatalog::catalog_provider_id(provider);
    match catalog.as_str() {
        "super-grok" => XaiAuthPreference::PreferOauth,
        _ => XaiAuthPreference::PreferKey,
    }
}

/// Resolve xAI credentials for agent chat (key-preferring default).
///
/// Precedence (deterministic for PreferKey):
/// 1. Non-empty `XAI_API_KEY` and `!force_refresh` → static/API key (or prior inject)
/// 2. Else SuperGrok OAuth in auth.json (`xai-oauth`) → bearer + base URL
/// 3. Else error with CTA
pub async fn prepare_xai_credentials(force_refresh: bool) -> Result<()> {
    prepare_xai_credentials_with_preference(force_refresh, XaiAuthPreference::PreferKey).await
}

/// Resolve xAI credentials with catalog-aware preference (SuperGrok vs API key).
pub async fn prepare_xai_credentials_with_preference(
    force_refresh: bool,
    preference: XaiAuthPreference,
) -> Result<()> {
    let env_key = std::env::var("XAI_API_KEY")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    let oauth_ready = super_grok_oauth_ready();
    let mode = std::env::var(EDGECRAB_XAI_AUTH_MODE_ENV).unwrap_or_default();

    // SuperGrok path: prefer live OAuth tokens over a leftover static key so
    // selecting super-grok/* always rides the subscription when signed in.
    let prefer_oauth_now =
        matches!(preference, XaiAuthPreference::PreferOauth) && oauth_ready && !force_refresh;

    if !force_refresh && !prefer_oauth_now && env_key.is_some() {
        if std::env::var(EDGECRAB_XAI_AUTH_MODE_ENV).is_err() {
            // SAFETY: session-scoped auth mode label for /status.
            unsafe {
                std::env::set_var(EDGECRAB_XAI_AUTH_MODE_ENV, EDGECRAB_XAI_AUTH_MODE_KEY);
            }
        }
        return Ok(());
    }

    if force_refresh {
        // Static console key cannot be refreshed via OAuth.
        if mode == EDGECRAB_XAI_AUTH_MODE_KEY && env_key.is_some() && !oauth_ready {
            return Ok(());
        }
        // PreferOauth force-refresh always goes through OAuth when tokens exist.
        if mode == EDGECRAB_XAI_AUTH_MODE_KEY && !oauth_ready {
            return Ok(());
        }
    }

    // PreferKey with no OAuth and no key falls through to error below.
    // PreferOauth with oauth_ready (or force_refresh) uses OAuth resolve.
    if !oauth_ready && env_key.is_none() {
        return Err(missing_credentials_error());
    }

    if !oauth_ready {
        // Key-only fallback (e.g. super-grok selected but only XAI_API_KEY present).
        if std::env::var(EDGECRAB_XAI_AUTH_MODE_ENV).is_err() {
            unsafe {
                std::env::set_var(EDGECRAB_XAI_AUTH_MODE_ENV, EDGECRAB_XAI_AUTH_MODE_KEY);
            }
        }
        return Ok(());
    }

    let auth_path = edgecrab_proxy::auth_path_for_provider(edgecrab_proxy::XAI_OAUTH_PROVIDER);
    let (bearer, base_url) = edgecrab_proxy::resolve_xai_credentials_async(
        &auth_path,
        edgecrab_proxy::XAI_OAUTH_PROVIDER,
        edgecrab_proxy::DEFAULT_XAI_API,
        0,
        force_refresh,
    )
    .await
    .map_err(|e| {
        anyhow!(
            "xAI Grok credentials unavailable: {e}\n\
             Fix one of:\n\
               edgecrab auth add grok     # SuperGrok / X Premium+ OAuth → ~/.edgecrab/auth.json\n\
               /login grok                # same flow in the TUI\n\
               export XAI_API_KEY=...     # static key from console.x.ai"
        )
    })?;

    // SAFETY: Provider construction / 401 refresh updates process-wide xAI credentials.
    unsafe {
        std::env::set_var("XAI_API_KEY", bearer);
        std::env::set_var("XAI_BASE_URL", base_url);
        std::env::set_var(EDGECRAB_XAI_AUTH_MODE_ENV, EDGECRAB_XAI_AUTH_MODE_OAUTH);
    }
    tracing::info!(
        force_refresh,
        preference = ?preference,
        "xAI credentials ready (oauth or refresh)"
    );
    Ok(())
}

fn missing_credentials_error() -> anyhow::Error {
    anyhow!(
        "xAI Grok credentials unavailable: not signed in\n\
         Fix one of:\n\
           /login grok                # SuperGrok / X Premium+ (recommended)\n\
           edgecrab auth add grok     # same flow from the shell\n\
           export XAI_API_KEY=...     # static key from console.x.ai"
    )
}

/// Whether SuperGrok OAuth tokens exist on disk (no network).
pub fn super_grok_oauth_ready() -> bool {
    let auth_path = edgecrab_proxy::auth_path_for_provider(edgecrab_proxy::XAI_OAUTH_PROVIDER);
    auth_path.is_file()
        && std::fs::read_to_string(&auth_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .is_some_and(|doc| {
                doc.pointer("/providers/xai-oauth/tokens/access_token")
                    .or_else(|| doc.pointer("/providers/xai-oauth/tokens/refresh_token"))
                    .and_then(|v| v.as_str())
                    .is_some_and(|t| !t.trim().is_empty())
            })
}

/// True when an error means SuperGrok/xAI auth is missing (open login, don't thrash).
pub fn is_xai_auth_missing_error(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    (e.contains("xai") || e.contains("grok") || e.contains("credentials unavailable"))
        && (e.contains("not signed in")
            || e.contains("not configured")
            || e.contains("credentials unavailable")
            || e.contains("login grok")
            || e.contains("auth add grok")
            || e.contains("no refresh")
            || e.contains("missing"))
}

/// Auth readiness for model picker badges (`Some(true/false)` only for xAI family).
pub fn model_picker_auth_ready(provider: &str) -> Option<bool> {
    let p = edgecrab_core::ModelCatalog::catalog_provider_id(provider);
    match p.as_str() {
        "super-grok" => Some(super_grok_oauth_ready()),
        "xai" => {
            let has_key = std::env::var("XAI_API_KEY")
                .ok()
                .is_some_and(|v| !v.trim().is_empty());
            Some(has_key || super_grok_oauth_ready())
        }
        _ => None,
    }
}

/// One-line operator status for TUI / doctor.
pub fn xai_credential_status_line() -> String {
    let mode = xai_auth_mode_label();
    let has_key = std::env::var("XAI_API_KEY")
        .ok()
        .is_some_and(|v| !v.trim().is_empty());
    let auth_path = edgecrab_proxy::auth_path_for_provider(edgecrab_proxy::XAI_OAUTH_PROVIDER);
    let has_oauth_file = auth_path.is_file()
        && std::fs::read_to_string(&auth_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .is_some_and(|doc| {
                doc.pointer("/providers/xai-oauth").is_some()
                    || doc
                        .pointer("/providers/xai-oauth/tokens/access_token")
                        .is_some()
            });

    match mode {
        "oauth" => "xai · SuperGrok OAuth".into(),
        "api_key" if has_key => "xai · API key".into(),
        _ if has_oauth_file => "xai · OAuth on disk (not loaded)".into(),
        _ => "xai · not configured — /login grok or XAI_API_KEY".into(),
    }
}

/// Friendly success line after `/login grok` finishes.
pub fn super_grok_login_success_agent_hint(pending_model: Option<&str>) -> String {
    match pending_model {
        Some(m) if !m.trim().is_empty() => {
            format!("SuperGrok signed in. Switching to {m}…")
        }
        _ => "SuperGrok signed in. Use /model super-grok/grok-4.5 (or pick SuperGrok in /model)."
            .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_needs_covers_xai_grok_super_grok() {
        assert!(provider_needs_xai_credentials("xai"));
        assert!(provider_needs_xai_credentials("XAI"));
        assert!(provider_needs_xai_credentials("grok"));
        assert!(provider_needs_xai_credentials("super-grok"));
        assert!(provider_needs_xai_credentials("super_grok"));
        assert!(!provider_needs_xai_credentials("anthropic"));
        assert!(!provider_needs_xai_credentials("openai"));
    }

    #[test]
    fn preference_super_grok_prefers_oauth() {
        assert_eq!(
            xai_auth_preference_for_provider("super-grok"),
            XaiAuthPreference::PreferOauth
        );
        assert_eq!(
            xai_auth_preference_for_provider("xai"),
            XaiAuthPreference::PreferKey
        );
        assert_eq!(
            xai_auth_preference_for_provider("grok"),
            XaiAuthPreference::PreferKey
        );
    }

    #[tokio::test]
    #[serial_test::serial(edgecrab_home_env)]
    #[allow(clippy::await_holding_lock)] // intentional: env isolation mutex across prepare await
    async fn prepare_uses_static_key_without_auth_json() {
        let _guard = crate::gateway_catalog::lock_test_env();
        let home = tempfile::tempdir().expect("home");
        // SAFETY: test isolation under shared env lock.
        unsafe {
            std::env::set_var("XAI_API_KEY", "test-static-key");
            std::env::remove_var(EDGECRAB_XAI_AUTH_MODE_ENV);
            std::env::set_var("EDGECRAB_HOME", home.path());
        }
        prepare_xai_credentials(false).await.expect("prepare");
        assert_eq!(
            std::env::var(EDGECRAB_XAI_AUTH_MODE_ENV).ok().as_deref(),
            Some(EDGECRAB_XAI_AUTH_MODE_KEY)
        );
        unsafe {
            std::env::remove_var("XAI_API_KEY");
            std::env::remove_var(EDGECRAB_XAI_AUTH_MODE_ENV);
            std::env::remove_var("EDGECRAB_HOME");
        }
    }

    #[tokio::test]
    #[serial_test::serial(edgecrab_home_env)]
    #[allow(clippy::await_holding_lock)] // intentional: env isolation mutex across prepare await
    async fn prepare_loads_oauth_from_auth_json() {
        let _guard = crate::gateway_catalog::lock_test_env();
        let home = tempfile::tempdir().expect("home");
        let edge = home.path().join(".edgecrab");
        std::fs::create_dir_all(&edge).expect("dir");
        // edgecrab_home uses EDGECRAB_HOME or ~/.edgecrab — set EDGECRAB_HOME to edge.
        unsafe {
            std::env::set_var("EDGECRAB_HOME", &edge);
            std::env::remove_var("XAI_API_KEY");
            std::env::remove_var(EDGECRAB_XAI_AUTH_MODE_ENV);
        }
        let auth = edge.join("auth.json");
        let doc = serde_json::json!({
            "providers": {
                "xai-oauth": {
                    "auth_mode": "oauth_pkce",
                    "tokens": {
                        "access_token": "at-from-oauth",
                        "refresh_token": "rt-test"
                    },
                    "base_url": "https://api.x.ai/v1"
                }
            }
        });
        std::fs::write(&auth, doc.to_string()).expect("write");

        prepare_xai_credentials(false).await.expect("oauth prepare");
        assert_eq!(
            std::env::var("XAI_API_KEY").ok().as_deref(),
            Some("at-from-oauth")
        );
        assert_eq!(
            std::env::var(EDGECRAB_XAI_AUTH_MODE_ENV).ok().as_deref(),
            Some(EDGECRAB_XAI_AUTH_MODE_OAUTH)
        );

        unsafe {
            std::env::remove_var("XAI_API_KEY");
            std::env::remove_var("XAI_BASE_URL");
            std::env::remove_var(EDGECRAB_XAI_AUTH_MODE_ENV);
            std::env::remove_var("EDGECRAB_HOME");
        }
    }

    #[tokio::test]
    #[serial_test::serial(edgecrab_home_env)]
    #[allow(clippy::await_holding_lock)] // intentional: env isolation mutex across prepare await
    async fn prefer_oauth_overrides_static_key_when_tokens_present() {
        let _guard = crate::gateway_catalog::lock_test_env();
        let home = tempfile::tempdir().expect("home");
        let edge = home.path().join(".edgecrab");
        std::fs::create_dir_all(&edge).expect("dir");
        unsafe {
            std::env::set_var("EDGECRAB_HOME", &edge);
            std::env::set_var("XAI_API_KEY", "static-console-key");
            std::env::remove_var(EDGECRAB_XAI_AUTH_MODE_ENV);
        }
        let auth = edge.join("auth.json");
        let doc = serde_json::json!({
            "providers": {
                "xai-oauth": {
                    "tokens": {
                        "access_token": "at-oauth-wins",
                        "refresh_token": "rt-test"
                    },
                    "base_url": "https://api.x.ai/v1"
                }
            }
        });
        std::fs::write(&auth, doc.to_string()).expect("write");

        prepare_xai_credentials_with_preference(false, XaiAuthPreference::PreferOauth)
            .await
            .expect("oauth prefer");
        assert_eq!(
            std::env::var("XAI_API_KEY").ok().as_deref(),
            Some("at-oauth-wins"),
            "super-grok must ride OAuth when signed in, not a leftover shell key"
        );
        assert_eq!(
            std::env::var(EDGECRAB_XAI_AUTH_MODE_ENV).ok().as_deref(),
            Some(EDGECRAB_XAI_AUTH_MODE_OAUTH)
        );

        unsafe {
            std::env::remove_var("XAI_API_KEY");
            std::env::remove_var("XAI_BASE_URL");
            std::env::remove_var(EDGECRAB_XAI_AUTH_MODE_ENV);
            std::env::remove_var("EDGECRAB_HOME");
        }
    }

    #[test]
    #[serial_test::serial(edgecrab_home_env)]
    fn status_line_missing_when_unconfigured() {
        let _guard = crate::gateway_catalog::lock_test_env();
        let home = tempfile::tempdir().expect("home");
        unsafe {
            std::env::set_var("EDGECRAB_HOME", home.path().join(".edgecrab"));
            std::env::remove_var("XAI_API_KEY");
            std::env::remove_var(EDGECRAB_XAI_AUTH_MODE_ENV);
        }
        let line = xai_credential_status_line();
        assert!(
            line.contains("not configured") || line.contains("login"),
            "{line}"
        );
        unsafe {
            std::env::remove_var("EDGECRAB_HOME");
        }
    }

    #[test]
    fn auth_missing_error_classifier() {
        assert!(is_xai_auth_missing_error(
            "xAI Grok credentials unavailable: not signed in\nFix: /login grok"
        ));
        assert!(!is_xai_auth_missing_error("rate limit 429"));
    }
}
