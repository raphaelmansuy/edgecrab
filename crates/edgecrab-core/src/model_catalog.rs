//! # Model Catalog — single source of truth for available models
//!
//! SOLID/DRY: every component (TUI model selector, `/models` command,
//! `setup` wizard, pricing engine) reads from this one module instead
//! of maintaining its own hardcoded list.
//!
//! ```text
//!   ModelCatalog::get()
//!     ├── 1. Embedded default (compiled-in YAML)
//!     ├── 2. User overrides (~/.edgecrab/models.yaml)
//!     └── 3. Cached live discovery (future: OpenRouter /models)
//! ```
//!
//! The catalog is loaded once (lazy, thread-safe) and shared read-only.
//! To refresh, call `ModelCatalog::reload()`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

// ─── Types ───────────────────────────────────────────────────────────

/// Performance tier for UI grouping and smart routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelTier {
    Fast,
    #[default]
    Standard,
    Reasoning,
}

impl std::fmt::Display for ModelTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelTier::Fast => write!(f, "Fast"),
            ModelTier::Standard => write!(f, "Standard"),
            ModelTier::Reasoning => write!(f, "Reasoning"),
        }
    }
}

/// A single model entry in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    /// Short model name (e.g. "gpt-5-mini", "claude-sonnet-4.5").
    pub model: String,
    /// Context window in tokens.
    #[serde(default = "default_context")]
    pub context: u64,
    /// Performance tier.
    #[serde(default)]
    pub tier: ModelTier,
    /// Per-million-token pricing (input, output). None = unknown/free.
    #[serde(default)]
    pub pricing: Option<PricingPair>,
}

/// Input/output pricing per million tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingPair {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write: f64,
}

fn default_context() -> u64 {
    128_000
}

/// Provider entry: a named provider with its list of models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    /// Display label (e.g. "GitHub Copilot", "Anthropic").
    #[serde(default)]
    pub label: String,
    /// Default model for this provider (index into models or model name).
    #[serde(default)]
    pub default_model: Option<String>,
    /// Available models.
    pub models: Vec<ModelEntry>,
}

/// The full deserialized catalog.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CatalogData {
    /// provider_id → ProviderEntry
    pub providers: BTreeMap<String, ProviderEntry>,
}

// ─── Embedded default catalog ────────────────────────────────────────

/// The default catalog, compiled into the binary.
const EMBEDDED_CATALOG_YAML: &str = include_str!("model_catalog_default.yaml");

// ─── Global singleton ────────────────────────────────────────────────

static CATALOG: OnceLock<RwLock<CatalogData>> = OnceLock::new();

/// The model catalog singleton.
pub struct ModelCatalog;

impl ModelCatalog {
    /// Get the global catalog (loads on first call).
    pub fn get() -> std::sync::RwLockReadGuard<'static, CatalogData> {
        let lock = CATALOG.get_or_init(|| {
            let data = Self::load_merged();
            RwLock::new(data)
        });
        lock.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Reload the catalog from disk + embedded defaults.
    pub fn reload() {
        if let Some(lock) = CATALOG.get() {
            let data = Self::load_merged();
            if let Ok(mut guard) = lock.write() {
                *guard = data;
            }
        }
    }

    /// Load and merge: embedded defaults ← user overrides.
    fn load_merged() -> CatalogData {
        // 1. Parse embedded default
        let mut catalog: CatalogData =
            serde_yml::from_str(EMBEDDED_CATALOG_YAML).unwrap_or_else(|e| {
                tracing::error!(error = %e, "failed to parse embedded model catalog");
                CatalogData::default()
            });

        // 2. Merge user overrides from ~/.edgecrab/models.yaml
        let user_path = user_catalog_path();
        if user_path.exists() {
            match std::fs::read_to_string(&user_path) {
                Ok(content) => match serde_yml::from_str::<CatalogData>(&content) {
                    Ok(user_data) => {
                        merge_catalogs(&mut catalog, &user_data);
                        tracing::debug!(path = %user_path.display(), "merged user model catalog");
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %user_path.display(),
                            error = %e,
                            "failed to parse user model catalog, using defaults"
                        );
                    }
                },
                Err(e) => {
                    tracing::debug!(path = %user_path.display(), error = %e, "no user model catalog");
                }
            }
        }

