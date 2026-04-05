//! # SubAgentRunner — implementation for delegate_task
//!
//! WHY here (edgecrab-core): The SubAgentRunner trait is defined in
//! edgecrab-tools to break the circular dependency. edgecrab-core
//! implements it because it has access to Agent/AgentBuilder/execute_loop.
//!
//! ```text
//!   edgecrab-tools (trait)    edgecrab-core (impl)
//!   ────────────────────      ────────────────────
//!   SubAgentRunner trait  ←── CoreSubAgentRunner struct
//!   delegate_task tool         uses AgentBuilder + execute_loop
//! ```

use std::sync::Arc;

use async_trait::async_trait;
use edgecrab_tools::registry::ToolRegistry;
use edgecrab_tools::{SubAgentResult, SubAgentRunner};
use edgecrab_types::Platform;
use edgequake_llm::{LLMProvider, ProviderFactory, VsCodeCopilotProvider};

/// Real implementation of SubAgentRunner that spawns child Agent instances.
///
/// WHY struct with fields: The runner needs the parent's provider and
/// tool registry to construct child agents. These are cloned (Arc) at
/// runner creation time, so the parent's state is shared but the child
/// gets its own Agent instance with independent session state.
pub struct CoreSubAgentRunner {
    /// LLM provider shared with parent (Arc, not cloned data)
    provider: Arc<dyn LLMProvider>,
    /// Tool registry shared with parent (same compile-time tools)
    tool_registry: Arc<ToolRegistry>,
    /// Platform context inherited from parent
    platform: Platform,
    /// Model name (e.g. "anthropic/claude-sonnet-4-20250514")
    model: String,
}

impl CoreSubAgentRunner {
    /// Create a new runner with the parent agent's shared resources.
    pub fn new(
        provider: Arc<dyn LLMProvider>,
        tool_registry: Arc<ToolRegistry>,
        platform: Platform,
        model: String,
    ) -> Self {
        Self {
            provider,
            tool_registry,
            platform,
            model,
        }
    }
}

#[async_trait]
impl SubAgentRunner for CoreSubAgentRunner {
    async fn run_task(
        &self,
        goal: &str,
        system_prompt: &str,
        enabled_toolsets: Vec<String>,
        max_iterations: u32,
        model_override: Option<String>,
        parent_cancel: tokio_util::sync::CancellationToken,
    ) -> Result<SubAgentResult, String> {
        let (child_provider, child_model) =
            self.resolve_child_provider_and_model(model_override.as_deref())?;

        // Build child Agent with inherited provider + registry but
        // independent session state and filtered toolsets.
        let child = Arc::new(
            crate::AgentBuilder::new(&child_model)
                .provider(child_provider)
                .tools(self.tool_registry.clone())
                .max_iterations(max_iterations)
                .platform(self.platform)
                .quiet_mode(true)
                .build()
                .map_err(|e| format!("Failed to build child agent: {e}"))?,
        );

        // Override enabled toolsets on the child's config
        {
            let mut config = child.config.write().await;
            config.enabled_toolsets = enabled_toolsets;
            // Children should never delegate further (enforced by depth check
            // in delegate_task, but belt-and-suspenders here too)
            config.disabled_toolsets.push("delegation".to_string());
            // Skip memory and context file loading — subagents get a focused
            // system prompt and don't need the parent's memory/context.
            // Matches hermes-agent: skip_context_files=True, skip_memory=True.
            config.skip_memory = true;
            config.skip_context_files = true;
        }

        // Propagate parent cancellation into the child agent so delegated runs
        // stop promptly when the parent turn is interrupted.
        let child_for_cancel = child.clone();
        let cancel_watch = tokio::spawn(async move {
            parent_cancel.cancelled().await;
            child_for_cancel.interrupt();
        });

        // Run the full conversation loop with the focused system prompt.
        // The child gets its own execute_loop: tool dispatch, compression,
        // retry, budget — everything the parent has.
        let result = child
            .run_conversation(goal, Some(system_prompt), None)
            .await;
        cancel_watch.abort();
        let result = result.map_err(|e| format!("Child agent execution failed: {e}"))?;

        Ok(SubAgentResult {
            summary: result.final_response,
            api_calls: result.api_calls,
            input_tokens: result.usage.input_tokens,
            output_tokens: result.usage.output_tokens,
            model: Some(result.model),
            interrupted: result.interrupted,
            budget_exhausted: result.budget_exhausted,
            messages: result.messages,
        })
    }
}

impl CoreSubAgentRunner {
    fn resolve_child_provider_and_model(
        &self,
        model_override: Option<&str>,
    ) -> Result<(Arc<dyn LLMProvider>, String), String> {
        let Some(raw_model) = model_override.map(str::trim).filter(|m| !m.is_empty()) else {
            return Ok((self.provider.clone(), self.model.clone()));
        };

        // If provider/model is explicitly requested, create that provider.
        if let Some((provider_name, model_name)) = raw_model.split_once('/') {
            let canonical = match provider_name {
                "copilot" => "vscode-copilot",
                other => other,
            };
            if canonical == "vscode-copilot" {
                let provider = VsCodeCopilotProvider::new()
                    .model(model_name)
                    .with_vision(true) // Enable vision so copilot-vision-request header is sent
                    .build()
                    .map_err(|e| {
                        format!(
                            "Failed to create delegation provider '{}' for model '{}': {}",
                            canonical, model_name, e
                        )
                    })?;
                return Ok((Arc::new(provider), raw_model.to_string()));
            }

            let provider =
                ProviderFactory::create_llm_provider(canonical, model_name).map_err(|e| {
                    format!(
                        "Failed to create delegation provider '{}' for model '{}': {}",
                        canonical, model_name, e
                    )
                })?;
            return Ok((provider, raw_model.to_string()));
        }

        // Bare model name: reuse parent's provider credentials and endpoint.
        Ok((self.provider.clone(), raw_model.to_string()))
    }
}
