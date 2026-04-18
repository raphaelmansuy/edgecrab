//! SDK type re-exports and model catalog access.
//!
//! All core message, tool, and usage types are re-exported from the crate root.
//! This module provides additional SDK-specific type helpers and the model catalog.

use edgecrab_core::model_catalog::{ModelCatalog, PricingPair};

/// Query the model catalog for available providers and models.
///
/// The catalog is a compiled-in + user-overridable registry of all supported
/// LLM providers and their models, including pricing and context window data.
///
/// # Example
///
/// ```rust,no_run
/// use edgecrab_sdk_core::types::SdkModelCatalog;
///
/// let providers = SdkModelCatalog::provider_ids();
/// for provider in &providers {
///     println!("Provider: {provider}");
/// }
/// ```
pub struct SdkModelCatalog;

impl SdkModelCatalog {
    /// List all known provider IDs (e.g. `["anthropic", "openai", "google", ...]`).
    pub fn provider_ids() -> Vec<String> {
        ModelCatalog::provider_ids()
    }

    /// List models for a specific provider. Returns `(model_id, display_name)` pairs.
    pub fn models_for_provider(provider: &str) -> Vec<(String, String)> {
        ModelCatalog::models_for_provider(provider)
    }

    /// Flat list of all models: `(provider, model_id, display_name)`.
    pub fn flat_catalog() -> Vec<(String, String, String)> {
        ModelCatalog::flat_catalog()
    }

    /// Get the context window size for a provider/model pair.
    pub fn context_window(provider: &str, model: &str) -> Option<u64> {
        ModelCatalog::context_window(provider, model)
    }

    /// Get pricing for a provider/model pair (input/output cost per token).
    pub fn pricing(provider: &str, model: &str) -> Option<PricingPair> {
        ModelCatalog::pricing_for(provider, model)
    }

    /// Get the default model for a provider.
    pub fn default_model_for(provider: &str) -> Option<String> {
        ModelCatalog::default_model_for(provider)
    }

    /// Estimate the cost (USD) of a hypothetical request.
    ///
    /// Returns `(input_cost, output_cost, total_cost)` or `None` if
    /// pricing data is unavailable for the provider/model pair.
    pub fn estimate_cost(
        provider: &str,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Option<(f64, f64, f64)> {
        let p = ModelCatalog::pricing_for(provider, model)?;
        let ic = p.input * input_tokens as f64 / 1_000_000.0;
        let oc = p.output * output_tokens as f64 / 1_000_000.0;
        Some((ic, oc, ic + oc))
    }
}

// Re-export the pricing type for convenience
pub use edgecrab_core::model_catalog::PricingPair as ModelPricing;
