//! LLM provider call path — streaming, retry, Hermes API hooks (spec 015 P2.1).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use edgecrab_plugins::{hermes_supports_hook, invoke_hermes_hook};
use edgecrab_types::{AgentError, Message};
use edgequake_llm::traits::{StreamChunk, StreamUsage};
use edgequake_llm::{LLMProvider, ToolDefinition};
use futures::StreamExt;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::progress_sink::emit_activity;

const BASE_BACKOFF: Duration = Duration::from_millis(500);
#[cfg(test)]
const STREAM_FIRST_CHUNK_TIMEOUT: Duration = Duration::from_millis(50);
#[cfg(not(test))]
const STREAM_FIRST_CHUNK_TIMEOUT: Duration = Duration::from_secs(20);

/// Copilot opens SSE quickly (`StreamChunk::Connected`) but often needs 30–120s
/// before the first tool-call delta on large prompts. Without a post-connect
/// budget we hit `STREAM_FIRST_CHUNK_TIMEOUT` and fall back to non-streaming,
/// which frequently returns empty `choices`.
#[cfg(test)]
const STREAM_POST_CONNECT_FIRST_BYTE_TIMEOUT: Duration = Duration::from_millis(200);
#[cfg(not(test))]
const STREAM_POST_CONNECT_FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(90);

/// Deadline for the first user-visible chunk (content, reasoning, or tool delta).
fn stream_first_byte_timeout(
    provider_name: &str,
    has_tools: bool,
    transport_connected: bool,
) -> Duration {
    // Copilot (and xAI SuperGrok/Grok reasoning) can open SSE quickly then spend
    // a long time prefill+reasoning before the first tool/content delta.
    // A tight 20s first-byte budget feels like a hang and forces non-stream fallback.
    if transport_connected && has_tools && matches!(provider_name, "vscode-copilot" | "xai") {
        STREAM_POST_CONNECT_FIRST_BYTE_TIMEOUT
    } else if has_tools && provider_name == "xai" {
        // Pre-connect budget for xAI with tools: still generous (reasoning models).
        STREAM_POST_CONNECT_FIRST_BYTE_TIMEOUT
    } else {
        STREAM_FIRST_CHUNK_TIMEOUT
    }
}

fn stream_first_byte_deadline(
    provider: &dyn LLMProvider,
    has_tools: bool,
    transport_connected: bool,
) -> tokio::time::Instant {
    tokio::time::Instant::now()
        + stream_first_byte_timeout(provider.name(), has_tools, transport_connected)
}

/// Maximum time between consecutive stream chunks before declaring the stream
/// stale. Providers can silently hang mid-stream (FP10: Stale-Stream Detect).
/// In tests this is set very short; in production 60s covers provider variance.
#[cfg(test)]
const STREAM_INTER_CHUNK_TIMEOUT: Duration = Duration::from_millis(50);
#[cfg(not(test))]
const STREAM_INTER_CHUNK_TIMEOUT: Duration = Duration::from_secs(60);

/// Emit a shelf notice when a provider names a tool but stops sending arg deltas.
const TOOL_ARGS_STALL_NOTICE_SECS: u64 =
    edgecrab_tools::tool_progress_tail::TOOL_ARGS_STALL_NOTICE_SECS;

/// Abort streamed tool drafting when args stall — independent of thinking keepalive.
const TOOL_ARGS_STALL_BREAK_SECS: u64 =
    edgecrab_tools::tool_progress_tail::TOOL_ARGS_STALL_BREAK_SECS;

/// Finish reason when a streamed tool draft stalls or times out mid-chunk.
pub const FINISH_REASON_STREAM_INTERRUPTED: &str = "stream_interrupted";

/// User-visible assistant text: `content` when present, else OpenAI `refusal`.
pub fn assistant_display_text(response: &edgequake_llm::LLMResponse) -> String {
    if !response.content.trim().is_empty() {
        return response.content.clone();
    }
    response.refusal.clone().unwrap_or_default()
}

/// Whether the model produced anything worth keeping (not an empty nudge candidate).
pub fn response_has_visible_output(response: &edgequake_llm::LLMResponse) -> bool {
    !response.content.trim().is_empty()
        || response
            .refusal
            .as_deref()
            .is_some_and(|r| !r.trim().is_empty())
        || response
            .thinking_content
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty())
        || response.has_tool_calls()
}

pub fn estimate_stream_prompt_tokens(
    messages: &[edgequake_llm::ChatMessage],
    tool_defs: &[ToolDefinition],
) -> usize {
    use edgecrab_types::MULTIMODAL_IMAGE_TOKEN_ESTIMATE;

    let mut total = estimate_tokens_from_json(tool_defs);
    for m in messages {
        total += estimate_tokens_from_text(&m.content);
        if let Some(images) = &m.images {
            total += images.len() * MULTIMODAL_IMAGE_TOKEN_ESTIMATE;
        }
    }
    total
}

/// Prompt mass for compression / context-pressure decisions (includes system + tools).
pub fn estimate_request_prompt_tokens(
    system_prompt: Option<&str>,
    messages: &[Message],
    tool_defs: &[ToolDefinition],
) -> usize {
    let mut total = estimate_tokens_from_json(tool_defs);
    if let Some(sp) = system_prompt {
        total += estimate_tokens_from_text(sp);
    }
    total + crate::compression::estimate_tokens(messages)
}

pub fn estimate_stream_completion_tokens(
    content: &str,
    thinking: Option<&str>,
    tool_calls: &[edgequake_llm::ToolCall],
) -> usize {
    estimate_tokens_from_json(&(content, thinking, tool_calls))
}

pub fn estimate_tokens_from_json<T: serde::Serialize + ?Sized>(value: &T) -> usize {
    let serialized = match serde_json::to_string(value) {
        Ok(serialized) => serialized,
        Err(_) => return 0,
    };
    estimate_tokens_from_text(&serialized)
}

