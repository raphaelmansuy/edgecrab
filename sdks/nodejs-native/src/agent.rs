//! Node.js Agent bindings — the primary entry point.

use std::sync::Arc;

use napi::Result;

use edgecrab_sdk_core::{MemoryManager, SdkAgent};

use crate::config::JsConfig;
use crate::types::{JsConversationResult, JsSessionSearchHit, JsSessionSummary, JsStreamEvent};

/// Extract text content from a Message's content field.
fn content_to_string(content: &Option<edgecrab_sdk_core::Content>) -> Option<String> {
    match content {
        Some(edgecrab_sdk_core::Content::Text(s)) => Some(s.clone()),
        Some(edgecrab_sdk_core::Content::Parts(parts)) => {
            let texts: Vec<&str> = parts
                .iter()
                .filter_map(|p| match p {
                    edgecrab_sdk_core::ContentPart::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join(""))
            }
        }
        None => None,
    }
}

fn sdk_err(e: impl std::fmt::Display) -> napi::Error {
    napi::Error::from_reason(e.to_string())
}

/// Options for creating a new Agent.
#[napi(object)]
pub struct AgentOptions {
    /// Model identifier (e.g. "anthropic/claude-sonnet-4").
    pub model: Option<String>,
    /// Maximum tool-use iterations per turn.
    pub max_iterations: Option<u32>,
    /// Sampling temperature.
    pub temperature: Option<f64>,
    /// Enable token streaming.
    pub streaming: Option<bool>,
    /// Resume a specific session.
    pub session_id: Option<String>,
    /// Suppress internal logging.
    pub quiet_mode: Option<bool>,
    /// Custom instructions appended to system prompt.
    pub instructions: Option<String>,
    /// Enable only these toolsets.
    pub toolsets: Option<Vec<String>>,
    /// Disable these toolsets.
    pub disabled_toolsets: Option<Vec<String>>,
    /// Disable specific tools by name.
    pub disabled_tools: Option<Vec<String>>,
    /// Skip loading workspace context files like AGENTS.md.
    pub skip_context_files: Option<bool>,
    /// Skip loading persistent memory and user profile sections.
    pub skip_memory: Option<bool>,
}

/// The EdgeCrab Agent — build and interact with autonomous AI agents.
///
/// ```js
/// const { Agent } = require('edgecrab');
/// const agent = new Agent({ model: 'anthropic/claude-sonnet-4' });
/// const reply = await agent.chat('Hello!');
/// ```
#[napi(js_name = "Agent")]
pub struct JsAgent {
    inner: Arc<SdkAgent>,
}

#[napi]
impl JsAgent {
    /// Create a new Agent with the given options.
    #[napi(constructor)]
    pub fn new(options: Option<AgentOptions>) -> Result<Self> {
        let opts = options.unwrap_or(AgentOptions {
            model: None,
            max_iterations: None,
            temperature: None,
            streaming: None,
            session_id: None,
            quiet_mode: None,
            instructions: None,
            toolsets: None,
            disabled_toolsets: None,
            disabled_tools: None,
            skip_context_files: None,
            skip_memory: None,
        });

        let model = opts.model.as_deref().unwrap_or("anthropic/claude-sonnet-4");

        let rt = tokio::runtime::Runtime::new().map_err(sdk_err)?;
        let agent = rt.block_on(async {
            let mut builder = SdkAgent::builder(model).map_err(sdk_err)?;
            if let Some(n) = opts.max_iterations {
                builder = builder.max_iterations(n);
            }
            if let Some(t) = opts.temperature {
                builder = builder.temperature(t as f32);
            }
            if let Some(s) = opts.streaming {
                builder = builder.streaming(s);
            }
            if let Some(id) = opts.session_id {
                builder = builder.session_id(id);
            }
            if let Some(q) = opts.quiet_mode {
                builder = builder.quiet_mode(q);
            }
            if let Some(prompt) = opts.instructions {
                builder = builder.instructions(prompt);
            }
            if let Some(ts) = opts.toolsets {
                builder = builder.enabled_toolsets(ts);
            }
            if let Some(ts) = opts.disabled_toolsets {
                builder = builder.disabled_toolsets(ts);
            }
            if let Some(tools) = opts.disabled_tools {
                builder = builder.disabled_tools(tools);
            }
            if let Some(skip) = opts.skip_context_files {
                builder = builder.skip_context_files(skip);
            }
            if let Some(skip) = opts.skip_memory {
                builder = builder.skip_memory(skip);
            }
            builder.build().map_err(sdk_err)
        })?;

        Ok(Self {
            inner: Arc::new(agent),
        })
    }

    /// Create an Agent from a loaded Config object.
    #[napi(factory)]
    pub fn from_config(config: &JsConfig) -> Result<Self> {
        let agent = SdkAgent::from_config(&config.inner).map_err(sdk_err)?;
        Ok(Self {
            inner: Arc::new(agent),
        })
    }

