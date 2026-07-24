//! Model selector catalog helpers — Hermes `modelPicker.tsx` data layer (DRY).
//!
//! Auth badges for SuperGrok / xAI are pure presentation (SOLID S) — credential
//! readiness is injected by the App; this module never runs OAuth.

use std::collections::{BTreeMap, BTreeSet};

use edgecrab_core::{DiscoveryAvailability, DiscoverySource, ModelCatalog};

use crate::fuzzy_selector::{FuzzyItem, FuzzySelector};

/// A single model entry for fuzzy selector overlays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelEntry {
    pub display: String,
    pub provider: String,
    pub model_name: String,
    pub detail: String,
}

impl FuzzyItem for ModelEntry {
    fn primary(&self) -> &str {
        &self.display
    }

    fn secondary(&self) -> &str {
        &self.detail
    }

    fn tag(&self) -> &str {
        &self.provider
    }
}

/// How the operator authenticates this provider (picker badge).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAuthSurface {
    /// Console / env API key only.
    ApiKey,
    /// Subscription OAuth (SuperGrok, Claude Pro, …).
    Oauth,
    /// Either key or OAuth accepted at runtime.
    KeyOrOauth,
    Unknown,
}

/// Deterministic auth surface from provider id (no I/O).
pub fn provider_auth_surface(provider: &str) -> ProviderAuthSurface {
    let p = ModelCatalog::catalog_provider_id(provider);
    match p.as_str() {
        "super-grok" => ProviderAuthSurface::Oauth,
        "xai" => ProviderAuthSurface::KeyOrOauth,
        "claude-pro" | "anthropic" => ProviderAuthSurface::KeyOrOauth,
        "openai-codex" | "chatgpt-pro" => ProviderAuthSurface::Oauth,
        "openai" | "openrouter" | "mistral" | "groq" | "deepseek" | "nvidia" => {
            ProviderAuthSurface::ApiKey
        }
        "copilot" | "vscode-copilot" => ProviderAuthSurface::Oauth,
        _ => ProviderAuthSurface::Unknown,
    }
}

/// Short badge for detail column / inventory.
pub fn provider_auth_badge(provider: &str) -> Option<&'static str> {
    match provider_auth_surface(provider) {
        ProviderAuthSurface::ApiKey => Some("🔑 key"),
        ProviderAuthSurface::Oauth => Some("🪪 OAuth"),
        ProviderAuthSurface::KeyOrOauth => Some("🔑/🪪"),
        ProviderAuthSurface::Unknown => None,
    }
}

/// Optional readiness: `Some(true)` ready, `Some(false)` missing, `None` unknown.
pub type ProviderAuthReadyFn<'a> = dyn Fn(&str) -> Option<bool> + 'a;

pub fn discovery_source_label(source: DiscoverySource) -> &'static str {
    match source {
        DiscoverySource::Live => "live discovery",
        DiscoverySource::Cache => "cached discovery",
        DiscoverySource::Static => "static catalog",
    }
}

pub fn discovery_availability_short(availability: DiscoveryAvailability) -> String {
    match availability {
        DiscoveryAvailability::Supported => "live discovery".to_string(),
        DiscoveryAvailability::FeatureGated(feature) => {
            format!("live discovery gated by `{feature}`")
        }
        DiscoveryAvailability::Unsupported => "static catalog".to_string(),
    }
}

pub fn discovery_availability_detail(
    provider: &str,
    availability: DiscoveryAvailability,
) -> String {
    match availability {
        DiscoveryAvailability::Supported => {
            format!("{provider} supports live discovery in this build.")
        }
        DiscoveryAvailability::FeatureGated(feature) => format!(
            "{provider} supports live discovery, but this build falls back to the embedded catalog because `{feature}` is disabled."
        ),
        DiscoveryAvailability::Unsupported => {
            format!("{provider} is served from the embedded catalog.")
        }
    }
}

#[allow(dead_code)] // Thin wrapper retained for call sites that omit auth readiness.
pub fn build_model_selector_entries(
    grouped: &[(String, Vec<String>)],
    dynamic_lookup: Option<&BTreeMap<String, (DiscoverySource, BTreeSet<String>)>>,
) -> Vec<ModelEntry> {
    build_model_selector_entries_with_auth(grouped, dynamic_lookup, None)
}

