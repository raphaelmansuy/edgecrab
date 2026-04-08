//! # Conversation loop — the core agent execution cycle
//!
//! WHY separate from agent.rs: The conversation loop is ~200 lines of
//! complex async control flow. Keeping it in its own module makes both
//! files easier to reason about and test independently.
//!
//! ```text
//!   run_loop(messages)
//!       │
//!       ├── expand @context refs in user message
//!       ├── classify message (model routing hint)
//!       ├── load memory + skills into system prompt (first turn)
//!       ├── budget check ─── exhausted? → break
//!       ├── cancel check ─── cancelled? → break (interrupted)
//!       ├── context compression ── prune tools + LLM summarize
//!       ├── API call ──────── retry up to 3× with backoff
//!       │       │
//!       │       ├── tool_calls? → dispatch → append results → continue
//!       │       └── text only?  → final_response → break
//!       │
//!       ├── [if ≥5 tool calls] run_learning_reflection()
//!       │       │                ← CLOSED LEARNING LOOP
//!       │       └── agent may call skill_manage / memory_write
//!       │
//!       └── persist session to SQLite + return ConversationResult
//! ```
//!
//! ## Closed Learning Loop
//!
//! Mirrors hermes-agent's self-improvement architecture. On sessions that
//! use 5+ tools the loop appends a reflection prompt. The agent can then:
//! - Save a reusable workflow via `skill_manage(action='create', ...)`.
//! - Patch an outdated skill via `skill_manage(action='patch', ...)`.
//! - Record project/user facts via `memory_write`.
//!
//! The SKILLS_GUIDANCE constant in `prompt_builder.rs` nudges the agent
//! proactively during the session. The explicit reflection step at the end
//! provides a reliable second trigger with zero user effort.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use edgecrab_tools::config_ref::AppConfigRef;
use edgecrab_tools::registry::{
    ApprovalRequest, ApprovalResponse, DelegationEvent, ToolContext, ToolRegistry,
    to_llm_definitions,
};
use edgecrab_types::trajectory::{
    TrajectoryMetadata, convert_scratchpad_to_think, save_trajectory,
};
use edgecrab_types::{
    AgentError, Content, Cost, Message, Role, ToolError, ToolErrorResponse, Trajectory, Usage,
};
use edgequake_llm::traits::{StreamChunk, StreamUsage};
use edgequake_llm::{CachePromptConfig, LLMProvider, apply_cache_control};
use futures::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::agent::{Agent, ConversationResult, SessionState, resolve_tool_policy};
use crate::compression::{
    CompressionParams, CompressionStatus, check_compression_status_for_estimate, compress_with_llm,
};
use crate::config::edgecrab_home;
use crate::context_references::expand_context_refs_with_policy;
use crate::model_router::{RoutingThresholds, SmartRoutingConfig, resolve_turn_route};
use crate::pricing::{CanonicalUsage, estimate_cost};
use crate::prompt_builder::{
    PromptBuilder, load_global_soul, load_memory_sections, load_preloaded_skills,
    load_skill_summary,
};
use crate::sub_agent_runner::CoreSubAgentRunner;

/// Maximum API retries before giving up.
const MAX_RETRIES: u32 = 3;

/// Base backoff delay between retries (doubles each attempt).
const BASE_BACKOFF: Duration = Duration::from_millis(500);

/// Minimum tool-call count in a session before the end-of-session
/// learning reflection fires. Mirrors hermes-agent's "5+ tool calls" rule.
const SKILL_REFLECTION_THRESHOLD: u32 = 5;

struct ApiCallContext<'a> {
    options: Option<&'a edgequake_llm::CompletionOptions>,
    cancel: &'a CancellationToken,
    event_tx: Option<&'a tokio::sync::mpsc::UnboundedSender<crate::StreamEvent>>,
    use_native_streaming: bool,
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
    matches!(
        error,
        edgequake_llm::LlmError::RateLimited(_)
            | edgequake_llm::LlmError::NetworkError(_)
            | edgequake_llm::LlmError::Timeout
            | edgequake_llm::LlmError::AuthError(_)
            | edgequake_llm::LlmError::ApiError(_)
            | edgequake_llm::LlmError::ProviderError(_)
    )
}

fn completion_options_for(config: &crate::agent::AgentConfig) -> edgequake_llm::CompletionOptions {
    edgequake_llm::CompletionOptions {
        max_tokens: config.model_config.max_tokens.map(|tokens| tokens as usize),
        temperature: config.temperature.or(config.model_config.temperature),
        reasoning_effort: config.reasoning_effort.clone(),
        ..Default::default()
    }
}

fn provider_prefers_nonstreaming_tool_turns(provider: &dyn LLMProvider) -> bool {
    matches!(provider.name(), "vscode-copilot")
}

pub(crate) fn should_use_native_streaming(
    provider: &dyn LLMProvider,
    tool_defs: &[edgequake_llm::ToolDefinition],
    streaming_enabled: bool,
    event_tx_present: bool,
) -> bool {
    if !streaming_enabled || !event_tx_present || !provider.supports_tool_streaming() {
        return false;
    }

    if tool_defs.is_empty() {
        return true;
    }

    !provider_prefers_nonstreaming_tool_turns(provider)
}

/// Build a `ToolContext` from shared agent state.
///
/// WHY extracted: This was duplicated in `execute_loop` and
/// `dispatch_single_tool` with subtly inconsistent `cwd` handling.
/// Centralizing ensures the same working directory is used everywhere.
#[allow(clippy::too_many_arguments)]
fn build_tool_context(
    cwd: &std::path::Path,
    app_config_ref: AppConfigRef,
    cancel: &CancellationToken,
    state_db: &Option<Arc<edgecrab_state::SessionDb>>,
    platform: edgecrab_types::Platform,
    process_table: &Arc<edgecrab_tools::ProcessTable>,
    provider: Option<Arc<dyn edgequake_llm::LLMProvider>>,
    tool_registry: Option<Arc<ToolRegistry>>,
    sub_agent_runner: Option<Arc<dyn edgecrab_tools::SubAgentRunner>>,
    delegation_event_tx: Option<tokio::sync::mpsc::UnboundedSender<DelegationEvent>>,
    clarify_tx: Option<
        tokio::sync::mpsc::UnboundedSender<edgecrab_tools::registry::ClarifyRequest>,
    >,
    approval_tx: Option<tokio::sync::mpsc::UnboundedSender<ApprovalRequest>>,
    tool_progress_tx: Option<
        tokio::sync::mpsc::UnboundedSender<edgecrab_tools::ToolProgressUpdate>,
    >,
    gateway_sender: Option<Arc<dyn edgecrab_tools::registry::GatewaySender>>,
    origin_chat: Option<(String, String)>,
    current_tool_call_id: Option<String>,
    current_tool_name: Option<String>,
    // Stable per-conversation session identifier — used as the browser session key
    // so all tool calls within one session share the same Chrome tab.
    conversation_session_id: &str,
    // Per-conversation todo store — survives context compression.
    todo_store: Option<Arc<edgecrab_tools::TodoStore>>,
) -> ToolContext {
    ToolContext {
        task_id: uuid::Uuid::new_v4().to_string(),
        cwd: cwd.to_path_buf(),
        session_id: conversation_session_id.to_string(),
        user_task: None,
        cancel: cancel.clone(),
        config: app_config_ref,
        state_db: state_db.clone(),
        platform,
        process_table: Some(process_table.clone()),
        provider,
        tool_registry,
        delegate_depth: 0,
        sub_agent_runner,
        delegation_event_tx,
        clarify_tx,
        approval_tx,
        // Wires the skills prompt-cache invalidation hook into every
        // SkillManageTool mutation (create/edit/patch/delete) without
        // creating a circular crate dependency from edgecrab-tools →
        // edgecrab-core.  The closure captures nothing — zero allocation
        // overhead per invocation.
        on_skills_changed: Some(std::sync::Arc::new(
            crate::prompt_builder::invalidate_skills_cache,
        )),
        gateway_sender,
        origin_chat: origin_chat.clone(),
        // Build the session key from origin_chat (gateway mode) or fall back
        // to conversation_session_id (CLI mode).  Mirrors hermes-agent's
        // ProcessSession.session_key used for gateway reset protection.
        session_key: Some(match &origin_chat {
            Some((platform, chat_id)) => format!("{}:{}", platform, chat_id),
            None => conversation_session_id.to_string(),
        }),
        todo_store,
        current_tool_call_id,
        current_tool_name,
        tool_progress_tx,
    }
}

/// What happened after processing one API response.
enum LoopAction {
    /// Tool calls were dispatched — loop again for the next LLM response.
    Continue,
    /// LLM produced a final text response — exit the loop.
    Done(String),
}

const MAX_DELEGATE_TASK_CALLS_PER_TURN: usize = 3;

/// Shared context passed to `process_response` and `dispatch_single_tool`.
///
/// WHY: Both functions previously took 8 parameters, tripping the
/// `clippy::too_many_arguments` lint. Grouping the 6 shared dispatch
/// params into one struct reduces argument count to 3 for each function
/// and makes the shared state explicit.
struct DispatchContext<'a> {
    cwd: std::path::PathBuf,
    registry: Option<&'a Arc<ToolRegistry>>,
    cancel: &'a CancellationToken,
    state_db: &'a Option<Arc<edgecrab_state::SessionDb>>,
    platform: edgecrab_types::Platform,
    process_table: &'a Arc<edgecrab_tools::ProcessTable>,
    provider: Option<Arc<dyn edgequake_llm::LLMProvider>>,
    gateway_sender: Option<Arc<dyn edgecrab_tools::registry::GatewaySender>>,
    sub_agent_runner: Option<Arc<dyn edgecrab_tools::SubAgentRunner>>,
    event_tx: Option<&'a tokio::sync::mpsc::UnboundedSender<crate::StreamEvent>>,
    delegation_event_tx: Option<tokio::sync::mpsc::UnboundedSender<DelegationEvent>>,
    /// Channel for interactive clarify requests (None in batch/gateway modes).
    clarify_tx:
        Option<tokio::sync::mpsc::UnboundedSender<edgecrab_tools::registry::ClarifyRequest>>,
    /// Channel for interactive dangerous-command approval requests.
    approval_tx: Option<tokio::sync::mpsc::UnboundedSender<ApprovalRequest>>,
    /// Origin chat context — (platform_name, chat_id) from gateway sessions.
    /// Used by manage_cron_jobs to set the job's origin so deliver='origin' works.
    origin_chat: Option<(String, String)>,
    /// Per-turn tool configuration snapshot propagated into ToolContext.
    app_config_ref: AppConfigRef,
    /// Stable per-conversation session ID — shared by all tool calls so browser
    /// sessions persist across sequential tool invocations within one conversation.
    conversation_session_id: String,
    /// Per-session todo store — propagated into every ToolContext.
    todo_store: Option<Arc<edgecrab_tools::TodoStore>>,
    /// Hard capability failures already observed in this conversation.
    capability_suppressions: Arc<Mutex<HashMap<String, ToolErrorResponse>>>,
}

impl Agent {
    /// Full conversation loop with tool dispatch and retry.
    ///
    /// Replaces the simple single-call implementation in agent.rs.
    /// This is the main entry point called by `Agent::chat()` and
    /// `Agent::run_conversation()`.
    pub(crate) async fn execute_loop(
        &self,
        user_message: &str,
        system_message: Option<&str>,
        history: Option<Vec<Message>>,
        event_tx: Option<&tokio::sync::mpsc::UnboundedSender<crate::StreamEvent>>,
        cwd_override: Option<&std::path::Path>,
    ) -> Result<ConversationResult, AgentError> {
        let _conversation_guard = self.conversation_lock.lock().await;
        self.budget.reset();

        // Extract (and optionally reset) the cancel token for this conversation turn.
        //
        // WHY reset: CancellationToken is a one-way latch — once cancelled it
        // cannot be un-cancelled. Without resetting, a Ctrl+C would permanently
        // break all future conversations. We swap in a fresh token when the
        // previous turn was interrupted so each turn has independent cancellation.
        let cancel = {
            let mut guard = self.cancel.lock().expect("cancel mutex not poisoned");
            if guard.is_cancelled() {
                *guard = CancellationToken::new();
            }
            guard.clone()
        };

        // Snapshot config and provider at loop start so in-flight
        // conversations are not affected by a /model hot-swap.
        let config = self.config.read().await.clone();
        let provider = self.provider.read().await.clone();

        let mut session = {
            let mut shared = self.session.write().await;
            // Seed from history if provided (gateway mode: fresh Agent per message)
            if let Some(hist) = history {
                shared.messages = hist;
            }
            shared.clone()
        };

        // Resolve cwd early — used by both PromptBuilder and @context expansion.
        let cwd = cwd_override
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| {
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
            });

        // Expand parent toolset configuration once and reuse for schema filtering,
        // ToolContext propagation, and child delegation restrictions.
        let gateway_running = self.gateway_sender.read().await.is_some();
        let tool_policy = resolve_tool_policy(&config);
        let expanded_enabled = tool_policy.expanded_enabled.clone();
        let expanded_disabled = tool_policy.expanded_disabled.clone();
        let app_config_ref = config.to_app_config_ref(gateway_running, &tool_policy);

        // Apply config-based env passthrough so it is available to every
        // PersistentShell::spawn() call within this session.
        if !config.terminal_env_passthrough.is_empty() {
            edgecrab_tools::tools::backends::local::register_env_passthrough(
                &config.terminal_env_passthrough,
            );
        }

        // Build tool definitions first so we can pass available tool names to
        // PromptBuilder. The schemas are computed once per session; converting
        // to edgequake-llm ToolDefinition here avoids allocations in the loop.
        //
        // WHY before prompt: PromptBuilder gates guidance sections (memory,
        // session_search, skills) on whether the corresponding tools are
        // actually enabled. Without this, the agent gets instructions for tools
        // it cannot use.
        let (mut active_tool_defs, tool_names_for_prompt) =
            if let Some(ref registry) = self.tool_registry {
                let ctx = build_tool_context(
                    &cwd,
                    app_config_ref.clone(),
                    &cancel,
                    &self.state_db,
                    config.platform,
                    &self.process_table,
                    Some(provider.clone()),
                    self.tool_registry.clone(),
                    None,
                    None,
                    None, // clarify_tx not needed for schema resolution
                    None, // approval_tx not needed for schema resolution
                    None, // tool_progress_tx not needed for schema resolution
                    self.gateway_sender.read().await.clone(),
                    config.origin_chat.clone(),
                    None,                // current_tool_call_id not needed for schema resolution
                    None,                // current_tool_name not needed for schema resolution
                    "schema-resolution", // placeholder — schemas are not browser-session-sensitive
                    Some(self.todo_store.clone()),
                );

                // "all" sentinel / genuinely empty → pass None (no filtering).
                let enabled_filter = if config.enabled_toolsets.is_empty()
                    || edgecrab_tools::toolsets::contains_all_sentinel(&config.enabled_toolsets)
                    || expanded_enabled.is_empty()
                {
                    None
                } else {
                    Some(expanded_enabled.as_slice())
                };
                let disabled_filter = if expanded_disabled.is_empty() {
                    None
                } else {
                    Some(expanded_disabled.as_slice())
                };

                let schemas = registry.get_definitions(enabled_filter, disabled_filter, &ctx);
                let names: Vec<String> = schemas.iter().map(|s| s.name.clone()).collect();
                (to_llm_definitions(&schemas), names)
            } else {
                (Vec::new(), Vec::new())
            };

