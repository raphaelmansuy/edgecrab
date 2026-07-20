//! OpenTelemetry metrics for harness LLM and tool operations (GenAI semconv names).
//!
//! Instruments are installed when OTLP export initializes ([`crate::otel_export::maybe_otel_layer`]).

use std::sync::OnceLock;

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, MeterProvider};
use opentelemetry_sdk::metrics::SdkMeterProvider;

static METRICS: OnceLock<Option<HarnessMetrics>> = OnceLock::new();

struct HarnessMetrics {
    llm_duration_ms: Histogram<f64>,
    llm_tokens: Counter<u64>,
    tool_duration_ms: Histogram<f64>,
    tool_errors: Counter<u64>,
    harness_turns: Counter<u64>,
    turn_phase_transitions: Counter<u64>,
}

/// Whether OTLP metrics instruments are active in this process.
pub fn metrics_active() -> bool {
    METRICS.get().is_some_and(|m| m.is_some())
}

/// Install GenAI/harness metric instruments (idempotent — first call wins).
pub fn install(meter_provider: &SdkMeterProvider, _service_name: &str) {
    if METRICS.get().is_some() {
        return;
    }
    let meter = meter_provider.meter("edgecrab.harness");
    let metrics = HarnessMetrics {
        llm_duration_ms: meter
            .f64_histogram("gen_ai.client.operation.duration")
            .with_description("Duration of LLM client operations")
            .with_unit("ms")
            .build(),
        llm_tokens: meter
            .u64_counter("gen_ai.client.token.usage")
            .with_description("LLM token usage")
            .with_unit("{token}")
            .build(),
        tool_duration_ms: meter
            .f64_histogram("edgecrab.tool.duration")
            .with_description("Tool execution duration")
            .with_unit("ms")
            .build(),
        tool_errors: meter
            .u64_counter("edgecrab.tool.errors")
            .with_description("Tool execution errors")
            .with_unit("{error}")
            .build(),
        harness_turns: meter
            .u64_counter("edgecrab.harness.turns")
            .with_description("Completed agent conversation turns")
            .with_unit("{turn}")
            .build(),
        turn_phase_transitions: meter
            .u64_counter("edgecrab.turn.phase.transitions")
            .with_description("Agent turn lifecycle phase transitions")
            .with_unit("{transition}")
            .build(),
    };
    let _ = METRICS.set(Some(metrics));
}

pub struct LlmOperationMetrics<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub platform: &'a str,
    pub operation: &'a str,
    pub elapsed_ms: u64,
    pub success: bool,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
}

pub fn record_llm_operation(metrics: LlmOperationMetrics<'_>) {
    let Some(inst) = METRICS.get().and_then(|m| m.as_ref()) else {
        return;
    };
    let LlmOperationMetrics {
        provider,
        model,
        platform,
        operation,
        elapsed_ms,
        success,
        prompt_tokens,
        completion_tokens,
    } = metrics;
    let base = [
        KeyValue::new("gen_ai.system", provider.to_string()),
        KeyValue::new("gen_ai.request.model", model.to_string()),
        KeyValue::new("gen_ai.operation.name", operation.to_string()),
        KeyValue::new("edgecrab.platform", platform.to_string()),
        KeyValue::new("edgecrab.success", success),
    ];
    inst.llm_duration_ms.record(elapsed_ms as f64, &base);
    if prompt_tokens > 0 {
        let mut attrs = base.to_vec();
        attrs.push(KeyValue::new("gen_ai.token.type", "input"));
        inst.llm_tokens
            .add(prompt_tokens.min(u64::MAX as usize) as u64, &attrs);
    }
    if completion_tokens > 0 {
        let mut attrs = base.to_vec();
        attrs.push(KeyValue::new("gen_ai.token.type", "output"));
        inst.llm_tokens
            .add(completion_tokens.min(u64::MAX as usize) as u64, &attrs);
    }
}

pub fn record_tool_operation(tool_name: &str, duration_ms: u64, is_error: bool) {
    let Some(metrics) = METRICS.get().and_then(|m| m.as_ref()) else {
        return;
    };
    let attrs = [KeyValue::new("edgecrab.tool.name", tool_name.to_string())];
    metrics.tool_duration_ms.record(duration_ms as f64, &attrs);
    if is_error {
        metrics.tool_errors.add(1, &attrs);
    }
}

pub fn record_harness_turn(platform: &str, decision: &str, harness_blocked: bool) {
    let Some(metrics) = METRICS.get().and_then(|m| m.as_ref()) else {
        return;
    };
    let attrs = [
        KeyValue::new("edgecrab.platform", platform.to_string()),
        KeyValue::new("edgecrab.decision", decision.to_string()),
        KeyValue::new("edgecrab.harness_blocked", harness_blocked),
    ];
    metrics.harness_turns.add(1, &attrs);
}

/// Record a typed turn lifecycle transition (no-op when OTLP metrics are inactive).
pub fn record_turn_phase_transition(from: &str, to: &str) {
    let Some(metrics) = METRICS.get().and_then(|m| m.as_ref()) else {
        return;
    };
    let attrs = [
        KeyValue::new("edgecrab.turn.phase.from", from.to_string()),
        KeyValue::new("edgecrab.turn.phase.to", to.to_string()),
    ];
    metrics.turn_phase_transitions.add(1, &attrs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_llm_operation_noops_without_install() {
        record_llm_operation(LlmOperationMetrics {
            provider: "mock",
            model: "mock-model",
            platform: "cli",
            operation: "chat",
            elapsed_ms: 42,
            success: true,
            prompt_tokens: 10,
            completion_tokens: 5,
        });
        assert!(!metrics_active());
    }

    #[test]
    fn record_harness_turn_noops_without_install() {
        record_harness_turn("cli", "completed", false);
        assert!(!metrics_active());
    }
}
