//! Inject subscription OAuth tokens into process env before provider construction.

use super::anthropic::resolve_anthropic_oauth_access_token;
use super::codex::{DEFAULT_CODEX_BASE_URL, resolve_codex_access_token};
use super::{is_xai_oauth_alias, XAI_OAUTH_PROVIDER};

fn env_nonempty(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .is_some_and(|v| !v.trim().is_empty())
}

fn anthropic_key_from_env() -> bool {
    env_nonempty("ANTHROPIC_API_KEY") || env_nonempty("ANTHROPIC_AUTH_TOKEN")
}

fn openai_key_from_env() -> bool {
    env_nonempty("OPENAI_API_KEY")
}

/// True when the model provider segment needs xAI Grok credentials (key or OAuth).
///
/// Covers: `xai`, `grok`, `super-grok`, and OAuth aliases.
pub fn provider_needs_xai_credentials(provider: &str) -> bool {
    let p = provider.to_ascii_lowercase();
    matches!(p.as_str(), "xai" | "grok" | "super-grok" | "super_grok")
        || is_xai_oauth_alias(&p)
}

/// Env flag set when `XAI_API_KEY` was injected from SuperGrok OAuth (not a static key).
pub const EDGECRAB_XAI_AUTH_MODE_ENV: &str = "EDGECRAB_XAI_AUTH_MODE";
pub const EDGECRAB_XAI_AUTH_MODE_OAUTH: &str = "oauth";
pub const EDGECRAB_XAI_AUTH_MODE_KEY: &str = "api_key";

/// Set `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` from OAuth stores when env is unset.
pub async fn inject_subscription_oauth_env(provider: &str) -> Result<(), String> {
    let canonical = provider.to_ascii_lowercase();
    match canonical.as_str() {
        "anthropic" | "claude-pro" | "claude" => {
            if !anthropic_key_from_env()
                && let Some(token) = resolve_anthropic_oauth_access_token().await?
            {
                // SAFETY: provider construction runs once per session startup.
                unsafe { std::env::set_var("ANTHROPIC_API_KEY", token) };
            }
        }
        "openai-codex" | "chatgpt-pro" | "codex" => {
            if !openai_key_from_env()
                && let Some(token) = resolve_codex_access_token().await?
            {
                unsafe { std::env::set_var("OPENAI_API_KEY", token) };
            }
        }
        // xAI is handled by CLI/gateway `prepare_xai_credentials` (needs proxy refresh).
        _ if provider_needs_xai_credentials(&canonical) => {}
        _ => {}
    }
    let _ = XAI_OAUTH_PROVIDER; // keep symbol used for docs/callers
    Ok(())
}

/// Classify current process xAI auth mode for TUI/status (deterministic).
pub fn xai_auth_mode_label() -> &'static str {
    match std::env::var(EDGECRAB_XAI_AUTH_MODE_ENV).as_deref() {
        Ok(EDGECRAB_XAI_AUTH_MODE_OAUTH) => "oauth",
        Ok(EDGECRAB_XAI_AUTH_MODE_KEY) => "api_key",
        _ if env_nonempty("XAI_API_KEY") => "api_key",
        _ => "missing",
    }
}

/// Map Codex OAuth bearer into `openai-compatible` env vars (edgequake-llm factory).
pub fn prepare_openai_codex_compatible_env() {
    if !env_nonempty("OPENAI_COMPATIBLE_BASE_URL") {
        unsafe { std::env::set_var("OPENAI_COMPATIBLE_BASE_URL", DEFAULT_CODEX_BASE_URL) };
    }
    if !env_nonempty("OPENAI_COMPATIBLE_API_KEY")
        && let Ok(key) = std::env::var("OPENAI_API_KEY")
        && !key.trim().is_empty()
    {
        unsafe { std::env::set_var("OPENAI_COMPATIBLE_API_KEY", key.trim()) };
    }
}
