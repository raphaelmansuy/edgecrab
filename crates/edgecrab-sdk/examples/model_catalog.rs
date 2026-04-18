//! # Model Catalog — Explore providers, pricing, and cost estimation
//!
//! Demonstrates the ModelCatalog for querying available models,
//! checking context windows, pricing, and pre-flight cost estimation.
//!
//! This example runs entirely offline — no API key needed.
//!
//! ```bash
//! cargo run --example model_catalog
//! ```

use edgecrab_sdk::types::SdkModelCatalog;

fn main() {
    // ── 1. List all providers ───────────────────────────────────────
    let providers = SdkModelCatalog::provider_ids();
    println!("Supported providers ({}):", providers.len());
    for p in &providers {
        let models = SdkModelCatalog::models_for_provider(p);
        println!("  {p}: {} models", models.len());
    }

    // ── 2. Show models for a specific provider ──────────────────────
    println!("\nOpenAI models:");
    for (id, display_name) in SdkModelCatalog::models_for_provider("openai") {
        let ctx = SdkModelCatalog::context_window("openai", &id);
        println!(
            "  {id} ({display_name}) — context: {}",
            ctx.map_or("unknown".into(), |w| format!("{w} tokens")),
        );
    }

    // ── 3. Check pricing ────────────────────────────────────────────
    println!("\nPricing comparison (per 1M tokens):");
    let models = [
        ("openai", "gpt-4o"),
        ("openai", "gpt-5-mini"),
        ("copilot", "gpt-5-mini"),
        ("ollama", "gemma4:latest"),
    ];
    for (provider, model) in &models {
        if let Some(pricing) = SdkModelCatalog::pricing(provider, model) {
            println!(
                "  {provider}/{model}: ${:.2} input / ${:.2} output",
                pricing.input, pricing.output,
            );
        }
    }

    // ── 4. Pre-flight cost estimation ───────────────────────────────
    println!("\nCost estimation for a 10K-input / 2K-output request:");
    if let Some((input_cost, output_cost, total)) =
        SdkModelCatalog::estimate_cost("openai", "gpt-5-mini", 10_000, 2_000)
    {
        println!("  Input cost:  ${input_cost:.6}");
        println!("  Output cost: ${output_cost:.6}");
        println!("  Total:       ${total:.6}");
    }

    // ── 5. Default models per provider ──────────────────────────────
    println!("\nDefault models:");
    for p in &providers {
        if let Some(default) = SdkModelCatalog::default_model_for(p) {
            println!("  {p}: {default}");
        }
    }
}
