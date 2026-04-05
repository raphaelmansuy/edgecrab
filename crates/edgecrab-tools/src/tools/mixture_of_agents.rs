//! # mixture_of_agents — Multi-model consensus reasoning
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
//!   mixture_of_agents(user_prompt)
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
use std::sync::Arc;

use edgecrab_types::{ToolError, ToolSchema};
use edgequake_llm::traits::{CacheControl, ChatMessage, CompletionOptions};

use crate::registry::{ToolContext, ToolHandler};

// ─── Default configuration ─────────────────────────────────────────────────

/// Reference models that provide diverse initial responses.
/// These must be available via the active provider (gateway/openrouter).
const DEFAULT_REFERENCE_MODELS: &[&str] = &[
    "anthropic/claude-opus-4.6",
    "google/gemini-2.5-pro",
    "openai/gpt-4.1",
    "deepseek/deepseek-r1",
];

/// Aggregator model — synthesizes reference responses.
const DEFAULT_AGGREGATOR_MODEL: &str = "anthropic/claude-opus-4.6";

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

#[async_trait]
impl ToolHandler for MixtureOfAgentsTool {
    fn name(&self) -> &'static str {
        "mixture_of_agents"
    }

    fn toolset(&self) -> &'static str {
        "moa"
    }

    fn emoji(&self) -> &'static str {
        "🧠"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "mixture_of_agents".into(),
            description: "Process a complex query using multiple frontier LLMs in parallel, then \
                synthesize their responses with an aggregator model. Produces higher-quality output \
                than any single model for hard reasoning, math, coding, and analysis tasks. \
                Use when: (1) a task is genuinely difficult and you want a second/third opinion, \
                (2) you need consensus across models, (3) single-model answers feel uncertain. \
                Requires a provider that supports model routing (e.g., OpenRouter)."
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
            tool: "mixture_of_agents".into(),
            message: e.to_string(),
        })?;

        let provider = ctx
            .provider
            .as_ref()
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: "mixture_of_agents".into(),
                message: "No LLM provider available. mixture_of_agents requires a provider.".into(),
            })?;

        let reference_models: Vec<String> = args.reference_models.unwrap_or_else(|| {
            DEFAULT_REFERENCE_MODELS
                .iter()
                .map(|s| s.to_string())
                .collect()
        });

        let aggregator_model_id = args
            .aggregator_model
            .unwrap_or_else(|| DEFAULT_AGGREGATOR_MODEL.to_string());

        if reference_models.is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: "mixture_of_agents".into(),
                message: "reference_models must not be empty".into(),
            });
        }

        tracing::info!(
            "mixture_of_agents: running {} reference models in parallel",
            reference_models.len()
        );

        // ── Helper: Parse "provider/model" → (provider_name, model_name) ──
        // If the active provider is OpenRouter, we create per-model instances.
        // Otherwise we use the active provider for all calls (single-model MoA).
        let active_provider_name = provider.name().to_string();
        let is_openrouter = active_provider_name.to_lowercase() == "openrouter";

        // Step 1: Query all reference models in parallel
        let prompt = Arc::new(args.user_prompt.clone());
        let provider_arc = Arc::clone(provider);

        let mut join_handles: Vec<tokio::task::JoinHandle<(String, Option<String>)>> = Vec::new();

        for model_id in reference_models.iter() {
            let model_id_clone = model_id.clone();
            let prompt_clone = Arc::clone(&prompt);

            // Try to build a per-model provider if on OpenRouter or matching provider prefix
            let per_model_provider: Arc<dyn edgequake_llm::LLMProvider> = if is_openrouter {
                // On OpenRouter, create a clone pointing at this specific model
                match edgequake_llm::ProviderFactory::create_llm_provider(
                    "openrouter",
                    &model_id_clone,
                ) {
                    Ok(p) => p,
                    Err(_) => Arc::clone(&provider_arc),
                }
            } else {
                // Try prefix-based (e.g., "anthropic/claude-opus-4.6" → provider="anthropic")
                let maybe_provider =
                    if let Some((prefix, sub_model)) = model_id_clone.split_once('/') {
                        edgequake_llm::ProviderFactory::create_llm_provider(prefix, sub_model).ok()
                    } else {
                        None
                    };
                maybe_provider.unwrap_or_else(|| Arc::clone(&provider_arc))
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
                            tracing::warn!(
                                "mixture_of_agents: {} returned empty content",
                                model_id_clone
                            );
                            (model_id_clone, None)
                        } else {
                            tracing::info!(
                                "mixture_of_agents: {} responded ({} chars)",
                                model_id_clone,
                                content.len()
                            );
                            (model_id_clone, Some(content))
                        }
                    }
                    Err(e) => {
                        tracing::warn!("mixture_of_agents: {} failed: {}", model_id_clone, e);
                        (model_id_clone, None)
                    }
                }
            }));
        }

        // Collect reference responses
        let mut successful_responses: Vec<(String, String)> = Vec::new();
        let mut failed_models: Vec<String> = Vec::new();

        for handle in join_handles {
            match handle.await {
                Ok((model, Some(content))) => successful_responses.push((model, content)),
                Ok((model, None)) => failed_models.push(model),
                Err(e) => tracing::warn!("mixture_of_agents: join error: {}", e),
            }
        }

        if successful_responses.len() < MIN_SUCCESSFUL_REFERENCES {
            let failed_summary = failed_models.join(", ");
            return Err(ToolError::ExecutionFailed {
                tool: "mixture_of_agents".into(),
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
            "mixture_of_agents: {} successful, {} failed. Running aggregator: {}",
            successful_responses.len(),
            failed_models.len(),
            aggregator_model_id
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

        // Step 3: Create aggregator provider
        let aggregator_provider: Arc<dyn edgequake_llm::LLMProvider> = if is_openrouter {
            edgequake_llm::ProviderFactory::create_llm_provider("openrouter", &aggregator_model_id)
                .unwrap_or_else(|_| Arc::clone(&provider_arc))
        } else {
            let maybe = if let Some((prefix, sub_model)) = aggregator_model_id.split_once('/') {
                edgequake_llm::ProviderFactory::create_llm_provider(prefix, sub_model).ok()
            } else {
                None
            };
            maybe.unwrap_or_else(|| Arc::clone(&provider_arc))
        };

        // Step 4: Run aggregator
        let mut agg_system_msg = ChatMessage::system(&aggregator_system);
        // Hint for caching (Anthropic/compatible providers)
        agg_system_msg.cache_control = Some(CacheControl::ephemeral());

        let agg_user_msg = ChatMessage::user(prompt.as_str());
        let agg_messages = vec![agg_system_msg, agg_user_msg];

        let agg_options = CompletionOptions {
            temperature: Some(AGGREGATOR_TEMPERATURE),
            ..Default::default()
        };

        let agg_response = aggregator_provider
            .chat(&agg_messages, Some(&agg_options))
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "mixture_of_agents".into(),
                message: format!("Aggregator ({aggregator_model_id}) failed: {e}"),
            })?;

        let final_response = agg_response.content.trim().to_string();

        // Build structured output
        let used_ref_models: Vec<String> = successful_responses
            .iter()
            .map(|(m, _)| m.clone())
            .collect();

        let mut output = format!(
            "**Mixture-of-Agents Result**\n\
             Reference models: {}\n\
             Aggregator: {}\n\
             Failed models: {}\n\n\
             ---\n\n\
             {}",
            used_ref_models.join(", "),
            aggregator_model_id,
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
        assert_eq!(schema.name, "mixture_of_agents");
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
        assert_eq!(tool.name(), "mixture_of_agents");
    }
}