    /// Send a message and get the response. Returns a Promise<string>.
    #[napi]
    pub async fn chat(&self, message: String) -> Result<String> {
        let agent = Arc::clone(&self.inner);
        agent.chat(&message).await.map_err(sdk_err)
    }

    /// Send a message in a specific working directory. Returns a Promise<string>.
    #[napi]
    pub async fn chat_in_cwd(&self, message: String, cwd: String) -> Result<String> {
        let agent = Arc::clone(&self.inner);
        let path = std::path::PathBuf::from(cwd);
        agent.chat_in_cwd(&message, &path).await.map_err(sdk_err)
    }

    /// Run a full conversation and get detailed results. Returns Promise<ConversationResult>.
    #[napi]
    pub async fn run(&self, message: String) -> Result<JsConversationResult> {
        let agent = Arc::clone(&self.inner);
        let result = agent.run(&message).await.map_err(sdk_err)?;
        Ok(JsConversationResult::from(result))
    }

    /// Run a conversation with optional system prompt and history.
    #[napi]
    pub async fn run_conversation(
        &self,
        message: String,
        system: Option<String>,
        history: Option<Vec<String>>,
    ) -> Result<JsConversationResult> {
        let agent = Arc::clone(&self.inner);
        let hist = history.map(|msgs| {
            msgs.into_iter()
                .map(|text| edgecrab_sdk_core::Message::user(&text))
                .collect::<Vec<_>>()
        });
        let result = agent
            .run_conversation(&message, system.as_deref(), hist)
            .await
            .map_err(sdk_err)?;
        Ok(JsConversationResult::from(result))
    }