        // Cache system prompt on first turn — assemble via PromptBuilder
        //
        // WHY PromptBuilder here: The system prompt is the agent's identity,
        // platform awareness, memory, skills, and context files. Without it,
        // the agent is a generic "helpful assistant" with no capabilities.
        // The prompt is cached (frozen snapshot) — mid-session memory writes
        // update disk but NOT this cached prompt (preserves cache efficiency).
        if session.cached_system_prompt.is_none() {
            let prompt = if let Some(explicit) = system_message {
                // Caller provided an explicit system prompt (e.g. gateway, tests)
                explicit.to_string()
            } else {
                // Assemble the full system prompt from all sources
                let home = edgecrab_home();
                let memory_sections = if config.skip_memory {
                    Vec::new()
                } else {
                    load_memory_sections(&home)
                };
                // Build skill summary with filtering:
                // - disabled_skills from config.skills.disabled
                // - platform-specific disabled from config.skills.platform_disabled
                let platform_str = config.platform.to_string();
                let mut disabled_skills = config.skills_config.disabled.clone();
                if let Some(platform_disabled) =
                    config.skills_config.platform_disabled.get(&platform_str)
                {
                    disabled_skills.extend(platform_disabled.iter().cloned());
                }
                let toolsets_for_prompt = if let Some(registry) = self.tool_registry.as_ref() {
                    available_toolsets_for_prompt(registry, &tool_names_for_prompt)
                } else {
                    Vec::new()
                };
                let skill_summary = load_skill_summary(
                    &home,
                    &disabled_skills,
                    Some(&tool_names_for_prompt),
                    Some(&toolsets_for_prompt),
                );
                // Load preloaded skills (from -s/--skill flag or config.skills.preloaded)
                // and prepend their full content before the auto-discovered skill summary.
                let preloaded_content =
                    load_preloaded_skills(&home, &config.skills_config.preloaded);
                let combined_skill_prompt: Option<String> =
                    match (preloaded_content.is_empty(), skill_summary) {
                        (false, Some(summary)) => Some(format!("{preloaded_content}\n\n{summary}")),
                        (false, None) => Some(preloaded_content),
                        (true, summary) => summary,
                    };
                // Load global SOUL.md from ~/.edgecrab/SOUL.md as identity override (slot #1).
                // WHY: hermes-agent loads SOUL.md from HERMES_HOME as the agent's baseline
                // identity. We do the same here — the global SOUL.md replaces DEFAULT_IDENTITY.
                // Project-level SOUL.md files are loaded separately as context file sections
                // by discover_context_files(), allowing per-project tuning on top.
                let global_soul = load_global_soul(&home);
                let has_filesystem_sensitive_tools = tool_names_for_prompt.iter().any(|name| {
                    matches!(
                        name.as_str(),
                        "read_file"
                            | "write_file"
                            | "patch"
                            | "search_files"
                            | "terminal"
                            | "execute_code"
                    )
                });
                let execution_guidance = has_filesystem_sensitive_tools.then(|| {
                    edgecrab_tools::describe_execution_filesystem(&app_config_ref, &cwd)
                        .render_prompt_block()
                });
                PromptBuilder::new(config.platform)
                    .skip_context_files(config.skip_context_files)
                    .execution_environment_guidance(execution_guidance)
                    .available_tools(tool_names_for_prompt)
                    .build(
                        global_soul.as_deref(), // global SOUL.md is identity override
                        Some(&cwd),
                        &memory_sections,
                        combined_skill_prompt.as_deref(),
                    )
            };
            // Append personality addon if configured (e.g. kawaii, pirate, philosopher)
            let prompt = if let Some(ref addon) = config.personality_addon {
                format!("{prompt}\n\n## Personality\n\n{addon}")
            } else {
                prompt
            };
            session.cached_system_prompt = Some(prompt);
        }

        // Expand @context references in the user message before appending.
        //
        // WHY before append: @file, @diff etc. inject raw content into the
        // message. The LLM must see the expanded content, not the @ref token.
        // Expansion also validates security (no .ssh, no path traversal).
        let context_path_policy = app_config_ref.file_path_policy(&cwd);
        let mut expansion =
            expand_context_refs_with_policy(user_message, &cwd, &context_path_policy);
        if !expansion.refs_found.is_empty() {
            tracing::debug!(
                refs = expansion.refs_found.len(),
                errors = expansion.errors.len(),
                "expanded @context references"
            );
        }
        for err in &expansion.errors {
            tracing::warn!(error = %err, "context reference expansion error");
        }

        // Context-injection token budget — mirrors hermes-agent's hard/soft limits.
        //
        // WHY: Injecting unlimited @file or @folder content can consume most of
        // the context window before the first API call, crowding out the system
        // prompt, tool schemas, and working memory. Hermes blocks at 50% and
        // warns at 25% of the context window.
        //
        // The heuristic is chars/4 ≈ tokens (same as estimate_tokens in compression.rs).
        // Only fires when the expansion actually added content beyond the original text.
        if !expansion.refs_found.is_empty() {
            let context_window =
                CompressionParams::from_model_config(&config.model, &config.compression)
                    .context_window;
            let injected_chars = expansion.expanded.len().saturating_sub(user_message.len());
            let injected_tokens = injected_chars / 4;
            let hard_limit = context_window / 2; // 50% hard stop
            let soft_limit = context_window / 4; // 25% soft warn

            if injected_tokens > hard_limit {
                tracing::warn!(
                    injected_tokens,
                    hard_limit,
                    "@context injection exceeds 50% of context window — stripping injected content"
                );
                // Hard block: discard injected content, use original message.
                // Inject a notice so the LLM (and user) know why the refs were dropped.
                let notice = format!(
                    "{user_message}\n\n[Warning: @context injection (~{injected_tokens} tokens) \
                     exceeds the 50% context-window limit ({hard_limit} tokens). \
                     Injected content was removed to protect the context budget.]"
                );
                expansion.expanded = notice;
                expansion.budget_blocked = true;
            } else if injected_tokens > soft_limit {
                tracing::warn!(
                    injected_tokens,
                    soft_limit,
                    "@context injection exceeds 25% of context window — approaching budget limit"
                );
                expansion.budget_warning = true;
            }
        }

        // Classify the message for model routing.
        //
        // WHY route at conversation start: Simple messages ("thanks", "yes")
        // don't need a frontier model. We resolve the route once here and
        // use it for all API calls in this conversation turn.
        let smart_routing = SmartRoutingConfig {
            enabled: config.model_config.smart_routing.enabled,
            cheap_model: config.model_config.smart_routing.cheap_model.clone(),
            cheap_base_url: config.model_config.smart_routing.cheap_base_url.clone(),
            cheap_api_key_env: config.model_config.smart_routing.cheap_api_key_env.clone(),
            thresholds: RoutingThresholds::default(),
        };
        let route = resolve_turn_route(&expansion.expanded, &config.model_config, &smart_routing);
        if let Some(ref label) = route.label {
            tracing::info!(route = %label, "model routing decision");
        }

