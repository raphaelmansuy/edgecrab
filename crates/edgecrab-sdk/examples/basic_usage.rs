use edgecrab_sdk::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agent = SdkAgent::builder("copilot/gpt-5-mini")?
        .max_iterations(10)
        .build()?;

    let reply = agent.chat("Say hello in one short sentence.").await?;
    println!("Reply: {reply}");
    Ok(())
}
