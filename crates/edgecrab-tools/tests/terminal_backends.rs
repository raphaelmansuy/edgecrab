//! # End-to-end integration tests for execution backends
//!
//! These tests exercise the full backend stack as the terminal tool sees it:
//! `ToolContext::execute()` → `terminal.rs` → `get_or_create_backend()` →
//! `BackendKind::Local` → `LocalBackend::execute()` → `PersistentShell`.
//!
//! ## What is tested
//!
//! | Scenario                            | Backend | Notes                           |
//! |-------------------------------------|---------|----------------------------------|
//! | Basic echo via TerminalTool         | Local   | Full dispatch path               |
//! | Env-var blocklist (B-03)            | Local   | OPENAI_API_KEY must not leak     |
//! | Persistent shell state (B-02)       | Local   | Export → echo across two calls   |
//! | Working directory isolation         | Local   | ctx.cwd passed to backend        |
//! | Timeout (exit code 124)             | Local   | hard cap respected               |
//! | Security scanner blocks rm -rf /    | Local   | edgecrab-security integration    |
//! | Multi-call performance              | Local   | cached backend (no re-spawn)     |
//! | ExecOutput format edges             | -       | Unit test of format() method     |
//! | BackendKind roundtrip               | -       | Serde + Display                  |
//!
//! ## Running
//! ```bash
//! cargo test -p edgecrab-tools --test terminal_backends
//! ```

#![allow(clippy::await_holding_lock)]

use edgecrab_tools::{
    AppConfigRef, ProcessTable,
    registry::{ToolContext, ToolHandler},
    tools::{
        backends::{BackendKind, ExecOutput, ModalTransportMode},
        process::{GetProcessOutputTool, KillProcessTool, RunProcessTool, WaitForProcessTool},
        terminal::{TerminalTool, cleanup_backend_for_task},
    },
};
use edgecrab_types::Platform;
use serde_json::json;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

static BACKEND_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ─── Helpers ─────────────────────────────────────────────────────────

#[cfg(unix)]
fn lock_backend_env() -> std::sync::MutexGuard<'static, ()> {
    BACKEND_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(unix)]
fn write_executable_script(dir: &TempDir, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.path().join(name);
    std::fs::write(&path, body).expect("write script");
    let mut perms = std::fs::metadata(&path).expect("metadata").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).expect("chmod");
    path
}

#[cfg(unix)]
fn write_fake_daytona_helper(dir: &TempDir, name: &str) -> std::path::PathBuf {
    write_executable_script(
        dir,
        name,
        r#"#!/usr/bin/env python3
import json
import os
import subprocess
import sys

action = os.environ.get("EDGECRAB_DAYTONA_ACTION")
if action == "exec":
    command = os.environ["EDGECRAB_DAYTONA_COMMAND"]
    cwd = os.environ.get("EDGECRAB_DAYTONA_CWD") or None
    timeout = max(1, int(os.environ.get("EDGECRAB_DAYTONA_TIMEOUT", "60")))
    try:
        result = subprocess.run(
            ["sh", "-c", command],
            cwd=cwd,
            text=True,
            capture_output=True,
            timeout=timeout,
        )
        payload = {
            "ok": True,
            "stdout": result.stdout,
            "stderr": result.stderr,
            "exit_code": result.returncode,
        }
    except subprocess.TimeoutExpired as exc:
        payload = {
            "ok": True,
            "stdout": exc.stdout or "",
            "stderr": exc.stderr or "",
            "exit_code": 124,
        }
else:
    payload = {"ok": True}

sys.stdout.write(json.dumps(payload))
"#,
    )
}

#[cfg(unix)]
struct FakeModalServer {
    base_url: String,
    _storage_root: TempDir,
    snapshot_helper_path: PathBuf,
    shutdown_tx: Option<std::sync::mpsc::Sender<()>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[cfg(unix)]
struct FakeModalGatewayState {
    execs: HashMap<String, FakeManagedExec>,
    direct_sandboxes: HashMap<String, FakeDirectSandbox>,
    sandboxes_dir: PathBuf,
    snapshots_dir: PathBuf,
    next_direct_id: u64,
}

#[cfg(unix)]
struct FakeManagedExec {
    output: String,
    returncode: i32,
}

#[cfg(unix)]
struct FakeDirectSandbox {
    root_dir: PathBuf,
}

#[cfg(unix)]
struct FakeHttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

#[cfg(unix)]
impl FakeModalServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake modal server");
        listener
            .set_nonblocking(true)
            .expect("set fake modal server nonblocking");
        let addr = listener.local_addr().expect("fake modal server addr");
        let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel::<()>();
        let storage_root = TempDir::new().expect("fake modal storage root");
        let sandboxes_dir = storage_root.path().join("sandboxes");
        let snapshots_dir = storage_root.path().join("snapshots");
        std::fs::create_dir_all(&sandboxes_dir).expect("create fake modal sandboxes dir");
        std::fs::create_dir_all(&snapshots_dir).expect("create fake modal snapshots dir");
        let snapshot_helper_path = write_executable_script(
            &storage_root,
            "fake_modal_snapshot_helper.py",
            &format!(
                r#"#!/usr/bin/env python3
import os
import shutil
import sys
import uuid

root = {root:?}
sandbox_id = sys.argv[1]
source = os.path.join(root, "sandboxes", sandbox_id)
if not os.path.isdir(source):
    sys.stderr.write(f"missing sandbox {{sandbox_id}}\n")
    sys.exit(1)
snapshot_id = f"im-{{uuid.uuid4().hex[:12]}}"
target = os.path.join(root, "snapshots", snapshot_id)
shutil.copytree(source, target)
sys.stdout.write(snapshot_id)
"#,
                root = storage_root.path().to_string_lossy(),
            ),
        );
        let state = Arc::new(std::sync::Mutex::new(FakeModalGatewayState {
            execs: HashMap::new(),
            direct_sandboxes: HashMap::new(),
            sandboxes_dir,
            snapshots_dir,
            next_direct_id: 0,
        }));
        let handle = std::thread::spawn(move || {
            loop {
                if shutdown_rx.try_recv().is_ok() {
                    break;
                }
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_nonblocking(false)
                            .expect("set accepted fake modal stream blocking");
                        handle_fake_modal_request(&mut stream, &state);
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("fake modal server accept failed: {err}"),
                }
            }
        });

        Self {
            base_url: format!("http://{addr}/v1"),
            _storage_root: storage_root,
            snapshot_helper_path,
            shutdown_tx: Some(shutdown_tx),
            handle: Some(handle),
        }
    }

    fn snapshot_helper_path(&self) -> &Path {
        &self.snapshot_helper_path
    }
}

