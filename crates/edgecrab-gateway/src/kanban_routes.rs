//! Kanban HTTP + WebSocket routes — Hermes dashboard API subset.

use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use edgecrab_core::{
    check_kanban_token, decompose_outcome_json, decompose_task_by_id, describe_outcome_json,
    describe_profile, edgecrab_home, ensure_kanban_api_token, get_orchestration_settings,
    install_root, kanban_api, load_kanban_api_token, patch_orchestration_settings,
    profiles_api_json, write_profile_description, AppConfig, OrchestrationSettingsPatch, TaskPatch,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::run::GatewayState;

/// Hermes-compatible poll interval for the event tail loop.
const EVENT_POLL_MS: u64 = 300;

#[derive(Debug, Deserialize, Default)]
pub struct BoardQuery {
    pub board: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct EventsQuery {
    pub board: Option<String>,
    #[serde(default)]
    pub since: Option<i64>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub token: Option<String>,
}

fn kanban_disabled() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "kanban disabled" })),
    )
}

fn api_err(e: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": e.to_string() })),
    )
}

fn not_found(msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": msg.into() })),
    )
}

fn unauthorized(msg: &'static str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": msg })),
    )
}

fn kanban_cfg() -> AppConfig {
    AppConfig::load().unwrap_or_default()
}

fn kanban_enabled(cfg: &AppConfig) -> bool {
    cfg.kanban.enabled
}

fn events_params(params: &EventsQuery) -> (Option<String>, i64, usize) {
    let since = params.since.unwrap_or(0).max(0);
    let limit = params.limit.unwrap_or(200).clamp(1, 500);
    (params.board.clone(), since, limit)
}

fn bearer_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .map(str::to_string)
}

fn header_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("X-Kanban-Token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .map(str::to_string)
}

