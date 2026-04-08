use std::sync::Arc;

use edgequake_llm::{LLMProvider, ProviderFactory, VsCodeCopilotProvider};

use crate::vision_models::normalize_provider_name;

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

    if canonical == "vscode-copilot" {
        return VsCodeCopilotProvider::new()
            .model(model_name)
            .with_vision(true)
            .build()
            .map(|provider| Arc::new(provider) as Arc<dyn LLMProvider>)
            .map_err(|err| err.to_string());
    }

    ProviderFactory::create_llm_provider(&canonical, model_name).map_err(|err| err.to_string())
}
