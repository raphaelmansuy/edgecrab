use std::fs;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Json;
use axum::extract::State;
use axum::routing::post;
use axum::{Router, http::StatusCode};
use tempfile::tempdir;

fn edgecrab() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgecrab"))
}

#[derive(Clone)]
struct MockOauthState {
    token_calls: Arc<AtomicUsize>,
}

async fn mock_device_endpoint() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "device_code": "device-code-1",
        "user_code": "ABCD-EFGH",
        "verification_uri": "https://example.com/activate",
        "verification_uri_complete": "https://example.com/activate?user_code=ABCD-EFGH",
        "expires_in": 60,
        "interval": 0
    }))
}

async fn mock_token_endpoint(
    State(state): State<MockOauthState>,
) -> (StatusCode, Json<serde_json::Value>) {
    let _ = state.token_calls.fetch_add(1, Ordering::SeqCst);
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "access_token": "access-token-1",
            "refresh_token": "refresh-token-1",
            "expires_in": 3600
        })),
    )
}

fn spawn_mock_oauth_server() -> (String, tokio::sync::oneshot::Sender<()>, Arc<AtomicUsize>) {
    let token_calls = Arc::new(AtomicUsize::new(0));
    let state = MockOauthState {
        token_calls: token_calls.clone(),
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
                .route("/device", post(mock_device_endpoint))
                .route("/token", post(mock_token_endpoint))
                .with_state(state);
            let _ = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
        });
    });

    let addr = addr_rx.recv().expect("receive addr");
    (format!("http://{}", addr), shutdown_tx, token_calls)
}

#[test]
fn mcp_doctor_reports_static_stdio_failures() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");

    let cwd_file = home.path().join("not-a-directory.txt");
    fs::write(&cwd_file, "x").expect("cwd file");

    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        format!(
            "mcp_servers:\n  broken:\n    command: definitely-not-an-edgecrab-command\n    cwd: {}\n    enabled: true\n",
            cwd_file.display()
        ),
    )
    .expect("config");

    let output = edgecrab()
        .arg("--config")
        .arg(&config_path)
        .args(["mcp", "doctor", "broken"])
        .env("HOME", home.path())
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "mcp doctor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("broken  fail"), "stdout:\n{stdout}");
    assert!(stdout.contains("command: fail"), "stdout:\n{stdout}");
    assert!(stdout.contains("cwd: not-a-directory"), "stdout:\n{stdout}");
}

#[test]
fn mcp_install_accepts_path_with_spaces_and_persists_it() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");

    let workspace_dir = home.path().join("workspace with spaces");
    fs::create_dir_all(&workspace_dir).expect("workspace dir");

    let config_path = config_dir.join("config.yaml");

    let output = edgecrab()
        .arg("--config")
        .arg(&config_path)
        .args(["mcp", "install", "filesystem", "--path"])
        .arg(&workspace_dir)
        .env("HOME", home.path())
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "mcp install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Configured MCP server 'filesystem'."),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("edgecrab mcp doctor filesystem"),
        "stdout:\n{stdout}"
    );

    let written = fs::read_to_string(&config_path).expect("read config");
    let rendered_path = workspace_dir.display().to_string();
    assert!(
        written.contains(&rendered_path),
        "expected persisted config to contain path {rendered_path}, got:\n{written}"
    );
}

#[test]
fn mcp_search_uses_cached_official_results_when_remote_sources_fail() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    let cache_dir = config_dir.join("cache");
    fs::create_dir_all(&cache_dir).expect("cache dir");

    let config_path = config_dir.join("config.yaml");
    fs::write(&config_path, "mcp_servers: {}\n").expect("config");
    fs::write(
        cache_dir.join("mcp_official_catalog.json"),
        serde_json::to_vec(&serde_json::json!({
            "fetched_at_epoch_secs": 1,
            "entries": [{
                "id": "time",
                "display_name": "Time",
                "description": "Timezone conversion capabilities.",
                "source_url": "https://github.com/modelcontextprotocol/servers/tree/main/src/time",
                "homepage": "https://github.com/modelcontextprotocol/servers/tree/main/src/time",
                "tags": ["official", "reference"],
                "installable_preset_id": "time"
            }]
        }))
        .expect("json"),
    )
    .expect("write cache");

    let output = edgecrab()
        .arg("--config")
        .arg(&config_path)
        .args(["mcp", "search", "timezone"])
        .env("HOME", home.path())
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "mcp search failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Official MCP search"), "stdout:\n{stdout}");
    assert!(stdout.contains("MCP Reference"), "stdout:\n{stdout}");
    assert!(stdout.contains("time"), "stdout:\n{stdout}");
}

