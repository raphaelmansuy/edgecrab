//! Node.js type bindings for SDK types.

/// Result of a full conversation run.
#[napi(object)]
#[derive(Clone)]
pub struct JsConversationResult {
    /// The final assistant response text.
    pub response: String,
    /// Session ID used for this conversation.
    pub session_id: String,
    /// Number of API calls made.
    pub api_calls: u32,
    /// Whether the agent was interrupted.
    pub interrupted: bool,
    /// Whether the iteration budget was exhausted.
    pub budget_exhausted: bool,
    /// Model used.
    pub model: String,
    /// Total input tokens.
    pub input_tokens: i64,
    /// Total output tokens.
    pub output_tokens: i64,
    /// Total cost in USD.
    pub total_cost: f64,
}

impl From<edgecrab_sdk_core::ConversationResult> for JsConversationResult {
    fn from(r: edgecrab_sdk_core::ConversationResult) -> Self {
        Self {
            response: r.final_response,
            session_id: r.session_id,
            api_calls: r.api_calls,
            interrupted: r.interrupted,
            budget_exhausted: r.budget_exhausted,
            model: r.model,
            input_tokens: r.usage.input_tokens as i64,
            output_tokens: r.usage.output_tokens as i64,
            total_cost: r.cost.total_cost,
        }
    }
}

/// A streaming event from the agent.
#[napi(object)]
#[derive(Clone)]
pub struct JsStreamEvent {
    /// Event type: "token", "reasoning", "tool_exec", "tool_result", "done", "error", "info"
    pub event_type: String,
    /// Event data (text content, tool name, error message, etc.)
    pub data: String,
}

impl From<edgecrab_sdk_core::StreamEvent> for JsStreamEvent {
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
            other => Self {
                event_type: "info".into(),
                data: format!("{other:?}"),
            },
        }
    }
}

/// Session summary.
#[napi(object)]
#[derive(Clone)]
pub struct JsSessionSummary {
    /// Unique session identifier.
    pub session_id: String,
    /// Session source.
    pub source: String,
    /// Model used (if known).
    pub model: Option<String>,
    /// Start timestamp (Unix seconds).
    pub started_at: f64,
    /// Number of messages.
    pub message_count: i64,
    /// Session title (if set).
    pub title: Option<String>,
}

impl From<edgecrab_sdk_core::SessionSummary> for JsSessionSummary {
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
#[napi(object)]
#[derive(Clone)]
pub struct JsSessionSearchHit {
    /// Session ID where the match was found.
    pub session_id: String,
    /// Role of the matching message.
    pub role: String,
    /// Matching text snippet.
    pub snippet: String,
    /// Relevance score.
    pub score: f64,
}

impl From<edgecrab_sdk_core::SessionSearchHit> for JsSessionSearchHit {
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
#[napi(js_name = "Session")]
pub struct JsSession {
    inner: edgecrab_sdk_core::SdkSession,
}

#[napi]
impl JsSession {
    /// Open the session database. If path is not given, opens the default DB.
    #[napi(constructor)]
    pub fn new(path: Option<String>) -> napi::Result<Self> {
        let db_path = match path {
            Some(p) => std::path::PathBuf::from(p),
            None => edgecrab_sdk_core::edgecrab_home().join("sessions.db"),
        };
        let inner = edgecrab_sdk_core::SdkSession::open(&db_path)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(Self { inner })
    }

    /// List recent sessions.
    #[napi]
    pub fn list_sessions(&self, limit: Option<u32>) -> napi::Result<Vec<JsSessionSummary>> {
        let lim = limit.unwrap_or(20) as usize;
        let sessions = self
            .inner
            .list_sessions(lim)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(sessions.into_iter().map(JsSessionSummary::from).collect())
    }

    /// Search sessions by text.
    #[napi]
    pub fn search_sessions(
        &self,
        query: String,
        limit: Option<u32>,
    ) -> napi::Result<Vec<JsSessionSearchHit>> {
        let lim = limit.unwrap_or(10) as usize;
        let hits = self
            .inner
            .search_sessions(&query, lim)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(hits.into_iter().map(JsSessionSearchHit::from).collect())
    }

    /// Get all messages for a session.
    #[napi]
    pub fn get_messages(&self, session_id: String) -> napi::Result<Vec<serde_json::Value>> {
        let messages = self
            .inner
            .get_messages(&session_id)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(messages
            .into_iter()
            .map(|m| {
                let mut obj = serde_json::Map::new();
                obj.insert("role".into(), serde_json::json!(format!("{:?}", m.role)));
                if let Some(ref content) = m.content {
                    match content {
                        edgecrab_sdk_core::Content::Text(text) => {
                            obj.insert("content".into(), serde_json::json!(text));
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
                            obj.insert("content".into(), serde_json::json!(texts.join("")));
                        }
                    }
                }
                serde_json::Value::Object(obj)
            })
            .collect())
    }

