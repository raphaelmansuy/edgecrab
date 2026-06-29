//! Policy for local inference providers (LM Studio, Ollama).
//!
//! Local servers cannot cancel in-flight generations when the HTTP client
//! times out or retries. A single responsibility module keeps transport,
//! streaming-fallback, and completion-token rules in one place (DRY/SOLID).
//!
//! First principles for tool turns:
//! - **`max_tokens`** = `mutation_turn_policy::output_token_budget_for_tool_turn` (same
//!   formula as pre-dispatch argument guards — not failure-adaptive).
//! - **`reasoning_effort: none`** always on tool turns (reasoning burns the completion budget).
//! - **`tool_choice: required`** when tools are present (model must emit tool JSON, not prose).

use edgequake_llm::{CompletionOptions, LlmError, ToolChoice, LLMProvider};

/// Default HTTP timeout for LM Studio chat/completions (seconds).
pub const DEFAULT_LOCAL_HTTP_TIMEOUT_SECS: u64 = 600;

/// Reasoning mode forced on every local tool turn.
pub const LOCAL_TOOL_TURN_REASONING_EFFORT: &str = "none";

/// Emit a shelf warning when this fraction of the HTTP timeout elapses.
pub const LOCAL_HTTP_TIMEOUT_WARN_RATIO: f64 = 0.80;

/// Default estimated prompt tokens above which local tool turns run structural prefill prune.
///
/// Deliberately **below** the 50% LLM-compression threshold on synced LM Studio context
/// and **below** the homelab length-failure band (~34–37k @ 262k ctx) so tool-heavy research
/// reclaims context before slow local prefill — without an LLM summarisation call.
pub const LOCAL_PREFILL_PRUNE_TOKEN_BUDGET: usize = 32_000;

/// Context-length divisor for prefill budget: `min(BUDGET, ctx / DIVISOR)`.
pub const LOCAL_PREFILL_CONTEXT_DIVISOR: usize = 8;

/// Local mid-band structural compress threshold as a fraction of active context (no LLM).
///
/// Fills the gap between preflight prune (~32k @ 262k) and LLM compress (50% ctx).
/// **0.20** (not 0.22): homelab agent.jsonl reports ~56–57k prompts @ 262k ctx; at 0.22
/// the threshold is 57 671 — sessions at 57 000 never compress (deterministic gap).
pub const LOCAL_STRUCTURAL_COMPRESS_THRESHOLD_RATIO: f32 = 0.20;

/// Resolve mid-band structural compress ratio: env override > compile-time default.
pub fn local_structural_compress_threshold_ratio() -> f32 {
    std::env::var("EDGECRAB_LOCAL_STRUCTURAL_COMPRESS_RATIO")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .filter(|ratio| *ratio > 0.0 && *ratio < 1.0)
        .unwrap_or(LOCAL_STRUCTURAL_COMPRESS_THRESHOLD_RATIO)
}

/// Providers that run on localhost and queue generations server-side.
pub fn is_local_inference_provider(provider_name: &str) -> bool {
    matches!(provider_name, "lmstudio" | "ollama")
}

/// First segment of `provider/model` or bare provider name.
pub fn provider_prefix(model_or_provider: &str) -> &str {
    model_or_provider.split('/').next().unwrap_or(model_or_provider)
}

/// Homelab write path default: config flag OR auto-on for local providers.
pub fn effective_local_write_create_dirs(config_flag: bool, provider_or_model: &str) -> bool {
    config_flag || is_local_inference_provider(provider_prefix(provider_or_model))
}

/// P0–P5 local harness + tool-call pipeline gate (tools must be present).
pub fn local_tool_harness_active(provider_name: &str, has_tools: bool) -> bool {
    has_tools && is_local_inference_provider(provider_name)
}

static LOCAL_HARNESS_ACTIVATION_LOGGED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// One info log per process when local harness is active — confirms 014 + pipeline are wired.
pub fn log_local_harness_activated(provider_name: &str, has_tools: bool, write_create_dirs: bool) {
    if !is_local_inference_provider(provider_name) {
        return;
    }
    if LOCAL_HARNESS_ACTIVATION_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    tracing::info!(
        target: "edgecrab::local_llm",
        provider = provider_name,
        has_tools,
        write_create_dirs,
        structural_prefill_prune = has_tools,
        mid_band_compress = has_tools,
        tool_call_pipeline = has_tools,
        "local inference harness activated (default-on for lmstudio/ollama)"
    );
}

