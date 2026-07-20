//! E2E: `edgecrab auth` CLI for Grok / xAI OAuth (non-interactive).

use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn edgecrab(home: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_edgecrab"));
    cmd.env("HOME", home);
    cmd
}

fn write_xai_oauth_auth(home: &std::path::Path) {
    let path = home.join(".edgecrab").join("auth.json");
    fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    let doc = serde_json::json!({
        "providers": {
            "xai-oauth": {
                "auth_mode": "oauth_pkce",
                "tokens": {
                    "access_token": "at-test",
                    "refresh_token": "rt-test"
                },
                "discovery": {
                    "token_endpoint": "https://auth.x.ai/oauth2/token"
                }
            }
        }
    });
    fs::write(path, serde_json::to_string_pretty(&doc).expect("json")).expect("write");
}

#[test]
fn auth_list_includes_grok_line() {
    let home = tempdir().expect("home");
    fs::create_dir_all(home.path().join(".edgecrab")).expect("dir");

    let output = edgecrab(home.path())
        .args(["auth", "list"])
        .output()
        .expect("run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("grok"),
        "expected grok in auth list:\n{stdout}"
    );
}

#[test]
fn auth_status_grok_without_credentials() {
    let home = tempdir().expect("home");
    fs::create_dir_all(home.path().join(".edgecrab")).expect("dir");

    let output = edgecrab(home.path())
        .args(["auth", "status", "grok"])
        .output()
        .expect("run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("grok") || stdout.contains("xai"));
    assert!(stdout.contains("edgecrab auth add grok"));
}

#[test]
fn gx_e2_provider_needs_and_auth_status_with_oauth_file() {
    // Unit-level coverage for agent path aliases lives in xai_credentials module tests.
    assert!(edgecrab_core::oauth::provider_needs_xai_credentials("xai"));
    assert!(edgecrab_core::oauth::provider_needs_xai_credentials(
        "super-grok"
    ));
    assert!(!edgecrab_core::oauth::provider_needs_xai_credentials(
        "anthropic"
    ));
}

#[test]
fn auth_status_grok_with_mock_auth_json() {
    let home = tempdir().expect("home");
    write_xai_oauth_auth(home.path());

    let output = edgecrab(home.path())
        .args(["auth", "status", "grok"])
        .output()
        .expect("run");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.to_ascii_lowercase().contains("ready") || stdout.contains("oauth"),
        "expected ready/oauth hint:\n{stdout}"
    );
}

#[test]
fn auth_add_grok_rejects_static_token() {
    let home = tempdir().expect("home");
    fs::create_dir_all(home.path().join(".edgecrab")).expect("dir");

    let output = edgecrab(home.path())
        .args(["auth", "add", "grok", "--token", "not-valid"])
        .output()
        .expect("run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}{stdout}");
    assert!(
        combined.contains("OAuth") || combined.contains("browser"),
        "expected OAuth hint: {combined}"
    );
}
