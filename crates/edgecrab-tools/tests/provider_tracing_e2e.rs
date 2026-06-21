//! Provider tracing factory e2e — GenAI span wrapper is applied at creation time.

use std::sync::Arc;

use edgecrab_tools::{create_provider_for_model, llm_tracing_enabled, wrap_provider_with_tracing};
use edgequake_llm::{LLMProvider, MockProvider};

#[test]
fn create_provider_for_model_wraps_mock_with_tracing() {
    unsafe { std::env::remove_var("EDGECRAB_TRACE_LLM") };
    let provider =
        create_provider_for_model("mock", "mock-model").expect("mock provider should construct");
    assert_eq!(provider.name(), "mock");
    assert_ne!(
        std::any::type_name_of_val(provider.as_ref()),
        std::any::type_name_of_val(&MockProvider::new()),
        "provider should be wrapped when tracing is enabled"
    );
}

#[test]
fn tracing_wrap_can_be_disabled_for_tests() {
    unsafe { std::env::set_var("EDGECRAB_TRACE_LLM", "0") };
    let inner: Arc<dyn LLMProvider> = Arc::new(MockProvider::new());
    let ptr = Arc::as_ptr(&inner);
    let wrapped = wrap_provider_with_tracing(inner);
    assert_eq!(Arc::as_ptr(&wrapped), ptr);
    unsafe { std::env::remove_var("EDGECRAB_TRACE_LLM") };
    assert!(llm_tracing_enabled());
}

#[tokio::test]
async fn traced_mock_provider_chat_succeeds() {
    unsafe { std::env::remove_var("EDGECRAB_TRACE_LLM") };
    let provider = create_provider_for_model("mock", "mock-model").expect("mock provider");
    let messages = vec![edgequake_llm::ChatMessage::user("ping")];
    let response = provider.chat(&messages, None).await.expect("chat");
    assert!(!response.content.is_empty());
}
