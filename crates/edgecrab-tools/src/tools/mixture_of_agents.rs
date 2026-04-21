//! # moa — Multi-model consensus reasoning
//!
//! WHY MoA: Complex reasoning tasks benefit from diverse LLM perspectives.
//! By running the same prompt through multiple frontier models in parallel
//! and then synthesizing with an aggregator, we achieve higher accuracy
//! than any single model for hard problems.
//!
//! Based on: "Mixture-of-Agents Enhances Large Language Model Capabilities"
//! (Wang et al., arXiv:2406.04692)
//!
//! ```text
//!   moa(user_prompt)
//!       │
//!       ├── spawn N reference model tasks in parallel (tokio::spawn)
//!       │   ├── model_a.chat([user_prompt]) → response_a
//!       │   ├── model_b.chat([user_prompt]) → response_b
//!       │   └── model_c.chat([user_prompt]) → response_c
//!       │
//!       └── aggregator.chat([system: collected_responses, user: prompt])
//!               → final_synthesized_response
//! ```
//!
//! The aggregator system prompt is derived from the original research paper.
//! Models that fail (rate limit, error) are skipped if at least 1 succeeds.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::Arc;

use edgecrab_types::{ToolError, ToolSchema};
use edgequake_llm::traits::{CacheControl, ChatMessage, CompletionOptions};

use crate::registry::{ToolContext, ToolHandler};
use crate::vision_models::{
    normalize_model_name, normalize_provider_name, parse_provider_model_spec,
};

// ─── Default configuration ─────────────────────────────────────────────────

/// Reference models that provide diverse initial responses.
/// These must be available via the active provider (gateway/openrouter).
pub const DEFAULT_REFERENCE_MODELS: &[&str] = &[
    "anthropic/claude-opus-4.6",
    "google/gemini-2.5-pro",
    "openai/gpt-4.1",
    "deepseek/deepseek-r1",
];

/// Aggregator model — synthesizes reference responses.
pub const DEFAULT_AGGREGATOR_MODEL: &str = "anthropic/claude-opus-4.6";
pub const MOA_TOOL_NAME: &str = "moa";
pub const LEGACY_MOA_TOOL_NAME: &str = "mixture_of_agents";

pub fn default_reference_models() -> Vec<String> {
    DEFAULT_REFERENCE_MODELS
        .iter()
        .map(|model| (*model).to_string())
        .collect()
}

pub fn normalize_moa_model_spec(spec: &str) -> Option<String> {
    let (provider, model) = parse_provider_model_spec(spec)?;
    Some(format!("{provider}/{model}"))
}