/// Tool turns that should use atomic non-streaming completion.
///
/// Copilot is included here for the same buffering reasons as local servers.
pub fn prefers_nonstreaming_tool_turns(provider: &dyn LLMProvider) -> bool {
    matches!(
        provider.name(),
        "vscode-copilot" | "lmstudio" | "ollama"
    )
}

/// Whether EdgeCrab must not retry a failed transport call.
///
/// Retrying after timeout/network error starts a second HTTP request while the
/// server keeps generating the orphaned job (dual GEN counters in LM Studio).
pub fn blocks_transport_retry(provider: &dyn LLMProvider, error: &LlmError) -> bool {
    if !is_local_inference_provider(provider.name()) {
        return false;
    }
    matches!(
        error,
        LlmError::Timeout | LlmError::NetworkError(_)
    )
}

/// Whether streaming→non-streaming fallback must be skipped for this error.
///
/// The fallback path issues a second in-flight generation on local providers.
pub fn blocks_streaming_fallback(provider: &dyn LLMProvider, error: &LlmError) -> bool {
    blocks_transport_retry(provider, error)
}

/// Shelf notice when a local provider transport call stalls or times out.
pub fn transport_stall_user_notice(provider: &dyn LLMProvider) -> String {
    edgecrab_tools::tool_progress_tail::format_local_transport_stall_notice(provider.name())
}

/// User-facing suffix appended to the final LLM error for local transport stalls.
pub fn transport_stall_error_suffix(provider_name: &str) -> Option<&'static str> {
    match provider_name {
        "lmstudio" => Some(
            "Local inference timed out — LM Studio may still be generating in the background. \
             Wait for the GEN counter to finish or restart LM Studio before retrying; EdgeCrab did \
             not start a duplicate request to avoid stacked generations.",
        ),
        "ollama" => Some(
            "Local inference timed out — Ollama may still be generating in the background. \
             Wait for the server to finish or restart Ollama before retrying; EdgeCrab did not \
             start a duplicate request to avoid stacked generations.",
        ),
        name if is_local_inference_provider(name) => Some(
            "Local inference timed out — the server may still be generating. Wait before \
             retrying; EdgeCrab did not start a duplicate request.",
        ),
        _ => None,
    }
}

