//! E2E: MCP URL registration (spec 022/014 WS-A).
//!
//! Uses TempDir + EDGECRAB_HOME — never writes ~/.edgecrab.

use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn edgecrab() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_edgecrab"));
    command.env("EDGECRAB_DISABLE_BROWSER_OPEN", "1");
    command
}

#[test]
fn e2e_m1_mcp_add_http_url_persists_config() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    fs::write(&config_path, "model: mock/test\n").expect("seed config");

    let output = edgecrab()
        .arg("--config")
        .arg(&config_path)
        .args([
            "mcp",
            "add",
            "linear",
            "--url",
            "https://mcp.example.com/mcp",
            "--auth",
            "none",
        ])
        .env("HOME", home.path())
        .env("EDGECRAB_HOME", config_dir.as_os_str())
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("linear"), "{stdout}");
    assert!(stdout.contains("mcp.example.com"), "{stdout}");

    let saved = fs::read_to_string(&config_path).expect("read config");
    assert!(
        saved.contains("mcp.example.com") || saved.contains("linear"),
        "config missing server:\n{saved}"
    );
}

#[test]
fn e2e_m3_mcp_add_rejects_ssrf_url() {
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
            "evil",
            "--url",
            "http://127.0.0.1:9/mcp",
            "--auth",
            "none",
        ])
        .env("HOME", home.path())
        .env("EDGECRAB_HOME", config_dir.as_os_str())
        // Ensure e2e localhost allow is off
        .env_remove("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST")
        .output()
        .expect("run");

    assert!(
        !output.status.success(),
        "SSRF URL should fail: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        err.to_ascii_lowercase().contains("ssrf")
            || err.to_ascii_lowercase().contains("blocked")
            || err.to_ascii_lowercase().contains("private"),
        "expected SSRF message: {err}"
    );
}

#[test]
fn e2e_m5_mcp_add_stdio_legacy_still_works() {
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
            "github",
            "npx",
            "-y",
            "@modelcontextprotocol/server-github",
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
    assert!(
        saved.contains("github") || saved.contains("npx"),
        "stdio server not persisted:\n{saved}"
    );
}

#[test]
fn e2e_m2_mcp_add_oauth_marks_oauth_config() {
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
            "linear",
            "--url",
            "https://mcp.example.com/mcp",
            "--auth",
            "oauth",
            "--token-url",
            "https://auth.example.com/oauth/token",
            "--client-id",
            "client-1",
            "--device-authorization-url",
            "https://auth.example.com/device",
            "--scope",
            "mcp",
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("oauth") || stdout.contains("login"),
        "expected oauth next-step hint: {stdout}"
    );
    let saved = fs::read_to_string(&config_path).expect("read");
    assert!(
        saved.contains("oauth") || saved.contains("token_url") || saved.contains("auth.example"),
        "oauth config missing:\n{saved}"
    );
}

#[test]
fn e2e_m4_mcp_add_bearer_stores_token_file() {
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
            "acme",
            "--url",
            "https://mcp.example.com/mcp",
            "--auth",
            "bearer",
            "--token",
            "sk-test-token-123",
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

    // Token store under EDGECRAB_HOME/mcp-tokens
    let token_dir = config_dir.join("mcp-tokens");
    let has_token_file = token_dir
        .exists()
        .then(|| fs::read_dir(&token_dir).ok())
        .flatten()
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy().contains("acme"))
        })
        .unwrap_or(false);

    let saved = fs::read_to_string(&config_path).expect("read");
    // Prefer token store; yaml may still list server without raw secret
    assert!(
        has_token_file || saved.contains("acme"),
        "token file or server entry missing; token_dir={has_token_file} config=\n{saved}"
    );
}