#[cfg(unix)]
impl Drop for FakeModalServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(unix)]
fn handle_fake_modal_request(
    stream: &mut TcpStream,
    state: &Arc<std::sync::Mutex<FakeModalGatewayState>>,
) {
    let request = read_http_request(stream);
    let auth = request
        .headers
        .get("authorization")
        .map(|value| value.as_str())
        .unwrap_or("");

    match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/v1/sandboxes") if auth.starts_with("Basic ") => {
            let payload: serde_json::Value =
                serde_json::from_slice(&request.body).expect("fake modal create payload");
            let image = payload["image"]
                .as_str()
                .expect("fake modal image payload")
                .to_string();
            let mut state = state.lock().expect("fake direct modal state lock");
            state.next_direct_id += 1;
            let sandbox_id = format!("fake-sandbox-{}", state.next_direct_id);
            let root_dir = state.sandboxes_dir.join(&sandbox_id);
            if image.starts_with("im-") {
                let snapshot_dir = state.snapshots_dir.join(&image);
                if !snapshot_dir.is_dir() {
                    write_json_response(
                        stream,
                        "400 Bad Request",
                        &json!({"error": format!("snapshot not found: {image}")}).to_string(),
                    );
                    return;
                }
                copy_dir_all(&snapshot_dir, &root_dir);
            } else {
                std::fs::create_dir_all(root_dir.join("root"))
                    .expect("create fake sandbox root filesystem");
            }
            state
                .direct_sandboxes
                .insert(sandbox_id.clone(), FakeDirectSandbox { root_dir });
            write_json_response(
                stream,
                "200 OK",
                &json!({"sandbox_id": sandbox_id}).to_string(),
            );
        }
        ("POST", "/v1/sandboxes") if auth.starts_with("Bearer ") => write_json_response(
            stream,
            "200 OK",
            &json!({"id": "managed-sandbox"}).to_string(),
        ),
        ("DELETE", path) if path.starts_with("/v1/sandboxes/fake-sandbox-") => {
            let sandbox_id = path.trim_start_matches("/v1/sandboxes/").to_string();
            if let Some(sandbox) = state
                .lock()
                .expect("fake direct modal state lock")
                .direct_sandboxes
                .remove(&sandbox_id)
            {
                let _ = std::fs::remove_dir_all(sandbox.root_dir);
            }
            write_empty_response(stream, "204 No Content");
        }
        ("POST", path)
            if path.starts_with("/v1/sandboxes/fake-sandbox-") && path.ends_with("/commands") =>
        {
            let payload: serde_json::Value =
                serde_json::from_slice(&request.body).expect("fake modal command payload");
            let command = payload["command"]
                .as_array()
                .and_then(|argv| argv.last())
                .and_then(|value| value.as_str())
                .expect("fake modal shell command");
            let sandbox_id = path
                .trim_start_matches("/v1/sandboxes/")
                .trim_end_matches("/commands")
                .trim_end_matches('/')
                .to_string();
            let sandbox_root = state
                .lock()
                .expect("fake direct modal state lock")
                .direct_sandboxes
                .get(&sandbox_id)
                .expect("fake direct sandbox present")
                .root_dir
                .clone();
            let output = execute_fake_modal_direct_command(&sandbox_root, command);
            write_json_response(
                stream,
                "200 OK",
                &json!({
                    "stdout": output.stdout,
                    "stderr": output.stderr,
                    "exit_code": output.exit_code,
                })
                .to_string(),
            );
        }
        ("POST", "/v1/sandboxes/managed-sandbox/execs") => {
            let payload: serde_json::Value =
                serde_json::from_slice(&request.body).expect("fake managed modal exec payload");
            let exec_id = payload["execId"]
                .as_str()
                .expect("fake managed modal exec id")
                .to_string();
            let command = payload["command"]
                .as_str()
                .expect("fake managed modal command");
            let cwd = payload["cwd"].as_str().unwrap_or(".");
            let output = execute_fake_shell_command(cwd, command);
            state.lock().expect("fake managed state lock").execs.insert(
                exec_id.clone(),
                FakeManagedExec {
                    output: output.stdout,
                    returncode: output.exit_code,
                },
            );
            write_json_response(
                stream,
                "200 OK",
                &json!({"execId": exec_id, "status": "running"}).to_string(),
            );
        }
        ("GET", path) if path.starts_with("/v1/sandboxes/managed-sandbox/execs/") => {
            let exec_id = path.rsplit('/').next().expect("managed exec id in path");
            if let Some(exec) = state
                .lock()
                .expect("fake managed state lock")
                .execs
                .get(exec_id)
            {
                write_json_response(
                    stream,
                    "200 OK",
                    &json!({
                        "execId": exec_id,
                        "status": "completed",
                        "output": exec.output,
                        "returncode": exec.returncode,
                    })
                    .to_string(),
                );
            } else {
                write_json_response(
                    stream,
                    "404 Not Found",
                    &json!({"error": "exec not found"}).to_string(),
                );
            }
        }
        ("POST", path)
            if path.starts_with("/v1/sandboxes/managed-sandbox/execs/")
                && path.ends_with("/cancel") =>
        {
            write_empty_response(stream, "204 No Content");
        }
        ("POST", "/v1/sandboxes/managed-sandbox/terminate") => {
            write_empty_response(stream, "204 No Content");
        }
        _ => write_json_response(
            stream,
            "404 Not Found",
            &json!({"error": format!("unhandled route: {} {}", request.method, request.path)})
                .to_string(),
        ),
    }
}

#[cfg(unix)]
struct FakeShellOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

#[cfg(unix)]
fn execute_fake_shell_command(cwd: &str, command: &str) -> FakeShellOutput {
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(command);
    if !cwd.is_empty() && cwd != "." {
        cmd.current_dir(cwd);
    }
    let output = cmd.output().expect("fake modal command output");
    FakeShellOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code().unwrap_or(-1),
    }
}

#[cfg(unix)]
fn execute_fake_modal_direct_command(sandbox_root: &Path, command: &str) -> FakeShellOutput {
    std::fs::create_dir_all(sandbox_root.join("root")).expect("create fake modal direct root");
    // Rewrite sandbox-relative paths to the actual sandbox root on the host.
    // Use mutually exclusive logic to avoid double-replacement.
    let rewritten = if command.contains("/modal-sandbox") {
        // Tests that use /modal-sandbox as the remote cwd (CI-safe: never exists on host).
        command.replace(
            "/modal-sandbox",
            &sandbox_root.join("root").to_string_lossy(),
        )
    } else {
        // Legacy tests that use /root as the remote cwd.
        command.replace("/root", &sandbox_root.join("root").to_string_lossy())
    };
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(rewritten)
        .output()
        .expect("fake direct modal command output");
    FakeShellOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code().unwrap_or(-1),
    }
}

