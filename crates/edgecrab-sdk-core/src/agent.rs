//! SDK Agent — the primary entry point for interacting with EdgeCrab.
//!
//! [`SdkAgent`] wraps the internal [`Agent`] with a stable, ergonomic API.
//! It handles provider auto-creation from a model string and exposes
//! simple/streaming/full conversation interfaces.

use std::path::Path;
use std::sync::Arc;

use tokio::sync::mpsc;

use edgecrab_core::agent::{
    Agent, AgentBuilder, ConversationResult, IsolatedAgentOptions, SessionSnapshot, StreamEvent,
};
use edgecrab_state::{SessionDb, SessionSearchHit, SessionSummary};
use edgecrab_tools::registry::ToolRegistry;
use edgecrab_types::{Message, Platform};

use crate::config::SdkConfig;
use crate::convert::parse_model_string;
use crate::error::SdkError;
use crate::memory::MemoryManager;

/// The SDK agent — wraps the EdgeCrab agent runtime with a stable API.
///
/// # Construction
///
/// ```rust,no_run
/// use edgecrab_sdk_core::SdkAgent;
///
/// # async fn example() -> Result<(), edgecrab_sdk_core::SdkError> {
/// // Minimal — model string with auto provider detection
/// let agent = SdkAgent::new("anthropic/claude-sonnet-4")?;
///
/// // With configuration
/// let agent = SdkAgent::builder("anthropic/claude-sonnet-4")?
///     .max_iterations(20)
///     .temperature(0.7)
///     .build()?;
/// # Ok(())
/// # }
/// ```
///
/// # Simple API
///
/// ```rust,no_run
/// # use edgecrab_sdk_core::SdkAgent;
/// # async fn example() -> Result<(), edgecrab_sdk_core::SdkError> {
/// let agent = SdkAgent::new("anthropic/claude-sonnet-4")?;
/// let reply = agent.chat("What is EdgeCrab?").await?;
/// println!("{reply}");
/// # Ok(())
/// # }
/// ```
pub struct SdkAgent {
    inner: Agent,
}

/// Full session export containing the snapshot metadata and complete message history.
#[derive(Debug, Clone)]
pub struct SessionExport {
    /// Session metadata snapshot.
    pub snapshot: SessionSnapshot,
    /// Complete conversation message history.
    pub messages: Vec<Message>,
}

// ── Builder ──────────────────────────────────────────────────────────

/// Fluent builder for [`SdkAgent`].
pub struct SdkAgentBuilder {
    model: String,
    provider_name: String,
    max_iterations: Option<u32>,
    temperature: Option<f32>,
    streaming: Option<bool>,
    platform: Option<Platform>,
    session_id: Option<String>,
    quiet_mode: Option<bool>,
    tool_registry: Option<Arc<ToolRegistry>>,
    state_db: Option<Arc<SessionDb>>,
    enabled_toolsets: Option<Vec<String>>,
    disabled_toolsets: Option<Vec<String>>,
    disabled_tools: Option<Vec<String>>,
    instructions: Option<String>,
    skip_context_files: Option<bool>,
    skip_memory: Option<bool>,
}

impl SdkAgentBuilder {
    fn new(model: &str) -> Result<Self, SdkError> {
        let (provider_name, model_name) = parse_model_string(model)?;
        Ok(Self {
            model: format!("{provider_name}/{model_name}"),
            provider_name,
            max_iterations: None,
            temperature: None,
            streaming: None,
            platform: None,
            session_id: None,
            quiet_mode: None,
            tool_registry: None,
            state_db: None,
            enabled_toolsets: None,
            disabled_toolsets: None,
            disabled_tools: None,
            instructions: None,
            skip_context_files: None,
            skip_memory: None,
        })
    }

    /// Set the maximum number of ReAct loop iterations.
    pub fn max_iterations(mut self, n: u32) -> Self {
        self.max_iterations = Some(n);
        self
    }

