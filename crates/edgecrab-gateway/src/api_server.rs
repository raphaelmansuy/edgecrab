//! # API Server adapter — OpenAI-compatible HTTP server
//!
//! Exposes OpenAI-compatible endpoints so external tools can interact
//! with the agent using the standard API format.
//!
//! ## Endpoints
//!
//! | Method | Path                     | Description                     |
//! |--------|--------------------------|---------------------------------|
//! | POST   | `/v1/chat/completions`   | Chat completions (streaming OK) |
//! | GET    | `/v1/models`             | List available models           |
//! | POST   | `/v1/responses`          | Create a response (Responses API) |
//! | GET    | `/v1/responses/{id}`     | Get a response by ID            |
//! | DELETE | `/v1/responses/{id}`     | Delete a response               |
//! | GET    | `/v1/health`             | Health check (versioned)        |
//! | GET    | `/health`                | Health check (legacy)           |
//!
//! ## Environment variables
//!
//! | Variable                | Required | Description                              |
//! |-------------------------|----------|------------------------------------------|
//! | `API_SERVER_ENABLED`    | No       | Set to `true` to enable (default: false) |
//! | `API_SERVER_PORT`       | No       | Port (default: 8642)                     |
//! | `API_SERVER_HOST`       | No       | Bind address (default: 127.0.0.1)        |
//! | `API_SERVER_KEY`        | No       | Bearer token for authentication          |
//! | `API_SERVER_CORS_ORIGINS`| No      | Comma-separated allowed origins          |
//!
//! ## Limits
//!
//! - Max message length: **100000** characters

use std::convert::Infallible;
use std::env;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::Router;
use axum::extract::{Json, Path, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use edgecrab_types::Platform;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, mpsc, oneshot};
use tracing::{debug, error, info, warn};

use crate::platform::{IncomingMessage, MessageMetadata, OutgoingMessage, PlatformAdapter};

const MAX_MESSAGE_LENGTH: usize = 100_000;
const DEFAULT_PORT: u16 = 8642;
const DEFAULT_HOST: &str = "127.0.0.1";

pub struct ApiServerAdapter {
    host: String,
    port: u16,
    api_key: Option<String>,
    cors_origins: Vec<String>,
}

