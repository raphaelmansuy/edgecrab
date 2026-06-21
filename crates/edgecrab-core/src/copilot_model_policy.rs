//! Copilot model eligibility for the EdgeCrab agent loop (Hermes `models.py` parity).
//!
//! Copilot advertises routing/picker models that must not be sent to `/chat/completions`.
//! Filtering at discovery + preflight avoids infinite "waiting for first token" stalls.

use edgequake_llm::CopilotModel;

/// Whether a user-facing model spec targets GitHub Copilot.
pub fn is_copilot_model_spec(spec: &str) -> bool {
    let lower = spec.trim().to_ascii_lowercase();
    lower.starts_with("copilot/") || lower.starts_with("vscode-copilot/")
}

/// Fast local check — no network. Returns a user-facing reason when the model id
/// is obviously not a direct agent chat target.
pub fn copilot_model_id_reject_reason(model_id: &str) -> Option<&'static str> {
    let id = model_id.trim().trim_start_matches("copilot/");
    if id.is_empty() || id.eq_ignore_ascii_case("auto") {
        return None;
    }
    if id.ends_with("-picker") || id.contains("flash-picker") {
        return Some(
            "Copilot picker/routing models (names ending in `-picker`) cannot be used as the agent model. Try `/model copilot/auto` or a chat-capable model such as `copilot/gpt-4.1-mini`.",
        );
    }
    None
}

fn supports_endpoint(model: &CopilotModel, endpoint: &str) -> bool {
    model
        .supported_endpoints
        .as_ref()
        .map(|endpoints| {
            endpoints.is_empty()
                || endpoints
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(endpoint))
        })
        .unwrap_or(true)
}

/// Whether a Copilot catalog entry is selectable in `/model` for the agent loop.
pub fn copilot_model_is_agent_selectable(model: &CopilotModel) -> bool {
    if copilot_model_id_reject_reason(&model.id).is_some() {
        return false;
    }

    let picker_enabled = model.model_picker_enabled.unwrap_or(true);
    let is_chat = model
        .capabilities
        .as_ref()
        .and_then(|capabilities| capabilities.model_type.as_deref())
        .map(|model_type| model_type.eq_ignore_ascii_case("chat"))
        .unwrap_or(true);

    picker_enabled && is_chat && supports_endpoint(model, "/chat/completions")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_copilot_model_specs() {
        assert!(is_copilot_model_spec("copilot/gpt-4.1"));
        assert!(is_copilot_model_spec("vscode-copilot/claude-haiku-4.5"));
        assert!(!is_copilot_model_spec("anthropic/claude-opus-4.6"));
    }

    #[test]
    fn rejects_picker_suffix_ids() {
        assert!(copilot_model_id_reject_reason("mai-code-1-flash-picker").is_some());
        assert!(copilot_model_id_reject_reason("copilot/mai-code-1-flash-picker").is_some());
        assert!(copilot_model_id_reject_reason("copilot/auto").is_none());
    }

    #[test]
    fn filters_copilot_catalog_entries() {
        let response: edgequake_llm::CopilotModelsResponse =
            serde_json::from_value(serde_json::json!({
                "data": [
                    {
                        "id": "gpt-4.1",
                        "model_picker_enabled": true,
                        "supported_endpoints": ["/chat/completions"],
                        "capabilities": { "type": "chat" }
                    },
                    {
                        "id": "mai-code-1-flash-picker",
                        "model_picker_enabled": true,
                        "supported_endpoints": ["/chat/completions"],
                        "capabilities": { "type": "chat" }
                    },
                    {
                        "id": "gpt-5.4",
                        "model_picker_enabled": true,
                        "supported_endpoints": ["/responses"],
                        "capabilities": { "type": "chat" }
                    }
                ]
            }))
            .expect("copilot response");

        let selectable: Vec<_> = response
            .data
            .iter()
            .filter(|model| copilot_model_is_agent_selectable(model))
            .map(|model| model.id.as_str())
            .collect();
        assert_eq!(selectable, vec!["gpt-4.1"]);
    }
}