        catalog
    }

    // ─── Query helpers ───────────────────────────────────────────────

    /// List all provider IDs in alphabetical order.
    pub fn provider_ids() -> Vec<String> {
        let cat = Self::get();
        cat.providers.keys().cloned().collect()
    }

    /// Get models for a provider as (display_name, model_id) tuples.
    pub fn models_for_provider(provider: &str) -> Vec<(String, String)> {
        let cat = Self::get();
        cat.providers
            .get(provider)
            .map(|p| {
                p.models
                    .iter()
                    .map(|m| {
                        let display = format!("{provider}/{}", m.model);
                        (display, m.model.clone())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Build a flat list of (provider, model_name) for the TUI model selector.
    pub fn flat_catalog() -> Vec<(String, String, String)> {
        let cat = Self::get();
        let mut result = Vec::new();
        for (pid, entry) in &cat.providers {
            for m in &entry.models {
                let display = format!("{pid}/{}", m.model);
                result.push((display, pid.clone(), m.model.clone()));
            }
        }
        result
    }

    /// Build provider → models list for `/models` display.
    pub fn grouped_catalog() -> Vec<(String, Vec<String>)> {
        let cat = Self::get();
        cat.providers
            .iter()
            .map(|(pid, entry)| {
                let models: Vec<String> = entry.models.iter().map(|m| m.model.clone()).collect();
                (pid.clone(), models)
            })
            .collect()
    }

    /// Get the default model for a provider.
    pub fn default_model_for(provider: &str) -> Option<String> {
        let cat = Self::get();
        let entry = cat.providers.get(provider)?;
        if let Some(ref dm) = entry.default_model {
            Some(format!("{provider}/{dm}"))
        } else {
            entry
                .models
                .first()
                .map(|m| format!("{provider}/{}", m.model))
        }
    }

    /// Get context window for a specific model.
    pub fn context_window(provider: &str, model: &str) -> Option<u64> {
        let cat = Self::get();
        cat.providers.get(provider).and_then(|p| {
            p.models
                .iter()
                .find(|m| m.model == model)
                .map(|m| m.context)
        })
    }

    /// Get pricing for a model. Returns (input_per_million, output_per_million).
    pub fn pricing_for(provider: &str, model: &str) -> Option<PricingPair> {
        let cat = Self::get();
        cat.providers.get(provider).and_then(|p| {
            p.models
                .iter()
                .find(|m| m.model == model)
                .and_then(|m| m.pricing.clone())
        })
    }

    /// Get the provider label (display name).
    pub fn provider_label(provider: &str) -> String {
        let cat = Self::get();
        cat.providers
            .get(provider)
            .map(|p| {
                if p.label.is_empty() {
                    provider.to_string()
                } else {
                    p.label.clone()
                }
            })
            .unwrap_or_else(|| provider.to_string())
    }
}

// ─── Merge logic ─────────────────────────────────────────────────────

/// Merge user catalog into base. User entries override/extend base.
fn merge_catalogs(base: &mut CatalogData, user: &CatalogData) {
    for (pid, user_entry) in &user.providers {
        let base_entry = base
            .providers
            .entry(pid.clone())
            .or_insert_with(|| ProviderEntry {
                label: String::new(),
                default_model: None,
                models: Vec::new(),
            });

        // Override label if user provides one
        if !user_entry.label.is_empty() {
            base_entry.label = user_entry.label.clone();
        }

        // Override default_model if user provides one
        if user_entry.default_model.is_some() {
            base_entry.default_model = user_entry.default_model.clone();
        }

        // Merge models: user models override same-name base models, new ones are appended
        for um in &user_entry.models {
            if let Some(bm) = base_entry.models.iter_mut().find(|m| m.model == um.model) {
                *bm = um.clone();
            } else {
                base_entry.models.push(um.clone());
            }
        }
    }
}

/// Path to the user's custom model catalog.
fn user_catalog_path() -> PathBuf {
    crate::config::edgecrab_home().join("models.yaml")
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_parses() {
        let cat: CatalogData =
            serde_yml::from_str(EMBEDDED_CATALOG_YAML).expect("embedded catalog YAML must parse");
        assert!(!cat.providers.is_empty(), "catalog must have providers");
        // Check a few expected providers
        assert!(cat.providers.contains_key("anthropic"), "missing anthropic");
        assert!(cat.providers.contains_key("bedrock"), "missing bedrock");
        assert!(cat.providers.contains_key("copilot"), "missing copilot");
        assert!(cat.providers.contains_key("openai"), "missing openai");
    }

    #[test]
    fn singleton_loads() {
        let cat = ModelCatalog::get();
        assert!(!cat.providers.is_empty());
    }

    #[test]
    fn flat_catalog_not_empty() {
        let flat = ModelCatalog::flat_catalog();
        assert!(!flat.is_empty());
    }

    #[test]
    fn default_model_resolution() {
        let dm = ModelCatalog::default_model_for("copilot");
        assert!(dm.is_some(), "copilot should have a default model");
        let dm = dm.expect("checked");
        assert!(dm.starts_with("copilot/"));
    }

    #[test]
    fn merge_adds_new_provider() {
        let mut base = CatalogData::default();
        let mut user = CatalogData::default();
        user.providers.insert(
            "custom-test".into(),
            ProviderEntry {
                label: "Test".into(),
                default_model: Some("test-model".into()),
                models: vec![ModelEntry {
                    model: "test-model".into(),
                    context: 8000,
                    tier: ModelTier::Fast,
                    pricing: None,
                }],
            },
        );
        merge_catalogs(&mut base, &user);
        assert!(base.providers.contains_key("custom-test"));
    }

    #[test]
    fn merge_overrides_existing_model() {
        let mut base = CatalogData::default();
        base.providers.insert(
            "test".into(),
            ProviderEntry {
                label: "".into(),
                default_model: None,
                models: vec![ModelEntry {
                    model: "m1".into(),
                    context: 1000,
                    tier: ModelTier::Standard,
                    pricing: None,
                }],
            },
        );
        let mut user = CatalogData::default();
        user.providers.insert(
            "test".into(),
            ProviderEntry {
                label: "Override".into(),
                default_model: None,
                models: vec![ModelEntry {
                    model: "m1".into(),
                    context: 99999,
                    tier: ModelTier::Reasoning,
                    pricing: None,
                }],
            },
        );
        merge_catalogs(&mut base, &user);
        let entry = &base.providers["test"];
        assert_eq!(entry.label, "Override");
        assert_eq!(entry.models[0].context, 99999);
    }
}