        // If smart routing selected a cheaper model, create an alternate provider.
        // This overrides the primary provider for this turn only.
        let effective_provider = if !route.is_primary {
            if let Some((prov_name, model_name)) = route.model.split_once('/') {
                let canonical = edgecrab_tools::vision_models::normalize_provider_name(prov_name);
                // Special-case copilot: build directly to use direct API mode
                let cheap_opt: Option<Arc<dyn LLMProvider>> = if canonical == "vscode-copilot" {
                    match edgecrab_tools::create_provider_for_model(&canonical, model_name) {
                        Ok(p) => Some(p),
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to create copilot provider, using primary");
                            None
                        }
                    }
                } else {
                    // When the primary provider is Vertex AI and smart routing selects a
                    // Gemini model, preserve the Vertex AI endpoint.
                    //
                    // Without this, `create_llm_provider("google", "gemini-2.5-flash")` would
                    // call `GeminiProvider::from_env()` which prefers GEMINI_API_KEY and silently
                    // routes to Google AI Studio (free tier, 20 RPM) instead of
                    // aiplatform.googleapis.com, exhausting the quote and causing 429 errors.
                    //
                    // WHY the fix is simple now: edgequake-llm has `ProviderType::VertexAI` as a
                    // distinct variant. Passing canonical = "vertexai" calls
                    // `GeminiProvider::from_env_vertex_ai()` unconditionally — it never touches
                    // GEMINI_API_KEY.
                    let is_gemini_canonical = matches!(
                        canonical.as_str(),
                        "google" | "gemini" | "vertex" | "vertexai"
                    );
                    let primary_is_vertex = provider.name() == "vertex-ai";

                    let (effective_canonical, effective_model) =
                        if is_gemini_canonical && primary_is_vertex {
                            tracing::info!(
                                cheap_model = %route.model,
                                "smart routing: using Vertex AI endpoint for cheap Gemini model \
                                 (primary is vertex-ai)"
                            );
                            // Strip any vertexai: prefix the user may have included —
                            // the canonical "vertexai" now handles this unambiguously.
                            let bare = model_name.strip_prefix("vertexai:").unwrap_or(model_name);
                            ("vertexai", bare)
                        } else {
                            (canonical.as_str(), model_name)
                        };

                    match edgecrab_tools::create_provider_for_model(
                        effective_canonical,
                        effective_model,
                    ) {
                        Ok(p) => Some(p),
                        Err(e) => {
                            tracing::warn!(error = %e, "failed to create cheap model provider, using primary");
                            None
                        }
                    }
                };
                match cheap_opt {
                    Some(cheap) => {
                        tracing::info!(model = %route.model, "using smart-routed cheap model");
                        cheap
                    }
                    None => provider.clone(),
                }
            } else {
                provider.clone()
            }
        } else {
            provider.clone()
        };

        // Scan user input for prompt injection attempts.
        let injection_threats = crate::prompt_builder::scan_for_injection(&expansion.expanded);
        if !injection_threats.is_empty() {
            tracing::warn!(
                threats = injection_threats.len(),
                "prompt injection patterns detected in user input"
            );
            for threat in &injection_threats {
                tracing::warn!(
                    pattern = %threat.pattern_name,
                    severity = ?threat.severity,
                    "injection threat"
                );
            }
        }

        // Append the expanded user message
        session.messages.push(Message::user(&expansion.expanded));
        session.user_turn_count += 1;

        // Snapshot the tool call count at the start of this turn so we can
        // compute the per-turn delta later for the reflection gate.
        let initial_turn_tool_call_count = session.session_tool_call_count;

        // Build SubAgentRunner for delegate_task tool.
        // WHY here: We need the provider and registry to construct child agents.
        // Created once per conversation, shared across all tool dispatches.
        let sub_agent_runner: Option<Arc<dyn edgecrab_tools::SubAgentRunner>> =
            if let Some(ref registry) = self.tool_registry {
                Some(Arc::new(CoreSubAgentRunner::new(
                    provider.clone(),
                    registry.clone(),
                    config.platform,
                    config.model.clone(),
                )))
            } else {
                None
            };

        // Build clarify channel for interactive user Q&A.
        //
        // WHY: The ClarifyTool needs to pause execution and wait for the
        // user to answer a question. We create an unbounded mpsc channel
        // that the tool sends ClarifyRequest items into, and a forwarder
        // task relays them as StreamEvent::Clarify to the TUI. The TUI
        // then stores the oneshot sender and routes the user's next Enter
        // key press back to the waiting tool.
        let (clarify_req_tx, mut clarify_req_rx) =
            tokio::sync::mpsc::unbounded_channel::<edgecrab_tools::registry::ClarifyRequest>();
        let (approval_req_tx, mut approval_req_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let (delegation_req_tx, mut delegation_req_rx) =
            tokio::sync::mpsc::unbounded_channel::<DelegationEvent>();

        // Only wire the forwarder when we have a streaming event channel.
        if let Some(ev_tx) = event_tx {
            let clarify_ev_tx = ev_tx.clone();
            tokio::spawn(async move {
                while let Some(req) = clarify_req_rx.recv().await {
                    let _ = clarify_ev_tx.send(crate::StreamEvent::Clarify {
                        question: req.question,
                        choices: req.choices,
                        response_tx: req.response_tx,
                    });
                }
            });

            let approval_ev_tx = ev_tx.clone();
            tokio::spawn(async move {
                while let Some(req) = approval_req_rx.recv().await {
                    let (decision_tx, decision_rx) =
                        tokio::sync::oneshot::channel::<crate::ApprovalChoice>();
                    let _ = approval_ev_tx.send(crate::StreamEvent::Approval {
                        command: req.command,
                        full_command: req.full_command,
                        reasons: req.reasons,
                        response_tx: decision_tx,
                    });

                    let mapped = match decision_rx.await {
                        Ok(crate::ApprovalChoice::Once) => ApprovalResponse::Once,
                        Ok(crate::ApprovalChoice::Session) => ApprovalResponse::Session,
                        Ok(crate::ApprovalChoice::Always) => ApprovalResponse::Always,
                        Ok(crate::ApprovalChoice::Deny) | Err(_) => ApprovalResponse::Deny,
                    };
                    let _ = req.response_tx.send(mapped);
                }
            });

            let delegation_ev_tx = ev_tx.clone();
            tokio::spawn(async move {
                while let Some(req) = delegation_req_rx.recv().await {
                    match req {
                        DelegationEvent::TaskStarted {
                            task_index,
                            task_count,
                            goal,
                        } => {
                            let _ = delegation_ev_tx.send(crate::StreamEvent::SubAgentStart {
                                task_index,
                                task_count,
                                goal,
                            });
                        }
                        DelegationEvent::Thinking {
                            task_index,
                            task_count,
                            text,
                        } => {
                            let _ = delegation_ev_tx.send(crate::StreamEvent::SubAgentReasoning {
                                task_index,
                                task_count,
                                text,
                            });
                        }
                        DelegationEvent::ToolCalled {
                            task_index,
                            task_count,
                            tool_name,
                            args_json,
                        } => {
                            let _ = delegation_ev_tx.send(crate::StreamEvent::SubAgentToolExec {
                                task_index,
                                task_count,
                                name: tool_name,
                                args_json,
                            });
                        }
                        DelegationEvent::TaskFinished {
                            task_index,
                            task_count,
                            status,
                            duration_ms,
                            summary,
                            api_calls,
                            model,
                        } => {
                            let _ = delegation_ev_tx.send(crate::StreamEvent::SubAgentFinish {
                                task_index,
                                task_count,
                                status,
                                duration_ms,
                                summary,
                                api_calls,
                                model,
                            });
                        }
                    }
                }
            });
        }
        // In non-streaming paths (gateway, tests) just drop the receiver —
        // the tool will fall back to returning the [CLARIFY] marker.
        let clarify_tx_for_dispatch = Some(clarify_req_tx);
        let approval_tx_for_dispatch = Some(approval_req_tx);
        let delegation_tx_for_dispatch = if event_tx.is_some() {
            Some(delegation_req_tx)
        } else {
            None
        };
        let turn_started_at = std::time::Instant::now();

        // Resolve a stable session_id early — BEFORE the main tool-dispatch loop.
        //
        // WHY early: `build_tool_context` is called once per tool invocation, and
        // each call previously generated a fresh UUID for task_id, meaning every
        // tool call in a session got a *different* browser session key. This caused
        // browser_snapshot to operate on a *new blank tab* instead of the tab that
        // browser_navigate just loaded. By resolving the session_id once here and
        // passing it as `conversation_session_id` through DispatchContext, all tool
        // calls within this user turn — and across sequential turns — share the same
        // stable browser session (parity with hermes-agent's task_id="default" pattern).
        let conversation_session_id = session
            .session_id
            .clone()
            .or_else(|| config.session_id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        // Persist so the next user turn reuses the same ID.
        if session.session_id.is_none() {
            session.session_id = Some(conversation_session_id.clone());
        }
        self.publish_session_state(&session).await;

        // Main loop: each iteration = one API call
        let mut final_response = String::new();
        let mut interrupted = false;
        let mut budget_exhausted = false;
        // Accumulate per-tool-call error records — mirrors hermes AgentResult.tool_errors.
        let mut tool_errors_acc: Vec<edgecrab_types::ToolErrorRecord> = Vec::new();
        let capability_suppressions: Arc<Mutex<HashMap<String, ToolErrorResponse>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let mut tool_defs_dirty = false;
        // Track whether we already emitted a ContextPressure warning this turn
        // so we do not spam the UI on every iteration when compression fails to
        // bring usage below the warning level.
        let mut pressure_warned = false;

        loop {
            if tool_defs_dirty {
                active_tool_defs = if let Some(ref registry) = self.tool_registry {
                    let schema_ctx = build_tool_context(
                        &cwd,
                        app_config_ref.clone(),
                        &cancel,
                        &self.state_db,
                        config.platform,
                        &self.process_table,
                        Some(effective_provider.clone()),
                        self.tool_registry.clone(),
                        None,
                        None,
                        None,
                        None,
                        None,
                        self.gateway_sender.read().await.clone(),
                        config.origin_chat.clone(),
                        None,
                        None,
                        &conversation_session_id,
                        Some(self.todo_store.clone()),
                    );
                    let enabled_filter = if config.enabled_toolsets.is_empty()
                        || edgecrab_tools::toolsets::contains_all_sentinel(&config.enabled_toolsets)
                        || expanded_enabled.is_empty()
                    {
                        None
                    } else {
                        Some(expanded_enabled.as_slice())
                    };
                    let disabled_filter = if expanded_disabled.is_empty() {
                        None
                    } else {
                        Some(expanded_disabled.as_slice())
                    };
                    let schemas =
                        registry.get_definitions(enabled_filter, disabled_filter, &schema_ctx);
                    to_llm_definitions(&schemas)
                } else {
                    Vec::new()
                };
                tool_defs_dirty = false;
            }

            // Budget gate
            if !self.budget.try_consume() {
                tracing::warn!(
                    used = self.budget.used(),
                    max = self.budget.max(),
                    "iteration budget exhausted"
                );
                budget_exhausted = true;
                break;
            }

            // Cancellation gate
            if cancel.is_cancelled() {
                interrupted = true;
                break;
            }

            // Sanitize orphaned tool results before estimating prompt pressure or
            // building the next provider payload.
            sanitize_orphaned_tool_results(&mut session.messages);

            // Context compression: check status, emit pressure warning, compress if needed.
            //
            // WHY before the API call: Compressing after the call is too late —
            // the prompt has already been sent. Compressing here ensures the
            // next API call stays within the context window.
            //
            // WHY check_compression_status: Unlike the old boolean needs_compression(),
            // this returns a 3-way enum so we can emit a UI warning before firing.
            let compression_params =
                CompressionParams::from_model_config(&config.model, &config.compression);
            let estimated_prompt_tokens = estimate_request_prompt_tokens(
                session.cached_system_prompt.as_deref(),
                &session.messages,
                &active_tool_defs,
            );
            match check_compression_status_for_estimate(
                estimated_prompt_tokens,
                &compression_params,
            ) {
                CompressionStatus::NeedsCompression => {
                    tracing::info!(
                        messages = session.messages.len(),
                        estimated_prompt_tokens,
                        "compressing context before API call"
                    );
                    session.messages =
                        compress_with_llm(&session.messages, &compression_params, &provider).await;
                    // Re-inject active todo items preserved outside message history.
                    // Compression can summarize away earlier plan-tracking turns.
                    if let Some(snapshot) = self.todo_store.format_for_injection() {
                        session.messages.push(Message::user(&snapshot));
                    }
                    // Re-check: if compression succeeded, clear the pressure flag.
                    let recomputed_prompt_tokens = estimate_request_prompt_tokens(
                        session.cached_system_prompt.as_deref(),
                        &session.messages,
                        &active_tool_defs,
                    );
                    if check_compression_status_for_estimate(
                        recomputed_prompt_tokens,
                        &compression_params,
                    ) == CompressionStatus::Ok
                    {
                        pressure_warned = false;
                    }
                    self.publish_session_state(&session).await;
                }
                CompressionStatus::PressureWarning if !pressure_warned => {
                    let threshold_tokens = (compression_params.context_window as f32
                        * compression_params.threshold)
                        as usize;
                    tracing::warn!(
                        estimated_tokens = estimated_prompt_tokens,
                        threshold_tokens,
                        "context approaching compression threshold"
                    );
                    if let Some(tx) = event_tx {
                        let _ = tx.send(crate::StreamEvent::ContextPressure {
                            estimated_tokens: estimated_prompt_tokens,
                            threshold_tokens,
                        });
                    }
                    pressure_warned = true;
                }
                _ => {}
            }

            // Build edgequake-llm messages from our message history
            //
            // WHY cache config here: Anthropic prompt caching requires stable
            // cache_control breakpoints on the system prompt and last N messages.
            // We derive the config from the user's prompt_caching setting.
            let cache_cfg =
                prompt_cache_config_for(&effective_provider, config.model_config.prompt_caching);
            let chat_messages = build_chat_messages(
                session.cached_system_prompt.as_deref(),
                &session.messages,
                cache_cfg.as_ref(),
            );
            let completion_options = completion_options_for(&config);

            // API call with retry — sends tool definitions so LLM can request tool calls.
            // On failure, attempt the fallback provider if configured.
            // WHY cancel passed here: api_call_with_retry now races every sleep
            // and every API future against the CancellationToken so Ctrl+C takes
            // effect within one event-loop tick rather than after all retries finish.
            let native_streaming_active = should_use_native_streaming(
                effective_provider.as_ref(),
                &active_tool_defs,
                config.streaming,
                event_tx.is_some(),
            );
            let response = match api_call_with_retry(
                &effective_provider,
                &chat_messages,
                &active_tool_defs,
                MAX_RETRIES,
                ApiCallContext {
                    options: Some(&completion_options),
                    cancel: &cancel,
                    event_tx,
                    use_native_streaming: native_streaming_active,
                },
            )
            .await
            {
                Ok(r) => r,
                // Cancellation during the API call — break cleanly without
                // attempting the fallback provider (user wants to stop NOW).
                Err(AgentError::Interrupted) => {
                    interrupted = true;
                    break;
                }
                Err(primary_err) => {
                    // In native streaming mode, partial output may already have
                    // been shown to the user. Retrying or swapping to a fallback
                    // provider would duplicate / scramble the live transcript, so
                    // fail fast and let the user decide whether to retry.
                    if native_streaming_active {
                        self.publish_session_state(&session).await;
                        return Err(primary_err);
                    }
                    // Try fallback provider if configured
                    if let Some(ref fb) = config.model_config.fallback {
                        let fb_route = crate::model_router::fallback_route(fb);
                        tracing::warn!(
                            primary_error = %primary_err,
                            fallback = %fb_route.model,
                            "primary API failed, trying fallback"
                        );
                        if let Some((fb_prov_name, fb_model_name)) = fb_route.model.split_once('/')
                        {
                            let fb_canonical =
                                edgecrab_tools::vision_models::normalize_provider_name(
                                    fb_prov_name,
                                );
                            // Special-case copilot: build directly to use direct API mode
                            let fb_prov_opt: Option<Arc<dyn LLMProvider>> =
                                edgecrab_tools::create_provider_for_model(
                                    &fb_canonical,
                                    fb_model_name,
                                )
                                .ok();
                            if let Some(fb_prov) = fb_prov_opt {
                                let fallback_native_streaming = should_use_native_streaming(
                                    fb_prov.as_ref(),
                                    &active_tool_defs,
                                    config.streaming,
                                    event_tx.is_some(),
                                );
                                match api_call_with_retry(
                                    &fb_prov,
                                    &chat_messages,
                                    &active_tool_defs,
                                    1,
                                    ApiCallContext {
                                        options: Some(&completion_options),
                                        cancel: &cancel,
                                        event_tx,
                                        use_native_streaming: fallback_native_streaming,
                                    },
                                )
                                .await
                                {
                                    Ok(r) => r,
                                    // Also handle cancellation during fallback call.
                                    Err(AgentError::Interrupted) => {
                                        interrupted = true;
                                        break;
                                    }
                                    Err(fb_err) => {
                                        tracing::error!(fallback_error = %fb_err, "fallback also failed");
                                        self.publish_session_state(&session).await;
                                        return Err(primary_err);
                                    }
                                }
                            } else {
                                self.publish_session_state(&session).await;
                                return Err(primary_err);
                            }
                        } else {
                            self.publish_session_state(&session).await;
                            return Err(primary_err);
                        }
                    } else {
                        self.publish_session_state(&session).await;
                        return Err(primary_err);
                    }
                }
            };

            // ── Post-call cancellation check ──────────────────────────────
            // The API call returned successfully but the token may have been
            // triggered while we were processing the response bytes. Break
            // before dispatching any tool calls so the agent stops immediately.
            if cancel.is_cancelled() {
                interrupted = true;
                break;
            }

            session.api_call_count += 1;

            // Track usage — prompt, completion, cache, and reasoning tokens
            session.session_input_tokens += response.prompt_tokens as u64;
            session.session_output_tokens += response.completion_tokens as u64;
            if let Some(cache_tokens) = response.cache_hit_tokens {
                session.session_cache_read_tokens += cache_tokens as u64;
            }
            if let Some(reasoning_tokens) = response.thinking_tokens {
                session.session_reasoning_tokens += reasoning_tokens as u64;
            }
            session.last_prompt_tokens =
                response.prompt_tokens as u64 + response.cache_hit_tokens.unwrap_or(0) as u64;

            // Empty response nudge: if the LLM returned no content and no
            // tool calls, inject a "please continue" prompt and retry.
            if response.content.trim().is_empty()
                && !response.has_tool_calls()
                && response.finish_reason.as_deref() != Some("length")
            {
                tracing::info!("empty response from LLM, nudging to continue");
                session.messages.push(Message::user(
                    "[system: your response was empty — please provide a response]",
                ));
                continue;
            }

            // Process response
            let dctx = DispatchContext {
                cwd: cwd.clone(),
                registry: self.tool_registry.as_ref(),
                cancel: &cancel,
                state_db: &self.state_db,
                platform: config.platform,
                process_table: &self.process_table,
                provider: Some(provider.clone()),
                gateway_sender: self.gateway_sender.read().await.clone(),
                sub_agent_runner: sub_agent_runner.clone(),
                event_tx,
                delegation_event_tx: delegation_tx_for_dispatch.clone(),
                clarify_tx: clarify_tx_for_dispatch.clone(),
                approval_tx: approval_tx_for_dispatch.clone(),
                origin_chat: config.origin_chat.clone(),
                app_config_ref: app_config_ref.clone(),
                conversation_session_id: conversation_session_id.clone(),
                todo_store: Some(self.todo_store.clone()),
                capability_suppressions: capability_suppressions.clone(),
            };
            let action = match process_response(
                &response,
                &mut session,
                &dctx,
                &mut tool_errors_acc,
            )
            .await
            {
                Ok(action) => action,
                Err(err) => {
                    self.publish_session_state(&session).await;
                    return Err(err);
                }
            };
            self.publish_session_state(&session).await;

            match action {
                LoopAction::Done(text) => {
                    // Length truncation continuation: if finish_reason is "length",
                    // the model was cut off mid-response. Auto-continue by appending
                    // the partial text and asking for more.
                    if response.finish_reason.as_deref() == Some("length") {
                        tracing::info!(
                            partial_len = text.len(),
                            "response truncated (finish_reason=length), auto-continuing"
                        );
                        // The partial response is already in session.messages
                        // (appended by process_response). Add a continuation nudge.
                        session.messages.push(Message::user(
                            "[system: your response was truncated due to length — please continue exactly where you left off]",
                        ));
                        continue;
                    }
                    final_response = text;
                    break;
                }
                LoopAction::Continue => {
                    // Tool results have been appended to session.messages.
                    // Inject budget pressure warning if approaching iteration limit.
                    if let Some(warning) =
                        get_budget_warning(session.api_call_count, config.max_iterations)
                    {
                        inject_budget_warning(&mut session.messages, &warning);
                    }
                    tool_defs_dirty = true;
                    self.publish_session_state(&session).await;
                    continue;
                }
            }
        }

        // ─── Synthesize fallback response when budget exhausted ──────────────
        // If the loop exited via budget exhaustion (not cancellation) and the
        // agent never produced a text response (it was mid-tool-chain), inject a
        // synthetic fallback so callers always receive a non-empty string.
        //
        // WHY: Without this, `chat()` returns `Ok("")` — the user sees nothing
        // and the session appears to have silently failed.  Hermes-agent mirrors
        // this pattern, returning a budget-exhausted notice.
        if budget_exhausted && final_response.is_empty() {
            let msg = format!(
                "[Agent reached the {} iteration limit before completing the task. \
                 Please try rephrasing your request or increase the iteration budget.]",
                self.budget.max()
            );
            tracing::warn!(
                max = self.budget.max(),
                "emitting budget-exhausted fallback response"
            );
            // Push as an assistant message so history is consistent.
            session.messages.push(Message::assistant(&msg));
            if let Some(tx) = event_tx {
                let _ = tx.send(crate::StreamEvent::Token(msg.clone()));
            }
            final_response = msg;
        }

        // ─── Learning reflection ──────────────────────────────────────────
        // If this session involved 5+ tool calls (complex task) and the agent
        // has a tool registry with skill_manage / memory_write available,
        // run one extra "reflection turn". The agent decides whether to save
        // a new skill, patch an existing one, or update memory — closing the
        // learning loop without any human intervention.
        //
        // WHY: Mirrors hermes-agent's explicit reflection step. The system
        // prompt already has SKILLS_GUIDANCE, but a targeted nudge at session
        // end is more reliable than hoping the LLM fires proactively.
        //
        // Design choices:
        //  - Only fires when tool_call_count ≥ SKILL_REFLECTION_THRESHOLD.
        //  - Gated on !config.skip_memory (respects opt-out).
        //  - Non-fatal: if the extra API call fails the session still succeeds.
        //  - Does NOT count toward iteration budget (session already complete).
        //  - Reflection messages ARE persisted so the user can inspect them.
        // WHY turn_tool_calls instead of session_tool_call_count:
        // session_tool_call_count accumulates across ALL turns in the same
        // session. Using it for the reflection threshold means subsequent
        // turns trigger reflection even when the current turn made zero tool
        // calls (e.g. a second "Hello" after a heavy research turn). Instead
        // we compute the delta for this turn only.
        let turn_tool_calls = session
            .session_tool_call_count
            .saturating_sub(initial_turn_tool_call_count);

        if !interrupted
            && !config.skip_memory
            && self.tool_registry.is_some()
            && turn_tool_calls >= SKILL_REFLECTION_THRESHOLD
        {
            // WHY tokio::spawn (fire-and-forget):
            // Mirrors hermes-agent's `_spawn_background_review()` which runs in a
            // detached daemon thread. The reflection makes a separate API call to
            // the LLM which can take several seconds (or minutes on reasoning
            // models like grok-4). Awaiting it inline would delay `StreamEvent::Done`
            // — keeping `is_processing = true` in the TUI until reflection finishes.
            // By spawning detached, `execute_loop` returns immediately, the TUI
            // unlocks, and reflection (skill_manage / memory_write) runs silently
            // in the background.
            //
            // WHY snapshot ownership: `DispatchContext<'a>` holds references with
            // lifetimes tied to `self` and local variables. A detached `tokio::spawn`
            // needs a `'static` future. We clone lightweight Arc handles and plain
            // data, then borrow from the owned `BackgroundReflectionCtx` struct
            // inside the task.
            let bg_ctx = BackgroundReflectionCtx {
                messages: session.messages.clone(),
                system_prompt: session.cached_system_prompt.clone(),
                tool_defs: active_tool_defs.clone(),
                cwd: cwd.clone(),
                registry: self.tool_registry.as_ref().map(Arc::clone),
                cancel: cancel.clone(),
                state_db: self.state_db.clone(),
                platform: config.platform,
                process_table: Arc::clone(&self.process_table),
                provider: Arc::clone(&effective_provider),
                gateway_sender: self.gateway_sender.read().await.clone(),
                sub_agent_runner: sub_agent_runner.clone(),
                app_config_ref: app_config_ref.clone(),
                conversation_session_id: conversation_session_id.clone(),
                origin_chat: config.origin_chat.clone(),
                todo_store: Some(self.todo_store.clone()),
            };
            tokio::spawn(run_learning_reflection_bg(bg_ctx));
        }
        self.publish_session_state(&session).await;

        // Resolve session_id: prefer SessionState's, then config's, then generate.
        let session_id = session
            .session_id
            .clone()
            .or_else(|| config.session_id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        // Store into session state for downstream use (slash commands, etc.)
        session.session_id = Some(session_id.clone());
        self.publish_session_state(&session).await;

        let usage = Usage {
            input_tokens: session.session_input_tokens,
            output_tokens: session.session_output_tokens,
            cache_read_tokens: session.session_cache_read_tokens,
            cache_write_tokens: session.session_cache_write_tokens,
            reasoning_tokens: session.session_reasoning_tokens,
            ..Default::default()
        };

        // Compute cost estimate from the pricing engine.
        // config.model is already in "provider/model" format (e.g. "anthropic/claude-sonnet-4-20250514")
        let canonical_usage = CanonicalUsage {
            input_tokens: session.session_input_tokens,
            output_tokens: session.session_output_tokens,
            cache_read_tokens: session.session_cache_read_tokens,
            cache_write_tokens: session.session_cache_write_tokens,
            reasoning_tokens: session.session_reasoning_tokens,
        };
        let cost_result = estimate_cost(&canonical_usage, &config.model);
        let cost = Cost {
            input_cost: canonical_usage.input_tokens as f64 * cost_result.amount_usd.unwrap_or(0.0)
                / canonical_usage.total_tokens().max(1) as f64,
            output_cost: canonical_usage.output_tokens as f64
                * cost_result.amount_usd.unwrap_or(0.0)
                / canonical_usage.total_tokens().max(1) as f64,
            total_cost: cost_result.amount_usd.unwrap_or(0.0),
            ..Default::default()
        };

        // Persist session to SQLite state DB.
        //
        // WHY async persist after loop: We don't want DB latency affecting
        // the REACT loop's interactivity. Persisting once at the end is
        // both cheaper (one write) and safe — incomplete sessions won't
        // appear in `/history` if the process is killed mid-loop.
        if let Some(ref db) = self.state_db {
            let title = session
                .messages
                .iter()
                .find(|m| m.role == Role::User)
                .map(|m| {
                    let t = m.text_content();
                    if t.len() > 80 {
                        format!("{}…", crate::safe_truncate(&t, 80))
                    } else {
                        t
                    }
                })
                .unwrap_or_else(|| "Untitled session".to_string());

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            let (source, routing_key) = match &config.origin_chat {
                Some((platform, chat_id)) => (platform.clone(), Some(chat_id.clone())),
                None => ("cli".to_string(), None),
            };

            let record = edgecrab_state::SessionRecord {
                id: session_id.clone(),
                source,
                user_id: routing_key,
                model: Some(config.model.clone()),
                system_prompt: session.cached_system_prompt.clone(),
                parent_session_id: None,
                started_at: now,
                ended_at: Some(now),
                end_reason: if interrupted {
                    Some("interrupted".to_string())
                } else {
                    None
                },
                message_count: session.messages.len() as i64,
                tool_call_count: session.session_tool_call_count as i64,
                input_tokens: session.session_input_tokens as i64,
                output_tokens: session.session_output_tokens as i64,
                cache_read_tokens: session.session_cache_read_tokens as i64,
                cache_write_tokens: session.session_cache_write_tokens as i64,
                reasoning_tokens: session.session_reasoning_tokens as i64,
                estimated_cost_usd: cost_result.amount_usd,
                title: Some(title),
            };

            if let Err(e) = db.save_session(&record) {
                tracing::warn!(error = %e, "failed to save session to state DB");
            }

            if let Err(e) = db.replace_messages(&session_id, &session.messages, now) {
                tracing::warn!(error = %e, "failed to save messages to state DB");
            }

            // Auto-title: on the first exchange, spawn a background task that
            // calls the LLM to generate a short, descriptive 3-7 word title.
            // This mirrors hermes-agent's `maybe_auto_title()` pattern.
            // Only fires when this is the first user turn (session.user_turn_count == 1).
            if session.user_turn_count == 1 && !final_response.is_empty() {
                let user_snippet = user_message.chars().take(500).collect::<String>();
                let asst_snippet = final_response.chars().take(500).collect::<String>();
                let db_clone = db.clone();
                let sid_clone = session_id.clone();
                let prov_clone = effective_provider.clone();
                tokio::spawn(async move {
                    auto_title_session(db_clone, sid_clone, user_snippet, asst_snippet, prov_clone)
                        .await;
                });
            }
        }

        let completed = !interrupted && !final_response.trim().is_empty();
        if config.save_trajectories {
            let trajectory_dir = edgecrab_home().join("trajectories");
            let trajectory_path = trajectory_dir.join(if completed {
                "trajectory_samples.jsonl"
            } else {
                "failed_trajectories.jsonl"
            });

            if let Err(e) = std::fs::create_dir_all(&trajectory_dir) {
                tracing::warn!(error = %e, path = %trajectory_dir.display(), "failed to create trajectory directory");
            } else {
                let trajectory = build_trajectory(
                    &session_id,
                    &config.model,
                    &session.messages,
                    session.api_call_count,
                    cost.total_cost,
                    completed,
                    turn_started_at.elapsed().as_secs_f64(),
                );
                if let Err(e) = save_trajectory(&trajectory_path, &trajectory) {
                    tracing::warn!(error = %e, path = %trajectory_path.display(), "failed to save trajectory");
                }
            }
        }

        let messages = session.messages.clone();
        let api_calls = session.api_call_count;
        let model = config.model.clone();

        Ok(ConversationResult {
            final_response,
            messages,
            session_id,
            api_calls,
            interrupted,
            budget_exhausted,
            model,
            usage,
            cost,
            tool_errors: tool_errors_acc,
        })
    }
}

/// Build edgequake-llm ChatMessages from our internal message list.
///
/// WHY careful role mapping matters:
/// ```text
///   Internal Role    edgequake-llm ChatMessage
///   ─────────────    ──────────────────────────
///   System        →  ChatMessage::system(text)
///   User          →  ChatMessage::user(text)
///   Assistant     →  ChatMessage::assistant(text)
///     (with tool_calls) → ChatMessage::assistant_with_tools(text, tool_calls)
///   Tool          →  ChatMessage::tool_result(tool_call_id, text)
/// ```
/// Without this mapping, providers can't correlate tool results with
/// the assistant message that requested them, breaking multi-turn
/// tool calling.
///
/// Public so agent.rs streaming path can reuse it.
///
/// WHY cache_config param: Anthropic prompt caching requires
/// `cache_control: ephemeral` breakpoints on the system message
/// and last N user messages. edgequake-llm's `apply_cache_control()`
/// injects these markers. We call it here so both the conversation
/// loop and streaming path get consistent cache annotations.
pub fn build_chat_messages(
    system_prompt: Option<&str>,
    messages: &[Message],
    cache_config: Option<&CachePromptConfig>,
) -> Vec<edgequake_llm::ChatMessage> {
    let mut out = Vec::with_capacity(messages.len() + 1);

    // Prepend system message
    if let Some(sys) = system_prompt {
        out.push(edgequake_llm::ChatMessage::system(sys));
    }

    for m in messages {
        let text = m.text_content();
        match m.role {
            Role::System => out.push(edgequake_llm::ChatMessage::system(&text)),
            Role::User => out.push(edgequake_llm::ChatMessage::user(&text)),
            Role::Assistant => {
                if let Some(ref tool_calls) = m.tool_calls {
                    if !tool_calls.is_empty() {
                        // Convert our ToolCall → edgequake-llm ToolCall
                        let llm_calls: Vec<edgequake_llm::ToolCall> =
                            tool_calls.iter().map(|tc| tc.to_llm()).collect();
                        out.push(edgequake_llm::ChatMessage::assistant_with_tools(
                            &text, llm_calls,
                        ));
                        continue;
                    }
                }
                out.push(edgequake_llm::ChatMessage::assistant(&text));
            }
            Role::Tool => {
                // Map tool result messages with their tool_call_id for correlation.
                let tool_call_id = m.tool_call_id.as_deref().unwrap_or("unknown");
                let mut chat_msg = edgequake_llm::ChatMessage::tool_result(tool_call_id, &text);
                // Propagate the tool function name so Gemini/VertexAI providers can
                // build the correct FunctionResponse.name in convert_messages().
                // The name is stored in Message::name by Message::tool_result().
                chat_msg.name = m.name.clone();
                out.push(chat_msg);
            }
        }
    }

    // Inject Anthropic cache_control breakpoints when prompt caching is enabled.
    // System message → always cacheable; last N user messages → breakpoints.
    if let Some(cfg) = cache_config {
        apply_cache_control(&mut out, cfg);
    }

    out
}

fn provider_supports_prompt_caching(provider_name: &str) -> bool {
    matches!(provider_name, "anthropic")
}

fn prompt_cache_config_for(
    provider: &Arc<dyn LLMProvider>,
    prompt_caching_enabled: bool,
) -> Option<CachePromptConfig> {
    (prompt_caching_enabled && provider_supports_prompt_caching(provider.name()))
        .then(CachePromptConfig::default)
}

fn augment_provider_error(provider: &Arc<dyn LLMProvider>, error: String) -> String {
    if provider.name() == "vscode-copilot" && error.contains("api.githubcopilot.com") {
        return format!(
            "{error} GitHub Copilot direct mode could not reach the remote API. If you rely on a local Copilot proxy, set `VSCODE_COPILOT_DIRECT=false` or configure `VSCODE_COPILOT_PROXY_URL`."
        );
    }
    error
}

/// Check whether a tool result string represents an error.
///
/// Extracted to eliminate the duplicated condition in the parallel and
/// sequential dispatch arms of `process_response`.
#[inline]
fn parse_tool_error_response(result: &str) -> Option<ToolErrorResponse> {
    let parsed = serde_json::from_str::<ToolErrorResponse>(result).ok()?;
    (parsed.response_type == "tool_error").then_some(parsed)
}

fn tool_attempt_fingerprint(name: &str, args_json: &str) -> String {
    let normalized_args = serde_json::from_str::<serde_json::Value>(args_json)
        .ok()
        .and_then(|value| serde_json::to_string(&value).ok())
        .unwrap_or_else(|| args_json.trim().to_string());
    format!("{name}:{normalized_args}")
}

fn suppressed_retry_response(
    name: &str,
    args_json: &str,
    prior: &ToolErrorResponse,
) -> ToolErrorResponse {
    let mut error = ToolError::capability_denied(
        name,
        "suppressed_capability_retry",
        format!(
            "EdgeCrab already blocked an equivalent `{name}` call earlier in this conversation because `{}` is unresolved. Repeating the same call would be flaky. Change the approach or complete the required user action first.",
            prior.code
        ),
    )
    .with_suppression_key(tool_attempt_fingerprint(name, args_json));

    if let Some(suggested_tool) = &prior.suggested_tool {
        error = error.with_suggested_tool(suggested_tool.clone());
    }
    if let Some(suggested_action) = &prior.suggested_action {
        error = error.with_suggested_action(suggested_action.clone());
    }
    error.to_llm_payload()
}

#[inline]
fn is_tool_error(result: &str) -> bool {
    parse_tool_error_response(result).is_some() || result.starts_with("Tool error:")
}

/// Emit a `ToolDone` stream event if a subscriber is connected.
///
/// Extracted because the identical `if let Some(tx) = ...` block appeared
/// in both the parallel-results loop and the sequential-dispatch loop inside
/// `process_response`.
fn emit_tool_done(
    tx: Option<&tokio::sync::mpsc::UnboundedSender<crate::StreamEvent>>,
    tool_call_id: &str,
    name: &str,
    args_json: &str,
    tool_result: &str,
    duration_ms: u64,
    is_error: bool,
) {
    if let Some(tx) = tx {
        let _ = tx.send(crate::StreamEvent::ToolDone {
            tool_call_id: tool_call_id.to_string(),
            name: name.to_string(),
            args_json: args_json.to_string(),
            result_preview: summarize_tool_result_preview(name, tool_result, is_error),
            duration_ms,
            is_error,
        });
    }
}

fn make_tool_progress_tx(
    event_tx: Option<&tokio::sync::mpsc::UnboundedSender<crate::StreamEvent>>,
) -> Option<tokio::sync::mpsc::UnboundedSender<edgecrab_tools::ToolProgressUpdate>> {
    let event_tx = event_tx.cloned()?;
    let (tool_progress_tx, mut tool_progress_rx) =
        tokio::sync::mpsc::unbounded_channel::<edgecrab_tools::ToolProgressUpdate>();
    tokio::spawn(async move {
        while let Some(update) = tool_progress_rx.recv().await {
            let _ = event_tx.send(crate::StreamEvent::ToolProgress {
                tool_call_id: update.tool_call_id,
                name: update.tool_name,
                message: update.message,
            });
        }
    });
    Some(tool_progress_tx)
}

fn summarize_tool_result_preview(name: &str, tool_result: &str, is_error: bool) -> Option<String> {
    fn first_nonempty_line(text: &str) -> Option<String> {
        text.lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(ToOwned::to_owned)
    }

    fn truncate(text: &str, limit: usize) -> String {
        crate::safe_truncate(text, limit).to_string()
    }

    if is_error {
        let line = extract_tool_error_text(tool_result);
        let line = if line.trim().is_empty() {
            first_nonempty_line(tool_result)?
        } else {
            line
        };
        return Some(truncate(&line, 88));
    }

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(tool_result) {
        if let Some(obj) = value.as_object() {
            for key in ["summary", "message", "status", "result", "path"] {
                if let Some(text) = obj.get(key).and_then(|v| v.as_str()) {
                    let text = text.trim();
                    if !text.is_empty() {
                        return Some(truncate(text, 88));
                    }
                }
            }
        }
    }

    if name == "terminal" {
        let mut lines = tool_result.lines();
        let _header = lines.next();
        let body = lines
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with("exit code:"));
        if let Some(body) = body {
            return Some(truncate(body, 88));
        }
        if let Some(exit_line) = tool_result
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("exit code:"))
        {
            return Some(truncate(exit_line, 88));
        }
    }

    first_nonempty_line(tool_result).map(|line| truncate(&line, 88))
}

