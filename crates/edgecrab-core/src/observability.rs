//! Structured observability for the agent harness (turn lifecycle, hooks).
//!
//! LLM HTTP spans are emitted by edgequake-llm [`trace_llm_arc`] at the provider
//! factory — not duplicated here (DRY).

use crate::config::ObservabilityConfig;

/// Tracing target for harness turn lifecycle (completion, gates, loop boundaries).
pub const TARGET_HARNESS: &str = "edgecrab::harness";

/// Tracing target for local inference plan/detail (see `local_provider_policy`).
pub const TARGET_LOCAL_LLM: &str = "edgecrab::local_llm";

/// Tracing target for long-blocking provider waits (timeout heartbeats).
pub const TARGET_PROVIDER_LLM: &str = "edgecrab::provider_llm";

/// GenAI span module from edgequake-llm (visible at `logging.level: warn`).
pub const TARGET_GENAI_SPANS: &str = "edgequake_llm::providers::tracing";

/// Directives merged into the runtime filter so harness + GenAI spans stay visible
/// when `config.logging.level` is `warn` (homelab default).
pub const OBSERVABILITY_FILTER_DIRECTIVES: &[&str] = &[
    "edgecrab::harness=info",
    "edgecrab::local_llm=info",
    "edgecrab::provider_llm=info",
    "edgequake_llm::providers::tracing=info",
];

/// Apply resolved observability config to process env before subscriber init.
///
/// Existing env vars win (layered config: env > yaml > defaults).
pub fn apply_runtime_from_config(config: &ObservabilityConfig, default_service_name: &str) {
    set_env_if_absent(
        "EDGECRAB_TRACE_LLM",
        if config.trace_llm { "1" } else { "0" },
    );
    if config.otel_export {
        if let Some(endpoint) = config
            .otel_endpoint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            set_env_if_absent("OTEL_EXPORTER_OTLP_ENDPOINT", endpoint);
        } else {
            set_env_if_absent("EDGECRAB_OTEL_EXPORT", "1");
        }
    }
    let service_name = config
        .service_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_service_name);
    set_env_if_absent("OTEL_SERVICE_NAME", service_name);
    if config.capture_content {
        set_env_if_absent("EDGECODE_CAPTURE_CONTENT", "1");
    }
    if !config.otel_metrics {
        set_env_if_absent("EDGECRAB_OTEL_METRICS", "0");
    }
    if !config.otel_traces {
        set_env_if_absent("EDGECRAB_OTEL_TRACES", "0");
    }
}

/// Load `config.yaml` and apply observability env before logging/OTLP subscriber init.
pub fn load_app_config_and_apply_observability(default_service_name: &str) -> crate::AppConfig {
    let config = crate::AppConfig::load().unwrap_or_default();
    apply_runtime_from_config(&config.observability, default_service_name);
    config
}

fn set_env_if_absent(key: &str, value: &str) {
    if std::env::var(key).is_err() {
        // SAFETY: called once at process startup before other threads read these vars.
        unsafe { std::env::set_var(key, value) };
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LlmCorrelation<'a> {
    pub session_id: &'a str,
    pub api_call_count: u32,
    pub attempt: u32,
    pub platform: edgecrab_types::Platform,
}

#[derive(Debug, Clone, Copy)]
pub struct LlmRequestStart<'a> {
    pub correlation: LlmCorrelation<'a>,
    pub provider: &'a str,
    pub model: &'a str,
    pub streaming: bool,
    pub tool_count: usize,
    pub prompt_tokens_estimated: usize,
    pub context_length: usize,
    pub http_timeout_secs: u64,
    pub tool_choice_required: bool,
    pub max_tokens: Option<usize>,
    pub reasoning_effort: Option<&'a str>,
}

/// Parent span for one user turn (wraps `execute_loop` / streaming entry).
pub fn agent_conversation_span(
    session_id: &str,
    platform: edgecrab_types::Platform,
) -> tracing::Span {
    tracing::info_span!(
        target: TARGET_HARNESS,
        "agent_conversation",
        session_id,
        platform = %platform,
    )
}

/// Parent span for one LLM HTTP attempt (child: edgequake `gen_ai.*` spans).
pub fn llm_attempt_span(start: &LlmRequestStart<'_>) -> tracing::Span {
    let LlmRequestStart {
        correlation,
        provider,
        model,
        streaming,
        tool_count,
        prompt_tokens_estimated,
        context_length,
        http_timeout_secs,
        tool_choice_required,
        max_tokens,
        reasoning_effort,
    } = start;

    tracing::info_span!(
        target: TARGET_HARNESS,
        "harness.llm_attempt",
        session_id = correlation.session_id,
        api_call_count = correlation.api_call_count,
        attempt = correlation.attempt,
        platform = %correlation.platform,
        provider,
        model,
        streaming,
        tool_count,
        prompt_tokens_estimated,
        context_length,
        http_timeout_secs,
        tool_choice_required,
        max_tokens,
        reasoning_effort,
    )
}

