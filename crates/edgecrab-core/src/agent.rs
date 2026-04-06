//! # Agent — core entry point for conversation execution
//!
//! WHY a builder: The Agent needs ~10 dependencies injected (provider,
//! tools, state DB, callbacks, config). A builder prevents 10-argument
//! constructors and makes optional dependencies explicit.
//!
//! ```text
//!   AgentBuilder::new("model")
//!       .provider(provider)
//!       .tools(registry)
//!       .state_db(db)
//!       .build()? ──→ Agent
//!                      │
//!                      ├── .chat("hi")        → simple interface
//!                      └── .run_conversation() → full interface
//! ```

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use edgecrab_state::SessionDb;
use edgecrab_tools::ProcessTable;
use edgecrab_tools::TodoStore;
use edgecrab_tools::registry::{GatewaySender, ToolRegistry};
use edgecrab_types::{AgentError, ApiMode, Cost, Message, Platform, Role, Usage};
use edgequake_llm::LLMProvider;

use crate::config::AppConfig;

// ─── Agent ────────────────────────────────────────────────────────────

pub struct Agent {
    /// WHY RwLock on config: Model hot-swap (/model command) updates the
    /// model name at runtime. RwLock allows concurrent reads during the
    /// conversation loop while permitting rare writes on model switch.
    pub(crate) config: RwLock<AgentConfig>,
    /// WHY RwLock on provider: The /model command swaps the LLM provider
    /// at runtime. The conversation loop clones the Arc at loop start,
    /// so in-flight conversations aren't affected by a swap.
    pub(crate) provider: RwLock<Arc<dyn LLMProvider>>,
    #[allow(dead_code)] // Used in Phase 1.6 conversation loop
    pub(crate) state_db: Option<Arc<SessionDb>>,
    pub(crate) tool_registry: Option<Arc<ToolRegistry>>,
    /// Gateway-backed outbound sender for `send_message`.
    ///
    /// None in plain CLI / cron sessions. Set by the messaging gateway runtime
    /// so tool execution can deliver to external platforms without duplicating
    /// transport logic inside the agent loop.
    pub(crate) gateway_sender: RwLock<Option<Arc<dyn GatewaySender>>>,
    /// Shared process table for background-process management tools.
    /// WHY on Agent: All tool invocations in the same session share the
    /// same process namespace — Agent lifetime == session lifetime.
    pub(crate) process_table: Arc<ProcessTable>,
    pub(crate) session: RwLock<SessionState>,
    pub(crate) budget: Arc<IterationBudget>,
    /// Cancel token is wrapped in a Mutex so it can be RESET before each new
    /// conversation turn. CancellationToken is a one-way latch — once cancelled
    /// it cannot be un-cancelled. By replacing it with a fresh token at the
    /// start of execute_loop we ensure Ctrl+C only stops the current turn, not
    /// all future turns.
    pub(crate) cancel: std::sync::Mutex<CancellationToken>,
    /// Dedicated cancel token for the process-table GC task.
    ///
    /// WHY separate from `cancel`: `cancel` is reset on every new conversation
    /// turn (see execute_loop) so it can't drive a long-lived background task.
    /// `gc_cancel` lives for the full Agent lifetime and is cancelled via
    /// `Drop` so the GC stops when the Agent is dropped.  Mirrors the cleanup
    /// semantics of hermes-agent's `FINISHED_TTL_SECONDS` periodic cleanup.
    pub(crate) gc_cancel: CancellationToken,
    /// Per-session task list — survives context compression.
    ///
    /// WHY on Agent: The Agent lifetime == session lifetime. Placing the store
    /// here mirrors hermes-agent's `self._todo_store` on the `AIAgent` class.
    /// After each compression `format_for_injection()` re-injects active items
    /// so the model never loses its plan across context-window boundaries.
    pub(crate) todo_store: Arc<edgecrab_tools::TodoStore>,
}

/// Options for cloning an agent into a fresh isolated session.
#[derive(Debug, Clone, Default)]
pub struct IsolatedAgentOptions {
    /// Optional fixed session identifier for the child session.
    pub session_id: Option<String>,
    /// Optional platform override for the child session.
    pub platform: Option<Platform>,
    /// Optional quiet-mode override.
    pub quiet_mode: Option<bool>,
    /// Optional origin chat override for gateway-created isolated sessions.
    pub origin_chat: Option<(String, String)>,
}

/// Immutable per-agent configuration (subset of AppConfig relevant to the loop).
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub model: String,
    pub max_iterations: u32,
    pub enabled_toolsets: Vec<String>,
    pub disabled_toolsets: Vec<String>,
    pub streaming: bool,
    pub temperature: Option<f32>,
    pub platform: Platform,
    pub api_mode: ApiMode,
    pub session_id: Option<String>,
    pub quiet_mode: bool,
    pub save_trajectories: bool,
    pub skip_context_files: bool,
    pub skip_memory: bool,
    pub reasoning_effort: Option<String>,
    /// Optional persona/personality instruction appended to the system prompt.
    /// Resolved from `config.display.personality` via `resolve_personality()`.
    pub personality_addon: Option<String>,
    /// Model config for routing (base_url, api_key_env, smart routing).
    pub model_config: crate::config::ModelConfig,
    /// Skills config — disabled skills, platform-specific disabled.
    pub skills_config: crate::config::SkillsConfig,
    /// Delegation runtime controls mirrored from AppConfig.delegation.
    pub delegation_enabled: bool,
    pub delegation_model: Option<String>,
    pub delegation_provider: Option<String>,
    pub delegation_max_subagents: u32,
    pub delegation_max_iterations: u32,
    /// Origin of the current session — (platform_name, chat_id).
    ///
    /// Set by the gateway when a message arrives from a real chat
    /// (Telegram, WhatsApp, Discord, etc.).  Passed into every `ToolContext`
    /// so that `manage_cron_jobs(action='create', deliver='origin')` can
    /// record the correct delivery target without the LLM needing to know it.
    /// None in CLI / cron / test sessions.
    pub origin_chat: Option<(String, String)>,
    /// Browser automation config (recording, timeouts).
    pub browser: crate::config::BrowserConfig,
    /// Whether automatic checkpoints are enabled.
    pub checkpoints_enabled: bool,
    /// Maximum checkpoints to retain per working directory.
    pub checkpoints_max_snapshots: u32,
    /// Active terminal backend and backend-specific configuration.
    pub terminal_backend: edgecrab_tools::tools::backends::BackendKind,
    pub terminal_docker: edgecrab_tools::tools::backends::DockerBackendConfig,
    pub terminal_ssh: edgecrab_tools::tools::backends::SshBackendConfig,
    pub terminal_modal: edgecrab_tools::tools::backends::ModalBackendConfig,
    pub terminal_daytona: edgecrab_tools::tools::backends::DaytonaBackendConfig,
    pub terminal_singularity: edgecrab_tools::tools::backends::SingularityBackendConfig,
    /// Context-compression policy copied from AppConfig at session start.
    pub compression: crate::config::CompressionConfig,
    /// Auxiliary side-task routing (vision, compression, other helper calls).
    pub auxiliary: crate::config::AuxiliaryConfig,
    /// Env-var names allowed to pass through the subprocess security blocklist.
    ///
    /// Populated from `terminal.env_passthrough` in config.yaml and applied
    /// to the global registry when the Agent is built.  Skills that declare
    /// `required_environment_variables` also feed the registry at load time.
    pub terminal_env_passthrough: Vec<String>,
    /// Additional file roots trusted by file tools beyond the active workspace.
    pub file_allowed_roots: Vec<std::path::PathBuf>,
    /// Denied prefixes layered over the workspace and allow-root policy.
    pub path_restrictions: Vec<std::path::PathBuf>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            model: "anthropic/claude-opus-4.6".into(),
            max_iterations: 90,
            enabled_toolsets: Vec::new(),
            disabled_toolsets: Vec::new(),
            streaming: true,
            temperature: None,
            platform: Platform::Cli,
            api_mode: ApiMode::ChatCompletions,
            session_id: None,
            quiet_mode: false,
            save_trajectories: false,
            skip_context_files: false,
            skip_memory: false,
            reasoning_effort: None,
            personality_addon: None,
            model_config: crate::config::ModelConfig::default(),
            skills_config: crate::config::SkillsConfig::default(),
            delegation_enabled: true,
            delegation_model: None,
            delegation_provider: None,
            delegation_max_subagents: 3,
            delegation_max_iterations: 50,
            origin_chat: None,
            browser: crate::config::BrowserConfig::default(),
            checkpoints_enabled: true,
            checkpoints_max_snapshots: 50,
            terminal_backend: edgecrab_tools::tools::backends::BackendKind::Local,
            terminal_docker: edgecrab_tools::tools::backends::DockerBackendConfig::default(),
            terminal_ssh: edgecrab_tools::tools::backends::SshBackendConfig::default(),
            terminal_modal: edgecrab_tools::tools::backends::ModalBackendConfig::default(),
            terminal_daytona: edgecrab_tools::tools::backends::DaytonaBackendConfig::default(),
            terminal_singularity:
                edgecrab_tools::tools::backends::SingularityBackendConfig::default(),
            compression: crate::config::CompressionConfig::default(),
            auxiliary: crate::config::AuxiliaryConfig::default(),
            terminal_env_passthrough: Vec::new(),
            file_allowed_roots: Vec::new(),
            path_restrictions: Vec::new(),
        }
    }
}

