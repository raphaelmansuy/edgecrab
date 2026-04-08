use std::sync::Arc;
use std::time::Duration;

use edgequake_llm::{LLMProvider, ProviderFactory, VsCodeCopilotProvider};

use crate::vision_models::{normalize_model_name, normalize_provider_name};

const COPILOT_REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

pub fn build_copilot_provider(
    model_name: &str,
    supports_vision: bool,
) -> Result<VsCodeCopilotProvider, String> {
    let mut builder = VsCodeCopilotProvider::new()
        .model(model_name)
        .timeout(COPILOT_REQUEST_TIMEOUT);
    if supports_vision {
        builder = builder.with_vision(true);
    }

    builder.build().map_err(|err| err.to_string())
}

/// Create a provider for an explicit provider/model target.
///
/// WHY this helper exists: `ProviderFactory::create_llm_provider("vscode-copilot", ...)`
/// currently forces proxy mode via localhost:4141, while the main CLI/TUI paths use
/// `VsCodeCopilotProvider::new()` directly so Copilot works in normal sessions.
/// Centralizing the branching here keeps all secondary-provider flows consistent.
pub fn create_provider_for_model(
    provider_name: &str,
    model_name: &str,
) -> Result<Arc<dyn LLMProvider>, String> {
    let canonical = normalize_provider_name(provider_name);
    let normalized_model = normalize_model_name(&canonical, model_name);

    if canonical == "vscode-copilot" {
        return build_copilot_provider(&normalized_model, true)
            .map(|provider| Arc::new(provider) as Arc<dyn LLMProvider>);
    }

    if canonical == "vertexai" {
        ensure_google_cloud_project()?;
        maybe_set_vertex_global_region(&normalized_model);
        let vertex_model = format!("vertexai:{normalized_model}");
        return ProviderFactory::create_llm_provider(&canonical, &vertex_model)
            .map_err(|err| err.to_string());
    }

    ProviderFactory::create_llm_provider(&canonical, &normalized_model)
        .map_err(|err| err.to_string())
}

pub fn create_copilot_provider_for_model(
    model_name: &str,
    supports_vision: bool,
) -> Result<Arc<dyn LLMProvider>, String> {
    let normalized_model = normalize_model_name("vscode-copilot", model_name);
    build_copilot_provider(&normalized_model, supports_vision)
        .map(|provider| Arc::new(provider) as Arc<dyn LLMProvider>)
}

fn ensure_google_cloud_project() -> Result<(), String> {
    if std::env::var("GOOGLE_CLOUD_PROJECT").is_ok() {
        return Ok(());
    }

    match std::process::Command::new("gcloud")
        .args(["config", "get-value", "project"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let raw = String::from_utf8_lossy(&output.stdout);
            let project = raw.trim();
            if project.is_empty() || project == "(unset)" {
                return Err(
                    "VertexAI requires GOOGLE_CLOUD_PROJECT, but gcloud returned empty/unset.\n\
                     Fix: gcloud config set project <your-project-id>\n\
                     or: export GOOGLE_CLOUD_PROJECT=<your-project-id>"
                        .into(),
                );
            }
            // SAFETY: provider construction happens during request startup; env writes
            // only establish process-wide provider configuration.
            unsafe { std::env::set_var("GOOGLE_CLOUD_PROJECT", project) };
            Ok(())
        }
        Ok(_) => Err(
            "gcloud exited with a non-zero status while detecting GOOGLE_CLOUD_PROJECT.\n\
             Fix: export GOOGLE_CLOUD_PROJECT=<your-project-id>"
                .into(),
        ),
        Err(_) => Err(
            "GOOGLE_CLOUD_PROJECT is not set and gcloud was not found in PATH.\n\
             Fix: export GOOGLE_CLOUD_PROJECT=<your-project-id>\n\
             or: install the Google Cloud SDK and run gcloud auth login"
                .into(),
        ),
    }
}

fn maybe_set_vertex_global_region(model_name: &str) {
    if model_name.starts_with("gemini-3") && std::env::var("GOOGLE_CLOUD_REGION").is_err() {
        // SAFETY: provider construction establishes process-wide provider config.
        unsafe { std::env::set_var("GOOGLE_CLOUD_REGION", "global") };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_model_name_strips_vertex_prefix_for_provider_creation() {
        assert_eq!(
            normalize_model_name("vertexai", "vertexai:gemini-3.1-pro-preview"),
            "gemini-3.1-pro-preview"
        );
        assert_eq!(
            normalize_model_name("vertexai", "gemini-2.5-flash"),
            "gemini-2.5-flash"
        );
    }

    #[test]
    fn normalize_provider_name_is_applied_before_provider_creation() {
        assert_eq!(normalize_provider_name("copilot"), "vscode-copilot");
        assert_eq!(normalize_provider_name("vertex-ai"), "vertexai");
    }

    #[test]
    fn copilot_provider_helper_normalizes_model_names() {
        let normalized = normalize_model_name("vscode-copilot", "gpt-4.1");
        assert_eq!(normalized, "gpt-4.1");
    }
}