#[cfg(unix)]
fn copy_dir_all(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).expect("create destination dir");
    for entry in std::fs::read_dir(src).expect("read source dir") {
        let entry = entry.expect("source dir entry");
        let ty = entry.file_type().expect("source file type");
        let target = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &target);
        } else if ty.is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).expect("create target parent");
            }
            std::fs::copy(entry.path(), target).expect("copy target file");
        }
    }
}

#[cfg(unix)]
fn read_http_request(stream: &mut TcpStream) -> FakeHttpRequest {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        let read = stream.read(&mut chunk).expect("read fake modal request");
        assert!(read > 0, "unexpected EOF while reading fake modal request");
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(pos) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break pos;
        }
    };

    let headers = String::from_utf8_lossy(&buffer[..header_end]).to_string();
    let mut lines = headers.lines();
    let request_line = lines.next().expect("fake modal request line");
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().expect("fake modal method").to_string();
    let path = request_parts.next().expect("fake modal path").to_string();
    let mut header_map = HashMap::new();
    let content_length = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            header_map.insert(name.clone(), value.clone());
            (name == "content-length")
                .then(|| value.parse::<usize>().ok())
                .flatten()
        })
        .next()
        .unwrap_or(0);

    let body_start = header_end + 4;
    while buffer.len().saturating_sub(body_start) < content_length {
        let read = stream.read(&mut chunk).expect("read fake modal body");
        assert!(read > 0, "unexpected EOF while reading fake modal body");
        buffer.extend_from_slice(&chunk[..read]);
    }

    let body = buffer[body_start..body_start + content_length].to_vec();
    FakeHttpRequest {
        method,
        path,
        headers: header_map,
        body,
    }
}

#[cfg(unix)]
fn write_json_response(stream: &mut TcpStream, status: &str, body: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body,
    );
    stream
        .write_all(response.as_bytes())
        .expect("write fake modal response");
}

#[cfg(unix)]
fn write_empty_response(stream: &mut TcpStream, status: &str) {
    let response = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream
        .write_all(response.as_bytes())
        .expect("write fake modal empty response");
}

/// Build a minimal ToolContext for integration tests.
///
/// We replicate the logic from `ToolContext::test_context()` which is
/// gated with `#[cfg(test)]` and therefore unavailable in integration tests.
fn make_ctx(dir: &Path) -> ToolContext {
    ToolContext {
        task_id: format!("e2e-{}", uuid::Uuid::new_v4().simple()),
        cwd: dir.to_path_buf(),
        session_id: "e2e-session".into(),
        user_task: None,
        cancel: CancellationToken::new(),
        config: AppConfigRef::default(),
        state_db: None,
        platform: Platform::Cli,
        process_table: None,
        provider: None,
        tool_registry: None,
        delegate_depth: 0,
        sub_agent_runner: None,
        delegation_event_tx: None,
        clarify_tx: None,
        approval_tx: None,
        on_skills_changed: None,
        gateway_sender: None,
        origin_chat: None,
        session_key: None,
        todo_store: None,
        current_tool_call_id: None,
        current_tool_name: None,
        injected_messages: None,
        tool_progress_tx: None,
        watch_notification_tx: None,
    }
}

fn extract_process_id(result: &str) -> String {
    // R18: result is JSON {"ok":true,"process_id":"proc-X","command":"..."}
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(result)
        && let Some(pid) = v.get("process_id").and_then(|p| p.as_str())
    {
        return pid.to_string();
    }
    // Legacy prose fallback: "Process started: <cmd> (id=proc-X)."
    let marker = "id=";
    let start = result.find(marker).expect("process id marker") + marker.len();
    let rest = &result[start..];
    rest.split(|c: char| c == ')' || c.is_whitespace())
        .next()
        .expect("process id token")
        .to_string()
}

#[cfg(unix)]
fn configure_direct_modal_ctx(ctx: &mut ToolContext, task_id: &str) {
    ctx.task_id = task_id.to_string();
    ctx.config.terminal_backend = BackendKind::Modal;
    ctx.config.terminal_modal.mode = ModalTransportMode::Direct;
    ctx.config.terminal_modal.image = "fake-modal-image".into();
    ctx.config.terminal_modal.token_id = "fake-token-id".into();
    ctx.config.terminal_modal.token_secret = "fake-token-secret".into();
}

// ─── Full dispatch tests (TerminalTool → LocalBackend) ───────────────

#[tokio::test]
async fn e2e_basic_echo() {
    let dir = TempDir::new().expect("tmpdir");
    let ctx = make_ctx(dir.path());
    let result = TerminalTool
        .execute(json!({"command": "echo hello-e2e"}), &ctx)
        .await
        .expect("execute");
    assert!(
        result.contains("hello-e2e"),
        "expected 'hello-e2e' in: {result}"
    );
}

#[tokio::test]
async fn e2e_exit_code_in_output() {
    let dir = TempDir::new().expect("tmpdir");
    let ctx = make_ctx(dir.path());
    let result = TerminalTool
        .execute(json!({"command": "exit 99"}), &ctx)
        .await
        .expect("execute");
    assert!(
        result.contains("exit code: 99") || result.contains("[exit code: 99]"),
        "expected exit code 99 in: {result}"
    );
}

#[tokio::test]
async fn e2e_stderr_captured() {
    let dir = TempDir::new().expect("tmpdir");
    let ctx = make_ctx(dir.path());
    let result = TerminalTool
        .execute(
            json!({"command": "echo stdoutval && echo stderrval >&2"}),
            &ctx,
        )
        .await
        .expect("execute");
    assert!(result.contains("stdoutval"), "missing stdout: {result}");
    assert!(result.contains("stderrval"), "missing stderr: {result}");
}

