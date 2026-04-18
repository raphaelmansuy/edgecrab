//! # Streaming Example — Watch tokens arrive in real-time
//!
//! ```bash
//! cargo run --example streaming
//! ```

use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    let agent = SdkAgent::builder("anthropic/claude-sonnet-4")?
        .streaming(true)
        .build()?;

    let mut rx = agent.stream("Write a haiku about Rust programming.").await?;

    while let Some(event) = rx.recv().await {
        match &event {
            StreamEvent::Token(token) => print!("{token}"),
            StreamEvent::Done => println!("\n--- Done ---"),
            other => println!("[event: {other:?}]"),
        }
    }

    Ok(())
}
