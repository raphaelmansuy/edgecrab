//! E2E: configured MCP servers must be probeable and covered by core toolset policy.
//!
//! Uses TempDir + EDGECRAB_HOME — never writes ~/.edgecrab.
//! Mock HTTP MCP server returns tools/list; `edgecrab mcp test` exercises discovery.

use std::fs;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::Response;
use axum::routing::post;
use tempfile::tempdir;

fn edgecrab() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_edgecrab"));
    command.env("EDGECRAB_DISABLE_BROWSER_OPEN", "1");
    command
}

#[derive(Clone)]
struct MockMcpState {
    list_calls: Arc<AtomicUsize>,
    /// Unique session ids issued on initialize (Wave-1 isolation).
    sessions_issued: Arc<AtomicUsize>,
}

/// GPS-like Streamable HTTP mock: 406 without dual Accept; SSE JSON-RPC bodies.
fn accept_ok(headers: &HeaderMap) -> bool {
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    accept.contains("application/json") && accept.contains("text/event-stream")
}

fn sse_message(payload: serde_json::Value, session_id: Option<&str>) -> Response {
    let body = format!("event: message\ndata: {payload}\n\n");
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream");
    if let Some(session_id) = session_id {
        builder = builder.header("Mcp-Session-Id", session_id);
    }
    builder.body(Body::from(body)).expect("sse response")
}

async fn mock_mcp_rpc(
    State(state): State<MockMcpState>,
    headers: HeaderMap,
    body: axum::Json<serde_json::Value>,
) -> Response {
    if !accept_ok(&headers) {
        return Response::builder()
            .status(StatusCode::NOT_ACCEPTABLE)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"error":"Client must accept both application/json and text/event-stream"}"#,
            ))
            .expect("406");
    }

    let request_session = headers
        .get("mcp-session-id")
        .or_else(|| headers.get("Mcp-Session-Id"))
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let method = body.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = body.get("id").cloned().unwrap_or(serde_json::json!(1));
    match method {
        "initialize" => {
            let n = state.sessions_issued.fetch_add(1, Ordering::SeqCst) + 1;
            let body = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2025-03-26",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "mock-gps", "version": "0.1.0" }
                }
            });
            let sse = format!("event: message\ndata: {body}\n\n");
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .header("Mcp-Session-Id", format!("mock-session-{n}"))
                .body(Body::from(sse))
                .expect("sse response")
        }
        "notifications/initialized" => Response::builder()
            .status(StatusCode::ACCEPTED)
            .body(Body::empty())
            .expect("202"),
        "tools/list" => {
            let _ = state.list_calls.fetch_add(1, Ordering::SeqCst);
            sse_message(
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": [{
                            "name": "list_funds",
                            "description": "List funds in the GPS portfolio system",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        }]
                    }
                }),
                request_session.as_deref(),
            )
        }
        _ => sse_message(
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("unknown method {method}") }
            }),
            request_session.as_deref(),
        ),
    }
}

fn spawn_mock_mcp() -> (
    String,
    tokio::sync::oneshot::Sender<()>,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    let list_calls = Arc::new(AtomicUsize::new(0));
    let sessions_issued = Arc::new(AtomicUsize::new(0));
    let state = MockMcpState {
        list_calls: list_calls.clone(),
        sessions_issued: sessions_issued.clone(),
    };
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let (addr_tx, addr_rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let addr = listener.local_addr().expect("addr");
            addr_tx.send(addr).expect("send addr");
            let router = Router::new()
                .route("/mcp", post(mock_mcp_rpc))
                .with_state(state);
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
    });

    let addr = addr_rx.recv().expect("receive addr");
    (
        format!("http://{addr}/mcp"),
        shutdown_tx,
        list_calls,
        sessions_issued,
    )
}