/// Per-session mutable state, protected by RwLock.
#[derive(Default)]
pub struct SessionState {
    /// Unique session identifier — set once at conversation start,
    /// persisted to SQLite at loop end for session search/history.
    pub session_id: Option<String>,
    pub messages: Vec<Message>,
    pub cached_system_prompt: Option<String>,
    pub user_turn_count: u32,
    pub api_call_count: u32,
    pub session_input_tokens: u64,
    pub session_output_tokens: u64,
    pub session_cache_read_tokens: u64,
    pub session_cache_write_tokens: u64,
    pub session_reasoning_tokens: u64,
    /// Prompt-side tokens from the most recent model call.
    ///
    /// This tracks current context pressure. Session token counters above track
    /// cumulative spend across the whole conversation.
    pub last_prompt_tokens: u64,
    pub session_tool_call_count: u32,
}

/// Lock-free iteration budget — prevents runaway tool loops.
///
/// WHY AtomicU32: The budget is checked on every loop iteration and
/// decremented atomically. No mutex contention on the hot path.
pub struct IterationBudget {
    remaining: AtomicU32,
    max: u32,
}

impl IterationBudget {
    pub fn new(max: u32) -> Self {
        Self {
            remaining: AtomicU32::new(max),
            max,
        }
    }

    /// Try to consume one iteration. Returns false when exhausted.
    pub fn try_consume(&self) -> bool {
        // CAS loop: only decrement if remaining > 0.
        loop {
            let current = self.remaining.load(Ordering::Relaxed);
            if current == 0 {
                return false;
            }
            if self
                .remaining
                .compare_exchange_weak(current, current - 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return true;
            }
        }
    }

    pub fn remaining(&self) -> u32 {
        self.remaining.load(Ordering::Relaxed)
    }

    pub fn max(&self) -> u32 {
        self.max
    }

    pub fn used(&self) -> u32 {
        self.max.saturating_sub(self.remaining())
    }

    pub fn reset(&self) {
        self.remaining.store(self.max, Ordering::Relaxed);
    }
}

/// Result of a full conversation run.
#[derive(Debug, Clone)]
pub struct ConversationResult {
    pub final_response: String,
    pub messages: Vec<Message>,
    pub session_id: String,
    pub api_calls: u32,
    pub interrupted: bool,
    /// True when the iteration budget was exhausted before the LLM produced
    /// a final text response. Distinct from `interrupted` (user Ctrl+C).
    pub budget_exhausted: bool,
    pub model: String,
    pub usage: Usage,
    pub cost: Cost,
    /// Per-tool-call error records accumulated across the entire conversation.
    ///
    /// Mirrors hermes-agent's `AgentResult.tool_errors: List[ToolError]`.
    /// Each entry captures the turn number, tool name, arguments, and the
    /// error text — enabling structured observability and RL training without
    /// requiring callers to parse raw message history.
    pub tool_errors: Vec<edgecrab_types::ToolErrorRecord>,
}

// ─── Simple API ───────────────────────────────────────────────────────

impl Agent {
    fn build_runtime_clone(
        config: AgentConfig,
        provider: Arc<dyn LLMProvider>,
        state_db: Option<Arc<SessionDb>>,
        tool_registry: Option<Arc<ToolRegistry>>,
    ) -> Self {
        let budget = Arc::new(IterationBudget::new(config.max_iterations));

        let gc_cancel = CancellationToken::new();
        let process_table = Arc::new(ProcessTable::new());
        process_table.spawn_gc_task(gc_cancel.clone());

        Self {
            config: RwLock::new(config),
            provider: RwLock::new(provider),
            state_db,
            tool_registry,
            gateway_sender: RwLock::new(None),
            process_table,
            session: RwLock::new(SessionState::default()),
            budget,
            cancel: std::sync::Mutex::new(CancellationToken::new()),
            gc_cancel,
            todo_store: Arc::new(TodoStore::new()),
        }
    }

    /// Simple interface — send a message, get a response string.
    pub async fn chat(&self, message: &str) -> Result<String, AgentError> {
        let result = self.run_conversation(message, None, None).await?;
        Ok(result.final_response)
    }

    /// Session-aware interface — run a turn relative to the provided workspace.
    pub async fn chat_in_cwd(&self, message: &str, cwd: &Path) -> Result<String, AgentError> {
        let result = self
            .run_conversation_in_cwd(message, None, None, cwd)
            .await?;
        Ok(result.final_response)
    }

    /// Inject or replace the gateway-backed outbound sender used by `send_message`.
    pub async fn set_gateway_sender(&self, sender: Arc<dyn GatewaySender>) {
        *self.gateway_sender.write().await = Some(sender);
    }

    /// Gateway interface — send a message with origin context (platform + chat_id).
    ///
    /// Unlike `chat()`, this sets the origin so that `manage_cron_jobs` jobs
    /// created in this session will have deliver='origin' route back to the
    /// correct chat automatically.  Also updates `config.platform` so the
    /// system prompt includes the correct platform hints (WhatsApp, Telegram, etc.).
    pub async fn chat_with_origin(
        &self,
        message: &str,
        platform: &str,
        chat_id: &str,
    ) -> Result<String, AgentError> {
        // Set origin_chat and platform for this conversation turn.
        {
            let mut cfg = self.config.write().await;
            cfg.origin_chat = Some((platform.to_string(), chat_id.to_string()));
            cfg.platform = platform_from_str(platform).unwrap_or(cfg.platform);
        }
        let result = self.run_conversation(message, None, None).await?;
        {
            // Clear origin after the turn so it isn't stale for the next message.
            let mut cfg = self.config.write().await;
            cfg.origin_chat = None;
        }
        Ok(result.final_response)
    }