    /// Stream events from the agent. Returns Promise<StreamEvent[]>.
    #[napi]
    pub async fn stream(&self, message: String) -> Result<Vec<JsStreamEvent>> {
        let agent = Arc::clone(&self.inner);
        let mut rx = agent.stream(&message).await.map_err(sdk_err)?;
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(JsStreamEvent::from(event));
        }
        Ok(events)
    }

    /// Interrupt the current agent run.
    #[napi]
    pub fn interrupt(&self) {
        self.inner.interrupt();
    }

    /// Check if the agent has been cancelled.
    #[napi(getter)]
    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    /// Start a new session (reset conversation).
    #[napi]
    pub async fn new_session(&self) {
        let agent = Arc::clone(&self.inner);
        agent.new_session().await;
    }

    /// Get the current model name.
    #[napi(getter)]
    pub async fn model(&self) -> String {
        let agent = Arc::clone(&self.inner);
        agent.model().await
    }

    /// Get the current session ID, if any.
    #[napi(getter)]
    pub async fn session_id(&self) -> Option<String> {
        let agent = Arc::clone(&self.inner);
        agent.session_id().await
    }

    /// Get the conversation history as an array of message objects.
    #[napi]
    pub async fn get_history(&self) -> Vec<serde_json::Value> {
        let agent = Arc::clone(&self.inner);
        let messages = agent.messages().await;
        messages
            .into_iter()
            .map(|m| {
                let mut obj = serde_json::Map::new();
                obj.insert("role".into(), serde_json::json!(format!("{:?}", m.role)));
                if let Some(text) = content_to_string(&m.content) {
                    obj.insert("content".into(), serde_json::json!(text));
                }
                serde_json::Value::Object(obj)
            })
            .collect()
    }

    /// Fork the agent into an isolated copy for parallel/background work.
    #[napi]
    pub async fn fork(&self) -> Result<JsAgent> {
        let agent = Arc::clone(&self.inner);
        let forked = agent.fork().await.map_err(sdk_err)?;
        Ok(JsAgent {
            inner: Arc::new(forked),
        })
    }

    /// Export the current session (snapshot + full message history).
    #[napi]
    pub async fn export(&self) -> serde_json::Value {
        let agent = Arc::clone(&self.inner);
        let export = agent.export().await;
        let messages: Vec<serde_json::Value> = export
            .messages
            .iter()
            .map(|m| {
                let mut obj = serde_json::Map::new();
                obj.insert("role".into(), serde_json::json!(format!("{:?}", m.role)));
                if let Some(text) = content_to_string(&m.content) {
                    obj.insert("content".into(), serde_json::json!(text));
                }
                serde_json::Value::Object(obj)
            })
            .collect();
        serde_json::json!({
            "session_id": export.snapshot.session_id,
            "model": export.snapshot.model,
            "message_count": export.snapshot.message_count,
            "api_call_count": export.snapshot.api_call_count,
            "input_tokens": export.snapshot.input_tokens,
            "output_tokens": export.snapshot.output_tokens,
            "messages": messages,
        })
    }

    /// List available tool names.
    #[napi]
    pub async fn tool_names(&self) -> Vec<String> {
        let agent = Arc::clone(&self.inner);
        agent.tool_names().await
    }

    /// Get a summary of toolsets and their tool counts.
    #[napi]
    pub async fn toolset_summary(&self) -> Vec<Vec<serde_json::Value>> {
        let agent = Arc::clone(&self.inner);
        agent
            .toolset_summary()
            .await
            .into_iter()
            .map(|(name, count)| vec![serde_json::json!(name), serde_json::json!(count)])
            .collect()
    }

    /// List recent sessions.
    #[napi]
    pub fn list_sessions(&self, limit: Option<u32>) -> Result<Vec<JsSessionSummary>> {
        let lim = limit.unwrap_or(20) as usize;
        let sessions = self.inner.list_sessions(lim).map_err(sdk_err)?;
        Ok(sessions.into_iter().map(JsSessionSummary::from).collect())
    }

    /// Search sessions by text.
    #[napi]
    pub fn search_sessions(
        &self,
        query: String,
        limit: Option<u32>,
    ) -> Result<Vec<JsSessionSearchHit>> {
        let lim = limit.unwrap_or(10) as usize;
        let hits = self.inner.search_sessions(&query, lim).map_err(sdk_err)?;
        Ok(hits.into_iter().map(JsSessionSearchHit::from).collect())
    }

    /// Set the reasoning effort level.
    #[napi]
    pub async fn set_reasoning_effort(&self, effort: Option<String>) {
        let agent = Arc::clone(&self.inner);
        agent.set_reasoning_effort(effort).await;
    }

    /// Enable or disable streaming mode.
    #[napi]
    pub async fn set_streaming(&self, enabled: bool) {
        let agent = Arc::clone(&self.inner);
        agent.set_streaming(enabled).await;
    }

    /// Get a snapshot of the current session state.
    #[napi]
    pub async fn session_snapshot(&self) -> serde_json::Value {
        let agent = Arc::clone(&self.inner);
        let snap = agent.session_snapshot().await;
        serde_json::json!({
            "session_id": snap.session_id,
            "model": snap.model,
            "message_count": snap.message_count,
            "api_call_count": snap.api_call_count,
            "input_tokens": snap.input_tokens,
            "output_tokens": snap.output_tokens,
        })
    }

    /// Force context compression — summarise conversation history to free context window.
    #[napi]
    pub async fn compress(&self) {
        let agent = Arc::clone(&self.inner);
        agent.compress().await;
    }

    /// Hot-swap the model at runtime. Takes "provider/model" string.
    #[napi]
    pub async fn set_model(&self, model: String) -> Result<()> {
        let agent = Arc::clone(&self.inner);
        agent.set_model(&model).await.map_err(sdk_err)
    }

    /// Run multiple prompts in parallel, returning results in order.
    /// Failed items are returned as null.
    #[napi]
    pub async fn batch(&self, messages: Vec<String>) -> Vec<Option<String>> {
        let agent = Arc::clone(&self.inner);
        let refs: Vec<&str> = messages.iter().map(|s| s.as_str()).collect();
        let results = agent.batch(&refs).await;
        results.into_iter().map(|r| r.ok()).collect()
    }

    /// Get a MemoryManager for reading/writing agent memory.
    #[napi(getter)]
    pub fn memory(&self) -> JsMemoryManager {
        JsMemoryManager {
            inner: self.inner.memory(),
        }
    }
}

/// Programmatic access to the agent's persistent memory files.
///
/// ```js
/// const mem = agent.memory;
/// const content = await mem.read('memory');
/// await mem.write('memory', 'Important fact');
/// ```
#[napi(js_name = "MemoryManager")]
pub struct JsMemoryManager {
    inner: MemoryManager,
}

#[napi]
impl JsMemoryManager {
    /// Read the contents of a memory file.
    /// key: "memory" (MEMORY.md) or "user" (USER.md).
    #[napi]
    pub async fn read(&self, key: Option<String>) -> Result<String> {
        let k = key.as_deref().unwrap_or("memory");
        self.inner.read(k).await.map_err(sdk_err)
    }

    /// Write (append) a new entry to a memory file.
    #[napi]
    pub async fn write(&self, key: String, value: String) -> Result<()> {
        self.inner.write(&key, &value).await.map_err(sdk_err)
    }

    /// Remove an entry by substring match.
    /// Returns true if an entry was removed.
    #[napi]
    pub async fn remove(&self, key: String, old_content: String) -> Result<bool> {
        self.inner.remove(&key, &old_content).await.map_err(sdk_err)
    }

    /// List all entries from a memory file as an array of strings.
    #[napi]
    pub async fn entries(&self, key: Option<String>) -> Result<Vec<String>> {
        let k = key.as_deref().unwrap_or("memory");
        self.inner.entries(k).await.map_err(sdk_err)
    }
}
