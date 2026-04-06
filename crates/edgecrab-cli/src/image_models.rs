//! Shared image-generation model inventory and parsing helpers for the CLI.
//!
//! WHY a dedicated module: `generate_image`, `/image_model`, and persisted
//! config must agree on provider aliases and the available provider/model
//! combinations exposed by `edgequake-llm`.

use edgequake_llm::{FalImageGen, GeminiImageGenProvider, ImageGenProvider, VertexAIImageGen};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageModelOption {
    pub selection_spec: String,
    pub provider: String,
    pub model: String,
    pub detail: String,
}

pub fn canonical_provider(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "google" | "gemini" => "gemini".to_string(),
        "vertex" | "vertexai" | "vertex-gemini" => "vertexai".to_string(),
        "imagen" | "vertex-imagen" => "imagen".to_string(),
        "fal" | "fal-ai" => "fal".to_string(),
        "openai" => "openai".to_string(),
        other => other.to_string(),
    }
}

pub fn parse_selection_spec(spec: &str) -> Option<(String, String)> {
    let (provider, model) = spec.trim().split_once('/')?;
    let provider = canonical_provider(provider);
    let model = model.trim();
    if provider.is_empty() || model.is_empty() {
        return None;
    }
    Some((provider, model.to_string()))
}

pub fn default_selection_spec() -> String {
    "gemini/gemini-2.5-flash-image".to_string()
}

pub fn available_image_model_options() -> Vec<ImageModelOption> {
    let mut options = Vec::new();

    append_provider_models(
        &mut options,
        "gemini",
        "Google Gemini image via edgequake-llm",
        GeminiImageGenProvider::new("test-key").available_models(),
    );
    append_provider_models(
        &mut options,
        "vertexai",
        "Vertex AI Gemini image via edgequake-llm",
        GeminiImageGenProvider::vertex_ai("test-project", "us-central1", "token")
            .available_models(),
    );
    append_provider_models(
        &mut options,
        "imagen",
        "Vertex Imagen via edgequake-llm",
        VertexAIImageGen::new("test-project", "us-central1", "token").available_models(),
    );
    append_provider_models(
        &mut options,
        "fal",
        "FAL image generation via edgequake-llm",
        FalImageGen::new("test-key").available_models(),
    );
    options.push(ImageModelOption {
        selection_spec: "openai/dall-e-3".into(),
        provider: "openai".into(),
        model: "dall-e-3".into(),
        detail: "Legacy OpenAI fallback outside edgequake-llm".into(),
    });

    options.sort_by(|left, right| {
        image_model_rank(&left.selection_spec)
            .cmp(&image_model_rank(&right.selection_spec))
            .then_with(|| left.selection_spec.cmp(&right.selection_spec))
    });
    options
}

fn append_provider_models(
    out: &mut Vec<ImageModelOption>,
    provider: &str,
    detail: &str,
    models: Vec<&str>,
) {
    for model in models {
        out.push(ImageModelOption {
            selection_spec: format!("{provider}/{model}"),
            provider: provider.to_string(),
            model: model.to_string(),
            detail: detail.to_string(),
        });
    }
}

fn image_model_rank(selection_spec: &str) -> usize {
    match selection_spec {
        "gemini/gemini-2.5-flash-image" => 0,
        "vertexai/gemini-2.5-flash-image" => 1,
        "imagen/imagen-4.0-fast-generate-001" => 2,
        _ => 10,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_selection_spec_normalizes_aliases() {
        let (provider, model) =
            parse_selection_spec("google/gemini-2.5-flash-image").expect("selection spec");
        assert_eq!(provider, "gemini");
        assert_eq!(model, "gemini-2.5-flash-image");
    }

    #[test]
    fn inventory_includes_default_edgequake_models() {
        let options = available_image_model_options();
        assert!(
            options
                .iter()
                .any(|option| option.selection_spec == "gemini/gemini-2.5-flash-image")
        );
        assert!(
            options
                .iter()
                .any(|option| option.selection_spec == "imagen/imagen-4.0-generate-001")
        );
        assert!(
            options
                .iter()
                .any(|option| option.selection_spec == "fal/fal-ai/flux/dev")
        );
    }
}
