//! Dev-server port hints for `http.server` / static preview workflows (spec 015 P1.7).
//!
//! Port-bind truth (018 P6): session ports are recorded only after a **TCP listen
//! probe** succeeds. Spawn text and log lines alone never imply a known port.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::LazyLock;
use std::time::Duration;

use dashmap::DashMap;

static SESSION_HTTP_PORTS: LazyLock<DashMap<String, Vec<u16>>> = LazyLock::new(DashMap::new);

/// Pending preview binds: session → (port, process_id) while TCP not yet ready.
static SESSION_PENDING_PREVIEW: LazyLock<DashMap<String, Vec<(u16, String)>>> =
    LazyLock::new(DashMap::new);

/// Remember an http.server port **after TCP listen probe** (not at spawn).
///
/// Optimistic spawn-time recording is forbidden — it suppresses navigate recovery
/// when the bind later fails (game005 / `ERR_EMPTY_RESPONSE` forensics).
pub fn record_session_http_server(session_id: &str, command: &str) {
    let Some(port) = infer_http_server_port(command) else {
        return;
    };
    record_session_http_port_if_listening(session_id, port);
}

/// Record a port only when loopback TCP connect succeeds.
pub fn record_session_http_port_if_listening(session_id: &str, port: u16) {
    if !probe_loopback_http_port(port) {
        return;
    }
    record_session_http_port_unchecked(session_id, port);
}

/// Record a port without probing (tests / explicit verified callers only).
pub fn record_session_http_port_unchecked(session_id: &str, port: u16) {
    if port == 0 {
        return;
    }
    let mut entry = SESSION_HTTP_PORTS
        .entry(session_id.to_string())
        .or_default();
    if !entry.contains(&port) {
        entry.push(port);
        entry.sort_unstable();
    }
}

/// Backward-compatible name — now requires a successful listen probe.
pub fn record_session_http_port(session_id: &str, port: u16) {
    record_session_http_port_if_listening(session_id, port);
}

/// TCP connect to `127.0.0.1:port` (and `::1` fallback) with a short timeout.
pub fn probe_loopback_http_port(port: u16) -> bool {
    if port == 0 {
        return false;
    }
    let timeout = Duration::from_millis(200);
    let candidates = [format!("127.0.0.1:{port}"), format!("[::1]:{port}")];
    for cand in candidates {
        if let Ok(addrs) = cand.to_socket_addrs() {
            for addr in addrs {
                if tcp_connect_timeout(addr, timeout) {
                    return true;
                }
            }
        }
    }
    false
}

fn tcp_connect_timeout(addr: SocketAddr, timeout: Duration) -> bool {
    TcpStream::connect_timeout(&addr, timeout).is_ok()
}

/// Drop a previously recorded port (bind failed, process exited, or stale).
pub fn unrecord_session_http_port(session_id: &str, port: u16) {
    if port == 0 {
        return;
    }
    let Some(mut entry) = SESSION_HTTP_PORTS.get_mut(session_id) else {
        return;
    };
    entry.retain(|p| *p != port);
}

/// Unrecord the port inferred from a preview-server command (if any).
pub fn unrecord_session_http_server(session_id: &str, command: &str) {
    if let Some(port) = infer_http_server_port(command) {
        unrecord_session_http_port(session_id, port);
    }
}

/// True when terminal/process output shows bind failure (EADDRINUSE / errno 48).
pub fn is_address_already_in_use(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("address already in use")
        || lower.contains("errno 48")
        || lower.contains("[errno 48]")
        || lower.contains("eaddrinuse")
}

/// Infer loopback port from a URL like `http://127.0.0.1:8000/…`.
pub fn port_from_loopback_url(url: &str) -> Option<u16> {
    let lower = url.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| lower.strip_prefix("http://localhost:"))
        .or_else(|| lower.strip_prefix("http://[::1]:"))?;
    let port_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    port_str.parse().ok().filter(|p| *p > 0)
}