pub fn estimate_tokens_from_text(text: &str) -> usize {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0;
    }
    trimmed.chars().count().div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistant_display_text_prefers_content_then_refusal() {
        let mut response = edgequake_llm::LLMResponse::new("hello", "mock");
        assert_eq!(assistant_display_text(&response), "hello");
        response.content.clear();
        response.refusal = Some("policy decline".into());
        assert_eq!(assistant_display_text(&response), "policy decline");
    }

    #[test]
    fn response_has_visible_output_includes_refusal() {
        let mut response = edgequake_llm::LLMResponse::new("", "mock");
        assert!(!response_has_visible_output(&response));
        response.refusal = Some("no".into());
        assert!(response_has_visible_output(&response));
    }

    #[test]
    fn estimate_request_prompt_tokens_includes_fixed_prompt_mass() {
        let messages = vec![Message::user("hello")];
        let tool_defs = vec![edgequake_llm::ToolDefinition::function(
            "read_file",
            "Read",
            serde_json::json!({"type": "object"}),
        )];
        let bare = estimate_request_prompt_tokens(None, &messages, &[]);
        let inflated = estimate_request_prompt_tokens(Some("system prompt"), &messages, &tool_defs);
        assert!(inflated > bare);
    }

    // ── FP19: parse_retry_after tests ──────────────────────────────────────

    #[test]
    fn parse_retry_after_try_again_in_pattern() {
        let msg = "rate_limit_exceeded: You are sending requests too quickly. Try again in 1.197s.";
        let dur = parse_retry_after(msg).expect("should parse retry-after");
        assert!(dur.as_millis() >= 1397, "should include safety margin");
        assert!(dur.as_millis() < 2000, "should not be wildly over");
    }

    #[test]
    fn parse_retry_after_retry_after_pattern() {
        let msg = "Too Many Requests. Retry after 3s.";
        let dur = parse_retry_after(msg).expect("should parse retry-after");
        assert!(dur.as_millis() >= 3200, "3s + 200ms margin");
        assert!(dur.as_millis() < 4000);
    }

    #[test]
    fn parse_retry_after_please_wait_pattern() {
        let msg = "Please wait 2 seconds before retrying.";
        let dur = parse_retry_after(msg).expect("should parse retry-after");
        assert!(dur.as_millis() >= 2200);
    }

    #[test]
    fn parse_retry_after_returns_none_for_no_hint() {
        let msg = "Internal Server Error: upstream timeout";
        assert!(
            parse_retry_after(msg).is_none(),
            "no retry hint should return None"
        );
    }

    #[test]
    fn parse_retry_after_rejects_zero_wait() {
        let msg = "Try again in 0s.";
        assert!(
            parse_retry_after(msg).is_none(),
            "zero wait is not a valid retry hint"
        );
    }

    #[test]
    fn stream_first_byte_timeout_extends_after_copilot_sse_open() {
        let pre_connect = stream_first_byte_timeout("vscode-copilot", true, false);
        let post_connect = stream_first_byte_timeout("vscode-copilot", true, true);
        assert_eq!(pre_connect, STREAM_FIRST_CHUNK_TIMEOUT);
        assert_eq!(post_connect, STREAM_POST_CONNECT_FIRST_BYTE_TIMEOUT);
        assert!(post_connect > pre_connect);
    }

    #[test]
    fn stream_first_byte_timeout_unchanged_for_other_providers() {
        let timeout = stream_first_byte_timeout("anthropic", true, true);
        assert_eq!(timeout, STREAM_FIRST_CHUNK_TIMEOUT);
    }

    #[test]
    fn stream_first_byte_timeout_extends_for_xai_tool_turns() {
        let pre = stream_first_byte_timeout("xai", true, false);
        let post = stream_first_byte_timeout("xai", true, true);
        assert_eq!(pre, STREAM_POST_CONNECT_FIRST_BYTE_TIMEOUT);
        assert_eq!(post, STREAM_POST_CONNECT_FIRST_BYTE_TIMEOUT);
        let no_tools = stream_first_byte_timeout("xai", false, false);
        assert_eq!(no_tools, STREAM_FIRST_CHUNK_TIMEOUT);
    }

    #[test]
    fn stream_403_is_not_retryable_as_flaky_transport() {
        let err = edgequake_llm::LlmError::NetworkError(
            "Stream error: Invalid status code: 403 Forbidden".into(),
        );
        assert!(is_permanent_auth_or_permission_error(&err));
        assert!(!is_retryable_nonvisible_stream_error(&err));
        let credits = edgequake_llm::LlmError::ApiError(
            r#"permission-denied: used all available credits"#.into(),
        );
        assert!(is_permanent_auth_or_permission_error(&credits));
        assert!(!is_retryable_nonvisible_stream_error(&credits));
    }

    #[test]
    fn parse_retry_after_rejects_unreasonably_large_wait() {
        let msg = "Try again in 999s.";
        assert!(
            parse_retry_after(msg).is_none(),
            "wait > 300s should be rejected as implausible"
        );
    }

    #[test]
    fn streamed_tool_capability_error_detection_is_specific() {
        assert!(is_streamed_tool_capability_error(
            &edgequake_llm::LlmError::InvalidRequest(
                "Tool calling is not supported in streaming mode".into(),
            )
        ));
        assert!(!is_streamed_tool_capability_error(
            &edgequake_llm::LlmError::InvalidRequest("temperature must be <= 2".into())
        ));
    }
}

pub struct ApiCallContext<'a> {
    pub options: Option<&'a edgequake_llm::CompletionOptions>,
    pub cancel: &'a CancellationToken,
    pub event_tx: Option<&'a tokio::sync::mpsc::UnboundedSender<crate::StreamEvent>>,
    pub use_native_streaming: bool,
    pub discovered_plugins: Option<&'a edgecrab_plugins::PluginDiscovery>,
    pub conversation_session_id: &'a str,
    pub platform: edgecrab_types::Platform,
    pub api_call_count: u32,
    /// Session sizing for failover disambiguation (disconnect vs overflow, generic 400, etc.).
    pub session: crate::failover::ClassifyContext,
}

/// Build failover session context from an in-flight API call.
pub fn classify_session_for_call(
    provider: &dyn LLMProvider,
    messages: &[edgequake_llm::ChatMessage],
    approx_tokens: u32,
) -> crate::failover::ClassifyContext {
    crate::failover::ClassifyContext {
        approx_tokens,
        context_length: provider.max_context_length().max(1) as u32,
        num_messages: messages.len() as u32,
    }
}

fn should_abort_api_retries(
    error: &edgequake_llm::LlmError,
    session: crate::failover::ClassifyContext,
) -> bool {
    if crate::multimodal_tool_content::is_tool_message_order_error(&error.to_string()) {
        return true;
    }
    !crate::failover::classify_llm_error(error, session).retryable
}

fn provider_manages_transport_retries(provider: &dyn LLMProvider) -> bool {
    matches!(provider.name(), "vscode-copilot")
}

fn is_transport_retry_error(error: &edgequake_llm::LlmError) -> bool {
    matches!(
        error,
        edgequake_llm::LlmError::RateLimited(_)
            | edgequake_llm::LlmError::NetworkError(_)
            | edgequake_llm::LlmError::Timeout
            | edgequake_llm::LlmError::AuthError(_)
    )
}

fn is_retryable_nonvisible_stream_error(error: &edgequake_llm::LlmError) -> bool {
    // 401/403 on SSE open are permanent for this credential — retrying only
    // leaves the TUI on "awaiting first token" for tens of seconds.
    if is_permanent_auth_or_permission_error(error) {
        return false;
    }
    matches!(
        error,
        edgequake_llm::LlmError::RateLimited(_)
            | edgequake_llm::LlmError::NetworkError(_)
            | edgequake_llm::LlmError::Timeout
            | edgequake_llm::LlmError::ProviderError(_)
            | edgequake_llm::LlmError::NotSupported(_)
    )
}

/// Auth / quota rejections that must not be treated as flaky transport.
pub(crate) fn is_permanent_auth_or_permission_error(error: &edgequake_llm::LlmError) -> bool {
    let msg = error.to_string().to_ascii_lowercase();
    msg.contains("401")
        || msg.contains("403")
        || msg.contains("unauthorized")
        || msg.contains("forbidden")
        || msg.contains("permission-denied")
        || msg.contains("permission denied")
        || msg.contains("invalid api key")
        || msg.contains("invalid token")
        || msg.contains("invalid_api_key")
        || msg.contains("spending limit")
        || msg.contains("used all available credits")
        || msg.contains("monthly spending limit")
}

/// FP19: Parse a provider-suggested retry-after duration from a rate limit error message.
///
/// WHY: Providers embed a "try again in X.Ys" hint in their error body. Using the
/// provider-stated wait instead of our fixed BASE_BACKOFF avoids under-sleeping
/// (which immediately re-hits the limit) and over-sleeping (which wastes wall time).
///
/// Cross-ref: Hermes `_parse_retry_after(error_str)` in `run_agent.py`.
///
/// Recognised patterns (case-insensitive):
///   "try again in 1.197s"   → 1.197s + 200ms safety margin
///   "retry after 2s"        → 2.0s  + 200ms safety margin
///   "please wait 3 seconds" → 3.0s  + 200ms safety margin
///
/// Returns `None` if no numeric wait hint is found.
pub fn parse_retry_after(error_msg: &str) -> Option<Duration> {
    let lower = error_msg.to_ascii_lowercase();

    // Walk the string looking for candidate float values that follow a retry keyword.
    let keywords = ["try again in ", "retry after ", "please wait ", "wait "];

    for keyword in &keywords {
        if let Some(pos) = lower.find(keyword) {
            let after = &lower[pos + keyword.len()..];
            // Collect leading digit/dot chars to form the number string.
            let num_str: String = after
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if let Ok(seconds) = num_str.parse::<f64>()
                && seconds > 0.0
                && seconds < 300.0
            {
                // Add 200ms safety margin so we don't arrive just as the
                // quota window resets and immediately hit the limit again.
                let millis = (seconds * 1000.0) as u64 + 200;
                return Some(Duration::from_millis(millis));
            }
        }
    }
    None
}

