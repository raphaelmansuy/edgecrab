//! Copilot agent-turn health probe (chat + tools).
//!
//! Plain `complete("ping")` can succeed while `chat_with_tools` fails with
//! `model_not_supported` — doctor and preflight use this module to match the
//! real agent loop.

use edgequake_llm::{ChatMessage, CompletionOptions, LLMProvider, ToolDefinition};

/// User-facing hint when Copilot rejects a model for agent turns.
pub const MODEL_NOT_SUPPORTED_HINT: &str = "GitHub Copilot rejected this model for agent chat+tools (model_not_supported). \
     Try `/model copilot/auto` or `/model copilot/gpt-4.1-mini`. \
     Run `edgecrab doctor` — the agent ping exercises chat+tools, not plain completion.";

/// Whether an error string indicates Copilot `model_not_supported`.
pub fn is_copilot_model_not_supported_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("model_not_supported")
}

/// Minimal tool schema for agent-turn probes (one no-arg function).
pub fn agent_probe_tools() -> Vec<ToolDefinition> {
    vec![ToolDefinition::function(
        "ping",
        "Health-check stub; do not call unless asked.",
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
    )]
}

/// Probe the configured provider the same way the agent loop does: chat + tools.
pub async fn probe_agent_chat_with_tools(provider: &dyn LLMProvider) -> Result<(), String> {
    let messages = [ChatMessage::user(
        "Reply with exactly the single word PONG. Do not call any tools.",
    )];
    let tools = agent_probe_tools();
    let opts = CompletionOptions {
        temperature: Some(0.0),
        max_tokens: Some(32),
        ..Default::default()
    };

    provider
        .chat_with_tools(&messages, &tools, None, Some(&opts))
        .await
        .map(|_| ())
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_model_not_supported_errors() {
        assert!(is_copilot_model_not_supported_error(
            r#"{"error":{"code":"model_not_supported","message":"nope"}}"#
        ));
        assert!(!is_copilot_model_not_supported_error("bad credentials"));
    }
}
