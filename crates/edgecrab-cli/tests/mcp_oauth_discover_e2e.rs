//! E2E: URL-only MCP OAuth discovery (RFC 9728 + AS metadata + DCR).
//!
//! Spawns an MCP Authorization-shaped mock resource + authorization server on loopback.

use std::fs;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::routing::{get, post};
use axum::{Router, response::IntoResponse};
use serde_json::json;
use tempfile::tempdir;

fn edgecrab() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_edgecrab"));
    command.env("EDGECRAB_DISABLE_BROWSER_OPEN", "1");
    command.env("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST", "1");
    command
}

#[derive(Clone)]
struct MockState {
    base: String,
    register_calls: Arc<AtomicUsize>,
}

async fn mcp_probe(State(state): State<MockState>) -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    let meta = format!(
        "Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource\", scope=\"mcp:read\"",
        state.base
    );
    headers.insert(
        axum::http::header::WWW_AUTHENTICATE,
        HeaderValue::from_str(&meta).expect("header"),
    );
    (StatusCode::UNAUTHORIZED, headers, "unauthorized")
}

async fn prm(State(state): State<MockState>) -> Json<serde_json::Value> {
    Json(json!({
        "resource": format!("{}/mcp", state.base),
        "resource_name": "Example MCP Wiki",
        "authorization_servers": [state.base],
        "scopes_supported": ["mcp:read"],
        "bearer_methods_supported": ["header"]
    }))
}

async fn prm_with_path(State(state): State<MockState>) -> Json<serde_json::Value> {
    prm(State(state)).await
}

async fn as_metadata(State(state): State<MockState>) -> Json<serde_json::Value> {
    Json(json!({
        "issuer": state.base,
        "authorization_endpoint": format!("{}/authorize", state.base),
        "token_endpoint": format!("{}/token", state.base),
        "registration_endpoint": format!("{}/register", state.base),
        "response_types_supported": ["code"],
        "code_challenge_methods_supported": ["S256"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "token_endpoint_auth_methods_supported": ["none"],
        "scopes_supported": ["mcp:read", "offline_access"]
    }))
}

async fn register(
    State(state): State<MockState>,
    Json(body): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    state.register_calls.fetch_add(1, Ordering::SeqCst);
    assert_eq!(body["token_endpoint_auth_method"], "none");
    assert_eq!(body["client_name"], "EdgeCrab");
    let redirects = body["redirect_uris"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    // Accept dynamic-port loopback (`:0`) — reject portless (real MCP AS behavior).
    let has_dynamic = redirects.iter().any(|u| {
        u.as_str().is_some_and(|s| {
            s.starts_with("http://localhost:0/") || s.starts_with("http://127.0.0.1:0/")
        })
    });
    if !has_dynamic {
        let bad = redirects
            .first()
            .and_then(|v| v.as_str())
            .unwrap_or("http://127.0.0.1/callback");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_client_metadata",
                "error_description": format!(
                    "redirect_uri is not allowed for MCP OAuth: {bad}"
                )
            })),
        );
    }
    (
        StatusCode::CREATED,
        Json(json!({
            "client_id": "edgecrab-discovered-client",
            "client_id_issued_at": 1,
            "redirect_uris": ["http://localhost:0/callback"]
        })),
    )
}

fn spawn_mock_mcp_oauth() -> (String, tokio::sync::oneshot::Sender<()>, Arc<AtomicUsize>) {
    let register_calls = Arc::new(AtomicUsize::new(0));
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (addr_tx, addr_rx) = std::sync::mpsc::channel();
    let register_calls_thread = register_calls.clone();

    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let addr = listener.local_addr().expect("addr");
            let base = format!("http://{addr}");
            addr_tx.send(base.clone()).expect("send base");
            let state = MockState {
                base,
                register_calls: register_calls_thread,
            };
            let router = Router::new()
                .route("/mcp", post(mcp_probe).get(mcp_probe))
                .route("/.well-known/oauth-protected-resource", get(prm))
                .route(
                    "/.well-known/oauth-protected-resource/mcp",
                    get(prm_with_path),
                )
                .route("/.well-known/oauth-authorization-server", get(as_metadata))
                .route("/register", post(register))
                .with_state(state);
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
    });

    let base = addr_rx.recv().expect("receive base");
    (base, shutdown_tx, register_calls)
}

#[test]
fn e2e_mcp_add_discovers_oauth_from_url() {
    let (base, shutdown_tx, register_calls) = spawn_mock_mcp_oauth();
    let mcp_url = format!("{base}/mcp");

    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    fs::write(&config_path, "model: mock/test\n").expect("seed");

    let output = edgecrab()
        .arg("--config")
        .arg(&config_path)
        .args([
            "mcp",
            "add",
            "remote",
            "--url",
            &mcp_url,
            "--auth",
            "oauth",
            "--allow-loopback",
        ])
        .env("HOME", home.path())
        .env("EDGECRAB_HOME", config_dir.as_os_str())
        .output()
        .expect("run edgecrab");

    let _ = shutdown_tx.send(());

    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("remote") && (stdout.contains("oauth") || stdout.contains("login")),
        "stdout:\n{stdout}"
    );
    assert!(
        register_calls.load(Ordering::SeqCst) >= 1,
        "expected DCR registration call"
    );

    let saved = fs::read_to_string(&config_path).expect("read config");
    assert!(
        saved.contains("edgecrab-discovered-client"),
        "client_id missing:\n{saved}"
    );
    assert!(
        saved.contains("token_url") || saved.contains("/token"),
        "token_url missing:\n{saved}"
    );
    assert!(
        saved.contains("authorization_url") || saved.contains("/authorize"),
        "authorization_url missing:\n{saved}"
    );
    assert!(
        saved.contains("resource:") || saved.contains(&format!("{base}/mcp")),
        "resource missing:\n{saved}"
    );
    assert!(saved.contains("mcp:read"), "scopes missing:\n{saved}");
    assert!(
        saved.contains("issuer:") || saved.contains(&base),
        "issuer missing:\n{saved}"
    );
    assert!(
        saved.contains("localhost:0/callback"),
        "expected dynamic localhost:0 redirect_url:\n{saved}"
    );
    assert!(
        saved.contains("refresh_token")
            || saved.contains("authorization_code")
            || saved.contains("grant_type"),
        "expected oauth grant config for refresh:\n{saved}"
    );
}

#[test]
fn e2e_mcp_add_no_discover_keeps_manual_oauth() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    fs::write(&config_path, "model: mock/test\n").expect("seed");

    let output = edgecrab()
        .arg("--config")
        .arg(&config_path)
        .args([
            "mcp",
            "add",
            "manual",
            "--url",
            "https://mcp.example.com/mcp",
            "--auth",
            "oauth",
            "--no-discover",
            "--token-url",
            "https://auth.example.com/token",
            "--authorization-url",
            "https://auth.example.com/authorize",
            "--client-id",
            "manual-client",
            "--scope",
            "mcp:read",
        ])
        .env("HOME", home.path())
        .env("EDGECRAB_HOME", config_dir.as_os_str())
        .output()
        .expect("run");

    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let saved = fs::read_to_string(&config_path).expect("read");
    assert!(saved.contains("manual-client"), "{saved}");
    assert!(saved.contains("auth.example.com"), "{saved}");
}
