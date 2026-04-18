//! Python Agent bindings — the primary entry point.

use std::sync::Arc;

use pyo3::prelude::*;

use edgecrab_sdk_core::{MemoryManager, SdkAgent};

use crate::config::PyConfig;
use crate::error::sdk_err;
use crate::types::{PyConversationResult, PySessionSearchHit, PySessionSummary, PyStreamEvent};

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

/// The EdgeCrab Agent — build and interact with autonomous AI agents.
///
/// Usage (sync):
///     agent = Agent("anthropic/claude-sonnet-4")
///     reply = agent.chat_sync("Hello!")
#[pyclass(name = "Agent")]
pub struct PyAgent {
    inner: Arc<SdkAgent>,
    /// Tokio runtime for sync methods.
    rt: Arc<tokio::runtime::Runtime>,
}

#[pymethods]
impl PyAgent {
    /// Create a new Agent for the given model string (e.g., "anthropic/claude-sonnet-4").
    #[new]
    #[pyo3(signature = (model, *, max_iterations=None, temperature=None, streaming=None, session_id=None, quiet_mode=None, instructions=None, toolsets=None, disabled_toolsets=None, disabled_tools=None, skip_context_files=None, skip_memory=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        model: &str,
        max_iterations: Option<u32>,
        temperature: Option<f32>,
        streaming: Option<bool>,
        session_id: Option<String>,
        quiet_mode: Option<bool>,
        instructions: Option<String>,
        toolsets: Option<Vec<String>>,
        disabled_toolsets: Option<Vec<String>>,
        disabled_tools: Option<Vec<String>>,
        skip_context_files: Option<bool>,
        skip_memory: Option<bool>,
    ) -> PyResult<Self> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;

        let agent = rt.block_on(async {
            let mut builder = SdkAgent::builder(model).map_err(sdk_err)?;
            if let Some(n) = max_iterations {
                builder = builder.max_iterations(n);
            }
            if let Some(t) = temperature {
                builder = builder.temperature(t);
            }
            if let Some(s) = streaming {
                builder = builder.streaming(s);
            }
            if let Some(id) = session_id {
                builder = builder.session_id(id);
            }
            if let Some(q) = quiet_mode {
                builder = builder.quiet_mode(q);
            }
            if let Some(prompt) = instructions {
                builder = builder.instructions(prompt);
            }
            if let Some(ts) = toolsets {
                builder = builder.enabled_toolsets(ts);
            }
            if let Some(ts) = disabled_toolsets {
                builder = builder.disabled_toolsets(ts);
            }
            if let Some(tools) = disabled_tools {
                builder = builder.disabled_tools(tools);
            }
            if let Some(skip) = skip_context_files {
                builder = builder.skip_context_files(skip);
            }
            if let Some(skip) = skip_memory {
                builder = builder.skip_memory(skip);
            }
            builder.build().map_err(sdk_err)
        })?;

        Ok(Self {
            inner: Arc::new(agent),
            rt: Arc::new(rt),
        })
    }

    /// Create an Agent from a loaded Config object.
    #[staticmethod]
    fn from_config(config: &PyConfig) -> PyResult<Self> {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
        let agent = SdkAgent::from_config(&config.inner).map_err(sdk_err)?;
        Ok(Self {
            inner: Arc::new(agent),
            rt: Arc::new(rt),
        })
    }

    /// Send a message and get the response (sync/blocking).
    fn chat_sync(&self, message: &str) -> PyResult<String> {
        let agent = Arc::clone(&self.inner);
        let msg = message.to_string();
        self.rt
            .block_on(async move { agent.chat(&msg).await })
            .map_err(sdk_err)
    }

    /// Run a full conversation and get detailed results (sync/blocking).
    fn run_sync(&self, message: &str) -> PyResult<PyConversationResult> {
        let agent = Arc::clone(&self.inner);
        let msg = message.to_string();
        let result = self
            .rt
            .block_on(async move { agent.run(&msg).await })
            .map_err(sdk_err)?;
        Ok(PyConversationResult::from(result))
    }

    /// Stream events from the agent (sync — returns a list of events).
    fn stream_sync(&self, message: &str) -> PyResult<Vec<PyStreamEvent>> {
        let agent = Arc::clone(&self.inner);
        let msg = message.to_string();
        let events = self.rt.block_on(async move {
            let mut rx = agent.stream(&msg).await.map_err(sdk_err)?;
            let mut events = Vec::new();
            while let Some(event) = rx.recv().await {
                events.push(PyStreamEvent::from(event));
            }
            Ok::<_, PyErr>(events)
        })?;
        Ok(events)
    }

    /// Interrupt the current agent run.
    fn interrupt(&self) {
        self.inner.interrupt();
    }

    /// Check if the agent has been cancelled.
    fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    /// Start a new session (reset conversation).
    fn new_session(&self) -> PyResult<()> {
        let agent = Arc::clone(&self.inner);
        self.rt.block_on(async move {
            agent.new_session().await;
        });
        Ok(())
    }

    /// Get the current model name.
    #[getter]
    fn model(&self) -> PyResult<String> {
        let agent = Arc::clone(&self.inner);
        Ok(self.rt.block_on(async move { agent.model().await }))
    }

    /// List available tool names.
    fn tool_names(&self) -> PyResult<Vec<String>> {
        let agent = Arc::clone(&self.inner);
        Ok(self.rt.block_on(async move { agent.tool_names().await }))
    }

    /// Get a summary of toolsets and their tool counts.
    fn toolset_summary(&self) -> PyResult<Vec<(String, usize)>> {
        let agent = Arc::clone(&self.inner);
        Ok(self
            .rt
            .block_on(async move { agent.toolset_summary().await }))
    }

    /// List recent sessions.
    #[pyo3(signature = (limit=20))]
    fn list_sessions(&self, limit: usize) -> PyResult<Vec<PySessionSummary>> {
        let sessions = self.inner.list_sessions(limit).map_err(sdk_err)?;
        Ok(sessions.into_iter().map(PySessionSummary::from).collect())
    }

    /// Search sessions by text.
    #[pyo3(signature = (query, limit=10))]
    fn search_sessions(&self, query: &str, limit: usize) -> PyResult<Vec<PySessionSearchHit>> {
        let hits = self.inner.search_sessions(query, limit).map_err(sdk_err)?;
        Ok(hits.into_iter().map(PySessionSearchHit::from).collect())
    }

    /// Set the reasoning effort level.
    #[pyo3(signature = (effort=None))]
    fn set_reasoning_effort(&self, effort: Option<String>) -> PyResult<()> {
        let agent = Arc::clone(&self.inner);
        self.rt.block_on(async move {
            agent.set_reasoning_effort(effort).await;
        });
        Ok(())
    }

    /// Enable or disable streaming mode.
    fn set_streaming(&self, enabled: bool) -> PyResult<()> {
        let agent = Arc::clone(&self.inner);
        self.rt.block_on(async move {
            agent.set_streaming(enabled).await;
        });
        Ok(())
    }

    /// Send a message with a specific working directory.
    fn chat_in_cwd(&self, message: &str, cwd: &str) -> PyResult<String> {
        let agent = Arc::clone(&self.inner);
        let msg = message.to_string();
        let path = std::path::PathBuf::from(cwd);
        self.rt
            .block_on(async move { agent.chat_in_cwd(&msg, &path).await })
            .map_err(sdk_err)
    }

    /// Get a snapshot of the current session state as a dict.
    fn session_snapshot(&self) -> PyResult<PyObject> {
        let agent = Arc::clone(&self.inner);
        let snap = self
            .rt
            .block_on(async move { agent.session_snapshot().await });
        Python::with_gil(|py| {
            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("session_id", &snap.session_id)?;
            dict.set_item("model", &snap.model)?;
            dict.set_item("message_count", snap.message_count)?;
            dict.set_item("api_call_count", snap.api_call_count)?;
            dict.set_item("input_tokens", snap.input_tokens)?;
            dict.set_item("output_tokens", snap.output_tokens)?;
            Ok(dict.into())
        })
    }

    /// Force context compression — summarise conversation history to free context window.
    fn compress(&self) -> PyResult<()> {
        let agent = Arc::clone(&self.inner);
        self.rt.block_on(async move {
            agent.compress().await;
        });
        Ok(())
    }

    /// Hot-swap the model at runtime. Takes "provider/model" string.
    fn set_model(&self, model: &str) -> PyResult<()> {
        let agent = Arc::clone(&self.inner);
        let m = model.to_string();
        self.rt
            .block_on(async move { agent.set_model(&m).await })
            .map_err(sdk_err)
    }

    /// Run multiple prompts in parallel, returning results in order.
    fn batch(&self, messages: Vec<String>) -> PyResult<Vec<PyObject>> {
        let agent = Arc::clone(&self.inner);
        let results = self.rt.block_on(async move {
            let refs: Vec<&str> = messages.iter().map(|s| s.as_str()).collect();
            agent.batch(&refs).await
        });
        Python::with_gil(|py| {
            let out: Vec<PyObject> = results
                .into_iter()
                .map(|r| match r {
                    Ok(text) => text.into_pyobject(py).unwrap().into_any().unbind(),
                    Err(_e) => py.None(),
                })
                .collect();
            Ok(out)
        })
    }

    /// Get the current session ID, if any.
    #[getter]
    fn session_id(&self) -> PyResult<Option<String>> {
        let agent = Arc::clone(&self.inner);
        Ok(self.rt.block_on(async move { agent.session_id().await }))
    }

    /// Get the conversation history as a list of dicts.
    #[getter]
    fn history(&self) -> PyResult<Vec<PyObject>> {
        let agent = Arc::clone(&self.inner);
        let messages = self.rt.block_on(async move { agent.messages().await });
        Python::with_gil(|py| {
            messages
                .into_iter()
                .map(|m| {
                    let dict = pyo3::types::PyDict::new(py);
                    dict.set_item("role", format!("{:?}", m.role))?;
                    if let Some(text) = content_to_string(&m.content) {
                        dict.set_item("content", text)?;
                    }
                    Ok(dict.into())
                })
                .collect()
        })
    }

    /// Fork the agent into an isolated copy for parallel/background work.
    fn fork(&self) -> PyResult<Self> {
        let agent = Arc::clone(&self.inner);
        let forked = self
            .rt
            .block_on(async move { agent.fork().await })
            .map_err(sdk_err)?;
        Ok(Self {
            inner: Arc::new(forked),
            rt: Arc::clone(&self.rt),
        })
    }

    /// Run a conversation with optional system prompt and message history.
    #[pyo3(signature = (message, *, system=None, history=None))]
    fn run_conversation(
        &self,
        message: &str,
        system: Option<&str>,
        history: Option<Vec<String>>,
    ) -> PyResult<PyConversationResult> {
        let agent = Arc::clone(&self.inner);
        let msg = message.to_string();
        let sys = system.map(|s| s.to_string());
        // Convert simple string messages into user Messages
        let hist = history.map(|msgs| {
            msgs.into_iter()
                .map(|text| edgecrab_sdk_core::Message::user(&text))
                .collect::<Vec<_>>()
        });
        let result = self
            .rt
            .block_on(async move { agent.run_conversation(&msg, sys.as_deref(), hist).await })
            .map_err(sdk_err)?;
        Ok(PyConversationResult::from(result))
    }

    /// Export the current session (snapshot + full message history).
    fn export(&self) -> PyResult<PyObject> {
        let agent = Arc::clone(&self.inner);
        let export = self.rt.block_on(async move { agent.export().await });
        Python::with_gil(|py| {
            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("session_id", export.snapshot.session_id)?;
            dict.set_item("model", &export.snapshot.model)?;
            dict.set_item("message_count", export.snapshot.message_count)?;
            dict.set_item("api_call_count", export.snapshot.api_call_count)?;
            dict.set_item("input_tokens", export.snapshot.input_tokens)?;
            dict.set_item("output_tokens", export.snapshot.output_tokens)?;
            let messages: Vec<PyObject> = export
                .messages
                .iter()
                .map(|m| {
                    let d = pyo3::types::PyDict::new(py);
                    d.set_item("role", format!("{:?}", m.role)).ok();
                    if let Some(text) = content_to_string(&m.content) {
                        d.set_item("content", text).ok();
                    }
                    d.into()
                })
                .collect();
            dict.set_item("messages", messages)?;
            Ok(dict.into())
        })
    }

    /// Get a MemoryManager for reading/writing agent memory.
    #[getter]
    fn memory(&self) -> PyResult<PyMemoryManager> {
        let mem = self.inner.memory();
        Ok(PyMemoryManager {
            inner: mem,
            rt: Arc::clone(&self.rt),
        })
    }

    fn __repr__(&self) -> PyResult<String> {
        let agent = Arc::clone(&self.inner);
        let model = self.rt.block_on(async move { agent.model().await });
        Ok(format!("Agent(model='{model}')"))
    }
}