/// HTTP timeout (seconds) for a local provider's chat/completions client.
pub fn local_http_timeout_secs(provider_name: &str) -> u64 {
    match provider_name {
        "lmstudio" => std::env::var("LMSTUDIO_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_LOCAL_HTTP_TIMEOUT_SECS),
        "ollama" => std::env::var("OLLAMA_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_LOCAL_HTTP_TIMEOUT_SECS),
        _ => DEFAULT_LOCAL_HTTP_TIMEOUT_SECS,
    }
}

/// Resolve absolute completion cap: env `EDGECRAB_LOCAL_TOOL_MAX_TOKENS` > config > compile-time default.
pub fn local_tool_turn_absolute_max_tokens(config_value: usize) -> usize {
    std::env::var("EDGECRAB_LOCAL_TOOL_MAX_TOKENS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(config_value)
}

/// Compile-time default when no session config is available (prompt assembly).
pub fn local_tool_turn_absolute_max_tokens_default() -> usize {
    local_tool_turn_absolute_max_tokens(
        edgecrab_tools::mutation_turn_policy::LOCAL_TOOL_TURN_ABS_MAX_TOKENS,
    )
}

/// Deterministic `max_tokens` for a local tool turn (DRY with pre-dispatch guards).
pub fn local_tool_turn_max_tokens(
    provider: &dyn LLMProvider,
    max_mutation_payload_bytes: usize,
    config_absolute_max: usize,
) -> usize {
    edgecrab_tools::mutation_turn_policy::output_token_budget_for_tool_turn(
        max_mutation_payload_bytes,
        provider,
        local_tool_turn_absolute_max_tokens(config_absolute_max),
    )
}

/// `tool_choice` for providers that must emit tool calls instead of prose on tool turns.
pub fn local_tool_choice(provider: &dyn LLMProvider, has_tools: bool) -> Option<ToolChoice> {
    if has_tools && is_local_inference_provider(provider.name()) {
        Some(ToolChoice::required())
    } else {
        None
    }
}

/// Diagnostics emitted around each local tool-turn API call (structured logs + shelf).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalToolTurnPlan {
    pub provider: String,
    pub model: String,
    pub max_tokens: usize,
    pub reasoning_effort: String,
    pub reasoning_overridden: bool,
    pub tool_choice_required: bool,
    pub max_mutation_payload_bytes: usize,
    pub absolute_max_tokens: usize,
    pub http_timeout_secs: u64,
    pub context_length: usize,
    pub prompt_tokens_estimated: usize,
    pub max_tool_argument_bytes: usize,
    pub non_streaming: bool,
}

impl LocalToolTurnPlan {
    /// One-line summary for `tracing` and activity shelf preflight.
    pub fn log_line(&self) -> String {
        format!(
            "local tool turn: {} / {} · max_tokens={} · max_arg={}B · reasoning={}{} · \
             tool_choice=required · ~{}k/{}k ctx · non-streaming · HTTP timeout {}s",
            self.provider,
            self.model,
            self.max_tokens,
            self.max_tool_argument_bytes,
            self.reasoning_effort,
            if self.reasoning_overridden { " (forced)" } else { "" },
            self.prompt_tokens_estimated / 1000,
            self.context_length / 1000,
            self.http_timeout_secs,
        )
    }
}

/// Build a plan for structured logging after [`effective_completion_options`].
pub fn local_tool_turn_plan(
    provider: &dyn LLMProvider,
    options: &CompletionOptions,
    prompt_tokens_estimated: usize,
    max_mutation_payload_bytes: usize,
    base_reasoning_effort: Option<&str>,
    config_absolute_max: usize,
) -> Option<LocalToolTurnPlan> {
    if !is_local_inference_provider(provider.name()) {
        return None;
    }
    let absolute = local_tool_turn_absolute_max_tokens(config_absolute_max);
    let max_tokens = options.max_tokens.unwrap_or_else(|| {
        local_tool_turn_max_tokens(provider, max_mutation_payload_bytes, config_absolute_max)
    });
    let reasoning_effort = options
        .reasoning_effort
        .clone()
        .unwrap_or_else(|| LOCAL_TOOL_TURN_REASONING_EFFORT.to_string());
    Some(LocalToolTurnPlan {
        provider: provider.name().to_string(),
        model: provider.model().to_string(),
        max_tokens,
        reasoning_overridden: base_reasoning_effort != Some(LOCAL_TOOL_TURN_REASONING_EFFORT),
        reasoning_effort,
        tool_choice_required: true,
        max_mutation_payload_bytes,
        absolute_max_tokens: absolute,
        http_timeout_secs: local_http_timeout_secs(provider.name()),
        context_length: provider.max_context_length(),
        prompt_tokens_estimated,
        max_tool_argument_bytes: edgecrab_tools::mutation_turn_policy::max_tool_argument_bytes(
            max_mutation_payload_bytes,
            Some(provider),
        ),
        non_streaming: prefers_nonstreaming_tool_turns(provider),
    })
}

/// Apply local-provider completion limits for tool turns.
pub fn effective_completion_options(
    base: &CompletionOptions,
    provider: &dyn LLMProvider,
    has_tools: bool,
    max_mutation_payload_bytes: usize,
    config_absolute_max: usize,
) -> CompletionOptions {
    if !has_tools || !is_local_inference_provider(provider.name()) {
        return base.clone();
    }
    let cap = local_tool_turn_max_tokens(provider, max_mutation_payload_bytes, config_absolute_max);
    let mut options = base.clone();
    options.max_tokens = Some(base.max_tokens.map(|tokens| tokens.min(cap)).unwrap_or(cap));

    // Always force none — Qwen3 / DeepSeek R1 default reasoning ON when omitted or set to null.
    options.reasoning_effort = Some(LOCAL_TOOL_TURN_REASONING_EFFORT.to_string());

    options
}

/// Log the local tool-turn plan at INFO (call from `execute_loop` before API request).
pub fn log_local_tool_turn_plan(plan: &LocalToolTurnPlan) {
    tracing::info!(
        target: "edgecrab::local_llm",
        provider = %plan.provider,
        model = %plan.model,
        max_tokens = plan.max_tokens,
        max_mutation_payload_bytes = plan.max_mutation_payload_bytes,
        absolute_max_tokens = plan.absolute_max_tokens,
        reasoning_effort = %plan.reasoning_effort,
        reasoning_overridden = plan.reasoning_overridden,
        tool_choice_required = plan.tool_choice_required,
        http_timeout_secs = plan.http_timeout_secs,
        context_length = plan.context_length,
        prompt_tokens_estimated = plan.prompt_tokens_estimated,
        non_streaming = plan.non_streaming,
        "local_llm: tool-turn plan"
    );
}

/// Metrics captured after a local LLM HTTP response (structured investigation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalLlmResponseMetrics {
    pub elapsed_ms: u64,
    pub finish_reason: Option<String>,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub thinking_tokens: Option<usize>,
    pub tool_call_count: usize,
    pub content_len: usize,
    pub has_reasoning_content: bool,
    pub max_tokens: Option<usize>,
    pub tool_choice_required: bool,
}

/// Log a completed local LLM response at INFO.
pub fn log_local_llm_response(provider: &dyn LLMProvider, metrics: &LocalLlmResponseMetrics) {
    if !is_local_inference_provider(provider.name()) {
        return;
    }
    tracing::info!(
        target: "edgecrab::local_llm",
        provider = provider.name(),
        model = provider.model(),
        elapsed_ms = metrics.elapsed_ms,
        finish_reason = metrics.finish_reason.as_deref().unwrap_or("unknown"),
        prompt_tokens = metrics.prompt_tokens,
        completion_tokens = metrics.completion_tokens,
        thinking_tokens = metrics.thinking_tokens.unwrap_or(0),
        tool_call_count = metrics.tool_call_count,
        content_len = metrics.content_len,
        has_reasoning_content = metrics.has_reasoning_content,
        max_tokens = metrics.max_tokens,
        tool_choice_required = metrics.tool_choice_required,
        "local_llm: request complete"
    );
}

/// Deterministic prefill prune threshold for local tool turns (not failure-adaptive).
///
/// Formula: `min(LOCAL_PREFILL_PRUNE_TOKEN_BUDGET, active_context_length / 8)` when context
/// is known; otherwise `LOCAL_PREFILL_PRUNE_TOKEN_BUDGET`. Override via
/// `EDGECRAB_LOCAL_PREFILL_PRUNE_TOKENS`.
pub fn local_prefill_prune_token_budget(active_context_length: usize) -> usize {
    std::env::var("EDGECRAB_LOCAL_PREFILL_PRUNE_TOKENS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| {
            if active_context_length == 0 {
                LOCAL_PREFILL_PRUNE_TOKEN_BUDGET
            } else {
                LOCAL_PREFILL_PRUNE_TOKEN_BUDGET
                    .min(active_context_length / LOCAL_PREFILL_CONTEXT_DIVISOR)
            }
        })
}

