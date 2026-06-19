//! LLM provider tracing wrapper — single factory seam (SOLID: one place to decorate).
//!
//! Delegates GenAI span emission to edgequake-llm [`trace_llm_arc`] (DRY).

use std::sync::Arc;

use edgequake_llm::LLMProvider;

/// Whether to wrap providers with OpenTelemetry GenAI spans.
///
/// Disable with `EDGECRAB_TRACE_LLM=0` (tests, micro-benchmarks).
pub fn llm_tracing_enabled() -> bool {
    match std::env::var("EDGECRAB_TRACE_LLM") {
        Ok(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            normalized != "0" && normalized != "false" && normalized != "off"
        }
        Err(_) => true,
    }
}

/// Apply the shared GenAI tracing decorator when enabled.
pub fn wrap_provider_with_tracing(provider: Arc<dyn LLMProvider>) -> Arc<dyn LLMProvider> {
    if llm_tracing_enabled() {
        edgequake_llm::trace_llm_arc(provider)
    } else {
        provider
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgequake_llm::MockProvider;

    #[test]
    fn wrap_preserves_provider_identity() {
        let inner: Arc<dyn LLMProvider> = Arc::new(MockProvider::new());
        let wrapped = wrap_provider_with_tracing(inner);
        assert_eq!(wrapped.name(), "mock");
        assert_eq!(wrapped.model(), "mock-model");
    }

    #[test]
    fn tracing_can_be_disabled_via_env() {
        // SAFETY: test-only env mutation.
        unsafe { std::env::set_var("EDGECRAB_TRACE_LLM", "0") };
        let inner: Arc<dyn LLMProvider> = Arc::new(MockProvider::new());
        let ptr_before = Arc::as_ptr(&inner);
        let wrapped = wrap_provider_with_tracing(inner);
        assert_eq!(Arc::as_ptr(&wrapped), ptr_before);
        unsafe { std::env::remove_var("EDGECRAB_TRACE_LLM") };
    }
}