/// Machine-readable `llm:pre` hook payload (DRY with harness attempt span fields).
pub fn llm_pre_hook_json(start: &LlmRequestStart<'_>) -> serde_json::Value {
    let LlmRequestStart {
        correlation,
        provider,
        model,
        streaming,
        tool_count,
        prompt_tokens_estimated,
        context_length,
        http_timeout_secs,
        ..
    } = start;

    serde_json::json!({
        "event": "llm:pre",
        "session_id": correlation.session_id,
        "api_call_count": correlation.api_call_count,
        "attempt": correlation.attempt,
        "platform": correlation.platform.to_string(),
        "provider": provider,
        "model": model,
        "native_tool_streaming": streaming,
        "tool_count": tool_count,
        "prompt_tokens_estimated": prompt_tokens_estimated,
        "context_length": context_length,
        "http_timeout_secs": http_timeout_secs,
    })
}

/// Machine-readable `llm:post` hook payload.
pub fn llm_post_hook_json(
    correlation: LlmCorrelation<'_>,
    provider: &str,
    model: &str,
    streaming: bool,
    elapsed_ms: u64,
    response: &edgequake_llm::LLMResponse,
) -> serde_json::Value {
    serde_json::json!({
        "event": "llm:post",
        "session_id": correlation.session_id,
        "api_call_count": correlation.api_call_count,
        "attempt": correlation.attempt,
        "platform": correlation.platform.to_string(),
        "provider": provider,
        "model": model,
        "native_tool_streaming": streaming,
        "elapsed_ms": elapsed_ms,
        "finish_reason": response.finish_reason,
        "prompt_tokens": response.prompt_tokens,
        "completion_tokens": response.completion_tokens,
        "thinking_tokens": response.thinking_tokens,
        "tool_call_count": response.tool_calls.len(),
        "content_len": response.content.len(),
    })
}

/// Record OTEL metrics for a completed LLM operation (no-op when OTLP metrics inactive).
pub fn record_llm_operation(
    correlation: LlmCorrelation<'_>,
    provider: &str,
    model: &str,
    operation: &str,
    elapsed_ms: u64,
    success: bool,
    response: Option<&edgequake_llm::LLMResponse>,
) {
    let (prompt_tokens, completion_tokens) = response
        .map(|r| (r.prompt_tokens, r.completion_tokens))
        .unwrap_or((0, 0));
    crate::otel_metrics::record_llm_operation(crate::otel_metrics::LlmOperationMetrics {
        provider,
        model,
        platform: &correlation.platform.to_string(),
        operation,
        elapsed_ms,
        success,
        prompt_tokens,
        completion_tokens,
    });
}

/// Record OTEL metrics for a completed tool dispatch (no-op when OTLP metrics inactive).
pub fn record_tool_operation(tool_name: &str, duration_ms: u64, is_error: bool) {
    crate::otel_metrics::record_tool_operation(tool_name, duration_ms, is_error);
}

/// Log harness completion assessment (deterministic outcome, not model prose).
pub fn log_harness_completion(
    session_id: &str,
    platform: &str,
    decision: &str,
    exit_reason: &str,
    harness_blocked: bool,
    oracle_failures: usize,
    unresolved_mutations: usize,
) {
    tracing::info!(
        target: TARGET_HARNESS,
        session_id,
        platform,
        decision,
        exit_reason,
        harness_blocked,
        oracle_failures,
        unresolved_mutations,
        "harness: turn complete"
    );
    crate::otel_metrics::record_harness_turn(platform, decision, harness_blocked);
}

/// Log when the ReAct loop begins an API iteration.
pub fn log_harness_api_iteration(
    session_id: &str,
    api_call_count: u32,
    provider: &str,
    model: &str,
    tool_count: usize,
    message_count: usize,
) {
    tracing::info!(
        target: TARGET_HARNESS,
        session_id,
        api_call_count,
        provider,
        model,
        tool_count,
        message_count,
        "harness: api iteration"
    );
}