    /// Set the LLM temperature.
    pub fn temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }

    /// Enable or disable streaming.
    pub fn streaming(mut self, enabled: bool) -> Self {
        self.streaming = Some(enabled);
        self
    }

    /// Set the platform context.
    pub fn platform(mut self, p: Platform) -> Self {
        self.platform = Some(p);
        self
    }

    /// Set a specific session ID.
    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    /// Enable quiet mode (suppress tool progress output).
    pub fn quiet_mode(mut self, enabled: bool) -> Self {
        self.quiet_mode = Some(enabled);
        self
    }

    /// Provide a pre-built tool registry.
    pub fn tools(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    /// Provide a session database.
    pub fn state_db(mut self, db: Arc<SessionDb>) -> Self {
        self.state_db = Some(db);
        self
    }

    /// Set enabled toolsets (only tools from these toolsets will be available).
    pub fn enabled_toolsets(mut self, toolsets: Vec<String>) -> Self {
        self.enabled_toolsets = Some(toolsets);
        self
    }

    /// Set disabled toolsets (tools from these toolsets will be excluded).
    pub fn disabled_toolsets(mut self, toolsets: Vec<String>) -> Self {
        self.disabled_toolsets = Some(toolsets);
        self
    }

    /// Set disabled tools by name.
    pub fn disabled_tools(mut self, tools: Vec<String>) -> Self {
        self.disabled_tools = Some(tools);
        self
    }

    /// Set a custom system prompt (instructions) appended to the base prompt.
    pub fn instructions(mut self, prompt: impl Into<String>) -> Self {
        self.instructions = Some(prompt.into());
        self
    }

    /// Skip loading workspace context files such as AGENTS.md during prompt assembly.
    pub fn skip_context_files(mut self, skip: bool) -> Self {
        self.skip_context_files = Some(skip);
        self
    }

    /// Skip loading persistent memory and user profile sections during prompt assembly.
    pub fn skip_memory(mut self, skip: bool) -> Self {
        self.skip_memory = Some(skip);
        self
    }

    /// Build the [`SdkAgent`].
    pub fn build(self) -> Result<SdkAgent, SdkError> {
        // model_name is everything after "provider/" — guaranteed by parse_model_string
        let model_name = &self.model[self.provider_name.len() + 1..];

        let provider = edgecrab_tools::create_provider_for_model(&self.provider_name, model_name)
            .map_err(|msg| SdkError::Provider {
            model: self.model.clone(),
            message: msg,
        })?;

        // Build the internal Agent
        let mut builder = AgentBuilder::new(&self.model).provider(provider);

        if let Some(n) = self.max_iterations {
            builder = builder.max_iterations(n);
        }
        if let Some(t) = self.temperature {
            builder = builder.temperature(t);
        }
        if let Some(s) = self.streaming {
            builder = builder.streaming(s);
        }
        if let Some(p) = self.platform {
            builder = builder.platform(p);
        }
        if let Some(ref id) = self.session_id {
            builder = builder.session_id(id.clone());
        }
        if let Some(q) = self.quiet_mode {
            builder = builder.quiet_mode(q);
        }
        if let Some(registry) = self.tool_registry {
            builder = builder.tools(registry);
        }
        if let Some(db) = self.state_db {
            builder = builder.state_db(db);
        }
        if let Some(toolsets) = self.enabled_toolsets {
            builder = builder.enabled_toolsets(toolsets);
        }
        if let Some(toolsets) = self.disabled_toolsets {
            builder = builder.disabled_toolsets(toolsets);
        }
        if let Some(tools) = self.disabled_tools {
            builder = builder.disabled_tools(tools);
        }
        if let Some(prompt) = self.instructions {
            builder = builder.custom_system_prompt(prompt);
        }
        if let Some(skip) = self.skip_context_files {
            builder = builder.skip_context_files(skip);
        }
        if let Some(skip) = self.skip_memory {
            builder = builder.skip_memory(skip);
        }

        let agent = builder.build().map_err(SdkError::Agent)?;

        Ok(SdkAgent { inner: agent })
    }
}

// ── SdkAgent implementation ──────────────────────────────────────────

impl SdkAgent {
    /// Create an agent with default settings for the given model.
    ///
    /// The model string must be `"provider/model"` (e.g. `"anthropic/claude-sonnet-4"`).
    pub fn new(model: &str) -> Result<Self, SdkError> {
        SdkAgentBuilder::new(model)?.build()
    }

    /// Start building an agent with custom settings.
    pub fn builder(model: &str) -> Result<SdkAgentBuilder, SdkError> {
        SdkAgentBuilder::new(model)
    }