#[tokio::test]
async fn e2e_working_directory_respected() {
    let dir = TempDir::new().expect("tmpdir");
    std::fs::write(dir.path().join("probe.txt"), "probe-content").expect("write");
    let ctx = make_ctx(dir.path());
    let result = TerminalTool
        .execute(json!({"command": "cat probe.txt"}), &ctx)
        .await
        .expect("execute");
    assert!(
        result.contains("probe-content"),
        "expected file content in: {result}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn e2e_modal_backend_respects_working_directory_via_fake_http_api() {
    let _guard = lock_backend_env();
    let dir = TempDir::new().expect("tmpdir");
    let edgecrab_home = TempDir::new().expect("edgecrab home");
    let server = FakeModalServer::start();

    let mut ctx = make_ctx(dir.path());
    configure_direct_modal_ctx(
        &mut ctx,
        &format!("e2e-modal-cwd-{}", uuid::Uuid::new_v4().simple()),
    );
    ctx.cwd = PathBuf::from("/modal-sandbox");
    unsafe { std::env::set_var("EDGECRAB_HOME", edgecrab_home.path()) };
    unsafe { std::env::set_var("EDGECRAB_MODAL_BASE_URL", &server.base_url) };

    TerminalTool
        .execute(
            json!({"command": "mkdir -p /modal-sandbox/workspace && echo modal-probe >/modal-sandbox/workspace/probe.txt"}),
            &ctx,
        )
        .await
        .expect("seed remote workspace");

    ctx.cwd = PathBuf::from("/modal-sandbox/workspace");
    let result = TerminalTool
        .execute(json!({"command": "cat probe.txt"}), &ctx)
        .await
        .expect("execute");
    assert!(result.contains("modal-probe"), "got: {result}");

    let _ = cleanup_backend_for_task(&ctx.task_id).await;
    unsafe { std::env::remove_var("EDGECRAB_MODAL_BASE_URL") };
    unsafe { std::env::remove_var("EDGECRAB_HOME") };
}

#[cfg(unix)]
#[tokio::test]
async fn e2e_modal_direct_syncs_auth_skills_and_cache_files() {
    let _guard = lock_backend_env();
    let dir = TempDir::new().expect("tmpdir");
    let edgecrab_home = TempDir::new().expect("edgecrab home");
    let server = FakeModalServer::start();

    std::fs::create_dir_all(edgecrab_home.path().join("skills/demo-skill")).expect("skills dir");
    std::fs::create_dir_all(edgecrab_home.path().join("image_cache")).expect("cache dir");
    std::fs::write(
        edgecrab_home.path().join("auth.json"),
        r#"{"providers":{"nous":{"access_token":"sync-token"}}}"#,
    )
    .expect("write auth");
    std::fs::write(
        edgecrab_home.path().join("skills/demo-skill/SKILL.md"),
        "# Demo Skill\n",
    )
    .expect("write skill");
    std::fs::write(
        edgecrab_home.path().join("image_cache/upload.txt"),
        "cached-upload-v1",
    )
    .expect("write cache file");

    let mut ctx = make_ctx(dir.path());
    configure_direct_modal_ctx(
        &mut ctx,
        &format!("e2e-modal-sync-{}", uuid::Uuid::new_v4().simple()),
    );
    ctx.cwd = PathBuf::from(".");
    unsafe { std::env::set_var("EDGECRAB_HOME", edgecrab_home.path()) };
    unsafe { std::env::set_var("EDGECRAB_MODAL_BASE_URL", &server.base_url) };
    unsafe {
        std::env::set_var(
            "EDGECRAB_MODAL_SNAPSHOT_HELPER",
            server.snapshot_helper_path(),
        )
    };

    let result = TerminalTool
        .execute(
            json!({"command": "cat /root/.edgecrab/auth.json && cat /root/.edgecrab/skills/demo-skill/SKILL.md && cat /root/.edgecrab/image_cache/upload.txt"}),
            &ctx,
        )
        .await
        .expect("execute synced files");
    assert!(result.contains("sync-token"), "missing auth sync: {result}");
    assert!(
        result.contains("Demo Skill"),
        "missing skill sync: {result}"
    );
    assert!(
        result.contains("cached-upload-v1"),
        "missing cache sync: {result}"
    );

    std::fs::write(
        edgecrab_home.path().join("auth.json"),
        r#"{"providers":{"nous":{"access_token":"sync-token-v2"}}}"#,
    )
    .expect("rewrite auth");
    let updated = TerminalTool
        .execute(json!({"command": "cat /root/.edgecrab/auth.json"}), &ctx)
        .await
        .expect("execute updated sync");
    assert!(updated.contains("sync-token-v2"), "got: {updated}");

    let _ = cleanup_backend_for_task(&ctx.task_id).await;
    unsafe { std::env::remove_var("EDGECRAB_MODAL_SNAPSHOT_HELPER") };
    unsafe { std::env::remove_var("EDGECRAB_MODAL_BASE_URL") };
    unsafe { std::env::remove_var("EDGECRAB_HOME") };
}

#[cfg(unix)]
#[tokio::test]
async fn e2e_modal_direct_persists_filesystem_snapshots_across_backend_rebuilds() {
    let _guard = lock_backend_env();
    let dir = TempDir::new().expect("tmpdir");
    let edgecrab_home = TempDir::new().expect("edgecrab home");
    let server = FakeModalServer::start();
    let task_id = format!("e2e-modal-snapshot-{}", uuid::Uuid::new_v4().simple());

    unsafe { std::env::set_var("EDGECRAB_HOME", edgecrab_home.path()) };
    unsafe { std::env::set_var("EDGECRAB_MODAL_BASE_URL", &server.base_url) };
    unsafe {
        std::env::set_var(
            "EDGECRAB_MODAL_SNAPSHOT_HELPER",
            server.snapshot_helper_path(),
        )
    };

    let mut first_ctx = make_ctx(dir.path());
    configure_direct_modal_ctx(&mut first_ctx, &task_id);
    first_ctx.cwd = PathBuf::from(".");
    let first = TerminalTool
        .execute(
            json!({"command": "mkdir -p /root/work && echo persisted-state >/root/work/state.txt"}),
            &first_ctx,
        )
        .await
        .expect("write remote state");
    assert!(!first.contains("[stderr]"), "write failed: {first}");
    assert!(
        cleanup_backend_for_task(&task_id).await,
        "backend cleanup should snapshot"
    );

    let snapshots_path = edgecrab_home.path().join("modal_snapshots.json");
    let snapshots = std::fs::read_to_string(&snapshots_path).expect("snapshot store");
    assert!(
        snapshots.contains(&task_id),
        "missing task snapshot: {snapshots}"
    );
    assert!(
        snapshots.contains("im-"),
        "missing snapshot id: {snapshots}"
    );

    let mut second_ctx = make_ctx(dir.path());
    configure_direct_modal_ctx(&mut second_ctx, &task_id);
    second_ctx.cwd = PathBuf::from(".");
    let restored = TerminalTool
        .execute(json!({"command": "cat /root/work/state.txt"}), &second_ctx)
        .await
        .expect("restore snapshot state");
    assert!(restored.contains("persisted-state"), "got: {restored}");

    let _ = cleanup_backend_for_task(&task_id).await;
    unsafe { std::env::remove_var("EDGECRAB_MODAL_SNAPSHOT_HELPER") };
    unsafe { std::env::remove_var("EDGECRAB_MODAL_BASE_URL") };
    unsafe { std::env::remove_var("EDGECRAB_HOME") };
}

#[cfg(unix)]
#[tokio::test]
async fn e2e_modal_direct_invalid_snapshot_falls_back_to_base_image() {
    let _guard = lock_backend_env();
    let dir = TempDir::new().expect("tmpdir");
    let edgecrab_home = TempDir::new().expect("edgecrab home");
    let server = FakeModalServer::start();
    let task_id = format!("e2e-modal-bad-snapshot-{}", uuid::Uuid::new_v4().simple());

    std::fs::write(
        edgecrab_home.path().join("modal_snapshots.json"),
        json!({ format!("direct:{task_id}"): "im-missing-snapshot" }).to_string(),
    )
    .expect("write invalid snapshot store");

    unsafe { std::env::set_var("EDGECRAB_HOME", edgecrab_home.path()) };
    unsafe { std::env::set_var("EDGECRAB_MODAL_BASE_URL", &server.base_url) };
    unsafe {
        std::env::set_var(
            "EDGECRAB_MODAL_SNAPSHOT_HELPER",
            server.snapshot_helper_path(),
        )
    };

    let mut ctx = make_ctx(dir.path());
    configure_direct_modal_ctx(&mut ctx, &task_id);
    ctx.cwd = PathBuf::from(".");
    let result = TerminalTool
        .execute(json!({"command": "echo fallback-ok"}), &ctx)
        .await
        .expect("fallback execute");
    assert!(result.contains("fallback-ok"), "got: {result}");

    let snapshots = std::fs::read_to_string(edgecrab_home.path().join("modal_snapshots.json"))
        .expect("read snapshot store");
    assert!(
        !snapshots.contains("im-missing-snapshot"),
        "stale snapshot id should be removed: {snapshots}"
    );

    let _ = cleanup_backend_for_task(&ctx.task_id).await;
    unsafe { std::env::remove_var("EDGECRAB_MODAL_SNAPSHOT_HELPER") };
    unsafe { std::env::remove_var("EDGECRAB_MODAL_BASE_URL") };
    unsafe { std::env::remove_var("EDGECRAB_HOME") };
}

#[cfg(unix)]
#[tokio::test]
async fn e2e_managed_modal_backend_respects_working_directory_via_fake_gateway() {
    let _guard = lock_backend_env();
    let dir = TempDir::new().expect("tmpdir");
    std::fs::write(dir.path().join("probe.txt"), "managed-modal-probe").expect("write");
    let server = FakeModalServer::start();
    let gateway_origin = server.base_url.trim_end_matches("/v1").to_string();

    let mut ctx = make_ctx(dir.path());
    ctx.task_id = format!("e2e-managed-modal-cwd-{}", uuid::Uuid::new_v4().simple());
    ctx.config.terminal_backend = BackendKind::Modal;
    ctx.config.terminal_modal.mode = ModalTransportMode::Managed;
    ctx.config.terminal_modal.image = "fake-managed-image".into();
    ctx.config.terminal_modal.managed_gateway_url = Some(gateway_origin);
    ctx.config.terminal_modal.managed_user_token = Some("managed-user-token".into());

    let err = TerminalTool
        .execute(json!({"command": "cat probe.txt"}), &ctx)
        .await
        .expect_err("managed modal should reject host workspace cwd");
    match err {
        edgecrab_types::ToolError::ExecutionFailed { message, .. } => {
            assert!(message.contains("cannot access host workspace path"));
        }
        other => panic!("expected execution failure, got {other:?}"),
    }

    let _ = cleanup_backend_for_task(&ctx.task_id).await;
}

#[cfg(unix)]
#[tokio::test]
async fn e2e_modal_auto_mode_falls_back_to_managed_gateway() {
    let _guard = lock_backend_env();
    let dir = TempDir::new().expect("tmpdir");
    let server = FakeModalServer::start();
    let gateway_origin = server.base_url.trim_end_matches("/v1").to_string();

    let mut ctx = make_ctx(dir.path());
    ctx.task_id = format!("e2e-modal-auto-managed-{}", uuid::Uuid::new_v4().simple());
    ctx.config.terminal_backend = BackendKind::Modal;
    ctx.config.terminal_modal.mode = ModalTransportMode::Auto;
    ctx.config.terminal_modal.image = "fake-managed-image".into();
    ctx.config.terminal_modal.token_id.clear();
    ctx.config.terminal_modal.token_secret.clear();
    ctx.cwd = PathBuf::from(".");
    unsafe { std::env::set_var("MODAL_GATEWAY_URL", &gateway_origin) };
    unsafe { std::env::set_var("TOOL_GATEWAY_USER_TOKEN", "managed-user-token") };

    let result = TerminalTool
        .execute(json!({"command": "echo auto-managed-probe"}), &ctx)
        .await
        .expect("execute");
    assert!(result.contains("auto-managed-probe"), "got: {result}");

    unsafe { std::env::remove_var("MODAL_GATEWAY_URL") };
    unsafe { std::env::remove_var("TOOL_GATEWAY_USER_TOKEN") };
    let _ = cleanup_backend_for_task(&ctx.task_id).await;
}

#[cfg(unix)]
#[tokio::test]
async fn e2e_modal_direct_mode_without_credentials_fails_descriptively() {
    let _guard = lock_backend_env();
    let dir = TempDir::new().expect("tmpdir");
    let mut ctx = make_ctx(dir.path());
    ctx.config.terminal_backend = BackendKind::Modal;
    ctx.config.terminal_modal.mode = ModalTransportMode::Direct;
    ctx.config.terminal_modal.token_id.clear();
    ctx.config.terminal_modal.token_secret.clear();
    ctx.cwd = PathBuf::from(".");

    let err = TerminalTool
        .execute(json!({"command": "echo nope"}), &ctx)
        .await
        .expect_err("direct mode without creds should fail");

    match err {
        edgecrab_types::ToolError::ExecutionFailed { message, .. } => {
            assert!(message.contains("direct mode"), "got: {message}");
        }
        other => panic!("expected execution failure, got {other:?}"),
    }
}

#[tokio::test]
async fn e2e_env_var_blocklist_openai() {
    // Set a secret in the process env, then verify the agent shell can't see it
    let dir = TempDir::new().expect("tmpdir");
    let ctx = make_ctx(dir.path());

    // Use a unique env var name to avoid test interference
    unsafe { std::env::set_var("OPENAI_API_KEY", "sk-test-secret-12345-e2e") };

    let result = TerminalTool
        .execute(
            json!({"command": "echo \"KEY=${OPENAI_API_KEY:-NOT_SET}\""}),
            &ctx,
        )
        .await
        .expect("execute");

    unsafe { std::env::remove_var("OPENAI_API_KEY") };

    assert!(
        result.contains("NOT_SET") || !result.contains("sk-test-secret"),
        "OPENAI_API_KEY should be blocked from subprocess; got: {result}"
    );
}

#[tokio::test]
async fn e2e_env_var_blocklist_anthropic() {
    let dir = TempDir::new().expect("tmpdir");
    let ctx = make_ctx(dir.path());
    unsafe { std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-secret-99999") };

    let result = TerminalTool
        .execute(
            json!({"command": "echo \"KEY=${ANTHROPIC_API_KEY:-NOT_SET}\""}),
            &ctx,
        )
        .await
        .expect("execute");

    unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };

    assert!(
        result.contains("NOT_SET") || !result.contains("sk-ant-secret"),
        "ANTHROPIC_API_KEY should be blocked; got: {result}"
    );
}

#[tokio::test]
async fn e2e_env_var_blocklist_modal() {
    let dir = TempDir::new().expect("tmpdir");
    let ctx = make_ctx(dir.path());
    unsafe { std::env::set_var("MODAL_TOKEN_SECRET", "super-secret-modal") };

    let result = TerminalTool
        .execute(
            json!({"command": "echo \"SECRET=${MODAL_TOKEN_SECRET:-NOT_SET}\""}),
            &ctx,
        )
        .await
        .expect("execute");

    unsafe { std::env::remove_var("MODAL_TOKEN_SECRET") };

    assert!(
        result.contains("NOT_SET") || !result.contains("super-secret-modal"),
        "MODAL_TOKEN_SECRET should be blocked; got: {result}"
    );
}

#[tokio::test]
async fn e2e_persistent_shell_state_export() {
    // Two consecutive calls — the second should see the variable exported by the first
    let dir = TempDir::new().expect("tmpdir");
    // Use a unique task_id so we get a fresh backend not shared with other tests
    let mut ctx = make_ctx(dir.path());
    ctx.task_id = format!("e2e-persistent-{}", uuid::Uuid::new_v4().simple());

    TerminalTool
        .execute(json!({"command": "export MY_E2E_VAR=golden"}), &ctx)
        .await
        .expect("export");

    let result = TerminalTool
        .execute(
            json!({"command": "echo \"value=${MY_E2E_VAR:-NOT_SET}\""}),
            &ctx,
        )
        .await
        .expect("echo");

    assert!(
        result.contains("golden"),
        "persistent shell should retain MY_E2E_VAR; got: {result}"
    );
}

#[tokio::test]
async fn e2e_timeout_produces_correct_exit_code() {
    let dir = TempDir::new().expect("tmpdir");
    let mut ctx = make_ctx(dir.path());
    ctx.task_id = format!("e2e-timeout-{}", uuid::Uuid::new_v4().simple());

    let result = TerminalTool
        .execute(json!({"command": "sleep 300", "timeout_seconds": 1}), &ctx)
        .await
        .expect("execute");

    // After timeout: exit 124 (bash timeout convention)
    // The persistent shell may be dead after kill, so the output may contain
    // the exit code or just be "[command completed with exit code: 124]"
    assert!(
        result.contains("124") || result.contains("130"),
        "expected timeout/interrupt exit code; got: {result}"
    );
}

#[tokio::test]
async fn e2e_security_scanner_blocks_dangerous_command() {
    let dir = TempDir::new().expect("tmpdir");
    let ctx = make_ctx(dir.path());

    // edgecrab-security scanner should block `rm -rf /`
    let result = TerminalTool
        .execute(json!({"command": "rm -rf /"}), &ctx)
        .await;

    // Expect either PermissionDenied error or blocked output
    match result {
        Err(edgecrab_types::ToolError::PermissionDenied(_)) => {} // correct
        Ok(s) if s.contains("blocked") || s.contains("PermissionDenied") => {} // also ok
        Ok(s) => panic!("rm -rf / should have been blocked; got: {s}"),
        Err(e) => panic!("unexpected error type: {e:?}"),
    }
}

#[tokio::test]
async fn e2e_multi_call_caching() {
    // Multiple calls with the same task_id reuse the cached backend.
    // Time the calls — if each spawned a new process, it would be ~10× slower.
    let dir = TempDir::new().expect("tmpdir");
    let mut ctx = make_ctx(dir.path());
    ctx.task_id = "e2e-cache-perf".into();

    let start = std::time::Instant::now();
    for i in 0..10u32 {
        let r = TerminalTool
            .execute(json!({"command": format!("echo iter-{i}")}), &ctx)
            .await
            .expect("execute");
        assert!(r.contains(&format!("iter-{i}")));
    }
    let elapsed = start.elapsed();

    // 10 calls should complete in < 5 s on any sane system when the shell is reused
    // (spawning 10 new processes would be slower). This is a loose bound.
    assert!(
        elapsed < Duration::from_secs(5),
        "10 backend calls took {elapsed:?} — possible re-spawn overhead"
    );
    let _ = cleanup_backend_for_task(&ctx.task_id).await;
}

#[tokio::test]
async fn e2e_idle_backend_cleanup_rebuilds_fresh_shell() {
    let dir = TempDir::new().expect("tmpdir");
    let mut ctx = make_ctx(dir.path());
    ctx.task_id = format!("e2e-idle-cleanup-{}", uuid::Uuid::new_v4().simple());

    TerminalTool
        .execute(json!({"command": "export EDGECRAB_IDLE_PROBE=warm"}), &ctx)
        .await
        .expect("export");

    let cleaned = cleanup_backend_for_task(&ctx.task_id).await;
    assert!(cleaned, "expected cached backend for task to be cleaned");

    let result = TerminalTool
        .execute(
            json!({"command": "echo \"probe=${EDGECRAB_IDLE_PROBE:-NOT_SET}\""}),
            &ctx,
        )
        .await
        .expect("echo");

    assert!(
        result.contains("NOT_SET"),
        "idle cleanup should force a fresh backend; got: {result}"
    );
    let _ = cleanup_backend_for_task(&ctx.task_id).await;
}

#[cfg(unix)]
#[tokio::test]
async fn e2e_daytona_backend_via_fake_helper() {
    let _guard = lock_backend_env();
    let dir = TempDir::new().expect("tmpdir");
    let fake = write_fake_daytona_helper(&dir, "fake-python");

    let mut ctx = make_ctx(dir.path());
    ctx.config.terminal_backend = BackendKind::Daytona;
    ctx.cwd = PathBuf::from(".");
    unsafe { std::env::set_var("EDGECRAB_DAYTONA_PYTHON", &fake) };

    let result = TerminalTool
        .execute(json!({"command": "echo hello-daytona"}), &ctx)
        .await
        .expect("execute");
    assert!(result.contains("hello-daytona"), "got: {result}");

    unsafe { std::env::remove_var("EDGECRAB_DAYTONA_PYTHON") };
}

#[cfg(unix)]
#[tokio::test]
async fn e2e_daytona_remote_run_process_waits_and_collects_output() {
    let _guard = lock_backend_env();
    let dir = TempDir::new().expect("tmpdir");
    let fake = write_fake_daytona_helper(&dir, "fake-python-process");

    let mut ctx = make_ctx(dir.path());
    ctx.task_id = format!("e2e-daytona-process-{}", uuid::Uuid::new_v4().simple());
    ctx.config.terminal_backend = BackendKind::Daytona;
    ctx.process_table = Some(Arc::new(ProcessTable::new()));
    unsafe { std::env::set_var("EDGECRAB_DAYTONA_PYTHON", &fake) };

    let started = RunProcessTool
        .execute(
            json!({"command": "printf 'alpha\\n'; sleep 1; printf 'omega\\n'"}),
            &ctx,
        )
        .await
        .expect("run_process");
    let process_id = extract_process_id(&started);

    let waited = WaitForProcessTool
        .execute(json!({"process_id": process_id, "timeout_secs": 10}), &ctx)
        .await
        .expect("wait_for_process");

    assert!(waited.contains("exited (code 0)"), "got: {waited}");
    assert!(waited.contains("alpha"), "missing alpha: {waited}");
    assert!(waited.contains("omega"), "missing omega: {waited}");

    unsafe { std::env::remove_var("EDGECRAB_DAYTONA_PYTHON") };
    let _ = cleanup_backend_for_task(&ctx.task_id).await;
}

#[cfg(unix)]
#[tokio::test]
async fn e2e_daytona_remote_run_process_can_be_killed() {
    let _guard = lock_backend_env();
    let dir = TempDir::new().expect("tmpdir");
    let fake = write_fake_daytona_helper(&dir, "fake-python-kill");

    let mut ctx = make_ctx(dir.path());
    ctx.task_id = format!("e2e-daytona-kill-{}", uuid::Uuid::new_v4().simple());
    ctx.config.terminal_backend = BackendKind::Daytona;
    ctx.process_table = Some(Arc::new(ProcessTable::new()));
    unsafe { std::env::set_var("EDGECRAB_DAYTONA_PYTHON", &fake) };

    let started = RunProcessTool
        .execute(json!({"command": "printf 'start\\n'; sleep 30"}), &ctx)
        .await
        .expect("run_process");
    let process_id = extract_process_id(&started);

    tokio::time::sleep(Duration::from_secs(1)).await;

    let killed = KillProcessTool
        .execute(json!({"process_id": process_id.clone()}), &ctx)
        .await
        .expect("kill_process");
    assert!(killed.contains("has been killed"), "got: {killed}");

    tokio::time::sleep(Duration::from_secs(1)).await;

    let output = GetProcessOutputTool
        .execute(json!({"process_id": process_id}), &ctx)
        .await
        .expect("get_process_output");
    assert!(output.contains("killed"), "got: {output}");
    assert!(
        output.contains("start") || output.contains("no output yet"),
        "got: {output}"
    );

    unsafe { std::env::remove_var("EDGECRAB_DAYTONA_PYTHON") };
    let _ = cleanup_backend_for_task(&ctx.task_id).await;
}

#[cfg(unix)]
#[tokio::test]
async fn e2e_managed_modal_remote_run_process_waits_and_collects_output() {
    let _guard = lock_backend_env();
    let dir = TempDir::new().expect("tmpdir");
    let server = FakeModalServer::start();
    let gateway_origin = server.base_url.trim_end_matches("/v1").to_string();

    let mut ctx = make_ctx(dir.path());
    ctx.task_id = format!(
        "e2e-managed-modal-process-{}",
        uuid::Uuid::new_v4().simple()
    );
    ctx.config.terminal_backend = BackendKind::Modal;
    ctx.config.terminal_modal.mode = ModalTransportMode::Managed;
    ctx.config.terminal_modal.image = "fake-managed-image".into();
    ctx.config.terminal_modal.managed_gateway_url = Some(gateway_origin);
    ctx.config.terminal_modal.managed_user_token = Some("managed-user-token".into());
    ctx.process_table = Some(Arc::new(ProcessTable::new()));

    let started = RunProcessTool
        .execute(
            json!({"command": "printf 'alpha\\n'; sleep 1; printf 'omega\\n'"}),
            &ctx,
        )
        .await
        .expect("run_process");
    let process_id = extract_process_id(&started);

    let waited = WaitForProcessTool
        .execute(json!({"process_id": process_id, "timeout_secs": 10}), &ctx)
        .await
        .expect("wait_for_process");

    assert!(waited.contains("exited (code 0)"), "got: {waited}");
    assert!(waited.contains("alpha"), "missing alpha: {waited}");
    assert!(waited.contains("omega"), "missing omega: {waited}");

    let _ = cleanup_backend_for_task(&ctx.task_id).await;
}

#[cfg(unix)]
#[tokio::test]
async fn e2e_singularity_backend_via_fake_runtime() {
    let _guard = lock_backend_env();
    let dir = TempDir::new().expect("tmpdir");
    let fake = write_executable_script(
        &dir,
        "fake-apptainer",
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  version) echo "apptainer 1.0.0" ;;
  instance) exit 0 ;;
  exec)
    last=""
    for arg in "$@"; do last="$arg"; done
    printf 'singularity:%s\n' "$last"
    ;;
