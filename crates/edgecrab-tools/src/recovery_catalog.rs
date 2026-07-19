//! Domain-specific self-reflective recovery catalogs for EdgeCrab tools.
//!
//! Validators emit concise diagnoses; this module supplies structured
//! `recovery_feedback.suggestions[]` payloads the agent can merge on retry.
//! Single responsibility: map EdgeCrab policy rejections → recovery actions.

use edgecrab_types::{RecoveryAction, RecoveryFeedbackBuilder, ToolError};
use serde_json::json;

fn recovery_guidance() -> RecoveryFeedbackBuilder {
    RecoveryFeedbackBuilder::new("recovery_guidance")
}

/// Path already exists — agent intended to create a new file.
pub fn write_file_path_exists_abort(path: String, size_bytes: u64) -> ToolError {
    ToolError::InvalidArgs {
        tool: "write_file".into(),
        message: format!(
            "'{path}' already exists ({size_bytes} bytes). \
             Target path is occupied — choose another path or overwrite explicitly."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Target path already exists")
            .suggestion(
                RecoveryAction::UseDifferentPath,
                json!({ "tool": "write_file", "path": path.clone() }),
            )
            .suggestion(
                RecoveryAction::SetParameter,
                json!({
                    "tool": "write_file",
                    "path": path,
                    "if_exists": "overwrite"
                }),
            )
            .build(),
    )
}

/// Overwrite guard — snapshot recorded; retry or switch to patch.
pub fn write_file_overwrite_guard(path: String, preview: String, truncated: bool) -> ToolError {
    let trunc_note = if truncated {
        "\n[Preview truncated — read_file returns full content when needed.]"
    } else {
        ""
    };
    ToolError::InvalidArgs {
        tool: "write_file".into(),
        message: format!(
            "'{path}' already exists and requires an explicit overwrite decision.\n\
             Snapshot recorded for freshness.\n\
             --- preview ---\n{preview}{trunc_note}\n---"
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Existing file requires overwrite confirmation")
            .suggestion(
                RecoveryAction::RetrySameCall,
                json!({
                    "tool": "write_file",
                    "path": path.clone(),
                    "if_exists": "overwrite",
                    "note": "read snapshot already recorded — retry same call without read_file"
                }),
            )
            .suggestion(
                RecoveryAction::SwitchTool,
                json!({
                    "from_tool": "write_file",
                    "to_tool": "patch",
                    "path": path,
                    "reason": "targeted edits are more token-efficient than full overwrite"
                }),
            )
            .build(),
    )
}

/// File changed since last read — stale cached context guard.
pub fn stale_file_context(tool: &str, display_path: &str) -> ToolError {
    ToolError::InvalidArgs {
        tool: tool.into(),
        message: format!(
            "'{display_path}' changed since it was last read in this session. \
             Cached context may be stale."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("File modified since last read")
            .suggestion(
                RecoveryAction::CallToolFirst,
                json!({
                    "tool": "read_file",
                    "path": display_path,
                    "then_retry": tool
                }),
            )
            .build(),
    )
}

/// Single-call mutation payload exceeds configured limit.
pub fn mutation_payload_too_large(
    tool_name: &str,
    path: &str,
    bytes: usize,
    max_bytes: usize,
    creating: bool,
) -> ToolError {
    let max_kib = max_bytes / 1024;
    ToolError::InvalidArgs {
        tool: tool_name.into(),
        message: format!(
            "Refusing {tool_name} for '{path}' ({bytes} bytes > {max_bytes} bytes / {max_kib} KiB). \
             Payload exceeds the per-call mutation limit."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Mutation payload too large for one tool call")
            .suggestion(
                RecoveryAction::SplitPayload,
                json!({
                    "tool": tool_name,
                    "path": path,
                    "max_bytes": max_bytes,
                    "strategy": if creating {
                        "write minimal scaffold with write_file, then grow with patch/apply_patch"
                    } else {
                        "split into smaller focused patch/apply_patch steps"
                    }
                }),
            )
            .suggestion(
                RecoveryAction::SwitchTool,
                json!({
                    "from_tool": tool_name,
                    "to_tool": "patch",
                    "path": path
                }),
            )
            .build(),
    )
}

/// TOCTOU content mismatch during write.
pub fn write_file_content_mismatch(path: String) -> ToolError {
    ToolError::ContentMismatch {
        tool: "write_file".into(),
        path: path.clone(),
        message: format!(
            "'{path}' changed on disk while the write was being prepared. \
             Re-read the current file before mutating."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("File changed during write (TOCTOU)")
            .suggestion(
                RecoveryAction::CallToolFirst,
                json!({
                    "tool": "read_file",
                    "path": path,
                    "then_retry": "write_file"
                }),
            )
            .build(),
    )
}

/// Tool-call JSON exceeded the derived one-completion budget (pre-dispatch guard).
pub fn tool_argument_budget_exceeded(
    tool_name: &str,
    argument_bytes: usize,
    max_bytes: usize,
    estimated_tokens: usize,
) -> ToolError {
    let max_kib = max_bytes / 1024;
    ToolError::InvalidArgs {
        tool: tool_name.into(),
        message: format!(
            "Refusing {tool_name}: argument payload is {argument_bytes} bytes (~{estimated_tokens} tokens) \
             but the one-completion budget is {max_bytes} bytes ({max_kib} KiB). \
             Split into scaffold + patch steps."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Tool argument too large for one completion")
            .suggestion(
                RecoveryAction::SplitPayload,
                json!({
                    "tool": tool_name,
                    "max_bytes": max_bytes,
                    "strategy": "write minimal scaffold, then patch/apply_patch in ≤{max_kib} KiB chunks"
                }),
            )
            .suggestion(
                RecoveryAction::SwitchTool,
                json!({
                    "from_tool": tool_name,
                    "to_tool": "patch",
                    "recommended_tools": ["patch"],
                    "reason": "incremental edits fit local provider completion budgets"
                }),
            )
            .build(),
    )
}

/// write_file called without a resolvable `path` (Hermes #19096 parity).
pub fn write_file_missing_path() -> ToolError {
    ToolError::InvalidArgs {
        tool: "write_file".into(),
        message: "write_file: missing required field 'path'. Re-emit the tool call with \
                  both 'path' and 'content' set."
            .into(),
    }
    .with_recovery(
        recovery_guidance()
            .message("write_file missing path")
            .suggestion(
                RecoveryAction::SetParameter,
                json!({
                    "tool": "write_file",
                    "required": ["path", "content"],
                    "note": "use path not file_path; aliases are normalized at dispatch"
                }),
            )
            .build(),
    )
}

/// write_file called with path but no `content` key (dropped-arg under context pressure).
pub fn write_file_missing_content(max_argument_bytes: Option<usize>) -> ToolError {
    let budget_hint = max_argument_bytes.map_or(String::new(), |max| {
        format!(
            " One-completion argument budget is ~{max} bytes — split very large files \
             into scaffold + patch steps."
        )
    });
    ToolError::InvalidArgs {
        tool: "write_file".into(),
        message: format!(
            "write_file: missing required field 'content'. The tool call included a path \
             but no content argument — this is almost always a dropped-arg bug under \
             context pressure. Re-emit the tool call with the full content payload, or use \
             execute_code for very large files.{budget_hint}"
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("write_file missing content")
            .suggestion(
                RecoveryAction::RetrySameCall,
                json!({
                    "tool": "write_file",
                    "required": ["path", "content"],
                    "strategy": "re-emit full payload or write scaffold then patch"
                }),
            )
            .build(),
    )
}

/// Canonical VisualUx serve → navigate recipe (no port guessing).
pub fn preview_serve_then_navigate_recipe(serve_directory: &str) -> serde_json::Value {
    let dir = if serve_directory.trim().is_empty() {
        "."
    } else {
        serve_directory.trim()
    };
    let command = format!("python3 -m http.server 8000 --directory {dir}");
    json!({
        "tool": "terminal",
        "command": command,
        "background": true,
        "then": "browser_navigate",
        "then_url": "http://127.0.0.1:8000/",
        "forbidden": [
            "try other localhost ports",
            "browser_navigate to 8080/5050/5000/8888 without a recorded session server",
            "read ~/.edgecrab/config.yaml"
        ],
        "note": "Start this exact server first; then navigate only to http://127.0.0.1:8000/"
    })
}

/// Prefer recent `demo/…` write targets or `index.html` parents for `--directory`.
pub fn infer_preview_serve_directory(paths: &[&str]) -> String {
    let mut best_demo: Option<String> = None;
    let mut best_index_parent: Option<String> = None;
    for raw in paths {
        let path = raw.trim().trim_matches('"').replace('\\', "/");
        if path.is_empty() {
            continue;
        }
        let lower = path.to_ascii_lowercase();
        if lower.ends_with("index.html") {
            if let Some((parent, _)) = path.rsplit_once('/') {
                if !parent.is_empty() {
                    best_index_parent = Some(parent.to_string());
                }
            } else {
                best_index_parent = Some(".".into());
            }
        }
        if let Some(idx) = lower.find("demo/") {
            let rest = &path[idx..];
            let dir = if rest.to_ascii_lowercase().ends_with(".html")
                || rest.to_ascii_lowercase().ends_with(".css")
                || rest.to_ascii_lowercase().ends_with(".js")
            {
                rest.rsplit_once('/')
                    .map(|(p, _)| p.to_string())
                    .unwrap_or_else(|| rest.to_string())
            } else {
                rest.to_string()
            };
            if !dir.is_empty() {
                best_demo = Some(dir);
            }
        }
    }
    best_index_parent
        .or(best_demo)
        .unwrap_or_else(|| ".".into())
}

/// Collect path-like tokens from text, then reuse typed path inference (018 F4).
pub fn infer_preview_serve_directory_from_text(text: &str) -> String {
    let mut paths: Vec<&str> = Vec::new();
    for raw in text.split_whitespace() {
        let token = raw.trim_matches(|c: char| {
            matches!(c, '`' | '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']')
        });
        if token.is_empty() {
            continue;
        }
        let lower = token.to_ascii_lowercase();
        if lower.contains("demo/")
            || lower.ends_with(".html")
            || lower.ends_with(".css")
            || lower.ends_with(".js")
            || lower.ends_with("index.html")
        {
            paths.push(token);
        }
    }
    if paths.is_empty() {
        return ".".into();
    }
    infer_preview_serve_directory(&paths)
}

/// Parse loopback host + port from a navigate URL (for preview grants).
pub fn preview_loopback_host_port(url: &str) -> Option<(String, u16)> {
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?;
    let host_l = host.to_ascii_lowercase();
    let is_loopback = host_l == "localhost"
        || host_l == "127.0.0.1"
        || host_l == "::1"
        || host_l == "[::1]";
    if !is_loopback {
        return None;
    }
    let port = parsed.port_or_known_default().unwrap_or(80);
    let norm = if host_l == "localhost" || host_l == "::1" || host_l == "[::1]" {
        "127.0.0.1".to_string()
    } else {
        host_l
    };
    Some((norm, port))
}

/// Extract `preview_loopback` grant payload from a tool error's recovery block.
pub fn preview_loopback_grant_from_error(err: &ToolError) -> Option<(String, u16, String)> {
    let recovery = err.recovery_feedback()?;
    for s in &recovery.suggestions {
        if s.action != RecoveryAction::RequestUserGrant {
            continue;
        }
        let kind = s.parameters.get("grant_kind")?.as_str()?;
        if kind != "preview_loopback" {
            continue;
        }
        let host = s.parameters.get("host")?.as_str()?.to_string();
        let port = s.parameters.get("port")?.as_u64()? as u16;
        let url = s
            .parameters
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Some((host, port, url));
    }
    None
}

/// `browser_navigate` blocked by SSRF or disallowed scheme (spec 015 HA-16).
pub fn browser_navigate_blocked(url: &str, reason: &str, known_ports: &[u16]) -> ToolError {
    browser_navigate_blocked_with_dir(url, reason, known_ports, ".")
}

/// Like [`browser_navigate_blocked`] with an explicit serve directory for empty-port recovery.
pub fn browser_navigate_blocked_with_dir(
    url: &str,
    reason: &str,
    known_ports: &[u16],
    serve_directory: &str,
) -> ToolError {
    browser_navigate_blocked_with_session(url, reason, known_ports, serve_directory, "")
}

/// Session-aware block: prefer wait_bind when a preview spawn is pending (006).
pub fn browser_navigate_blocked_with_session(
    url: &str,
    reason: &str,
    known_ports: &[u16],
    serve_directory: &str,
    session_id: &str,
) -> ToolError {
    if known_ports.is_empty() && !session_id.is_empty() {
        let pending = crate::dev_server::pending_preview_binds(session_id);
        if let Some(port) = crate::dev_server::port_from_loopback_url(url)
            && let Some((_, proc_id)) = pending.iter().find(|(p, _)| *p == port)
        {
            return browser_navigate_wait_bind(url, port, proc_id);
        }
        if let Some((port, proc_id)) = pending.first() {
            return browser_navigate_wait_bind(url, *port, proc_id);
        }
    }
    let port_hint = crate::dev_server::format_dev_server_ports_hint(known_ports);
    let message = if let Some(hint) = port_hint.as_deref() {
        format!("URL blocked for browser navigation: {url} ({reason}). {hint}")
    } else {
        format!(
            "URL blocked for browser navigation: {url} ({reason}). \
             No session HTTP server is recorded — start the preview server first \
             (do not guess localhost ports)."
        )
    };
    let mut builder = recovery_guidance()
        .message("Browser navigation blocked — use HTTP preview, not file://");
    // Grantable SSRF denials: ask the user Once/Session/Always (spec 021).
    if reason.contains("SSRF")
        && let Some((host, port)) = preview_loopback_host_port(url)
    {
        builder = builder.suggestion(
            RecoveryAction::RequestUserGrant,
            json!({
                "grant_kind": "preview_loopback",
                "host": host,
                "port": port,
                "url": url,
                "note": "EdgeCrab will open an approval overlay — do not retry identical navigate until the user decides"
            }),
        );
    }
    builder = builder.suggestion(
        RecoveryAction::SetParameter,
        json!({
            "tool": "browser_navigate",
            "url_shape": "http://127.0.0.1:PORT/path",
            "fix_via": "/config preview on",
            "cli_alt": "edgecrab config set security.preview.enabled true",
            "do_not": [
                "read_file on ~/.edgecrab/config.yaml",
                "terminal cat/grep on home config",
                "try alternate localhost ports"
            ],
            "security_preview_yaml": {
                "security": {
                    "preview": {
                        "enabled": true,
                        "allow_localhost_ports": [8000, 8888, 5173, 3000]
                    }
                }
            },
            "then_verify": ["browser_snapshot", "vision_analyze"],
            "note": "file:// is never allowed; operator enables security.preview — agent cannot read home config"
        }),
    );
    if known_ports.is_empty() {
        builder = builder.suggestion(
            RecoveryAction::CallToolFirst,
            preview_serve_then_navigate_recipe(serve_directory),
        );
    } else {
        builder = builder.suggestion(
            RecoveryAction::SetParameter,
            json!({
                "tool": "browser_navigate",
                "detected_http_server_ports": known_ports,
                "recommended_urls": known_ports
                    .iter()
                    .map(|p| format!("http://127.0.0.1:{p}/"))
                    .collect::<Vec<_>>(),
                "forbidden": ["try ports not in detected_http_server_ports"],
            }),
        );
    }
    ToolError::PermissionDenied(message).with_recovery(builder.build())
}

/// Connection refused / empty-response / server not running on loopback.
pub fn browser_navigate_no_server(url: &str, serve_directory: &str) -> ToolError {
    ToolError::Unavailable {
        tool: "browser_navigate".into(),
        reason: format!(
            "No HTTP server is listening for {url} (connection refused or empty response). \
             Do not try other localhost ports until the preview server is bind-ready. \
             Start the preview server, wait for Serving HTTP…, then navigate to \
             http://127.0.0.1:8000/ only."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Start preview server before browser_navigate")
            .suggestion(
                RecoveryAction::CallToolFirst,
                preview_serve_then_navigate_recipe(serve_directory),
            )
            .build(),
    )
}

/// Navigate attempted while a preview spawn is pending TCP bind (006 bind latch).
///
/// One structured wait — do not spawn another http.server.
pub fn browser_navigate_wait_bind(url: &str, port: u16, process_id: &str) -> ToolError {
    ToolError::Unavailable {
        tool: "browser_navigate".into(),
        reason: format!(
            "Preview server for {url} spawned but bind_ready=false (TCP not listening on :{port} yet). \
             Do not spawn another http.server. Wait for process `{process_id}` bind, then retry navigate once."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Wait for TCP bind-ready before browser_navigate")
            .suggestion(
                RecoveryAction::CallToolFirst,
                json!({
                    "tool": "wait_for_process",
                    "process_id": process_id,
                    "wait_bind": true,
                    "port": port,
                    "then": {
                        "tool": "browser_navigate",
                        "url": format!("http://127.0.0.1:{port}/"),
                    },
                    "forbidden": [
                        "spawn another python3 -m http.server on the same port",
                        "port shopping"
                    ],
                }),
            )
            .build(),
    )
}

/// Stale recorded port / half-open server (ERR_EMPTY_RESPONSE after optimistic spawn).
pub fn browser_navigate_port_heal(url: &str, port: u16, serve_directory: &str) -> ToolError {
    let dir = if serve_directory.trim().is_empty() {
        ".".to_string()
    } else {
        serve_directory.trim().to_string()
    };
    ToolError::Unavailable {
        tool: "browser_navigate".into(),
        reason: format!(
            "Loopback navigate to {url} failed (empty response / connection reset). \
             Port {port} was recorded but is not serving. Heal the port, then navigate."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Heal stale preview port before browser_navigate")
            .suggestion(
                RecoveryAction::CallToolFirst,
                json!({
                    "tool": "terminal",
                    "steps": [
                        {
                            "command": format!(
                                "kill $(lsof -t -i:{port}) 2>/dev/null; \
                                 python3 -m http.server {port} --directory {dir}"
                            ),
                            "background": true,
                            "note": "free the port then restart preview server"
                        },
                        {
                            "tool": "browser_navigate",
                            "url": format!("http://127.0.0.1:{port}/")
                        }
                    ],
                    "alternate": {
                        "command": format!(
                            "python3 -m http.server 8010 --directory {dir}"
                        ),
                        "background": true,
                        "then_navigate": "http://127.0.0.1:8010/",
                        "note": "if kill is unavailable, bind a free port and navigate there"
                    },
                    "forbidden": ["navigate before Serving HTTP… ready"]
                }),
            )
            .build(),
    )
}

/// True when terminal/npm output indicates a Node engine / version-manager mismatch.
pub fn is_node_engine_mismatch(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("ebadengine")
        || lower.contains("engine \"node\"")
        || lower.contains("required: {\"node\"")
        || lower.contains("requires node")
        || lower.contains("requested version") && lower.contains("is not currently installed")
        || (lower.contains("nvm is not compatible") && lower.contains("node"))
        || lower.contains("the engine \"node\" is incompatible")
}

/// Structured recovery for Node/engine mismatches (spec 021 Wave E).
///
/// No retry counting and no silent `nvm use` — one clarify for toolchain preference,
/// then one user-approved terminal install/switch command.
pub fn terminal_node_engine_mismatch(command: &str, output: &str) -> Option<ToolError> {
    if !is_node_engine_mismatch(output) {
        return None;
    }
    let snippet: String = output.chars().take(400).collect();
    Some(
        ToolError::ExecutionFailed {
            tool: "terminal".into(),
            message: format!(
                "Node/engine version mismatch while running: {command}. \
                 Do not retry the same install until Node is switched. Output excerpt:\n{snippet}"
            ),
        }
        .with_recovery(
            recovery_guidance()
                .message(
                    "Host Node version does not satisfy the project engine — ask which toolchain to use",
                )
                .suggestion_with_message(
                    RecoveryAction::CallToolFirst,
                    json!({
                        "tool": "clarify",
                        "needs_user_preference": true,
                        "question": "Which Node toolchain should EdgeCrab use to install or switch to a newer Node (e.g. ≥22)?",
                        "choices": ["nvm", "fnm", "asdf", "Homebrew", "system package manager"],
                        "do_not": [
                            "retry npm/npx with the same Node",
                            "silent nvm use / fnm use without asking",
                            "auto-install Node without terminal approval"
                        ]
                    }),
                    "Call clarify once for toolchain preference; then propose one terminal command (approval-gated)",
                )
                .suggestion(
                    RecoveryAction::NoRecoveryAvailable,
                    json!({
                        "note": "Do not count retries. First structured mismatch → one clarify or one approved install."
                    }),
                )
                .build(),
        ),
    )
}

/// Preview-server bind failed with Address already in use / EADDRINUSE.
pub fn terminal_port_in_use(command: &str, port: u16, serve_directory: &str) -> ToolError {
    let dir = if serve_directory.trim().is_empty() {
        let inferred = infer_preview_serve_directory_from_text(command);
        if inferred.trim().is_empty() {
            ".".to_string()
        } else {
            inferred
        }
    } else {
        serve_directory.trim().to_string()
    };
    ToolError::ExecutionFailed {
        tool: "terminal".into(),
        message: format!(
            "Preview server could not bind port {port}: Address already in use. \
             Free the port or start on another port, then browser_navigate."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Port conflict — heal before navigate")
            .suggestion(
                RecoveryAction::CallToolFirst,
                json!({
                    "tool": "terminal",
                    "steps": [
                        {
                            "command": format!(
                                "kill $(lsof -t -i:{port}) 2>/dev/null; \
                                 python3 -m http.server {port} --directory {dir}"
                            ),
                            "background": true
                        },
                        {
                            "tool": "browser_navigate",
                            "url": format!("http://127.0.0.1:{port}/")
                        }
                    ],
                    "alternate_port": {
                        "command": format!(
                            "python3 -m http.server 8010 --directory {dir}"
                        ),
                        "background": true,
                        "then_navigate": "http://127.0.0.1:8010/"
                    }
                }),
            )
            .build(),
    )
}

/// `patch` fuzzy miss / ContentMismatch — typed recovery (parity with write/stale).
pub fn patch_content_mismatch(
    path: &str,
    diagnosis: &str,
    preview: &str,
    truncated: bool,
) -> ToolError {
    let trunc_note = if truncated {
        "\n[...truncated — file has more content; read_file if needed.]".to_string()
    } else {
        String::new()
    };
    ToolError::ContentMismatch {
        tool: "patch".into(),
        path: path.to_string(),
        message: format!(
            "{diagnosis}\n\
             Snapshot recorded — retry patch with the corrected old_string \
             (no read_file needed).\n\
             \n\
             Current file content (preview):\n\
             ---\n\
             {preview}{trunc_note}\n\
             ---"
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Patch old_string mismatch — retry with file preview")
            .suggestion(
                RecoveryAction::RetrySameCall,
                json!({
                    "tool": "patch",
                    "path": path,
                    "hint": "copy old_string from the preview below; avoid inventing whitespace"
                }),
            )
            .suggestion(
                RecoveryAction::CallToolFirst,
                json!({
                    "tool": "read_file",
                    "path": path,
                    "when": "preview is insufficient",
                    "then_retry": "patch"
                }),
            )
            .build(),
    )
}

/// `memory_write` exceeded per-file char cap (spec 015 HA-17).
pub fn memory_write_char_limit_exceeded(
    filename: &str,
    used_chars: usize,
    max_chars: usize,
    attempted_add: usize,
) -> ToolError {
    ToolError::InvalidArgs {
        tool: "memory_write".into(),
        message: format!(
            "{filename} would exceed {max_chars}-char limit ({used_chars} used + {attempted_add} new). \
             Prune old entries or use session_search before adding."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Memory file at capacity")
            .suggestion(
                RecoveryAction::CallToolFirst,
                json!({
                    "tool": "memory_read",
                    "target": if filename == "USER.md" { "user" } else { "memory" },
                    "then": "prune stale entries with replace/remove actions"
                }),
            )
            .suggestion(
                RecoveryAction::SwitchTool,
                json!({
                    "tool": "session_search",
                    "reason": "find prior session facts instead of duplicating memory",
                    "suggested_actions": ["prune_old", "session_search"]
                }),
            )
            .build(),
    )
}

/// Memory file modified externally — refuse mutation until operator resolves drift (HA-17 extension).
pub fn memory_external_drift(filename: &str, drift_backup: &str) -> ToolError {
    ToolError::InvalidArgs {
        tool: "memory_write".into(),
        message: format!(
            "{filename} shows external drift (manual edit, patch tool, or sister session). \
             A backup was saved to {drift_backup}. Read memory_read, prune stale entries, \
             or restore from backup before writing."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Memory file drift detected")
            .suggestion(
                RecoveryAction::CallToolFirst,
                json!({
                    "tool": "memory_read",
                    "target": if filename == "USER.md" { "user" } else { "memory" },
                    "then": "prune or replace entries to fit char limit"
                }),
            )
            .suggestion(
                RecoveryAction::SwitchTool,
                json!({
                    "tool": "session_search",
                    "reason": "recover facts without duplicating drifted memory",
                    "drift_backup": drift_backup,
                    "suggested_actions": ["prune_old", "session_search"]
                }),
            )
            .build(),
    )
}

/// Extract deferred tool names that recovery feedback wants the agent to call next.
///
/// Used to auto-materialize those tools onto the Indexed wire so SwitchTool /
/// CallToolFirst targets are callable without a lucky `tool_search` (game001).
pub fn tools_to_materialize_from_error_json(result: &str) -> Vec<String> {
    use edgecrab_types::{RecoveryAction, parse_tool_error_payload};
    use crate::tool_schema_index::TOOL_SEARCH_NAME;

    let Some(payload) = parse_tool_error_payload(result) else {
        return Vec::new();
    };

    let mut names: Vec<String> = Vec::new();
    let push = |names: &mut Vec<String>, candidate: &str| {
        let t = candidate.trim();
        if t.is_empty() || t == TOOL_SEARCH_NAME {
            return;
        }
        if !names.iter().any(|n| n == t) {
            names.push(t.to_string());
        }
    };
    let push_from_params = |names: &mut Vec<String>, params: &serde_json::Value| {
        for key in ["to_tool", "then_retry"] {
            if let Some(t) = params.get(key).and_then(|v| v.as_str()) {
                push(names, t);
            }
        }
        if let Some(arr) = params.get("recommended_tools").and_then(|v| v.as_array()) {
            for t in arr {
                if let Some(s) = t.as_str() {
                    push(names, s);
                }
            }
        }
    };

    if let Some(st) = payload.suggested_tool.as_deref() {
        push(&mut names, st);
    }

    if let Some(feedback) = payload.recovery_feedback.as_ref() {
        for suggestion in &feedback.suggestions {
            match suggestion.action {
                RecoveryAction::SwitchTool => {
                    push_from_params(&mut names, &suggestion.parameters);
                }
                RecoveryAction::CallToolFirst => {
                    let first = suggestion
                        .parameters
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if first == TOOL_SEARCH_NAME {
                        // Invent / discovery recovery: promote ranked candidates.
                        push_from_params(&mut names, &suggestion.parameters);
                    } else {
                        push(&mut names, first);
                        push_from_params(&mut names, &suggestion.parameters);
                    }
                }
                RecoveryAction::SplitPayload => {
                    if let Some(t) = suggestion.parameters.get("tool").and_then(|v| v.as_str()) {
                        push(&mut names, t);
                    }
                    push_from_params(&mut names, &suggestion.parameters);
                }
                _ => {}
            }
        }
    }

    // Cap so recovery never floods the materialize LRU.
    names.truncate(4);
    names
}

/// Shell heredoc rejected — steer to file tools (spec 015 HA-18).
pub fn terminal_heredoc_unsupported(command: &str) -> ToolError {
    ToolError::capability_denied(
        "terminal",
        "shell_heredoc_unsupported",
        format!(
            "Shell heredocs are not supported in `terminal`. A heredoc embeds multi-line input \
             directly inside the tool-call command string, which is unreliable for edits or \
             large stdin payloads.\nCommand: `{command}`"
        ),
    )
    .with_recovery(
        recovery_guidance()
            .message("Use write_file or patch instead of shell heredocs")
            .suggestion(
                RecoveryAction::SwitchTool,
                json!({
                    "from_tool": "terminal",
                    "to_tool": "write_file",
                    "recommended_tools": ["write_file", "patch"],
                    "reason": "file tools are the supported content-transport path"
                }),
            )
            .build(),
    )
}

/// Unknown / hallucinated tool name after repair.
///
/// First principle (July 2026 progressive disclosure): the registry is truth.
/// Recovery is [`RecoveryAction::CallToolFirst`] on `tool_search` — never
/// `RetrySameCall` on the invent name, and never a second parallel dictionary tool.
pub fn unknown_tool(
    invalid_name: &str,
    suggestion: Option<&str>,
    search_query: &str,
    sample_tools: &[String],
) -> ToolError {
    let suggest = suggestion
        .map(|s| format!(" Did you mean '{s}'?"))
        .unwrap_or_default();
    let sample = if sample_tools.is_empty() {
        "(none ranked — call tool_search)".to_string()
    } else {
        sample_tools.join(", ")
    };
    let query = if search_query.trim().is_empty() {
        invalid_name.replace(['_', '-', '.'], " ")
    } else {
        search_query.to_string()
    };
    ToolError::NotFound(format!(
        "Tool '{invalid_name}' does not exist.{suggest} \
         It is not in the tool registry. Call `tool_search` with query: \"{query}\" \
         (or tool_names for an exact candidate), then call one exact snake_case name \
         from the result. Candidate names from the registry: {sample}. \
         Do not retry the invalid name."
    ))
    .with_recovery(
        recovery_guidance()
            .message("Unknown tool name — discover via tool_search")
            .suggestion(
                RecoveryAction::CallToolFirst,
                json!({
                    "tool": "tool_search",
                    "query": query,
                    "limit": 5,
                    "invalid_name": invalid_name,
                    "suggested_name": suggestion,
                    "recommended_tools": sample_tools,
                    "then": "call an exact tool name returned by tool_search; do not invent names"
                }),
            )
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_types::RecoveryAction;

    #[test]
    fn node_engine_mismatch_recovery_marks_preference() {
        let output = r#"npm WARN EBADENGINE Unsupported engine {
  package: 'hyperframes',
  required: { node: '>=22' },
  current: { node: 'v20.19.0', npm: '10.8.2' }
}"#;
        assert!(is_node_engine_mismatch(output));
        let err = terminal_node_engine_mismatch("npm install", output).expect("err");
        let recovery = err.to_llm_payload().recovery_feedback.expect("recovery");
        let blob = serde_json::to_string(&recovery).expect("json");
        assert!(blob.contains("needs_user_preference"));
        assert!(blob.contains("clarify"));
        assert!(blob.contains("do_not"));
        assert!(!blob.contains("silent nvm use / fnm use without asking") || blob.contains("do_not"));
    }

    #[test]
    fn write_file_abort_includes_structured_recovery() {
        let err = write_file_path_exists_abort("src/main.rs".into(), 128);
        let payload = err.to_llm_payload();
        assert!(payload.error.contains("already exists"));
        let recovery = payload.recovery_feedback.expect("recovery attached");
        assert_eq!(recovery.feedback_type, "recovery_guidance");
        assert!(recovery.suggestions.len() >= 2);
        assert_eq!(
            recovery.suggestions[0].action,
            RecoveryAction::UseDifferentPath
        );
    }

    #[test]
    fn ha04_budget_exceeded_recommends_patch() {
        let err = tool_argument_budget_exceeded("write_file", 30_546, 27_852, 7_000);
        let recovery = err.to_llm_payload().recovery_feedback.expect("recovery");
        let blob = serde_json::to_string(&recovery.suggestions).expect("json");
        assert!(blob.contains("patch"), "expected patch in {blob}");
    }

    #[test]
    fn port_heal_and_address_in_use_include_recovery() {
        let heal = browser_navigate_port_heal("http://127.0.0.1:8000/", 8000, "demo/game005");
        let heal_rec = heal.to_llm_payload().recovery_feedback.expect("heal");
        let heal_blob = serde_json::to_string(&heal_rec.suggestions).expect("json");
        assert!(heal_blob.contains("lsof") || heal_blob.contains("8010"));

        let busy = terminal_port_in_use(
            "python3 -m http.server 8000 --directory demo/game005",
            8000,
            "demo/game005",
        );
        let busy_rec = busy.to_llm_payload().recovery_feedback.expect("busy");
        assert!(!busy_rec.suggestions.is_empty());
    }

    #[test]
    fn patch_content_mismatch_includes_structured_recovery() {
        let err = patch_content_mismatch("game.js", "old_string not found", "let x = 1;", false);
        let recovery = err.to_llm_payload().recovery_feedback.expect("recovery");
        assert!(
            recovery
                .suggestions
                .iter()
                .any(|s| s.action == RecoveryAction::RetrySameCall)
        );
    }

    /// Spec 021 G1 — SSRF loopback block carries RequestUserGrant (acceptance name).
    #[test]
    fn browser_navigate_blocked_includes_request_user_grant() {
        let err = browser_navigate_blocked("http://127.0.0.1:8000/", "SSRF policy", &[]);
        let recovery = err.to_llm_payload().recovery_feedback.expect("recovery");
        let blob = serde_json::to_string(&recovery.suggestions).expect("json");
        assert!(blob.contains("security.preview"));
        assert!(blob.contains("127.0.0.1"));
        assert!(
            recovery
                .suggestions
                .iter()
                .any(|s| s.action == RecoveryAction::CallToolFirst),
            "empty known_ports must CallToolFirst(terminal): {blob}"
        );
        assert!(
            recovery
                .suggestions
                .iter()
                .any(|s| s.action == RecoveryAction::RequestUserGrant),
            "SSRF loopback must RequestUserGrant: {blob}"
        );
        assert!(blob.contains("http.server"));
        assert!(blob.contains("forbidden") || blob.contains("try other localhost ports"));
        let grant = preview_loopback_grant_from_error(&err).expect("grant");
        assert_eq!(grant.0, "127.0.0.1");
        assert_eq!(grant.1, 8000);
    }

    #[test]
    fn ha16_browser_blocked_includes_preview_hint() {
        // Compat alias — keep HA-16 discoverability; delegates to 021 acceptance name.
        browser_navigate_blocked_includes_request_user_grant();
    }

    #[test]
    fn preview_serve_directory_prefers_demo_index() {
        assert_eq!(
            infer_preview_serve_directory(&["demo/game002/index.html", "demo/game002/style.css"]),
            "demo/game002"
        );
        assert_eq!(
            infer_preview_serve_directory_from_text(
                "Write a complete html5 game in ./demo/game002"
            ),
            "demo/game002"
        );
    }

    #[test]
    fn ha20c_browser_blocked_cites_detected_ports() {
        let err = browser_navigate_blocked("http://127.0.0.1:8888/", "SSRF policy", &[8000, 8888]);
        let payload = err.to_llm_payload();
        assert!(payload.error.contains("8000"));
        let recovery = payload.recovery_feedback.expect("recovery");
        let blob = serde_json::to_string(&recovery.suggestions).expect("json");
        assert!(blob.contains("detected_http_server_ports"));
        assert!(blob.contains("8888"));
    }

    #[test]
    fn ha17_memory_limit_includes_prune_guidance() {
        let err = memory_write_char_limit_exceeded("MEMORY.md", 2100, 2200, 150);
        let payload = err.to_llm_payload();
        assert!(payload.error.contains("2200"));
        let recovery = payload.recovery_feedback.expect("recovery");
        let blob = serde_json::to_string(&recovery.suggestions).expect("json");
        assert!(blob.contains("prune_old") || blob.contains("session_search"));
    }

    #[test]
    fn ha17_memory_drift_includes_backup_path() {
        let err = memory_external_drift("MEMORY.md", "/tmp/MEMORY.md.bak.123");
        let payload = err.to_llm_payload();
        assert!(payload.error.contains("drift"));
        assert!(payload.error.contains(".bak.123"));
    }

    #[test]
    fn ha18_heredoc_recommends_write_file() {
        let err = terminal_heredoc_unsupported("cat <<'EOF'\nhello\nEOF");
        let recovery = err.to_llm_payload().recovery_feedback.expect("recovery");
        let blob = serde_json::to_string(&recovery.suggestions).expect("json");
        assert!(blob.contains("write_file"));
        let json = err.to_llm_response();
        let targets = tools_to_materialize_from_error_json(&json);
        assert!(
            targets.iter().any(|n| n == "write_file"),
            "heredoc recovery must auto-materialize write_file: {targets:?}"
        );
    }

    #[test]
    fn recovery_materialize_skips_tool_search_meta() {
        let sample = vec!["write_file".into(), "patch".into()];
        let err = unknown_tool("invent_write", None, "invent write", &sample);
        let targets = tools_to_materialize_from_error_json(&err.to_llm_response());
        assert!(!targets.iter().any(|n| n == "tool_search"));
        assert!(targets.iter().any(|n| n == "write_file"));
    }

    #[test]
    fn unknown_tool_requires_tool_search_not_retry_same() {
        let sample = vec![
            "web_search".into(),
            "web_extract".into(),
            "read_file".into(),
        ];
        let err = unknown_tool("quick_stock_quote", None, "quick stock quote", &sample);
        let recovery = err.to_llm_payload().recovery_feedback.expect("recovery");
        assert_eq!(recovery.suggestions[0].action, RecoveryAction::CallToolFirst);
        let blob = serde_json::to_string(&recovery.suggestions[0].parameters).expect("json");
        assert!(blob.contains("tool_search"));
        assert!(blob.contains("quick stock quote") || blob.contains("query"));
        assert!(blob.contains("web_search"));
        let payload = err.to_llm_payload();
        assert!(payload.error.contains("tool_search"));
        assert!(payload.error.contains("Do not retry the invalid name"));
    }

    #[test]
    fn wait_bind_recovery_does_not_suggest_second_spawn() {
        crate::dev_server::mark_pending_preview("sess-wait-bind", 8000, "proc-9");
        let err = browser_navigate_wait_bind("http://127.0.0.1:8000/", 8000, "proc-9");
        let body = err.to_llm_response();
        assert!(body.contains("wait_bind") || body.contains("bind_ready") || body.contains("proc-9"));
        let parsed = edgecrab_types::parse_tool_error_payload(&body).expect("tool_error");
        assert!(
            parsed
                .recovery_feedback
                .as_ref()
                .is_some_and(|r| r.suggestions.iter().any(|s| {
                    s.action == RecoveryAction::CallToolFirst
                        && s.parameters
                            .get("wait_bind")
                            .and_then(|v| v.as_bool())
                            == Some(true)
                })),
            "expected wait_bind CallToolFirst: {body}"
        );
        crate::dev_server::clear_pending_preview("sess-wait-bind", 8000);
    }
}