#[derive(Default)]
struct PartialToolCall {
    id: Option<String>,
    function_name: Option<String>,
    arguments: String,
    thought_signature: Option<String>,
}

fn finalize_streamed_tool_calls(
    partials: BTreeMap<usize, PartialToolCall>,
) -> edgequake_llm::Result<Vec<edgequake_llm::ToolCall>> {
    partials
        .into_iter()
        .map(|(index, partial)| {
            let id = partial.id.unwrap_or_else(|| format!("stream_call_{index}"));
            let function_name = partial.function_name.ok_or_else(|| {
                edgequake_llm::LlmError::ApiError(format!(
                    "streamed tool call {id} finished without a function name"
                ))
            })?;
            let arguments = partial.arguments.trim();
            if arguments.is_empty() {
                return Err(edgequake_llm::LlmError::ApiError(format!(
                    "streamed tool call {id} ({function_name}) finished without arguments"
                )));
            }

            let parsed: serde_json::Value = serde_json::from_str(arguments).map_err(|err| {
                edgequake_llm::LlmError::ApiError(format!(
                    "streamed tool call {id} ({function_name}) produced invalid JSON arguments: \
                     {err}"
                ))
            })?;
            if !parsed.is_object() {
                return Err(edgequake_llm::LlmError::ApiError(format!(
                    "streamed tool call {id} ({function_name}) arguments must be a JSON object"
                )));
            }

            Ok(edgequake_llm::ToolCall {
                id,
                call_type: "function".to_string(),
                function: edgequake_llm::FunctionCall {
                    name: function_name,
                    arguments: arguments.to_string(),
                },
                thought_signature: partial.thought_signature,
            })
        })
        .collect()
}