/// Build picker entries with optional auth readiness (SuperGrok OAuth / API key).
pub fn build_model_selector_entries_with_auth(
    grouped: &[(String, Vec<String>)],
    dynamic_lookup: Option<&BTreeMap<String, (DiscoverySource, BTreeSet<String>)>>,
    auth_ready: Option<&ProviderAuthReadyFn<'_>>,
) -> Vec<ModelEntry> {
    let mut all_models = Vec::new();
    for (provider, models) in grouped {
        let auth_part = {
            let mut parts = Vec::new();
            if let Some(badge) = provider_auth_badge(provider) {
                parts.push(badge.to_string());
            }
            if let Some(ready_fn) = auth_ready {
                match ready_fn(provider) {
                    Some(true) => parts.push("ready".into()),
                    Some(false) => parts.push("sign-in".into()),
                    None => {}
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" · "))
            }
        };
        for model in models {
            let inv = match dynamic_lookup.and_then(|lookup| lookup.get(provider)) {
                Some((source, discovered_models)) if discovered_models.contains(model) => {
                    discovery_source_label(*source).to_string()
                }
                Some((DiscoverySource::Static, _)) => {
                    discovery_source_label(DiscoverySource::Static).to_string()
                }
                Some(_) => "catalog fallback".into(),
                None => discovery_source_label(DiscoverySource::Static).to_string(),
            };
            // Local free providers: surface a clear badge so they are not confused
            // with cloud OpenRouter substring hits (e.g. "olmo" vs "omlx").
            let local_badge = match provider.as_str() {
                "omlx" => Some("local MLX"),
                "mtplx" => Some("local MTP"),
                "llamacpp" => Some("local GGUF"),
                "vllm-mlx" => Some("local vLLM-MLX"),
                "mlx-lm" => Some("local mlx-lm"),
                "ollama" | "lmstudio" | "vllm" => Some("local"),
                _ => None,
            };
            let detail = match (&auth_part, local_badge) {
                (Some(a), Some(local)) => format!("{model} · {local} · {a} · {inv}"),
                (Some(a), None) => format!("{model} · {a} · {inv}"),
                (None, Some(local)) => format!("{model} · {local} · {inv}"),
                (None, None) => format!("{model} · {inv}"),
            };
            all_models.push(ModelEntry {
                display: format!("{provider}/{model}"),
                provider: provider.clone(),
                detail,
                model_name: model.clone(),
            });
        }
    }
    // Prefer SuperGrok / xAI flagships near top when sorting would bury them:
    // stable sort by display still works; filter "grok" matches provider tag.
    all_models.sort_by(|left, right| left.display.cmp(&right.display));
    all_models
}

/// Footer hint for model selector chrome (Hermes type-to-filter discoverability).
pub fn model_selector_status_hint(
    selector: &FuzzySelector<ModelEntry>,
    refresh_in_flight: bool,
    current_model: &str,
) -> Option<String> {
    if refresh_in_flight {
        return Some("live discovery running".into());
    }
    let matched = selector.filtered.len();
    let total = selector.items.len();
    if matched == 0 && !selector.query.is_empty() {
        return Some("no matches — try provider or model fragment".into());
    }
    if matched < total && !selector.query.is_empty() {
        return Some(format!("{matched}/{total} matched"));
    }
    if !current_model.is_empty() {
        return Some(format!("current: {current_model}"));
    }
    None
}