fn is_retryable_stream_tool_assembly_error(error: &edgequake_llm::LlmError) -> bool {
    let message = match error {
        edgequake_llm::LlmError::ApiError(message)
        | edgequake_llm::LlmError::InvalidRequest(message)
        | edgequake_llm::LlmError::ProviderError(message)
        | edgequake_llm::LlmError::NotSupported(message) => message,
        _ => return false,
    };

    let normalized = message.to_ascii_lowercase();
    normalized.contains("streamed tool call")
        && (normalized.contains("without arguments")
            || normalized.contains("without a function name")
            || normalized.contains("missing function name")
            || normalized.contains("invalid json arguments")
            || normalized.contains("arguments must be a json object"))
}

pub(crate) fn is_streamed_tool_capability_error(error: &edgequake_llm::LlmError) -> bool {
    let message = match error {
        edgequake_llm::LlmError::ApiError(message)
        | edgequake_llm::LlmError::InvalidRequest(message)
        | edgequake_llm::LlmError::ProviderError(message)
        | edgequake_llm::LlmError::NotSupported(message) => message,
        _ => return false,
    };

    let normalized = message.to_ascii_lowercase();
    let mentions_streaming = normalized.contains("stream");
    let mentions_tools = normalized.contains("tool")
        || normalized.contains("function call")
        || normalized.contains("function calling");
    let rejects_capability = normalized.contains("not supported")
        || normalized.contains("unsupported")
        || normalized.contains("does not support");

    mentions_streaming && mentions_tools && rejects_capability
}

fn augment_provider_error(provider: &Arc<dyn LLMProvider>, error: String) -> String {
    let lower = error.to_ascii_lowercase();
    if provider.name() == "xai"
        && (lower.contains("403")
            || lower.contains("forbidden")
            || lower.contains("permission-denied")
            || lower.contains("spending limit")
            || lower.contains("credits"))
    {
        return format!(
            "{error}\n\n\
             xAI rejected this request (403). Common causes:\n\
             • Console API key team is out of credits / at spend limit — check console.x.ai\n\
             • SuperGrok OAuth token not applied to the live agent — run /login grok, then \
               /model super-grok/grok-4.5 (force switch) or restart EdgeCrab\n\
             • Subscription without API access for this model\n\
             Tip: /login grok refreshes SuperGrok OAuth; export XAI_API_KEY only for console keys."
        );
    }
    if provider.name() == "vscode-copilot" {
        if lower.contains("bad credentials")
            || lower.contains("copilot token request failed: 401")
            || lower.contains("no github copilot credential")
        {
            return format!(
                "{error} GitHub Copilot needs a fresh login. Exit to a shell and run edgecrab auth login copilot, or rerun edgecrab setup, to perform the official GitHub device flow."
            );
        }
        if lower.contains("user_weekly_rate_limited")
            || lower.contains("user_global_rate_limited")
            || lower.contains("global-chat:global-cogs-7-day-key")
        {
            return format!(
                "GitHub Copilot authentication succeeded, but GitHub is currently rate limiting chat requests for this account. {error} If you are not already using Auto, try /model copilot/auto. If Auto is already selected, this is an account-wide GitHub limit, so wait for the reset window shown above or use another provider."
            );
        }
        if error.contains("api.githubcopilot.com") {
            return format!(
                "{error} GitHub Copilot direct mode could not reach the remote API. If you rely on a local Copilot proxy, set `VSCODE_COPILOT_DIRECT=false` or configure `VSCODE_COPILOT_PROXY_URL`."
            );
        }
        if lower.contains("unsupported_api_for_model")
            || lower.contains("not accessible via the /chat/completions endpoint")
        {
            return format!(
                "{error} This Copilot model cannot drive the agent loop via /chat/completions. Run `/model copilot/auto` or choose a chat-capable model (not a `*-picker` routing model)."
            );
        }
        if crate::copilot_agent_probe::is_copilot_model_not_supported_error(&error) {
            return format!(
                "{error} {}",
                crate::copilot_agent_probe::MODEL_NOT_SUPPORTED_HINT
            );
        }
    }
    error
}

#[derive(Default)]
struct PartialToolCall {
    id: Option<String>,
    function_name: Option<String>,
    arguments: String,
    thought_signature: Option<String>,
    last_generating_emit: Option<std::time::Instant>,
    /// When the provider first sent the tool name (args may still be empty).
    name_received_at: Option<std::time::Instant>,
    /// Last time a non-empty arg delta arrived.
    last_arg_at: Option<std::time::Instant>,
    stall_notice_sent: bool,
}

fn finalize_streamed_tool_calls(
    partials: BTreeMap<usize, PartialToolCall>,
) -> edgequake_llm::Result<Vec<edgequake_llm::ToolCall>> {
    let mapped: BTreeMap<usize, edgequake_llm::PartialStreamToolCall> = partials
        .into_iter()
        .map(|(index, partial)| {
            (
                index,
                edgequake_llm::PartialStreamToolCall {
                    id: partial.id,
                    function_name: partial.function_name,
                    arguments: partial.arguments,
                    thought_signature: partial.thought_signature,
                },
            )
        })
        .collect();
    edgequake_llm::finalize_streamed_tool_calls_with_repair(
        mapped,
        edgequake_llm::FinalizeStreamToolCallsOptions {
            empty_args_fallback:
                edgecrab_tools::tool_argument_pipeline::empty_args_fallback_enabled(),
        },
        Some(edgecrab_tools::tool_argument_pipeline::repair_stream_tool_arguments),
    )
    .map(|calls| {
        calls
            .into_iter()
            .map(|call| {
                let parsed: serde_json::Value =
                    serde_json::from_str(&call.function.arguments).unwrap_or(serde_json::json!({}));
                edgequake_llm::ToolCall {
                    function: edgequake_llm::FunctionCall {
                        arguments: edgecrab_tools::tool_argument_pipeline::canonical_tool_args_json(
                            &parsed,
                        ),
                        ..call.function
                    },
                    ..call
                }
            })
            .collect()
    })
}

fn emit_tool_generating_progress(
    tool_calls: &mut BTreeMap<usize, PartialToolCall>,
    event_tx: &tokio::sync::mpsc::UnboundedSender<crate::StreamEvent>,
) {
    let now = std::time::Instant::now();
    for (index, entry) in tool_calls.iter_mut() {
        let Some(name) = entry.function_name.clone() else {
            continue;
        };
        if !edgecrab_tools::tool_progress_tail::should_emit_progress(
            entry.last_generating_emit,
            now,
        ) {
            continue;
        }
        entry.last_generating_emit = Some(now);
        let tool_id = entry
            .id
            .clone()
            .unwrap_or_else(|| format!("stream-tool-{index}"));
        let _ = event_tx.send(crate::StreamEvent::ToolGenerating {
            tool_call_id: tool_id,
            name,
            partial_args: entry.arguments.clone(),
        });
    }
}

fn tool_arg_stall_break(tool_calls: &BTreeMap<usize, PartialToolCall>) -> Option<(String, usize)> {
    for entry in tool_calls.values() {
        let Some(name) = entry.function_name.as_ref() else {
            continue;
        };
        let Some(name_at) = entry.name_received_at else {
            continue;
        };
        let stall_from = entry.last_arg_at.unwrap_or(name_at);
        if stall_from.elapsed() < Duration::from_secs(TOOL_ARGS_STALL_BREAK_SECS) {
            continue;
        }
        return Some((name.clone(), entry.arguments.len()));
    }
    None
}