esac
"#,
    );

    let mut ctx = make_ctx(dir.path());
    ctx.config.terminal_backend = BackendKind::Singularity;
    unsafe { std::env::set_var("EDGECRAB_SINGULARITY_BIN", &fake) };

    let result = TerminalTool
        .execute(json!({"command": "echo hello-singularity"}), &ctx)
        .await
        .expect("execute");
    assert!(
        result.contains("singularity:mkdir -p '/tmp/edgecrab-tmp'"),
        "got: {result}"
    );
    assert!(
        result.contains("EDGECRAB_TMPDIR='/tmp/edgecrab-tmp'"),
        "got: {result}"
    );
    assert!(
        result.contains("&& echo hello-singularity"),
        "got: {result}"
    );

    unsafe { std::env::remove_var("EDGECRAB_SINGULARITY_BIN") };
}

#[cfg(unix)]
#[tokio::test]
async fn e2e_remote_run_process_waits_and_collects_output() {
    let _guard = lock_backend_env();
    let dir = TempDir::new().expect("tmpdir");
    let fake = write_executable_script(
        &dir,
        "fake-apptainer-bg",
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  version) echo "apptainer 1.0.0" ;;
  instance) exit 0 ;;
  exec)
    last=""
    prev=""
    workdir=""
    for arg in "$@"; do
      if [ "$prev" = "--pwd" ]; then workdir="$arg"; fi
      last="$arg"
      prev="$arg"
    done
    if [ -n "$workdir" ]; then cd "$workdir"; fi
    sh -c "$last"
    ;;