    /// Streaming gateway interface — sets origin context, then streams events.
    ///
    /// Combines the origin-context setup of `chat_with_origin()` with the
    /// progressive streaming of `chat_streaming()`.  This is the method the
    /// gateway dispatch loop should call so that:
    /// 1. `manage_cron_jobs` jobs route back to the originating chat.
    /// 2. The system prompt includes the correct platform hints.
    /// 3. Streamed `Token`, `Reasoning`, `ToolExec`, … events reach the caller.
    pub async fn chat_streaming_with_origin(
        &self,
        message: &str,
        platform: &str,
        chat_id: &str,
        chunk_tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<(), AgentError> {
        // Set origin and platform — identical to chat_with_origin().
        {
            let mut cfg = self.config.write().await;
            cfg.origin_chat = Some((platform.to_string(), chat_id.to_string()));
            cfg.platform = platform_from_str(platform).unwrap_or(cfg.platform);
        }

        let result = self.chat_streaming(message, chunk_tx).await;

        // Clear origin after the turn regardless of success/failure.
        {
            let mut cfg = self.config.write().await;
            cfg.origin_chat = None;
        }

        result
    }

    /// Streaming interface — sends tokens to `chunk_tx` as they arrive.
    ///
    /// WHY delegate to execute_loop: We want the full ReAct loop (tools,
    /// memory, prompt builder, retries) to remain the single source of truth.
    /// When the provider supports native tool streaming, `execute_loop()` now
    /// emits live token/reasoning events directly. Otherwise we fall back to
    /// streaming the final response in chunks after the synchronous turn ends.
    pub async fn chat_streaming(
        &self,
        message: &str,
        chunk_tx: tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    ) -> Result<(), AgentError> {
        let (streaming_enabled, use_native_streaming) = {
            let config = self.config.read().await;
            let provider = self.provider.read().await;
            let streaming_enabled = config.streaming;
            let use_native_streaming = streaming_enabled && provider.supports_tool_streaming();
            (streaming_enabled, use_native_streaming)
        };

        match self
            .execute_loop(message, None, None, Some(&chunk_tx), None)
            .await
        {
            Ok(result) => {
                // Fallback path: provider doesn't expose live deltas through
                // the full tool loop, so synthesize only what the user asked for:
                // - streaming ON  → chunk the final answer for progressive UX
                // - streaming OFF → send one complete answer at the end
                if !use_native_streaming {
                    if let Some(reasoning) = result
                        .messages
                        .iter()
                        .rev()
                        .find(|msg| msg.role == Role::Assistant)
                        .and_then(|msg| msg.reasoning.clone())
                        .filter(|reasoning| !reasoning.trim().is_empty())
                    {
                        let _ = chunk_tx.send(StreamEvent::Reasoning(reasoning));
                    }

                    if streaming_enabled {
                        for chunk in result.final_response.as_bytes().chunks(50) {
                            let text = String::from_utf8_lossy(chunk).into_owned();
                            let _ = chunk_tx.send(StreamEvent::Token(text));
                        }
                    } else if !result.final_response.is_empty() {
                        let _ = chunk_tx.send(StreamEvent::Token(result.final_response.clone()));
                    }
                }
                let _ = chunk_tx.send(StreamEvent::Done);
                Ok(())
            }
            Err(e) => {
                let _ = chunk_tx.send(StreamEvent::Error(e.to_string()));
                Err(e)
            }
        }
    }

    /// Full conversation interface.
    ///
    /// Delegates to `execute_loop()` (conversation.rs) which implements
    /// the full agent loop with retry, tool dispatch, and cancellation.
    pub async fn run_conversation(
        &self,
        user_message: &str,
        system_message: Option<&str>,
        conversation_history: Option<Vec<Message>>,
    ) -> Result<ConversationResult, AgentError> {
        self.execute_loop(
            user_message,
            system_message,
            conversation_history,
            None,
            None,
        )
        .await
    }

    /// Full conversation interface with an explicit workspace root.
    pub async fn run_conversation_in_cwd(
        &self,
        user_message: &str,
        system_message: Option<&str>,
        conversation_history: Option<Vec<Message>>,
        cwd: &Path,
    ) -> Result<ConversationResult, AgentError> {
        self.execute_loop(
            user_message,
            system_message,
            conversation_history,
            None,
            Some(cwd),
        )
        .await
    }

    /// Clone the agent runtime into a fresh isolated session.
    ///
    /// WHY this exists: `/background` and similar workflows need the current
    /// model, provider, tool configuration, and state DB wiring, but must not
    /// share conversation history, process tables, or cancellation state with
    /// the foreground session.
    pub async fn fork_isolated(&self, options: IsolatedAgentOptions) -> Result<Self, AgentError> {
        let mut config = self.config.read().await.clone();
        let provider = self.provider.read().await.clone();
        let gateway_sender = self.gateway_sender.read().await.clone();

        if let Some(session_id) = options.session_id {
            config.session_id = Some(session_id);
        } else {
            config.session_id = None;
        }
        if let Some(platform) = options.platform {
            config.platform = platform;
        }
        if let Some(quiet_mode) = options.quiet_mode {
            config.quiet_mode = quiet_mode;
        }
        config.origin_chat = options.origin_chat;

        let child = Self::build_runtime_clone(
            config,
            provider,
            self.state_db.clone(),
            self.tool_registry.clone(),
        );
        if let Some(sender) = gateway_sender {
            child.set_gateway_sender(sender).await;
        }
        Ok(child)
    }

    /// Signal the agent to stop at the next iteration boundary.
    pub fn interrupt(&self) {
        self.cancel
            .lock()
            .expect("cancel mutex not poisoned")
            .cancel();
    }

    /// Whether the cancellation token has been triggered.
    pub fn is_cancelled(&self) -> bool {
        self.cancel
            .lock()
            .expect("cancel mutex not poisoned")
            .is_cancelled()
    }

    /// Reset session state for a new conversation.
    pub async fn new_session(&self) {
        // Clear the per-session env passthrough registry so stale skill
        // registrations from the previous session don't leak.  Re-register
        // config-level passthrough entries immediately so they remain
        // available for the new session's very first PersistentShell spawn.
        let passthrough = {
            let cfg = self.config.read().await;
            cfg.terminal_env_passthrough.clone()
        };
        edgecrab_tools::tools::backends::local::clear_env_passthrough();
        if !passthrough.is_empty() {
            edgecrab_tools::tools::backends::local::register_env_passthrough(&passthrough);
        }

        let mut session = self.session.write().await;
        *session = SessionState::default();
        self.budget.reset();
    }

    /// Hot-swap the LLM model and provider at runtime.
    ///
    /// WHY: The `/model` command needs to switch providers without
    /// restarting the CLI. In-flight conversations are not affected
    /// because `execute_loop()` clones the provider Arc at loop start.
    pub async fn swap_model(&self, model: String, provider: Arc<dyn LLMProvider>) {
        {
            let mut cfg = self.config.write().await;
            cfg.model = model;
        }
        {
            let mut prov = self.provider.write().await;
            *prov = provider;
        }
    }

    /// Get the current model name.
    pub async fn model(&self) -> String {
        self.config.read().await.model.clone()
    }

    /// Snapshot of live session stats for `/status`, `/cost`, `/history` commands.
    pub async fn session_snapshot(&self) -> SessionSnapshot {
        let session = self.session.read().await;
        let config = self.config.read().await;
        SessionSnapshot {
            session_id: session.session_id.clone(),
            model: config.model.clone(),
            message_count: session.messages.len(),
            user_turn_count: session.user_turn_count,
            api_call_count: session.api_call_count,
            input_tokens: session.session_input_tokens,
            output_tokens: session.session_output_tokens,
            cache_read_tokens: session.session_cache_read_tokens,
            cache_write_tokens: session.session_cache_write_tokens,
            reasoning_tokens: session.session_reasoning_tokens,
            last_prompt_tokens: session.last_prompt_tokens,
            budget_remaining: self.budget.remaining(),
            budget_max: self.budget.max(),
        }
    }

    /// Get the currently assembled system prompt (if cached).
    pub async fn system_prompt(&self) -> Option<String> {
        self.session.read().await.cached_system_prompt.clone()
    }

    /// Append a note to the cached system prompt.
    ///
    /// Used to inject runtime context (e.g. "browser is now connected to live
    /// Chrome") without consuming a full user→model conversation turn.
    /// The note is appended once; callers should guard for idempotency.
    /// If the system prompt hasn't been built yet it will be set as the full
    /// prompt at first-turn assembly time and this note will be ignored — the
    /// note is silently discarded rather than force-building the prompt early.
    pub async fn append_to_system_prompt(&self, note: &str) {
        let mut session = self.session.write().await;
        if let Some(ref mut prompt) = session.cached_system_prompt {
            prompt.push_str("\n\n");
            prompt.push_str(note);
        }
        // If the system prompt isn't cached yet (no messages sent) we store the
        // note in a pending field so it can be appended at build time.
        // For simplicity we skip that path — callers send /browser connect after
        // the first message, so the prompt is already cached.
    }

    pub async fn invalidate_system_prompt(&self) {
        let mut session = self.session.write().await;
        session.cached_system_prompt = None;
    }

    pub async fn set_personality_addon(&self, addon: Option<String>) {
        {
            let mut config = self.config.write().await;
            config.personality_addon = addon;
        }
        self.invalidate_system_prompt().await;
    }

    /// Inject a synthetic assistant message directly into the conversation history.
    ///
    /// Used after runtime context changes (e.g. `/browser connect`) to make the
    /// model "remember" that its capabilities changed, overriding any prior turns
    /// where it claimed not to have those capabilities.  The injected message is
    /// NOT sent to the LLM — it appears in the history context that the LLM reads
    /// on the NEXT real user turn.
    pub async fn inject_assistant_context(&self, text: &str) {
        let mut session = self.session.write().await;
        session.messages.push(Message::assistant(text));
    }

    /// Get full message history for export.
    pub async fn messages(&self) -> Vec<Message> {
        self.session.read().await.messages.clone()
    }

    /// Remove the last user + assistant turn from history (undo).
    /// Returns the number of messages removed (0 if history is empty).
    pub async fn undo_last_turn(&self) -> usize {
        let mut session = self.session.write().await;
        let mut removed = 0;
        // Walk backwards: remove assistant/tool messages, then the user message.
        while let Some(m) = session.messages.last() {
            if m.role == Role::User {
                session.messages.pop();
                removed += 1;
                break;
            }
            session.messages.pop();
            removed += 1;
        }
        removed
    }

    /// Force context compression on the next turn.
    pub async fn force_compress(&self) {
        let provider = self.provider.read().await.clone();
        let config = self.config.read().await.clone();
        let mut session = self.session.write().await;
        let params = crate::compression::CompressionParams::from_model_config(
            &config.model,
            &config.compression,
        );
        session.messages =
            crate::compression::compress_with_llm(&session.messages, &params, &provider).await;
    }

    /// Set the session title (persisted on next DB write).
    pub async fn set_session_title(&self, title: String) {
        let session = self.session.read().await;
        if let (Some(db), Some(sid)) = (&self.state_db, &session.session_id) {
            let _ = db.update_session_title(sid, &title);
        }
    }

    /// Restore a persisted session from the state DB into the live session state.
    pub async fn restore_session(&self, id: &str) -> Result<usize, AgentError> {
        let db = self
            .state_db
            .as_ref()
            .ok_or_else(|| AgentError::Config("No state database configured".into()))?;

        let record = db
            .get_session(id)?
            .ok_or_else(|| AgentError::Config(format!("Session not found: {id}")))?;
        let messages = db.get_messages(id)?;

        {
            let mut session = self.session.write().await;
            session.session_id = Some(record.id.clone());
            session.cached_system_prompt = record.system_prompt.clone();
            session.user_turn_count = messages
                .iter()
                .filter(|m| matches!(m.role, edgecrab_types::Role::User))
                .count() as u32;
            session.api_call_count = 0;
            session.session_input_tokens = record.input_tokens.max(0) as u64;
            session.session_output_tokens = record.output_tokens.max(0) as u64;
            session.session_cache_read_tokens = record.cache_read_tokens.max(0) as u64;
            session.session_cache_write_tokens = record.cache_write_tokens.max(0) as u64;
            session.session_reasoning_tokens = record.reasoning_tokens.max(0) as u64;
            session.last_prompt_tokens = 0;
            session.messages = messages;
        }

        if let Some(model) = record.model {
            let mut config = self.config.write().await;
            config.model = model;
        }
        self.budget.reset();

        Ok(self.session.read().await.messages.len())
    }

    /// List persisted sessions (delegates to SessionDb).
    pub fn list_sessions(
        &self,
        limit: usize,
    ) -> Result<Vec<edgecrab_state::SessionSummary>, AgentError> {
        match &self.state_db {
            Some(db) => db.list_sessions(limit),
            None => Ok(Vec::new()),
        }
    }

    /// Delete a persisted session by ID (delegates to SessionDb).
    pub fn delete_session(&self, id: &str) -> Result<(), AgentError> {
        match &self.state_db {
            Some(db) => db.delete_session(id),
            None => Err(AgentError::Config("No state database configured".into())),
        }
    }

    /// Rename a persisted session (set or change its title).
    pub fn rename_session(&self, id: &str, title: &str) -> Result<(), AgentError> {
        match &self.state_db {
            Some(db) => db.update_session_title(id, title),
            None => Err(AgentError::Config("No state database configured".into())),
        }
    }

    /// Prune old ended sessions. Returns the number of sessions deleted.
    pub fn prune_sessions(
        &self,
        older_than_days: u32,
        source: Option<&str>,
    ) -> Result<usize, AgentError> {
        match &self.state_db {
            Some(db) => db.prune_sessions(older_than_days, source),
            None => Err(AgentError::Config("No state database configured".into())),
        }
    }

    /// Check if a state database is configured.
    pub fn has_state_db(&self) -> bool {
        self.state_db.is_some()
    }

    /// Return a clone of the state DB handle, if configured.
    pub async fn state_db(&self) -> Option<Arc<SessionDb>> {
        self.state_db.clone()
    }

    /// Return a clone of the current provider handle.
    ///
    /// Used by the gateway for deterministic pre-processing steps such as
    /// eager image analysis before the conversation turn starts.
    pub async fn provider_handle(&self) -> Arc<dyn LLMProvider> {
        self.provider.read().await.clone()
    }

    /// Return a clone of the current auxiliary side-task routing config.
    pub async fn auxiliary_config(&self) -> crate::config::AuxiliaryConfig {
        self.config.read().await.auxiliary.clone()
    }

    /// List all registered tool names.
    pub async fn tool_names(&self) -> Vec<String> {
        match &self.tool_registry {
            Some(reg) => reg
                .tool_names()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
            None => Vec::new(),
        }
    }

    /// List toolsets with their tool counts.
    pub async fn toolset_summary(&self) -> Vec<(String, usize)> {
        match &self.tool_registry {
            Some(reg) => reg.toolset_summary(),
            None => Vec::new(),
        }
    }

    /// Set reasoning effort on the agent config.
    pub async fn set_reasoning_effort(&self, level: Option<String>) {
        let mut config = self.config.write().await;
        config.reasoning_effort = level;
    }

    /// Enable or disable live token streaming for future turns.
    pub async fn set_streaming(&self, enabled: bool) {
        let mut config = self.config.write().await;
        config.streaming = enabled;
        config.model_config.streaming = enabled;
    }

    /// Update auxiliary side-task routing for future turns.
    pub async fn set_auxiliary_config(&self, auxiliary: crate::config::AuxiliaryConfig) {
        let mut config = self.config.write().await;
        config.auxiliary = auxiliary;
    }
}

/// Read-only snapshot of current session state for display.
#[derive(Debug, Clone)]
pub struct SessionSnapshot {
    pub session_id: Option<String>,
    pub model: String,
    pub message_count: usize,
    pub user_turn_count: u32,
    pub api_call_count: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
    pub last_prompt_tokens: u64,
    pub budget_remaining: u32,
    pub budget_max: u32,
}

impl SessionSnapshot {
    pub fn prompt_tokens(&self) -> u64 {
        self.input_tokens + self.cache_read_tokens + self.cache_write_tokens
    }