/// Native provider-streaming path used by the TUI.
///
/// WHY separate helper: real-time streaming and tool-call accumulation are a
/// different concern from retry logic. This function turns provider-native
/// `StreamChunk`s back into one normalized `LLMResponse` while also forwarding
/// text/reasoning deltas to the UI.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn api_call_streaming(
    provider: &Arc<dyn LLMProvider>,
    messages: &[edgequake_llm::ChatMessage],
    tool_defs: &[edgequake_llm::ToolDefinition],
    tool_choice: Option<edgequake_llm::ToolChoice>,
    options: Option<&edgequake_llm::CompletionOptions>,
    event_tx: &tokio::sync::mpsc::UnboundedSender<crate::StreamEvent>,
    any_tokens_sent: &std::sync::atomic::AtomicBool,
    api_iteration: Option<u32>,
) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
    tracing::info!(
        provider = provider.name(),
        "api_call_streaming: opening SSE stream"
    );
    let mut stream = provider
        .chat_with_tools_stream(messages, tool_defs, tool_choice, options)
        .await?;
    tracing::info!("api_call_streaming: SSE stream opened, waiting for first chunk");

    let mut content = String::new();
    let mut thinking = String::new();
    let mut thinking_tokens = 0usize;
    let mut final_usage: Option<StreamUsage> = None;
    let mut finish_reason: Option<String> = None;
    let mut tool_calls: BTreeMap<usize, PartialToolCall> = BTreeMap::new();
    let has_tools = !tool_defs.is_empty();
    let mut first_chunk_deadline = stream_first_byte_deadline(provider.as_ref(), has_tools, false);
    let mut saw_meaningful_chunk = false;
    let mut sse_open_announced = false;
    let stream_started = std::time::Instant::now();
    let wait_ctx_base = edgecrab_tools::tool_progress_tail::LlmWaitContext {
        prompt_tokens_estimated: Some(estimate_stream_prompt_tokens(messages, tool_defs) as u64),
        context_length: Some(provider.max_context_length() as u64),
        prefill_pct: None,
        api_iteration,
        native_streaming: true,
    };
    let stream_wait_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut stream_wait_handle = {
        let event_tx = event_tx.clone();
        let provider_name = provider.name().to_string();
        let wait_ctx = wait_ctx_base;
        let stop = stream_wait_stop.clone();
        Some(tokio::spawn(async move {
            let started = std::time::Instant::now();
            let heartbeat_secs =
                edgecrab_tools::tool_progress_tail::nonstreaming_wait_heartbeat_secs(
                    &provider_name,
                );
            loop {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let elapsed = started.elapsed().as_secs();
                let _ = event_tx.send(crate::StreamEvent::LlmWaitProgress {
                    provider: provider_name.clone(),
                    elapsed_secs: elapsed,
                    has_tools,
                    prompt_tokens_estimated: wait_ctx.prompt_tokens_estimated,
                    context_length: wait_ctx.context_length,
                    prefill_pct: wait_ctx.prefill_pct,
                    api_iteration: wait_ctx.api_iteration,
                    native_streaming: wait_ctx.native_streaming,
                });
                tokio::time::sleep(Duration::from_secs(heartbeat_secs)).await;
            }
        }))
    };

    loop {
        // Local KV engines (Ollama/LM Studio/vLLM) can pause for minutes during
        // prefill — Hermes disables stale-stream kills for local endpoints.
        let local_provider =
            crate::local_provider_policy::is_local_inference_provider(provider.name());

        let next_chunk = if saw_meaningful_chunk {
            if local_provider {
                // No inter-chunk stale timeout — long local prefill/generation gaps are normal.
                stream.next().await
            } else {
                // Inter-chunk timeout — detect stale streams (FP10).
                match tokio::time::timeout(STREAM_INTER_CHUNK_TIMEOUT, stream.next()).await {
                    Ok(chunk) => chunk,
                    Err(_) => {
                        for entry in tool_calls.values() {
                            if entry.function_name.is_some() {
                                let name = entry.function_name.as_deref().unwrap_or("tool");
                                emit_activity(
                                    Some(event_tx),
                                    edgecrab_tools::tool_progress_tail::format_tool_args_stream_stall(
                                        name,
                                        entry.arguments.len(),
                                        STREAM_INTER_CHUNK_TIMEOUT.as_secs(),
                                    ),
                                );
                            }
                        }
                        tracing::warn!(
                            "api_call_streaming: inter-chunk timeout ({:?}) elapsed — \
                             stream stale, returning partial content",
                            STREAM_INTER_CHUNK_TIMEOUT,
                        );
                        finish_reason = Some(FINISH_REASON_STREAM_INTERRUPTED.to_string());
                        break;
                    }
                }
            }
        } else {
            let remaining = first_chunk_deadline
                .checked_duration_since(tokio::time::Instant::now())
                .unwrap_or(Duration::ZERO);
            match tokio::time::timeout(remaining, stream.next()).await {
                Ok(chunk) => chunk,
                Err(_) => {
                    abort_streaming_wait_progress(&stream_wait_stop, &mut stream_wait_handle);
                    return Err(edgequake_llm::LlmError::Timeout);
                }
            }
        };
        let Some(chunk) = next_chunk else {
            break;
        };
        match chunk? {
            StreamChunk::PrefillProgress { progress } => {
                let prefill_pct = Some((progress.clamp(0.0, 1.0) * 100.0) as f32);
                if !saw_meaningful_chunk {
                    first_chunk_deadline =
                        stream_first_byte_deadline(provider.as_ref(), has_tools, true);
                }
                let _ = event_tx.send(crate::StreamEvent::LlmWaitProgress {
                    provider: provider.name().to_string(),
                    elapsed_secs: stream_started.elapsed().as_secs(),
                    has_tools,
                    prompt_tokens_estimated: wait_ctx_base.prompt_tokens_estimated,
                    context_length: wait_ctx_base.context_length,
                    prefill_pct,
                    api_iteration: wait_ctx_base.api_iteration,
                    native_streaming: true,
                });
            }
            StreamChunk::Connected { phase } => {
                if !saw_meaningful_chunk {
                    first_chunk_deadline =
                        stream_first_byte_deadline(provider.as_ref(), has_tools, true);
                }
                tracing::debug!(
                    provider = provider.name(),
                    phase,
                    post_connect_timeout_secs = if has_tools && provider.name() == "vscode-copilot"
                    {
                        STREAM_POST_CONNECT_FIRST_BYTE_TIMEOUT.as_secs()
                    } else {
                        STREAM_FIRST_CHUNK_TIMEOUT.as_secs()
                    },
                    "api_call_streaming: transport connected, awaiting first model byte"
                );
                let _ = event_tx.send(crate::StreamEvent::LlmWaitProgress {
                    provider: provider.name().to_string(),
                    elapsed_secs: stream_started.elapsed().as_secs(),
                    has_tools,
                    prompt_tokens_estimated: wait_ctx_base.prompt_tokens_estimated,
                    context_length: wait_ctx_base.context_length,
                    prefill_pct: None,
                    api_iteration: wait_ctx_base.api_iteration,
                    native_streaming: true,
                });
                if provider.name() == "vscode-copilot" && has_tools && !sse_open_announced {
                    sse_open_announced = true;
                    emit_activity(
                        Some(event_tx),
                        edgecrab_tools::tool_progress_tail::format_copilot_stream_sse_open(
                            wait_ctx_base.prompt_tokens_estimated,
                            wait_ctx_base.context_length,
                        ),
                    );
                }
            }
            StreamChunk::Content(delta) => {
                if !delta.is_empty() {
                    saw_meaningful_chunk = true;
                    stream_wait_stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    any_tokens_sent.store(true, std::sync::atomic::Ordering::Relaxed);
                    content.push_str(&delta);
                    let _ = event_tx.send(crate::StreamEvent::Token(delta));
                }
            }
            StreamChunk::ThinkingContent {
                text, tokens_used, ..
            } => {
                if !text.is_empty() {
                    saw_meaningful_chunk = true;
                    stream_wait_stop.store(true, std::sync::atomic::Ordering::Relaxed);
                    any_tokens_sent.store(true, std::sync::atomic::Ordering::Relaxed);
                    thinking.push_str(&text);
                    let _ = event_tx.send(crate::StreamEvent::Reasoning(text));
                }
                if let Some(tokens) = tokens_used {
                    thinking_tokens += tokens;
                }
            }
            StreamChunk::ToolCallDelta {
                index,
                id,
                function_name,
                function_arguments,
                thought_signature,
            } => {
                stream_wait_stop.store(true, std::sync::atomic::Ordering::Relaxed);
                let entry = tool_calls.entry(index).or_default();
                if let Some(id) = id {
                    entry.id = Some(id);
                }
                let args_just_arrived = function_arguments.is_some();
                if let Some(args) = function_arguments {
                    if !args.is_empty() {
                        entry.last_arg_at = Some(std::time::Instant::now());
                    }
                    entry.arguments.push_str(&args);
                }
                if thought_signature.is_some() {
                    entry.thought_signature = thought_signature;
                }
                saw_meaningful_chunk = true;
                let name_just_arrived = function_name.is_some();
                let tool_id = entry
                    .id
                    .clone()
                    .unwrap_or_else(|| format!("stream-tool-{index}"));
                if let Some(name) = function_name {
                    entry.function_name = Some(name.clone());
                    if entry.name_received_at.is_none() {
                        entry.name_received_at = Some(std::time::Instant::now());
                    }
                }
                let emit_name = entry
                    .function_name
                    .clone()
                    .unwrap_or_else(|| "tool".to_string());
                let now = std::time::Instant::now();
                if name_just_arrived
                    || (args_just_arrived
                        && edgecrab_tools::tool_progress_tail::should_emit_progress(
                            entry.last_generating_emit,
                            now,
                        ))
                {
                    entry.last_generating_emit = Some(now);
                    let _ = event_tx.send(crate::StreamEvent::ToolGenerating {
                        tool_call_id: tool_id,
                        name: emit_name.clone(),
                        partial_args: entry.arguments.clone(),
                    });
                }
                if !entry.stall_notice_sent
                    && entry.arguments.is_empty()
                    && entry.name_received_at.is_some_and(|at| {
                        at.elapsed() >= Duration::from_secs(TOOL_ARGS_STALL_NOTICE_SECS)
                    })
                {
                    entry.stall_notice_sent = true;
                    emit_activity(
                        Some(event_tx),
                        edgecrab_tools::tool_progress_tail::format_tool_args_stream_stall(
                            &emit_name,
                            0,
                            TOOL_ARGS_STALL_NOTICE_SECS,
                        ),
                    );
                }
            }
            StreamChunk::Finished { reason, usage, .. } => {
                saw_meaningful_chunk = true;
                finish_reason = Some(reason);
                if usage.is_some() {
                    final_usage = usage;
                }
            }
        }

        emit_tool_generating_progress(&mut tool_calls, event_tx);

        if let Some((stalled_name, arg_bytes)) = tool_arg_stall_break(&tool_calls) {
            let notice = edgecrab_tools::mutation_turn_policy::tool_draft_stall_recovery_notice(
                &stalled_name,
                arg_bytes,
                edgecrab_tools::edit_contract::DEFAULT_MAX_MUTATION_PAYLOAD_BYTES,
                Some(provider.as_ref()),
            );
            emit_activity(Some(event_tx), notice);
            tracing::warn!(
                tool = %stalled_name,
                arg_bytes,
                stall_secs = TOOL_ARGS_STALL_BREAK_SECS,
                "api_call_streaming: tool-arg stall — aborting streamed draft"
            );
            finish_reason = Some(FINISH_REASON_STREAM_INTERRUPTED.to_string());
            break;
        }
    }

    abort_streaming_wait_progress(&stream_wait_stop, &mut stream_wait_handle);

    let mut response = edgequake_llm::LLMResponse::new(content, provider.model().to_string());
    if let Some(reason) = finish_reason {
        response.finish_reason = Some(reason);
    }
    if !thinking.is_empty() {
        response.thinking_content = Some(thinking);
    }

    response.tool_calls = match finalize_streamed_tool_calls(tool_calls) {
        Ok(tool_calls) => tool_calls,
        Err(err)
            if is_retryable_stream_tool_assembly_error(&err)
                && (!response.content.trim().is_empty()
                    || response
                        .thinking_content
                        .as_deref()
                        .is_some_and(|t| !t.trim().is_empty())) =>
        {
            tracing::warn!(
                provider = provider.name(),
                model = provider.model(),
                error = %err,
                "streamed tool-call assembly failed after visible output; preserving partial response without tool calls"
            );
            response.finish_reason = Some(FINISH_REASON_STREAM_INTERRUPTED.to_string());
            Vec::new()
        }
        Err(err) => {
            abort_streaming_wait_progress(&stream_wait_stop, &mut stream_wait_handle);
            return Err(err);
        }
    };

    if let Some(usage) = final_usage {
        response = response.with_usage(usage.prompt_tokens, usage.completion_tokens);
        if let Some(cache_hit_tokens) = usage.cache_hit_tokens {
            response = response.with_cache_hit_tokens(cache_hit_tokens);
        }
        if let Some(cache_write_tokens) = usage.cache_write_tokens {
            response = response.with_cache_write_tokens(cache_write_tokens);
        }
        if let Some(authoritative_thinking_tokens) = usage.thinking_tokens {
            response = response.with_thinking_tokens(authoritative_thinking_tokens);
        } else if thinking_tokens > 0 {
            response = response.with_thinking_tokens(thinking_tokens);
        }
    } else {
        let estimated_prompt_tokens = estimate_stream_prompt_tokens(messages, tool_defs);
        let estimated_completion_tokens = estimate_stream_completion_tokens(
            &response.content,
            response.thinking_content.as_deref(),
            &response.tool_calls,
        );
        if estimated_prompt_tokens > 0 || estimated_completion_tokens > 0 {
            response = response.with_usage(estimated_prompt_tokens, estimated_completion_tokens);
        }
        if thinking_tokens > 0 {
            response = response.with_thinking_tokens(thinking_tokens);
        }
    }

    Ok(response)
}

