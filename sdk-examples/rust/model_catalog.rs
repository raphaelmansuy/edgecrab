//! # Model Catalog — Offline exploration of providers and pricing
//!
//! No API key needed — runs entirely against the built-in catalog.

use edgecrab_sdk::types::SdkModelCatalog;

fn main() {
    println!("=== EdgeCrab Model Catalog ===\n");

    // List providers
    let providers = SdkModelCatalog::provider_ids();
    println!("{} providers:", providers.len());
    for p in &providers {
        let count = SdkModelCatalog::models_for_provider(p).len();
        let default = SdkModelCatalog::default_model_for(p)
            .unwrap_or_else(|| "—".into());
        println!("  {p}: {count} models (default: {default})");
    }

    // Cost estimation
    println!("\nCost estimate (anthropic/claude-sonnet-4, 10K in / 2K out):");
    if let Some((ic, oc, total)) =
        SdkModelCatalog::estimate_cost("anthropic", "claude-sonnet-4-20250514", 10_000, 2_000)
    {
        println!("  ${total:.6} (input: ${ic:.6}, output: ${oc:.6})");
    }
}
