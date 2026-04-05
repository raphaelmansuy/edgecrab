//! Shared vision-model inventory and parsing helpers for the CLI.
//!
//! WHY a dedicated module: the TUI selector, `/vision_model` command, and
//! setup wizard must agree on what counts as a selectable vision backend and
//! how provider aliases are normalized. Centralizing this keeps the UX and
//! persisted config consistent.

use std::collections::BTreeSet;

use edgecrab_core::ModelCatalog;
use edgecrab_tools::vision_models::{
    VisionSupportLevel, model_supports_vision, parse_provider_model_spec, vision_support_level,
};
use edgequake_llm::{ModelType, ModelsConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisionModelOption {
    pub selection_spec: String,
    pub provider: String,
    pub model: String,
    pub detail: String,
    pub support_level: VisionSupportLevel,
}

pub fn display_provider(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "vscode-copilot" => "copilot".to_string(),
        "gemini" => "google".to_string(),
        "vertexai" => "vertexai".to_string(),
        other => other.to_string(),
    }
}

pub fn canonical_provider(provider: &str) -> String {
    edgecrab_tools::vision_models::normalize_provider_name(provider)
}

pub fn parse_selection_spec(spec: &str) -> Option<(String, String)> {
    parse_provider_model_spec(spec)
}

pub fn current_model_supports_vision(model_spec: &str) -> bool {
    let Some((provider, model)) = parse_selection_spec(model_spec) else {
        return false;
    };

    let config = ModelsConfig::load().ok();
    model_supports_vision(config.as_ref(), &provider, &model)
}

pub fn available_vision_model_options() -> Vec<VisionModelOption> {
    available_vision_model_options_with_dynamic(&[])
}

fn vision_support_detail(level: VisionSupportLevel, provider_detail: &str) -> String {
    let prefix = match level {
        VisionSupportLevel::Declared => "Vision-ready",
        VisionSupportLevel::Likely => "Likely multimodal",
        VisionSupportLevel::Unknown => "Unverified for vision",
    };
    format!("{prefix} - {provider_detail}")
}

pub fn available_vision_model_options_with_dynamic(
    dynamic: &[(String, Vec<String>)],
) -> Vec<VisionModelOption> {
    let mut options = Vec::new();
    let mut seen = BTreeSet::new();

    if let Ok(config) = ModelsConfig::load() {
        let mut providers: Vec<_> = config
            .providers
            .iter()
            .filter(|provider| provider.enabled)
            .collect();
        providers.sort_by_key(|provider| provider.priority);

        for provider in providers {
            for model in provider.models.iter().filter(|model| {
                matches!(model.model_type, ModelType::Llm | ModelType::Multimodal)
                    && !model.deprecated
            }) {
                let display_provider = display_provider(&provider.name);
                let selection_spec = format!("{display_provider}/{}", model.name);
                if !seen.insert(selection_spec.clone()) {
                    continue;
                }
                let support_level =
                    vision_support_level(Some(&config), &provider.name, &model.name);
                options.push(VisionModelOption {
                    selection_spec,
                    provider: display_provider,
                    model: model.name.clone(),
                    detail: vision_support_detail(support_level, &provider.display_name),
                    support_level,
                });
            }
        }
    }

    for (provider, model) in ModelCatalog::flat_catalog()
        .into_iter()
        .map(|(_display, provider, model)| (provider, model))
    {
        let display_provider = display_provider(&provider);
        let selection_spec = format!("{display_provider}/{model}");
        if !seen.insert(selection_spec.clone()) {
            continue;
        }
        let support_level = vision_support_level(None, &provider, &model);
        options.push(VisionModelOption {
            selection_spec,
            provider: display_provider,
            model,
            detail: vision_support_detail(support_level, &ModelCatalog::provider_label(&provider)),
            support_level,
        });
    }

    for (provider, models) in dynamic {
        for model in models {
            let display_provider = display_provider(provider);
            let selection_spec = format!("{display_provider}/{model}");
            if !seen.insert(selection_spec.clone()) {
                continue;
            }
            let support_level = vision_support_level(None, provider, model);
            options.push(VisionModelOption {
                selection_spec,
                provider: display_provider,
                model: model.clone(),
                detail: vision_support_detail(
                    support_level,
                    &ModelCatalog::provider_label(provider),
                ),
                support_level,
            });
        }
    }

    options.sort_by(|left, right| {
        right
            .support_level
            .cmp(&left.support_level)
            .then_with(|| left.selection_spec.cmp(&right.selection_spec))
    });
    options
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_aliases_round_trip() {
        assert_eq!(display_provider("vscode-copilot"), "copilot");
        assert_eq!(canonical_provider("copilot"), "vscode-copilot");
        assert_eq!(canonical_provider("google"), "gemini");
    }

    #[test]
    fn parse_selection_spec_normalizes_aliases() {
        let (provider, model) =
            parse_selection_spec("copilot/gpt-5.4").expect("selection spec should parse");
        assert_eq!(provider, "vscode-copilot");
        assert_eq!(model, "gpt-5.4");
    }

    #[test]
    fn vision_options_include_auto_candidates() {
        let options = available_vision_model_options();
        assert!(!options.is_empty());
        assert!(
            options
                .iter()
                .any(|option| option.selection_spec == "openai/gpt-4o")
        );
        assert!(
            options
                .iter()
                .any(|option| option.selection_spec == "openai/gpt-3.5-turbo")
        );
    }

    #[test]
    fn copilot_is_treated_as_vision_capable_in_cli_policy() {
        assert!(current_model_supports_vision("copilot/gpt-5.4"));
    }
}