fn abort_streaming_wait_progress(
    stop: &std::sync::atomic::AtomicBool,
    handle: &mut Option<tokio::task::JoinHandle<()>>,
) {
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(h) = handle.take() {
        h.abort();
    }
}

/// Emit periodic shelf notices while a non-streaming LLM request blocks (local servers, Copilot, etc.).
async fn invoke_pre_api_request_hooks(
    ctx: &ApiCallContext<'_>,
    provider: &Arc<dyn LLMProvider>,
    messages: &[edgequake_llm::ChatMessage],
    tool_defs: &[edgequake_llm::ToolDefinition],
    attempt: u32,
) {
    let Some(discovery) = ctx.discovered_plugins else {
        return;
    };

    let approx_input_tokens = estimate_stream_prompt_tokens(messages, tool_defs);
    let request_char_count = serde_json::to_string(messages)
        .map(|serialized| serialized.chars().count())
        .unwrap_or_default();
    let max_tokens = ctx
        .options
        .and_then(|options| options.max_tokens)
        .unwrap_or(0);

    for plugin in discovery
        .plugins
        .iter()
        .filter(|plugin| hermes_supports_hook(plugin, "pre_api_request"))
    {
        if let Err(error) = invoke_hermes_hook(
            plugin,
            "pre_api_request",
            serde_json::json!({
                "task_id": ctx.conversation_session_id,
                "session_id": ctx.conversation_session_id,
                "platform": ctx.platform.to_string(),
                "model": provider.model(),
                "provider": provider.name(),
                "base_url": serde_json::Value::Null,
                "api_mode": if tool_defs.is_empty() { "chat" } else { "chat_with_tools" },
                "api_call_count": ctx.api_call_count + attempt + 1,
                "message_count": messages.len(),
                "tool_count": tool_defs.len(),
                "approx_input_tokens": approx_input_tokens,
                "request_char_count": request_char_count,
                "max_tokens": max_tokens,
            }),
        )
        .await
        {
            tracing::warn!(plugin = %plugin.name, ?error, "Hermes pre_api_request hook failed");
        }
    }
}

async fn invoke_post_api_request_hooks(
    ctx: &ApiCallContext<'_>,
    provider: &Arc<dyn LLMProvider>,
    messages: &[edgequake_llm::ChatMessage],
    tool_defs: &[edgequake_llm::ToolDefinition],
    response: &edgequake_llm::LLMResponse,
    attempt: u32,
    started_at: std::time::Instant,
) {
    let Some(discovery) = ctx.discovered_plugins else {
        return;
    };

    let usage = serde_json::json!({
        "prompt_tokens": response.prompt_tokens,
        "completion_tokens": response.completion_tokens,
        "total_tokens": response.total_tokens,
        "cache_hit_tokens": response.cache_hit_tokens,
        "thinking_tokens": response.thinking_tokens,
    });

    for plugin in discovery
        .plugins
        .iter()
        .filter(|plugin| hermes_supports_hook(plugin, "post_api_request"))
    {
        if let Err(error) = invoke_hermes_hook(
            plugin,
            "post_api_request",
            serde_json::json!({
                "task_id": ctx.conversation_session_id,
                "session_id": ctx.conversation_session_id,
                "platform": ctx.platform.to_string(),
                "model": provider.model(),
                "provider": provider.name(),
                "base_url": serde_json::Value::Null,
                "api_mode": if tool_defs.is_empty() { "chat" } else { "chat_with_tools" },
                "api_call_count": ctx.api_call_count + attempt + 1,
                "api_duration": started_at.elapsed().as_secs_f64(),
                "finish_reason": response.finish_reason.clone().unwrap_or_else(|| "stop".into()),
                "message_count": messages.len(),
                "response_model": response.model.clone(),
                "usage": usage,
                "assistant_content_chars": response.content.chars().count(),
                "assistant_tool_call_count": response.tool_calls.len(),
            }),
        )
        .await
        {
            tracing::warn!(plugin = %plugin.name, ?error, "Hermes post_api_request hook failed");
        }
    }
}