    /// Create an agent from a loaded SDK config.
    ///
    /// This uses [`AgentBuilder::from_config()`] internally which wires all
    /// AppConfig fields (toolsets, delegation, compression, etc.) into the agent.
    pub fn from_config(config: &SdkConfig) -> Result<Self, SdkError> {
        let agent_builder = AgentBuilder::from_config(config.as_inner());
        let model_str = &config.as_inner().model.default_model;
        let (provider_name, model_name) = parse_model_string(model_str)?;
        let provider = edgecrab_tools::create_provider_for_model(&provider_name, &model_name)
            .map_err(|msg| SdkError::Provider {
                model: model_str.clone(),
                message: msg,
            })?;
        let agent = agent_builder
            .provider(provider)
            .build()
            .map_err(SdkError::Agent)?;
        Ok(Self { inner: agent })
    }

    // ── Simple API ───────────────────────────────────────────────────

    /// Send a message and get the final response.
    ///
    /// This is the simplest way to use the agent — one message in, one response out.
    pub async fn chat(&self, message: &str) -> Result<String, SdkError> {
        self.inner.chat(message).await.map_err(SdkError::Agent)
    }

    /// Send a message with a specific working directory.
    pub async fn chat_in_cwd(&self, message: &str, cwd: &Path) -> Result<String, SdkError> {
        self.inner
            .chat_in_cwd(message, cwd)
            .await
            .map_err(SdkError::Agent)
    }

    // ── Streaming API ────────────────────────────────────────────────

    /// Send a message and stream events back.
    ///
    /// Returns a receiver channel that yields [`StreamEvent`] values as the
    /// agent processes the request. Events include tokens, tool executions,
    /// sub-agent activity, and completion.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use edgecrab_sdk_core::SdkAgent;
    /// # async fn example() -> Result<(), edgecrab_sdk_core::SdkError> {
    /// let agent = SdkAgent::new("anthropic/claude-sonnet-4")?;
    /// let mut rx = agent.stream("Explain Rust ownership").await?;
    /// while let Some(event) = rx.recv().await {
    ///     match event {
    ///         edgecrab_sdk_core::StreamEvent::Token(text) => print!("{text}"),
    ///         edgecrab_sdk_core::StreamEvent::Done => break,
    ///         _ => {}
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn stream(
        &self,
        message: &str,
    ) -> Result<mpsc::UnboundedReceiver<StreamEvent>, SdkError> {
        let (tx, rx) = mpsc::unbounded_channel();
        let msg = message.to_string();
        let agent = &self.inner;

        // Spawn the streaming task — it sends events on `tx`
        agent
            .chat_streaming(&msg, tx)
            .await
            .map_err(SdkError::Agent)?;

        Ok(rx)
    }

    // ── Full Conversation API ────────────────────────────────────────

    /// Run a full conversation and get a detailed result.
    ///
    /// Returns [`ConversationResult`] with the response, usage, cost,
    /// tool errors, and full message trace.
    pub async fn run(&self, message: &str) -> Result<ConversationResult, SdkError> {
        self.inner
            .run_conversation(message, None, None)
            .await
            .map_err(SdkError::Agent)
    }

    /// Run a conversation with custom system prompt and/or history.
    pub async fn run_conversation(
        &self,
        message: &str,
        system: Option<&str>,
        history: Option<Vec<Message>>,
    ) -> Result<ConversationResult, SdkError> {
        self.inner
            .run_conversation(message, system, history)
            .await
            .map_err(SdkError::Agent)
    }

    // ── Session Management ───────────────────────────────────────────

    /// Fork the agent into an isolated copy for parallel/background work.
    pub async fn fork(&self) -> Result<Self, SdkError> {
        let inner = self
            .inner
            .fork_isolated(IsolatedAgentOptions::default())
            .await
            .map_err(SdkError::Agent)?;
        Ok(Self { inner })
    }

    /// Interrupt the current agent run (cancellation).
    pub fn interrupt(&self) {
        self.inner.interrupt();
    }

    /// Check if the agent has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    /// Start a new session (reset conversation history).
    pub async fn new_session(&self) {
        self.inner.new_session().await;
    }

    /// Get the current session ID, if any.
    pub async fn session_id(&self) -> Option<String> {
        self.inner.session_snapshot().await.session_id
    }