    pub fn total_tokens(&self) -> u64 {
        self.prompt_tokens() + self.output_tokens + self.reasoning_tokens
    }

    pub fn context_pressure_tokens(&self) -> u64 {
        if self.last_prompt_tokens > 0 {
            self.last_prompt_tokens
        } else {
            self.prompt_tokens()
        }
    }
}

/// Risk-graduated approval choices surfaced by `StreamEvent::Approval`.
///
/// - `Once`    — approve this specific invocation only.
/// - `Session` — approve all identical commands for the rest of the session.
/// - `Always`  — persist approval to disk so future sessions skip the dialog.
/// - `Deny`    — refuse; the agent should not execute the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalChoice {
    Once,
    Session,
    Always,
    Deny,
}

/// Events sent from the streaming agent to the TUI.
///
/// WHY an enum: The channel carries multiple event types — partial
/// tokens, completion signal, and errors. A single `String` channel
/// can't distinguish done from error, forcing consumers to use
/// sentinel strings (fragile). An enum is explicit and exhaustive.
pub enum StreamEvent {
    /// A partial response token/chunk.
    Token(String),
    /// A reasoning / think-mode chunk.
    Reasoning(String),
    /// A tool execution has started.
    ToolExec {
        /// Tool name (e.g. "web_search")
        name: String,
        /// Raw JSON arguments string (for preview extraction in the TUI)
        args_json: String,
    },
    /// A tool execution has completed.
    ToolDone {
        /// Tool name (e.g. "web_search")
        name: String,
        /// Raw JSON arguments string (for preview extraction in the TUI)
        args_json: String,
        /// Short machine-generated summary of the tool outcome.
        result_preview: Option<String>,
        /// Elapsed milliseconds
        duration_ms: u64,
        /// Whether the result looks like an error
        is_error: bool,
    },
    /// A delegated child agent has started.
    SubAgentStart {
        task_index: usize,
        task_count: usize,
        goal: String,
    },
    /// A delegated child agent surfaced intermediate reasoning text.
    SubAgentReasoning {
        task_index: usize,
        task_count: usize,
        text: String,
    },
    /// A delegated child agent called a tool.
    SubAgentToolExec {
        task_index: usize,
        task_count: usize,
        name: String,
        args_json: String,
    },
    /// A delegated child agent has finished.
    SubAgentFinish {
        task_index: usize,
        task_count: usize,
        status: String,
        duration_ms: u64,
        summary: String,
        api_calls: u32,
        model: Option<String>,
    },
    /// The response is complete.
    Done,
    /// An error occurred — the response is incomplete.
    Error(String),
    /// The agent needs a clarifying answer from the user.
    /// The caller must send the answer to `response_tx` to unblock the agent.
    Clarify {
        question: String,
        /// Up to 4 predefined answer choices, or None for open-ended.
        choices: Option<Vec<String>>,
        response_tx: tokio::sync::oneshot::Sender<String>,
    },
    /// The agent is requesting approval before executing a potentially risky command.
    ///
    /// The caller presents a risk-graduated dialog (once / session / always / deny)
    /// and sends the user's `ApprovalChoice` to `response_tx` to unblock the agent.
    /// When `deny` is chosen the agent should abort the tool execution.
    Approval {
        /// Short human-readable description of the action to be approved.
        command: String,
        /// Full command string (may be >70 chars; "view" expands this in the TUI).
        full_command: String,
        /// Concrete policy reasons that caused the approval gate to trigger.
        reasons: Vec<String>,
        /// Channel to send the user's choice back to the agent.
        response_tx: tokio::sync::oneshot::Sender<ApprovalChoice>,
    },
    /// The agent is requesting a secret string from the user (e.g. an API key,
    /// environment variable value, or sudo password).
    ///
    /// The TUI should render a masked input overlay (`•••`) so the value never
    /// appears in the scrollback. It then sends the secret string to
    /// `response_tx` to unblock the agent. Sending an empty string aborts.
    SecretRequest {
        /// The name of the variable or credential being requested (e.g. "OPENAI_API_KEY").
        var_name: String,
        /// Human-readable prompt to show the user.
        prompt: String,
        /// Whether this is a sudo / privilege-escalation prompt (affects the UI colour).
        is_sudo: bool,
        /// Channel to send the secret value back to the agent (empty = abort).
        response_tx: tokio::sync::oneshot::Sender<String>,
    },
    /// A lifecycle hook event emitted from the conversation loop.
    ///
    /// Subscribers (gateway, CLI) receive these events and forward them to
    /// the `HookRegistry` so tool:pre/post and llm:pre/post hooks fire without
    /// creating a circular dependency from edgecrab-core into edgecrab-gateway.
    HookEvent {
        /// The event type (e.g. "tool:pre", "llm:post").
        event: String,
        /// JSON-serialized context payload.
        context_json: String,
    },
    /// Context pressure warning: token usage is approaching the compression threshold.
    ///
    /// Emitted when estimated tokens exceed 85 % of the compression threshold —
    /// before compression fires. The TUI / gateway surfaces this as a status
    /// indicator so the user knows the context is filling up. After a successful
    /// compression the status reverts to `Ok` (no event is emitted for that).
    ContextPressure {
        /// Estimated current token usage.
        estimated_tokens: usize,
        /// Compression threshold in tokens (context_window × threshold_fraction).
        threshold_tokens: usize,
    },
}