fn auth_or_err(
    state: &GatewayState,
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<(), (StatusCode, Json<Value>)> {
    let cfg = kanban_cfg();
    let bearer_owned = bearer_from_headers(headers);
    let hdr_owned = header_token(headers);
    let bearer = bearer_owned.as_deref();
    let hdr = hdr_owned.as_deref();
    check_kanban_token(
        &cfg.kanban,
        &state.gateway_host,
        bearer,
        hdr,
        query_token,
    )
    .map_err(unauthorized)
}

/// `GET /kanban` — dashboard HTML with embedded API token when configured.
pub async fn kanban_dashboard(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Query(params): Query<BoardQuery>,
) -> Result<axum::response::Html<String>, (StatusCode, Json<Value>)> {
    let cfg = kanban_cfg();
    if !kanban_enabled(&cfg) {
        return Err(kanban_disabled());
    }
    auth_or_err(&state, &headers, params.token.as_deref())?;

    let token_json = load_kanban_api_token(&cfg.kanban)
        .ok()
        .flatten()
        .map(|t| serde_json::to_string(&t).unwrap_or_else(|_| "null".into()))
        .unwrap_or_else(|| "null".into());

    let html = include_str!("kanban_dashboard.html").replace("__KANBAN_TOKEN__", &token_json);
    Ok(axum::response::Html(html))
}

/// `GET /api/kanban/board?board=<slug>`
pub async fn kanban_board(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Query(params): Query<BoardQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let cfg = kanban_cfg();
    if !kanban_enabled(&cfg) {
        return Err(kanban_disabled());
    }
    auth_or_err(&state, &headers, params.token.as_deref())?;
    kanban_api::board_snapshot(Some(&edgecrab_home()), params.board.as_deref())
        .map(Json)
        .map_err(api_err)
}

/// `GET /api/kanban/boards`
pub async fn kanban_boards(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Query(params): Query<BoardQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let cfg = kanban_cfg();
    if !kanban_enabled(&cfg) {
        return Err(kanban_disabled());
    }
    auth_or_err(&state, &headers, params.token.as_deref())?;
    kanban_api::boards_list(Some(&edgecrab_home()))
        .map(Json)
        .map_err(api_err)
}

/// `GET /api/kanban/tasks/:id?board=<slug>`
pub async fn kanban_task_detail(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Query(params): Query<BoardQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let cfg = kanban_cfg();
    if !kanban_enabled(&cfg) {
        return Err(kanban_disabled());
    }
    auth_or_err(&state, &headers, params.token.as_deref())?;
    match kanban_api::task_detail(
        Some(&edgecrab_home()),
        params.board.as_deref(),
        &task_id,
    ) {
        Ok(v) => Ok(Json(v)),
        Err(edgecrab_types::AgentError::Validation(msg)) => Err(not_found(msg)),
        Err(e) => Err(api_err(e)),
    }
}

/// `POST /api/kanban/tasks/:id/decompose` — Hermes dashboard ⚗ button.
pub async fn kanban_decompose_task(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Query(params): Query<BoardQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let cfg = kanban_cfg();
    if !kanban_enabled(&cfg) {
        return Err(kanban_disabled());
    }
    auth_or_err(&state, &headers, params.token.as_deref())?;

    let Some(agent) = state.agent.clone() else {
        return Err(api_err("kanban decompose requires a running gateway agent"));
    };

    let provider = agent.provider_handle().await;
    let model = agent.model().await;
    let outcome = decompose_task_by_id(
        Some(&edgecrab_home()),
        &task_id,
        provider,
        &model,
        &cfg,
    )
    .await;
    Ok(Json(decompose_outcome_json(&outcome)))
}

/// `PATCH /api/kanban/tasks/:id` — status / assignee / priority / title / body.
pub async fn kanban_task_patch(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Query(params): Query<BoardQuery>,
    Json(body): Json<TaskPatch>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let cfg = kanban_cfg();
    if !kanban_enabled(&cfg) {
        return Err(kanban_disabled());
    }
    auth_or_err(&state, &headers, params.token.as_deref())?;
    kanban_api::patch_task(
        Some(&edgecrab_home()),
        params.board.as_deref(),
        &task_id,
        &body,
        &install_root(),
    )
    .map(Json)
    .map_err(validation_err)
}

/// `DELETE /api/kanban/tasks/:id` — hard-delete task + cascaded rows.
pub async fn kanban_task_delete(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(task_id): Path<String>,
    Query(params): Query<BoardQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let cfg = kanban_cfg();
    if !kanban_enabled(&cfg) {
        return Err(kanban_disabled());
    }
    auth_or_err(&state, &headers, params.token.as_deref())?;
    match kanban_api::delete_task(
        Some(&edgecrab_home()),
        params.board.as_deref(),
        &task_id,
    ) {
        Ok(v) => Ok(Json(v)),
        Err(edgecrab_types::AgentError::Validation(msg)) => Err(not_found(msg)),
        Err(e) => Err(api_err(e)),
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct DescribeAutoBody {
    #[serde(default)]
    pub overwrite: bool,
}

/// `POST /api/kanban/profiles/:name/describe-auto`
pub async fn kanban_profile_describe_auto(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(profile_name): Path<String>,
    Query(params): Query<BoardQuery>,
    Json(body): Json<DescribeAutoBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let cfg = kanban_cfg();
    if !kanban_enabled(&cfg) {
        return Err(kanban_disabled());
    }
    auth_or_err(&state, &headers, params.token.as_deref())?;

    let Some(agent) = state.agent.clone() else {
        return Err(api_err("profile describer requires a running gateway agent"));
    };

    let provider = agent.provider_handle().await;
    let model = agent.model().await;
    let outcome = describe_profile(&profile_name, body.overwrite, provider, &model, &cfg).await;
    Ok(Json(describe_outcome_json(&outcome)))
}

/// `GET /api/kanban/events?since=<id>&board=<slug>&limit=<n>`
pub async fn kanban_events_poll(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Query(params): Query<EventsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let cfg = kanban_cfg();
    if !kanban_enabled(&cfg) {
        return Err(kanban_disabled());
    }
    auth_or_err(&state, &headers, params.token.as_deref())?;
    let (board, since, limit) = events_params(&params);
    kanban_api::events_since(
        Some(&edgecrab_home()),
        board.as_deref(),
        since,
        limit,
    )
    .map(Json)
    .map_err(api_err)
}

/// `GET /api/kanban/events/ws?since=<id>&board=<slug>&token=<token>`
pub async fn kanban_events_ws(
    State(state): State<GatewayState>,
    Query(params): Query<EventsQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let cfg = kanban_cfg();
    if !kanban_enabled(&cfg) {
        return kanban_disabled().into_response();
    }
    if auth_or_err(&state, &headers, params.token.as_deref()).is_err() {
        return unauthorized("missing kanban API token").into_response();
    }
    let (board, since, limit) = events_params(&params);
    ws.on_upgrade(move |socket| kanban_events_stream(socket, board, since, limit))
}

async fn kanban_events_stream(
    mut socket: WebSocket,
    board: Option<String>,
    mut cursor: i64,
    limit: usize,
) {
    let home = edgecrab_home();
    loop {
        let board = board.clone();
        let home = home.clone();
        let fetch = tokio::task::spawn_blocking(move || {
            kanban_api::events_since(Some(&home), board.as_deref(), cursor, limit)
        })
        .await;

        match fetch {
            Ok(Ok(body)) => {
                if let Some(new_cursor) = body.get("cursor").and_then(|v| v.as_i64()) {
                    cursor = new_cursor;
                }
                let has_events = body
                    .get("events")
                    .and_then(|v| v.as_array())
                    .is_some_and(|a| !a.is_empty());
                if has_events {
                    let text = match serde_json::to_string(&body) {
                        Ok(t) => t,
                        Err(_) => break,
                    };
                    if socket.send(Message::Text(text)).await.is_err() {
                        break;
                    }
                }
            }
            Ok(Err(e)) => {
                let err = json!({ "error": e.to_string() });
                if socket
                    .send(Message::Text(err.to_string()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Err(_) => break,
        }

        tokio::time::sleep(Duration::from_millis(EVENT_POLL_MS)).await;
    }
}

fn validation_err(e: edgecrab_types::AgentError) -> (StatusCode, Json<Value>) {
    match e {
        edgecrab_types::AgentError::Validation(msg) => {
            if let Some(body) = edgecrab_core::parse_conflict(&msg) {
                return (StatusCode::CONFLICT, Json(body));
            }
            (StatusCode::BAD_REQUEST, Json(json!({ "error": msg })))
        }
        other => api_err(other),
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct ProfileDescriptionBody {
    pub description: Option<String>,
}

/// `GET /api/kanban/orchestration`
pub async fn kanban_orchestration_get(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Query(params): Query<BoardQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let cfg = kanban_cfg();
    if !kanban_enabled(&cfg) {
        return Err(kanban_disabled());
    }
    auth_or_err(&state, &headers, params.token.as_deref())?;
    Ok(Json(get_orchestration_settings(&cfg)))
}

/// `PUT /api/kanban/orchestration`
pub async fn kanban_orchestration_put(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Query(params): Query<BoardQuery>,
    Json(body): Json<OrchestrationSettingsPatch>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let cfg = kanban_cfg();
    if !kanban_enabled(&cfg) {
        return Err(kanban_disabled());
    }
    auth_or_err(&state, &headers, params.token.as_deref())?;
    patch_orchestration_settings(body)
        .map(Json)
        .map_err(validation_err)
}

/// `GET /api/kanban/profiles`
pub async fn kanban_profiles_list(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Query(params): Query<BoardQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let cfg = kanban_cfg();
    if !kanban_enabled(&cfg) {
        return Err(kanban_disabled());
    }
    auth_or_err(&state, &headers, params.token.as_deref())?;
    Ok(Json(profiles_api_json(&install_root())))
}

/// `PATCH /api/kanban/profiles/:name`
pub async fn kanban_profile_patch(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    Path(profile_name): Path<String>,
    Query(params): Query<BoardQuery>,
    Json(body): Json<ProfileDescriptionBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let cfg = kanban_cfg();
    if !kanban_enabled(&cfg) {
        return Err(kanban_disabled());
    }
    auth_or_err(&state, &headers, params.token.as_deref())?;
    let text = body.description.unwrap_or_default();
    let saved = write_profile_description(&install_root(), &profile_name, &text)
        .map_err(validation_err)?;
    Ok(Json(json!({
        "ok": true,
        "profile": profile_name,
        "description": saved,
    })))
}

/// Log token path on gateway startup when kanban is enabled.
pub fn init_kanban_api_auth(cfg: &AppConfig) {
    if !cfg.kanban.enabled || !cfg.kanban.require_api_auth {
        return;
    }
    match ensure_kanban_api_token(&cfg.kanban) {
        Ok(Some(_)) => {
            let path = edgecrab_core::resolved_kanban_token_path(&cfg.kanban);
            tracing::info!(
                path = %path.display(),
                "kanban API auth enabled (Bearer or X-Kanban-Token; loopback bypass if configured)"
            );
        }
        Ok(None) => tracing::warn!("kanban API auth disabled — no token configured"),
        Err(e) => tracing::warn!(error = %e, "kanban API token setup failed"),
    }
}
