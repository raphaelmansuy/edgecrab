//! # Batch + Set Model — Parallel prompts and hot-swapping models

use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    let agent = SdkAgent::builder("anthropic/claude-sonnet-4")?
        .max_iterations(5)
        .quiet_mode(true)
        .build()?;

    // Parallel batch
    let prompts = [
        "Summarize the Rust ownership model in one sentence.",
        "Summarize Python's GIL in one sentence.",
        "Summarize Go's goroutines in one sentence.",
    ];
    let refs: Vec<&str> = prompts.iter().map(|s| s.as_ref()).collect();
    let results = agent.batch(&refs).await;

    for (p, r) in prompts.iter().zip(results.iter()) {
        match r {
            Ok(reply) => println!("{p}\n  → {reply}\n"),
            Err(e) => println!("{p}\n  → ERROR: {e}\n"),
        }
    }

    // Hot-swap to cheaper model
    agent.set_model("anthropic/claude-haiku-3").await?;
    println!("Switched to: {}", agent.model().await);
    let reply = agent.chat("Say hi.").await?;
    println!("Haiku says: {reply}");

    Ok(())
}