/// Whether to run cheap structural prune/spill before a local tool-turn API call.
pub fn should_structural_prefill_prune(
    estimated_prompt_tokens: usize,
    active_context_length: usize,
) -> bool {
    estimated_prompt_tokens > local_prefill_prune_token_budget(active_context_length)
}

/// Phase for local structural tool-output prune (preflight threshold vs length-recovery force).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalStructuralPrunePhase {
    /// Before API when prompt exceeds deterministic prefill budget.
    Preflight,
    /// After `finish_reason=length` with no tool_calls — always attempt prune.
    LengthRecovery,
}

/// Whether this phase should attempt structural prune for the given prompt estimate.
pub fn gate_local_structural_prune(
    phase: LocalStructuralPrunePhase,
    estimated_prompt_tokens: usize,
    active_context_length: usize,
) -> bool {
    match phase {
        LocalStructuralPrunePhase::Preflight => {
            should_structural_prefill_prune(estimated_prompt_tokens, active_context_length)
        }
        LocalStructuralPrunePhase::LengthRecovery => true,
    }
}

/// Gate + apply structural tool-output prune; returns `None` when skipped or nothing to prune.
pub fn try_apply_structural_tool_output_prune(
    phase: LocalStructuralPrunePhase,
    estimated_prompt_tokens: usize,
    active_context_length: usize,
    messages: &[edgecrab_types::Message],
    spill_ctx: Option<&crate::compression::PruneSpillContext<'_>>,
) -> Option<(Vec<edgecrab_types::Message>, crate::compression::StructuralPruneOutcome)> {
    if !gate_local_structural_prune(phase, estimated_prompt_tokens, active_context_length) {
        return None;
    }
    crate::compression::apply_structural_tool_output_prune(messages, spill_ctx)
}

/// Token threshold for local mid-band structural compress (no LLM).
pub fn local_structural_compress_token_threshold(active_context_length: usize) -> usize {
    if active_context_length == 0 {
        return 0;
    }
    (active_context_length as f32 * local_structural_compress_threshold_ratio()) as usize
}

/// Whether to run `compress_structural_only` on local tool turns (between prefill and LLM compress).
pub fn should_local_structural_compress(
    estimated_prompt_tokens: usize,
    active_context_length: usize,
    llm_compress_threshold_tokens: usize,
) -> bool {
    let mid = local_structural_compress_token_threshold(active_context_length);
    mid > 0
        && estimated_prompt_tokens > mid
        && estimated_prompt_tokens < llm_compress_threshold_tokens
}

