//! Dev-server port hints for `http.server` / static preview workflows (spec 015 P1.7).

use std::sync::LazyLock;

use dashmap::DashMap;

static SESSION_HTTP_PORTS: LazyLock<DashMap<String, Vec<u16>>> = LazyLock::new(DashMap::new);

/// Remember an http.server port started in this session (foreground terminal or background).
pub fn record_session_http_server(session_id: &str, command: &str) {
    let Some(port) = infer_http_server_port(command) else {
        return;
    };
    record_session_http_port(session_id, port);
}

/// Record a dev-server port directly (e.g. inferred from execute_code payload).
pub fn record_session_http_port(session_id: &str, port: u16) {
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

/// Remember ports embedded in execute_code / subprocess snippets for this session.
pub fn record_session_http_from_text(session_id: &str, text: &str) {
    for port in collect_http_server_ports_from_text(text) {
        record_session_http_port(session_id, port);
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
    if lower.contains("http.server") {
        if let Some(port) = infer_port_near_http_server(command) {
            return Some(port);
        }
        return Some(8000);
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

/// Append harness hint to run_process / terminal JSON/text results.
pub fn append_spawn_hint(command: &str, body: &str) -> String {
    let Some(port) = infer_http_server_port(command) else {
        return body.to_string();
    };
    let url = format!("http://127.0.0.1:{port}/");
    if body.contains(&url) {
        return body.to_string();
    }
    format!(
        "{body}\n\n[harness] Dev server expected at {url} — enable security.preview in config \
         before browser_navigate."
    )
}

/// Detect Python http.server "Serving HTTP" ready line for shelf notify (HA-26).
pub fn maybe_http_server_ready_notice(command: &str, line: &str) -> Option<String> {
    let port = infer_http_server_port(command)?;
    let lower = line.to_ascii_lowercase();
    if lower.contains("serving http") || lower.contains("serving http on") {
        Some(format!(
            "✓ http.server ready — preview at http://127.0.0.1:{port}/ (enable security.preview)"
        ))
    } else {
        None
    }
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
    }

    #[test]
    fn ha20c_spawn_hint_includes_url() {
        let out = append_spawn_hint(
            "python3 -m http.server 8000",
            r#"{"ok":true,"process_id":"proc-1"}"#,
        );
        assert!(out.contains("127.0.0.1:8000"));
        assert!(out.contains("security.preview"));
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
    fn ha26_detects_serving_http_line() {
        let notice = maybe_http_server_ready_notice(
            "python3 -m http.server 8000",
            "Serving HTTP on :: port 8000 (http://[::]:8000/) ...",
        );
        assert!(notice.is_some_and(|n| n.contains("8000")));
    }

    #[test]
    fn session_ports_persist_for_browser_preview() {
        record_session_http_server("sess-a", "python3 -m http.server 7777");
        assert_eq!(session_http_server_ports("sess-a"), vec![7777]);
        assert_eq!(merge_dev_server_ports("sess-a", &[8000]), vec![7777, 8000]);
    }

    #[test]
    fn execute_code_snippet_records_http_server_port() {
        let code = r#"
import subprocess
subprocess.Popen(["python3", "-m", "http.server", "8765"])
"#;
        record_session_http_from_text("sess-exec", code);
        assert!(session_http_server_ports("sess-exec").contains(&8765));
    }
}
