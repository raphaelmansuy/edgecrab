use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn edgecrab() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgecrab"))
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
