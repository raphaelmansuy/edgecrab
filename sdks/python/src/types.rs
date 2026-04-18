//! Python type bindings for SDK types.

use pyo3::prelude::*;

/// Result of a full conversation run.
#[pyclass(name = "ConversationResult")]
#[derive(Clone)]
pub struct PyConversationResult {
    /// The final assistant response text.
    #[pyo3(get)]
    pub response: String,
    /// Session ID used for this conversation.
    #[pyo3(get)]
    pub session_id: String,
    /// Number of API calls made.
    #[pyo3(get)]
    pub api_calls: u32,
    /// Whether the agent was interrupted.
    #[pyo3(get)]
    pub interrupted: bool,
    /// Whether the iteration budget was exhausted.
    #[pyo3(get)]
    pub budget_exhausted: bool,
    /// Model used.
    #[pyo3(get)]
    pub model: String,
    /// Total input tokens.
    #[pyo3(get)]
    pub input_tokens: u64,
    /// Total output tokens.
    #[pyo3(get)]
    pub output_tokens: u64,
    /// Total cost in USD.
    #[pyo3(get)]
    pub total_cost: f64,
}

#[pymethods]
impl PyConversationResult {
    fn __repr__(&self) -> String {
        format!(
            "ConversationResult(model='{}', api_calls={}, cost=${:.4})",
            self.model, self.api_calls, self.total_cost
        )
    }
}

impl From<edgecrab_sdk_core::ConversationResult> for PyConversationResult {
    fn from(r: edgecrab_sdk_core::ConversationResult) -> Self {
        Self {
            response: r.final_response,
            session_id: r.session_id,
            api_calls: r.api_calls,
            interrupted: r.interrupted,
            budget_exhausted: r.budget_exhausted,
            model: r.model,
            input_tokens: r.usage.input_tokens,
            output_tokens: r.usage.output_tokens,
            total_cost: r.cost.total_cost,
        }
    }
}

/// A streaming event from the agent.
#[pyclass(name = "StreamEvent")]
#[derive(Clone)]
pub struct PyStreamEvent {
    /// Event type: "token", "reasoning", "tool_exec", "tool_result", "done", "error"
    #[pyo3(get)]
    pub event_type: String,
    /// Event data (text content, tool name, error message, etc.)
    #[pyo3(get)]
    pub data: String,
}

#[pymethods]
impl PyStreamEvent {
    fn __repr__(&self) -> String {
        format!(
            "StreamEvent(type='{}', data='{}')",
            self.event_type,
            &self.data[..self.data.len().min(50)]
        )
    }
}

impl From<edgecrab_sdk_core::StreamEvent> for PyStreamEvent {
    fn from(e: edgecrab_sdk_core::StreamEvent) -> Self {
        match e {
            edgecrab_sdk_core::StreamEvent::Token(text) => Self {
                event_type: "token".into(),
                data: text,
            },
            edgecrab_sdk_core::StreamEvent::Reasoning(text) => Self {
                event_type: "reasoning".into(),
                data: text,
            },
            edgecrab_sdk_core::StreamEvent::ToolExec {
                name, args_json, ..
            } => Self {
                event_type: "tool_exec".into(),
                data: format!("{name}: {args_json}"),
            },
            edgecrab_sdk_core::StreamEvent::ToolDone {
                name,
                result_preview,
                ..
            } => Self {
                event_type: "tool_result".into(),
                data: format!("{name}: {}", result_preview.unwrap_or_default()),
            },
            edgecrab_sdk_core::StreamEvent::Done => Self {
                event_type: "done".into(),
                data: String::new(),
            },
            edgecrab_sdk_core::StreamEvent::Error(msg) => Self {
                event_type: "error".into(),
                data: msg,
            },
            // Map all other variants to a generic event
            other => Self {
                event_type: "info".into(),
                data: format!("{other:?}"),
            },
        }
    }
}

/// Model catalog entry.
#[pyclass(name = "ModelCatalog")]
pub struct PyModelCatalog;

#[pymethods]
impl PyModelCatalog {
    #[new]
    fn new() -> Self {
        Self
    }

    /// List all provider IDs.
    fn provider_ids(&self) -> Vec<String> {
        edgecrab_sdk_core::types::SdkModelCatalog::provider_ids()
    }

