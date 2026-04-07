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
use edgecrab_tools::registry::{DelegationEvent, SubAgentRunRequest, ToolRegistry};
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
    async fn run_task(&self, request: SubAgentRunRequest) -> Result<SubAgentResult, String> {
        let SubAgentRunRequest {
            goal,
            system_prompt,
            enabled_toolsets,
            max_iterations,
            model_override,
            parent_cancel,
            progress_tx,
            task_index,
            task_count,
        } = request;
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
        let (child_event_tx, mut child_event_rx) = tokio::sync::mpsc::unbounded_channel();
        let progress_forwarder = if let Some(progress_tx) = progress_tx {
            Some(tokio::spawn(async move {
                while let Some(event) = child_event_rx.recv().await {
                    match event {
                        crate::StreamEvent::Reasoning(text) => {
                            if !text.trim().is_empty() {
                                let _ = progress_tx.send(DelegationEvent::Thinking {
                                    task_index,
                                    task_count,
                                    text,
                                });
                            }
                        }
                        crate::StreamEvent::ToolExec { name, args_json } => {
                            let _ = progress_tx.send(DelegationEvent::ToolCalled {
                                task_index,
                                task_count,
                                tool_name: name,
                                args_json,
                            });
                        }
                        _ => {}
                    }
                }
            }))
        } else {
            drop(child_event_rx);
            None
        };

        let result = child
            .execute_loop(
                &goal,
                Some(&system_prompt),
                None,
                Some(&child_event_tx),
                None,
            )
            .await;
        cancel_watch.abort();
        drop(child_event_tx);
        if let Some(forwarder) = progress_forwarder {
            let _ = forwarder.await;
        }
        let result = result.map_err(|e| format!("Child agent execution failed: {e}"))?;

        Ok(SubAgentResult {
            summary: result.final_response,
            api_calls: result.api_calls,
            input_tokens: result.usage.input_tokens,
            output_tokens: result.usage.output_tokens,
            cache_read_tokens: result.usage.cache_read_tokens,
            cache_write_tokens: result.usage.cache_write_tokens,
            reasoning_tokens: result.usage.reasoning_tokens,
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
            let canonical = edgecrab_tools::vision_models::normalize_provider_name(provider_name);
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
                ProviderFactory::create_llm_provider(&canonical, model_name).map_err(|e| {
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