/// Native provider-streaming path used by the TUI.
///
/// WHY separate helper: real-time streaming and tool-call accumulation are a
/// different concern from retry logic. This function turns provider-native
/// `StreamChunk`s back into one normalized `LLMResponse` while also forwarding
/// text/reasoning deltas to the UI.
async fn api_call_streaming(
    provider: &Arc<dyn LLMProvider>,
    messages: &[edgequake_llm::ChatMessage],
    tool_defs: &[edgequake_llm::ToolDefinition],
    options: Option<&edgequake_llm::CompletionOptions>,
    event_tx: &tokio::sync::mpsc::UnboundedSender<crate::StreamEvent>,
    any_tokens_sent: &std::sync::atomic::AtomicBool,
) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
    let mut stream = provider
        .chat_with_tools_stream(messages, tool_defs, None, options)
        .await?;

    let mut content = String::new();
    let mut thinking = String::new();
    let mut thinking_tokens = 0usize;
    let mut final_usage: Option<StreamUsage> = None;
    let mut finish_reason: Option<String> = None;
    let mut tool_calls: BTreeMap<usize, PartialToolCall> = BTreeMap::new();

    while let Some(chunk) = stream.next().await {
        match chunk? {
            StreamChunk::Content(delta) => {
                if !delta.is_empty() {
                    any_tokens_sent.store(true, std::sync::atomic::Ordering::Relaxed);
                    content.push_str(&delta);
                    let _ = event_tx.send(crate::StreamEvent::Token(delta));
                }
            }
            StreamChunk::ThinkingContent {
                text, tokens_used, ..
            } => {
                if !text.is_empty() {
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
                let entry = tool_calls.entry(index).or_default();
                if let Some(id) = id {
                    entry.id = Some(id);
                }
                if let Some(name) = function_name {
                    entry.function_name = Some(name);
                }
                if let Some(args) = function_arguments {
                    entry.arguments.push_str(&args);
                }
                if thought_signature.is_some() {
                    entry.thought_signature = thought_signature;
                }
            }
            StreamChunk::Finished { reason, usage, .. } => {
                finish_reason = Some(reason);
                if usage.is_some() {
                    final_usage = usage;
                }
            }
        }
    }

    let mut response = edgequake_llm::LLMResponse::new(content, provider.model().to_string());
    if let Some(reason) = finish_reason {
        response.finish_reason = Some(reason);
    }
    if !thinking.is_empty() {
        response.thinking_content = Some(thinking);
    }

    response.tool_calls = finalize_streamed_tool_calls(tool_calls)?;

    if let Some(usage) = final_usage {
        response = response.with_usage(usage.prompt_tokens, usage.completion_tokens);
        if let Some(cache_hit_tokens) = usage.cache_hit_tokens {
            response = response.with_cache_hit_tokens(cache_hit_tokens);
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

fn estimate_stream_prompt_tokens(
    messages: &[edgequake_llm::ChatMessage],
    tool_defs: &[edgequake_llm::ToolDefinition],
) -> usize {
    estimate_tokens_from_json(&(messages, tool_defs))
}

fn estimate_request_prompt_tokens(
    system_prompt: Option<&str>,
    messages: &[Message],
    tool_defs: &[edgequake_llm::ToolDefinition],
) -> usize {
    let chat_messages = build_chat_messages(system_prompt, messages, None);
    estimate_stream_prompt_tokens(&chat_messages, tool_defs)
}

fn available_toolsets_for_prompt(
    registry: &edgecrab_tools::registry::ToolRegistry,
    tool_names: &[String],
) -> Vec<String> {
    let mut toolsets: Vec<String> = tool_names
        .iter()
        .filter_map(|name| registry.toolset_for_tool(name))
        .collect();
    toolsets.sort();
    toolsets.dedup();
    toolsets
}

fn estimate_stream_completion_tokens(
    content: &str,
    thinking: Option<&str>,
    tool_calls: &[edgequake_llm::ToolCall],
) -> usize {
    estimate_tokens_from_json(&(content, thinking, tool_calls))
}

fn estimate_tokens_from_json<T: serde::Serialize>(value: &T) -> usize {
    let serialized = match serde_json::to_string(value) {
        Ok(serialized) => serialized,
        Err(_) => return 0,
    };
    estimate_tokens_from_text(&serialized)
}

fn estimate_tokens_from_text(text: &str) -> usize {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0;
    }

    // Heuristic fallback only: most modern BPE tokenizers average ~4 chars/token
    // on mixed code+English payloads. This is intentionally used only when the
    // provider cannot supply authoritative streaming usage.
    trimmed.chars().count().div_ceil(4)
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
async fn api_call_with_retry(
    provider: &Arc<dyn LLMProvider>,
    messages: &[edgequake_llm::ChatMessage],
    tool_defs: &[edgequake_llm::ToolDefinition],
    max_retries: u32,
    ctx: ApiCallContext<'_>,
) -> Result<edgequake_llm::LLMResponse, AgentError> {
    let mut last_err = None;
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

    for attempt in 0..=retry_budget {
        // ── Backoff sleep — interruptible ──────────────────────────────
        if attempt > 0 {
            let delay = BASE_BACKOFF * 2u32.saturating_pow(attempt - 1);
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
        let tokens_sent = std::sync::atomic::AtomicBool::new(false);

        // ── API call — interruptible ────────────────────────────────────
        // We race the provider future against the cancel token.
        // Dropping the provider future is safe: HTTP futures in reqwest
        // are cancel-safe (the TCP connection is simply closed).

        // Emit llm:pre hook event (fire-and-forget, informational)
        if let Some(tx) = ctx.event_tx {
            let ctx_json = serde_json::json!({
                "event": "llm:pre",
                "model": provider.model(),
                "attempt": attempt,
            })
            .to_string();
            let _ = tx.send(crate::StreamEvent::HookEvent {
                event: "llm:pre".to_string(),
                context_json: ctx_json,
            });
        }

        let call_fut = async {
            if ctx.use_native_streaming {
                let tx = ctx
                    .event_tx
                    .expect("native streaming requires event channel");
                api_call_streaming(provider, messages, tool_defs, ctx.options, tx, &tokens_sent)
                    .await
            } else if tool_defs.is_empty() {
                provider.chat(messages, ctx.options).await
            } else {
                provider
                    .chat_with_tools(messages, tool_defs, None, ctx.options)
                    .await
            }
        };

        let result = tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => {
                tracing::debug!(attempt, "api_call_with_retry: cancelled during API call");
                return Err(AgentError::Interrupted);
            }
            r = call_fut => r,
        };

        match result {
            Ok(response) => {
                // Emit llm:post hook event
                if let Some(tx) = ctx.event_tx {
                    let ctx_json = serde_json::json!({
                        "event": "llm:post",
                        "model": provider.model(),
                        "prompt_tokens": response.prompt_tokens,
                        "completion_tokens": response.completion_tokens,
                    })
                    .to_string();
                    let _ = tx.send(crate::StreamEvent::HookEvent {
                        event: "llm:post".to_string(),
                        context_json: ctx_json,
                    });
                }
                return Ok(response);
            }
            Err(e) => {
                tracing::warn!(attempt, error = %e, "API call failed");

                let provider_handles_error = provider_manages_transport_retries(provider.as_ref())
                    && is_transport_retry_error(&e);

                // For native streaming, only continue retrying if the error
                // happened before any visible output was emitted. Tool-call
                // deltas are buffered locally and are not user-visible, so a
                // malformed streamed tool call can safely be retried.
                if ctx.use_native_streaming {
                    let visible_output_sent =
                        tokens_sent.load(std::sync::atomic::Ordering::Relaxed);
                    if visible_output_sent || !is_retryable_nonvisible_stream_error(&e) {
                        let err = augment_provider_error(provider, e.to_string());
                        return Err(AgentError::Llm(format!(
                            "API call failed after {} retries: {}",
                            attempt, err
                        )));
                    }
                }

                last_err = Some(e);
                if provider_handles_error {
                    break;
                }
            }
        }
    }

    Err(AgentError::Llm(format!(
        "API call failed after {} retries: {}",
        retry_budget,
        last_err.map_or_else(
            || "unknown error".to_string(),
            |e| augment_provider_error(provider, e.to_string())
        )
    )))
}

// ─── Budget pressure warnings ─────────────────────────────────────────────

/// Compute a budget pressure warning string when approaching max iterations.
///
/// WHY: Mirrors hermes-agent's `_get_budget_warning()` method. When the LLM
/// is approaching its iteration limit, injecting a text warning nudges it to
/// wrap up without making multi-step tool chains.
///
/// - ≥ 90% → URGENT: you must respond now
/// - ≥ 70% → BUDGET: start wrapping up
/// - < 70% → None
fn get_budget_warning(api_call_count: u32, max_iterations: u32) -> Option<String> {
    if max_iterations == 0 {
        return None;
    }
    let progress = api_call_count as f64 / max_iterations as f64;
    if progress >= 0.9 {
        Some(format!(
            "[URGENT: {}% of iteration budget used ({}/{}). You MUST provide a final response NOW — do not make further tool calls.]",
            (progress * 100.0) as u32,
            api_call_count,
            max_iterations
        ))
    } else if progress >= 0.7 {
        Some(format!(
            "[BUDGET: {}% of iteration budget used ({}/{}). Start wrapping up — avoid multi-step tool chains.]",
            (progress * 100.0) as u32,
            api_call_count,
            max_iterations
        ))
    } else {
        None
    }
}

/// Inject a budget pressure warning into the last tool result message.
///
/// WHY inject into tool result: The warning must appear as part of a message
/// the LLM already received (the last tool result), not as a new user message.
/// Inserting it as a new user message would break the message structure by
/// placing a user message between tool results and the next assistant turn.
///
/// Mirrors hermes-agent's `_inject_budget_warning` pattern: tries to add a
/// `_budget_warning` field to JSON content, falls back to appending plain text.
///
/// WHY fallback to user message: If there are no tool messages (pure text
/// conversation), a warning appended to a non-existent tool message would be
/// silently dropped. In that case we inject a plain user message so the LLM
/// still receives the pressure signal.
fn inject_budget_warning(messages: &mut Vec<Message>, warning: &str) {
    // Find the last tool-result message
    if let Some(msg) = messages.iter_mut().rev().find(|m| m.role == Role::Tool) {
        let current = msg.text_content();
        // Try to inject into JSON object
        let new_content = if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&current) {
            if let Some(obj) = v.as_object_mut() {
                obj.insert(
                    "_budget_warning".to_string(),
                    serde_json::Value::String(warning.to_string()),
                );
                serde_json::to_string(&v).unwrap_or_else(|_| format!("{}\n\n{}", current, warning))
            } else {
                format!("{}\n\n{}", current, warning)
            }
        } else {
            format!("{}\n\n{}", current, warning)
        };
        msg.content = Some(Content::Text(new_content));
    } else {
        // No tool messages in history — inject as a user message so the LLM
        // still receives the budget signal even in pure text conversations.
        tracing::debug!("no tool messages found, injecting budget warning as user message");
        messages.push(Message::user(warning));
    }
}

fn build_trajectory(
    session_id: &str,
    model: &str,
    messages: &[Message],
    api_calls: u32,
    total_cost: f64,
    completed: bool,
    duration_seconds: f64,
) -> Trajectory {
    let normalized_messages = normalize_messages_for_trajectory(messages);
    let total_tokens = normalized_messages
        .iter()
        .map(|message| message.text_content().len() as u64 / 4)
        .sum();

    Trajectory {
        session_id: session_id.to_string(),
        model: model.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        messages: normalized_messages,
        metadata: TrajectoryMetadata {
            task_id: None,
            total_tokens,
            total_cost,
            api_calls,
            tools_used: collect_used_tools(messages),
            completed,
            duration_seconds,
        },
    }
}

fn normalize_messages_for_trajectory(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .cloned()
        .map(|mut message| {
            if let Some(Content::Text(text)) = &message.content {
                message.content = Some(Content::Text(convert_scratchpad_to_think(text)));
            }
            if let Some(reasoning) = &message.reasoning {
                message.reasoning = Some(convert_scratchpad_to_think(reasoning));
            }
            message
        })
        .collect()
}

fn collect_used_tools(messages: &[Message]) -> Vec<String> {
    let mut tools = Vec::new();
    for message in messages.iter().filter(|message| message.role == Role::Tool) {
        if let Some(name) = &message.name {
            if !tools.iter().any(|existing| existing == name) {
                tools.push(name.clone());
            }
        }
    }
    tools
}

/// Extract the human-readable error message from a tool result string.
///
/// Tries JSON `"error"` field first (structured), falls back to the raw
/// string (which already starts with "Tool error:" from dispatch_single_tool).
#[inline]
fn extract_tool_error_text(result: &str) -> String {
    if let Some(payload) = parse_tool_error_response(result) {
        return payload.error;
    }
    result.to_string()
}

fn remember_tool_suppression(
    suppressions: &Arc<Mutex<HashMap<String, ToolErrorResponse>>>,
    name: &str,
    args_json: &str,
    result: &str,
) {
    let Some(payload) = parse_tool_error_response(result) else {
        return;
    };
    if !payload.suppress_retry {
        return;
    }

    let mut guard = suppressions
        .lock()
        .expect("capability suppression cache lock poisoned");
    guard.insert(tool_attempt_fingerprint(name, args_json), payload.clone());
    if let Some(extra_key) = payload.suppression_key.clone() {
        guard.insert(extra_key, payload);
    }
}

/// Process an LLM response: extract text or dispatch tool calls.
///
/// WHY parallel dispatch: Independent tools (e.g. two file reads) don't
/// need to run sequentially. Using JoinSet cuts wall-clock time for
/// multi-tool turns. Sequential tools (terminal) are still dispatched
/// one at a time via the parallel_safe() check.
///
/// ```text
///   response.tool_calls
///       │
///       ├── parallel safe? ──→ JoinSet::spawn (concurrent)
///       └── sequential?    ──→ await inline (ordered)
/// ```
async fn process_response(
    response: &edgequake_llm::LLMResponse,
    session: &mut SessionState,
    dctx: &DispatchContext<'_>,
    tool_errors: &mut Vec<edgecrab_types::ToolErrorRecord>,
) -> Result<LoopAction, AgentError> {
    // Check for tool calls
    if response.has_tool_calls() {
        let max_delegate_calls = match dctx.app_config_ref.delegation_max_subagents {
            0 => MAX_DELEGATE_TASK_CALLS_PER_TURN,
            configured => configured
                .min(MAX_DELEGATE_TASK_CALLS_PER_TURN as u32)
                .max(1) as usize,
        };
        let effective_tool_calls =
            cap_delegate_task_calls(&response.tool_calls, max_delegate_calls);

        // Convert LLM tool calls → our internal ToolCall type and store
        // on the assistant message so build_chat_messages() can reconstruct
        // the assistant_with_tools ChatMessage later.
        let our_tool_calls: Vec<edgecrab_types::ToolCall> = effective_tool_calls
            .iter()
            .map(edgecrab_types::ToolCall::from_llm)
            .collect();

        let assistant_text = response.content.clone();
        let mut assistant_msg = Message::assistant_with_tool_calls(&assistant_text, our_tool_calls);
        if let Some(ref thinking) = response.thinking_content {
            assistant_msg.reasoning = Some(thinking.clone());
        }
        session.messages.push(assistant_msg);
        session.session_tool_call_count += effective_tool_calls.len() as u32;

        // Partition tools into parallel-safe and sequential
        let mut parallel_tasks = tokio::task::JoinSet::new();
        let mut sequential_calls = Vec::new();
        // Track parallel tool call IDs/names so we can inject error results
        // for any task that panics — otherwise the assistant message has
        // tool_calls with no matching tool_results and the next API call fails.
        let mut parallel_submitted: Vec<(String, String)> = Vec::new();

        for tc in &effective_tool_calls {
            let is_parallel = dctx
                .registry
                .as_ref()
                .map(|r| r.is_parallel_safe(&tc.function.name))
                .unwrap_or(false);

            // Notify TUI that a tool execution is starting
            if let Some(tx) = dctx.event_tx {
                let _ = tx.send(crate::StreamEvent::ToolExec {
                    tool_call_id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    args_json: tc.function.arguments.clone(),
                });
            }

            if is_parallel {
                // Track the tool call so we can detect panics after join_next.
                parallel_submitted.push((tc.id.clone(), tc.function.name.clone()));
                // Spawn parallel-safe tools concurrently
                let tc_id = tc.id.clone();
                let tc_name = tc.function.name.clone();
                let tc_args = tc.function.arguments.clone();
                let reg = dctx.registry.cloned();
                let cancel_token = dctx.cancel.clone();
                let state = dctx.state_db.clone();
                let plat = dctx.platform;
                let proc_table = dctx.process_table.clone();
                let prov = dctx.provider.clone();
                let gateway_sender = dctx.gateway_sender.clone();
                let sar = dctx.sub_agent_runner.clone();
                let clarify = dctx.clarify_tx.clone();
                let approval = dctx.approval_tx.clone();
                let args_for_done = tc.function.arguments.clone();
                let origin = dctx.origin_chat.clone();
                let app_cfg_ref = dctx.app_config_ref.clone();
                let conv_sess_id = dctx.conversation_session_id.clone();
                let todo_store_clone = dctx.todo_store.clone();
                let capability_suppressions = dctx.capability_suppressions.clone();
                let dispatch_cwd = dctx.cwd.clone();

                parallel_tasks.spawn(async move {
                    let started = std::time::Instant::now();
                    let inner = DispatchContext {
                        cwd: dispatch_cwd,
                        registry: reg.as_ref(),
                        cancel: &cancel_token,
                        state_db: &state,
                        platform: plat,
                        process_table: &proc_table,
                        provider: prov,
                        gateway_sender,
                        sub_agent_runner: sar,
                        event_tx: None, // ToolExec event already sent before dispatch
                        delegation_event_tx: None,
                        clarify_tx: clarify,
                        approval_tx: approval,
                        origin_chat: origin,
                        app_config_ref: app_cfg_ref,
                        conversation_session_id: conv_sess_id,
                        todo_store: todo_store_clone,
                        capability_suppressions,
                    };
                    let result = dispatch_single_tool(&tc_id, &tc_name, &tc_args, &inner).await;
                    let duration_ms = started.elapsed().as_millis() as u64;
                    (tc_id, tc_name, args_for_done, result, duration_ms)
                });
            } else {
                sequential_calls.push(tc);
            }
        }

        // Collect parallel results
        let mut received_parallel_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        while let Some(join_result) = parallel_tasks.join_next().await {
            match join_result {
                Ok((tc_id, tc_name, args_json, tool_result, duration_ms)) => {
                    let is_error = is_tool_error(&tool_result);
                    emit_tool_done(
                        dctx.event_tx,
                        &tc_id,
                        &tc_name,
                        &args_json,
                        &tool_result,
                        duration_ms,
                        is_error,
                    );
                    if is_error {
                        remember_tool_suppression(
                            &dctx.capability_suppressions,
                            &tc_name,
                            &args_json,
                            &tool_result,
                        );
                        tool_errors.push(edgecrab_types::ToolErrorRecord {
                            turn: session.api_call_count,
                            tool_name: tc_name.clone(),
                            arguments: args_json.clone(),
                            error: extract_tool_error_text(&tool_result),
                            tool_result: tool_result.clone(),
                        });
                    }
                    received_parallel_ids.insert(tc_id.clone());
                    session
                        .messages
                        .push(Message::tool_result(&tc_id, &tc_name, &tool_result));
                }
                Err(e) => {
                    tracing::error!(error = %e, "parallel tool task panicked");
                    // The panicked task's id/name is unknown at this point —
                    // we will inject error results for all missing IDs below.
                }
            }
        }

        // Inject synthetic error results for any parallel tasks that panicked.
        // Without this, the assistant message has tool_calls with no corresponding
        // tool_results, which causes most LLM APIs to return a 400 error on the
        // next iteration.
        for (tc_id, tc_name) in &parallel_submitted {
            if !received_parallel_ids.contains(tc_id) {
                tracing::warn!(
                    tool_call_id = %tc_id,
                    tool_name = %tc_name,
                    "injecting error result for panicked parallel tool task"
                );
                session.messages.push(Message::tool_result(
                    tc_id,
                    tc_name,
                    &format!("Tool error: task panicked — internal error executing '{tc_name}'"),
                ));
            }
        }

        // Dispatch sequential tools in order
        for tc in sequential_calls {
            let started = std::time::Instant::now();
            let tool_result =
                dispatch_single_tool(&tc.id, &tc.function.name, &tc.function.arguments, dctx).await;
            let duration_ms = started.elapsed().as_millis() as u64;

            let is_error = is_tool_error(&tool_result);
            emit_tool_done(
                dctx.event_tx,
                &tc.id,
                &tc.function.name,
                &tc.function.arguments,
                &tool_result,
                duration_ms,
                is_error,
            );
            if is_error {
                remember_tool_suppression(
                    &dctx.capability_suppressions,
                    &tc.function.name,
                    &tc.function.arguments,
                    &tool_result,
                );
                tool_errors.push(edgecrab_types::ToolErrorRecord {
                    turn: session.api_call_count,
                    tool_name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                    error: extract_tool_error_text(&tool_result),
                    tool_result: tool_result.clone(),
                });
            }
            session.messages.push(Message::tool_result(
                &tc.id,
                &tc.function.name,
                &tool_result,
            ));
        }

        return Ok(LoopAction::Continue);
    }

    // Text response — we're done.
    // Extract reasoning/thinking content from the LLM response if present.
    let text = response.content.clone();
    let mut msg = Message::assistant(&text);
    if let Some(ref thinking) = response.thinking_content {
        msg.reasoning = Some(thinking.clone());
    }
    session.messages.push(msg);
    Ok(LoopAction::Done(text))
}

fn cap_delegate_task_calls(
    tool_calls: &[edgequake_llm::ToolCall],
    max_delegate_calls: usize,
) -> Vec<edgequake_llm::ToolCall> {
    let delegate_count = tool_calls
        .iter()
        .filter(|tc| tc.function.name == "delegate_task")
        .count();
    if delegate_count <= max_delegate_calls {
        return tool_calls.to_vec();
    }

    let mut kept_delegates = 0usize;
    let mut truncated = Vec::with_capacity(tool_calls.len());
    for tc in tool_calls {
        if tc.function.name == "delegate_task" {
            if kept_delegates < max_delegate_calls {
                truncated.push(tc.clone());
                kept_delegates += 1;
            }
        } else {
            truncated.push(tc.clone());
        }
    }

    tracing::warn!(
        delegate_count,
        max_delegate_calls,
        "truncated excess delegate_task tool calls in a single turn"
    );
    truncated
}

/// Dispatch a single tool call through the registry.
async fn dispatch_single_tool(
    tool_call_id: &str,
    name: &str,
    args_json: &str,
    dctx: &DispatchContext<'_>,
) -> String {
    let Some(reg) = dctx.registry else {
        return format!(
            "Tool '{}' execution is not yet wired (no ToolRegistry provided).",
            name
        );
    };

    let attempt_key = tool_attempt_fingerprint(name, args_json);
    if let Some(prior) = dctx
        .capability_suppressions
        .lock()
        .expect("capability suppression cache lock poisoned")
        .get(&attempt_key)
        .cloned()
    {
        return serde_json::to_string(&suppressed_retry_response(name, args_json, &prior))
            .expect("suppressed retry payload serializes");
    }

    // Emit tool:pre hook event — informational (fire-and-forget, no cancellation)
    if let Some(tx) = dctx.event_tx {
        let ctx_json = serde_json::json!({
            "event": "tool:pre",
            "tool_name": name,
            "args_json": args_json,
            "session_id": &dctx.conversation_session_id,
        })
        .to_string();
        let _ = tx.send(crate::StreamEvent::HookEvent {
            event: "tool:pre".to_string(),
            context_json: ctx_json,
        });
    }

    let ctx = build_tool_context(
        &dctx.cwd,
        dctx.app_config_ref.clone(),
        dctx.cancel,
        dctx.state_db,
        dctx.platform,
        dctx.process_table,
        dctx.provider.clone(),
        dctx.registry.cloned(), // Pass registry so execute_code can dispatch RPC tool calls
        dctx.sub_agent_runner.clone(),
        dctx.delegation_event_tx.clone(),
        dctx.clarify_tx.clone(),
        dctx.approval_tx.clone(),
        make_tool_progress_tx(dctx.event_tx),
        dctx.gateway_sender.clone(),
        dctx.origin_chat.clone(),
        Some(tool_call_id.to_string()),
        Some(name.to_string()),
        &dctx.conversation_session_id,
        dctx.todo_store.clone(),
    );

    let args: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => {
            // Malformed JSON arguments — report as a tool error so the LLM
            // can self-correct (e.g. fix quoting or truncated output) rather
            // than silently receiving `{}` and producing nonsensical output.
            tracing::warn!(tool_name = %name, error = %e, args_json = %args_json, "malformed tool arguments JSON");
            return ToolError::InvalidArgs {
                tool: name.to_string(),
                message: format!("invalid JSON arguments: {e}"),
            }
            .to_llm_response();
        }
    };

    let result = match reg.dispatch(name, args, &ctx).await {
        Ok(output) => output,
        Err(e) => e.to_llm_response(),
    };

    // Emit tool:post hook event
    if let Some(tx) = dctx.event_tx {
        let is_error = is_tool_error(&result);
        let ctx_json = serde_json::json!({
            "event": "tool:post",
            "tool_name": name,
            "session_id": &dctx.conversation_session_id,
            "is_error": is_error,
        })
        .to_string();
        let _ = tx.send(crate::StreamEvent::HookEvent {
            event: "tool:post".to_string(),
            context_json: ctx_json,
        });
    }

    result
}

// ─── Auto-title generation ───────────────────────────────────────────

/// Generate a short session title from the first user/assistant exchange.
///
/// WHY: Mirrors hermes-agent's `title_generator.py` / `maybe_auto_title()`.
/// A human-readable title makes `/history` and the TUI sidebar useful.
/// The title is generated from the first exchange only (cheapest/fastest path).
///
/// Runs as a fire-and-forget tokio task — no latency impact on the user.
async fn auto_title_session(
    db: Arc<edgecrab_state::SessionDb>,
    session_id: String,
    user_snippet: String,
    assistant_snippet: String,
    provider: Arc<dyn LLMProvider>,
) {
    // Check if a user-set title already exists — if so, don't overwrite it
    match db.get_session(&session_id) {
        Ok(Some(rec)) => {
            if let Some(ref existing) = rec.title {
                // A proper (non-truncated) title is already set
                if !existing.is_empty() && existing.len() < 80 && !existing.ends_with('…') {
                    tracing::debug!("session already has a title, skipping auto-title");
                    return;
                }
            }
        }
        _ => return,
    }

    let prompt = format!(
        "Generate a short, descriptive title (3-7 words) for a conversation that starts with:\n\
         User: {user_snippet}\n\nAssistant: {assistant_snippet}\n\n\
         Return ONLY the title. No quotes, no punctuation at the end, no prefixes."
    );
    let messages = vec![
        edgequake_llm::ChatMessage::system(
            "You generate ultra-short session titles. Respond with ONLY the title, nothing else.",
        ),
        edgequake_llm::ChatMessage::user(&prompt),
    ];

    match provider.chat(&messages, None).await {
        Ok(resp) => {
            let mut title = resp.content.trim().to_string();
            // Strip surrounding quotes and common prefixes
            title = title.trim_matches(|c| c == '"' || c == '\'').to_string();
            if title.to_lowercase().starts_with("title:") {
                title = title[6..].trim().to_string();
            }
            if title.len() > 80 {
                title = format!("{}…", crate::safe_truncate(&title, 77));
            }
            if !title.is_empty() {
                if let Err(e) = db.update_session_title(&session_id, &title) {
                    tracing::debug!(error = %e, "auto-title DB update failed");
                } else {
                    tracing::debug!(title, "auto-generated session title");
                }
            }
        }
        Err(e) => tracing::debug!(error = %e, "auto-title generation failed"),
    }
}

// ─── Background reflection context ───────────────────────────────────

/// Owned snapshot of everything needed for a detached learning reflection task.
///
/// WHY a dedicated struct: `DispatchContext<'a>` carries lifetime-bound references
/// that cannot be moved into a `tokio::spawn` future (`'static` bound). This struct
/// holds `Arc` handles and cloned data so the reflection can run in a detached task
/// independent of the `execute_loop` lifetime.
struct BackgroundReflectionCtx {
    messages: Vec<Message>,
    system_prompt: Option<String>,
    tool_defs: Vec<edgequake_llm::ToolDefinition>,
    cwd: std::path::PathBuf,
    registry: Option<Arc<ToolRegistry>>,
    cancel: CancellationToken,
    state_db: Option<Arc<edgecrab_state::SessionDb>>,
    platform: edgecrab_types::Platform,
    process_table: Arc<edgecrab_tools::ProcessTable>,
    provider: Arc<dyn edgequake_llm::LLMProvider>,
    gateway_sender: Option<Arc<dyn edgecrab_tools::registry::GatewaySender>>,
    sub_agent_runner: Option<Arc<dyn edgecrab_tools::SubAgentRunner>>,
    app_config_ref: AppConfigRef,
    conversation_session_id: String,
    origin_chat: Option<(String, String)>,
    todo_store: Option<Arc<edgecrab_tools::TodoStore>>,
}

/// Run learning reflection in a detached tokio task (fire-and-forget).
///
/// WHY detached: mirrors hermes-agent's `_spawn_background_review()` which
/// runs in a daemon thread and never blocks the main turn. In edgecrab the
/// original `run_learning_reflection` was awaited inline, delaying
/// `StreamEvent::Done` (and thus TUI unlock) by the full duration of an
/// extra LLM API call. Grok-4 reasoning responses can take minutes.
///
/// The reflection writes to disk via `skill_manage` / `memory_write` tool calls
/// dispatched on a cloned session snapshot. Changes to the in-memory session are NOT
/// propagated back — the learning outcome is durable (on-disk skills/memories).
async fn run_learning_reflection_bg(ctx: BackgroundReflectionCtx) {
    // Borrow from owned ctx to satisfy DispatchContext<'a> lifetime requirements.
    let registry_arc = ctx.registry.as_ref();
    let state_db_ref = &ctx.state_db;
    let process_table_ref = &ctx.process_table;

    let dctx = DispatchContext {
        cwd: ctx.cwd.clone(),
        registry: registry_arc,
        cancel: &ctx.cancel,
        state_db: state_db_ref,
        platform: ctx.platform,
        process_table: process_table_ref,
        provider: Some(Arc::clone(&ctx.provider)),
        gateway_sender: ctx.gateway_sender.clone(),
        sub_agent_runner: ctx.sub_agent_runner.clone(),
        event_tx: None, // background — no TUI event channel
        delegation_event_tx: None,
        clarify_tx: None,  // background — no interactive Q&A
        approval_tx: None, // background — no interactive approvals
        origin_chat: ctx.origin_chat.clone(),
        app_config_ref: ctx.app_config_ref.clone(),
        conversation_session_id: ctx.conversation_session_id.clone(),
        todo_store: ctx.todo_store.clone(),
        capability_suppressions: Arc::new(Mutex::new(HashMap::new())),
    };

    // Work on local session clone — we don't need results propagated back.
    let mut session = SessionState {
        messages: ctx.messages,
        cached_system_prompt: ctx.system_prompt,
        ..Default::default()
    };

    run_learning_reflection(&mut session, &ctx.tool_defs, &ctx.provider, &dctx).await;
}

// ─── Learning reflection ──────────────────────────────────────────────

/// Run the end-of-session learning reflection step.
///
/// This is the **closed learning loop** mirror of hermes-agent's reflection
/// step. After a complex session (≥ `SKILL_REFLECTION_THRESHOLD` tool calls),
/// the agent is given a targeted reflection prompt and ONE additional API call
/// where it can:
/// - Call `skill_manage(action='create', ...)` to save a reusable workflow.
/// - Call `skill_manage(action='patch', ...)` to improve an existing skill.
/// - Call `memory_write` to record important facts about the project or user.
/// - Respond with "nothing to save" and stop immediately.
///
/// WHY a separate function: Keeps `execute_loop` readable and makes the
/// reflection step easy to test in isolation.
///
/// WHY non-fatal: A reflection failure must NEVER fail the session.
/// We log at debug level and silently clean up the prompt if the call fails.
///
/// Returns the number of tool calls made during reflection (0 if skipped).
async fn run_learning_reflection(
    session: &mut SessionState,
    tool_defs: &[edgequake_llm::ToolDefinition],
    provider: &Arc<dyn LLMProvider>,
    dctx: &DispatchContext<'_>,
) {
    const REFLECTION_PROMPT: &str = "\
[system: learning checkpoint] This session used multiple tool calls. \
Please reflect briefly (1-2 sentences of thinking, not shown to the user): \
Did you discover a reusable workflow, debugging technique, or non-trivial \
pattern worth saving? If yes, call skill_manage(action='create', name='...', \
content='---\\nname: ...\\ndescription: ...\\n---\\n# Steps\\n...') to save it. \
Did you learn something important about the user, their project, or environment \
that should persist? If yes, call memory_write to record it. \
If nothing is worth saving, respond with exactly 'reflection: nothing to save' \
and stop — do NOT call any tools.";

    session.messages.push(Message::user(REFLECTION_PROMPT));

    let chat_messages = build_chat_messages(
        session.cached_system_prompt.as_deref(),
        &session.messages,
        None, // No cache control for reflection turn
    );

    let response = match provider
        .chat_with_tools(&chat_messages, tool_defs, None, None)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(error = %e, "learning reflection API call failed (non-fatal)");
            // Remove the reflection prompt so it doesn't confuse the user's
            // next turn if they continue the session.
            session.messages.pop();
            return;
        }
    };

    // Dispatch any tool calls the agent made (skill_manage, memory_write, etc.)
    // Use process_response — it appends messages and runs tools properly.
    // Reflection tool errors are non-fatal and not surfaced to the caller.
    let mut _reflection_tool_errors: Vec<edgecrab_types::ToolErrorRecord> = Vec::new();
    if let Err(e) = process_response(&response, session, dctx, &mut _reflection_tool_errors).await {
        tracing::debug!(error = %e, "learning reflection tool dispatch failed (non-fatal)");
    }
}

