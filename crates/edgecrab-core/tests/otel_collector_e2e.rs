//! OTLP collector integration tests.
//!
//! Auto-skip when no collector is listening. For a full export smoke test:
//!
//! ```bash
//! docker compose -f infra/otel/docker-compose.yaml up -d
//! cargo test -p edgecrab-core --test otel_collector_e2e -- --ignored
//! ```

use std::time::Duration;

use edgecrab_core::{
    ObservabilityConfig, apply_runtime_from_config, collector_reachable, maybe_otel_layer,
    observability::{
        LlmCorrelation, agent_conversation_span, record_llm_operation, record_tool_operation,
    },
    otel_export_enabled, otel_metrics_enabled, otel_traces_enabled,
};
use edgecrab_types::Platform;
use tracing::Instrument;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const COLLECTOR_ENDPOINT: &str = "http://localhost:4317";

async fn setup_otel_subscriber() -> edgecrab_core::OtelGuard {
    apply_runtime_from_config(
        &ObservabilityConfig {
            otel_export: true,
            otel_endpoint: Some(COLLECTOR_ENDPOINT.into()),
            service_name: Some("edgecrab-otel-e2e".into()),
            trace_llm: true,
            capture_content: false,
            otel_metrics: true,
            otel_traces: true,
        },
        "edgecrab",
    );

    let (otel_layer, guard) = maybe_otel_layer();
    let otel_layer = otel_layer.expect("OTLP layer should initialize under Tokio test runtime");
    let _ = tracing_subscriber::registry().with(otel_layer).try_init();
    guard
}

#[tokio::test]
async fn otel_collector_auto_skips_when_unreachable() {
    if collector_reachable(COLLECTOR_ENDPOINT).await {
        eprintln!(
            "collector reachable — run `cargo test -p edgecrab-core --test otel_collector_e2e -- --ignored` for full smoke test"
        );
        return;
    }
    record_tool_operation("read_file", 12, false);
    record_llm_operation(
        LlmCorrelation {
            session_id: "offline",
            api_call_count: 1,
            attempt: 0,
            platform: Platform::Cli,
        },
        "mock",
        "mock-model",
        "chat",
        5,
        true,
        None,
    );
}

#[tokio::test]
#[ignore = "requires OTLP collector: docker compose -f infra/otel/docker-compose.yaml up -d"]
async fn otel_layer_exports_spans_and_metrics() {
    if !collector_reachable(COLLECTOR_ENDPOINT).await {
        panic!(
            "OTLP collector not reachable at {COLLECTOR_ENDPOINT}; start infra/otel/docker-compose.yaml"
        );
    }

    let guard = setup_otel_subscriber().await;
    assert!(guard.is_active());
    assert!(guard.metrics_active());

    async {
        record_llm_operation(
            LlmCorrelation {
                session_id: "otel-e2e-session",
                api_call_count: 1,
                attempt: 0,
                platform: Platform::Cli,
            },
            "mock",
            "mock-model",
            "chat_with_tools",
            120,
            true,
            None,
        );
        record_tool_operation("read_file", 45, false);
        tracing::info!("otel collector e2e probe");
    }
    .instrument(agent_conversation_span("otel-e2e-session", Platform::Cli))
    .await;

    tokio::time::sleep(Duration::from_secs(2)).await;
}

#[tokio::test]
async fn otel_traces_and_metrics_toggle_independently() {
    unsafe {
        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4317");
        std::env::remove_var("EDGECRAB_OTEL_EXPORT");
        std::env::set_var("EDGECRAB_OTEL_TRACES", "0");
        std::env::remove_var("EDGECRAB_OTEL_METRICS");
    }

    assert!(otel_export_enabled());
    assert!(!otel_traces_enabled());
    assert!(otel_metrics_enabled());

    unsafe {
        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
        std::env::remove_var("EDGECRAB_OTEL_TRACES");
        std::env::remove_var("EDGECRAB_OTEL_METRICS");
    }
}
