//! # Cost-Aware Code Review — Tutorial 1
//!
//! Demonstrates two-tier triage: use a cheap model (`copilot/gpt-5-mini`) to
//! risk-score every diff, then hot-swap to a flagship model only for
//! high-risk diffs. Cuts LLM spend by ~80% on realistic review streams.
//!
//! ```bash
//! cargo run --example cost_aware_review
//! ```
//!
//! See: site/src/content/docs/tutorials/01-cost-aware-review.md

use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let diffs = vec![
        "Renamed `count` to `total_count` in stats.rs",
        "Added new auth middleware with JWT + refresh token logic",
        "Updated README typo: 'recieve' -> 'receive'",
    ];

    let agent = SdkAgent::builder("copilot/gpt-5-mini")?
        .max_iterations(4)
        .quiet_mode(true)
        .instructions(
            "You are a strict code reviewer. For each diff, \
             output ONE line: RISK=<1-10> SUMMARY=<10 words max>.",
        )
        .build()?;

    let mut total_cost = 0.0;
    let mut escalated = 0;

    for diff in &diffs {
        let triage = agent.run(&format!("Diff:\n{diff}")).await?;
        total_cost += triage.cost.total_cost;
        let risk = parse_risk(&triage.final_response);

        let head = triage.final_response.lines().next().unwrap_or("");
        println!("[tier1] risk={risk} | {head}");

        if risk < 7 {
            println!("  → APPROVED (no escalation)\n");
            continue;
        }

        escalated += 1;
        agent.set_model("copilot/claude-sonnet-4.6").await?;

        let deep = agent
            .run(
                "The diff was flagged high-risk. \
                 Give a detailed review and 2 concrete fix suggestions.",
            )
            .await?;
        total_cost += deep.cost.total_cost;
        let head = deep.final_response.lines().next().unwrap_or("");
        println!("  [tier2] → {head}\n");

        agent.set_model("copilot/gpt-5-mini").await?;
    }

    println!("──────────────────────────────");
    println!("Total diffs:   {}", diffs.len());
    println!(
        "Escalated:     {} ({:.0}%)",
        escalated,
        100.0 * escalated as f64 / diffs.len() as f64
    );
    println!("Total cost:    ${:.6}", total_cost);
    println!("Cost per diff: ${:.6}", total_cost / diffs.len() as f64);
    Ok(())
}

fn parse_risk(s: &str) -> u32 {
    s.split_whitespace()
        .find_map(|w| w.strip_prefix("RISK="))
        .and_then(|v| v.parse().ok())
        .unwrap_or(5)
}