/// Scan shell commands, Python snippets, or mixed text for http.server ports.
pub fn collect_http_server_ports_from_text(text: &str) -> Vec<u16> {
    let mut ports = collect_http_server_ports(text.lines());
    if let Some(port) = infer_http_server_port(text)
        && !ports.contains(&port)
    {
        ports.push(port);
    }
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// Probe-and-record ports mentioned in text (never records unbound ports).
///
/// Prefer calling after the process is expected to listen — not at code submit.
pub fn record_session_http_from_text(session_id: &str, text: &str) {
    for port in collect_http_server_ports_from_text(text) {
        record_session_http_port_if_listening(session_id, port);
    }
}

/// Ports inferred from http.server commands run in this session.
pub fn session_http_server_ports(session_id: &str) -> Vec<u16> {
    SESSION_HTTP_PORTS
        .get(session_id)
        .map(|v| v.clone())
        .unwrap_or_default()
}

/// Merge process-table and session-inferred dev-server ports (sorted, deduped).
pub fn merge_dev_server_ports(session_id: &str, process_ports: &[u16]) -> Vec<u16> {
    let mut ports: Vec<u16> = process_ports.to_vec();
    for p in session_http_server_ports(session_id) {
        if !ports.contains(&p) {
            ports.push(p);
        }
    }
    ports.sort_unstable();
    ports
}

/// Collect unique inferred ports from a set of shell commands (sorted ascending).
pub fn collect_http_server_ports<'a, I>(commands: I) -> Vec<u16>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut ports: Vec<u16> = commands
        .into_iter()
        .filter_map(infer_http_server_port)
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// Human-readable hint listing active dev-server ports for browser preview recovery.
pub fn format_dev_server_ports_hint(ports: &[u16]) -> Option<String> {
    if ports.is_empty() {
        return None;
    }
    let urls: Vec<String> = ports
        .iter()
        .map(|p| format!("http://127.0.0.1:{p}/"))
        .collect();
    Some(format!(
        "Detected running http.server on port(s): {} — navigate to one of these URLs, not an arbitrary port.",
        urls.join(", ")
    ))
}

/// True when a shell/run_process command starts a static HTTP preview server.
///
/// Argv-shaped detection only — never bare `contains("serve")` (018 F3).
pub fn is_preview_server_command(command: &str) -> bool {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    if tokens.is_empty() {
        return false;
    }
    // `python[3] -m http.server …`
    for i in 0..tokens.len().saturating_sub(2) {
        let py = tokens[i].rsplit('/').next().unwrap_or(tokens[i]);
        if matches!(py, "python" | "python3")
            && tokens[i + 1] == "-m"
            && tokens[i + 2].contains("http.server")
        {
            return true;
        }
    }
    // `npx|pnpm dlx|yarn dlx [flags] serve|http-server|live-server …`
    let lower_tokens: Vec<String> = tokens.iter().map(|t| t.to_ascii_lowercase()).collect();
    let Some(li) = lower_tokens
        .iter()
        .position(|t| matches!(t.as_str(), "npx" | "pnpm" | "yarn"))
    else {
        return false;
    };
    let launcher = lower_tokens[li].as_str();
    if matches!(launcher, "pnpm" | "yarn") {
        let next = lower_tokens.get(li + 1).map(String::as_str);
        if !matches!(next, Some("dlx") | Some("exec")) {
            return false;
        }
    }
    lower_tokens.iter().skip(li + 1).any(|t| {
        matches!(
            t.as_str(),
            "serve" | "http-server" | "live-server" | "@vercel/serve"
        )
    })
}

/// Extract a shell `command` field from terminal / run_process tool args JSON.
pub fn command_from_tool_args_json(args_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    v.get("command")
        .and_then(|c| c.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            // execute_code may embed http.server in `code`
            v.get("code")
                .and_then(|c| c.as_str())
                .filter(|s| is_preview_server_command(s))
                .map(str::to_string)
        })
}

/// Infer bound port from a shell command launching Python's http.server.
pub fn infer_http_server_port(command: &str) -> Option<u16> {
    let lower = command.to_ascii_lowercase();
    if !lower.contains("http.server") {
        return None;
    }
    // `python -m http.server 8000` or `--bind 127.0.0.1 8000`
    let tokens: Vec<&str> = command.split_whitespace().collect();
    for (i, token) in tokens.iter().enumerate() {
        if token.contains("http.server")
            && let Some(next) = tokens.get(i + 1)
            && let Ok(port) = next.parse::<u16>()
            && port > 0
        {
            return Some(port);
        }
        if *token == "--bind" || *token == "-b" {
            if let Some(port_token) = tokens.get(i + 2)
                && let Ok(port) = port_token.parse::<u16>()
            {
                return Some(port);
            }
            if let Some(port_token) = tokens.get(i + 1)
                && let Ok(port) = port_token.parse::<u16>()
            {
                return Some(port);
            }
        }
    }
    // No invented default port (018 F3) — require an explicit port token.
    if lower.contains("http.server") {
        return infer_port_near_http_server(command);
    }
    None
}