    /// Get a snapshot of the current session state.
    pub async fn session_snapshot(&self) -> SessionSnapshot {
        self.inner.session_snapshot().await
    }

    /// Get the conversation history.
    pub async fn messages(&self) -> Vec<Message> {
        self.inner.messages().await
    }

    /// Get the current model name.
    pub async fn model(&self) -> String {
        self.inner.model().await
    }

    /// List recent sessions from the state database.
    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionSummary>, SdkError> {
        self.inner.list_sessions(limit).map_err(SdkError::Agent)
    }

    /// Search sessions using full-text search.
    pub fn search_sessions(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SessionSearchHit>, SdkError> {
        match self.inner.state_db_handle() {
            Some(db) => db
                .search_sessions_rich(query, limit)
                .map_err(SdkError::Agent),
            None => Ok(vec![]),
        }
    }

    /// Export the current session: snapshot + full message history.
    pub async fn export(&self) -> SessionExport {
        let snapshot = self.inner.session_snapshot().await;
        let messages = self.inner.messages().await;
        SessionExport { snapshot, messages }
    }

    // ── Configuration ────────────────────────────────────────────────

    /// List available tool names.
    pub async fn tool_names(&self) -> Vec<String> {
        self.inner.tool_names().await
    }

    /// Get a summary of toolsets and their tool counts.
    pub async fn toolset_summary(&self) -> Vec<(String, usize)> {
        self.inner.toolset_summary().await
    }

    /// Set the reasoning effort level.
    pub async fn set_reasoning_effort(&self, level: Option<String>) {
        self.inner.set_reasoning_effort(level).await;
    }

    /// Enable or disable streaming.
    pub async fn set_streaming(&self, enabled: bool) {
        self.inner.set_streaming(enabled).await;
    }

    /// Force context compression — summarises the conversation history to free
    /// context window space. Equivalent to the CLI `/compress` command.
    pub async fn compress(&self) {
        self.inner.force_compress().await;
    }

    /// Hot-swap the model at runtime. Takes a `"provider/model"` string
    /// (e.g. `"openai/gpt-4o"`). Builds the new provider automatically.
    ///
    /// In-flight conversations are not affected — the new model takes effect
    /// on the next `chat()` / `run()` call.
    pub async fn set_model(&self, model: &str) -> Result<(), SdkError> {
        let (provider_name, model_name) = crate::convert::parse_model_string(model)?;
        let provider = edgecrab_tools::create_provider_for_model(&provider_name, &model_name)
            .map_err(SdkError::Config)?;
        self.inner
            .swap_model(format!("{provider_name}/{model_name}"), provider)
            .await;
        Ok(())
    }

    /// Run multiple prompts in parallel, returning results in order.
    ///
    /// Each prompt runs in a forked agent so conversation histories stay
    /// independent. Errors in individual prompts don't abort the batch.
    pub async fn batch(&self, messages: &[&str]) -> Vec<Result<String, SdkError>> {
        let mut handles = Vec::with_capacity(messages.len());
        for msg in messages {
            let forked = match self.fork().await {
                Ok(f) => f,
                Err(e) => {
                    handles.push(tokio::spawn(async move { Err(e) }));
                    continue;
                }
            };
            let msg = msg.to_string();
            handles.push(tokio::spawn(async move { forked.chat(&msg).await }));
        }

        let mut results = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(Err(SdkError::Config(format!("Task panicked: {e}")))),
            }
        }
        results
    }

    // ── Internal access ──────────────────────────────────────────────

    /// Get a reference to the underlying `Agent`.
    ///
    /// Use this for advanced operations not covered by the SDK API.
    pub fn as_inner(&self) -> &Agent {
        &self.inner
    }

    /// Get a [`MemoryManager`] for programmatic access to the agent's
    /// persistent memory files (MEMORY.md / USER.md).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use edgecrab_sdk_core::SdkAgent;
    /// # async fn example() -> Result<(), edgecrab_sdk_core::SdkError> {
    /// let agent = SdkAgent::new("anthropic/claude-sonnet-4")?;
    /// let mem = agent.memory();
    /// mem.write("memory", "User prefers concise answers").await?;
    /// let content = mem.read("memory").await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn memory(&self) -> MemoryManager {
        let home = edgecrab_core::config::edgecrab_home();
        MemoryManager::new(home)
    }
}
