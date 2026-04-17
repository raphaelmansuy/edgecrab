//! # Full Conversation — Detailed results with cost, usage, and tool errors
//!
//! Demonstrates `agent.run()` which returns a `ConversationResult` with
//! token usage, cost breakdown, tool error records, and session metadata.
//!
//! ```bash
//! cargo run --example full_conversation
//! ```

use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agent = SdkAgent::builder("copilot/claude-sonnet-4.6")?
        .max_iterations(15)
        .quiet_mode(true)
        .build()?;

    // ── Run a full conversation ─────────────────────────────────────
    let result = agent
        .run("What are the top 3 design patterns in Rust? Give a brief example for each.")
        .await?;

    // ── Inspect the result ──────────────────────────────────────────
    println!("=== Conversation Result ===\n");
    println!(
        "Response:\n{}\n",
        &result.final_response[..result.final_response.len().min(500)]
    );
    println!("--- Metadata ---");
    println!("Model:            {}", result.model);
    println!("Session ID:       {}", &result.session_id[..12]);
    println!("API calls:        {}", result.api_calls);
    println!("Interrupted:      {}", result.interrupted);
    println!("Budget exhausted: {}", result.budget_exhausted);

    println!("\n--- Token Usage ---");
    println!("Input tokens:     {}", result.usage.input_tokens);
    println!("Output tokens:    {}", result.usage.output_tokens);
    println!("Cache read:       {}", result.usage.cache_read_tokens);
    println!("Cache write:      {}", result.usage.cache_write_tokens);
    println!("Reasoning tokens: {}", result.usage.reasoning_tokens);
    println!("Total tokens:     {}", result.usage.total_tokens);

    println!("\n--- Cost Breakdown ---");
    println!("Input cost:       ${:.6}", result.cost.input_cost);
    println!("Output cost:      ${:.6}", result.cost.output_cost);
    println!("Cache read cost:  ${:.6}", result.cost.cache_read_cost);
    println!("Cache write cost: ${:.6}", result.cost.cache_write_cost);
    println!("Total cost:       ${:.6}", result.cost.total_cost);

    if !result.tool_errors.is_empty() {
        println!("\n--- Tool Errors ---");
        for err in &result.tool_errors {
            println!("  Turn {}: {} — {}", err.turn, err.tool_name, err.error);
        }
    }

    println!("\nMessages in conversation: {}", result.messages.len());

    Ok(())
}