/// Apply mid-band structural compress when gated; returns `None` if skipped or no shrink.
pub fn try_local_midband_structural_compress(
    messages: &[edgecrab_types::Message],
    compression_params: &crate::compression::CompressionParams,
    active_context_length: usize,
    estimated_prompt_tokens: usize,
    spill_ctx: Option<&crate::compression::PruneSpillContext<'_>>,
) -> Option<(Vec<edgecrab_types::Message>, usize, usize)> {
    let llm_threshold =
        (compression_params.context_window as f32 * compression_params.threshold) as usize;
    if !should_local_structural_compress(
        estimated_prompt_tokens,
        active_context_length,
        llm_threshold,
    ) {
        return None;
    }
    let tokens_before = crate::compression::estimate_tokens(messages);
    let compressed = crate::compression::compress_structural_only(messages, compression_params, spill_ctx);
    let tokens_after = crate::compression::estimate_tokens(&compressed);
    if tokens_after >= tokens_before {
        return None;
    }
    Some((compressed, tokens_before, tokens_after))
}

/// Log local mid-band structural compress at INFO.
pub fn log_local_structural_compress(
    provider: &dyn LLMProvider,
    tokens_before: usize,
    tokens_after: usize,
) {
    if !is_local_inference_provider(provider.name()) {
        return;
    }
    tracing::info!(
        target: "edgecrab::local_llm",
        provider = provider.name(),
        model = provider.model(),
        tokens_before,
        tokens_after,
        mid_band_threshold = local_structural_compress_token_threshold(provider.max_context_length()),
        "local_llm: mid-band structural compress"
    );
}

/// Log structural prefill prune at INFO (`reason`: `"preflight"` | `"length_recovery"`).
pub fn log_local_prefill_prune(
    provider: &dyn LLMProvider,
    tokens_before: usize,
    tokens_after: usize,
    tools_pruned: usize,
    reason: &str,
) {
    if !is_local_inference_provider(provider.name()) {
        return;
    }
    tracing::info!(
        target: "edgecrab::local_llm",
        provider = provider.name(),
        model = provider.model(),
        tokens_before,
        tokens_after,
        tools_pruned,
        reason,
        prefill_budget = local_prefill_prune_token_budget(provider.max_context_length()),
        "local_llm: structural prefill prune"
    );
}

/// Log finish_reason=length with no tool_calls (structured investigation event).
pub fn log_local_tool_length_failure(
    provider: &dyn LLMProvider,
    metrics: &LocalLlmResponseMetrics,
) {
    if !is_local_inference_provider(provider.name()) {
        return;
    }
    tracing::warn!(
        target: "edgecrab::local_llm",
        provider = provider.name(),
        model = provider.model(),
        finish_reason = "length",
        completion_tokens = metrics.completion_tokens,
        thinking_tokens = metrics.thinking_tokens.unwrap_or(0),
        max_tokens = metrics.max_tokens.unwrap_or(0),
        prompt_tokens = metrics.prompt_tokens,
        content_len = metrics.content_len,
        has_reasoning_content = metrics.has_reasoning_content,
        tool_choice_required = metrics.tool_choice_required,
        "local_llm: max_tokens exhausted without tool_calls — incremental-edit recovery"
    );
}

