//! # edgecrab-proxy
//!
//! Local OpenAI-compatible HTTP server that exposes EdgeCrab-configured LLM
//! providers to third-party clients (Aider, OpenAI SDK, LiteLLM, etc.).
//!
//! ## Modes
//!
//! - **Provider bridge (Mode B):** OpenAI JSON → [`edgequake_llm::LLMProvider`] → OpenAI JSON/SSE.
//! - **Credential forwarder (Mode A):** verbatim HTTP proxy with upstream OAuth bearer
//!   (Hermes `hermes proxy` style) — see [`backend::forwarder`].
//!
//! This is **not** the gateway API server: no agent loop, no tool execution.

#![deny(clippy::unwrap_used)]

pub mod auth;
pub mod backend;
pub mod cors;
pub mod error;
pub mod guide;
mod http_client;
pub mod oauth;
pub mod registry;
pub mod resolve;
pub mod server;
pub mod stream_agg;
pub mod wire;

/// Mock stack helpers for integration tests (`tests/e2e_grok_xai_http.rs`).
#[doc(hidden)]
pub mod e2e_harness;

pub use auth::{ensure_proxy_token, load_proxy_token, write_proxy_token};
pub use backend::adapter::describe_adapter;
pub use backend::auth_file::{auth_path_for_provider, default_auth_path, remove_provider_state};
pub use backend::nous::state_requires_relogin;
pub use backend::nous::{
    DEFAULT_NOUS_INFERENCE, NousDeviceLoginOptions, login_nous_portal, persist_nous_oauth,
    resolve_nous_credentials_async,
};
pub use backend::xai::{
    DEFAULT_XAI_API, PENDING_SESSION_MAX_AGE_SECS, XAI_OAUTH_PROVIDER, XaiOAuthAuthorizePrompt,
    XaiOAuthLoginOptions, XaiOAuthStarted, default_xai_pending_path,
    extract_xai_oauth_code_from_paste, finish_xai_oauth_login, login_xai_oauth,
    login_xai_oauth_finish, peek_xai_pending_session, resolve_xai_credentials_async,
    start_xai_oauth_login,
};
#[doc(hidden)]
pub use e2e_harness::e2e_http_client;
pub use error::ProxyError;
pub use guide::{
    ALL_RECIPES, AuthProbe, BuiltinRecipe, ClientSnippet, RECIPE_NOUS, RECIPE_XAI, apply_recipe,
    auth_probe_message, client_snippet, probe_oauth_auth, resolve_recipe,
};
pub use http_client::enable_e2e_direct_http;
pub use registry::{
    builtin_upstream_catalog_lines, ensure_forward_upstream_ready, format_upstream_status_table,
    list_forward_upstream_keys,
};
pub use resolve::build_forward_adapters;
pub use server::{ProxyRunOptions, ProxyState, build_router, run_server};