/// Scan for a port literal near `http.server` in Python/shell snippets (execute_code, subprocess).
fn infer_port_near_http_server(text: &str) -> Option<u16> {
    let lower = text.to_ascii_lowercase();
    let idx = lower.find("http.server")?;
    let tail = &text[idx + "http.server".len()..];
    for token in tail.split(|c: char| !c.is_ascii_digit()) {
        if (2..=5).contains(&token.len())
            && let Ok(port) = token.parse::<u16>()
            && port > 0
        {
            return Some(port);
        }
    }
    None
}

/// Poll loopback TCP until ready or timeout (bind latch — 006 pptx / game005).
pub fn await_bind_ready(port: u16, timeout: Duration) -> bool {
    if port == 0 {
        return false;
    }
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if probe_loopback_http_port(port) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Mark a preview spawn as pending bind for this session.
pub fn mark_pending_preview(session_id: &str, port: u16, process_id: &str) {
    if session_id.is_empty() || port == 0 {
        return;
    }
    let mut entry = SESSION_PENDING_PREVIEW
        .entry(session_id.to_string())
        .or_default();
    entry.retain(|(p, _)| *p != port);
    entry.push((port, process_id.to_string()));
}

/// Clear pending preview for a port (bound, failed, or reused).
pub fn clear_pending_preview(session_id: &str, port: u16) {
    if let Some(mut entry) = SESSION_PENDING_PREVIEW.get_mut(session_id) {
        entry.retain(|(p, _)| *p != port);
    }
}

/// Pending (port, process_id) pairs that have not yet passed TCP probe.
pub fn pending_preview_binds(session_id: &str) -> Vec<(u16, String)> {
    SESSION_PENDING_PREVIEW
        .get(session_id)
        .map(|v| v.clone())
        .unwrap_or_default()
}

/// True when a session already has a listening (or recorded) server on `port`.
pub fn preview_port_already_bound(session_id: &str, port: u16) -> bool {
    if port == 0 {
        return false;
    }
    if session_http_server_ports(session_id).contains(&port) {
        return true;
    }
    probe_loopback_http_port(port)
}

/// Cap: refuse a second preview spawn on the same port — return reuse JSON instead.
pub fn preview_reuse_result(session_id: &str, port: u16, command: &str) -> Option<String> {
    if !is_preview_server_command(command) {
        return None;
    }
    if !preview_port_already_bound(session_id, port) {
        return None;
    }
    record_session_http_port_if_listening(session_id, port);
    clear_pending_preview(session_id, port);
    Some(
        serde_json::json!({
            "ok": true,
            "reused": true,
            "bind_ready": true,
            "port": port,
            "process_id": null,
            "command": command,
            "note": format!(
                "Preview server already listening on port {port} — reused existing bind \
                 (no second spawn). Navigate to http://127.0.0.1:{port}/"
            ),
        })
        .to_string(),
    )
}

/// Enrich background spawn JSON with `bind_ready` + `port` after a short TCP poll.
///
/// Call after process spawn for preview commands. Records the port only when ready.
pub fn finalize_preview_spawn_result(
    session_id: &str,
    command: &str,
    process_id: &str,
    body: &str,
) -> String {
    let Some(port) = infer_http_server_port(command) else {
        return append_spawn_hint_text(command, body);
    };
    mark_pending_preview(session_id, port, process_id);
    let bind_ready = await_bind_ready(port, Duration::from_millis(1500));
    let mut v: serde_json::Value = serde_json::from_str(body).unwrap_or_else(|_| {
        serde_json::json!({
            "ok": true,
            "process_id": process_id,
            "command": command,
        })
    });
    if let Some(obj) = v.as_object_mut() {
        obj.insert("port".into(), serde_json::json!(port));
        obj.insert("bind_ready".into(), serde_json::json!(bind_ready));
        if bind_ready {
            record_session_http_port_unchecked(session_id, port);
            clear_pending_preview(session_id, port);
            obj.insert(
                "preview_url".into(),
                serde_json::json!(format!("http://127.0.0.1:{port}/")),
            );
            obj.insert(
                "note".into(),
                serde_json::json!(format!(
                    "bind_ready=true — navigate to http://127.0.0.1:{port}/ then browser_snapshot"
                )),
            );
        } else {
            obj.insert(
                "note".into(),
                serde_json::json!(format!(
                    "bind_ready=false — TCP not listening yet on :{port}. \
                     Call wait_for_process or retry browser_navigate once after bind; \
                     do not spawn another http.server on the same port."
                )),
            );
        }
    }
    let json = v.to_string();
    if bind_ready {
        json
    } else {
        append_spawn_hint_text(command, &json)
    }
}

/// Append harness hint to run_process / terminal JSON/text results.
///
/// Honesty: spawn ≠ listening. Port is recorded only after TCP listen probe.
pub fn append_spawn_hint(command: &str, body: &str) -> String {
    append_spawn_hint_text(command, body)
}

fn append_spawn_hint_text(command: &str, body: &str) -> String {
    let Some(port) = infer_http_server_port(command) else {
        return body.to_string();
    };
    let url = format!("http://127.0.0.1:{port}/");
    if body.contains("bind_ready")
        || body.contains("bind-ready")
        || body.contains(&format!("ready — preview at {url}"))
    {
        return body.to_string();
    }
    format!(
        "{body}\n\n[harness] Dev server spawn requested for {url} — wait for TCP bind-ready \
         before browser_navigate. Port is not trusted until a listen probe succeeds. \
         Enable security.preview in config if localhost is blocked."
    )
}

/// Ready notice when the inferred port accepts a loopback TCP connect (018 F3).
///
/// English log lines are never required — TCP listen is the law.
pub fn maybe_http_server_ready_notice(command: &str, _line: &str) -> Option<String> {
    let port = infer_http_server_port(command)?;
    if !probe_loopback_http_port(port) {
        return None;
    }
    Some(format!(
        "✓ http.server ready — preview at http://127.0.0.1:{port}/ (enable security.preview)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ha20c_infers_port_from_http_server_command() {
        assert_eq!(
            infer_http_server_port("python3 -m http.server 8888"),
            Some(8888)
        );
        assert_eq!(
            infer_http_server_port("python -m http.server --bind 127.0.0.1 8000"),
            Some(8000)
        );
        assert_eq!(infer_http_server_port("cargo test"), None);
        // No invented default when port token is absent.
        assert_eq!(infer_http_server_port("python3 -m http.server"), None);
        assert_eq!(
            infer_http_server_port("python3 -m http.server --directory demo"),
            None
        );
    }

    #[test]
    fn preview_server_command_detects_http_server_and_serve() {
        assert!(is_preview_server_command(
            "python3 -m http.server 8000 --directory demo/game002"
        ));
        assert!(is_preview_server_command("npx --yes serve . -l 3000"));
        assert!(!is_preview_server_command("ls -la demo/game002"));
        assert!(!is_preview_server_command("cargo test"));
        // Bare "serve" in unrelated paths must not match.
        assert!(!is_preview_server_command(
            "cat demo/game002/observe_server_notes.txt"
        ));
    }

    #[test]
    fn command_from_tool_args_json_reads_command_field() {
        let cmd = command_from_tool_args_json(
            r#"{"command":"python3 -m http.server 8000","background":true}"#,
        )
        .expect("command");
        assert!(is_preview_server_command(&cmd));
    }

    #[test]
    fn ha20c_spawn_hint_includes_url() {
        let out = append_spawn_hint(
            "python3 -m http.server 8000",
            r#"{"ok":true,"process_id":"proc-1"}"#,
        );
        assert!(out.contains("127.0.0.1:8000"));
        assert!(out.contains("security.preview"));
        assert!(out.contains("bind-ready"));
        assert!(out.contains("listen probe"));
    }

    #[test]
    fn preview_reuse_when_port_already_bound() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let sid = format!("sess-reuse-{port}");
        let cmd = format!("python3 -m http.server {port}");
        let reuse = preview_reuse_result(&sid, port, &cmd).expect("reuse");
        let v: serde_json::Value = serde_json::from_str(&reuse).expect("json");
        assert_eq!(v["bind_ready"], true);
        assert_eq!(v["reused"], true);
        assert_eq!(v["port"], port);
        drop(listener);
    }

    #[test]
    fn finalize_preview_spawn_sets_bind_ready_field() {
        let sid = "sess-finalize-bind";
        let out = finalize_preview_spawn_result(
            sid,
            "python3 -m http.server 1",
            "proc-test",
            r#"{"ok":true,"process_id":"proc-test","command":"python3 -m http.server 1"}"#,
        );
        // Port 1 will not bind — bind_ready false still present.
        assert!(out.contains("bind_ready"), "got: {out}");
        assert!(
            out.contains("\"port\":1") || out.contains("\"port\": 1"),
            "got: {out}"
        );
        clear_pending_preview(sid, 1);
    }

    #[test]
    fn port_bind_truth_unrecord_session_http() {
        let sid = "sess-unrecord-port-truth";
        record_session_http_port_unchecked(sid, 8111);
        assert_eq!(session_http_server_ports(sid), vec![8111]);
        unrecord_session_http_server(sid, "python3 -m http.server 8111");
        assert!(session_http_server_ports(sid).is_empty());
    }

    #[test]
    fn probe_loopback_http_port_requires_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        assert!(probe_loopback_http_port(port));
        drop(listener);
        // Port freed — probe must fail (best-effort; OS may linger).
        // Unbound high port should fail.
        assert!(!probe_loopback_http_port(1));
    }

    #[test]
    fn record_if_listening_skips_unbound_port() {
        let sid = "sess-probe-unbound";
        record_session_http_port_if_listening(sid, 1);
        assert!(session_http_server_ports(sid).is_empty());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        record_session_http_port_if_listening(sid, port);
        assert_eq!(session_http_server_ports(sid), vec![port]);
        drop(listener);
        unrecord_session_http_port(sid, port);
    }

    #[test]
    fn address_already_in_use_detects_errno48() {
        assert!(is_address_already_in_use(
            "OSError: [Errno 48] Address already in use"
        ));
        assert!(is_address_already_in_use("EADDRINUSE: bind failed"));
        assert!(!is_address_already_in_use("Serving HTTP on :: port 8000"));
    }

    #[test]
    fn port_from_loopback_url_parses() {
        assert_eq!(
            port_from_loopback_url("http://127.0.0.1:8000/index.html"),
            Some(8000)
        );
        assert_eq!(port_from_loopback_url("https://example.com"), None);
    }

    #[test]
    fn collect_ports_dedupes_and_sorts() {
        let cmds = [
            "python3 -m http.server 8888",
            "python3 -m http.server 8000",
            "python3 -m http.server 8888",
        ];
        assert_eq!(
            collect_http_server_ports(cmds.iter().copied()),
            vec![8000, 8888]
        );
    }

    #[test]
    fn format_ports_hint_lists_urls() {
        let hint = format_dev_server_ports_hint(&[8000, 8888]).expect("hint");
        assert!(hint.contains("127.0.0.1:8000"));
        assert!(hint.contains("127.0.0.1:8888"));
    }

    #[test]
    fn ha26_ready_notice_requires_tcp_listen() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let cmd = format!("python3 -m http.server {port}");
        let notice = maybe_http_server_ready_notice(&cmd, "irrelevant log line");
        assert!(notice.is_some_and(|n| n.contains(&port.to_string())));
        drop(listener);
        assert!(maybe_http_server_ready_notice(&cmd, "Serving HTTP…").is_none());
    }

    #[test]
    fn session_ports_persist_for_browser_preview() {
        record_session_http_port_unchecked("sess-a", 7777);
        assert_eq!(session_http_server_ports("sess-a"), vec![7777]);
        assert_eq!(merge_dev_server_ports("sess-a", &[8000]), vec![7777, 8000]);
    }

    #[test]
    fn execute_code_snippet_does_not_record_unbound_port() {
        // Pick an ephemeral port that is not listening after we drop the bind.
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            let port = listener.local_addr().expect("addr").port();
            drop(listener);
            port
        };
        let code = format!(
            r#"
import subprocess
subprocess.Popen(["python3", "-m", "http.server", "{port}"])
"#
        );
        let sid = format!("sess-exec-no-spawn-record-{port}");
        record_session_http_from_text(&sid, &code);
        // Port truth: text alone never records without a listen probe.
        assert!(
            !session_http_server_ports(&sid).contains(&port),
            "unbound port {port} must not be recorded"
        );
    }
}
