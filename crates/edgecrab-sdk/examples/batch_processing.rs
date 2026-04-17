//! # Batch Processing — Run multiple prompts in parallel
//!
//! Demonstrates the batch API: send N prompts simultaneously, each
//! running in an isolated fork. Perfect for data extraction, grading,
//! translation, or any map-style workload.
//!
//! ```bash
//! cargo run --example batch_processing
//! ```

use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agent = SdkAgent::builder("copilot/claude-sonnet-4.6")?
        .max_iterations(5)
        .quiet_mode(true)
        .build()?;

    // ── 1. Parallel translation ─────────────────────────────────────
    let prompts = [
        "Translate to French: 'The quick brown fox jumps over the lazy dog.'",
        "Translate to Spanish: 'The quick brown fox jumps over the lazy dog.'",
        "Translate to Japanese: 'The quick brown fox jumps over the lazy dog.'",
        "Translate to German: 'The quick brown fox jumps over the lazy dog.'",
    ];

    println!("Sending {} prompts in parallel...\n", prompts.len());
    let refs: Vec<&str> = prompts.iter().map(|s| s.as_ref()).collect();
    let results = agent.batch(&refs).await;

    for (prompt, result) in prompts.iter().zip(results.iter()) {
        let lang = prompt.split(':').next().unwrap_or("?");
        match result {
            Ok(reply) => println!("{lang}\n  {reply}\n"),
            Err(e) => println!("{lang}\n  ERROR: {e}\n"),
        }
    }

    // ── 2. Hot-swap model mid-session ───────────────────────────────
    println!("Current model: {}", agent.model().await);

    agent.set_model("copilot/gpt-5-mini").await?;
    println!("Switched to: {}", agent.model().await);

    let cheap_reply = agent.chat("Say hello in one word.").await?;
    println!("Haiku reply: {cheap_reply}");

    Ok(())
}