fn spawn_nonstreaming_wait_heartbeat(
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<crate::StreamEvent>>,
    cancel: &tokio_util::sync::CancellationToken,
    provider: &dyn LLMProvider,
    has_tools: bool,
    wait_ctx: edgecrab_tools::tool_progress_tail::LlmWaitContext,
    http_timeout_secs: u64,
) -> Option<tokio::task::JoinHandle<()>> {
    let tx = event_tx.cloned()?;
    let provider_name = provider.name().to_string();
    let cancel = cancel.clone();
    let timeout_warn_at = ((http_timeout_secs as f64)
        * crate::local_provider_policy::LOCAL_HTTP_TIMEOUT_WARN_RATIO)
        as u64;
    Some(tokio::spawn(async move {
        let started = std::time::Instant::now();
        let mut timeout_warned = false;
        let heartbeat_secs =
            edgecrab_tools::tool_progress_tail::nonstreaming_wait_heartbeat_secs(&provider_name);
        emit_activity(
            Some(&tx),
            edgecrab_tools::tool_progress_tail::format_nonstreaming_llm_start(
                &provider_name,
                has_tools,
                wait_ctx,
            ),
        );
        loop {
            let elapsed = started.elapsed().as_secs();
            if has_tools && !timeout_warned && http_timeout_secs > 0 && elapsed >= timeout_warn_at {
                timeout_warned = true;
                tracing::warn!(
                    target: crate::observability::TARGET_PROVIDER_LLM,
                    provider = %provider_name,
                    elapsed_secs = elapsed,
                    http_timeout_secs,
                    prompt_tokens_estimated = ?wait_ctx.prompt_tokens_estimated,
                    context_length = ?wait_ctx.context_length,
                    "provider_llm: approaching HTTP timeout while composing tool call"
                );
                let notice =
                    edgecrab_tools::tool_progress_tail::format_local_timeout_proximity_notice(
                        &provider_name,
                        elapsed,
                        http_timeout_secs,
                    );
                emit_activity(Some(&tx), notice);
            }
            let _ = tx.send(crate::StreamEvent::LlmWaitProgress {
                provider: provider_name.clone(),
                elapsed_secs: elapsed,
                has_tools,
                prompt_tokens_estimated: wait_ctx.prompt_tokens_estimated,
                context_length: wait_ctx.context_length,
                prefill_pct: wait_ctx.prefill_pct,
                api_iteration: wait_ctx.api_iteration,
                native_streaming: false,
            });
            let delay = Duration::from_secs(heartbeat_secs);
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(delay) => {}
            }
        }
    }))
}

fn emit_local_transport_stall_notice(
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<crate::StreamEvent>>,
    provider: &dyn LLMProvider,
) {
    emit_activity(
        event_tx,
        crate::local_provider_policy::transport_stall_user_notice(provider),
    );
}

