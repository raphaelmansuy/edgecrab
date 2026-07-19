//! Subscription OAuth primitives (spec 024).
//!
//! Provider-specific login flows live in `edgecrab-proxy` (forwarder auth.json)
//! or `edgequake_llm` (Copilot GitHub device flow). This module holds shared
//! crypto and provider identifiers for the model router phase.

pub mod anthropic;
pub mod auth_store;
pub mod codex;
pub mod pkce;
pub mod runtime;

/// Hermes-compatible provider id for xAI Grok OAuth.
pub const XAI_OAUTH_PROVIDER: &str = "xai-oauth";

/// Hermes-compatible provider id for Nous Portal.
pub const NOUS_PROVIDER: &str = "nous";

/// CLI-facing aliases that resolve to [`XAI_OAUTH_PROVIDER`].
pub const XAI_OAUTH_ALIASES: &[&str] = &[
    "grok",
    "grok-oauth",
    "x-ai-oauth",
    "xai-grok-oauth",
    "super-grok",
    "super_grok",
];

pub fn is_xai_oauth_alias(target: &str) -> bool {
    let t = target.to_ascii_lowercase();
    t == XAI_OAUTH_PROVIDER || XAI_OAUTH_ALIASES.contains(&t.as_str())
}

/// CLI targets for Claude Pro OAuth (`edgecrab auth add claude-pro`).
pub const ANTHROPIC_OAUTH_ALIASES: &[&str] = &["anthropic", "claude-pro", "claude_pro", "claude"];

pub fn is_anthropic_oauth_alias(target: &str) -> bool {
    let t = target.to_ascii_lowercase();
    ANTHROPIC_OAUTH_ALIASES.contains(&t.as_str())
}

pub use anthropic::{
    AnthropicOAuthLoginOptions, anthropic_oauth_path, login_anthropic_oauth,
    read_anthropic_oauth_file, refresh_anthropic_from_store, remove_anthropic_oauth_file,
    resolve_anthropic_oauth_access_token,
};
pub use codex::{
    CodexDeviceLoginOptions, CodexDevicePrompt, OPENAI_CODEX_PROVIDER, codex_has_credentials,
    is_openai_codex_alias, login_codex_device_oauth, refresh_codex_from_store, remove_codex_oauth,
    resolve_codex_access_token,
};
pub use runtime::{
    EDGECRAB_XAI_AUTH_MODE_ENV, EDGECRAB_XAI_AUTH_MODE_KEY, EDGECRAB_XAI_AUTH_MODE_OAUTH,
    inject_subscription_oauth_env, prepare_openai_codex_compatible_env,
    provider_needs_xai_credentials, xai_auth_mode_label,
};
