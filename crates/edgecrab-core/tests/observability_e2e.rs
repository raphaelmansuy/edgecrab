//! E2E-style tests for structured observability (hooks + filter directives).
//!
//! Deterministic — no live provider or subscriber init required.

use edgecrab_core::{
    OBSERVABILITY_FILTER_DIRECTIVES, TARGET_GENAI_SPANS, TARGET_HARNESS, TARGET_LOCAL_LLM,
    TARGET_PROVIDER_LLM,
    observability::{LlmCorrelation, LlmRequestStart, agent_conversation_span, apply_runtime_from_config, llm_post_hook_json, llm_pre_hook_json},
    ObservabilityConfig,
};
use edgecrab_types::Platform;
use edgequake_llm::LLMResponse;

#[test]
fn observability_targets_are_stable_strings() {
    assert_eq!(TARGET_PROVIDER_LLM, "edgecrab::provider_llm");
    assert_eq!(TARGET_HARNESS, "edgecrab::harness");
    assert_eq!(TARGET_LOCAL_LLM, "edgecrab::local_llm");
    assert_eq!(TARGET_GENAI_SPANS, "edgequake_llm::providers::tracing");
    assert_eq!(OBSERVABILITY_FILTER_DIRECTIVES.len(), 4);
}

#[test]
fn observability_directives_include_genai_span_module() {
    let joined = OBSERVABILITY_FILTER_DIRECTIVES.join(",");
    assert!(joined.contains("edgequake_llm::providers::tracing=info"));
    assert!(joined.contains("edgecrab::harness=info"));
}

#[test]
fn llm_hook_json_round_trip_correlation_fields() {
    let correlation = LlmCorrelation {
        session_id: "obs-e2e-session",
        api_call_count: 7,
        attempt: 2,
        platform: Platform::Api,
    };

    let pre = llm_pre_hook_json(&LlmRequestStart {
        correlation,
        provider: "bedrock",
        model: "nvidia.nemotron-nano-9b-v2",
        streaming: false,
        tool_count: 107,
        prompt_tokens_estimated: 20_000,
        context_length: 128_000,
        http_timeout_secs: 600,
        tool_choice_required: false,
        max_tokens: None,
        reasoning_effort: None,
    });
    assert_eq!(pre["session_id"], "obs-e2e-session");
    assert_eq!(pre["api_call_count"], 7);
    assert_eq!(pre["attempt"], 2);
    assert_eq!(pre["platform"], "api");

    let response = LLMResponse {
        content: String::new(),
        prompt_tokens: 512,
        completion_tokens: 64,
        total_tokens: 576,
        model: "nvidia.nemotron-nano-9b-v2".into(),
        finish_reason: Some("tool_calls".into()),
        tool_calls: vec![],
        metadata: std::collections::HashMap::new(),
        cache_hit_tokens: None,
        cache_write_tokens: None,
        thinking_tokens: None,
        thinking_content: None,
        refusal: None,
    };
    let post = llm_post_hook_json(
        correlation,
        "bedrock",
        "nvidia.nemotron-nano-9b-v2",
        false,
        98_500,
        &response,
    );
    assert_eq!(post["session_id"], "obs-e2e-session");
    assert_eq!(post["elapsed_ms"], 98_500);
    assert_eq!(post["finish_reason"], "tool_calls");
    assert_eq!(post["tool_call_count"], 0);
}

#[test]
fn apply_runtime_from_config_sets_trace_llm_off() {
    unsafe {
        std::env::remove_var("EDGECRAB_TRACE_LLM");
    }
    apply_runtime_from_config(
        &ObservabilityConfig {
            trace_llm: false,
            ..ObservabilityConfig::default()
        },
        "edgecrab",
    );
    assert_eq!(std::env::var("EDGECRAB_TRACE_LLM").ok().as_deref(), Some("0"));
    unsafe {
        std::env::remove_var("EDGECRAB_TRACE_LLM");
    }
}

#[test]
fn agent_conversation_span_constructible() {
    let _span = agent_conversation_span("obs-root", Platform::Cli);
}