    /// List model IDs for a given provider. Returns list of (model_id, display_name).
    fn models_for_provider(&self, provider: &str) -> Vec<(String, String)> {
        edgecrab_sdk_core::types::SdkModelCatalog::models_for_provider(provider)
    }

    /// Get the context window size for a model.
    fn context_window(&self, provider: &str, model: &str) -> Option<u64> {
        edgecrab_sdk_core::types::SdkModelCatalog::context_window(provider, model)
    }

    /// Get pricing for a provider/model pair as (input_cost, output_cost) per 1M tokens.
    fn pricing(&self, provider: &str, model: &str) -> Option<(f64, f64)> {
        edgecrab_sdk_core::types::SdkModelCatalog::pricing(provider, model)
            .map(|p| (p.input, p.output))
    }

    /// Flat list of all models: [(provider, model_id, display_name), ...].
    fn flat_catalog(&self) -> Vec<(String, String, String)> {
        edgecrab_sdk_core::types::SdkModelCatalog::flat_catalog()
    }

    /// Get the default model for a provider.
    fn default_model_for(&self, provider: &str) -> Option<String> {
        edgecrab_sdk_core::types::SdkModelCatalog::default_model_for(provider)
    }

    /// Estimate cost (USD) for a given token count. Returns (input_cost, output_cost, total).
    fn estimate_cost(
        &self,
        provider: &str,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Option<(f64, f64, f64)> {
        edgecrab_sdk_core::types::SdkModelCatalog::estimate_cost(
            provider,
            model,
            input_tokens,
            output_tokens,
        )
    }

    fn __repr__(&self) -> String {
        let providers = edgecrab_sdk_core::types::SdkModelCatalog::provider_ids();
        format!("ModelCatalog(providers={})", providers.len())
    }
}

/// Session summary.
#[pyclass(name = "SessionSummary")]
#[derive(Clone)]
pub struct PySessionSummary {
    #[pyo3(get)]
    pub session_id: String,
    #[pyo3(get)]
    pub source: String,
    #[pyo3(get)]
    pub model: Option<String>,
    #[pyo3(get)]
    pub started_at: f64,
    #[pyo3(get)]
    pub message_count: i64,
    #[pyo3(get)]
    pub title: Option<String>,
}

#[pymethods]
impl PySessionSummary {
    fn __repr__(&self) -> String {
        format!(
            "SessionSummary(id='{}', model={:?}, messages={})",
            &self.session_id[..8.min(self.session_id.len())],
            self.model,
            self.message_count
        )
    }
}

impl From<edgecrab_sdk_core::SessionSummary> for PySessionSummary {
    fn from(s: edgecrab_sdk_core::SessionSummary) -> Self {
        Self {
            session_id: s.id,
            source: s.source,
            model: s.model,
            started_at: s.started_at,
            message_count: s.message_count,
            title: s.title,
        }
    }
}

/// Session search result.
#[pyclass(name = "SessionSearchHit")]
#[derive(Clone)]
pub struct PySessionSearchHit {
    #[pyo3(get)]
    pub session_id: String,
    #[pyo3(get)]
    pub role: String,
    #[pyo3(get)]
    pub snippet: String,
    #[pyo3(get)]
    pub score: f64,
}

#[pymethods]
impl PySessionSearchHit {
    fn __repr__(&self) -> String {
        format!(
            "SessionSearchHit(id='{}', score={:.2})",
            &self.session_id[..8.min(self.session_id.len())],
            self.score
        )
    }
}

impl From<edgecrab_sdk_core::SessionSearchHit> for PySessionSearchHit {
    fn from(h: edgecrab_sdk_core::SessionSearchHit) -> Self {
        Self {
            session_id: h.session.id,
            role: h.role,
            snippet: h.snippet,
            score: h.score,
        }
    }
}

/// Standalone session store access — query sessions without creating an Agent.
///
/// Usage:
///     session = Session.open()  # default path
///     sessions = session.list_sessions()
#[pyclass(name = "Session")]
pub struct PySession {
    inner: edgecrab_sdk_core::SdkSession,
}

#[pymethods]
impl PySession {
    /// Open the session database. If path is None, opens the default DB.
    #[new]
    #[pyo3(signature = (path=None))]
    fn new(path: Option<&str>) -> PyResult<Self> {
        let db_path = match path {
            Some(p) => std::path::PathBuf::from(p),
            None => edgecrab_sdk_core::edgecrab_home().join("sessions.db"),
        };
        let inner = edgecrab_sdk_core::SdkSession::open(&db_path).map_err(
            |e: edgecrab_sdk_core::error::SdkError| {
                pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
            },
        )?;
        Ok(Self { inner })
    }

