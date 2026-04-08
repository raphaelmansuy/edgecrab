use std::fs;
use std::process::Command;

use edgecrab_state::{SessionDb, SessionRecord};
use edgecrab_types::Message;
use tempfile::tempdir;

fn edgecrab() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgecrab"))
}

fn sample_session(id: &str, title: &str, started_at: f64) -> SessionRecord {
    SessionRecord {
        id: id.into(),
        source: "cli".into(),
        user_id: None,
        model: Some("copilot/gpt-5-mini".into()),
        system_prompt: None,
        parent_session_id: None,
        started_at,
        ended_at: None,
        end_reason: None,
        message_count: 0,
        tool_call_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        estimated_cost_usd: None,
        title: Some(title.into()),
    }
}

#[test]
fn sessions_list_uses_rich_summary_output() {
    let home = tempdir().expect("temp home");
    let edgecrab_home = home.path().join(".edgecrab");
    fs::create_dir_all(&edgecrab_home).expect("edgecrab home");
    fs::write(edgecrab_home.join("config.yaml"), "mcp_servers: {}\n").expect("config");

    let db = SessionDb::open(&edgecrab_home.join("state.db")).expect("db");
    db.save_session(&sample_session(
        "sess-list-1",
        "List polish",
        1_720_000_000.0,
    ))
    .expect("save session");
    db.save_message(
        "sess-list-1",
        &Message::user("preview line for the list"),
        1_720_000_001.0,
    )
    .expect("save message");

    let output = edgecrab()
        .arg("--config")
        .arg(edgecrab_home.join("config.yaml"))
        .env("HOME", home.path())
        .env("EDGECRAB_HOME", &edgecrab_home)
        .args(["sessions", "list", "--limit", "5"])
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "sessions list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Preview"), "stdout:\n{stdout}");
    assert!(stdout.contains("List polish"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("preview line for the list"),
        "stdout:\n{stdout}"
    );
    assert!(stdout.contains("id=sess-list-1"), "stdout:\n{stdout}");
}

#[test]
fn sessions_browse_query_uses_rich_search_output() {
    let home = tempdir().expect("temp home");
    let edgecrab_home = home.path().join(".edgecrab");
    fs::create_dir_all(&edgecrab_home).expect("edgecrab home");
    fs::write(edgecrab_home.join("config.yaml"), "mcp_servers: {}\n").expect("config");

    let db = SessionDb::open(&edgecrab_home.join("state.db")).expect("db");
    db.save_session(&sample_session(
        "sess-search-1",
        "Socket recovery",
        1_720_000_100.0,
    ))
    .expect("save session");
    db.save_message(
        "sess-search-1",
        &Message::user("trace websocket reconnect jitter in production"),
        1_720_000_101.0,
    )
    .expect("save message");

    let output = edgecrab()
        .arg("--config")
        .arg(edgecrab_home.join("config.yaml"))
        .env("HOME", home.path())
        .env("EDGECRAB_HOME", &edgecrab_home)
        .args([
            "sessions",
            "browse",
            "--query",
            "websocket jitter",
            "--limit",
            "5",
        ])
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "sessions browse failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("sess-search-"), "stdout:\n{stdout}");
    assert!(stdout.contains("Socket recovery"), "stdout:\n{stdout}");
    assert!(stdout.contains("match:"), "stdout:\n{stdout}");
    assert!(stdout.contains("preview:"), "stdout:\n{stdout}");
}