impl ApiServerAdapter {
    pub fn from_env() -> Option<Self> {
        if !Self::is_available() {
            return None;
        }
        let host = env::var("API_SERVER_HOST").unwrap_or_else(|_| DEFAULT_HOST.to_string());
        let port: u16 = env::var("API_SERVER_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_PORT);
        let api_key = env::var("API_SERVER_KEY").ok();
        let cors_origins = env::var("API_SERVER_CORS_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.trim().to_string())
            .collect();

        Some(Self {
            host,
            port,
            api_key,
            cors_origins,
        })
    }

    pub fn is_available() -> bool {
        env::var("API_SERVER_ENABLED")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false)
    }
}

type ResponseMap = Arc<dashmap::DashMap<String, oneshot::Sender<String>>>;

/// Stored response for the /v1/responses API.
#[derive(Debug, Clone, Serialize)]
struct StoredResponse {
    id: String,
    object: String,
    created: u64,
    output: String,
}

type ResponseStore = Arc<dashmap::DashMap<String, StoredResponse>>;

#[derive(Clone)]
struct AppState {
    tx: mpsc::Sender<IncomingMessage>,
    response_map: ResponseMap,
    responses: ResponseStore,
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatRequest {
    messages: Vec<ChatMessage>,
    #[allow(dead_code)]
    model: Option<String>,
    #[allow(dead_code)]
    stream: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<ChatChoice>,
    usage: ChatUsage,
}

#[derive(Debug, Serialize)]
struct ChatChoice {
    index: u32,
    message: ChatMessage,
    finish_reason: String,
}

#[derive(Debug, Serialize)]
struct ChatUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

// ── SSE streaming types ──────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct StreamChunk {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Serialize)]
struct StreamChoice {
    index: u32,
    delta: StreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct StreamDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

// ── /v1/models types ─────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ModelsResponse {
    object: String,
    data: Vec<ModelEntry>,
}

#[derive(Debug, Serialize)]
struct ModelEntry {
    id: String,
    object: String,
    created: u64,
    owned_by: String,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Auth helper ──────────────────────────────────────────────────────

fn check_auth(state: &AppState, headers: &HeaderMap) -> Result<(), StatusCode> {
    if let Some(ref expected) = state.api_key {
        let auth = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let provided = auth.strip_prefix("Bearer ").unwrap_or(auth);
        if provided != expected {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }
    Ok(())
}

// ── GET /health  /v1/health ──────────────────────────────────────────

async fn health() -> impl IntoResponse {
    let body = serde_json::json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") });
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )],
        Json(body),
    )
}

// ── POST /v1/responses ───────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CreateResponseRequest {
    input: String,
    #[allow(dead_code)]
    model: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateResponseResponse {
    id: String,
    object: String,
    created: u64,
    status: String,
}

async fn create_response(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateResponseRequest>,
) -> Result<(StatusCode, Json<CreateResponseResponse>), StatusCode> {
    check_auth(&state, &headers)?;

    if req.input.is_empty() || req.input.len() > MAX_MESSAGE_LENGTH {
        return Err(StatusCode::BAD_REQUEST);
    }

    let id = uuid::Uuid::new_v4().to_string();
    let now = unix_now();
    let (resp_tx, resp_rx) = oneshot::channel::<String>();
    state.response_map.insert(id.clone(), resp_tx);

    let incoming = IncomingMessage {
        platform: Platform::Api,
        user_id: "api".to_string(),
        channel_id: Some(id.clone()),
        text: req.input,
        thread_id: None,
        metadata: MessageMetadata {
            message_id: Some(id.clone()),
            channel_id: Some(id.clone()),
            thread_id: None,
            user_display_name: Some("API Client".to_string()),
            attachments: Vec::new(),
            ..Default::default()
        },
    };

    if state.tx.send(incoming).await.is_err() {
        error!("API server: message channel closed");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    // Await agent response (max 5 min), then store it.
    let store = state.responses.clone();
    let stored_id = id.clone();
    tokio::spawn(async move {
        if let Ok(Ok(text)) = tokio::time::timeout(Duration::from_secs(300), resp_rx).await {
            store.insert(
                stored_id.clone(),
                StoredResponse {
                    id: stored_id,
                    object: "response".into(),
                    created: now,
                    output: text,
                },
            );
        }
    });

    Ok((
        StatusCode::CREATED,
        Json(CreateResponseResponse {
            id,
            object: "response".into(),
            created: now,
            status: "in_progress".into(),
        }),
    ))
}

// ── GET /v1/responses/{id} ────────────────────────────────────────────

async fn get_response(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<StoredResponse>, StatusCode> {
    check_auth(&state, &headers)?;
    state
        .responses
        .get(&id)
        .map(|r| Json(r.clone()))
        .ok_or(StatusCode::NOT_FOUND)
}

// ── DELETE /v1/responses/{id} ─────────────────────────────────────────

async fn delete_response(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    check_auth(&state, &headers)?;
    if state.responses.remove(&id).is_some() {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

// ── GET /v1/models ───────────────────────────────────────────────────

async fn list_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ModelsResponse>, StatusCode> {
    check_auth(&state, &headers)?;

    let now = unix_now();

    Ok(Json(ModelsResponse {
        object: "list".into(),
        data: vec![ModelEntry {
            id: "edgecrab".into(),
            object: "model".into(),
            created: now,
            owned_by: "edgecrab".into(),
        }],
    }))
}

// ── POST /v1/chat/completions ────────────────────────────────────────

async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<ChatRequest>,
) -> Result<axum::response::Response, StatusCode> {
    check_auth(&state, &headers)?;

    // Extract the last user message
    let user_msg = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();

    if user_msg.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let streaming = req.stream.unwrap_or(false);
    let request_id = uuid::Uuid::new_v4().to_string();

    if streaming {
        // SSE streaming mode
        let (resp_tx, resp_rx) = oneshot::channel::<String>();
        state.response_map.insert(request_id.clone(), resp_tx);

        let incoming = IncomingMessage {
            platform: Platform::Api,
            user_id: "api".to_string(),
            channel_id: Some(request_id.clone()),
            text: user_msg,
            thread_id: None,
            metadata: MessageMetadata {
                message_id: Some(request_id.clone()),
                channel_id: Some(request_id.clone()),
                thread_id: None,
                user_display_name: Some("API Client".to_string()),
                attachments: Vec::new(),
                ..Default::default()
            },
        };

        if state.tx.send(incoming).await.is_err() {
            error!("API server: message channel closed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }

        let rid = request_id.clone();
        let stream = make_sse_stream(rid, resp_rx);
        let sse = Sse::new(stream).keep_alive(KeepAlive::default());
        Ok(sse.into_response())
    } else {
        // Non-streaming mode (original behaviour)
        let (resp_tx, resp_rx) = oneshot::channel::<String>();
        state.response_map.insert(request_id.clone(), resp_tx);

        let incoming = IncomingMessage {
            platform: Platform::Api,
            user_id: "api".to_string(),
            channel_id: Some(request_id.clone()),
            text: user_msg,
            thread_id: None,
            metadata: MessageMetadata {
                message_id: Some(request_id.clone()),
                channel_id: Some(request_id.clone()),
                thread_id: None,
                user_display_name: Some("API Client".to_string()),
                attachments: Vec::new(),
                ..Default::default()
            },
        };

        if state.tx.send(incoming).await.is_err() {
            error!("API server: message channel closed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }

        // Wait for response with timeout
        let response_text = match tokio::time::timeout(Duration::from_secs(300), resp_rx).await {
            Ok(Ok(text)) => text,
            Ok(Err(_)) => {
                state.response_map.remove(&request_id);
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }
            Err(_) => {
                state.response_map.remove(&request_id);
                return Err(StatusCode::GATEWAY_TIMEOUT);
            }
        };

        let now = unix_now();
        let request_id_preview = edgecrab_core::safe_truncate(&request_id, 8);

        let resp = ChatResponse {
            id: format!("chatcmpl-{request_id_preview}"),
            object: "chat.completion".into(),
            created: now,
            model: "edgecrab".into(),
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: response_text,
                },
                finish_reason: "stop".into(),
            }],
            usage: ChatUsage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        };

        Ok(Json(resp).into_response())
    }
}

/// Build an SSE stream that waits for the full agent response, then
/// emits it as a series of token-sized chunks (simulating streaming)
/// followed by a `[DONE]` sentinel.
fn make_sse_stream(
    request_id: String,
    resp_rx: oneshot::Receiver<String>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let chat_id = format!("chatcmpl-{}", edgecrab_core::safe_truncate(&request_id, 8));

        // Role chunk
        yield Ok(Event::default().data(serde_json::to_string(&StreamChunk {
            id: chat_id.clone(),
            object: "chat.completion.chunk".into(),
            created: now,
            model: "edgecrab".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: Some("assistant".into()),
                    content: None,
                },
                finish_reason: None,
            }],
        }).unwrap_or_default()));

        // Wait for the full response from the agent
        match tokio::time::timeout(Duration::from_secs(300), resp_rx).await {
            Ok(Ok(text)) => {
                // Emit content in word-boundary chunks for a natural streaming feel
                for chunk in text.split_inclusive(|c: char| c.is_whitespace() || c == '\n') {
                    yield Ok(Event::default().data(serde_json::to_string(&StreamChunk {
                        id: chat_id.clone(),
                        object: "chat.completion.chunk".into(),
                        created: now,
                        model: "edgecrab".into(),
                        choices: vec![StreamChoice {
                            index: 0,
                            delta: StreamDelta {
                                role: None,
                                content: Some(chunk.to_string()),
                            },
                            finish_reason: None,
                        }],
                    }).unwrap_or_default()));
                }
            }
            _ => {
                // Timeout or error — emit empty stop
            }
        }

        // Final stop chunk
        yield Ok(Event::default().data(serde_json::to_string(&StreamChunk {
            id: chat_id.clone(),
            object: "chat.completion.chunk".into(),
            created: now,
            model: "edgecrab".into(),
            choices: vec![StreamChoice {
                index: 0,
                delta: StreamDelta {
                    role: None,
                    content: None,
                },
                finish_reason: Some("stop".into()),
            }],
        }).unwrap_or_default()));

        // [DONE] sentinel
        yield Ok(Event::default().data("[DONE]"));
    }
}

#[async_trait]
impl PlatformAdapter for ApiServerAdapter {
    fn platform(&self) -> Platform {
        Platform::Api
    }

