//! # Session Management — Browse, search, rename, prune, and inspect sessions
//!
//! Demonstrates the full session lifecycle: listing, searching, renaming,
//! cost/usage stats, and cleanup of old sessions.
//!
//! ```bash
//! cargo run --example session_management
//! ```

use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── 1. Create an agent and generate a session ───────────────────
    let agent = SdkAgent::builder("copilot/claude-sonnet-4.6")?
        .quiet_mode(true)
        .build()?;

    let reply = agent
        .chat("Explain the Rust borrow checker in two sentences.")
        .await?;
    println!("Agent: {reply}");

    let session_id = agent.session_id().await.unwrap();
    println!("\nSession ID: [redacted] (len={})", session_id.len());

    // ── 2. List recent sessions ─────────────────────────────────────
    let sessions = agent.list_sessions(5)?;
    println!("\nRecent sessions ({}):", sessions.len());
    for s in &sessions {
        println!(
            "  {} | model={:?} | msgs={} | {:?}",
            &s.id[..8.min(s.id.len())],
            s.model,
            s.message_count,
            s.title,
        );
    }

    // ── 3. Search across all sessions ───────────────────────────────
    let hits = agent.search_sessions("borrow checker", 5)?;
    println!("\nSearch results ({}):", hits.len());
    for h in &hits {
        println!(
            "  session={}... | score={:.2}",
            &h.session.id[..8.min(h.session.id.len())],
            h.score
        );
        println!("    snippet: {}...", &h.snippet[..h.snippet.len().min(80)]);
    }

    // ── 4. Export session snapshot ──────────────────────────────────
    let export = agent.export().await;
    println!(
        "\nExported session: {} messages, model={}",
        export.messages.len(),
        export.snapshot.model,
    );

    // ── 5. Fork and continue in a new branch ───────────────────────
    let child = agent.fork().await?;
    let followup = child.chat("Now explain lifetimes.").await?;
    println!("\nForked agent: {}", &followup[..followup.len().min(120)]);

    Ok(())
}