pub fn normalize_reference_models(models: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut normalized = Vec::new();

    for model in models {
        let Some(spec) = normalize_moa_model_spec(model) else {
            continue;
        };
        if seen.insert(spec.clone()) {
            normalized.push(spec);
        }
    }

    normalized
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveMoaConfig {
    pub enabled: bool,
    pub reference_models: Vec<String>,
    pub aggregator_model: String,
}

pub fn sanitize_moa_config(
    enabled: bool,
    reference_models: &[String],
    aggregator_model: &str,
) -> EffectiveMoaConfig {
    let reference_models = {
        let normalized = normalize_reference_models(reference_models);
        if normalized.is_empty() {
            default_reference_models()
        } else {
            normalized
        }
    };

    let aggregator_model = normalize_moa_model_spec(aggregator_model)
        .unwrap_or_else(|| DEFAULT_AGGREGATOR_MODEL.to_string());

    EffectiveMoaConfig {
        enabled,
        reference_models,
        aggregator_model,
    }
}

pub fn recommended_moa_config_for_model_spec(active_model_spec: &str) -> EffectiveMoaConfig {
    if let Some(active_model_spec) = normalize_moa_model_spec(active_model_spec) {
        return EffectiveMoaConfig {
            enabled: true,
            reference_models: vec![active_model_spec.clone()],
            aggregator_model: active_model_spec,
        };
    }

    sanitize_moa_config(true, &[], DEFAULT_AGGREGATOR_MODEL)
}

fn resolve_effective_moa_config(
    args: &MoAArgs,
    ctx: &ToolContext,
) -> Result<EffectiveMoaConfig, ToolError> {
    let configured = sanitize_moa_config(
        ctx.config.moa_enabled,
        &ctx.config.moa_reference_models,
        ctx.config
            .moa_aggregator_model
            .as_deref()
            .unwrap_or(DEFAULT_AGGREGATOR_MODEL),
    );

    if !configured.enabled {
        return Err(ToolError::Unavailable {
            tool: MOA_TOOL_NAME.into(),
            reason: "Mixture-of-Agents is disabled in config (`moa.enabled: false`)".into(),
        });
    }

    let reference_models = match &args.reference_models {
        Some(models) => {
            let normalized = normalize_reference_models(models);
            if normalized.is_empty() {
                return Err(ToolError::InvalidArgs {
                    tool: MOA_TOOL_NAME.into(),
                    message:
                        "reference_models must contain at least one valid provider/model entry"
                            .into(),
                });
            }
            normalized
        }
        None => configured.reference_models,
    };

    let aggregator_model = match &args.aggregator_model {
        Some(model) => normalize_moa_model_spec(model).ok_or_else(|| ToolError::InvalidArgs {
            tool: MOA_TOOL_NAME.into(),
            message: "aggregator_model must be a valid provider/model entry".into(),
        })?,
        None => configured.aggregator_model,
    };

    Ok(EffectiveMoaConfig {
        enabled: true,
        reference_models,
        aggregator_model,
    })
}

/// Temperature for reference models — higher for diversity.
const REFERENCE_TEMPERATURE: f32 = 0.6;

/// Temperature for aggregator — lower for focused synthesis.
const AGGREGATOR_TEMPERATURE: f32 = 0.4;

/// Minimum successful reference responses before aggregation.
const MIN_SUCCESSFUL_REFERENCES: usize = 1;

/// Maximum tokens per reference model response.
const REFERENCE_MAX_TOKENS: usize = 8192;

/// Aggregator system prompt from the research paper.
const AGGREGATOR_SYSTEM_PROMPT: &str = "You have been provided with a set of responses from \
    various open-source models to the latest user query. Your task is to synthesize these \
    responses into a single, high-quality response. It is crucial to critically evaluate the \
    information provided in these responses, recognizing that some of it may be biased or \
    incorrect. Your response should not simply replicate the given answers but should offer a \
    refined, accurate, and comprehensive reply to the instruction. Ensure your response is \
    well-structured, coherent, and adheres to the highest standards of accuracy and reliability.\n\n\
    Responses from models:";

// ─── Tool struct ───────────────────────────────────────────────────────────

pub struct MixtureOfAgentsTool;

#[derive(Deserialize)]
struct MoAArgs {
    /// The complex question or task to process through multiple models
    user_prompt: String,
    /// Optional override of reference models (must be available via provider)
    #[serde(default)]
    reference_models: Option<Vec<String>>,
    /// Optional override of aggregator model
    #[serde(default)]
    aggregator_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProviderRequest {
    provider: String,
    model: String,
    reuse_active: bool,
}

fn build_provider_request(
    active_provider_name: &str,
    active_model: &str,
    target_spec: &str,
) -> Result<ProviderRequest, String> {
    let active_provider = normalize_provider_name(active_provider_name);
    let active_model = normalize_model_name(&active_provider, active_model);
    let (target_provider, target_model) = parse_provider_model_spec(target_spec)
        .ok_or_else(|| format!("invalid model spec: {target_spec}"))?;

    let (provider, model) = if active_provider == "openrouter" {
        let routed_model = if target_provider == "openrouter" {
            target_model.clone()
        } else {
            format!("{target_provider}/{target_model}")
        };
        ("openrouter".to_string(), routed_model)
    } else if target_provider == "openrouter" {
        ("openrouter".to_string(), target_model.clone())
    } else {
        (target_provider.clone(), target_model.clone())
    };

    let normalized_request_model = normalize_model_name(&provider, &model);
    let reuse_active = active_provider == provider && active_model == normalized_request_model;

    Ok(ProviderRequest {
        provider,
        model,
        reuse_active,
    })
}

fn provider_for_request(
    active_provider: &Arc<dyn edgequake_llm::LLMProvider>,
    request: &ProviderRequest,
) -> Result<Arc<dyn edgequake_llm::LLMProvider>, String> {
    if request.reuse_active {
        return Ok(Arc::clone(active_provider));
    }

    crate::create_provider_for_model(&request.provider, &request.model).map_err(|e| {
        format!(
            "failed to create provider {} for model {}: {}",
            request.provider, request.model, e
        )
    })
}

fn active_model_spec(active_provider_name: &str, active_model: &str) -> Option<String> {
    normalize_moa_model_spec(&format!(
        "{}/{}",
        normalize_provider_name(active_provider_name),
        active_model.trim()
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReferenceExecutionPlan {
    models: Vec<String>,
    implicit_fallback_model: Option<String>,
}

fn build_reference_execution_plan(
    configured_reference_models: &[String],
    active_provider_name: &str,
    active_model: &str,
) -> ReferenceExecutionPlan {
    let mut models = configured_reference_models.to_vec();
    let active_model_spec = active_model_spec(active_provider_name, active_model);
    let mut implicit_fallback_model = None;

    if let Some(active_model_spec) = active_model_spec
        && !models.iter().any(|model| model == &active_model_spec)
    {
        models.push(active_model_spec.clone());
        implicit_fallback_model = Some(active_model_spec);
    }

    ReferenceExecutionPlan {
        models,
        implicit_fallback_model,
    }
}

fn build_aggregator_candidates(
    requested_aggregator_model: &str,
    active_provider_name: &str,
    active_model: &str,
    successful_responses: &[(String, String)],
) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();

    for candidate in std::iter::once(requested_aggregator_model.to_string())
        .chain(active_model_spec(active_provider_name, active_model))
        .chain(successful_responses.iter().map(|(model, _)| model.clone()))
    {
        if seen.insert(candidate.clone()) {
            candidates.push(candidate);
        }
    }

    candidates
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AggregatorExecutionResult {
    requested_model: String,
    used_model: String,
    content: String,
    failures: Vec<String>,
}

#[derive(Clone)]
struct MoaProgressReporter {
    tool_call_id: Option<String>,
    tool_name: Option<String>,
    tx: Option<tokio::sync::mpsc::UnboundedSender<crate::ToolProgressUpdate>>,
}

impl MoaProgressReporter {
    fn from_ctx(ctx: &ToolContext) -> Self {
        Self {
            tool_call_id: ctx.current_tool_call_id.clone(),
            tool_name: ctx.current_tool_name.clone(),
            tx: ctx.tool_progress_tx.clone(),
        }
    }

    fn emit(&self, message: impl Into<String>) {
        let Some(tx) = &self.tx else {
            return;
        };
        let Some(tool_call_id) = &self.tool_call_id else {
            return;
        };
        let Some(tool_name) = &self.tool_name else {
            return;
        };
        let message = message.into();
        if message.trim().is_empty() {
            return;
        }
        let _ = tx.send(crate::ToolProgressUpdate {
            tool_call_id: tool_call_id.clone(),
            tool_name: tool_name.clone(),
            message,
        });
    }
}

async fn run_aggregator_with_fallbacks(
    request: AggregatorRunRequest<'_>,
) -> Result<AggregatorExecutionResult, ToolError> {
    let candidates = build_aggregator_candidates(
        request.requested_aggregator_model,
        request.active_provider_name,
        request.active_model,
        request.successful_responses,
    );
    let mut failures = Vec::new();

    for (idx, candidate) in candidates.iter().enumerate() {
        if idx == 0 {
            request
                .progress
                .emit(format!("aggregating with {candidate}"));
        } else {
            request.progress.emit(format!(
                "aggregator fallback: trying {candidate} after earlier failure"
            ));
        }
        let provider_request = match build_provider_request(
            request.active_provider_name,
            request.active_model,
            candidate,
        ) {
            Ok(request) => request,
            Err(err) => {
                failures.push(format!("{candidate} (setup failed: {err})"));
                continue;
            }
        };

        let aggregator_provider =
            match provider_for_request(request.active_provider, &provider_request) {
                Ok(provider) => provider,
                Err(err) => {
                    failures.push(format!("{candidate} (setup failed: {err})"));
                    continue;
                }
            };

        let mut agg_system_msg = ChatMessage::system(request.aggregator_system);
        agg_system_msg.cache_control = Some(CacheControl::ephemeral());

        let agg_messages = vec![agg_system_msg, ChatMessage::user(request.prompt)];
        let agg_options = CompletionOptions {
            temperature: Some(AGGREGATOR_TEMPERATURE),
            ..Default::default()
        };

        match aggregator_provider
            .chat(&agg_messages, Some(&agg_options))
            .await
        {
            Ok(response) => {
                let content = response.content.trim().to_string();
                if content.is_empty() {
                    failures.push(format!("{candidate} (returned empty content)"));
                    continue;
                }
                return Ok(AggregatorExecutionResult {
                    requested_model: request.requested_aggregator_model.to_string(),
                    used_model: candidate.clone(),
                    content,
                    failures,
                });
            }
            Err(err) => failures.push(format!("{candidate} ({err})")),
        }
    }

    Err(ToolError::ExecutionFailed {
        tool: MOA_TOOL_NAME.into(),
        message: format!(
            "All aggregator candidates failed. Requested aggregator: {}. Tried: {}. Errors: {}",
            request.requested_aggregator_model,
            candidates.join(", "),
            failures.join(", ")
        ),
    })
}

struct AggregatorRunRequest<'a> {
    active_provider: &'a Arc<dyn edgequake_llm::LLMProvider>,
    requested_aggregator_model: &'a str,
    active_provider_name: &'a str,
    active_model: &'a str,
    prompt: &'a str,
    aggregator_system: &'a str,
    successful_responses: &'a [(String, String)],
    progress: &'a MoaProgressReporter,
}

#[async_trait]
impl ToolHandler for MixtureOfAgentsTool {
    fn name(&self) -> &'static str {
        MOA_TOOL_NAME
    }

    fn aliases(&self) -> &'static [&'static str] {
        &[LEGACY_MOA_TOOL_NAME]
    }

    fn toolset(&self) -> &'static str {
        "moa"
    }

    fn emoji(&self) -> &'static str {
        "🧠"
    }

    fn check_fn(&self, ctx: &ToolContext) -> bool {
        ctx.config.moa_enabled
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: MOA_TOOL_NAME.into(),
            description:
                "MoA (Mixture of Agents): process a complex query using multiple frontier \
                LLMs in parallel, then synthesize their responses with an aggregator model. \
                Produces higher-quality output than any single model for hard reasoning, math, \
                coding, and analysis tasks. Use when: (1) a task is genuinely difficult and you \
                want a second or third opinion, (2) you need consensus across models, (3) \
                single-model answers feel uncertain. Requires a provider that supports model \
                routing (for example OpenRouter)."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "user_prompt": {
                        "type": "string",
                        "description": "The complex question or task to route through multiple models. \
                            Be specific and complete — the same prompt is sent verbatim to each reference model."
                    },
                    "reference_models": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional override list of reference model IDs. Defaults to \
                            claude-opus-4, gemini-2.5-pro, gpt-4.1, deepseek-r1."
                    },
                    "aggregator_model": {
                        "type": "string",
                        "description": "Optional override for the aggregator model ID. \
                            Defaults to claude-opus-4.6."
                    }
                },
                "required": ["user_prompt"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        // Always available — providers may reject unsupported models at runtime
        true
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: MoAArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: MOA_TOOL_NAME.into(),
            message: e.to_string(),
        })?;

        if args.user_prompt.trim().is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: MOA_TOOL_NAME.into(),
                message: "user_prompt must not be empty".into(),
            });
        }

        let provider = ctx
            .provider
            .as_ref()
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: MOA_TOOL_NAME.into(),
                message: "No LLM provider available. moa requires a provider.".into(),
            })?;

        let effective = resolve_effective_moa_config(&args, ctx)?;
        let reference_plan = build_reference_execution_plan(
            &effective.reference_models,
            provider.name(),
            provider.model(),
        );
        let reference_models = reference_plan.models;
        let requested_aggregator_model_id = effective.aggregator_model;
        let progress = MoaProgressReporter::from_ctx(ctx);

        progress.emit(format!(
            "dispatching {} expert(s) for consensus",
            reference_models.len()
        ));
        if let Some(model) = reference_plan.implicit_fallback_model.as_deref() {
            progress.emit(format!("added {model} as the active-model safety expert"));
        }

        tracing::info!(
            "moa: running {} reference models in parallel",
            reference_models.len()
        );

        // Step 1: Query all reference models in parallel
        let prompt = Arc::new(args.user_prompt.clone());
        let provider_arc = Arc::clone(provider);
        let mut join_handles: Vec<tokio::task::JoinHandle<(String, Result<String, String>)>> =
            Vec::new();
        let mut failed_models: Vec<String> = Vec::new();

        for model_id in reference_models.iter() {
            let model_id_clone = model_id.clone();
            let prompt_clone = Arc::clone(&prompt);
            let progress_clone = progress.clone();
            let request =
                match build_provider_request(provider.name(), provider.model(), &model_id_clone) {
                    Ok(request) => request,
                    Err(err) => {
                        tracing::warn!("moa: {} failed: {}", model_id_clone, err);
                        progress.emit(format!("skipping {model_id_clone}: {err}"));
                        failed_models.push(format!("{model_id_clone} ({err})"));
                        continue;
                    }
                };
            let per_model_provider = match provider_for_request(&provider_arc, &request) {
                Ok(provider) => provider,
                Err(err) => {
                    tracing::warn!("moa: {} failed: {}", model_id_clone, err);
                    progress.emit(format!("expert setup failed for {model_id_clone}: {err}"));
                    failed_models.push(format!("{model_id_clone} ({err})"));
                    continue;
                }
            };

            join_handles.push(tokio::spawn(async move {
                let messages = vec![ChatMessage::user(prompt_clone.as_str())];
                let options = CompletionOptions {
                    temperature: Some(REFERENCE_TEMPERATURE),
                    max_tokens: Some(REFERENCE_MAX_TOKENS),
                    ..Default::default()
                };
                match per_model_provider.chat(&messages, Some(&options)).await {
                    Ok(resp) => {
                        let content = resp.content.trim().to_string();
                        if content.is_empty() {
                            tracing::warn!("moa: {} returned empty content", model_id_clone);
                            progress_clone
                                .emit(format!("expert returned empty content: {model_id_clone}"));
                            (model_id_clone, Err("returned empty content".into()))
                        } else {
                            tracing::info!(
                                "moa: {} responded ({} chars)",
                                model_id_clone,
                                content.len()
                            );
                            progress_clone.emit(format!("expert completed: {model_id_clone}"));
                            (model_id_clone, Ok(content))
                        }
                    }
                    Err(e) => {
                        tracing::warn!("moa: {} failed: {}", model_id_clone, e);
                        progress_clone.emit(format!("expert failed: {model_id_clone}"));
                        (model_id_clone, Err(e.to_string()))
                    }
                }
            }));
        }

        // Collect reference responses
        let mut successful_responses: Vec<(String, String)> = Vec::new();

        for handle in join_handles {
            match handle.await {
                Ok((model, Ok(content))) => successful_responses.push((model, content)),
                Ok((model, Err(err))) => failed_models.push(format!("{model} ({err})")),
                Err(e) => {
                    tracing::warn!("moa: join error: {}", e);
                    failed_models.push(format!("task join failure ({e})"));
                }
            }
        }

        progress.emit(format!(
            "{} expert(s) succeeded; {} failed",
            successful_responses.len(),
            failed_models.len()
        ));

        if successful_responses.len() < MIN_SUCCESSFUL_REFERENCES {
            let failed_summary = failed_models.join(", ");
            return Err(ToolError::ExecutionFailed {
                tool: MOA_TOOL_NAME.into(),
                message: format!(
                    "Too few successful reference model responses ({}/{}). \
                     Failed models: {}",
                    successful_responses.len(),
                    reference_models.len(),
                    if failed_summary.is_empty() {
                        "none".to_string()
                    } else {
                        failed_summary
                    }
                ),
            });
        }

        tracing::info!(
            "moa: {} successful, {} failed. Running aggregator: {}",
            successful_responses.len(),
            failed_models.len(),
            requested_aggregator_model_id
        );

        // Step 2: Build aggregator system prompt
        let numbered_responses: Vec<String> = successful_responses
            .iter()
            .enumerate()
            .map(|(i, (model, content))| format!("{}. [{}]\n{}", i + 1, model, content))
            .collect();

        let aggregator_system = format!(
            "{}\n\n{}",
            AGGREGATOR_SYSTEM_PROMPT,
            numbered_responses.join("\n\n---\n\n")
        );

        // Step 3: Run aggregator with explicit fallbacks
        let aggregator_result = run_aggregator_with_fallbacks(AggregatorRunRequest {
            active_provider: &provider_arc,
            requested_aggregator_model: &requested_aggregator_model_id,
            active_provider_name: provider.name(),
            active_model: provider.model(),
            prompt: prompt.as_str(),
            aggregator_system: &aggregator_system,
            successful_responses: &successful_responses,
            progress: &progress,
        })
        .await?;
        let final_response = aggregator_result.content;
        failed_models.extend(aggregator_result.failures.iter().cloned());
        progress.emit(format!(
            "aggregation completed with {}",
            aggregator_result.used_model
        ));

        // Build structured output
        let used_ref_models: Vec<String> = successful_responses
            .iter()
            .map(|(m, _)| m.clone())
            .collect();

        let mut output = format!(
            "**Mixture-of-Agents Result**\n\
             Reference models: {}\n\
             Requested aggregator: {}\n\
             Aggregator: {}\n\
             Active-model safety expert: {}\n\
             Failed models: {}\n\n\
             ---\n\n\
             {}",
            used_ref_models.join(", "),
            aggregator_result.requested_model,
            aggregator_result.used_model,
            reference_plan
                .implicit_fallback_model
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            if failed_models.is_empty() {
                "none".to_string()
            } else {
                failed_models.join(", ")
            },
            final_response
        );

        // Optionally append individual reference responses for transparency
        if successful_responses.len() > 1 {
            output.push_str("\n\n---\n\n**Individual Reference Responses:**\n");
            for (i, (model, content)) in successful_responses.iter().enumerate() {
                let preview = if content.len() > 300 {
                    format!("{}… [truncated]", crate::safe_truncate(content, 300))
                } else {
                    content.clone()
                };
                output.push_str(&format!("\n**{}. {}:**\n{}\n", i + 1, model, preview));
            }
        }

        Ok(output)
    }
}