/// Remove orphaned tool_result messages that have no matching assistant
/// tool_call in the history. Orphans can appear after /undo or /compress
/// operations that remove the assistant message but leave the tool result.
///
/// WHY: Sending orphaned tool results to the API causes errors on most
/// providers (OpenAI returns 400, Anthropic ignores them).
fn sanitize_orphaned_tool_results(messages: &mut Vec<Message>) {
    use std::collections::HashSet;

    // Collect all tool_call IDs from assistant messages
    let mut valid_ids: HashSet<String> = HashSet::new();
    for msg in messages.iter() {
        if msg.role == Role::Assistant {
            if let Some(ref calls) = msg.tool_calls {
                for tc in calls {
                    valid_ids.insert(tc.id.clone());
                }
            }
        }
    }

    // Remove tool-result messages whose tool_call_id is not in the valid set
    let before = messages.len();
    messages.retain(|msg| {
        if msg.role == Role::Tool {
            msg.tool_call_id
                .as_ref()
                .is_some_and(|id| valid_ids.contains(id))
        } else {
            true
        }
    });
    let removed = before - messages.len();
    if removed > 0 {
        tracing::info!(removed, "sanitized orphaned tool result messages");
    }
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentBuilder;
    use async_trait::async_trait;
    use edgecrab_tools::{ProcessTable, ToolRegistry};
    use edgequake_llm::traits::StreamUsage;
    use edgequake_llm::{ChatMessage, CompletionOptions, FunctionCall, ToolChoice, ToolDefinition};
    use serde_json::json;
    use tempfile::TempDir;

    #[derive(Clone)]
    struct StreamingUsageProvider {
        chunks: Vec<StreamChunk>,
    }

    #[derive(Clone)]
    struct OrphanRejectingProvider;

    #[derive(Clone)]
    struct RetryCountingProvider {
        provider_name: &'static str,
        attempts: Arc<std::sync::atomic::AtomicUsize>,
        last_options: Arc<Mutex<Option<CompletionOptions>>>,
    }
    struct FlakyToolStreamProvider {
        attempts: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait]
    impl LLMProvider for StreamingUsageProvider {
        fn name(&self) -> &str {
            "streaming-usage-test"
        }

        fn model(&self) -> &str {
            "streaming-usage-test-model"
        }

        fn max_context_length(&self) -> usize {
            8192
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
            messages: &[ChatMessage],
            options: Option<&CompletionOptions>,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            self.chat_with_tools(messages, &[], None, options).await
        }

        async fn chat_with_tools(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
            _tool_choice: Option<ToolChoice>,
            _options: Option<&CompletionOptions>,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            Ok(edgequake_llm::LLMResponse::new("non-stream", self.model()))
        }

        async fn chat_with_tools_stream(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
            _tool_choice: Option<ToolChoice>,
            _options: Option<&CompletionOptions>,
        ) -> edgequake_llm::Result<
            futures::stream::BoxStream<'static, edgequake_llm::Result<StreamChunk>>,
        > {
            use futures::StreamExt;

            Ok(futures::stream::iter(self.chunks.clone().into_iter().map(Ok)).boxed())
        }

        fn supports_tool_streaming(&self) -> bool {
            true
        }
    }

    #[async_trait]
    impl LLMProvider for OrphanRejectingProvider {
        fn name(&self) -> &str {
            "orphan-rejecting-test"
        }

        fn model(&self) -> &str {
            "orphan-rejecting-test-model"
        }

        fn max_context_length(&self) -> usize {
            8192
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
            messages: &[ChatMessage],
            _options: Option<&CompletionOptions>,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            self.assert_no_orphaned_tool_result(messages)?;
            Ok(edgequake_llm::LLMResponse::new(
                "clean history",
                self.model(),
            ))
        }
    }

    impl OrphanRejectingProvider {
        fn assert_no_orphaned_tool_result(
            &self,
            messages: &[ChatMessage],
        ) -> edgequake_llm::Result<()> {
            let mut valid_tool_ids = std::collections::HashSet::new();
            for message in messages {
                if matches!(message.role, edgequake_llm::ChatRole::Assistant) {
                    if let Some(tool_calls) = &message.tool_calls {
                        for tool_call in tool_calls {
                            valid_tool_ids.insert(tool_call.id.clone());
                        }
                    }
                }
                let tool_call_id = message.tool_call_id.as_deref();
                if matches!(message.role, edgequake_llm::ChatRole::Tool)
                    && tool_call_id.is_none_or(|id| !valid_tool_ids.contains(id))
                {
                    return Err(edgequake_llm::LlmError::ApiError(
                        "orphaned tool result reached provider".into(),
                    ));
                }
            }
            Ok(())
        }
    }

    #[async_trait]
    impl LLMProvider for RetryCountingProvider {
        fn name(&self) -> &str {
            self.provider_name
        }

        fn model(&self) -> &str {
            "retry-counting-model"
        }

        fn max_context_length(&self) -> usize {
            8192
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
            _messages: &[ChatMessage],
            options: Option<&CompletionOptions>,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            self.attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            *self.last_options.lock().expect("lock") = options.cloned();
            Err(edgequake_llm::LlmError::NetworkError(
                "synthetic network failure".into(),
            ))
        }
    }

    #[async_trait]
    impl LLMProvider for FlakyToolStreamProvider {
        fn name(&self) -> &str {
            "flaky-tool-stream"
        }

        fn model(&self) -> &str {
            "flaky-tool-stream-model"
        }

        fn max_context_length(&self) -> usize {
            8192
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
            messages: &[ChatMessage],
            options: Option<&CompletionOptions>,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            self.chat_with_tools(messages, &[], None, options).await
        }

        async fn chat_with_tools(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
            _tool_choice: Option<ToolChoice>,
            _options: Option<&CompletionOptions>,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            Ok(edgequake_llm::LLMResponse::new("non-stream", self.model()))
        }

        async fn chat_with_tools_stream(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
            _tool_choice: Option<ToolChoice>,
            _options: Option<&CompletionOptions>,
        ) -> edgequake_llm::Result<
            futures::stream::BoxStream<'static, edgequake_llm::Result<StreamChunk>>,
        > {
            use futures::StreamExt;

            let attempt = self
                .attempts
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let chunks = if attempt == 0 {
                vec![
                    StreamChunk::ToolCallDelta {
                        index: 0,
                        id: Some("call_write".into()),
                        function_name: Some("write_file".into()),
                        function_arguments: None,
                        thought_signature: None,
                    },
                    StreamChunk::Finished {
                        reason: "stop".into(),
                        ttft_ms: None,
                        usage: None,
                    },
                ]
            } else {
                vec![
                    StreamChunk::Content("recovered".into()),
                    StreamChunk::Finished {
                        reason: "stop".into(),
                        ttft_ms: None,
                        usage: None,
                    },
                ]
            };

            Ok(futures::stream::iter(chunks.into_iter().map(Ok)).boxed())
        }

        fn supports_tool_streaming(&self) -> bool {
            true
        }
    }

    #[tokio::test]
    async fn api_call_streaming_preserves_authoritative_usage() {
        let provider: Arc<dyn LLMProvider> = Arc::new(StreamingUsageProvider {
            chunks: vec![
                StreamChunk::Content("streamed answer".to_string()),
                StreamChunk::Finished {
                    reason: "stop".to_string(),
                    ttft_ms: None,
                    usage: Some(
                        StreamUsage::new(11, 7)
                            .with_cache_hit_tokens(2)
                            .with_thinking_tokens(5),
                    ),
                },
            ],
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let tokens_sent = std::sync::atomic::AtomicBool::new(false);

        let response = api_call_streaming(
            &provider,
            &[ChatMessage::user("hello")],
            &[],
            Some(&CompletionOptions {
                max_tokens: Some(256),
                ..Default::default()
            }),
            &tx,
            &tokens_sent,
        )
        .await
        .expect("stream call");

        assert_eq!(response.prompt_tokens, 11);
        assert_eq!(response.completion_tokens, 7);
        assert_eq!(response.total_tokens, 18);
        assert_eq!(response.cache_hit_tokens, Some(2));
        assert_eq!(response.thinking_tokens, Some(5));
        assert_eq!(response.finish_reason.as_deref(), Some("stop"));
    }

    #[tokio::test]
    async fn api_call_streaming_estimates_usage_when_provider_omits_it() {
        let provider: Arc<dyn LLMProvider> = Arc::new(StreamingUsageProvider {
            chunks: vec![
                StreamChunk::ThinkingContent {
                    text: "reasoning trace".to_string(),
                    tokens_used: Some(3),
                    budget_total: None,
                },
                StreamChunk::Content("streamed answer".to_string()),
                StreamChunk::Finished {
                    reason: "stop".to_string(),
                    ttft_ms: None,
                    usage: None,
                },
            ],
        });
        let tool_defs = vec![ToolDefinition::function(
            "echo",
            "Echo input",
            json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"}
                },
                "required": ["text"]
            }),
        )];
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let tokens_sent = std::sync::atomic::AtomicBool::new(false);

        let response = api_call_streaming(
            &provider,
            &[
                ChatMessage::system("system"),
                ChatMessage::user("hello world"),
            ],
            &tool_defs,
            Some(&CompletionOptions {
                max_tokens: Some(512),
                ..Default::default()
            }),
            &tx,
            &tokens_sent,
        )
        .await
        .expect("stream call");

        assert!(
            response.prompt_tokens > 0,
            "prompt tokens should be estimated"
        );
        assert!(
            response.completion_tokens > 0,
            "completion tokens should be estimated"
        );
        assert_eq!(response.thinking_tokens, Some(3));
        assert_eq!(response.finish_reason.as_deref(), Some("stop"));
    }

    #[tokio::test]
    async fn api_call_streaming_rejects_tool_calls_without_arguments() {
        let provider: Arc<dyn LLMProvider> = Arc::new(StreamingUsageProvider {
            chunks: vec![
                StreamChunk::ToolCallDelta {
                    index: 0,
                    id: Some("call_execute".into()),
                    function_name: Some("execute_code".into()),
                    function_arguments: None,
                    thought_signature: None,
                },
                StreamChunk::Finished {
                    reason: "stop".to_string(),
                    ttft_ms: None,
                    usage: None,
                },
            ],
        });
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let tokens_sent = std::sync::atomic::AtomicBool::new(false);

        let err = api_call_streaming(
            &provider,
            &[ChatMessage::user("hello")],
            &[],
            None,
            &tx,
            &tokens_sent,
        )
        .await
        .expect_err("missing streamed arguments must be rejected");

        assert!(
            err.to_string().contains("finished without arguments"),
            "unexpected error: {err}"
        );
        assert!(
            !tokens_sent.load(std::sync::atomic::Ordering::Relaxed),
            "tool-call deltas alone must not count as visible streamed output"
        );
    }

    #[tokio::test]
    async fn api_call_with_retry_does_not_double_retry_copilot_requests() {
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let provider: Arc<dyn LLMProvider> = Arc::new(RetryCountingProvider {
            provider_name: "vscode-copilot",
            attempts: attempts.clone(),
            last_options: Arc::new(Mutex::new(None)),
        });
        let cancel = CancellationToken::new();

        let err = api_call_with_retry(
            &provider,
            &[ChatMessage::user("hello")],
            &[],
            3,
            ApiCallContext {
                options: None,
                cancel: &cancel,
                event_tx: None,
                use_native_streaming: false,
            },
        )
        .await
        .expect_err("copilot request should fail");

        assert!(matches!(err, AgentError::Llm(_)));
        assert_eq!(
            attempts.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "Copilot already retries internally; the outer loop must not multiply attempts"
        );
    }

    #[tokio::test]
    async fn api_call_with_retry_forwards_completion_options() {
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let last_options = Arc::new(Mutex::new(None));
        let provider: Arc<dyn LLMProvider> = Arc::new(RetryCountingProvider {
            provider_name: "options-test-provider",
            attempts,
            last_options: last_options.clone(),
        });
        let cancel = CancellationToken::new();
        let options = CompletionOptions {
            max_tokens: Some(2048),
            temperature: Some(0.1),
            reasoning_effort: Some("low".into()),
            ..Default::default()
        };

        let _ = api_call_with_retry(
            &provider,
            &[ChatMessage::user("hello")],
            &[],
            0,
            ApiCallContext {
                options: Some(&options),
                cancel: &cancel,
                event_tx: None,
                use_native_streaming: false,
            },
        )
        .await;

        let recorded = last_options.lock().expect("lock").clone().expect("options");
        assert_eq!(recorded.max_tokens, Some(2048));
        assert_eq!(recorded.temperature, Some(0.1));
        assert_eq!(recorded.reasoning_effort.as_deref(), Some("low"));
    }

    #[tokio::test]
    async fn api_call_with_retry_retries_malformed_streamed_tool_calls() {
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let provider: Arc<dyn LLMProvider> = Arc::new(FlakyToolStreamProvider {
            attempts: attempts.clone(),
        });
        let cancel = CancellationToken::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

        let response = api_call_with_retry(
            &provider,
            &[ChatMessage::user("hello")],
            &[],
            1,
            ApiCallContext {
                options: None,
                cancel: &cancel,
                event_tx: Some(&tx),
                use_native_streaming: true,
            },
        )
        .await
        .expect("malformed tool stream should be retried once");

        assert_eq!(response.content, "recovered");
        assert_eq!(response.finish_reason.as_deref(), Some("stop"));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn completion_options_include_model_budget_and_reasoning_policy() {
        let mut config = crate::agent::AgentConfig::default();
        config.temperature = Some(0.2);
        config.reasoning_effort = Some("medium".into());
        config.model_config.max_tokens = Some(3072);

        let options = completion_options_for(&config);

        assert_eq!(options.max_tokens, Some(3072));
        assert_eq!(options.temperature, Some(0.2));
        assert_eq!(options.reasoning_effort.as_deref(), Some("medium"));
    }

    #[test]
    fn native_streaming_policy_disables_copilot_for_tool_turns() {
        let copilot_provider: Arc<dyn LLMProvider> = Arc::new(RetryCountingProvider {
            provider_name: "vscode-copilot",
            attempts: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            last_options: Arc::new(Mutex::new(None)),
        });
        let streaming_provider: Arc<dyn LLMProvider> = Arc::new(FlakyToolStreamProvider {
            attempts: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        });
        let tool_defs = vec![ToolDefinition::function(
            "write_file",
            "Write a file",
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                },
                "required": ["path", "content"]
            }),
        )];

        assert!(
            !should_use_native_streaming(copilot_provider.as_ref(), &tool_defs, true, true),
            "Copilot tool turns should use the safer non-native path"
        );
        assert!(
            should_use_native_streaming(streaming_provider.as_ref(), &[], true, true),
            "Plain-text turns can still use native streaming"
        );
    }

    #[test]
    fn cap_delegate_task_calls_truncates_excess_and_preserves_other_calls() {
        let delegate = |id: &str| edgequake_llm::ToolCall {
            id: id.into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "delegate_task".into(),
                arguments: "{}".into(),
            },
            thought_signature: None,
        };
        let terminal = edgequake_llm::ToolCall {
            id: "tool-terminal".into(),
            call_type: "function".into(),
            function: FunctionCall {
                name: "terminal".into(),
                arguments: r#"{"command":"pwd"}"#.into(),
            },
            thought_signature: None,
        };

        let tool_calls = vec![
            delegate("delegate-1"),
            terminal.clone(),
            delegate("delegate-2"),
            delegate("delegate-3"),
            delegate("delegate-4"),
        ];

        let capped = cap_delegate_task_calls(&tool_calls, 3);
        assert_eq!(capped.len(), 4);
        assert_eq!(capped[0].id, "delegate-1");
        assert_eq!(capped[1].id, "tool-terminal");
        assert_eq!(capped[2].id, "delegate-2");
        assert_eq!(capped[3].id, "delegate-3");
    }

    #[tokio::test]
    async fn execute_loop_basic() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .build()
            .expect("build");

        let result = agent
            .execute_loop("hello", Some("Be helpful."), None, None, None)
            .await
            .expect("loop");

        assert!(!result.final_response.is_empty());
        assert_eq!(result.api_calls, 1);
        assert!(!result.interrupted);
    }

    #[tokio::test]
    async fn execute_loop_with_history() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .build()
            .expect("build");

        let history = vec![
            Message::user("previous question"),
            Message::assistant("previous answer"),
        ];

        let result = agent
            .execute_loop("follow-up", None, Some(history), None, None)
            .await
            .expect("loop");

        // History (2) + user (1) + assistant (1) = 4
        assert_eq!(result.messages.len(), 4);
    }

    #[tokio::test]
    async fn execute_loop_sanitizes_history_before_provider_call() {
        let provider: Arc<dyn LLMProvider> = Arc::new(OrphanRejectingProvider);
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .build()
            .expect("build");
        let history = vec![
            Message::user("previous question"),
            Message::tool_result("orphan-id", "read_file", "stale output"),
        ];

        let result = agent
            .execute_loop("follow-up", None, Some(history), None, None)
            .await
            .expect("loop");

        assert_eq!(result.final_response, "clean history");
        assert!(
            result
                .messages
                .iter()
                .all(|message| message.tool_call_id.as_deref() != Some("orphan-id")),
            "orphaned tool result should be removed before persistence"
        );
    }

    #[tokio::test]
    async fn execute_loop_uses_cwd_override_for_context_discovery() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .build()
            .expect("build");
        let workspace = TempDir::new().expect("workspace");
        std::fs::write(
            workspace.path().join("AGENTS.md"),
            "# Workspace Rules\n\nUse the override workspace.",
        )
        .expect("write AGENTS.md");

        agent
            .execute_loop("hello", None, None, None, Some(workspace.path()))
            .await
            .expect("loop");

        let session = agent.session.read().await;
        let prompt = session
            .cached_system_prompt
            .as_deref()
            .expect("cached system prompt");
        assert!(prompt.contains("Use the override workspace."));
    }

    #[test]
    fn build_trajectory_normalizes_reasoning_and_collects_tools() {
        let messages = vec![
            Message::user("hello"),
            Message::assistant("<REASONING_SCRATCHPAD>plan</REASONING_SCRATCHPAD>done"),
            Message::tool_result("call_1", "read_file", "contents"),
            Message::tool_result("call_2", "read_file", "more contents"),
        ];

        let trajectory =
            build_trajectory("session-1", "provider/model", &messages, 2, 0.25, true, 1.5);

        assert_eq!(trajectory.session_id, "session-1");
        assert_eq!(trajectory.metadata.api_calls, 2);
        assert_eq!(
            trajectory.metadata.tools_used,
            vec!["read_file".to_string()]
        );
        assert!(
            trajectory.messages[1]
                .text_content()
                .contains("<think>plan</think>")
        );
    }

    #[tokio::test]
    async fn execute_loop_resets_preexisting_interrupt() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .build()
            .expect("build");

        // Interrupt before the loop starts.
        // execute_loop resets the cancel token so the NEXT turn can still run
        // (this is the intentional fix for the "Ctrl+C breaks all future turns"
        // bug).  A pre-loop interrupt should therefore NOT result in interrupted=true;
        // that was the broken behaviour that treated the cancel token as permanent.
        agent.interrupt();

        let result = agent
            .execute_loop("hello", None, None, None, None)
            .await
            .expect("loop");

        // With the reset fix the loop runs normally and is NOT interrupted.
        assert!(
            !result.interrupted,
            "pre-loop interrupt must be reset, not permanently sticky"
        );
        // Confirm a real response was produced
        assert!(!result.final_response.is_empty());
        // Token is no longer cancelled after a clean (non-interrupted) turn
        assert!(!agent.is_cancelled());
    }

    #[tokio::test]
    async fn execute_loop_budget_exhaust() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .max_iterations(1)
            .build()
            .expect("build");

        let result = agent
            .execute_loop("hello", None, None, None, None)
            .await
            .expect("loop");

        // With budget=1, should complete one iteration normally
        assert_eq!(result.api_calls, 1);
    }

    #[test]
    fn build_chat_messages_prepends_system() {
        let messages = vec![Message::user("hi")];
        let chat_msgs = build_chat_messages(Some("system prompt"), &messages, None);
        assert_eq!(chat_msgs.len(), 2);
    }

    #[test]
    fn build_chat_messages_no_system() {
        let messages = vec![Message::user("hi")];
        let chat_msgs = build_chat_messages(None, &messages, None);
        assert_eq!(chat_msgs.len(), 1);
    }

    #[test]
    fn build_chat_messages_with_cache_config() {
        let messages = vec![Message::user(
            "a long user message that is at least one thousand chars. "
                .repeat(20)
                .as_str(),
        )];
        let cfg = CachePromptConfig::default();
        let chat_msgs = build_chat_messages(Some("system prompt"), &messages, Some(&cfg));
        // System + user = 2 messages; cache breakpoints should be set
        assert_eq!(chat_msgs.len(), 2);
        // System message should have cache_control set
        assert!(chat_msgs[0].cache_control.is_some());
    }

    #[test]
    fn prompt_cache_config_is_provider_aware() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        assert!(
            prompt_cache_config_for(&provider, true).is_none(),
            "non-Anthropic providers should not receive Anthropic cache markers"
        );
        assert!(provider_supports_prompt_caching("anthropic"));
    }

    #[test]
    fn estimate_request_prompt_tokens_includes_fixed_prompt_mass() {
        let messages = vec![Message::user("hi")];
        let tool_defs = vec![edgequake_llm::ToolDefinition::function(
            "terminal",
            "Run shell commands.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string"}
                }
            }),
        )];

        let bare = estimate_request_prompt_tokens(None, &messages, &[]);
        let inflated = estimate_request_prompt_tokens(Some("system prompt"), &messages, &tool_defs);
        assert!(
            inflated > bare,
            "system prompt + tool schemas must increase request pressure"
        );
    }

    #[test]
    fn available_toolsets_for_prompt_deduplicates_registry_matches() {
        let registry = edgecrab_tools::registry::ToolRegistry::new();
        let toolsets = available_toolsets_for_prompt(
            &registry,
            &[
                "read_file".to_string(),
                "write_file".to_string(),
                "read_file".to_string(),
            ],
        );
        assert_eq!(toolsets, vec!["file".to_string()]);
    }

    #[test]
    fn sanitize_removes_orphaned_tool_results() {
        let mut messages = vec![
            Message::user("hi"),
            // Tool result with no matching assistant tool_call – orphaned
            Message::tool_result("orphan-id", "read_file", "file content"),
            Message::assistant("hello"),
        ];
        sanitize_orphaned_tool_results(&mut messages);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[1].role, Role::Assistant);
    }

    #[test]
    fn sanitize_keeps_valid_tool_results() {
        let tc = edgecrab_types::ToolCall {
            id: "valid-id".into(),
            r#type: "function".into(),
            function: edgecrab_types::FunctionCall {
                name: "read_file".into(),
                arguments: "{}".into(),
            },
            thought_signature: None,
        };
        let mut messages = vec![
            Message::user("hi"),
            Message::assistant_with_tool_calls("calling tool", vec![tc]),
            Message::tool_result("valid-id", "read_file", "file content"),
        ];
        sanitize_orphaned_tool_results(&mut messages);
        assert_eq!(messages.len(), 3, "valid tool result should be kept");
    }

    // ── Budget exhaustion edge cases ──────────────────────────────────────

    /// When the iteration budget is zero (max_iterations=0), the budget gate
    /// fires before ANY API call. The agent must return a non-empty synthetic
    /// fallback — NOT an empty string — and set budget_exhausted=true.
    #[tokio::test]
    async fn budget_exhaustion_at_gate_returns_synthetic_response() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        // max_iterations=0 → remaining=0 → try_consume() fails immediately
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .max_iterations(0)
            .build()
            .expect("build");

        let result = agent
            .execute_loop("do something", Some("Be helpful."), None, None, None)
            .await
            .expect("loop should not error on budget exhaustion");

        // Must have a non-empty response (synthetic fallback)
        assert!(
            !result.final_response.is_empty(),
            "budget-exhausted agent must not return empty response"
        );
        // The synthetic message should mention the limit
        assert!(
            result.final_response.contains("iteration limit"),
            "synthetic response should mention 'iteration limit'; got: '{}'",
            result.final_response
        );
        // budget_exhausted flag must be set
        assert!(
            result.budget_exhausted,
            "budget_exhausted must be true when loop exits via budget gate"
        );
        // interrupted must be false (user didn't cancel)
        assert!(!result.interrupted, "interrupted must be false");
        // Budget of 0 means no API calls were made
        assert_eq!(
            result.api_calls, 0,
            "no API calls should occur with budget=0"
        );
    }

    /// budget_exhausted flag should be in ConversationResult and chat() should return
    /// non-empty even when max_iterations forces immediate exhaustion.
    #[tokio::test]
    async fn chat_never_returns_empty_on_budget_exhaustion() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());

        // max_iterations=0 → budget gate fires immediately, no API calls
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .max_iterations(0)
            .build()
            .expect("build");

        let response = agent
            .chat("do a lot of things")
            .await
            .expect("chat should not error");
        assert!(
            !response.is_empty(),
            "chat() must not return empty string on budget exhaustion"
        );
    }

    /// Pure text conversation: agent replies with text on the first turn.
    /// budget_exhausted must be false, response non-empty.
    #[tokio::test]
    async fn normal_completion_resets_budget_exhausted_flag() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .max_iterations(10)
            .build()
            .expect("build");

        let result = agent
            .execute_loop("hello", Some("Be helpful."), None, None, None)
            .await
            .expect("loop");

        assert!(!result.final_response.is_empty());
        assert!(
            !result.budget_exhausted,
            "normal completion must not set budget_exhausted"
        );
        assert!(!result.interrupted);
    }

    /// With max_iterations=1, the first (and only) API call succeeds with a
    /// text response. After that, the budget is consumed and the loop exits
    /// normally. budget_exhausted must be false (a response WAS produced).
    #[tokio::test]
    async fn budget_exactly_one_produces_response() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .max_iterations(1)
            .build()
            .expect("build");

        let result = agent
            .execute_loop("hello", None, None, None, None)
            .await
            .expect("loop");

        // MockProvider returns text → LoopAction::Done → final_response non-empty
        assert!(!result.final_response.is_empty());
        assert!(
            !result.budget_exhausted,
            "text response was produced, not exhausted"
        );
        assert_eq!(result.api_calls, 1);
    }

    /// Structural test: ConversationResult.budget_exhausted reflects that the
    /// budget gate fired when max_iterations=0.
    #[tokio::test]
    async fn budget_exhausted_exactly_on_tool_turn_boundary() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());

        // budget=0 → gate fires before first API call → budget_exhausted=true
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .max_iterations(0)
            .build()
            .expect("build");

        let result = agent
            .execute_loop("run", None, None, None, None)
            .await
            .expect("loop");

        assert!(result.budget_exhausted, "budget_exhausted must be true");
        assert!(
            !result.final_response.is_empty(),
            "synthetic response must not be empty"
        );
        assert_eq!(result.api_calls, 0, "no API calls before budget gate");
    }

    /// Verify multi-turn text dialog works with sufficient budget.
    /// MockProvider always returns a text response, so after 1 turn the conversation
    /// completes normally with api_calls=1.
    #[tokio::test]
    async fn multi_turn_tool_chain_completes_with_sufficient_budget() {
        // MockProvider (basic) always returns "Mock response" as text.
        // With budget ≥ 1, the first API call -> text response -> Done.
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .max_iterations(10)
            .build()
            .expect("build");

        let result = agent
            .execute_loop("do something and respond", None, None, None, None)
            .await
            .expect("loop");

        assert!(
            !result.final_response.is_empty(),
            "response must be non-empty"
        );
        assert!(
            !result.budget_exhausted,
            "should complete normally without budget exhaustion"
        );
        assert!(!result.interrupted);
        // MockProvider returns text on every call → single iteration
        assert_eq!(result.api_calls, 1, "one API call for a text-only response");
    }

    // ── Budget warning injection ──────────────────────────────────────────

    #[test]
    fn inject_budget_warning_appends_to_tool_message_json() {
        let mut messages = vec![
            Message::user("task"),
            Message::tool_result("id1", "read_file", r#"{"output": "some content"}"#),
        ];
        inject_budget_warning(&mut messages, "[URGENT: wrap up]");

        let last = messages.last().expect("budget warning target exists");
        let text = last.text_content();
        assert!(
            text.contains("_budget_warning"),
            "budget warning should be injected into JSON tool message; got: {text}"
        );
        assert!(
            text.contains("wrap up"),
            "warning text should be present; got: {text}"
        );
    }

    #[test]
    fn inject_budget_warning_appends_to_tool_message_plain() {
        let mut messages = vec![
            Message::user("task"),
            Message::tool_result("id1", "read_file", "plain text output"),
        ];
        inject_budget_warning(&mut messages, "[URGENT: wrap up]");

        let last = messages.last().expect("budget warning target exists");
        let text = last.text_content();
        assert!(
            text.contains("wrap up"),
            "plain-text warning should be appended; got: {text}"
        );
    }

    #[test]
    fn inject_budget_warning_falls_back_to_user_message_when_no_tools() {
        // Pure text conversation — no Tool messages in history.
        let mut messages = vec![
            Message::user("hello"),
            Message::assistant("how can I help?"),
        ];
        let before = messages.len();
        inject_budget_warning(&mut messages, "[BUDGET: 70%]");

        assert_eq!(
            messages.len(),
            before + 1,
            "should inject a new user message as fallback"
        );
        let last = messages.last().expect("fallback warning message exists");
        assert_eq!(last.role, Role::User);
        assert!(last.text_content().contains("70%"));
    }

    // ── Budget warning thresholds ─────────────────────────────────────────

    #[test]
    fn get_budget_warning_none_below_70_percent() {
        assert!(
            get_budget_warning(6, 10).is_none(),
            "60% should produce no warning"
        );
    }

    #[test]
    fn get_budget_warning_at_70_percent() {
        let w = get_budget_warning(7, 10);
        assert!(w.is_some(), "70% should produce BUDGET warning");
        assert!(
            w.expect("70% warning should exist").contains("BUDGET"),
            "should say BUDGET"
        );
    }

    #[test]
    fn get_budget_warning_at_90_percent() {
        let w = get_budget_warning(9, 10);
        assert!(w.is_some(), "90% should produce URGENT warning");
        assert!(
            w.expect("90% warning should exist").contains("URGENT"),
            "should say URGENT"
        );
    }

    #[test]
    fn get_budget_warning_zero_max_iterations() {
        assert!(
            get_budget_warning(5, 0).is_none(),
            "zero max_iterations should produce no warning (avoid div-by-zero)"
        );
    }

    // ── Sanitize edge cases ───────────────────────────────────────────────

    #[test]
    fn sanitize_handles_empty_messages() {
        let mut messages: Vec<Message> = Vec::new();
        // Should not panic on empty input
        sanitize_orphaned_tool_results(&mut messages);
        assert_eq!(messages.len(), 0);
    }

    #[test]
    fn sanitize_removes_multiple_orphans() {
        let mut messages = vec![
            Message::user("hi"),
            Message::tool_result("orphan-1", "read_file", "content-a"),
            Message::tool_result("orphan-2", "write_file", "content-b"),
            Message::assistant("done"),
        ];
        sanitize_orphaned_tool_results(&mut messages);
        assert_eq!(messages.len(), 2, "both orphans should be removed");
    }

    #[test]
    fn sanitize_handles_tool_result_without_tool_call_id() {
        // A tool_result with tool_call_id = None should be removed (can't correlate).
        let mut msg = Message::tool_result("some-id", "read_file", "data");
        msg.tool_call_id = None; // Forcibly clear the ID
        let mut messages = vec![Message::user("hi"), msg, Message::assistant("done")];
        sanitize_orphaned_tool_results(&mut messages);
        // The None-id result has no entry in valid_ids, so it is removed.
        assert_eq!(messages.len(), 2, "None-id tool result should be removed");
    }

    #[test]
    fn summarize_tool_result_preview_prefers_terminal_body() {
        let preview = summarize_tool_result_preview(
            "terminal",
            "[terminal_result status=success backend=local cwd=/tmp exit_code=0]\nhello world\n",
            false,
        )
        .expect("preview");
        assert_eq!(preview, "hello world");
    }

    #[test]
    fn summarize_tool_result_preview_extracts_error_text() {
        let preview = summarize_tool_result_preview(
            "terminal",
            "Tool error: permission denied while executing command",
            true,
        )
        .expect("preview");
        assert!(preview.contains("permission denied"));
    }

    // ── build_chat_messages edge cases ───────────────────────────────────

    #[test]
    fn build_chat_messages_tool_role_uses_tool_call_id() {
        let tc = edgecrab_types::ToolCall {
            id: "tc-abc".into(),
            r#type: "function".into(),
            function: edgecrab_types::FunctionCall {
                name: "read_file".into(),
                arguments: "{}".into(),
            },
            thought_signature: None,
        };
        let messages = vec![
            Message::user("read something"),
            Message::assistant_with_tool_calls("sure", vec![tc]),
            Message::tool_result("tc-abc", "read_file", "contents"),
        ];
        let chat_msgs = build_chat_messages(None, &messages, None);
        // user + assistant_with_tools + tool_result = 3 messages
        assert_eq!(chat_msgs.len(), 3);
    }

    #[test]
    fn build_chat_messages_empty_input() {
        let chat_msgs = build_chat_messages(None, &[], None);
        assert_eq!(
            chat_msgs.len(),
            0,
            "empty messages with no system → 0 chat messages"
        );
    }

    fn make_dispatch_context_for_test<'a>(
        registry: &'a Arc<ToolRegistry>,
        cancel: &'a CancellationToken,
        state_db: &'a Option<Arc<edgecrab_state::SessionDb>>,
        process_table: &'a Arc<ProcessTable>,
        capability_suppressions: Arc<Mutex<HashMap<String, ToolErrorResponse>>>,
    ) -> DispatchContext<'a> {
        DispatchContext {
            cwd: std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            registry: Some(registry),
            cancel,
            state_db,
            platform: edgecrab_types::Platform::Cli,
            process_table,
            provider: None,
            gateway_sender: None,
            sub_agent_runner: None,
            event_tx: None,
            delegation_event_tx: None,
            clarify_tx: None,
            approval_tx: None,
            origin_chat: None,
            app_config_ref: AppConfigRef::default(),
            conversation_session_id: "test-conversation".into(),
            todo_store: None,
            capability_suppressions,
        }
    }

    #[tokio::test]
    async fn dispatch_single_tool_uses_dispatch_context_cwd() {
        let registry = Arc::new(ToolRegistry::new());
        let cancel = CancellationToken::new();
        let state_db = None;
        let process_table = Arc::new(ProcessTable::new());
        let capability_suppressions = Arc::new(Mutex::new(HashMap::new()));
        let mut dctx = make_dispatch_context_for_test(
            &registry,
            &cancel,
            &state_db,
            &process_table,
            capability_suppressions,
        );

        let workspace = TempDir::new().expect("workspace");
        std::fs::write(workspace.path().join("proof.txt"), "dispatch cwd works").expect("write");
        dctx.cwd = workspace.path().to_path_buf();

        let result = dispatch_single_tool(
            "call-read-file",
            "read_file",
            r#"{"path":"proof.txt","line_numbers":false}"#,
            &dctx,
        )
        .await;

        assert!(result.contains("dispatch cwd works"), "got: {result}");
    }

    #[tokio::test]
    async fn dispatch_single_tool_returns_structured_json_error() {
        let registry = Arc::new(ToolRegistry::new());
        let cancel = CancellationToken::new();
        let state_db = None;
        let process_table = Arc::new(ProcessTable::new());
        let capability_suppressions = Arc::new(Mutex::new(HashMap::new()));
        let dctx = make_dispatch_context_for_test(
            &registry,
            &cancel,
            &state_db,
            &process_table,
            capability_suppressions,
        );

        let result = dispatch_single_tool("call-read-file", "read_file", "{}", &dctx).await;
        let parsed = parse_tool_error_response(&result).expect("structured tool error");
        assert_eq!(parsed.response_type, "tool_error");
        assert_eq!(parsed.category, "arguments");
        assert_eq!(parsed.code, "invalid_arguments");
        assert_eq!(parsed.tool.as_deref(), Some("read_file"));
    }

    #[tokio::test]
    async fn dispatch_single_tool_suppresses_repeated_capability_retry() {
        let registry = Arc::new(ToolRegistry::new());
        let cancel = CancellationToken::new();
        let state_db = None;
        let process_table = Arc::new(ProcessTable::new());
        let capability_suppressions = Arc::new(Mutex::new(HashMap::new()));
        let dctx = make_dispatch_context_for_test(
            &registry,
            &cancel,
            &state_db,
            &process_table,
            capability_suppressions.clone(),
        );
        let args_json = r#"{"command":"top"}"#;

        let first = dispatch_single_tool("call-terminal-1", "terminal", args_json, &dctx).await;
        let first_payload = parse_tool_error_response(&first).expect("structured error");
        assert_eq!(first_payload.code, "non_interactive_terminal_required");
        remember_tool_suppression(&capability_suppressions, "terminal", args_json, &first);

        let second = dispatch_single_tool("call-terminal-2", "terminal", args_json, &dctx).await;
        let second_payload = parse_tool_error_response(&second).expect("structured error");
        assert_eq!(second_payload.code, "suppressed_capability_retry");
        assert!(second_payload.error.contains("already blocked"));
    }

    // ── Cancellation ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn cancellation_sets_interrupted_not_budget_exhausted() {
        // Cancel a long-running agent mid-loop and verify the flags are correct.
        // With MockProvider (always returns text), the first iteration completes
        // before we can cancel — so we verify the agent recovers cleanly.
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .max_iterations(100)
            .build()
            .expect("build");

        // A single text iteration completes normally; no budget exhausted, no interrupt.
        let result = agent
            .execute_loop("hello", None, None, None, None)
            .await
            .expect("loop");

        // Normal completion: no budget exhaustion, no interrupt
        assert!(
            !result.budget_exhausted,
            "normal completion must not set budget_exhausted"
        );
        assert!(
            !result.interrupted,
            "normal completion must not set interrupted"
        );
        assert!(!result.final_response.is_empty());
    }
}
