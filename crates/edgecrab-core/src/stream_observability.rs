//! Tool execution observability — structured facts and spans for harness tools.
//!
//! Complements edgequake GenAI spans (LLM) with tool lifecycle spans (OTel-shaped fields).

use crate::observability::TARGET_HARNESS;
use edgecrab_tools::CapabilityGrants;
use edgecrab_types::Platform;

/// Span for a single tool dispatch (pairs with [`log_tool_done`]).
pub fn tool_execution_span(
    tool_call_id: &str,
    tool_name: &str,
    session_id: &str,
    platform: Platform,
    capability_grants: Option<CapabilityGrants>,
) -> tracing::Span {
    tracing::info!(
        target: TARGET_HARNESS,
        tool_call_id,
        tool_name,
        session_id,
        platform = %platform,
        capability_file = capability_grants.map(|grants| grants.file),
        capability_net = capability_grants.map(|grants| grants.net),
        capability_mcp = capability_grants.map(|grants| grants.mcp),
        "harness: tool start"
    );
    tracing::info_span!(
        target: TARGET_HARNESS,
        "tool_execution",
        tool_call_id,
        tool_name,
        session_id,
        platform = %platform,
        capability_file = capability_grants.map(|grants| grants.file),
        capability_net = capability_grants.map(|grants| grants.net),
        capability_mcp = capability_grants.map(|grants| grants.mcp),
    )
}

/// Log tool dispatch start (legacy structured log; span also emitted by [`tool_execution_span`]).
pub fn log_tool_start(tool_call_id: &str, name: &str) {
    tracing::info!(
        target: TARGET_HARNESS,
        tool_call_id,
        tool_name = name,
        "harness: tool start"
    );
}

/// Log tool dispatch completion.
pub fn log_tool_done(tool_call_id: &str, name: &str, duration_ms: u64, is_error: bool) {
    crate::observability::record_tool_operation(name, duration_ms, is_error);
    tracing::info!(
        target: TARGET_HARNESS,
        tool_call_id,
        tool_name = name,
        tool_duration_ms = duration_ms,
        tool_is_error = is_error,
        "harness: tool complete"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_execution_span_constructible() {
        let _span = tool_execution_span("call-1", "read_file", "sess-a", Platform::Cli, None);
    }
}