esac
"#,
    );

    let mut ctx = make_ctx(dir.path());
    ctx.task_id = format!("e2e-remote-process-{}", uuid::Uuid::new_v4().simple());
    ctx.config.terminal_backend = BackendKind::Singularity;
    ctx.process_table = Some(Arc::new(ProcessTable::new()));
    unsafe { std::env::set_var("EDGECRAB_SINGULARITY_BIN", &fake) };

    let started = RunProcessTool
        .execute(
            json!({"command": "printf 'alpha\\n'; sleep 1; printf 'omega\\n'"}),
            &ctx,
        )
        .await
        .expect("run_process");
    let process_id = extract_process_id(&started);

    let waited = WaitForProcessTool
        .execute(json!({"process_id": process_id, "timeout_secs": 10}), &ctx)
        .await
        .expect("wait_for_process");

    assert!(waited.contains("exited (code 0)"), "got: {waited}");
    assert!(waited.contains("alpha"), "missing alpha: {waited}");
    assert!(waited.contains("omega"), "missing omega: {waited}");

    unsafe { std::env::remove_var("EDGECRAB_SINGULARITY_BIN") };
    let _ = cleanup_backend_for_task(&ctx.task_id).await;
}

#[cfg(unix)]
#[tokio::test]
async fn e2e_remote_run_process_can_be_killed() {
    let _guard = lock_backend_env();
    let dir = TempDir::new().expect("tmpdir");
    let fake = write_executable_script(
        &dir,
        "fake-apptainer-kill",
        r#"#!/bin/sh
set -eu
case "${1:-}" in
  version) echo "apptainer 1.0.0" ;;
  instance) exit 0 ;;
  exec)
    last=""
    prev=""
    workdir=""
    for arg in "$@"; do
      if [ "$prev" = "--pwd" ]; then workdir="$arg"; fi
      last="$arg"
      prev="$arg"
    done
    if [ -n "$workdir" ]; then cd "$workdir"; fi
    sh -c "$last"
    ;;