    /// List recent sessions.
    #[pyo3(signature = (limit=20))]
    fn list_sessions(&self, limit: usize) -> PyResult<Vec<PySessionSummary>> {
        let sessions =
            self.inner
                .list_sessions(limit)
                .map_err(|e: edgecrab_sdk_core::error::SdkError| {
                    pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
                })?;
        Ok(sessions.into_iter().map(PySessionSummary::from).collect())
    }

    /// Search sessions by text.
    #[pyo3(signature = (query, limit=10))]
    fn search_sessions(&self, query: &str, limit: usize) -> PyResult<Vec<PySessionSearchHit>> {
        let hits = self.inner.search_sessions(query, limit).map_err(
            |e: edgecrab_sdk_core::error::SdkError| {
                pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
            },
        )?;
        Ok(hits.into_iter().map(PySessionSearchHit::from).collect())
    }

    /// Get all messages for a session as a list of dicts.
    fn get_messages(&self, session_id: &str) -> PyResult<Vec<PyObject>> {
        let messages = self.inner.get_messages(session_id).map_err(
            |e: edgecrab_sdk_core::error::SdkError| {
                pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
            },
        )?;
        Python::with_gil(|py| {
            messages
                .into_iter()
                .map(|m| {
                    let dict = pyo3::types::PyDict::new(py);
                    dict.set_item("role", format!("{:?}", m.role))?;
                    if let Some(ref content) = m.content {
                        match content {
                            edgecrab_sdk_core::Content::Text(text) => {
                                dict.set_item("content", text)?;
                            }
                            edgecrab_sdk_core::Content::Parts(parts) => {
                                let texts: Vec<&str> = parts
                                    .iter()
                                    .filter_map(|p| match p {
                                        edgecrab_sdk_core::ContentPart::Text { text } => {
                                            Some(text.as_str())
                                        }
                                        _ => None,
                                    })
                                    .collect();
                                dict.set_item("content", texts.join(""))?;
                            }
                        }
                    }
                    Ok(dict.into())
                })
                .collect()
        })
    }

    /// Delete a session by ID.
    fn delete_session(&self, session_id: &str) -> PyResult<()> {
        self.inner
            .delete_session(session_id)
            .map_err(|e: edgecrab_sdk_core::error::SdkError| {
                pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
            })
    }

    /// Rename a session (set its title).
    fn rename_session(&self, session_id: &str, title: &str) -> PyResult<()> {
        self.inner.rename_session(session_id, title).map_err(
            |e: edgecrab_sdk_core::error::SdkError| {
                pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
            },
        )
    }

    /// Prune sessions older than the given number of days.
    /// Optionally filter by source. Returns number of deleted sessions.
    #[pyo3(signature = (older_than_days, source=None))]
    fn prune_sessions(&self, older_than_days: u32, source: Option<&str>) -> PyResult<usize> {
        self.inner.prune_sessions(older_than_days, source).map_err(
            |e: edgecrab_sdk_core::error::SdkError| {
                pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
            },
        )
    }

    /// Get aggregate statistics about the session store.
    fn stats(&self) -> PyResult<PySessionStats> {
        let s = self
            .inner
            .stats()
            .map_err(|e: edgecrab_sdk_core::error::SdkError| {
                pyo3::exceptions::PyRuntimeError::new_err(e.to_string())
            })?;
        Ok(PySessionStats {
            total_sessions: s.total_sessions,
            total_messages: s.total_messages,
            db_size_bytes: s.db_size_bytes,
        })
    }

    fn __repr__(&self) -> String {
        "Session(open)".to_string()
    }
}

/// Aggregate statistics about the session store.
#[pyclass(name = "SessionStats", get_all)]
pub struct PySessionStats {
    pub total_sessions: i64,
    pub total_messages: i64,
    pub db_size_bytes: i64,
}

#[pymethods]
impl PySessionStats {
    fn __repr__(&self) -> String {
        format!(
            "SessionStats(sessions={}, messages={}, db_size={})",
            self.total_sessions, self.total_messages, self.db_size_bytes
        )
    }
}
