//! # Hello World — Minimal EdgeCrab SDK Example
//!
//! Send a single message and print the reply.
//!
//! ```bash
//! cargo run --example hello
//! ```

use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    // Create an agent with the default model
    let agent = SdkAgent::new("anthropic/claude-sonnet-4")?;

    // Send a message
    let reply = agent.chat("What is EdgeCrab? Answer in one sentence.").await?;
    println!("Agent: {reply}");

    Ok(())
}