/// Record LLM attempt failure on the harness attempt span.
pub fn log_llm_attempt_failure(
    correlation: LlmCorrelation<'_>,
    provider: &str,
    model: &str,
    streaming: bool,
    elapsed_ms: u64,
    error: &str,
) {
    let operation = if streaming {
        "chat_with_tools_stream"
    } else {
        "chat_with_tools"
    };
    record_llm_operation(
        correlation,
        provider,
        model,
        operation,
        elapsed_ms,
        false,
        None,
    );
    tracing::warn!(
        target: TARGET_HARNESS,
        session_id = correlation.session_id,
        api_call_count = correlation.api_call_count,
        attempt = correlation.attempt,
        platform = %correlation.platform,
        provider,
        model,
        streaming,
        elapsed_ms,
        error,
        "harness: llm attempt failed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_types::Platform;

    #[test]
    fn llm_pre_hook_json_includes_correlation_fields() {
        let json = llm_pre_hook_json(&LlmRequestStart {
            correlation: LlmCorrelation {
                session_id: "sess-1",
                api_call_count: 2,
                attempt: 0,
                platform: Platform::Cli,
            },
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
        assert_eq!(json["session_id"], "sess-1");
        assert_eq!(json["api_call_count"], 2);
        assert_eq!(json["provider"], "bedrock");
        assert_eq!(json["tool_count"], 107);
        assert_eq!(json["native_tool_streaming"], false);
    }

    #[test]
    fn llm_post_hook_json_includes_response_metrics() {
        let response = edgequake_llm::LLMResponse {
            content: "ok".into(),
            prompt_tokens: 100,
            completion_tokens: 20,
            total_tokens: 120,
            model: "test-model".into(),
            finish_reason: Some("stop".into()),
            tool_calls: vec![],
            metadata: std::collections::HashMap::new(),
            cache_hit_tokens: None,
            cache_write_tokens: None,
            thinking_tokens: None,
            thinking_content: None,
            refusal: None,
        };
        let json = llm_post_hook_json(
            LlmCorrelation {
                session_id: "sess-2",
                api_call_count: 3,
                attempt: 1,
                platform: Platform::Cli,
            },
            "bedrock",
            "nvidia.nemotron-nano-9b-v2",
            false,
            42_000,
            &response,
        );
        assert_eq!(json["elapsed_ms"], 42_000);
        assert_eq!(json["tool_call_count"], 0);
        assert_eq!(json["finish_reason"], "stop");
        assert_eq!(json["prompt_tokens"], 100);
    }

    #[test]
    fn observability_directives_use_target_level_pairs() {
        for directive in OBSERVABILITY_FILTER_DIRECTIVES {
            let (target, level) = directive
                .split_once('=')
                .expect("directive must be target=level");
            assert!(target.contains("edgecrab::") || target.contains("edgequake_llm::"));
            assert!(!level.is_empty());
        }
    }

    #[test]
    fn apply_runtime_from_config_env_semantics() {
        unsafe {
            std::env::remove_var("EDGECRAB_TRACE_LLM");
            std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
            std::env::remove_var("EDGECRAB_OTEL_EXPORT");
            std::env::remove_var("OTEL_SERVICE_NAME");
            std::env::remove_var("EDGECODE_CAPTURE_CONTENT");
        }
        apply_runtime_from_config(
            &ObservabilityConfig {
                otel_export: true,
                otel_endpoint: Some("http://collector:4317".into()),
                service_name: None,
                trace_llm: false,
                capture_content: true,
                otel_metrics: true,
                otel_traces: true,
            },
            "edgecrab-gateway",
        );
        assert_eq!(
            std::env::var("EDGECRAB_TRACE_LLM").ok().as_deref(),
            Some("0")
        );
        assert_eq!(
            std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok().as_deref(),
            Some("http://collector:4317")
        );
        assert_eq!(
            std::env::var("OTEL_SERVICE_NAME").ok().as_deref(),
            Some("edgecrab-gateway")
        );
        assert_eq!(
            std::env::var("EDGECODE_CAPTURE_CONTENT").ok().as_deref(),
            Some("1")
        );

        unsafe {
            std::env::set_var("OTEL_SERVICE_NAME", "custom-service");
        }
        apply_runtime_from_config(&ObservabilityConfig::default(), "edgecrab");
        assert_eq!(
            std::env::var("OTEL_SERVICE_NAME").ok().as_deref(),
            Some("custom-service")
        );

        unsafe {
            std::env::remove_var("EDGECRAB_TRACE_LLM");
            std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
            std::env::remove_var("OTEL_SERVICE_NAME");
            std::env::remove_var("EDGECODE_CAPTURE_CONTENT");
        }
    }
}
