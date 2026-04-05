//! Shared vision-model normalization and capability policy.
//!
//! WHY centralize this: the CLI vision selector and the runtime
//! `vision_analyze` tool must agree on what `provider/model` means and which
//! backends are plausibly multimodal. Static provider metadata is incomplete
//! for several modern families, so we combine declared capability flags with
//! conservative family-level heuristics.

use edgequake_llm::ModelsConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VisionSupportLevel {
    Unknown,
    Likely,
    Declared,
}

pub fn normalize_provider_name(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "claude" => "anthropic".to_string(),
        "copilot" | "vscode" => "vscode-copilot".to_string(),
        "google" => "gemini".to_string(),
        "open-router" => "openrouter".to_string(),
        "vertex" | "vertex-ai" => "vertexai".to_string(),
        "azure-openai" | "azure_openai" | "azureopenai" => "azure".to_string(),
        "aws-bedrock" | "aws_bedrock" | "aws bedrock" => "bedrock".to_string(),
        other => other.to_string(),
    }
}

pub fn normalize_model_name(provider: &str, model: &str) -> String {
    let trimmed = model.trim();
    match normalize_provider_name(provider).as_str() {
        "vertexai" => trimmed
            .strip_prefix("vertexai:")
            .unwrap_or(trimmed)
            .to_string(),
        _ => trimmed.to_string(),
    }
}

pub fn parse_provider_model_spec(spec: &str) -> Option<(String, String)> {
    let trimmed = spec.trim();
    let (provider, model) = trimmed.split_once('/')?;
    let provider = normalize_provider_name(provider);
    let model = normalize_model_name(&provider, model);
    if provider.is_empty() || model.is_empty() {
        return None;
    }
    Some((provider, model))
}

fn model_family_supports_vision(provider: &str, model: &str) -> bool {
    let provider = normalize_provider_name(provider);
    let model = normalize_model_name(&provider, model);
    let lowered = model.to_ascii_lowercase();
    let matches_any = |patterns: &[&str]| patterns.iter().any(|pattern| lowered.contains(pattern));

    if provider == "vscode-copilot" {
        return true;
    }

    if provider == "gemini" || provider == "vertexai" || lowered.contains("gemini-") {
        return true;
    }

    // Model IDs can be routed or vendor-prefixed, for example:
    // - openai/gpt-5.4
    // - anthropic.claude-3-5-sonnet-20241022-v2:0
    if matches_any(&[
        "claude-3",
        "claude-haiku-4",
        "claude-sonnet-4",
        "claude-opus-4",
        "claude-4",
    ]) {
        return true;
    }

    if matches_any(&["gpt-4o", "gpt-4.1", "gpt-4-turbo", "gpt-5"]) {
        return true;
    }

    if matches_any(&["glm-4.5", "glm-4.7", "glm-5"]) {
        return true;
    }

    if matches_any(&["grok-2-vision", "grok-4"])
        || lowered.contains("amazon.nova")
        || lowered.contains("nova-lite")
        || lowered.contains("nova-pro")
        || lowered.contains("nova-premier")
        || lowered.contains("llava")
        || lowered.contains("bakllava")
        || lowered.contains("llama3.2-vision")
        || lowered.contains("llama-3.2-vision")
        || lowered.contains("llama3-2-11b")
        || lowered.contains("llama3-2-90b")
        || lowered.contains("qwen2-vl")
        || lowered.contains("qwen2.5-vl")
        || lowered.contains("qwen-vl")
        || lowered.starts_with("pixtral")
        || lowered.contains("moondream")
        || lowered.contains("florence")
        || lowered.contains("phi-3-vision")
        || lowered.contains("phi-4-multimodal")
        || lowered.contains("minicpm-v")
        || lowered.contains("vision")
        || lowered.contains("internvl")
        || lowered.contains("-vl")
        || lowered.contains("/vl")
    {
        return true;
    }

    false
}

pub fn vision_support_level(
    models: Option<&ModelsConfig>,
    provider: &str,
    model: &str,
) -> VisionSupportLevel {
    let provider = normalize_provider_name(provider);
    let model = normalize_model_name(&provider, model);

    let declared = models.and_then(|models| {
        models
            .get_model(&provider, &model)
            .map(|card| card.capabilities.supports_vision)
            .or_else(|| {
                models
                    .find_provider_and_model(&model)
                    .map(|(_, card)| card.capabilities.supports_vision)
            })
    });

    match declared {
        Some(true) => VisionSupportLevel::Declared,
        Some(false) => {
            if model_family_supports_vision(&provider, &model) {
                VisionSupportLevel::Likely
            } else {
                VisionSupportLevel::Unknown
            }
        }
        None => {
            if model_family_supports_vision(&provider, &model) {
                VisionSupportLevel::Likely
            } else {
                VisionSupportLevel::Unknown
            }
        }
    }
}

pub fn model_supports_vision(models: Option<&ModelsConfig>, provider: &str, model: &str) -> bool {
    !matches!(
        vision_support_level(models, provider, model),
        VisionSupportLevel::Unknown
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_aliases_normalize() {
        assert_eq!(normalize_provider_name("copilot"), "vscode-copilot");
        assert_eq!(normalize_provider_name("google"), "gemini");
        assert_eq!(normalize_provider_name("vertex-ai"), "vertexai");
        assert_eq!(normalize_provider_name("aws-bedrock"), "bedrock");
    }

    #[test]
    fn provider_model_spec_parses_nested_models() {
        let parsed =
            parse_provider_model_spec("openrouter/openai/gpt-5.4").expect("spec should parse");
        assert_eq!(parsed.0, "openrouter");
        assert_eq!(parsed.1, "openai/gpt-5.4");
    }

    #[test]
    fn heuristic_supports_modern_multimodal_families() {
        assert!(model_supports_vision(None, "openai", "gpt-5.4"));
        assert!(model_supports_vision(
            None,
            "anthropic",
            "claude-sonnet-4.5"
        ));
        assert!(model_supports_vision(None, "google", "gemini-2.5-pro"));
        assert!(model_supports_vision(None, "zai", "glm-4.7"));
        assert!(model_supports_vision(None, "xai", "grok-2-vision-1212"));
        assert!(model_supports_vision(None, "ollama", "llama3.2-vision:11b"));
        assert!(model_supports_vision(None, "ollama", "qwen2.5-vl:7b"));
        assert!(model_supports_vision(
            None,
            "lmstudio",
            "internvl3-14b-instruct"
        ));
        assert!(model_supports_vision(
            None,
            "bedrock",
            "anthropic.claude-3-5-sonnet-20241022-v2:0"
        ));
        assert!(model_supports_vision(
            None,
            "bedrock",
            "amazon.nova-pro-v1:0"
        ));
        assert!(model_supports_vision(
            None,
            "bedrock",
            "meta.llama3-2-11b-instruct-v1:0"
        ));
        assert!(model_supports_vision(None, "azure", "gpt-5.4"));
        assert!(!model_supports_vision(None, "openai", "gpt-3.5-turbo"));
    }

    #[test]
    fn explicit_declared_flag_beats_unknown_family() {
        let models = ModelsConfig::load().expect("built-in models");
        assert_eq!(
            vision_support_level(Some(&models), "openai", "gpt-4o"),
            VisionSupportLevel::Declared
        );
        assert_eq!(
            vision_support_level(Some(&models), "openai", "text-embedding-3-small"),
            VisionSupportLevel::Unknown
        );
    }
}
