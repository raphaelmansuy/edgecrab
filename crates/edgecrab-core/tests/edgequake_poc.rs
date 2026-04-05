//! edgequake-llm integration Proof-of-Concept.
//!
//! Validates that EdgeCrab can:
//! 1. Create a MockProvider and complete a chat
//! 2. Process tool call round-trips through the LLMProvider trait
//! 3. Use ToolDefinition::function() convenience builder
//!
//! These tests use MockProvider (no API keys needed).

use edgequake_llm::{
    ChatMessage, CompletionOptions, LLMProvider, MockProvider, ToolChoice, ToolDefinition,
};

/// Verify MockProvider can be instantiated and returns a chat completion.
#[tokio::test]
async fn mock_provider_chat_completion() {
    let provider = MockProvider::new();
    let messages = vec![ChatMessage::user("What is Rust?")];

    let response = provider.chat(&messages, None).await;
    assert!(response.is_ok(), "MockProvider chat should succeed");
    let resp = response.expect("checked above");
    assert!(
        !resp.content.is_empty(),
        "Response content should not be empty"
    );
}

/// Verify the trait interface works through a Box<dyn LLMProvider>.
#[tokio::test]
async fn provider_trait_object_dispatch() {
    let provider: Box<dyn LLMProvider> = Box::new(MockProvider::new());
    assert_eq!(provider.name(), "mock");

    let messages = vec![ChatMessage::user("hello")];
    let response = provider.chat(&messages, None).await;
    assert!(response.is_ok());
}

/// Verify tool calling works through the LLMProvider trait.
#[tokio::test]
async fn mock_provider_tool_calling() {
    let provider = MockProvider::new();
    let messages = vec![ChatMessage::user("Read the file src/main.rs")];

    let tools = vec![ToolDefinition::function(
        "read_file",
        "Read a file from disk",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to read" }
            },
            "required": ["path"]
        }),
    )];

    let response = provider
        .chat_with_tools(&messages, &tools, Some(ToolChoice::auto()), None)
        .await;
    assert!(response.is_ok(), "Tool calling should not error");
}

/// Verify CompletionOptions can be passed through.
#[tokio::test]
async fn mock_provider_with_options() {
    let provider = MockProvider::new();

    let options = CompletionOptions {
        temperature: Some(0.7),
        max_tokens: Some(1024),
        ..Default::default()
    };

    let response = provider.complete_with_options("Hello", &options).await;
    assert!(response.is_ok());
}

/// Verify provider metadata accessors.
#[tokio::test]
async fn provider_metadata() {
    let provider = MockProvider::new();
    assert_eq!(provider.name(), "mock");
    assert!(provider.max_context_length() > 0);
    // MockProvider still accepts tool calls via chat_with_tools even if
    // supports_function_calling() returns false (the trait has a default impl).
}