impl std::fmt::Debug for StreamEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Token(t) => write!(f, "Token({t:?})"),
            Self::Reasoning(t) => write!(f, "Reasoning({t:?})"),
            Self::ToolExec { name, .. } => write!(f, "ToolExec({name:?})"),
            Self::ToolDone {
                name,
                duration_ms,
                is_error,
                ..
            } => {
                write!(f, "ToolDone({name:?}, {duration_ms}ms, err={is_error})")
            }
            Self::SubAgentStart {
                task_index,
                task_count,
                goal,
            } => write!(
                f,
                "SubAgentStart({}/{}, {:?})",
                task_index + 1,
                task_count,
                goal
            ),
            Self::SubAgentReasoning {
                task_index,
                task_count,
                text,
            } => write!(
                f,
                "SubAgentReasoning({}/{}, {:?})",
                task_index + 1,
                task_count,
                text
            ),
            Self::SubAgentToolExec {
                task_index,
                task_count,
                name,
                ..
            } => write!(
                f,
                "SubAgentToolExec({}/{}, {:?})",
                task_index + 1,
                task_count,
                name
            ),
            Self::SubAgentFinish {
                task_index,
                task_count,
                status,
                duration_ms,
                ..
            } => write!(
                f,
                "SubAgentFinish({}/{}, {:?}, {}ms)",
                task_index + 1,
                task_count,
                status,
                duration_ms
            ),
            Self::Done => write!(f, "Done"),
            Self::Error(e) => write!(f, "Error({e:?})"),
            Self::Clarify {
                question, choices, ..
            } => {
                if choices.is_some() {
                    write!(f, "Clarify({question:?}, multiple-choice)")
                } else {
                    write!(f, "Clarify({question:?})")
                }
            }
            Self::Approval { command, .. } => write!(f, "Approval({command:?})"),
            Self::SecretRequest {
                var_name, is_sudo, ..
            } => write!(f, "SecretRequest({var_name:?}, sudo={is_sudo})"),
            Self::HookEvent { event, .. } => write!(f, "HookEvent({event:?})"),
            Self::ContextPressure {
                estimated_tokens,
                threshold_tokens,
            } => write!(
                f,
                "ContextPressure(est={estimated_tokens}, threshold={threshold_tokens})"
            ),
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────

