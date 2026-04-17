//! # Custom Tool Example — Register a custom tool with the SDK
//!
//! ```bash
//! cargo run --example custom_tool
//! ```

use edgecrab_sdk::prelude::*;

/// Get the current weather for a city.
#[edgecrab_tool(name = "get_weather", toolset = "demo", emoji = "🌤️")]
async fn get_weather(city: String) -> Result<String, edgecrab_types::ToolError> {
    Ok(format!("The weather in {city} is sunny and 22°C."))
}

#[tokio::main]
async fn main() -> Result<(), SdkError> {
    let agent = SdkAgent::builder("anthropic/claude-sonnet-4")?
        .max_iterations(5)
        .build()?;

    let result = agent
        .run("What's the weather in Paris? Use the get_weather tool.")
        .await?;

    println!("Reply: {}", result.final_response);
    println!("API calls: {}", result.api_calls);
    println!("Model: {}", result.model);

    Ok(())
}
