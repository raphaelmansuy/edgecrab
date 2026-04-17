use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agent = SdkAgent::new("copilot/gpt-5-mini")?;
    let memory = agent.memory();

    memory
        .write("memory", "User prefers concise answers")
        .await?;
    let content = memory.read("memory").await?;
    println!("Memory contents:\n{content}");

    let entries = memory.entries("memory").await?;
    println!("Entry count: {}", entries.len());
    Ok(())
}