esac
"#,
    );

    let mut ctx = make_ctx(dir.path());
    ctx.task_id = format!("e2e-remote-kill-{}", uuid::Uuid::new_v4().simple());
    ctx.config.terminal_backend = BackendKind::Singularity;
    ctx.process_table = Some(Arc::new(ProcessTable::new()));
    unsafe { std::env::set_var("EDGECRAB_SINGULARITY_BIN", &fake) };

    let started = RunProcessTool
        .execute(json!({"command": "printf 'start\\n'; sleep 30"}), &ctx)
        .await
        .expect("run_process");
    let process_id = extract_process_id(&started);

    tokio::time::sleep(Duration::from_secs(1)).await;

    let killed = KillProcessTool
        .execute(json!({"process_id": process_id}), &ctx)
        .await
        .expect("kill_process");
    assert!(killed.contains("has been killed"), "got: {killed}");

    tokio::time::sleep(Duration::from_secs(1)).await;

    let output = GetProcessOutputTool
        .execute(json!({"process_id": process_id}), &ctx)
        .await
        .expect("get_process_output");
    assert!(output.contains("killed"), "got: {output}");
    assert!(
        output.contains("start") || output.contains("no output yet"),
        "got: {output}"
    );

    unsafe { std::env::remove_var("EDGECRAB_SINGULARITY_BIN") };
    let _ = cleanup_backend_for_task(&ctx.task_id).await;
}

