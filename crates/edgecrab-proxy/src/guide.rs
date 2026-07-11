//! Built-in upstream recipes and client snippets (Hermes `ADAPTERS` as data).

use edgecrab_core::{ForwardAdapterKind, ForwardUpstreamConfig, ProxyConfig};

use crate::backend::auth_file::{bearer_and_base_from_entry, default_auth_path, load_auth_doc};
use crate::backend::auth_store::provider_state_from_doc;
use crate::backend::nous::state_requires_relogin;

/// One Hermes-style forward upstream preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinRecipe {
    pub key: &'static str,
    pub display_name: &'static str,
    pub adapter: ForwardAdapterKind,
    pub auth_provider: &'static str,
    pub base_url: &'static str,
    pub default_alias: &'static str,
    pub hermes_auth_cmd: &'static str,
}

pub const RECIPE_NOUS: BuiltinRecipe = BuiltinRecipe {
    key: "nous",
    display_name: "Nous Portal",
    adapter: ForwardAdapterKind::NousPortal,
    auth_provider: "nous",
    base_url: "https://inference-api.nousresearch.com/v1",
    default_alias: "nous-chat",
    hermes_auth_cmd: "edgecrab auth add nous",
};

pub const RECIPE_XAI: BuiltinRecipe = BuiltinRecipe {
    key: "xai",
    display_name: "xAI Grok OAuth",
    adapter: ForwardAdapterKind::XaiOauth,
    auth_provider: "xai-oauth",
    base_url: "https://api.x.ai/v1",
    default_alias: "grok",
    hermes_auth_cmd: "edgecrab auth add grok",
};

pub const ALL_RECIPES: &[BuiltinRecipe] = &[RECIPE_NOUS, RECIPE_XAI];

pub fn resolve_recipe(name: &str) -> Option<&'static BuiltinRecipe> {
    let n = name.trim().to_ascii_lowercase();
    ALL_RECIPES
        .iter()
        .find(|r| r.key == n || r.default_alias == n || (n == "grok" && r.key == "xai"))
}

/// Apply recipe to config (idempotent merge).
pub fn apply_recipe(cfg: &mut ProxyConfig, recipe: &BuiltinRecipe) {
    let upstream = cfg.forward_upstreams
        .entry(recipe.key.to_string())
        .or_insert_with(|| ForwardUpstreamConfig {
            base_url: recipe.base_url.into(),
            adapter: recipe.adapter,
            auth_provider: Some(recipe.auth_provider.into()),
            auth_hint: Some(recipe.hermes_auth_cmd.into()),
            ..Default::default()
        });
    if upstream.base_url.is_empty() {
        upstream.base_url = recipe.base_url.into();
    }
    cfg.model_aliases.insert(
        recipe.default_alias.to_string(),
        format!("forward:{}", recipe.key),
    );
    cfg.default_forward_upstream = Some(recipe.key.to_string());
}

/// Readiness from `auth.json` only (cheap; no network).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthProbe {
    Ready,
    MissingFile,
    ProviderMissing,
    NoBearer,
    /// Terminal OAuth failure — credentials quarantined in auth.json.
    ReloginRequired,
}

/// Like [`probe_oauth_auth`] but reads a specific auth file (tests and tooling).
#[doc(hidden)]
pub fn probe_oauth_auth_with_path(path: &std::path::Path, recipe: &BuiltinRecipe) -> AuthProbe {
    if !path.exists() {
        return AuthProbe::MissingFile;
    }
    let Ok(doc) = load_auth_doc(path) else {
        return AuthProbe::MissingFile;
    };
    let Some(state) = provider_state_from_doc(&doc, recipe.auth_provider) else {
        if recipe.key == "xai" {
            let pool =
                crate::backend::auth_file::read_credential_pool_entries(&doc, recipe.auth_provider);
            if pool
                .iter()
                .any(|e| bearer_and_base_from_entry(e, recipe.base_url).is_some())
            {
                return AuthProbe::Ready;
            }
        }
        return AuthProbe::ProviderMissing;
    };
    if state_requires_relogin(&state) {
        return AuthProbe::ReloginRequired;
    }
    if bearer_and_base_from_entry(&state, recipe.base_url).is_some() {
        return AuthProbe::Ready;
    }
    if state
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.is_empty())
        || state
            .get("tokens")
            .and_then(|t| t.get("refresh_token"))
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty())
    {
        return AuthProbe::Ready;
    }
    AuthProbe::NoBearer
}

pub fn probe_oauth_auth(recipe: &BuiltinRecipe) -> AuthProbe {
    probe_oauth_auth_with_path(&default_auth_path(), recipe)
}

pub fn auth_probe_message(recipe: &BuiltinRecipe, probe: AuthProbe) -> String {
    match probe {
        AuthProbe::Ready => format!("{} OAuth/auth file looks usable", recipe.display_name),
        AuthProbe::MissingFile => format!(
            "{}: no auth.json — run `{}`",
            recipe.display_name, recipe.hermes_auth_cmd
        ),
        AuthProbe::ProviderMissing => format!(
            "{}: provider '{}' missing in auth.json — run `{}`",
            recipe.display_name, recipe.auth_provider, recipe.hermes_auth_cmd
        ),
        AuthProbe::NoBearer => format!(
            "{}: logged in but no bearer yet — run `{}` or start proxy to refresh",
            recipe.display_name, recipe.hermes_auth_cmd
        ),
        AuthProbe::ReloginRequired => format!(
            "{}: credentials quarantined — run `{}` to re-authenticate",
            recipe.display_name, recipe.hermes_auth_cmd
        ),
    }
}

pub struct ClientSnippet {
    pub base_url: String,
    pub model_alias: String,
    pub forward_only_cmd: String,
    pub token: String,
}

pub fn client_snippet(
    cfg: &ProxyConfig,
    recipe: Option<&BuiltinRecipe>,
    token: &str,
) -> ClientSnippet {
    let host = &cfg.bind;
    let port = cfg.port;
    let base_url = format!("http://{host}:{port}/v1");
    let model_alias = recipe
        .map(|r| r.default_alias.to_string())
        .unwrap_or_else(|| {
            cfg.model_aliases
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| "your-alias".into())
        });
    let forward_only_cmd = recipe
        .map(|r| format!("edgecrab proxy start --provider {}", r.key))
        .unwrap_or_else(|| "edgecrab proxy start".into());
    ClientSnippet {
        base_url,
        model_alias,
        forward_only_cmd,
        token: token.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_grok_alias_to_xai() {
        assert_eq!(resolve_recipe("grok").map(|r| r.key), Some("xai"));
    }

    #[test]
    fn apply_recipe_is_idempotent() {
        let mut cfg = ProxyConfig::default();
        apply_recipe(&mut cfg, &RECIPE_XAI);
        assert!(cfg.forward_upstreams.contains_key("xai"));
        assert_eq!(
            cfg.model_aliases.get("grok"),
            Some(&"forward:xai".to_string())
        );
        assert_eq!(cfg.default_forward_upstream.as_deref(), Some("xai"));
    }

    #[test]
    fn probe_oauth_ready_when_refresh_token_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        let auth_path = dir.path().join("auth.json");
        std::fs::write(
            &auth_path,
            serde_json::json!({
                "providers": {
                    "xai-oauth": {
                        "tokens": { "refresh_token": "rt-only" }
                    }
                }
            })
            .to_string(),
        )
        .expect("write");
        assert_eq!(
            probe_oauth_auth_with_path(&auth_path, &RECIPE_XAI),
            AuthProbe::Ready
        );
    }
}