/// Parse a platform name string into a `Platform` variant.
///
/// Used by `chat_with_origin` so the gateway can pass the string name
/// of the originating platform and get the correct platform hint into
/// the system prompt.
fn platform_from_str(s: &str) -> Option<Platform> {
    match s {
        "cli" => Some(Platform::Cli),
        "telegram" => Some(Platform::Telegram),
        "discord" => Some(Platform::Discord),
        "slack" => Some(Platform::Slack),
        "whatsapp" => Some(Platform::Whatsapp),
        "feishu" => Some(Platform::Feishu),
        "wecom" => Some(Platform::Wecom),
        "signal" => Some(Platform::Signal),
        "email" => Some(Platform::Email),
        "matrix" => Some(Platform::Matrix),
        "mattermost" => Some(Platform::Mattermost),
        "dingtalk" => Some(Platform::DingTalk),
        "sms" => Some(Platform::Sms),
        "webhook" => Some(Platform::Webhook),
        "api" | "api_server" => Some(Platform::Api),
        "homeassistant" => Some(Platform::HomeAssistant),
        "cron" => Some(Platform::Cron),
        _ => None,
    }
}

// ─── Builder ──────────────────────────────────────────────────────────

pub struct AgentBuilder {
    config: AgentConfig,
    provider: Option<Arc<dyn LLMProvider>>,
    state_db: Option<Arc<SessionDb>>,
    tool_registry: Option<Arc<ToolRegistry>>,
}

impl AgentBuilder {
    pub fn new(model: &str) -> Self {
        Self {
            config: AgentConfig {
                model: model.to_string(),
                ..Default::default()
            },
            provider: None,
            state_db: None,
            tool_registry: None,
        }
    }

    /// Construct from an existing AppConfig.
    pub fn from_config(config: &AppConfig) -> Self {
        // Resolve personality preset → persona instruction addon
        let personality_addon =
            crate::config::resolve_personality(config, &config.display.personality);

        Self {
            config: AgentConfig {
                model: config.model.default_model.clone(),
                enabled_toolsets: config.tools.enabled_toolsets.clone().unwrap_or_default(),
                disabled_toolsets: config.tools.disabled_toolsets.clone().unwrap_or_default(),
                max_iterations: config.model.max_iterations,
                streaming: config.model.streaming,
                save_trajectories: config.save_trajectories,
                skip_context_files: config.skip_context_files,
                skip_memory: config.skip_memory,
                temperature: config.model.temperature,
                model_config: config.model.clone(),
                skills_config: config.skills.clone(),
                delegation_enabled: config.delegation.enabled,
                delegation_model: config.delegation.model.clone(),
                delegation_provider: config.delegation.provider.clone(),
                delegation_max_subagents: config.delegation.max_subagents,
                delegation_max_iterations: config.delegation.max_iterations,
                personality_addon,
                browser: config.browser.clone(),
                checkpoints_enabled: config.checkpoints.enabled,
                checkpoints_max_snapshots: config.checkpoints.max_snapshots,
                terminal_backend: config.terminal.backend.clone(),
                terminal_docker: config.terminal.docker.clone(),
                terminal_ssh: config.terminal.ssh.clone(),
                terminal_modal: config.terminal.modal.clone(),
                terminal_daytona: config.terminal.daytona.clone(),
                terminal_singularity: config.terminal.singularity.clone(),
                compression: config.compression.clone(),
                auxiliary: config.auxiliary.clone(),
                terminal_env_passthrough: config.terminal.env_passthrough.clone(),
                file_allowed_roots: config.tools.file.allowed_roots.clone(),
                path_restrictions: config.security.path_restrictions.clone(),
                ..Default::default()
            },
            provider: None,
            state_db: None,
            tool_registry: None,
        }
    }

    pub fn provider(mut self, p: Arc<dyn LLMProvider>) -> Self {
        self.provider = Some(p);
        self
    }

    pub fn state_db(mut self, db: Arc<SessionDb>) -> Self {
        self.state_db = Some(db);
        self
    }