#[test]
fn mcp_auth_explains_refresh_token_gap_and_operator_next_step() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");

    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        "mcp_servers:\n  oauth-demo:\n    url: https://example.com/mcp\n    enabled: true\n    oauth:\n      token_url: https://example.com/oauth/token\n      grant_type: refresh_token\n      auth_method: none\n",
    )
    .expect("config");

    let output = edgecrab()
        .arg("--config")
        .arg(&config_path)
        .args(["mcp", "auth", "oauth-demo"])
        .env("HOME", home.path())
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "mcp auth failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("MCP Auth"), "stdout:\n{stdout}");
    assert!(stdout.contains("grant=refresh_token"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("/mcp-token set-refresh oauth-demo <refresh-token>"),
        "stdout:\n{stdout}"
    );
}

#[test]
fn mcp_login_device_flow_caches_access_and_refresh_tokens() {
    let (base_url, shutdown_tx, token_calls) = spawn_mock_oauth_server();
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");

    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        format!(
            "mcp_servers:\n  oauth-device:\n    url: https://example.com/mcp\n    enabled: true\n    oauth:\n      token_url: {base_url}/token\n      device_authorization_url: {base_url}/device\n      grant_type: device_code\n      auth_method: none\n      client_id: edgecrab-device-client\n"
        ),
    )
    .expect("config");

    let output = edgecrab()
        .arg("--config")
        .arg(&config_path)
        .args(["mcp", "login", "oauth-device"])
        .env("HOME", home.path())
        .output()
        .expect("run edgecrab");

    let _ = shutdown_tx.send(());

    assert!(
        output.status.success(),
        "mcp login failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("OAuth device login"), "stdout:\n{stdout}");
    assert!(stdout.contains("OAuth login complete"), "stdout:\n{stdout}");
    assert_eq!(token_calls.load(Ordering::SeqCst), 1);

    let token_file = config_dir.join("mcp-tokens").join("oauth-device.json");
    let stored = fs::read_to_string(&token_file).expect("token file");
    assert!(stored.contains("access-token-1"), "stored:\n{stored}");
    assert!(stored.contains("refresh-token-1"), "stored:\n{stored}");
}

#[test]
fn mcp_auth_reports_dynamic_loopback_redirects_for_browser_flow() {
    let (base_url, shutdown_tx, _) = spawn_mock_oauth_server();
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");

    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        format!(
            "mcp_servers:\n  oauth-browser:\n    url: https://example.com/mcp\n    enabled: true\n    oauth:\n      token_url: {base_url}/token\n      authorization_url: {base_url}/authorize\n      redirect_url: http://127.0.0.1/callback\n      grant_type: authorization_code\n      auth_method: none\n      client_id: edgecrab-browser-client\n      use_pkce: true\n"
        ),
    )
    .expect("config");

    let output = edgecrab()
        .arg("--config")
        .arg(&config_path)
        .args(["mcp", "auth", "oauth-browser"])
        .env("HOME", home.path())
        .output()
        .expect("run edgecrab");

    let _ = shutdown_tx.send(());

    assert!(
        output.status.success(),
        "mcp auth failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let collected_stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        collected_stdout.contains("loopback-redirect=dynamic-port"),
        "stdout:\n{collected_stdout}"
    );
    assert!(
        collected_stdout.contains("/mcp login oauth-browser"),
        "stdout:\n{collected_stdout}"
    );
}