pub fn build_models_inventory_report(
    providers: &[(String, Vec<String>)],
    current_model: &str,
    filter: &str,
) -> String {
    let current_provider = current_model
        .split_once('/')
        .map(|(provider, _)| edgecrab_core::normalize_discovery_provider(provider));
    let discovery_statuses: BTreeMap<String, DiscoveryAvailability> =
        edgecrab_core::discovery_provider_statuses()
            .into_iter()
            .collect();
    let mut text = if filter.is_empty() {
        "Model inventory (* = current provider):\n\n".to_string()
    } else {
        format!("Providers matching '{filter}' (* = current provider):\n\n")
    };

    for (provider, models) in providers {
        let label = ModelCatalog::provider_label(provider);
        let marker = if current_provider.as_deref() == Some(provider.as_str()) {
            " *"
        } else {
            ""
        };
        let availability = discovery_statuses
            .get(provider)
            .copied()
            .unwrap_or(DiscoveryAvailability::Unsupported);
        let auth = provider_auth_badge(provider).unwrap_or("—");
        text.push_str(&format!(
            "  {provider:<12} {label:<22} {:>3} models  {}  {}{marker}\n",
            models.len(),
            discovery_availability_short(availability),
            auth,
        ));
    }

    text.push_str(
        "\nSuperGrok (OAuth): super-grok/grok-4.5  ·  xAI API key: xai/grok-4.5\n\
         Pick SuperGrok unsigned-in → EdgeCrab opens /login grok, then switches for you.\n\
         /login grok anytime. Use /models <provider> for full lists, \
         /models refresh, or /model for the selector.",
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_sorted_entries() {
        let grouped = vec![
            ("openai".into(), vec!["gpt-4o".into()]),
            ("anthropic".into(), vec!["claude-opus-4.6".into()]),
        ];
        let entries = build_model_selector_entries(&grouped, None);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].display < entries[1].display);
    }

    #[test]
    fn fuzzy_tag_matches_provider() {
        let mut selector = FuzzySelector::new();
        selector.set_items(vec![ModelEntry {
            display: "anthropic/claude-opus-4.6".into(),
            provider: "anthropic".into(),
            model_name: "claude-opus-4.6".into(),
            detail: "static catalog".into(),
        }]);
        selector.query = "anthropic".into();
        selector.update_filter();
        assert_eq!(selector.filtered.len(), 1);
    }

    #[test]
    fn fuzzy_ranks_omlx_provider_above_substring_model_id() {
        // Both rows match "omlx"; provider-tag / primary-prefix should win.
        let mut selector = FuzzySelector::new();
        selector.set_items(vec![
            ModelEntry {
                display: "openrouter/vendor/omlx-port-mirror".into(),
                provider: "openrouter".into(),
                model_name: "vendor/omlx-port-mirror".into(),
                detail: "key · live discovery".into(),
            },
            ModelEntry {
                display: "omlx/Qwen3.6-35B-A3B".into(),
                provider: "omlx".into(),
                model_name: "Qwen3.6-35B-A3B".into(),
                detail: "local MLX · live discovery".into(),
            },
        ]);
        selector.query = "omlx".into();
        selector.update_filter();
        assert_eq!(selector.filtered.len(), 2);
        let first = &selector.items[selector.filtered[0]];
        assert_eq!(
            first.provider, "omlx",
            "provider-prefix match should rank omlx above openrouter model-id substring"
        );
    }

    #[test]
    fn omlx_entries_show_local_mlx_badge() {
        let grouped = vec![("omlx".into(), vec!["Qwen3".into()])];
        let entries = build_model_selector_entries(&grouped, None);
        assert!(entries[0].detail.contains("local MLX"));
    }

    #[test]
    fn status_hint_on_filter_miss() {
        let mut selector = FuzzySelector::new();
        selector.set_items(vec![ModelEntry {
            display: "openai/gpt-4o".into(),
            provider: "openai".into(),
            model_name: "gpt-4o".into(),
            detail: "live".into(),
        }]);
        selector.query = "zzzzz".into();
        selector.update_filter();
        let hint = model_selector_status_hint(&selector, false, "openai/gpt-4o");
        assert!(hint.unwrap().contains("no matches"));
    }

    #[test]
    fn super_grok_auth_badge_is_oauth() {
        assert_eq!(
            provider_auth_surface("super-grok"),
            ProviderAuthSurface::Oauth
        );
        assert_eq!(provider_auth_badge("super-grok"), Some("🪪 OAuth"));
        assert_eq!(
            provider_auth_surface("xai"),
            ProviderAuthSurface::KeyOrOauth
        );
    }

    #[test]
    fn super_grok_entries_include_oauth_badge_and_grok_45() {
        let grouped = vec![(
            "super-grok".into(),
            vec!["grok-4.5".into(), "grok-4.3".into()],
        )];
        let entries = build_model_selector_entries(&grouped, None);
        assert!(
            entries.iter().any(|e| e.display == "super-grok/grok-4.5"),
            "missing super-grok/grok-4.5: {entries:?}"
        );
        let sg = entries
            .iter()
            .find(|e| e.display == "super-grok/grok-4.5")
            .expect("entry");
        assert!(
            sg.detail.contains("OAuth") || sg.detail.contains("🪪"),
            "detail should badge OAuth: {}",
            sg.detail
        );
    }

    #[test]
    fn auth_ready_fn_marks_sign_in() {
        let grouped = vec![("super-grok".into(), vec!["grok-4.5".into()])];
        let ready = |p: &str| {
            if p == "super-grok" { Some(false) } else { None }
        };
        let entries = build_model_selector_entries_with_auth(&grouped, None, Some(&ready));
        assert!(
            entries[0].detail.contains("sign-in"),
            "{}",
            entries[0].detail
        );
    }
}