    pub fn tools(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    pub fn max_iterations(mut self, n: u32) -> Self {
        self.config.max_iterations = n;
        self
    }

    pub fn streaming(mut self, enabled: bool) -> Self {
        self.config.streaming = enabled;
        self
    }

    pub fn platform(mut self, p: Platform) -> Self {
        self.config.platform = p;
        self
    }

    pub fn session_id(mut self, id: String) -> Self {
        self.config.session_id = Some(id);
        self
    }

    /// Set the origin chat context — (platform_name, chat_id) — for gateway sessions.
    ///
    /// This is forwarded into every `ToolContext` so that
    /// `manage_cron_jobs(action='create', deliver='origin')` knows where to
    /// deliver job output without the LLM needing to know the raw chat ID.
    pub fn origin_chat(mut self, platform: String, chat_id: String) -> Self {
        self.config.origin_chat = Some((platform, chat_id));
        self
    }

    pub fn temperature(mut self, t: f32) -> Self {
        self.config.temperature = Some(t);
        self
    }

    pub fn quiet_mode(mut self, enabled: bool) -> Self {
        self.config.quiet_mode = enabled;
        self
    }

    pub fn build(self) -> Result<Agent, AgentError> {
        let provider = self
            .provider
            .ok_or_else(|| AgentError::Config("provider is required".into()))?;
        Ok(Agent::build_runtime_clone(
            self.config,
            provider,
            self.state_db,
            self.tool_registry,
        ))
    }
}

// ─── Agent Drop ───────────────────────────────────────────────────────

impl Drop for Agent {
    /// Cancel the GC task when the Agent is dropped so the background
    /// tokio task doesn't outlive the process table it references.
    fn drop(&mut self) {
        self.gc_cancel.cancel();
    }
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use edgecrab_tools::registry::GatewaySender;
    use edgequake_llm::traits::{
        ChatMessage, CompletionOptions, StreamChunk, ToolChoice, ToolDefinition,
    };
    use futures::StreamExt;

    struct ReasoningStreamProvider;
    struct MockGatewaySender;

    #[test]
    fn platform_from_str_accepts_gateway_api_server_alias() {
        assert_eq!(platform_from_str("api"), Some(Platform::Api));
        assert_eq!(platform_from_str("api_server"), Some(Platform::Api));
    }

    #[async_trait]
    impl LLMProvider for ReasoningStreamProvider {
        fn name(&self) -> &str {
            "reasoning-stream"
        }

        fn model(&self) -> &str {
            "reasoning-stream/mock"
        }

        fn max_context_length(&self) -> usize {
            128_000
        }

        async fn complete(
            &self,
            _prompt: &str,
        ) -> edgequake_llm::Result<edgequake_llm::LLMResponse> {
            Ok(edgequake_llm::LLMResponse::new(
                "fallback complete",
                self.model(),
            ))
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
            let mut response = edgequake_llm::LLMResponse::new("nonstreamed answer", self.model());
            response.thinking_content = Some("hidden reasoning".to_string());
            Ok(response)
        }

        async fn stream(
            &self,
            _prompt: &str,
        ) -> edgequake_llm::Result<futures::stream::BoxStream<'static, edgequake_llm::Result<String>>>
        {
            Ok(futures::stream::iter(vec![Ok("plain stream".to_string())]).boxed())
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
            let chunks = vec![
                Ok(StreamChunk::ThinkingContent {
                    text: "live reasoning".to_string(),
                    tokens_used: Some(3),
                    budget_total: None,
                }),
                Ok(StreamChunk::Content("streamed answer".to_string())),
                Ok(StreamChunk::Finished {
                    reason: "stop".to_string(),
                    ttft_ms: None,
                    usage: None,
                }),
            ];
            Ok(futures::stream::iter(chunks).boxed())
        }

        fn supports_streaming(&self) -> bool {
            true
        }

        fn supports_tool_streaming(&self) -> bool {
            true
        }

        fn supports_function_calling(&self) -> bool {
            true
        }
    }

    #[async_trait]
    impl GatewaySender for MockGatewaySender {
        async fn send_message(
            &self,
            _platform: &str,
            _recipient: &str,
            _message: &str,
        ) -> Result<(), String> {
            Ok(())
        }

        async fn list_targets(&self) -> Result<Vec<String>, String> {
            Ok(vec!["telegram".into()])
        }
    }

    #[test]
    fn iteration_budget_counts_down() {
        let budget = IterationBudget::new(3);
        assert!(budget.try_consume());
        assert!(budget.try_consume());
        assert!(budget.try_consume());
        assert!(!budget.try_consume());
        assert_eq!(budget.used(), 3);
    }

    #[test]
    fn iteration_budget_reset() {
        let budget = IterationBudget::new(2);
        budget.try_consume();
        budget.try_consume();
        assert!(!budget.try_consume());
        budget.reset();
        assert!(budget.try_consume());
        assert_eq!(budget.remaining(), 1);
    }

    #[test]
    fn builder_requires_provider() {
        let result = AgentBuilder::new("test/model").build();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn builder_with_mock_provider() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .max_iterations(10)
            .build()
            .expect("build agent");

        let cfg = agent.config.read().await;
        assert_eq!(cfg.model, "mock");
        assert_eq!(cfg.max_iterations, 10);
        assert_eq!(agent.budget.remaining(), 10);
    }

    #[tokio::test]
    async fn chat_with_mock_provider() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .build()
            .expect("build agent");

        let result = agent.chat("hello").await;
        assert!(result.is_ok());
        // MockProvider returns a canned response
        let response = result.expect("response");
        assert!(!response.is_empty());
    }

    #[test]
    fn from_config_wires_agent_flags() {
        let config = AppConfig {
            save_trajectories: true,
            skip_context_files: true,
            skip_memory: true,
            ..Default::default()
        };

        let builder = AgentBuilder::from_config(&config);

        assert!(builder.config.save_trajectories);
        assert!(builder.config.skip_context_files);
        assert!(builder.config.skip_memory);
    }

    #[tokio::test]
    async fn new_session_resets_state() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .max_iterations(5)
            .build()
            .expect("build agent");

        // Use some budget
        agent.budget.try_consume();
        agent.budget.try_consume();
        assert_eq!(agent.budget.remaining(), 3);

        // Chat to add messages
        let _ = agent.chat("hi").await;

