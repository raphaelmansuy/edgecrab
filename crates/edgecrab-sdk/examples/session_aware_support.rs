//! # Tutorial 4 — Session-Aware Support Bot
//!
//! FTS5 search finds relevant prior tickets in <10ms, injects them as
//! context so the agent resolves issues faster using org memory.
//!
//! ```bash
//! cargo run --example session_aware_support
//! ```

use edgecrab_sdk::SdkSession;
use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let user_message = "My K8s pods keep OOMKilling under load.";

    // ── 1. Open the shared session DB and search prior tickets ──────
    let db_path = edgecrab_home().join("sessions.db");
    let prior_context = if db_path.exists() {
        let db = SdkSession::open(&db_path)?;
        let hits = db.search_sessions(user_message, 3)?;

        if hits.is_empty() {
            String::new()
        } else {
            let mut s = String::from("\n\n--- RELEVANT PRIOR TICKETS ---\n");
            for h in &hits {
                let id_short = &h.session.id[..8.min(h.session.id.len())];
                s.push_str(&format!(
                    "[session={id_short}, score={:.2}]\n{}\n\n",
                    h.score, h.snippet,
                ));
            }
            println!("[search] found {} prior session(s)", hits.len());
            s
        }
    } else {
        println!("[search] no session DB found — starting cold");
        String::new()
    };

    // ── 2. Spin up agent with prior context pre-loaded ──────────────
    let agent = SdkAgent::builder("copilot/gpt-5-mini")?
        .max_iterations(6)
        .quiet_mode(true)
        .instructions(
            "You are a senior support engineer. If prior tickets contain \
             the answer, reference them by session ID. Always state the \
             ROOT CAUSE before the FIX.",
        )
        .build()?;

    let result = agent
        .run(&format!("User ticket: {user_message}{prior_context}"))
        .await?;

    println!("Resolution:\n{}", result.final_response);
    println!("\nSession:  persisted for follow-up");
    println!("Cost:     ${:.6}", result.cost.total_cost);

    Ok(())
}
