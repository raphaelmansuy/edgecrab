//! # Multi-Agent Pipeline — Orchestrate multiple agents for complex tasks
//!
//! Demonstrates forking agents, setting different models, and composing
//! results from multiple specialists — a pattern for building supervisor
//! / worker architectures.
//!
//! ```bash
//! cargo run --example multi_agent
//! ```

use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── 1. Create a coordinator agent ───────────────────────────────
    let coordinator = SdkAgent::builder("copilot/claude-sonnet-4.6")?
        .max_iterations(5)
        .quiet_mode(true)
        .instructions("You are a technical writing coordinator.")
        .build()?;

    // Establish project context
    coordinator
        .chat("We're writing documentation for a REST API that manages bookmarks.")
        .await?;

    // ── 2. Fork specialists ─────────────────────────────────────────
    // Each fork inherits the conversation context but diverges from here.

    let api_designer = coordinator.fork().await?;
    let example_writer = coordinator.fork().await?;

    // Optionally switch to a faster/cheaper model for simpler tasks
    example_writer.set_model("copilot/gpt-5-mini").await.ok();

    // ── 3. Run specialists in parallel ──────────────────────────────
    let (endpoints, examples) = tokio::join!(
        api_designer.chat(
            "Design 5 REST endpoints for the bookmarks API. \
             Return a markdown table with Method, Path, and Description."
        ),
        example_writer.chat(
            "Write 3 curl examples for a bookmarks API: \
             create a bookmark, list all bookmarks, delete one."
        ),
    );

    println!("=== API Design ===\n{}\n", endpoints?);
    println!("=== Examples ===\n{}\n", examples?);

    // ── 4. Merge results back in the coordinator ────────────────────
    let api_text = api_designer
        .messages()
        .await
        .last()
        .map(|m| m.text_content())
        .unwrap_or_default();

    let summary = coordinator
        .chat(&format!(
            "Here are the API endpoints our team designed:\n{api_text}\n\n\
             Write a one-paragraph executive summary of this API."
        ))
        .await?;

    println!("=== Summary ===\n{summary}");

    Ok(())
}
