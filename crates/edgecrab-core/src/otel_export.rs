//! Optional OpenTelemetry OTLP export (shared by CLI, gateway, ACP).
//!
//! Activated when `OTEL_EXPORTER_OTLP_ENDPOINT` is set (standard OTel env).

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::trace::TracerProvider;

/// Holds SDK providers until process exit (flushes telemetry on drop).
pub struct OtelGuard {
    trace_provider: Option<TracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
}

impl OtelGuard {
    pub fn empty() -> Self {
        Self {
            trace_provider: None,
            meter_provider: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.trace_provider.is_some() || self.meter_provider.is_some()
    }

    pub fn traces_active(&self) -> bool {
        self.trace_provider.is_some()
    }

    pub fn metrics_active(&self) -> bool {
        self.meter_provider.is_some()
    }
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.meter_provider.take()
            && let Err(err) = provider.shutdown()
        {
            eprintln!("warning: OpenTelemetry metrics shutdown failed: {err}");
        }
        if let Some(provider) = self.trace_provider.take()
            && let Err(err) = provider.shutdown()
        {
            eprintln!("warning: OpenTelemetry trace shutdown failed: {err}");
        }
    }
}

/// Returns `true` when any OTLP export (traces and/or metrics) is configured.
pub fn otel_export_enabled() -> bool {
    std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .is_some_and(|endpoint| !endpoint.trim().is_empty())
        || std::env::var("EDGECRAB_OTEL_EXPORT")
            .ok()
            .is_some_and(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "on"
                )
            })
}

fn otel_signal_enabled(env_key: &str) -> bool {
    match std::env::var(env_key) {
        Ok(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            normalized != "0" && normalized != "false" && normalized != "off"
        }
        Err(_) => true,
    }
}

/// Returns `true` when OTLP trace spans should be exported.
pub fn otel_traces_enabled() -> bool {
    otel_export_enabled() && otel_signal_enabled("EDGECRAB_OTEL_TRACES")
}

/// Returns `true` when OTLP metrics should be exported.
pub fn otel_metrics_enabled() -> bool {
    otel_export_enabled() && otel_signal_enabled("EDGECRAB_OTEL_METRICS")
}

fn otlp_endpoint() -> String {
    std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "http://localhost:4317".into())
}

fn otlp_service_name() -> String {
    std::env::var("OTEL_SERVICE_NAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "edgecrab".into())
}

fn build_meter_provider(endpoint: &str) -> Result<SdkMeterProvider, String> {
    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint.to_string())
        .build()
        .map_err(|err| err.to_string())?;
    let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(
        exporter,
        opentelemetry_sdk::runtime::Tokio,
    )
    .build();
    Ok(opentelemetry_sdk::metrics::SdkMeterProvider::builder()
        .with_reader(reader)
        .build())
}

fn build_trace_provider(endpoint: &str) -> Result<TracerProvider, String> {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint.to_string())
        .build()
        .map_err(|err| err.to_string())?;
    Ok(TracerProvider::builder()
        .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
        .build())
}

fn init_meter_provider(endpoint: &str, service_name: &str) -> Option<SdkMeterProvider> {
    match build_meter_provider(endpoint) {
        Ok(provider) => {
            crate::otel_metrics::install(&provider, service_name);
            Some(provider)
        }
        Err(err) => {
            eprintln!("warning: failed to build OTLP metrics exporter for {endpoint}: {err}");
            None
        }
    }
}

fn init_trace_provider(endpoint: &str) -> Option<TracerProvider> {
    match build_trace_provider(endpoint) {
        Ok(provider) => Some(provider),
        Err(err) => {
            eprintln!("warning: failed to build OTLP trace exporter for {endpoint}: {err}");
            None
        }
    }
}

/// Build an OpenTelemetry bridge layer when trace export is enabled.
///
/// Metrics initialize independently — traces may be off while metrics stay on.
pub fn maybe_otel_layer<S>() -> (
    Option<tracing_opentelemetry::OpenTelemetryLayer<S, opentelemetry_sdk::trace::Tracer>>,
    OtelGuard,
)
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    if !otel_export_enabled() {
        return (None, OtelGuard::empty());
    }

    if tokio::runtime::Handle::try_current().is_err() {
        eprintln!("warning: OpenTelemetry OTLP export requires an active Tokio runtime; skipping");
        return (None, OtelGuard::empty());
    }

    if !otel_traces_enabled() && !otel_metrics_enabled() {
        return (None, OtelGuard::empty());
    }

    let endpoint = otlp_endpoint();
    let service_name = otlp_service_name();

    if !collector_reachable_sync(&endpoint) {
        eprintln!(
            "warning: OpenTelemetry collector unreachable at {endpoint}; skipping OTLP export"
        );
        return (None, OtelGuard::empty());
    }

    let trace_provider = if otel_traces_enabled() {
        init_trace_provider(&endpoint)
    } else {
        None
    };

    let meter_provider = if otel_metrics_enabled() {
        init_meter_provider(&endpoint, &service_name)
    } else {
        None
    };

    if trace_provider.is_none() && meter_provider.is_none() {
        return (None, OtelGuard::empty());
    }

    let trace_layer = trace_provider.as_ref().map(|provider| {
        let tracer = provider.tracer(service_name.clone());
        tracing_opentelemetry::layer().with_tracer(tracer)
    });

    (
        trace_layer,
        OtelGuard {
            trace_provider,
            meter_provider,
        },
    )
}

/// Probe whether an OTLP gRPC collector accepts connections on the given host/port.
pub async fn collector_reachable(endpoint: &str) -> bool {
    let host_port = endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("localhost:4317");
    tokio::time::timeout(
        std::time::Duration::from_millis(250),
        tokio::net::TcpStream::connect(host_port),
    )
    .await
    .is_ok_and(|result| result.is_ok())
}

fn collector_reachable_sync(endpoint: &str) -> bool {
    use std::net::ToSocketAddrs;
    let host_port = endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or("localhost:4317");
    if let Ok(mut addrs) = host_port.to_socket_addrs()
        && let Some(addr) = addrs.next()
    {
        return std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(250))
            .is_ok();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear_otel_env() {
        unsafe {
            std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
            std::env::remove_var("EDGECRAB_OTEL_EXPORT");
            std::env::remove_var("EDGECRAB_OTEL_METRICS");
            std::env::remove_var("EDGECRAB_OTEL_TRACES");
        }
    }

    #[test]
    fn otel_signal_env_semantics() {
        clear_otel_env();
        assert!(!otel_export_enabled());
        assert!(!otel_traces_enabled());
        assert!(!otel_metrics_enabled());

        unsafe { std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4317") };
        assert!(otel_export_enabled());
        assert!(otel_traces_enabled());
        assert!(otel_metrics_enabled());

        unsafe { std::env::set_var("EDGECRAB_OTEL_METRICS", "0") };
        assert!(otel_traces_enabled());
        assert!(!otel_metrics_enabled());

        unsafe {
            std::env::remove_var("EDGECRAB_OTEL_METRICS");
            std::env::set_var("EDGECRAB_OTEL_TRACES", "0");
        }
        assert!(!otel_traces_enabled());
        assert!(otel_metrics_enabled());

        clear_otel_env();
    }
}