#[test]
fn e2e_mcp_test_lists_tools_from_registered_http_server() {
    let (mcp_url, shutdown_tx, list_calls, _) = spawn_mock_mcp();
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    // Do not use `\n\` continuations here — Rust strips leading whitespace after `\`,
    // which collapses YAML indentation and makes mcp_servers unreadable.
    fs::write(
        &config_path,
        format!("model: mock/test\nmcp_servers:\n  GPS:\n    url: {mcp_url}\n    enabled: true\n"),
    )
    .expect("seed config");

    // Prefer EDGECRAB_HOME (what mcp_client reads) over --config alone.
    let list = edgecrab()
        .args(["mcp", "list"])
        .env("HOME", home.path())
        .env("EDGECRAB_HOME", config_dir.as_os_str())
        .env("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST", "1")
        .output()
        .expect("run edgecrab mcp list");
    let list_out = String::from_utf8_lossy(&list.stdout);
    assert!(
        list.status.success() && list_out.contains("GPS"),
        "preflight mcp list must see GPS; config={}\nstdout={list_out}\nstderr={}",
        fs::read_to_string(&config_path).unwrap_or_default(),
        String::from_utf8_lossy(&list.stderr)
    );

    let output = edgecrab()
        .args(["mcp", "test", "GPS"])
        .env("HOME", home.path())
        .env("EDGECRAB_HOME", config_dir.as_os_str())
        .env("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST", "1")
        .output()
        .expect("run edgecrab mcp test");

    let _ = shutdown_tx.send(());

    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("list_funds") && stdout.contains("ok"),
        "expected GPS tools probe to succeed:\n{stdout}"
    );
    assert!(
        list_calls.load(Ordering::SeqCst) >= 1,
        "mock MCP tools/list was never called"
    );
}

#[test]
fn e2e_mcp_pool_isolates_http_sessions_per_edgecrab_session() {
    use edgecrab_tools::tools::mcp_client::{
        pooled_http_session_id, probe_configured_server_with_isolation, reload_mcp_connections,
    };

    let (mcp_url, shutdown_tx, _list_calls, sessions_issued) = spawn_mock_mcp();
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        format!("model: mock/test\nmcp_servers:\n  GPS:\n    url: {mcp_url}\n    enabled: true\n"),
    )
    .expect("seed config");

    // SAFETY: test process isolation via EDGECRAB_HOME; no parallel tests share this home.
    unsafe {
        std::env::set_var("EDGECRAB_HOME", config_dir.as_os_str());
        std::env::set_var("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST", "1");
    }
    reload_mcp_connections();

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    runtime.block_on(async {
        probe_configured_server_with_isolation("GPS", "session-alpha")
            .await
            .expect("probe alpha");
        probe_configured_server_with_isolation("GPS", "session-beta")
            .await
            .expect("probe beta");
        let sid_a = pooled_http_session_id("GPS", "session-alpha")
            .await
            .expect("session alpha id");
        let sid_b = pooled_http_session_id("GPS", "session-beta")
            .await
            .expect("session beta id");
        assert_ne!(
            sid_a, sid_b,
            "distinct EdgeCrab sessions must not share Mcp-Session-Id"
        );
    });

    let _ = shutdown_tx.send(());
    assert!(
        sessions_issued.load(Ordering::SeqCst) >= 2,
        "mock must initialize twice for two isolations"
    );

    unsafe {
        std::env::remove_var("EDGECRAB_HOME");
        std::env::remove_var("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST");
    }
}

#[test]
fn e2e_core_toolset_policy_covers_mcp_server_toolsets() {
    // Pure policy gate (no network): core → mcp must cover mcp-* dynamic toolsets.
    use edgecrab_tools::toolsets::{expand_toolset_names, toolset_covered_by, toolset_enabled};

    let expanded = expand_toolset_names(&["core".to_string()]);
    assert!(
        expanded.iter().any(|s| s == "mcp"),
        "core must expand to include mcp"
    );
    assert!(toolset_covered_by(&expanded, "mcp-GPS"));
    assert!(toolset_enabled(
        Some(&["core".to_string()]),
        None,
        "mcp-GPS"
    ));
}
