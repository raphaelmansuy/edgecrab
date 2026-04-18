//! # Configuration & Profiles — Load config, switch profiles, save settings
//!
//! Demonstrates the SdkConfig API: loading from the default path, from
//! a named profile, and modifying settings at runtime.
//!
//! ```bash
//! cargo run --example config_profiles
//! ```

use edgecrab_sdk::prelude::*;
use edgecrab_sdk_core::SdkConfig;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── 1. Load default configuration ───────────────────────────────
    match SdkConfig::load() {
        Ok(config) => {
            println!("Default config loaded:");
            println!("  Model:          {}", config.default_model());
            println!("  Max iterations: {}", config.max_iterations());
            println!("  Temperature:    {:?}", config.temperature());
        }
        Err(e) => {
            println!("No config found ({}), using defaults", e);
            let config = SdkConfig::default_config();
            println!("  Default model: {}", config.default_model());
        }
    }

    // ── 2. Modify configuration at runtime ──────────────────────────
    let mut config = SdkConfig::default_config();
    config.set_default_model("deepseek/deepseek-chat");
    config.set_max_iterations(20);
    config.set_temperature(Some(0.7));

    println!("\nModified config:");
    println!("  Model:       {}", config.default_model());
    println!("  Iterations:  {}", config.max_iterations());
    println!("  Temperature: {:?}", config.temperature());

    // ── 3. Show profile config paths ────────────────────────────────
    let profiles = ["default", "work", "creative", "coding"];
    println!("\nProfile config paths:");
    for name in &profiles {
        let path = SdkConfig::profile_config_path(name);
        let exists = path.exists();
        println!(
            "  {name}: {} {}",
            path.display(),
            if exists { "✓" } else { "" }
        );
    }

    // ── 4. Show EdgeCrab home directory ─────────────────────────────
    println!("\nEdgeCrab home: {}", edgecrab_home().display());

    Ok(())
}