    async fn start(&self, tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        info!("API Server adapter starting on {}:{}", self.host, self.port);

        let response_map: ResponseMap = Arc::new(dashmap::DashMap::new());
        let responses: ResponseStore = Arc::new(dashmap::DashMap::new());

        {
            let mut guard = RESPONSE_MAP.lock().await;
            *guard = Some(response_map.clone());
        }

        let state = AppState {
            tx,
            response_map,
            responses,
            api_key: self.api_key.clone(),
        };

        let cors_origins = Arc::new(self.cors_origins.clone());

        let app = Router::new()
            .route("/v1/chat/completions", post(chat_completions))
            .route("/v1/models", get(list_models))
            .route("/v1/responses", post(create_response))
            .route(
                "/v1/responses/{id}",
                get(get_response).delete(delete_response),
            )
            .route("/v1/health", get(health))
            .route("/health", get(health))
            .with_state(state)
            .layer(axum::middleware::from_fn(security_headers_middleware))
            .layer(axum::middleware::from_fn(move |req, next| {
                let origins = cors_origins.clone();
                cors_middleware(req, next, origins)
            }));

        let addr = format!("{}:{}", self.host, self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!(
            "API Server listening on http://{} (OpenAI-compatible)",
            addr
        );
        axum::serve(listener, app).await?;
        Ok(())
    }

    async fn send(&self, msg: OutgoingMessage) -> anyhow::Result<()> {
        let channel_id = msg
            .metadata
            .channel_id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("No API request_id"))?;

        let guard = RESPONSE_MAP.lock().await;
        if let Some(ref map) = *guard {
            if let Some((_, tx)) = map.remove(channel_id) {
                let _ = tx.send(msg.text);
                debug!("API response sent for {}", channel_id);
            } else {
                warn!("No pending API request for {}", channel_id);
            }
        }

        Ok(())
    }