    /// Delete a session by ID.
    #[napi]
    pub fn delete_session(&self, session_id: String) -> napi::Result<()> {
        self.inner
            .delete_session(&session_id)
            .map_err(|e| napi::Error::from_reason(e.to_string()))
    }

    /// Rename a session (set its title).
    #[napi]
    pub fn rename_session(&self, session_id: String, title: String) -> napi::Result<()> {
        self.inner
            .rename_session(&session_id, &title)
            .map_err(|e| napi::Error::from_reason(e.to_string()))
    }

    /// Prune sessions older than the given number of days.
    /// Optionally filter by source. Returns number of deleted sessions.
    #[napi]
    pub fn prune_sessions(
        &self,
        older_than_days: u32,
        source: Option<String>,
    ) -> napi::Result<u32> {
        self.inner
            .prune_sessions(older_than_days, source.as_deref())
            .map(|n| n as u32)
            .map_err(|e| napi::Error::from_reason(e.to_string()))
    }

    /// Get aggregate statistics about the session store.
    /// Returns { total_sessions, total_messages, db_size_bytes }.
    #[napi]
    pub fn stats(&self) -> napi::Result<JsSessionStats> {
        let s = self
            .inner
            .stats()
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;
        Ok(JsSessionStats {
            total_sessions: s.total_sessions,
            total_messages: s.total_messages,
            db_size_bytes: s.db_size_bytes,
        })
    }
}

/// Session statistics.
#[napi(object)]
#[derive(Clone)]
pub struct JsSessionStats {
    /// Total number of sessions.
    pub total_sessions: i64,
    /// Total number of messages across all sessions.
    pub total_messages: i64,
    /// Database size in bytes.
    pub db_size_bytes: i64,
}

/// Model catalog — query available models and pricing.
#[napi(js_name = "ModelCatalog")]
pub struct JsModelCatalog;

#[napi]
impl JsModelCatalog {
    /// Create a new ModelCatalog instance.
    #[napi(constructor)]
    pub fn new() -> Self {
        Self
    }

    /// List all provider IDs.
    #[napi]
    pub fn provider_ids(&self) -> Vec<String> {
        edgecrab_sdk_core::types::SdkModelCatalog::provider_ids()
    }

    /// List model IDs for a given provider. Returns array of [model_id, display_name].
    #[napi]
    pub fn models_for_provider(&self, provider: String) -> Vec<Vec<String>> {
        edgecrab_sdk_core::types::SdkModelCatalog::models_for_provider(&provider)
            .into_iter()
            .map(|(id, name)| vec![id, name])
            .collect()
    }

    /// Get the context window size for a model.
    #[napi]
    pub fn context_window(&self, provider: String, model: String) -> Option<i64> {
        edgecrab_sdk_core::types::SdkModelCatalog::context_window(&provider, &model)
            .map(|w| w as i64)
    }

    /// Get pricing for a provider/model pair. Returns [input_cost, output_cost] per 1M tokens.
    #[napi]
    pub fn pricing(&self, provider: String, model: String) -> Option<Vec<f64>> {
        edgecrab_sdk_core::types::SdkModelCatalog::pricing(&provider, &model)
            .map(|p| vec![p.input, p.output])
    }

    /// Flat list of all models: [[provider, model_id, display_name], ...].
    #[napi]
    pub fn flat_catalog(&self) -> Vec<Vec<String>> {
        edgecrab_sdk_core::types::SdkModelCatalog::flat_catalog()
            .into_iter()
            .map(|(p, m, d)| vec![p, m, d])
            .collect()
    }

    /// Get the default model for a provider.
    #[napi]
    pub fn default_model_for(&self, provider: String) -> Option<String> {
        edgecrab_sdk_core::types::SdkModelCatalog::default_model_for(&provider)
    }

    /// Estimate cost (USD) for a given token count.
    /// Returns [input_cost, output_cost, total_cost] or null.
    #[napi]
    pub fn estimate_cost(
        &self,
        provider: String,
        model: String,
        input_tokens: i64,
        output_tokens: i64,
    ) -> Option<Vec<f64>> {
        edgecrab_sdk_core::types::SdkModelCatalog::estimate_cost(
            &provider,
            &model,
            input_tokens as u64,
            output_tokens as u64,
        )
        .map(|(ic, oc, total)| vec![ic, oc, total])
    }
}
