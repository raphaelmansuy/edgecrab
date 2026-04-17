use edgecrab_sdk::prelude::*;
use edgecrab_sdk::types::SdkModelCatalog;

const MODEL: &str = "ollama/gemma4:latest";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("EdgeCrab Rust SDK — Business Case Showcase");
    println!("Model: {MODEL}");

    if let Some((input_cost, output_cost, total_cost)) =
        SdkModelCatalog::estimate_cost("openai", "gpt-5-mini", 2000, 600)
    {
        println!(
            "Reference cost estimate for openai/gpt-5-mini: input=${input_cost:.4}, output=${output_cost:.4}, total=${total_cost:.4}"
        );
    }

    let agent = SdkAgent::builder(MODEL)?
        .max_iterations(4)
        .quiet_mode(true)
        .skip_context_files(true)
        .instructions(
            "You are an operations copilot for a growth-stage software company. Be concise, structured, and business-minded.",
        )
        .build()?;

    let memory = agent.memory();
    memory
        .write(
            "memory",
            "Company context: AcmeCloud sells workflow automation to mid-market SaaS teams. Priority metrics are churn, expansion revenue, and support resolution speed.",
        )
        .await?;

    let scenarios = [
        (
            "Support triage",
            "A customer says: 'Our onboarding export is failing and we need a fix before tomorrow morning.' Reply with a severity, owner, and next action.",
        ),
        (
            "Executive brief",
            "Turn these notes into a short VP-ready update: churn stable, enterprise pipeline up 18%, support backlog down 12%, one release risk in payments.",
        ),
        (
            "Sales preparation",
            "Create a short account brief for a renewal call with a customer who wants better audit logs, faster support, and predictable pricing.",
        ),
    ];

    for (title, prompt) in scenarios {
        println!("\n=== {title} ===");
        let reply = agent.chat(prompt).await?;
        println!("{reply}\n");
    }

    println!("=== Batch customer quote summaries ===");
    let prompts = [
        "Summarize this quote in one sentence: 'We love the workflow builder but permissions are still confusing.'",
        "Summarize this quote in one sentence: 'The rollout was smooth and our ops team saved hours every week.'",
        "Summarize this quote in one sentence: 'We need stronger reporting before we expand to more teams.'",
    ];
    let refs: Vec<&str> = prompts.to_vec();
    for (idx, result) in agent.batch(&refs).await.into_iter().enumerate() {
        match result {
            Ok(text) => println!("{}. {}", idx + 1, text.replace('\n', " ")),
            Err(err) => println!("{}. ERROR: {err}", idx + 1),
        }
    }

    Ok(())
}
