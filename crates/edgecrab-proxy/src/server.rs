//! Axum server for the OpenAI-compatible proxy.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, OriginalUri, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use edgecrab_core::ProxyConfig;
use serde::Deserialize;
use tracing::info;

use crate::auth::{check_bearer, ensure_proxy_token, validate_bind_address, validate_public_bind};
use crate::backend::adapter::UpstreamAdapter;
use crate::backend::forwarder::{ForwardInbound, build_forwarder_client, forward_request};
use crate::backend::provider::handle_chat_completion;
use crate::cors::{CorsState, cors_middleware};
use crate::error::ProxyError;
use crate::registry::{ensure_forward_upstream_ready, get_forward_adapter};
use crate::resolve::{ResolvedRoute, build_forward_adapters, create_provider, resolve_route};
use crate::wire::openai::{ChatCompletionRequest, ModelObject, ModelsListResponse, unix_now};

#[derive(Clone)]
pub struct ProxyState {
    pub token: String,
    pub config: ProxyConfig,
    pub default_model_spec: Option<String>,
    pub forward_adapters: HashMap<String, Arc<dyn UpstreamAdapter>>,
    pub forward_client: Arc<reqwest::Client>,
    /// Hermes `proxy start --provider`: every allowed `/v1/*` route forwards verbatim.
    pub forward_only: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelPeek {
    model: String,
}

#[derive(Debug, Clone)]
pub struct ProxyRunOptions {
    pub bind: String,
    pub port: u16,
    pub allow_public: bool,
    pub token_path: std::path::PathBuf,
    pub config: ProxyConfig,
    pub default_model_spec: Option<String>,
    /// When set, runs Hermes-style single-upstream forward mode (`--provider`).
    pub forward_only: Option<String>,
}

/// Build the axum router (shared by `run_server` and integration tests).
pub fn build_router(state: ProxyState) -> Router {
    let limit = state.config.max_body_bytes;
    let cors = CorsState::from(&state.config);
    let mut v1 = Router::new()
        .route("/models", get(list_models))
        .route("/chat/completions", post(chat_completions))
        .route("/embeddings", post(embeddings));
    if state.forward_only.is_some() {
        v1 = v1.fallback(v1_forward_catchall);
    }
    Router::new()
        .route("/health", get(health))
        .route("/v1/health", get(health))
        .nest("/v1", v1)
        .layer(DefaultBodyLimit::max(limit))
        .layer(middleware::from_fn_with_state(cors, cors_middleware))
        .with_state(state)
}

/// Run the proxy server until the process is interrupted.
pub async fn run_server(opts: ProxyRunOptions) -> anyhow::Result<()> {
    validate_bind_address(&opts.bind, &opts.token_path)?;
    validate_public_bind(opts.allow_public, &opts.bind, &opts.token_path)?;
    let token = ensure_proxy_token(&opts.token_path)?;

    let forward_adapters = build_forward_adapters(&opts.config.forward_upstreams);
    let preflight_key = opts
        .forward_only
        .as_deref()
        .or(opts.config.default_forward_upstream.as_deref());
    if let Some(key) = preflight_key {
        ensure_forward_upstream_ready(&forward_adapters, key)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
    }
    let forward_client = Arc::new(build_forwarder_client()?);
    let state = ProxyState {
        token,
        config: opts.config,
        default_model_spec: opts.default_model_spec,
        forward_adapters,
        forward_client,
        forward_only: opts.forward_only,
    };

    let app = build_router(state);
    let addr: SocketAddr = format!("{}:{}", opts.bind, opts.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("edgecrab proxy listening on http://{addr}/v1 (OpenAI-compatible inference bridge)");
    axum::serve(listener, app).await?;
    Ok(())
}

fn active_forward_upstream(state: &ProxyState) -> Option<&str> {
    state
        .forward_only
        .as_deref()
        .or(state.config.default_forward_upstream.as_deref())
}

async fn forward_to_upstream(
    state: &ProxyState,
    key: &str,
    method: Method,
    rel_path: &str,
    query: Option<&str>,
    headers: &HeaderMap,
    body: Bytes,
) -> Result<axum::response::Response, ProxyError> {
    let adapter = get_forward_adapter(&state.forward_adapters, key)?;
    forward_request(
        state.forward_client.as_ref(),
        adapter.as_ref(),
        ForwardInbound {
            method,
            rel_path,
            query,
            headers,
            body,
        },
    )
    .await
}

async fn health(State(state): State<ProxyState>) -> impl IntoResponse {
    let forward_ready: Vec<_> = state
        .forward_adapters
        .iter()
        .filter(|(_, a)| a.is_authenticated())
        .map(|(k, a)| (k.clone(), a.display_name().to_string()))
        .collect();
    (
        StatusCode::OK,
        axum::Json(serde_json::json!({
            "status": "ok",
            "service": "edgecrab-proxy",
            "default_forward_upstream": state.config.default_forward_upstream,
            "forward_only": state.forward_only,
            "forward_upstreams_ready": forward_ready,
        })),
    )
}

pub async fn list_models(
    State(state): State<ProxyState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
) -> Result<axum::response::Response, ProxyError> {
    check_bearer(&headers, &state.token)?;

    if let Some(key) = active_forward_upstream(&state) {
        return forward_to_upstream(
            &state,
            key,
            Method::GET,
            "/models",
            uri.query(),
            &headers,
            Bytes::new(),
        )
        .await;
    }

    let now = unix_now();
    let mut data: Vec<ModelObject> = state
        .config
        .model_aliases
        .keys()
        .map(|id| ModelObject {
            id: id.clone(),
            object: "model",
            created: Some(now),
            owned_by: "edgecrab".to_string(),
        })
        .collect();
    if let Some(spec) = state.default_model_spec.as_ref()
        && let Some((_, model)) = spec.split_once('/')
    {
        let id = state
            .config
            .model_aliases
            .iter()
            .find(|(_, v)| *v == spec)
            .map(|(k, _)| k.clone())
            .unwrap_or_else(|| spec.to_string());
        if !data.iter().any(|m| m.id == id) {
            data.push(ModelObject {
                id,
                object: "model",
                created: Some(now),
                owned_by: "edgecrab".to_string(),
            });
        }
        let _ = model;
    }
    if data.is_empty() {
        data.push(ModelObject {
            id: "mock/test".into(),
            object: "model",
            created: Some(now),
            owned_by: "edgecrab".to_string(),
        });
    }
    Ok(axum::Json(ModelsListResponse {
        object: "list",
        data,
    })
    .into_response())
}

pub async fn chat_completions(
    State(state): State<ProxyState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::response::Response, ProxyError> {
    check_bearer(&headers, &state.token)?;
    let query = uri.query();

    if let Some(key) = state.forward_only.as_deref() {
        return forward_to_upstream(
            &state,
            key,
            Method::POST,
            "/chat/completions",
            query,
            &headers,
            body,
        )
        .await;
    }

    let peek: ModelPeek = serde_json::from_slice(&body)
        .map_err(|e| ProxyError::BadRequest(format!("invalid JSON body: {e}")))?;
    if peek.model.trim().is_empty() {
        return Err(ProxyError::BadRequest("model is required".into()));
    }

    let route = resolve_route(
        &peek.model,
        &state.config.model_aliases,
        state.default_model_spec.as_deref(),
        &state.config.forward_upstreams,
    )?;

    match route {
        ResolvedRoute::Forward(fwd) => {
            forward_to_upstream(
                &state,
                &fwd.upstream_key,
                Method::POST,
                "/chat/completions",
                query,
                &headers,
                body,
            )
            .await
        }
        ResolvedRoute::Provider(backend) => {
            let req: ChatCompletionRequest = serde_json::from_slice(&body)
                .map_err(|e| ProxyError::BadRequest(format!("invalid chat body: {e}")))?;
            if req.messages.is_empty() {
                return Err(ProxyError::BadRequest("messages must not be empty".into()));
            }
            let provider = create_provider(&backend)?;
            handle_chat_completion(provider, &backend, req).await
        }
    }
}

pub async fn embeddings(
    State(state): State<ProxyState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::response::Response, ProxyError> {
    check_bearer(&headers, &state.token)?;
    let query = uri.query();

    if let Some(key) = state.forward_only.as_deref() {
        return forward_to_upstream(
            &state,
            key,
            Method::POST,
            "/embeddings",
            query,
            &headers,
            body,
        )
        .await;
    }

    let peek: ModelPeek = serde_json::from_slice(&body)
        .map_err(|e| ProxyError::BadRequest(format!("invalid JSON body: {e}")))?;
    if peek.model.trim().is_empty() {
        return Err(ProxyError::BadRequest("model is required".into()));
    }

    let route = resolve_route(
        &peek.model,
        &state.config.model_aliases,
        state.default_model_spec.as_deref(),
        &state.config.forward_upstreams,
    )?;

    match route {
        ResolvedRoute::Forward(fwd) => {
            forward_to_upstream(
                &state,
                &fwd.upstream_key,
                Method::POST,
                "/embeddings",
                query,
                &headers,
                body,
            )
            .await
        }
        ResolvedRoute::Provider(_) => Err(ProxyError::NotImplemented(
            "embeddings via provider bridge is not implemented; use forward:<upstream> or a \
             provider-native client"
                .into(),
        )),
    }
}

/// Hermes catch-all under `/v1/*` when `--provider` forward-only mode is active.
async fn v1_forward_catchall(
    State(state): State<ProxyState>,
    method: Method,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::response::Response, ProxyError> {
    check_bearer(&headers, &state.token)?;
    let key = state.forward_only.as_deref().ok_or_else(|| {
        ProxyError::BadRequest("forward-only route without forward_only upstream".into())
    })?;
    let path = uri.path();
    let rel = path.strip_prefix("/v1").unwrap_or(path);
    forward_to_upstream(&state, key, method, rel, uri.query(), &headers, body).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, header};
    use tower::ServiceExt;

    fn test_state() -> ProxyState {
        ProxyState {
            token: "test-secret".into(),
            config: ProxyConfig::default(),
            default_model_spec: Some("mock/test-model".into()),
            forward_adapters: HashMap::new(),
            forward_client: Arc::new(build_forwarder_client().expect("client")),
            forward_only: None,
        }
    }

    #[tokio::test]
    async fn rejects_missing_auth() {
        let app = build_router(test_state());
        let body = serde_json::json!({
            "model": "mock/test-model",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn mock_chat_completion_non_stream() {
        let mut cfg = ProxyConfig::default();
        cfg.model_aliases
            .insert("mock-model".into(), "mock/test-model".into());
        let state = ProxyState {
            token: "test-secret".into(),
            config: cfg,
            default_model_spec: None,
            forward_adapters: HashMap::new(),
            forward_client: Arc::new(build_forwarder_client().expect("client")),
            forward_only: None,
        };
        let app = build_router(state);
        let body = serde_json::json!({
            "model": "mock-model",
            "messages": [{"role": "user", "content": "Say OK"}],
            "stream": false
        });
        let req = Request::builder()
            .method("POST")
            .uri("/v1/chat/completions")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer test-secret")
            .body(Body::from(body.to_string()))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn provider_embeddings_returns_501() {
        let app = build_router(test_state());
        let body = serde_json::json!({
            "model": "mock/test-model",
            "input": "hello"
        });
        let req = Request::builder()
            .method("POST")
            .uri("/v1/embeddings")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::AUTHORIZATION, "Bearer test-secret")
            .body(Body::from(body.to_string()))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    }
}
