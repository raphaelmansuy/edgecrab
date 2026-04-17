//! # Parallel Research — Tutorial 2
//!
//! Runs 5 independent queries concurrently via `batch()`. Wall-clock time is
//! bounded by the slowest call, not the sum.
//!
//! ```bash
//! cargo run --example parallel_research
//! ```
//!
//! See: site/src/content/docs/tutorials/02-parallel-research.md

use edgecrab_sdk::prelude::*;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agent = SdkAgent::builder("copilot/gpt-5-mini")?
        .max_iterations(3)
        .quiet_mode(true)
        .build()?;

    let queries = [
        "Summarize the Rust borrow checker in one sentence.",
        "Summarize Python's GIL in one sentence.",
        "Summarize Go's goroutines in one sentence.",
        "Summarize Erlang's actor model in one sentence.",
        "Summarize Haskell's laziness in one sentence.",
    ];
    let refs: Vec<&str> = queries.to_vec();

    let t0 = Instant::now();
    let results = agent.batch(&refs).await;
    let elapsed = t0.elapsed();

    for (q, r) in queries.iter().zip(results.iter()) {
        match r {
            Ok(reply) => println!("Q: {q}\n  → {reply}\n"),
            Err(e) => println!("Q: {q}\n  × ERROR: {e}\n"),
        }
    }

    println!("── wall-clock: {:.2}s ──", elapsed.as_secs_f64());
    Ok(())
}
