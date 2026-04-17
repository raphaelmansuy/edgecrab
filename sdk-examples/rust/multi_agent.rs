//! # Multi-Agent Pipeline — Fork specialists and merge results

use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    let coordinator = SdkAgent::builder("anthropic/claude-sonnet-4")?
        .max_iterations(5)
        .quiet_mode(true)
        .instructions("You are a technical writing coordinator.")
        .build()?;

    coordinator.chat("We are documenting a REST API for bookmarks.").await?;

    // Fork two specialists
    let designer = coordinator.fork().await?;
    let writer = coordinator.fork().await?;

    // Run in parallel
    let (endpoints, examples) = tokio::join!(
        designer.chat("Design 5 REST endpoints as a markdown table."),
        writer.chat("Write 3 curl examples for a bookmarks API."),
    );

    println!("=== Endpoints ===\n{}\n", endpoints?);
    println!("=== Examples ===\n{}\n", examples?);

    // Merge in coordinator
    let summary = coordinator
        .chat("Write a one-paragraph summary of our bookmarks API.")
        .await?;
    println!("=== Summary ===\n{summary}");

    Ok(())
}