        // Reset
        agent.new_session().await;
        assert_eq!(agent.budget.remaining(), 5);
        let session = agent.session.read().await;
        assert!(session.messages.is_empty());
    }

    #[tokio::test]
    async fn interrupt_triggers_cancellation() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .build()
            .expect("build agent");

        assert!(!agent.is_cancelled());
        agent.interrupt();
        assert!(agent.is_cancelled());
    }

    /// After an interrupt, execute_loop must reset the cancel token so the
    /// agent can still process subsequent conversation turns.  Without this
    /// reset, Ctrl+C would permanently break the agent for the rest of the
    /// session.
    #[tokio::test]
    async fn cancel_resets_between_turns() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .build()
            .expect("build agent");

        // Interrupt and confirm the token is now cancelled
        agent.interrupt();
        assert!(agent.is_cancelled());

        // A new chat call should succeed — execute_loop resets the token
        let result = agent.chat("hello after cancel").await;
        assert!(result.is_ok(), "expected success after cancel: {result:?}");
        // Token must not still be cancelled after a clean (non-interrupted) turn
        assert!(!agent.is_cancelled());
    }

    #[tokio::test]
    async fn conversation_result_structure() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .session_id("test-session".into())
            .build()
            .expect("build agent");

        let result = agent
            .run_conversation("hello", Some("You are helpful."), None)
            .await
            .expect("conversation");

        assert_eq!(result.session_id, "test-session");
        assert_eq!(result.api_calls, 1);
        assert!(!result.interrupted);
        assert_eq!(result.model, "mock");
        // Messages: user + assistant
        assert_eq!(result.messages.len(), 2);
    }

    #[tokio::test]
    async fn session_state_gets_session_id_after_chat() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .build()
            .expect("build agent");

        // Before chat, session_id is None
        {
            let session = agent.session.read().await;
            assert!(session.session_id.is_none());
        }

        // After chat, session_id is populated (auto-generated UUID)
        let _ = agent.chat("hello").await;
        {
            let session = agent.session.read().await;
            assert!(session.session_id.is_some());
        }
    }

    #[tokio::test]
    async fn fork_isolated_creates_fresh_session_state() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let parent = AgentBuilder::new("mock")
            .provider(provider)
            .max_iterations(7)
            .build()
            .expect("build agent");

        let _ = parent.chat("parent turn").await.expect("parent chat");
        let parent_before = parent.session_snapshot().await;

        let child = parent
            .fork_isolated(IsolatedAgentOptions {
                session_id: Some("bg-test".into()),
                quiet_mode: Some(true),
                ..Default::default()
            })
            .await
            .expect("fork isolated");

        let child_before = child.session_snapshot().await;
        let child_cfg = child.config.read().await;
        assert_eq!(child_before.message_count, 0);
        assert_eq!(child_before.api_call_count, 0);
        assert_eq!(child_before.model, parent_before.model);
        assert_eq!(child_cfg.session_id.as_deref(), Some("bg-test"));
        assert_eq!(child.budget.remaining(), 7);
        drop(child_cfg);

        let _ = child.chat("background turn").await.expect("child chat");

        let parent_after = parent.session_snapshot().await;
        let child_after = child.session_snapshot().await;
        assert_eq!(parent_after.message_count, parent_before.message_count);
        assert!(child_after.message_count > 0);
        assert_eq!(child_after.session_id.as_deref(), Some("bg-test"));
        assert_ne!(parent_after.session_id, child_after.session_id);
    }

    #[tokio::test]
    async fn fork_isolated_preserves_gateway_sender() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let parent = AgentBuilder::new("mock")
            .provider(provider)
            .build()
            .expect("build agent");
        parent.set_gateway_sender(Arc::new(MockGatewaySender)).await;

        let child = parent
            .fork_isolated(IsolatedAgentOptions::default())
            .await
            .expect("fork isolated");

        assert!(child.gateway_sender.read().await.is_some());
    }

    #[tokio::test]
    async fn model_config_propagates_to_agent() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .build()
            .expect("build agent");

        let cfg = agent.config.read().await;
        assert!(!cfg.model_config.smart_routing.enabled);
        assert!(cfg.model_config.smart_routing.cheap_model.is_empty());
    }

    #[tokio::test]
    async fn chat_streaming_emits_reasoning_when_enabled() {
        let provider: Arc<dyn LLMProvider> = Arc::new(ReasoningStreamProvider);
        let agent = AgentBuilder::new("reasoning-stream/mock")
            .provider(provider)
            .streaming(true)
            .build()
            .expect("build agent");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        agent
            .chat_streaming("hello", tx)
            .await
            .expect("streaming chat");

        let mut saw_reasoning = false;
        let mut saw_token = false;
        let mut saw_done = false;

        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::Reasoning(text) => saw_reasoning |= !text.is_empty(),
                StreamEvent::Token(text) => saw_token |= !text.is_empty(),
                StreamEvent::Done => {
                    saw_done = true;
                    break;
                }
                _ => {}
            }
        }

        assert!(
            saw_reasoning,
            "expected a live reasoning event when streaming is enabled"
        );
        assert!(saw_token, "expected streamed answer tokens");
        assert!(saw_done, "expected the stream to terminate cleanly");
    }

    #[tokio::test]
    async fn chat_streaming_sends_single_final_answer_when_streaming_disabled() {
        let provider: Arc<dyn LLMProvider> = Arc::new(ReasoningStreamProvider);
        let agent = AgentBuilder::new("reasoning-stream/mock")
            .provider(provider)
            .streaming(false)
            .build()
            .expect("build agent");

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        agent
            .chat_streaming("hello", tx)
            .await
            .expect("streaming chat");

        let mut saw_reasoning = false;
        let mut token_events = 0usize;
        let mut collected_tokens = String::new();

        while let Some(event) = rx.recv().await {
            match event {
                StreamEvent::Reasoning(text) => saw_reasoning |= !text.is_empty(),
                StreamEvent::Token(text) => {
                    token_events += 1;
                    collected_tokens.push_str(&text);
                }
                StreamEvent::Done => break,
                _ => {}
            }
        }

        assert!(
            saw_reasoning,
            "final reasoning content should still be available for think mode"
        );
        assert_eq!(
            token_events, 1,
            "streaming-off mode should emit one complete answer instead of pseudo-streaming chunks"
        );
        assert_eq!(collected_tokens, "nonstreamed answer");
    }

    #[tokio::test]
    async fn session_personality_overlay_replaces_previous_overlay() {
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let agent = AgentBuilder::new("mock")
            .provider(provider)
            .build()
            .expect("build agent");

        {
            let mut session = agent.session.write().await;
            session.cached_system_prompt = Some("Base prompt".to_string());
        }

        agent
            .set_personality_addon(Some("First overlay".to_string()))
            .await;
        assert!(agent.system_prompt().await.is_none());

        {
            let mut session = agent.session.write().await;
            session.cached_system_prompt = Some("Base prompt".to_string());
        }

        agent
            .set_personality_addon(Some("Second overlay".to_string()))
            .await;
        assert!(agent.system_prompt().await.is_none());

        agent.set_personality_addon(None).await;
        assert!(agent.system_prompt().await.is_none());
    }

    #[test]
    fn session_snapshot_prompt_tokens_include_cache_buckets() {
        let snap = SessionSnapshot {
            session_id: None,
            model: "mock/model".into(),
            message_count: 0,
            user_turn_count: 0,
            api_call_count: 0,
            input_tokens: 3,
            output_tokens: 10,
            cache_read_tokens: 15_000,
            cache_write_tokens: 2_000,
            reasoning_tokens: 7,
            last_prompt_tokens: 1_234,
            budget_remaining: 0,
            budget_max: 0,
        };

        assert_eq!(snap.prompt_tokens(), 17_003);
        assert_eq!(snap.total_tokens(), 17_020);
        assert_eq!(snap.context_pressure_tokens(), 1_234);
    }

    #[tokio::test]
    async fn restore_session_reuses_persisted_system_prompt() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = Arc::new(SessionDb::open(&tmp.path().join("sessions.db")).expect("db"));
        let provider: Arc<dyn LLMProvider> = Arc::new(edgequake_llm::MockProvider::new());
        let session = edgecrab_state::SessionRecord {
            id: "restore-me".into(),
            source: "cli".into(),
            user_id: None,
            model: Some("mock/model".into()),
            system_prompt: Some("Persisted system prompt".into()),
            parent_session_id: None,
            started_at: 1.0,
            ended_at: Some(2.0),
            end_reason: None,
            message_count: 2,
            tool_call_count: 0,
            input_tokens: 10,
            output_tokens: 4,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            estimated_cost_usd: None,
            title: Some("restore".into()),
        };
        db.save_session(&session).expect("save session");
        db.save_message("restore-me", &Message::user("hello"), 1.0)
            .expect("save user");
        db.save_message("restore-me", &Message::assistant("hi"), 2.0)
            .expect("save assistant");

        let agent = AgentBuilder::new("mock/model")
            .provider(provider)
            .state_db(db)
            .build()
            .expect("build agent");

        let restored = agent.restore_session("restore-me").await.expect("restore");
        assert_eq!(restored, 2);
        assert_eq!(
            agent.system_prompt().await.as_deref(),
            Some("Persisted system prompt")
        );
    }
}