/// Programmatic access to the agent's persistent memory files.
///
/// Usage:
///     mem = agent.memory
///     content = mem.read("memory")
///     mem.write("memory", "Important fact")
#[pyclass(name = "MemoryManager")]
pub struct PyMemoryManager {
    inner: MemoryManager,
    rt: Arc<tokio::runtime::Runtime>,
}

#[pymethods]
impl PyMemoryManager {
    /// Read the contents of a memory file.
    /// key: "memory" (MEMORY.md) or "user" (USER.md).
    #[pyo3(signature = (key="memory"))]
    fn read(&self, key: &str) -> PyResult<String> {
        let inner = self.inner.clone();
        let k = key.to_string();
        self.rt
            .block_on(async move { inner.read(&k).await })
            .map_err(sdk_err)
    }

    /// Write (append) a new entry to a memory file.
    /// key: "memory" or "user". value: content to add.
    #[pyo3(signature = (key, value))]
    fn write(&self, key: &str, value: &str) -> PyResult<()> {
        let inner = self.inner.clone();
        let k = key.to_string();
        let v = value.to_string();
        self.rt
            .block_on(async move { inner.write(&k, &v).await })
            .map_err(sdk_err)
    }

    /// Remove an entry by substring match.
    /// Returns True if an entry was removed.
    #[pyo3(signature = (key, old_content))]
    fn remove(&self, key: &str, old_content: &str) -> PyResult<bool> {
        let inner = self.inner.clone();
        let k = key.to_string();
        let old = old_content.to_string();
        self.rt
            .block_on(async move { inner.remove(&k, &old).await })
            .map_err(sdk_err)
    }

    /// List all entries from a memory file as a list of strings.
    #[pyo3(signature = (key="memory"))]
    fn entries(&self, key: &str) -> PyResult<Vec<String>> {
        let inner = self.inner.clone();
        let k = key.to_string();
        self.rt
            .block_on(async move { inner.entries(&k).await })
            .map_err(sdk_err)
    }

    fn __repr__(&self) -> String {
        "MemoryManager()".into()
    }
}