/// API call with exponential backoff retry.
///
/// WHY application-level retry: edgequake-llm doesn't retry internally.
/// Transient 429/503/529 errors are common with high-traffic providers.
/// 3 retries with 500ms/1s/2s backoff covers most transient failures
/// without being so aggressive that we anger rate limiters further.
///
/// WHY chat_with_tools when tools present: The LLM must see tool
/// definitions to request tool calls. Without them it can only
/// produce text responses, breaking the REACT loop.
/// Make an LLM API call with exponential-backoff retries, aborting
/// immediately when the `cancel` token is triggered (Ctrl+C).
///
/// WHY cancellation-aware:
/// - CancellationToken is a one-way latch set by `agent.interrupt()`.
/// - Without `tokio::select!` the function would finish all retries and
///   their sleep delays before the outer loop could notice the token,
///   leaving the TUI unresponsive for several seconds after Ctrl+C.
/// - We race both the backoff sleep and the in-flight API call against
///   `cancel.cancelled()` so cancellation takes effect immediately.
/// - Dropping the in-flight provider future is safe: reqwest/hyper
///   futures are cancel-safe (no protocol-level cleanup needed).
pub async fn api_call_with_retry(
    provider: &Arc<dyn LLMProvider>,
    messages: &[edgequake_llm::ChatMessage],
    tool_defs: &[edgequake_llm::ToolDefinition],
    max_retries: u32,
    ctx: ApiCallContext<'_>,
) -> Result<ApiCallOutcome, AgentError> {
    let mut last_err: Option<String> = None;
    let mut native_tool_streaming_enabled = ctx.use_native_streaming;
    let mut disabled_native_tool_streaming = false;
    // WHY max_retries for both streaming and non-streaming:
    //
    // The old code used `retry_budget = 0` for native streaming to avoid
    // re-sending duplicate tokens to the TUI on retry.  That protection was
    // too broad: it also prevented retrying *pre-stream* errors (e.g. HTTP 429
    // "Resource Exhausted") that are returned by the provider before any SSE
    // byte is emitted — so no token has ever been pushed to the channel.
    //
    // The correct invariant is:
    //   • LlmError::RateLimited  → always pre-stream; safe to retry.
    //   • LlmError::NetworkError / Timeout → connection-level errors before
    //     any streaming data; safe to retry.
    //   • LlmError::ApiError during streaming → may be mid-stream (partial
    //     tokens already sent); NOT safe to retry (would produce duplicates).
    //
    // We enforce this in the Err arm below.
    let retry_budget = max_retries;
    // FP19: Provider-stated retry-after delay, set when we receive a rate limit
    // error with an embedded "try again in X.Ys" hint. Used in the next
    // iteration's backoff instead of the fixed BASE_BACKOFF.
    let mut rate_limit_delay: Option<Duration> = None;
    let mut stream_assembly_retries: u32 = 0;
    let mut image_shrink_retried = false;
    let mut recovered_messages: Option<Vec<edgequake_llm::ChatMessage>> = None;
    const MAX_STREAM_ASSEMBLY_RETRIES: u32 = 2;
    const MAX_COPILOT_STREAM_ASSEMBLY_RETRIES: u32 = 5;
    let mut last_failed_attempt = 0u32;

    'attempt_loop: for attempt in 0..=retry_budget {
        // ── Backoff sleep — interruptible ──────────────────────────────
        if attempt > 0 {
            // FP19: Use provider-stated wait time when available; fall back to
            // exponential backoff. This avoids under-sleeping (hit limit again)
            // and over-sleeping (wastes wall time).
            let delay = rate_limit_delay
                .take()
                .unwrap_or_else(|| BASE_BACKOFF * 2u32.saturating_pow(attempt - 1));
            tracing::debug!(
                attempt,
                delay_ms = delay.as_millis(),
                "api_call_with_retry: sleeping before retry"
            );
            tokio::select! {
                biased;
                _ = ctx.cancel.cancelled() => {
                    tracing::debug!(attempt, "api_call_with_retry: cancelled during backoff sleep");
                    return Err(AgentError::Interrupted);
                }
                _ = tokio::time::sleep(delay) => {
                    tracing::debug!(attempt, "retrying API call after backoff");
                }
            }
        }

        // WHY per-attempt AtomicBool: tracks whether api_call_streaming
        // emitted at least one token (Content, Thinking, or ToolCallDelta)
        // before erroring. If tokens arrived, the error is mid-stream and
        // retrying would produce duplicate TUI content — so we abort instead.
        // A fresh AtomicBool for each attempt resets the flag correctly.
        let mut use_native_streaming_this_attempt = native_tool_streaming_enabled;

        loop {
            let tokens_sent = std::sync::atomic::AtomicBool::new(false);
            let request_messages = recovered_messages.as_deref().unwrap_or(messages);
            invoke_pre_api_request_hooks(&ctx, provider, request_messages, tool_defs, attempt)
                .await;
            let request_started_at = std::time::Instant::now();
            let prompt_tokens_est = estimate_stream_prompt_tokens(request_messages, tool_defs);
            let http_timeout_secs =
                crate::local_provider_policy::local_http_timeout_secs(provider.name());
            let tool_choice = crate::local_provider_policy::local_tool_choice(
                provider.as_ref(),
                !tool_defs.is_empty(),
            );
            let tool_choice_required = tool_choice.is_some();
            let correlation = crate::observability::LlmCorrelation {
                session_id: ctx.conversation_session_id,
                api_call_count: ctx.api_call_count,
                attempt,
                platform: ctx.platform,
            };
            let request_start = crate::observability::LlmRequestStart {
                correlation,
                provider: provider.name(),
                model: provider.model(),
                streaming: use_native_streaming_this_attempt,
                tool_count: tool_defs.len(),
                prompt_tokens_estimated: prompt_tokens_est,
                context_length: provider.max_context_length(),
                http_timeout_secs,
                tool_choice_required,
                max_tokens: ctx.options.and_then(|o| o.max_tokens),
                reasoning_effort: ctx.options.and_then(|o| o.reasoning_effort.as_deref()),
            };
            let attempt_span = crate::observability::llm_attempt_span(&request_start);

            // ── API call — interruptible ────────────────────────────────
            // We race the provider future against the cancel token.
            // Dropping the provider future is safe: HTTP futures in reqwest
            // are cancel-safe (the TCP connection is simply closed).
            if let Some(tx) = ctx.event_tx {
                let ctx_json = crate::observability::llm_pre_hook_json(&request_start).to_string();
                let _ = tx.send(crate::StreamEvent::HookEvent {
                    event: "llm:pre".to_string(),
                    context_json: ctx_json,
                });
            }

            let call_fut = async {
                let platform = ctx.platform.to_string();
                edgequake_llm::with_trace_context(ctx.conversation_session_id, &platform, async {
                    if use_native_streaming_this_attempt {
                        let tx = ctx
                            .event_tx
                            .expect("native streaming requires event channel");
                        api_call_streaming(
                            provider,
                            request_messages,
                            tool_defs,
                            tool_choice.clone(),
                            ctx.options,
                            tx,
                            &tokens_sent,
                            Some(ctx.api_call_count.saturating_add(1)),
                        )
                        .await
                    } else if tool_defs.is_empty() {
                        provider.chat(request_messages, ctx.options).await
                    } else {
                        provider
                            .chat_with_tools(request_messages, tool_defs, tool_choice, ctx.options)
                            .await
                    }
                })
                .await
            }
            .instrument(attempt_span);

            let heartbeat = if !use_native_streaming_this_attempt {
                let wait_ctx = edgecrab_tools::tool_progress_tail::LlmWaitContext {
                    prompt_tokens_estimated: Some(prompt_tokens_est as u64),
                    context_length: Some(provider.max_context_length() as u64),
                    prefill_pct: None,
                    api_iteration: Some(ctx.api_call_count.saturating_add(1)),
                    native_streaming: false,
                };
                spawn_nonstreaming_wait_heartbeat(
                    ctx.event_tx,
                    ctx.cancel,
                    provider.as_ref(),
                    !tool_defs.is_empty(),
                    wait_ctx,
                    http_timeout_secs,
                )
            } else {
                None
            };

            let result = tokio::select! {
                biased;
                _ = ctx.cancel.cancelled() => {
                    tracing::debug!(attempt, "api_call_with_retry: cancelled during API call");
                    return Err(AgentError::Interrupted);
                }
                r = call_fut => r,
            };

            if let Some(handle) = heartbeat {
                handle.abort();
            }

            match result {
                Ok(response) => {
                    let elapsed_ms = request_started_at.elapsed().as_millis() as u64;
                    invoke_post_api_request_hooks(
                        &ctx,
                        provider,
                        request_messages,
                        tool_defs,
                        &response,
                        attempt,
                        request_started_at,
                    )
                    .await;
                    let operation = if use_native_streaming_this_attempt {
                        "chat_with_tools_stream"
                    } else if tool_defs.is_empty() {
                        "chat"
                    } else {
                        "chat_with_tools"
                    };
                    crate::observability::record_llm_operation(
                        correlation,
                        provider.name(),
                        provider.model(),
                        operation,
                        elapsed_ms,
                        true,
                        Some(&response),
                    );
                    if response.finish_reason.as_deref() == Some(FINISH_REASON_STREAM_INTERRUPTED) {
                        tracing::warn!(
                            provider = provider.name(),
                            model = provider.model(),
                            "stream interrupted after partial output; keeping native streaming enabled for next turn"
                        );
                    }
                    if let Some(tx) = ctx.event_tx {
                        let ctx_json = crate::observability::llm_post_hook_json(
                            correlation,
                            provider.name(),
                            provider.model(),
                            use_native_streaming_this_attempt,
                            elapsed_ms,
                            &response,
                        )
                        .to_string();
                        let _ = tx.send(crate::StreamEvent::HookEvent {
                            event: "llm:post".to_string(),
                            context_json: ctx_json,
                        });
                    }
                    return Ok(ApiCallOutcome {
                        response,
                        disabled_native_tool_streaming,
                        provider_replacement: None,
                    });
                }
                Err(e) => {
                    let elapsed_ms = request_started_at.elapsed().as_millis() as u64;
                    crate::observability::log_llm_attempt_failure(
                        correlation,
                        provider.name(),
                        provider.model(),
                        use_native_streaming_this_attempt,
                        elapsed_ms,
                        &e.to_string(),
                    );
                    let visible_output_sent =
                        tokens_sent.load(std::sync::atomic::Ordering::Relaxed);
                    if !image_shrink_retried
                        && !visible_output_sent
                        && (crate::compression::is_image_too_large_error(&e)
                            || (crate::compression::is_payload_too_large_error(&e)
                                && crate::compression::provider_messages_have_images(
                                    request_messages,
                                )))
                    {
                        let mut shrunk = request_messages.to_vec();
                        image_shrink_retried = true;
                        if crate::compression::shrink_provider_images_for_retry(&mut shrunk) {
                            tracing::warn!(
                                provider = provider.name(),
                                model = provider.model(),
                                "provider rejected oversized image; retrying once with shrunken payload"
                            );
                            recovered_messages = Some(shrunk);
                            continue;
                        }
                    }
                    if crate::local_provider_policy::blocks_transport_retry(provider.as_ref(), &e) {
                        crate::local_provider_policy::log_local_llm_transport_failure(
                            provider.as_ref(),
                            elapsed_ms,
                            attempt,
                            &e.to_string(),
                        );
                        emit_local_transport_stall_notice(ctx.event_tx, provider.as_ref());
                        last_err = Some(e.to_string());
                        break 'attempt_loop;
                    }

                    let provider_handles_error =
                        provider_manages_transport_retries(provider.as_ref())
                            && is_transport_retry_error(&e);

                    if use_native_streaming_this_attempt {
                        // Fail fast on 401/403 — do not burn retries while the shelf
                        // shows "awaiting first token".
                        if !visible_output_sent && is_permanent_auth_or_permission_error(&e) {
                            let err = augment_provider_error(provider, e.to_string());
                            return Err(AgentError::Llm(err));
                        }
                        if !visible_output_sent && matches!(e, edgequake_llm::LlmError::Timeout) {
                            if provider.name() == "vscode-copilot" && !tool_defs.is_empty() {
                                tracing::warn!(
                                    provider = provider.name(),
                                    model = provider.model(),
                                    attempt,
                                    "Copilot stream timed out before first tool delta; \
                                     retrying streaming (non-stream fallback often returns empty choices)"
                                );
                                continue;
                            }
                            tracing::warn!(
                                provider = provider.name(),
                                model = provider.model(),
                                attempt,
                                "native streaming stalled before first visible chunk; falling back to non-streaming for this request"
                            );
                            use_native_streaming_this_attempt = false;
                            continue;
                        }
                        if !visible_output_sent && is_retryable_stream_tool_assembly_error(&e) {
                            stream_assembly_retries += 1;
                            let copilot_tools =
                                provider.name() == "vscode-copilot" && !tool_defs.is_empty();
                            let retry_cap = if copilot_tools {
                                MAX_COPILOT_STREAM_ASSEMBLY_RETRIES
                            } else {
                                MAX_STREAM_ASSEMBLY_RETRIES
                            };
                            if stream_assembly_retries <= retry_cap {
                                tracing::warn!(
                                    provider = provider.name(),
                                    model = provider.model(),
                                    attempt,
                                    stream_assembly_retries,
                                    error = %e,
                                    "streamed tool-call assembly failed before visible output; retrying streaming"
                                );
                                continue;
                            }
                            if copilot_tools {
                                let err = augment_provider_error(provider, e.to_string());
                                return Err(AgentError::Llm(format!(
                                    "Copilot streamed tool-call assembly failed after \
                                     {stream_assembly_retries} retries: {err}"
                                )));
                            }
                            tracing::warn!(
                                provider = provider.name(),
                                model = provider.model(),
                                attempt,
                                error = %e,
                                "streamed tool-call assembly failed; falling back to non-streaming for this request only"
                            );
                            use_native_streaming_this_attempt = false;
                            continue;
                        }
                        if !visible_output_sent
                            && !tool_defs.is_empty()
                            && is_streamed_tool_capability_error(&e)
                        {
                            tracing::warn!(
                                provider = provider.name(),
                                model = provider.model(),
                                "provider rejected streamed tool turns; downgrading this session to non-streaming tool calls"
                            );
                            use_native_streaming_this_attempt = false;
                            native_tool_streaming_enabled = false;
                            disabled_native_tool_streaming = true;
                            continue;
                        }

                        // For native streaming, only continue retrying if the error
                        // happened before any visible output was emitted. Tool-call
                        // deltas are buffered locally and are not user-visible, so a
                        // malformed streamed tool call can safely be retried.
                        if visible_output_sent || !is_retryable_nonvisible_stream_error(&e) {
                            let err = augment_provider_error(provider, e.to_string());
                            return Err(AgentError::Llm(format!(
                                "API call failed after {} retries: {}",
                                attempt, err
                            )));
                        }
                    }

                    last_err = Some(e.to_string());
                    last_failed_attempt = attempt;
                    if provider_handles_error {
                        break 'attempt_loop;
                    }
                    // Non-retryable errors: abort immediately instead of
                    // burning through the retry budget on a permanent failure
                    // (geo-block, bad API key, invalid request, etc.).
                    if should_abort_api_retries(&e, ctx.session) {
                        break 'attempt_loop;
                    }
                    // FP19: For rate-limit errors, parse the provider-stated
                    // retry-after duration so the next backoff sleeps the
                    // correct amount rather than a fixed BASE_BACKOFF.
                    if let edgequake_llm::LlmError::RateLimited(msg) = &e {
                        rate_limit_delay = parse_retry_after(msg.as_str());
                        if let Some(d) = rate_limit_delay {
                            tracing::info!(
                                provider = provider.name(),
                                model = provider.model(),
                                wait_ms = d.as_millis(),
                                "rate limited — using provider-stated retry-after delay"
                            );
                        }
                        // Wave-2: credential pool rotate when classification says so.
                        // UpstreamRateLimit does not rotate (see failover).
                        let classified = crate::failover::classify_llm_error(&e, ctx.session);
                        if classified.should_rotate_credential {
                            if let Some(new_key) =
                                crate::credential_pool::global_mark_exhausted_and_rotate(
                                    provider.name(),
                                )
                            {
                                let replacement = crate::credential_pool::apply_rotated_token(
                                    provider.name(),
                                    &new_key,
                                )
                                .and_then(|_| {
                                    edgecrab_tools::create_provider_for_model(
                                        provider.name(),
                                        provider.model(),
                                    )
                                });
                                tracing::warn!(
                                    provider = provider.name(),
                                    rebuilt = replacement.is_ok(),
                                    "credential pool rotated after rate limit"
                                );
                                if let Some(tx) = ctx.event_tx {
                                    let _ = tx.send(crate::StreamEvent::HookEvent {
                                        event: "credential:rotated".into(),
                                        context_json: format!(
                                            r#"{{"provider":"{}","status":"rotated"}}"#,
                                            provider.name()
                                        ),
                                    });
                                }
                                if let Ok(replacement) = replacement {
                                    // The failed request future has completed and emitted no
                                    // visible output. Re-enter through a freshly constructed
                                    // provider so the next attempt uses the rotated key.
                                    let mut outcome = Box::pin(api_call_with_retry(
                                        &replacement,
                                        messages,
                                        tool_defs,
                                        max_retries.saturating_sub(attempt),
                                        ctx,
                                    ))
                                    .await?;
                                    if outcome.provider_replacement.is_none() {
                                        outcome.provider_replacement = Some(replacement);
                                    }
                                    return Ok(outcome);
                                }
                            }
                        } else if matches!(
                            classified.reason,
                            crate::failover::FailoverReason::UpstreamRateLimit
                        ) {
                            tracing::info!(
                                provider = provider.name(),
                                "upstream rate limit — not rotating credentials (fallback model instead)"
                            );
                        }
                    }
                    break;
                }
            }
        }
    }

    let raw_err = last_err.map_or_else(
        || "unknown error".to_string(),
        |e| augment_provider_error(provider, e),
    );

    let classified = crate::failover::classify_api_error(crate::failover::ClassifyInput {
        raw_message: &raw_err,
        body_message: None,
        metadata_message: None,
        status_code: None,
        error_code: None,
        error_type_name: None,
        provider: provider.name(),
        model: provider.model(),
        session: ctx.session,
    });

    let transport_stall =
        crate::local_provider_policy::is_local_inference_provider(provider.name())
            && crate::local_provider_policy::transport_stall_error_suffix(provider.name())
                .is_some()
            && {
                let lower = raw_err.to_ascii_lowercase();
                lower.contains("timeout")
                    || lower.contains("timed out")
                    || lower.contains("network")
            };

    // FP18: For rate-limit errors, produce a clear message that names the model
    // and suggests the user wait before retrying — mirrors hermes-agent guidance.
    let final_err_msg = if !classified.retryable {
        crate::provider_error_class::format_classified(&classified, last_failed_attempt, &raw_err)
    } else if transport_stall {
        format!(
            "{} Provider error: {}",
            crate::local_provider_policy::transport_stall_error_suffix(provider.name())
                .expect("checked above"),
            raw_err
        )
    } else if matches!(
        classified.reason,
        crate::failover::FailoverReason::RateLimit
    ) || raw_err.to_ascii_lowercase().contains("rate limit")
        || raw_err.to_ascii_lowercase().contains("rate_limit")
        || raw_err.to_ascii_lowercase().contains("429")
        || raw_err.to_ascii_lowercase().contains("too many requests")
    {
        format!(
            "Rate limit exceeded for model {} after {} retries. \
             Wait a minute and retry, or reduce context size / switch to a model with higher TPM limits. \
             Provider error: {}",
            provider.model(),
            retry_budget,
            raw_err
        )
    } else {
        format!(
            "API call failed after {} retries: {}",
            last_failed_attempt, raw_err
        )
    };

    Err(AgentError::Llm(final_err_msg))
}

pub struct ApiCallOutcome {
    pub response: edgequake_llm::LLMResponse,
    pub disabled_native_tool_streaming: bool,
    /// Fresh provider constructed after credential rotation at a request boundary.
    pub provider_replacement: Option<Arc<dyn LLMProvider>>,
}

impl std::fmt::Debug for ApiCallOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiCallOutcome")
            .field("response", &self.response)
            .field(
                "disabled_native_tool_streaming",
                &self.disabled_native_tool_streaming,
            )
            .field(
                "provider_replacement",
                &self.provider_replacement.as_ref().map(|_| "<provider>"),
            )
            .finish()
    }
}
