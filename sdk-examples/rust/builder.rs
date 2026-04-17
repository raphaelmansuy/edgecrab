//! # Builder Example — Configure the agent with the builder API
//!
//! ```bash
//! cargo run --example builder
//! ```

use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    // Full builder API demonstration
    let agent = SdkAgent::builder("anthropic/claude-sonnet-4")?
        .max_iterations(10)
        .temperature(0.5)
        .streaming(false)
        .quiet_mode(true)
        .build()?;

    // Simple chat
    let reply = agent.chat("What is 2 + 2?").await?;
    println!("Reply: {reply}");

    // Full conversation result
    let result = agent.run("Explain monads in one sentence.").await?;
    println!("Model: {}", result.model);
    println!("API calls: {}", result.api_calls);
    println!("Interrupted: {}", result.interrupted);
    println!("Response: {}", result.final_response);

    Ok(())
}