// ─── ExecOutput unit tests ───────────────────────────────────────────

#[test]
fn exec_output_format_stdout_only_no_suffix() {
    let o = ExecOutput {
        stdout: "hello\n".into(),
        stderr: String::new(),
        exit_code: 0,
    };
    let s = o.format(usize::MAX, usize::MAX);
    assert_eq!(s, "hello\n");
}

#[test]
fn exec_output_format_includes_stderr_block() {
    let o = ExecOutput {
        stdout: "out\n".into(),
        stderr: "err\n".into(),
        exit_code: 0,
    };
    let s = o.format(usize::MAX, usize::MAX);
    assert!(s.contains("[stderr]\nerr\n"));
    assert!(s.contains("out\n"));
}

#[test]
fn exec_output_format_nonzero_adds_exit_code_line() {
    let o = ExecOutput {
        stdout: "x\n".into(),
        stderr: String::new(),
        exit_code: 42,
    };
    let s = o.format(usize::MAX, usize::MAX);
    assert!(s.contains("[exit code: 42]"), "got: {s}");
}

#[test]
fn exec_output_format_truncates_stdout() {
    let long = "a".repeat(500);
    let o = ExecOutput {
        stdout: long,
        stderr: String::new(),
        exit_code: 0,
    };
    let s = o.format(100, usize::MAX);
    // head+tail split: omit marker says "bytes omitted", NOT "truncated".
    assert!(s.contains("omitted"), "expected omit marker; got: {s}");
}

// ─── BackendKind tests ────────────────────────────────────────────────

#[test]
fn backend_kind_display_roundtrip() {
    for kind in [
        BackendKind::Local,
        BackendKind::Docker,
        BackendKind::Ssh,
        BackendKind::Modal,
        BackendKind::Daytona,
        BackendKind::Singularity,
    ] {
        let s = kind.to_string();
        let parsed: BackendKind = s.parse().expect("infallible");
        assert_eq!(parsed, kind, "roundtrip failed for {s}");
    }
}

#[test]
fn backend_kind_serde_roundtrip() {
    let kinds = vec![
        BackendKind::Local,
        BackendKind::Docker,
        BackendKind::Ssh,
        BackendKind::Modal,
        BackendKind::Daytona,
        BackendKind::Singularity,
    ];
    let json_str = serde_json::to_string(&kinds).expect("serialize");
    let parsed: Vec<BackendKind> = serde_json::from_str(&json_str).expect("deserialize");
    assert_eq!(kinds, parsed);
}

#[test]
fn backend_kind_default_is_local() {
    assert_eq!(BackendKind::default(), BackendKind::Local);
}