    fn format_response(&self, text: &str, _metadata: &MessageMetadata) -> String {
        text.to_string()
    }

    fn max_message_length(&self) -> usize {
        MAX_MESSAGE_LENGTH
    }

    fn supports_markdown(&self) -> bool {
        true
    }

    fn supports_images(&self) -> bool {
        false
    }

    fn supports_files(&self) -> bool {
        false
    }
}

// ── Security headers middleware ───────────────────────────────────────

async fn security_headers_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    response
}

// ── CORS middleware ───────────────────────────────────────────────────

async fn cors_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
    allowed_origins: Arc<Vec<String>>,
) -> axum::response::Response {
    let origin = request
        .headers()
        .get(axum::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let mut response = next.run(request).await;

    // SECURITY: when no CORS origins are configured, no CORS headers are added at all.
    // Only add CORS headers when the request origin is in the explicit allowlist.
    let allow_origin: Option<HeaderValue> = if allowed_origins.is_empty() {
        None
    } else if let Some(ref o) = origin {
        if allowed_origins.iter().any(|a| a == o) {
            o.parse().ok()
        } else {
            None
        }
    } else {
        None
    };

    if let Some(origin_value) = allow_origin {
        let headers = response.headers_mut();
        headers.insert(
            axum::http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
            origin_value,
        );
        headers.insert(
            HeaderName::from_static("access-control-allow-methods"),
            HeaderValue::from_static("GET, POST, DELETE, OPTIONS"),
        );
        headers.insert(
            HeaderName::from_static("access-control-allow-headers"),
            HeaderValue::from_static("Content-Type, Authorization, Idempotency-Key"),
        );
        // 10-minute preflight cache per spec
        headers.insert(
            HeaderName::from_static("access-control-max-age"),
            HeaderValue::from_static("600"),
        );
    }

    response
}

static RESPONSE_MAP: std::sync::LazyLock<Mutex<Option<ResponseMap>>> =
    std::sync::LazyLock::new(|| Mutex::new(None));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_max_length() {
        assert_eq!(MAX_MESSAGE_LENGTH, 100_000);
    }
}