inventory::submit!(&MixtureOfAgentsTool as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moa_schema_has_required_field() {
        let tool = MixtureOfAgentsTool;
        let schema = tool.schema();
        assert_eq!(schema.name, MOA_TOOL_NAME);
        let required = schema.parameters["required"]
            .as_array()
            .expect("required must be array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("user_prompt")),
            "user_prompt must be required"
        );
    }

    #[test]
    fn moa_toolset_is_moa() {
        let tool = MixtureOfAgentsTool;
        assert_eq!(tool.toolset(), "moa");
    }

    #[test]
    fn moa_name_matches() {
        let tool = MixtureOfAgentsTool;
        assert_eq!(tool.name(), MOA_TOOL_NAME);
    }

    #[test]
    fn moa_legacy_alias_is_exposed() {
        let tool = MixtureOfAgentsTool;
        assert_eq!(tool.aliases(), &[LEGACY_MOA_TOOL_NAME]);
    }

    #[test]
    fn normalize_moa_model_spec_handles_aliases_and_nested_specs() {
        assert_eq!(
            normalize_moa_model_spec("copilot/gpt-4.1-mini").as_deref(),
            Some("vscode-copilot/gpt-4.1-mini")
        );
        assert_eq!(
            normalize_moa_model_spec("openrouter/openai/gpt-4.1").as_deref(),
            Some("openrouter/openai/gpt-4.1")
        );
    }

    #[test]
    fn normalize_reference_models_filters_invalid_and_duplicates() {
        let models = vec![
            " google/gemini-2.5-pro ".to_string(),
            "google/gemini-2.5-pro".to_string(),
            "".to_string(),
            "invalid".to_string(),
            "anthropic/claude-opus-4.6".to_string(),
        ];
        assert_eq!(
            normalize_reference_models(&models),
            vec![
                "gemini/gemini-2.5-pro".to_string(),
                "anthropic/claude-opus-4.6".to_string()
            ]
        );
    }

    #[test]
    fn sanitize_moa_config_restores_safe_defaults() {
        let effective = sanitize_moa_config(true, &[], " ");
        assert!(effective.enabled);
        assert_eq!(effective.reference_models, default_reference_models());
        assert_eq!(effective.aggregator_model, DEFAULT_AGGREGATOR_MODEL);
    }

    #[test]
    fn recommended_moa_config_for_model_spec_uses_active_model_only() {
        let effective = recommended_moa_config_for_model_spec("copilot/gpt-5-mini");
        assert!(effective.enabled);
        assert_eq!(
            effective.reference_models,
            vec!["vscode-copilot/gpt-5-mini".to_string()]
        );
        assert_eq!(effective.aggregator_model, "vscode-copilot/gpt-5-mini");
    }

    #[test]
    fn build_provider_request_routes_cross_provider_targets_explicitly() {
        let request = build_provider_request("anthropic", "claude-opus-4.6", "openai/gpt-4.1")
            .expect("request");
        assert_eq!(
            request,
            ProviderRequest {
                provider: "openai".into(),
                model: "gpt-4.1".into(),
                reuse_active: false,
            }
        );
    }

    #[test]
    fn build_provider_request_uses_openrouter_for_mixed_rosters() {
        let request =
            build_provider_request("openrouter", "anthropic/claude-opus-4.6", "openai/gpt-4.1")
                .expect("request");
        assert_eq!(
            request,
            ProviderRequest {
                provider: "openrouter".into(),
                model: "openai/gpt-4.1".into(),
                reuse_active: false,
            }
        );
    }

    #[test]
    fn build_provider_request_reuses_active_provider_only_for_exact_match() {
        let request =
            build_provider_request("openai", "gpt-4.1", "openai/gpt-4.1").expect("request");
        assert!(request.reuse_active);
        assert_eq!(request.provider, "openai");
        assert_eq!(request.model, "gpt-4.1");
    }

    #[test]
    fn build_reference_execution_plan_appends_active_model_once() {
        let plan = build_reference_execution_plan(
            &["openai/gpt-4.1".into()],
            "vscode-copilot",
            "gpt-5-mini",
        );
        assert_eq!(
            plan.models,
            vec![
                "openai/gpt-4.1".to_string(),
                "vscode-copilot/gpt-5-mini".to_string()
            ]
        );
        assert_eq!(
            plan.implicit_fallback_model.as_deref(),
            Some("vscode-copilot/gpt-5-mini")
        );
    }

    #[test]
    fn build_reference_execution_plan_does_not_duplicate_active_model() {
        let plan = build_reference_execution_plan(
            &["vscode-copilot/gpt-5-mini".into()],
            "vscode-copilot",
            "gpt-5-mini",
        );
        assert_eq!(plan.models, vec!["vscode-copilot/gpt-5-mini".to_string()]);
        assert_eq!(plan.implicit_fallback_model, None);
    }

    #[test]
    fn build_aggregator_candidates_prefers_requested_then_active_then_successes() {
        let candidates = build_aggregator_candidates(
            "vscode-copilot/gpt-4.1",
            "vscode-copilot",
            "gpt-5-mini",
            &[
                ("vscode-copilot/gpt-4.1".into(), "one".into()),
                ("gemini/gemini-2.5-pro".into(), "two".into()),
            ],
        );
        assert_eq!(
            candidates,
            vec![
                "vscode-copilot/gpt-4.1".to_string(),
                "vscode-copilot/gpt-5-mini".to_string(),
                "gemini/gemini-2.5-pro".to_string()
            ]
        );
    }
}