/// Log a local transport failure at WARN/ERROR.
pub fn log_local_llm_transport_failure(
    provider: &dyn LLMProvider,
    elapsed_ms: u64,
    attempt: u32,
    error: &str,
) {
    if !is_local_inference_provider(provider.name()) {
        return;
    }
    tracing::warn!(
        target: "edgecrab::local_llm",
        provider = provider.name(),
        model = provider.model(),
        elapsed_ms,
        attempt,
        error,
        http_timeout_secs = local_http_timeout_secs(provider.name()),
        will_retry = false,
        "local_llm: transport failure (no retry — avoid duplicate GEN)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;

    struct NamedProvider {
        name: &'static str,
        context_length: usize,
        default_output: Option<usize>,
    }

    impl NamedProvider {
        fn lmstudio(context_length: usize, default_output: Option<usize>) -> Self {
            Self {
                name: "lmstudio",
                context_length,
                default_output,
            }
        }
    }

    #[async_trait]
    impl LLMProvider for NamedProvider {
        fn name(&self) -> &str {
            self.name
        }

        fn model(&self) -> &str {
            "test-model"
        }

        fn max_context_length(&self) -> usize {
            self.context_length
        }

        fn default_max_output_tokens(&self) -> Option<usize> {
            self.default_output
        }

        async fn complete(
            &self,
            prompt: &str,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            Ok(edgequake_llm::LLMResponse::new(prompt, self.model()))
        }

        async fn complete_with_options(
            &self,
            prompt: &str,
            _options: &CompletionOptions,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            self.complete(prompt).await
        }

        async fn chat(
            &self,
            messages: &[edgequake_llm::ChatMessage],
            _options: Option<&CompletionOptions>,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            Ok(edgequake_llm::LLMResponse::new(
                messages
                    .last()
                    .map(|message| message.content.as_str())
                    .unwrap_or(""),
                self.model(),
            ))
        }
    }

    const TEST_MUTATION_BYTES: usize = 32 * 1024;
    const TEST_ABS_MAX: usize = edgecrab_tools::mutation_turn_policy::LOCAL_TOOL_TURN_ABS_MAX_TOKENS;

    #[test]
    fn lh60_local_tool_turn_absolute_max_tokens_honors_config_before_default() {
        let custom = 12_288;
        assert_eq!(
            super::local_tool_turn_absolute_max_tokens(custom),
            custom
        );
    }

    #[test]
    fn local_provider_detection() {
        assert!(is_local_inference_provider("lmstudio"));
        assert!(is_local_inference_provider("ollama"));
        assert!(!is_local_inference_provider("anthropic"));
    }

    #[test]
    fn effective_local_write_create_dirs_default_on_for_local() {
        assert!(effective_local_write_create_dirs(false, "lmstudio/qwen"));
        assert!(effective_local_write_create_dirs(false, "ollama"));
        assert!(effective_local_write_create_dirs(true, "anthropic/claude"));
        assert!(!effective_local_write_create_dirs(false, "anthropic/claude"));
    }

    #[test]
    fn local_tool_harness_requires_tools() {
        assert!(local_tool_harness_active("lmstudio", true));
        assert!(!local_tool_harness_active("lmstudio", false));
        assert!(!local_tool_harness_active("anthropic", true));
    }

    #[test]
    fn blocks_transport_retry_only_for_local_timeout_and_network() {
        let lmstudio: Arc<dyn LLMProvider> = Arc::new(NamedProvider::lmstudio(8192, None));
        let openai: Arc<dyn LLMProvider> = Arc::new(NamedProvider {
            name: "openai",
            context_length: 8192,
            default_output: None,
        });

        assert!(blocks_transport_retry(
            lmstudio.as_ref(),
            &LlmError::Timeout
        ));
        assert!(blocks_transport_retry(
            lmstudio.as_ref(),
            &LlmError::NetworkError("reset".into())
        ));
        assert!(!blocks_transport_retry(
            lmstudio.as_ref(),
            &LlmError::RateLimited("429".into())
        ));
        assert!(!blocks_transport_retry(
            openai.as_ref(),
            &LlmError::Timeout
        ));
    }

    #[test]
    fn caps_local_tool_turn_max_tokens_and_forces_reasoning_none() {
        let synced: Arc<dyn LLMProvider> = Arc::new(NamedProvider::lmstudio(65_536, Some(8192)));
        let base = CompletionOptions {
            max_tokens: Some(16_384),
            reasoning_effort: Some("high".into()),
            ..Default::default()
        };
        let capped = effective_completion_options(
            &base,
            synced.as_ref(),
            true,
            TEST_MUTATION_BYTES,
            TEST_ABS_MAX,
        );
        assert_eq!(
            capped.max_tokens,
            Some(local_tool_turn_max_tokens(synced.as_ref(), TEST_MUTATION_BYTES, TEST_ABS_MAX))
        );
        assert_eq!(
            capped.reasoning_effort.as_deref(),
            Some(LOCAL_TOOL_TURN_REASONING_EFFORT)
        );

        let already_small = CompletionOptions {
            max_tokens: Some(512),
            ..Default::default()
        };
        let kept = effective_completion_options(
            &already_small,
            synced.as_ref(),
            true,
            TEST_MUTATION_BYTES,
            TEST_ABS_MAX,
        );
        assert_eq!(kept.max_tokens, Some(512));
        assert_eq!(
            kept.reasoning_effort.as_deref(),
            Some(LOCAL_TOOL_TURN_REASONING_EFFORT)
        );

        let plain = effective_completion_options(&base, synced.as_ref(), false, TEST_MUTATION_BYTES, TEST_ABS_MAX);
        assert_eq!(plain.max_tokens, Some(16_384));
        assert_eq!(plain.reasoning_effort.as_deref(), Some("high"));
    }

    #[test]
    fn always_forces_reasoning_none_even_when_user_pinned() {
        let provider: Arc<dyn LLMProvider> = Arc::new(NamedProvider::lmstudio(8192, None));
        let base = CompletionOptions {
            reasoning_effort: Some("none".into()),
            ..Default::default()
        };
        let options = effective_completion_options(
            &base,
            provider.as_ref(),
            true,
            TEST_MUTATION_BYTES,
            TEST_ABS_MAX,
        );
        assert_eq!(
            options.reasoning_effort.as_deref(),
            Some(LOCAL_TOOL_TURN_REASONING_EFFORT)
        );
        let plan = local_tool_turn_plan(
            provider.as_ref(),
            &options,
            50_000,
            TEST_MUTATION_BYTES,
            Some("none"),
            TEST_ABS_MAX,
        )
        .expect("plan");
        assert!(!plan.reasoning_overridden);
        assert!(plan.tool_choice_required);
    }

    #[test]
    fn overrides_high_reasoning_on_local_tool_turns() {
        let provider: Arc<dyn LLMProvider> = Arc::new(NamedProvider::lmstudio(8192, None));
        let base = CompletionOptions {
            reasoning_effort: Some("high".into()),
            ..Default::default()
        };
        let options = effective_completion_options(
            &base,
            provider.as_ref(),
            true,
            TEST_MUTATION_BYTES,
            TEST_ABS_MAX,
        );
        let plan = local_tool_turn_plan(
            provider.as_ref(),
            &options,
            50_000,
            TEST_MUTATION_BYTES,
            Some("high"),
            TEST_ABS_MAX,
        )
        .expect("plan");
        assert!(plan.reasoning_overridden);
    }

    #[test]
    fn local_tool_turn_plan_includes_timeout_and_context() {
        let provider: Arc<dyn LLMProvider> = Arc::new(NamedProvider::lmstudio(262_144, Some(8192)));
        let options = effective_completion_options(
            &CompletionOptions::default(),
            provider.as_ref(),
            true,
            TEST_MUTATION_BYTES,
            TEST_ABS_MAX,
        );
        let plan = local_tool_turn_plan(
            provider.as_ref(),
            &options,
            50_000,
            TEST_MUTATION_BYTES,
            None,
            TEST_ABS_MAX,
        )
        .expect("plan");
        assert_eq!(
            plan.max_tokens,
            local_tool_turn_max_tokens(provider.as_ref(), TEST_MUTATION_BYTES, TEST_ABS_MAX)
        );
        assert_eq!(plan.context_length, 262_144);
        assert_eq!(plan.http_timeout_secs, DEFAULT_LOCAL_HTTP_TIMEOUT_SECS);
        assert!(plan.log_line().contains("tool_choice=required"));
        assert!(plan.log_line().contains("reasoning=none"));
        assert!(plan.log_line().contains("max_arg="));
    }

    #[test]
    fn local_tool_choice_required_for_local_tool_turns() {
        let lmstudio: Arc<dyn LLMProvider> = Arc::new(NamedProvider::lmstudio(8192, None));
        let openai: Arc<dyn LLMProvider> = Arc::new(NamedProvider {
            name: "openai",
            context_length: 8192,
            default_output: None,
        });
        assert!(matches!(
            local_tool_choice(lmstudio.as_ref(), true),
            Some(ToolChoice::Required(_))
        ));
        assert!(local_tool_choice(lmstudio.as_ref(), false).is_none());
        assert!(local_tool_choice(openai.as_ref(), true).is_none());
    }

    #[test]
    fn prefers_nonstreaming_for_local_and_copilot() {
        let lmstudio: Arc<dyn LLMProvider> = Arc::new(NamedProvider::lmstudio(8192, None));
        let copilot: Arc<dyn LLMProvider> = Arc::new(NamedProvider {
            name: "vscode-copilot",
            context_length: 8192,
            default_output: None,
        });
        let openai: Arc<dyn LLMProvider> = Arc::new(NamedProvider {
            name: "openai",
            context_length: 8192,
            default_output: None,
        });

        assert!(prefers_nonstreaming_tool_turns(lmstudio.as_ref()));
        assert!(prefers_nonstreaming_tool_turns(copilot.as_ref()));
        assert!(!prefers_nonstreaming_tool_turns(openai.as_ref()));
    }

    #[test]
    fn transport_stall_notice_mentions_provider() {
        let lmstudio: Arc<dyn LLMProvider> = Arc::new(NamedProvider::lmstudio(8192, None));
        let notice = transport_stall_user_notice(lmstudio.as_ref());
        assert!(notice.contains("lmstudio"));
        assert!(notice.contains("GEN"));
    }

    #[test]
    fn prefill_prune_budget_is_min_of_cap_and_context_eighth() {
        assert_eq!(
            local_prefill_prune_token_budget(262_144),
            LOCAL_PREFILL_PRUNE_TOKEN_BUDGET.min(262_144 / LOCAL_PREFILL_CONTEXT_DIVISOR)
        );
        assert_eq!(local_prefill_prune_token_budget(180_000), 22_500);
        assert_eq!(
            local_prefill_prune_token_budget(0),
            LOCAL_PREFILL_PRUNE_TOKEN_BUDGET
        );
    }

    #[test]
    fn should_prefill_prune_when_prompt_exceeds_budget() {
        let synced_ctx = 262_144;
        let budget = local_prefill_prune_token_budget(synced_ctx);
        assert_eq!(budget, 32_000);
        assert!(!should_structural_prefill_prune(budget, synced_ctx));
        assert!(should_structural_prefill_prune(budget + 1, synced_ctx));
        // Homelab mid-band (~37k) and research (~46k) both trigger preflight.
        assert!(should_structural_prefill_prune(37_000, synced_ctx));
        assert!(should_structural_prefill_prune(46_000, synced_ctx));
        assert!(!should_structural_prefill_prune(30_000, synced_ctx));
    }

    #[test]
    fn length_recovery_gate_always_attempts_prune() {
        let synced_ctx = 262_144;
        assert!(gate_local_structural_prune(
            LocalStructuralPrunePhase::LengthRecovery,
            37_000,
            synced_ctx,
        ));
        assert!(gate_local_structural_prune(
            LocalStructuralPrunePhase::Preflight,
            37_000,
            synced_ctx,
        ));
    }

    #[test]
    fn try_apply_length_recovery_prune_reclaims_fat_tool_outputs() {
        use crate::compression::{count_long_tool_outputs, estimate_tokens};
        use edgecrab_types::Message;

        let messages: Vec<Message> = (0..8)
            .map(|i| {
                Message::tool_result(
                    &format!("id{i}"),
                    "web_extract",
                    &format!("body {i}\n{}", "x".repeat(15_400)),
                )
            })
            .collect();
        let before = estimate_tokens(&messages);
        assert!(before < local_prefill_prune_token_budget(262_144));
        assert!(!gate_local_structural_prune(
            LocalStructuralPrunePhase::Preflight,
            before,
            262_144,
        ));

        let (pruned, outcome) = try_apply_structural_tool_output_prune(
            LocalStructuralPrunePhase::LengthRecovery,
            before,
            262_144,
            &messages,
            None,
        )
        .expect("length recovery must prune fat tool outputs");
        assert_eq!(outcome.long_tool_outputs_remaining, 0);
        assert_eq!(count_long_tool_outputs(&pruned), 0);
        assert!(outcome.message_tokens_after < outcome.message_tokens_before);
    }

    #[test]
    fn should_local_structural_compress_mid_band_only() {
        let ctx = 262_144;
        let mid = local_structural_compress_token_threshold(ctx);
        assert_eq!(mid, 52_428);
        let llm = (ctx as f32 * 0.5) as usize;
        assert!(should_local_structural_compress(58_000, ctx, llm));
        assert!(!should_local_structural_compress(40_000, ctx, llm));
        assert!(!should_local_structural_compress(140_000, ctx, llm));
    }

    #[test]
    fn local_tool_turn_plan_includes_max_arg_bytes() {
        let provider: Arc<dyn LLMProvider> = Arc::new(NamedProvider::lmstudio(262_144, Some(8192)));
        let options = effective_completion_options(
            &CompletionOptions::default(),
            provider.as_ref(),
            true,
            TEST_MUTATION_BYTES,
            TEST_ABS_MAX,
        );
        let plan = local_tool_turn_plan(
            provider.as_ref(),
            &options,
            50_000,
            TEST_MUTATION_BYTES,
            None,
            TEST_ABS_MAX,
        )
        .expect("plan");
        let expected = edgecrab_tools::mutation_turn_policy::max_tool_argument_bytes(
            TEST_MUTATION_BYTES,
            Some(provider.as_ref()),
        );
        assert_eq!(plan.max_tool_argument_bytes, expected);
        assert!(plan.log_line().contains(&format!("max_arg={expected}B")));
    }
}
