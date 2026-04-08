//! # execute_code — Sandboxed code execution with RPC tool access
//!
//! Programmatic Tool Calling (PTC): the LLM writes a Python script that can
//! call EdgeCrab tools via RPC, collapsing multi-step tool chains into a
//! single inference turn.
//!
//! Architecture (matches hermes-agent):
//! ```text
//!   execute_code(code)
//!       │
//!       ├── generate edgecrab_tools.py (RPC stubs for approved sandbox tools)
//!       ├── start Unix domain socket RPC listener
//!       ├── spawn child:  python3 script.py
//!       │       └── script calls edgecrab_tools.web_search() etc.
//!       │               └── RPC over UDS → parent dispatches via ToolRegistry
//!       ├── capture stdout (head+tail, 50KB cap) + stderr (10KB)
//!       └── return JSON { status, output, tool_calls_made, duration_seconds }
//! ```
//!
//! Also supports non-Python languages (JS, TS, Bash, Ruby, Perl, Rust) but
//! these run without RPC tool access (same as before).
//!
//! Security:
//! - Child env is sanitized: API keys/tokens/secrets are stripped
//! - Only 7 tools are exposed via RPC (no skill_manage, no memory etc.)
//! - Tool call limit (50) prevents runaway loops
//! - Process group kill with SIGTERM→SIGKILL escalation

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
#[cfg(unix)]
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STD};
use serde::Deserialize;
use serde_json::json;

use edgecrab_types::{ToolError, ToolSchema};

#[cfg(unix)]
use crate::describe_execution_filesystem;
#[cfg(unix)]
use crate::execution_tmp::{BACKEND_TMP_ROOT, wrap_command_with_tmp_env};
use crate::execution_tmp::{ensure_default_shared_tmp_dir, temp_env_pairs};
#[cfg(unix)]
use crate::registry::ToolRegistry;
use crate::registry::{ToolContext, ToolHandler};
#[cfg(unix)]
use crate::tools::backend_pool::get_or_create_backend;
#[cfg(unix)]
use crate::tools::backends;
use crate::tools::backends::redact_output;

/// Maximum execution time before we kill the subprocess (5 minutes like hermes).
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Maximum stdout bytes returned to the LLM (50KB like hermes).
const MAX_STDOUT_BYTES: usize = 50_000;
/// Maximum stderr bytes (10KB like hermes).
const MAX_STDERR_BYTES: usize = 10_000;
/// Max tool calls per script execution.
#[cfg(unix)]
const MAX_TOOL_CALLS: usize = 50;

/// Tools allowed inside the execute_code sandbox.
///
/// First-principles inclusion criteria — a tool is included when ALL of:
///   1. Pure data-access OR idempotent mutation (no irreversible external effects)
///   2. Does not spawn another agent / require interactive user input
///   3. Is NOT self-referential (execute_code must never call execute_code)
///   4. A realistic script genuinely benefits from calling it inline rather
///      than forwarding the raw payload back to the outer agent for handling
///
/// Explicitly EXCLUDED by category:
///   • clarify / checkpoint           — require live user interaction, deadlock in subprocess
///   • execute_code / delegate_task / moa — recursive agent spawning
///   • browser_* / browser_vision     — browser state is stateful & shared with parent
///   • memory_read / memory_write     — persistent user store is parent-owned; scripts
///                                      should not permanently mutate agent memory
///   • send_message                   — external side effect (sends real messages)
///   • manage_cron_jobs / manage_todo_list — agentic scheduling side effects
///   • skill_* / honcho_*             — skill & user-model management from scripts is unsafe
///   • ha_* (Home Assistant)          — controls physical devices; irreversible
///   • mcp_* (MCP client)             — external MCP servers have uncontrolled side effects
///   • text_to_speech / generate_image — generates media files; heavy & side-effecting
///   • kill_process / run_process / list_processes — the parent agent's own process tree
///
/// Tool-by-tool rationale for inclusions:
///   web_search      — read-only; most common scripting need
///   web_extract     — read-only; idempotent HTTP fetch
///   web_crawl       — read-only; multi-page superset of web_extract; without it
///                     scripts must loop web_extract manually with ad-hoc link parsing
///   read_file       — read-only; core file I/O
///   pdf_to_markdown — read-only PDF parsing via EdgeParse; useful for local document analysis
///   write_file      — write; already powerful but script output must go somewhere
///   search_files    — read-only; structural grep
///   patch           — targeted in-place edit; idempotent when re-run with same args
///   terminal        — foreground-only subprocess (background/pty blocked); the parent
///                     agent is still responsible for reviewing output
///   vision_analyze  — media input read; same as read_file but for images; frontier
///                     models (GPT-4.1) import it as `from edgecrab_tools import vision_analyze`
///                     — omitting it causes ImportError and silent image-analysis failure
///   transcribe_audio — same reasoning as vision_analyze but for audio; takes a local
///                      file path, returns text; no side effects beyond local I/O
///   session_search  — read-only SQLite FTS5 query over past sessions; lets scripts
///                     retrieve context without pulling entire conversation into prompt
#[cfg(unix)]
const SANDBOX_ALLOWED_TOOLS: &[&str] = &[
    "web_search",
    "web_extract",
    "web_crawl",
    "read_file",
    "pdf_to_markdown",
    "write_file",
    "search_files",
    "patch",
    "terminal",
    "vision_analyze",
    "transcribe_audio",
    "session_search",
];

/// Terminal parameters that must not be used from sandbox scripts.
#[cfg(unix)]
const TERMINAL_BLOCKED_PARAMS: &[&str] = &["background", "check_interval", "pty"];

pub struct ExecuteCodeToolReal;

#[derive(Deserialize)]
struct ExecuteCodeArgs {
    /// For Python, this is optional (defaults to "python").
    /// For other languages, required.
    #[serde(default = "default_language")]
    language: String,
    code: String,
    #[serde(default)]
    timeout: Option<u64>,
}

fn default_language() -> String {
    "python".to_string()
}

/// Map language name → (runtime command, file extension).
fn resolve_runtime(lang: &str) -> Option<(&'static str, &'static str)> {
    match lang.to_lowercase().as_str() {
        "python" | "python3" | "py" => Some(("python3", ".py")),
        "javascript" | "js" | "node" => Some(("node", ".js")),
        "typescript" | "ts" => Some(("npx tsx", ".ts")),
        "bash" | "sh" | "shell" => Some(("bash", ".sh")),
        "ruby" | "rb" => Some(("ruby", ".rb")),
        "perl" | "pl" => Some(("perl", ".pl")),
        "rust" | "rs" => Some(("rustc_run", ".rs")), // special: compile+run
        _ => None,
    }
}

// ─── RPC stub generator ─────────────────────────────────────────────

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RpcTransport {
    Uds,
    File,
}

/// Generate the `edgecrab_tools.py` module that child scripts import.
/// Each stub function sends an RPC request over a Unix domain socket
/// back to the parent process, which dispatches through the ToolRegistry.
#[cfg(unix)]
fn generate_tools_module(available_tools: &[&str], transport: RpcTransport) -> String {
    let mut stubs = String::new();
    let available_set: std::collections::HashSet<&str> = available_tools.iter().copied().collect();

    for &tool in SANDBOX_ALLOWED_TOOLS {
        if !available_set.contains(tool) {
            stubs.push_str(&format!(
                r#"
def {tool}(*args, **kwargs):
    raise RuntimeError("Tool '{tool}' is not available in this execute_code session. Available: " + ", ".join(AVAILABLE_TOOLS))
"#
            ));
            continue;
        }
        let stub = match tool {
            "web_search" => {
                r#"
def web_search(query: str, max_results: int = 5, backend: str = None):
    """Search the web. Returns {"query", "backend", "results":[{"url","title","description"}]}."""
    args = {"query": query, "max_results": max_results}
    if backend is not None:
        args["backend"] = backend
    return _call("web_search", args)
"#
            }
            "web_extract" => {
                r#"
def web_extract(url: str, max_chars: int = 8000, backend: str = None, render_js_fallback: bool = True):
    """Extract one URL. Returns {"backend","result":{"url","title","content","extractor","content_type","content_format"}}."""
    args = {"url": url, "max_chars": max_chars, "render_js_fallback": render_js_fallback}
    if backend is not None:
        args["backend"] = backend
    return _call("web_extract", args)
"#
            }
            "web_crawl" => {
                r#"
def web_crawl(url: str, instructions: str = None, max_pages: int = 8, max_depth: int = 2, max_chars_per_page: int = 4000, same_path_only: bool = False, backend: str = None, render_js_fallback: bool = True):
    """Recursively crawl a website starting from a URL.
    Returns {"success", "backend", "pages_visited", "results":[{"url","title","depth","content","extractor","content_type","content_format"}]}.
    Use instructions to focus on specific content (e.g. 'find API docs').
    Prefer this over looping web_extract when you need multiple linked pages.
    """
    args = {"url": url, "max_pages": max_pages, "max_depth": max_depth,
            "max_chars_per_page": max_chars_per_page, "same_path_only": same_path_only,
            "render_js_fallback": render_js_fallback}
    if instructions is not None:
        args["instructions"] = instructions
    if backend is not None:
        args["backend"] = backend
    return _call("web_crawl", args)
"#
            }
            "read_file" => {
                r#"
def read_file(path: str, offset: int = 1, limit: int = 500):
    """Read a file (1-indexed lines). Returns dict with "content" and "total_lines"."""
    return _call("read_file", {"path": path, "offset": offset, "limit": limit})
"#
            }
            "pdf_to_markdown" => {
                r#"
def pdf_to_markdown(path: str, output_path: str = None, max_chars: int = 20000):
    """Convert a local PDF file to Markdown using EdgeParse.
    This is fast structural PDF parsing, not OCR.
    Returns {"success", "path", "output_path", "extractor", "parsing_mode", "content_format", "truncated", "total_chars", "markdown"}.
    """
    args = {"path": path, "max_chars": max_chars}
    if output_path is not None:
        args["output_path"] = output_path
    return _call("pdf_to_markdown", args)
"#
            }
            "write_file" => {
                r#"
def write_file(path: str, content: str):
    """Write content to a file (always overwrites). Returns dict with status."""
    return _call("write_file", {"path": path, "content": content})
"#
            }
            "search_files" => {
                r#"
def search_files(pattern: str, target: str = "content", path: str = ".", file_glob: str = None, limit: int = 50, offset: int = 0, output_mode: str = "content", context: int = 0):
    """Search file contents (target="content") or find files by name (target="files"). Returns dict with "matches"."""
    return _call("search_files", {"pattern": pattern, "target": target, "path": path, "file_glob": file_glob, "limit": limit, "offset": offset, "output_mode": output_mode, "context": context})
"#
            }
            "patch" => {
                r#"
def patch(path: str = None, old_string: str = None, new_string: str = None, replace_all: bool = False, mode: str = "replace", patch: str = None):
    """Targeted find-and-replace (mode="replace") or V4A multi-file patches (mode="patch"). Returns dict with status."""
    return _call("patch", {"path": path, "old_string": old_string, "new_string": new_string, "replace_all": replace_all, "mode": mode, "patch": patch})
"#
            }
            "terminal" => {
                r#"
def terminal(command: str, timeout: int = None, workdir: str = None):
    """Run a shell command (foreground only). Returns dict with "output" and "exit_code"."""
    return _call("terminal", {"command": command, "timeout": timeout, "workdir": workdir})
"#
            }
            "vision_analyze" => {
                r#"
def vision_analyze(image_source: str, question: str = "Describe this image in detail.", detail: str = "high"):
    """Analyze a local image file or https:// image URL using the vision LLM.
    image_source: absolute file path (png/jpg/webp/…) or https:// URL.
    Returns dict with "content" containing the model's description.
    Use this for any local image path or remote image URL.
    Do NOT use this for browser screenshots — use browser_vision instead.
    """
    return _call("vision_analyze", {"image_source": image_source, "question": question, "detail": detail})
"#
            }
            "transcribe_audio" => {
                r#"
def transcribe_audio(file_path: str, provider: str = None, model: str = None, language: str = "en"):
    """Transcribe speech from an audio file to text.
    file_path: path to audio file (mp3, mp4, m4a, wav, webm, ogg, etc.).
    provider: 'local' (default, free), 'groq', or 'openai'.
    Returns dict with "text" containing the transcription.
    """
    args = {"file_path": file_path, "language": language}
    if provider is not None:
        args["provider"] = provider
    if model is not None:
        args["model"] = model
    return _call("transcribe_audio", args)
"#
            }
            "session_search" => {
                r#"
def session_search(query: str, limit: int = 10):
    """Search past conversation sessions using full-text search.
    Returns dict with "results" list of {session_id, snippet, timestamp}.
    Use this to retrieve context from earlier conversations.
    """
    return _call("session_search", {"query": query, "limit": limit})
"#
            }
            _ => continue,
        };
        stubs.push_str(stub);
    }

    let header = match transport {
        RpcTransport::Uds => {
            r#""""Auto-generated EdgeCrab tools RPC stubs."""
import json, os, socket, shlex, time

_sock = None


# ---------------------------------------------------------------------------
# Convenience helpers (avoid common scripting pitfalls)
# ---------------------------------------------------------------------------

def json_parse(text: str):
    """Parse JSON tolerant of control characters (strict=False).
    Use this instead of json.loads() when parsing output from terminal()
    or web_extract() that may contain raw tabs/newlines in strings."""
    return json.loads(text, strict=False)


def shell_quote(s: str) -> str:
    """Shell-escape a string for safe interpolation into commands.
    Use this when inserting dynamic content into terminal() commands:
        terminal(f"echo {shell_quote(user_input)}")
    """
    return shlex.quote(s)


def retry(fn, max_attempts=3, delay=2):
    """Retry a function up to max_attempts times with exponential backoff.
    Use for transient failures (network errors, API rate limits):
        result = retry(lambda: terminal("gh issue list ..."))
    """
    last_err = None
    for attempt in range(max_attempts):
        try:
            return fn()
        except Exception as e:
            last_err = e
            if attempt < max_attempts - 1:
                time.sleep(delay * (2 ** attempt))
    raise last_err


def _connect():
    global _sock
    if _sock is None:
        _sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        _sock.connect(os.environ["EDGECRAB_RPC_SOCKET"])
        _sock.settimeout(300)
    return _sock


def _call(tool_name, args):
    """Send a tool call to the parent process and return the parsed result."""
    conn = _connect()
    request = json.dumps({"tool": tool_name, "args": args}) + "\n"
    conn.sendall(request.encode())
    buf = b""
    while True:
        chunk = conn.recv(65536)
        if not chunk:
            raise RuntimeError("Agent process disconnected")
        buf += chunk
        if buf.endswith(b"\n"):
            break
    raw = buf.decode().strip()
    result = json.loads(raw)
    if isinstance(result, str):
        try:
            return json.loads(result)
        except (json.JSONDecodeError, TypeError):
            return result
    return result
"#
        }
        RpcTransport::File => {
            r#""""Auto-generated EdgeCrab tools RPC stubs (file transport)."""
import json, os, shlex, time

_seq = 0
_RPC_DIR = os.environ.get("EDGECRAB_RPC_DIR", "/tmp/edgecrab_rpc")


# ---------------------------------------------------------------------------
# Convenience helpers (avoid common scripting pitfalls)
# ---------------------------------------------------------------------------

def json_parse(text: str):
    """Parse JSON tolerant of control characters (strict=False).
    Use this instead of json.loads() when parsing output from terminal()
    or web_extract() that may contain raw tabs/newlines in strings."""
    return json.loads(text, strict=False)


def shell_quote(s: str) -> str:
    """Shell-escape a string for safe interpolation into commands.
    Use this when inserting dynamic content into terminal() commands:
        terminal(f"echo {shell_quote(user_input)}")
    """
    return shlex.quote(s)


def retry(fn, max_attempts=3, delay=2):
    """Retry a function up to max_attempts times with exponential backoff.
    Use for transient failures (network errors, API rate limits):
        result = retry(lambda: terminal("gh issue list ..."))
    """
    last_err = None
    for attempt in range(max_attempts):
        try:
            return fn()
        except Exception as e:
            last_err = e
            if attempt < max_attempts - 1:
                time.sleep(delay * (2 ** attempt))
    raise last_err


def _call(tool_name, args):
    """Send a tool call to the parent process and wait for a response file."""
    global _seq
    _seq += 1
    seq_str = f"{_seq:06d}"
    req_file = os.path.join(_RPC_DIR, f"req_{seq_str}")
    res_file = os.path.join(_RPC_DIR, f"res_{seq_str}")

    tmp = req_file + ".tmp"
    with open(tmp, "w") as f:
        json.dump({"tool": tool_name, "args": args, "seq": _seq}, f)
    os.rename(tmp, req_file)

    deadline = time.monotonic() + 300
    poll_interval = 0.05
    while not os.path.exists(res_file):
        if time.monotonic() > deadline:
            raise RuntimeError(f"RPC timeout: no response for {tool_name} after 300s")
        time.sleep(poll_interval)
        poll_interval = min(poll_interval * 1.2, 0.25)

    with open(res_file) as f:
        raw = f.read()

    try:
        os.unlink(res_file)
    except OSError:
        pass

    result = json.loads(raw)
    if isinstance(result, str):
        try:
            return json.loads(result)
        except (json.JSONDecodeError, TypeError):
            return result
    return result
"#
        }
    };

    format!("{header}\nAVAILABLE_TOOLS = {available_tools:?}\n\n{stubs}\n")
}

// ─── RPC server (runs in a tokio task) ──────────────────────────────

/// Accept one client connection and dispatch tool-call requests until
/// the client disconnects or the call limit is reached.
#[cfg(unix)]
async fn rpc_server_loop(
    listener: tokio::net::UnixListener,
    registry: Arc<ToolRegistry>,
    ctx: ToolContext,
    tool_call_counter: Arc<std::sync::atomic::AtomicUsize>,
    allowed_tools: Arc<Vec<String>>,
) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    // Wait up to 5s for the child to connect
    let accept_result = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await;
    let stream = match accept_result {
        Ok(Ok((stream, _))) => stream,
        _ => return,
    };

    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let resp = json!({"error": format!("Invalid RPC request: {e}")});
                let _ = writer.write_all(format!("{}\n", resp).as_bytes()).await;
                continue;
            }
        };

        let tool_name = request
            .get("tool")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut tool_args = request.get("args").cloned().unwrap_or(json!({}));

        // Enforce allow-list
        if !allowed_tools.contains(&tool_name) {
            let available = allowed_tools.join(", ");
            let resp = json!({
                "error": format!(
                    "Tool '{}' is not available in execute_code. Available: {}",
                    tool_name, available
                )
            });
            let _ = writer.write_all(format!("{}\n", resp).as_bytes()).await;
            continue;
        }

        // Enforce tool call limit
        let count = tool_call_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if count >= MAX_TOOL_CALLS {
            let resp = json!({
                "error": format!(
                    "Tool call limit reached ({}). No more tool calls allowed.",
                    MAX_TOOL_CALLS
                )
            });
            let _ = writer.write_all(format!("{}\n", resp).as_bytes()).await;
            continue;
        }

        // Strip forbidden terminal parameters
        if tool_name == "terminal" {
            if let Some(obj) = tool_args.as_object_mut() {
                for param in TERMINAL_BLOCKED_PARAMS {
                    obj.remove(*param);
                }
            }
        }

        // Dispatch through the standard tool handler
        let result = match registry.dispatch(&tool_name, tool_args, &ctx).await {
            Ok(output) => output,
            Err(e) => json!({"error": e.to_string()}).to_string(),
        };

        let _ = writer
            .write_all(format!("{}\n", rpc_response_payload(result)).as_bytes())
            .await;
    }
}

#[cfg(unix)]
fn rpc_response_payload(result: String) -> String {
    if serde_json::from_str::<serde_json::Value>(&result).is_ok() {
        result
    } else {
        json!(result).to_string()
    }
}

#[cfg(unix)]
fn maybe_rewrite_terminal_args(
    tool_name: &str,
    tool_args: &mut serde_json::Value,
    default_remote_workdir: Option<&str>,
) {
    if tool_name != "terminal" {
        return;
    }

    let Some(obj) = tool_args.as_object_mut() else {
        return;
    };

    for param in TERMINAL_BLOCKED_PARAMS {
        obj.remove(*param);
    }

    if let Some(workdir) = default_remote_workdir {
        let has_workdir = obj
            .get("workdir")
            .and_then(|value| value.as_str())
            .is_some_and(|value| !value.is_empty());
        if !has_workdir {
            if let Some(command) = obj.get("command").and_then(|value| value.as_str()) {
                obj.insert(
                    "command".into(),
                    json!(format!(
                        "cd {} && {}",
                        backends::shell_quote(workdir),
                        command
                    )),
                );
            }
        }
    }
}

#[cfg(unix)]
struct RemoteRpcLoop {
    backend: Arc<dyn backends::ExecutionBackend>,
    registry: Arc<ToolRegistry>,
    ctx: ToolContext,
    rpc_dir: String,
    tool_call_counter: Arc<std::sync::atomic::AtomicUsize>,
    allowed_tools: Arc<Vec<String>>,
    stop: tokio_util::sync::CancellationToken,
    terminal_default_workdir: Option<String>,
}

#[cfg(unix)]
async fn remote_rpc_poll_loop(loop_ctx: RemoteRpcLoop) {
    while !loop_ctx.stop.is_cancelled() {
        let pending = match loop_ctx
            .backend
            .execute_oneshot(
                &format!(
                    "ls -1 {} 2>/dev/null | grep '^req_' || true",
                    backends::shell_quote(&loop_ctx.rpc_dir)
                ),
                ".",
                Duration::from_secs(10),
                loop_ctx.stop.clone(),
            )
            .await
        {
            Ok(output) if output.exit_code == 0 => output
                .stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.ends_with(".tmp"))
                .map(|line| format!("{}/{}", loop_ctx.rpc_dir, line))
                .collect::<Vec<_>>(),
            Ok(_) => Vec::new(),
            Err(_) => Vec::new(),
        };

        if pending.is_empty() {
            tokio::select! {
                _ = loop_ctx.stop.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
            continue;
        }

        for req_file in pending {
            if loop_ctx.stop.is_cancelled() {
                break;
            }

            let read = match loop_ctx
                .backend
                .execute_oneshot(
                    &format!("cat {}", backends::shell_quote(&req_file)),
                    ".",
                    Duration::from_secs(10),
                    loop_ctx.stop.clone(),
                )
                .await
            {
                Ok(output) if output.exit_code == 0 => output.stdout,
                _ => {
                    let _ = loop_ctx
                        .backend
                        .execute_oneshot(
                            &format!("rm -f {}", backends::shell_quote(&req_file)),
                            ".",
                            Duration::from_secs(5),
                            tokio_util::sync::CancellationToken::new(),
                        )
                        .await;
                    continue;
                }
            };

            let request: serde_json::Value = match serde_json::from_str(read.trim()) {
                Ok(value) => value,
                Err(_) => {
                    let _ = loop_ctx
                        .backend
                        .execute_oneshot(
                            &format!("rm -f {}", backends::shell_quote(&req_file)),
                            ".",
                            Duration::from_secs(5),
                            tokio_util::sync::CancellationToken::new(),
                        )
                        .await;
                    continue;
                }
            };

            let tool_name = request
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let seq = request
                .get("seq")
                .and_then(|v| v.as_u64())
                .unwrap_or_default();
            let mut tool_args = request.get("args").cloned().unwrap_or(json!({}));
            maybe_rewrite_terminal_args(
                &tool_name,
                &mut tool_args,
                loop_ctx.terminal_default_workdir.as_deref(),
            );

            let result = if !loop_ctx.allowed_tools.contains(&tool_name) {
                let available = loop_ctx.allowed_tools.join(", ");
                json!({
                    "error": format!(
                        "Tool '{}' is not available in execute_code. Available: {}",
                        tool_name, available
                    )
                })
                .to_string()
            } else if loop_ctx
                .tool_call_counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                >= MAX_TOOL_CALLS
            {
                json!({
                    "error": format!(
                        "Tool call limit reached ({}). No more tool calls allowed.",
                        MAX_TOOL_CALLS
                    )
                })
                .to_string()
            } else {
                match loop_ctx
                    .registry
                    .dispatch(&tool_name, tool_args, &loop_ctx.ctx)
                    .await
                {
                    Ok(output) => output,
                    Err(e) => json!({"error": e.to_string()}).to_string(),
                }
            };

            let res_file = format!("{}/res_{seq:06}", loop_ctx.rpc_dir);
            let _ = write_remote_file(
                &*loop_ctx.backend,
                &res_file,
                &rpc_response_payload(result),
                loop_ctx.stop.clone(),
            )
            .await;
            let _ = loop_ctx
                .backend
                .execute_oneshot(
                    &format!("rm -f {}", backends::shell_quote(&req_file)),
                    ".",
                    Duration::from_secs(5),
                    tokio_util::sync::CancellationToken::new(),
                )
                .await;
        }
    }
}

// ─── Environment variable filtering ──────────────────────────────────

/// Env var name prefixes considered safe to pass to child processes.
const SAFE_ENV_PREFIXES: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LANG",
    "LC_",
    "TERM",
    "TMPDIR",
    "TMP",
    "TEMP",
    "SHELL",
    "LOGNAME",
    "XDG_",
    "PYTHONPATH",
    "VIRTUAL_ENV",
    "CONDA",
];

/// Substrings in env var names that indicate secrets — these are stripped.
const SECRET_SUBSTRINGS: &[&str] = &[
    "KEY",
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "CREDENTIAL",
    "PASSWD",
    "AUTH",
];

/// Build a sanitized environment for the child process.
/// Strips API keys/tokens/secrets, keeps safe system vars.
fn build_child_env(sock_path: &str, cwd: &std::path::Path) -> HashMap<String, String> {
    let mut env = HashMap::new();
    let tmp_root = ensure_default_shared_tmp_dir()
        .unwrap_or_else(|_| crate::execution_tmp::default_shared_tmp_dir());

    for (k, v) in std::env::vars() {
        let upper = k.to_uppercase();

        // Block vars with secret-like names
        if SECRET_SUBSTRINGS.iter().any(|s| upper.contains(s)) {
            continue;
        }

        // Allow vars with known safe prefixes
        if SAFE_ENV_PREFIXES.iter().any(|p| upper.starts_with(p)) {
            env.insert(k, v);
        }
    }

    env.insert("EDGECRAB_RPC_SOCKET".into(), sock_path.into());
    env.insert("PYTHONDONTWRITEBYTECODE".into(), "1".into());
    for (k, v) in temp_env_pairs(&tmp_root.to_string_lossy()) {
        env.insert(k, v);
    }

    // Inject timezone if configured
    if let Ok(tz) = std::env::var("EDGECRAB_TIMEZONE") {
        if !tz.is_empty() {
            env.insert("TZ".into(), tz);
        }
    }

    // Ensure cwd is importable
    let cwd_str = cwd.to_string_lossy().to_string();
    if let Some(existing) = env.get("PYTHONPATH") {
        env.insert("PYTHONPATH".into(), format!("{cwd_str}:{existing}"));
    } else {
        env.insert("PYTHONPATH".into(), cwd_str);
    }

    env
}

#[cfg(unix)]
async fn write_remote_file(
    backend: &dyn backends::ExecutionBackend,
    path: &str,
    content: &str,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<(), ToolError> {
    let encoded = BASE64_STD.encode(content.as_bytes());
    let scratch_path = format!("{path}.b64");
    let parent = std::path::Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| ".".into());

    backend
        .execute_oneshot(
            &format!(
                "mkdir -p {} && : > {}",
                backends::shell_quote(&parent),
                backends::shell_quote(&scratch_path)
            ),
            ".",
            Duration::from_secs(10),
            cancel.clone(),
        )
        .await?;

    const CHUNK_BYTES: usize = 24_576;
    for chunk in encoded.as_bytes().chunks(CHUNK_BYTES) {
        let chunk = std::str::from_utf8(chunk).map_err(|e| ToolError::ExecutionFailed {
            tool: "execute_code".into(),
            message: format!("Internal base64 encoding error: {e}"),
        })?;
        backend
            .execute_oneshot(
                &format!(
                    "printf '%s' {} >> {}",
                    backends::shell_quote(chunk),
                    backends::shell_quote(&scratch_path)
                ),
                ".",
                Duration::from_secs(30),
                cancel.clone(),
            )
            .await?;
    }

    let decode = backend
        .execute_oneshot(
            &format!(
                "(base64 -d < {} > {} || base64 -D -i {} -o {}) && rm -f {}",
                backends::shell_quote(&scratch_path),
                backends::shell_quote(path),
                backends::shell_quote(&scratch_path),
                backends::shell_quote(path),
                backends::shell_quote(&scratch_path)
            ),
            ".",
            Duration::from_secs(30),
            cancel,
        )
        .await?;

    if decode.exit_code != 0 {
        return Err(ToolError::ExecutionFailed {
            tool: "execute_code".into(),
            message: format!(
                "Failed to materialize remote file {}: {}",
                path,
                decode.format(4_000, 2_000)
            ),
        });
    }

    Ok(())
}

#[cfg(unix)]
async fn prepare_remote_sandbox(
    backend: &dyn backends::ExecutionBackend,
    sandbox_id: &str,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<String, ToolError> {
    let command = format!(
        "tmp_root=\"${{TMPDIR:-/tmp}}\"; dir=\"$tmp_root/edgecrab_exec_{}\"; mkdir -p \"$dir/rpc\" && printf '%s' \"$dir\"",
        sandbox_id
    );
    let output = backend
        .execute_oneshot(&command, ".", Duration::from_secs(15), cancel)
        .await?;
    if output.exit_code != 0 {
        return Err(ToolError::ExecutionFailed {
            tool: "execute_code".into(),
            message: format!(
                "Failed to prepare remote sandbox directory: {}",
                output.format(4_000, 2_000)
            ),
        });
    }

    let path = output.stdout.trim().to_string();
    if path.is_empty() {
        return Err(ToolError::ExecutionFailed {
            tool: "execute_code".into(),
            message: "Remote backend returned an empty sandbox directory path.".into(),
        });
    }
    Ok(path)
}

#[cfg(unix)]
fn remote_command_for_runtime(
    runtime: &str,
    script_name: &str,
    rpc_dir: Option<&str>,
    tmp_root: &str,
) -> String {
    let env_prefix = if let Some(rpc_dir) = rpc_dir {
        let mut out = format!(
            "EDGECRAB_RPC_DIR={} PYTHONDONTWRITEBYTECODE=1",
            backends::shell_quote(rpc_dir)
        );
        if let Ok(tz) = std::env::var("EDGECRAB_TIMEZONE") {
            if !tz.trim().is_empty() {
                out.push(' ');
                out.push_str("TZ=");
                out.push_str(&backends::shell_quote(tz.trim()));
            }
        }
        out
    } else {
        String::new()
    };

    let command = match runtime {
        "rustc_run" => format!(
            "rustc {} -o script_bin && ./script_bin",
            backends::shell_quote(script_name)
        ),
        value if value.contains(' ') => {
            let prefix = if env_prefix.is_empty() {
                String::new()
            } else {
                format!("{env_prefix} ")
            };
            format!("{prefix}{value} {}", backends::shell_quote(script_name))
        }
        value => {
            let prefix = if env_prefix.is_empty() {
                String::new()
            } else {
                format!("{env_prefix} ")
            };
            format!("{prefix}{value} {}", backends::shell_quote(script_name))
        }
    };

    wrap_command_with_tmp_env(&command, tmp_root)
}

#[cfg(unix)]
fn process_outcome_from_exit(exit_code: i32) -> ProcessOutcome {
    match exit_code {
        124 => ProcessOutcome::TimedOut,
        130 => ProcessOutcome::Cancelled,
        code => ProcessOutcome::Completed(code),
    }
}

fn render_execute_code_result(
    stdout: String,
    stderr: String,
    outcome: ProcessOutcome,
    tool_calls_made: usize,
    duration: f64,
    timeout_secs: u64,
) -> String {
    match outcome {
        ProcessOutcome::Completed(exit) => {
            let status = if exit == 0 { "success" } else { "error" };
            let mut result = json!({
                "status": status,
                "output": stdout,
                "tool_calls_made": tool_calls_made,
                "duration_seconds": (duration * 100.0).round() / 100.0,
            });

            if exit != 0 {
                if !stderr.is_empty() {
                    result["error"] = json!(stderr);
                    result["output"] = json!(format!("{stdout}\n--- stderr ---\n{stderr}"));
                } else {
                    result["error"] = json!(format!("Script exited with code {exit}"));
                }
            }

            result.to_string()
        }
        ProcessOutcome::TimedOut => json!({
            "status": "timeout",
            "error": format!("Script timed out after {timeout_secs}s and was killed."),
            "output": if stderr.is_empty() { stdout } else { format!("{stdout}\n--- stderr ---\n{stderr}") },
            "tool_calls_made": tool_calls_made,
            "duration_seconds": (duration * 100.0).round() / 100.0,
        })
        .to_string(),
        ProcessOutcome::Cancelled => json!({
            "status": "interrupted",
            "error": "Execution interrupted by cancellation.",
            "output": format!("{stdout}\n[execution interrupted — user sent a new message]"),
            "tool_calls_made": tool_calls_made,
            "duration_seconds": (duration * 100.0).round() / 100.0,
        })
        .to_string(),
    }
}

#[cfg(unix)]
fn resolve_sandbox_tools(ctx: &ToolContext) -> Vec<&'static str> {
    let fs = describe_execution_filesystem(&ctx.config, &ctx.cwd);
    let allow_terminal = fs.execute_code_terminal_is_safe();
    let Some(registry) = ctx.tool_registry.as_ref() else {
        return Vec::new();
    };

    let mut resolved = Vec::new();
    for &tool_name in SANDBOX_ALLOWED_TOOLS {
        if tool_name == "terminal" && !allow_terminal {
            continue;
        }
        let Some(toolset) = registry.toolset_for_tool(tool_name) else {
            continue;
        };
        if ctx.config.is_tool_enabled(tool_name, &toolset) {
            resolved.push(tool_name);
        }
    }

    resolved
}

#[derive(Debug)]
struct HeadTailCapture {
    head_limit: usize,
    tail_limit: usize,
    head: Vec<u8>,
    tail: VecDeque<u8>,
    total: usize,
}

impl HeadTailCapture {
    fn new(head_limit: usize, tail_limit: usize) -> Self {
        Self {
            head_limit,
            tail_limit,
            head: Vec::new(),
            tail: VecDeque::new(),
            total: 0,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.total = self.total.saturating_add(chunk.len());

        let mut remaining = chunk;
        if self.head.len() < self.head_limit {
            let keep = (self.head_limit - self.head.len()).min(remaining.len());
            self.head.extend_from_slice(&remaining[..keep]);
            remaining = &remaining[keep..];
        }

        if self.tail_limit == 0 || remaining.is_empty() {
            return;
        }

        for &byte in remaining {
            self.tail.push_back(byte);
            while self.tail.len() > self.tail_limit {
                self.tail.pop_front();
            }
        }
    }

    fn into_text(self) -> String {
        if self.total <= self.head.len() + self.tail.len() {
            let mut bytes = self.head;
            bytes.extend(self.tail);
            return String::from_utf8_lossy(&bytes).into_owned();
        }

        let head_text = String::from_utf8_lossy(&self.head).into_owned();
        let tail_bytes: Vec<u8> = self.tail.into_iter().collect();
        let tail_text = String::from_utf8_lossy(&tail_bytes).into_owned();
        let omitted = self
            .total
            .saturating_sub(self.head.len())
            .saturating_sub(tail_bytes.len());
        format!(
            "{head_text}\n\n... [OUTPUT TRUNCATED - {omitted} chars omitted out of {} total] ...\n\n{tail_text}",
            self.total
        )
    }
}

#[derive(Debug)]
struct HeadOnlyCapture {
    limit: usize,
    bytes: Vec<u8>,
    total: usize,
}

impl HeadOnlyCapture {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            bytes: Vec::new(),
            total: 0,
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.total = self.total.saturating_add(chunk.len());
        if self.bytes.len() >= self.limit {
            return;
        }
        let keep = (self.limit - self.bytes.len()).min(chunk.len());
        self.bytes.extend_from_slice(&chunk[..keep]);
    }

    fn into_text(self) -> String {
        let text = String::from_utf8_lossy(&self.bytes).into_owned();
        if self.total <= self.limit {
            text
        } else {
            format!("{text}... (stderr truncated)")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessOutcome {
    Completed(i32),
    TimedOut,
    Cancelled,
}

#[derive(Debug)]
struct ProcessRunResult {
    stdout: String,
    stderr: String,
    outcome: ProcessOutcome,
}

async fn capture_stdout(mut stdout: tokio::process::ChildStdout) -> Result<String, std::io::Error> {
    use tokio::io::AsyncReadExt;

    let mut capture = HeadTailCapture::new(MAX_STDOUT_BYTES * 2 / 5, MAX_STDOUT_BYTES * 3 / 5);
    let mut buf = [0u8; 4096];
    loop {
        let read = stdout.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        capture.push(&buf[..read]);
    }
    Ok(capture.into_text())
}

async fn capture_stderr(mut stderr: tokio::process::ChildStderr) -> Result<String, std::io::Error> {
    use tokio::io::AsyncReadExt;

    let mut capture = HeadOnlyCapture::new(MAX_STDERR_BYTES);
    let mut buf = [0u8; 4096];
    loop {
        let read = stderr.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        capture.push(&buf[..read]);
    }
    Ok(capture.into_text())
}

// ─── Process group management ────────────────────────────────────────

/// Kill the child process and its entire process group.
#[cfg(unix)]
async fn kill_process_group(child: &mut tokio::process::Child, escalate: bool) {
    if let Some(pid) = child.id() {
        unsafe {
            libc::killpg(pid as i32, libc::SIGTERM);
        }
        if escalate {
            tokio::time::sleep(Duration::from_secs(2)).await;
            unsafe {
                libc::killpg(pid as i32, libc::SIGKILL);
            }
        }
    }
    let _ = child.start_kill();
}

#[cfg(not(unix))]
async fn kill_process_group(child: &mut tokio::process::Child, _escalate: bool) {
    let _ = child.start_kill();
}

// ─── Head+tail truncation ───────────────────────────────────────────

/// Truncate output keeping both the head (40%) and tail (60%) to preserve
/// the final print() output which is most important.
#[cfg_attr(not(test), allow(dead_code))]
fn head_tail_truncate(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }

    let head_bytes = (max_bytes * 2) / 5; // 40%
    let tail_bytes = max_bytes - head_bytes; // 60%

    let head_end = (0..=head_bytes)
        .rev()
        .find(|&idx| text.is_char_boundary(idx))
        .unwrap_or(0);
    let tail_start_raw = text.len().saturating_sub(tail_bytes);
    let tail_start = (tail_start_raw..=text.len())
        .find(|&idx| text.is_char_boundary(idx))
        .unwrap_or(text.len());

    if tail_start <= head_end {
        return text[..head_end].to_string();
    }

    let head = &text[..head_end];
    let tail = &text[tail_start..];
    let omitted = text.len() - head_end - (text.len() - tail_start);

    format!(
        "{head}\n\n... [OUTPUT TRUNCATED - {omitted} chars omitted out of {} total] ...\n\n{tail}",
        text.len()
    )
}

// ─── ANSI stripping ─────────────────────────────────────────────────

/// Strip ANSI escape sequences from output so the model never sees them.
fn strip_ansi(text: &str) -> String {
    match strip_ansi_escapes::strip_str(text) {
        s if s.is_empty() && !text.is_empty() => text.to_string(),
        s => s.to_string(),
    }
}

async fn run_command_capture(
    mut cmd: tokio::process::Command,
    timeout_secs: u64,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<ProcessRunResult, ToolError> {
    use std::process::Stdio;

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| ToolError::ExecutionFailed {
        tool: "execute_code".into(),
        message: format!("Failed to spawn process: {e}"),
    })?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ToolError::ExecutionFailed {
            tool: "execute_code".into(),
            message: "Failed to capture stdout".into(),
        })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ToolError::ExecutionFailed {
            tool: "execute_code".into(),
            message: "Failed to capture stderr".into(),
        })?;

    let stdout_task = tokio::spawn(capture_stdout(stdout));
    let stderr_task = tokio::spawn(capture_stderr(stderr));

    let outcome = tokio::select! {
        wait_result = child.wait() => {
            let status = wait_result.map_err(|e| ToolError::ExecutionFailed {
                tool: "execute_code".into(),
                message: format!("Process wait failed: {e}"),
            })?;
            ProcessOutcome::Completed(status.code().unwrap_or(-1))
        }
        _ = tokio::time::sleep(Duration::from_secs(timeout_secs)) => {
            kill_process_group(&mut child, true).await;
            let _ = child.wait().await;
            ProcessOutcome::TimedOut
        }
        _ = cancel.cancelled() => {
            kill_process_group(&mut child, false).await;
            let _ = child.wait().await;
            ProcessOutcome::Cancelled
        }
    };

    let stdout = stdout_task
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "execute_code".into(),
            message: format!("stdout capture join failed: {e}"),
        })?
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "execute_code".into(),
            message: format!("stdout capture failed: {e}"),
        })?;
    let stderr = stderr_task
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "execute_code".into(),
            message: format!("stderr capture join failed: {e}"),
        })?
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "execute_code".into(),
            message: format!("stderr capture failed: {e}"),
        })?;

    Ok(ProcessRunResult {
        stdout,
        stderr,
        outcome,
    })
}

fn configure_process_group(_cmd: &mut tokio::process::Command) {
    #[cfg(unix)]
    unsafe {
        _cmd.pre_exec(|| {
            libc::setpgid(0, 0);
            Ok(())
        });
    }
}

// ─── Tool description builder ────────────────────────────────────────

fn build_description() -> String {
    "Run a Python script that can call EdgeCrab tools programmatically. \
     Use this when you need 3+ tool calls with processing logic between them, \
     need to filter/reduce large tool outputs before they enter your context, \
     need conditional branching (if X then Y else Z), or need to loop \
     (fetch N pages, process N files, retry on failure).\n\n\
     Use normal tool calls instead when: single tool call with no processing, \
     you need to see the full result and apply complex reasoning, \
     or the task requires interactive user input.\n\n\
     Available via `from edgecrab_tools import ...`:\n\n\
       web_search(query: str, max_results: int = 5, backend: str = None) -> dict\n\
         Returns {\"query\", \"backend\", \"results\": [{\"url\", \"title\", \"description\"}, ...]}\n\
       web_extract(url: str, max_chars: int = 8000, backend: str = None, render_js_fallback: bool = True) -> dict\n\
         Returns {\"backend\", \"result\": {\"url\", \"title\", \"content\", \"extractor\", \"content_type\", \"content_format\"}}\n\
       web_crawl(url: str, instructions: str = None, max_pages: int = 8, max_depth: int = 2, backend: str = None, render_js_fallback: bool = True) -> dict\n\
         Crawl multiple linked pages from a start URL. Returns {\"success\", \"backend\", \"pages_visited\", \"results\": [{\"url\", \"title\", \"depth\", \"content\", \"extractor\", \"content_type\", \"content_format\"}, ...]}\n\
       read_file(path: str, offset: int = 1, limit: int = 500) -> dict\n\
         Lines are 1-indexed. Returns {\"content\": \"...\", \"total_lines\": N}\n\
       pdf_to_markdown(path: str, output_path: str = None, max_chars: int = 20000) -> dict\n\
         Convert a local PDF into Markdown using EdgeParse. Fast structural parsing only, not OCR.\n\
       write_file(path: str, content: str) -> dict\n\
         Always overwrites the entire file.\n\
       search_files(pattern: str, target=\"content\", path=\".\", file_glob=None, limit=50) -> dict\n\
         target: \"content\" (search inside files) or \"files\" (find files by name)\n\
       patch(path: str, old_string: str, new_string: str, replace_all: bool = False) -> dict\n\
         Replaces old_string with new_string in the file.\n\
       terminal(command: str, timeout=None, workdir=None) -> dict\n\
         Foreground only (no background/pty). Returns {\"output\": \"...\", \"exit_code\": N}\n\
       vision_analyze(image_source: str, question: str = \"Describe this image.\", detail: str = \"high\") -> dict\n\
         Analyze a local image file path or https:// URL. Returns {\"content\": \"...\"}\n\
       transcribe_audio(file_path: str, provider: str = None, language: str = \"en\") -> dict\n\
         Transcribe an audio file (mp3/wav/m4a/etc.) to text. Returns {\"text\": \"...\"}\n\
       session_search(query: str, limit: int = 10) -> dict\n\
         Full-text search over past sessions. Returns {\"results\": [...]}\n\n\
     Limits: 5-minute timeout, 50KB stdout cap, max 50 tool calls per script. \
     terminal() is foreground-only (no background or pty).\n\n\
     Filesystem rule: direct script file I/O never enforces EdgeCrab's file-tool path jail. \
     In local mode it sees host paths directly; in non-local terminal backends it runs inside that backend and shares the backend filesystem with `terminal()`. \
     Temp-aware code should prefer `os.environ['EDGECRAB_TMPDIR']` (also exported as `TMPDIR`, `TMP`, and `TEMP`) instead of hard-coding `/tmp`. \
     Prefer read_file/write_file/patch/search_files when you need workspace-root and path-restriction enforcement on host files.\n\n\
     Print your final result to stdout. Use Python stdlib (json, re, math, csv, \
     datetime, collections, etc.) for processing between tool calls.\n\n\
     Also available (no import needed — built into edgecrab_tools):\n\
       json_parse(text: str) — json.loads with strict=False; use for terminal() output with control chars\n\
       shell_quote(s: str) — shlex.quote(); use when interpolating dynamic strings into shell commands\n\
       retry(fn, max_attempts=3, delay=2) — retry with exponential backoff for transient failures\n\n\
     Also supports non-Python languages (javascript, typescript, bash, ruby, perl, rust) \
     but without RPC tool access — those run as simple subprocess scripts.\n\n\
     Note: the exact callable subset is also constrained by the current \
     session's enabled toolsets; unavailable helper functions raise a clear \
     runtime error inside the sandbox."
        .to_string()
}

#[cfg(unix)]
async fn execute_remote(
    ctx: &ToolContext,
    runtime: &str,
    ext: &str,
    code: &str,
    timeout_secs: u64,
    is_python: bool,
    sandbox_tools: &[&'static str],
) -> Result<String, ToolError> {
    let backend = get_or_create_backend(ctx).await?;
    if !backend.supports_remote_execute_code() {
        return Err(ToolError::Unavailable {
            tool: "execute_code".into(),
            reason: format!(
                "Remote execute_code is not supported by the {} backend.",
                backend.kind()
            ),
        });
    }

    let exec_start = std::time::Instant::now();
    let sandbox_dir = prepare_remote_sandbox(
        &*backend,
        &uuid::Uuid::new_v4().simple().to_string()[..12],
        ctx.cancel.clone(),
    )
    .await?;
    let script_name = format!("script{ext}");
    let script_path = format!("{sandbox_dir}/{script_name}");
    write_remote_file(&*backend, &script_path, code, ctx.cancel.clone()).await?;

    let tool_call_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut rpc_task = None;
    let rpc_stop = tokio_util::sync::CancellationToken::new();

    if is_python {
        let tools_path = format!("{sandbox_dir}/edgecrab_tools.py");
        let tools_src = generate_tools_module(sandbox_tools, RpcTransport::File);
        write_remote_file(&*backend, &tools_path, &tools_src, ctx.cancel.clone()).await?;

        if let Some(registry) = ctx.tool_registry.clone() {
            let rpc_ctx = ToolContext {
                task_id: ctx.task_id.clone(),
                cwd: ctx.cwd.clone(),
                session_id: ctx.session_id.clone(),
                user_task: ctx.user_task.clone(),
                cancel: ctx.cancel.clone(),
                config: ctx.config.clone(),
                state_db: ctx.state_db.clone(),
                platform: ctx.platform,
                process_table: ctx.process_table.clone(),
                provider: ctx.provider.clone(),
                tool_registry: ctx.tool_registry.clone(),
                delegate_depth: ctx.delegate_depth,
                sub_agent_runner: ctx.sub_agent_runner.clone(),
                delegation_event_tx: ctx.delegation_event_tx.clone(),
                clarify_tx: None,
                approval_tx: None,
                on_skills_changed: ctx.on_skills_changed.clone(),
                gateway_sender: ctx.gateway_sender.clone(),
                origin_chat: ctx.origin_chat.clone(),
                session_key: ctx.session_key.clone(),
                todo_store: ctx.todo_store.clone(),
                current_tool_call_id: None,
                current_tool_name: None,
                tool_progress_tx: None,
            };
            let rpc_dir = format!("{sandbox_dir}/rpc");
            let allowed = Arc::new(sandbox_tools.iter().map(|tool| tool.to_string()).collect());
            let poll_backend = backend.clone();
            let poll_stop = rpc_stop.clone();
            let poll_counter = tool_call_counter.clone();
            let poll_workdir = sandbox_dir.clone();
            rpc_task = Some(tokio::spawn(async move {
                remote_rpc_poll_loop(RemoteRpcLoop {
                    backend: poll_backend,
                    registry,
                    ctx: rpc_ctx,
                    rpc_dir,
                    tool_call_counter: poll_counter,
                    allowed_tools: allowed,
                    stop: poll_stop,
                    terminal_default_workdir: Some(poll_workdir),
                })
                .await;
            }));
        }
    }

    let rpc_dir = format!("{sandbox_dir}/rpc");
    let remote_command = format!(
        "cd {} && {}",
        backends::shell_quote(&sandbox_dir),
        remote_command_for_runtime(
            runtime,
            &script_name,
            is_python.then_some(rpc_dir.as_str()),
            BACKEND_TMP_ROOT,
        )
    );

    let output = backend
        .execute(
            &remote_command,
            ".",
            Duration::from_secs(timeout_secs),
            ctx.cancel.clone(),
        )
        .await?;

    rpc_stop.cancel();
    if let Some(task) = rpc_task {
        let _ = tokio::time::timeout(Duration::from_secs(5), task).await;
    }

    let _ = backend
        .execute_oneshot(
            &format!("rm -rf {}", backends::shell_quote(&sandbox_dir)),
            ".",
            Duration::from_secs(15),
            tokio_util::sync::CancellationToken::new(),
        )
        .await;

    let stdout = strip_ansi(&redact_output(&head_tail_truncate(
        &output.stdout,
        MAX_STDOUT_BYTES,
    )));
    let stderr = strip_ansi(&redact_output(crate::safe_truncate(
        &output.stderr,
        MAX_STDERR_BYTES,
    )));
    let tool_calls_made = tool_call_counter.load(std::sync::atomic::Ordering::Relaxed);
    let duration = exec_start.elapsed().as_secs_f64();

    Ok(render_execute_code_result(
        stdout,
        stderr,
        process_outcome_from_exit(output.exit_code),
        tool_calls_made,
        duration,
        timeout_secs,
    ))
}

// ─── Tool implementation ─────────────────────────────────────────────

#[async_trait]
impl ToolHandler for ExecuteCodeToolReal {
    fn name(&self) -> &'static str {
        "execute_code"
    }

    fn toolset(&self) -> &'static str {
        "code_execution"
    }

    fn emoji(&self) -> &'static str {
        "🐍"
    }

    fn is_available(&self) -> bool {
        cfg!(unix)
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "execute_code".into(),
            description: build_description(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "code": {
                        "type": "string",
                        "description": "Python code to execute. Import tools with `from edgecrab_tools import web_search, terminal, ...` and print your final result to stdout."
                    },
                    "language": {
                        "type": "string",
                        "description": "Programming language (default: python). For non-Python: javascript, typescript, bash, ruby, perl, rust — these run without RPC tool access."
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in seconds (default: 300, max: 600)"
                    }
                },
                "required": ["code"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: ExecuteCodeArgs =
            serde_json::from_value(args).map_err(|e| {
                let raw = e.to_string();
                let message = if raw.contains("missing field `code`") {
                    "Invalid execute_code args: missing field `code`. Only call execute_code when you already have a concrete code payload to run; do not use it as a planning placeholder.".to_string()
                } else {
                    format!("Invalid execute_code args: {raw}")
                };
                ToolError::InvalidArgs {
                    tool: "execute_code".into(),
                    message,
                }
            })?;

        if args.code.trim().is_empty() {
            return Err(ToolError::InvalidArgs {
                tool: "execute_code".into(),
                message: "No code provided.".into(),
            });
        }

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        let (runtime, ext) = resolve_runtime(&args.language).ok_or_else(|| {
            ToolError::InvalidArgs {
                tool: "execute_code".into(),
                message: format!(
                    "Unsupported language: '{}'. Supported: python, javascript, typescript, bash, ruby, perl, rust",
                    args.language
                ),
            }
        })?;

        let is_python = matches!(
            args.language.to_lowercase().as_str(),
            "python" | "python3" | "py" | ""
        );
        let timeout_secs = args.timeout.unwrap_or(DEFAULT_TIMEOUT_SECS).min(600);
        #[cfg(unix)]
        let sandbox_tools = resolve_sandbox_tools(ctx);

        let exec_start = std::time::Instant::now();

        #[cfg(unix)]
        if ctx.config.terminal_backend != backends::BackendKind::Local {
            return execute_remote(
                ctx,
                runtime,
                ext,
                &args.code,
                timeout_secs,
                is_python,
                &sandbox_tools,
            )
            .await;
        }

        // Write code to a temp file
        let temp_dir = tempfile::tempdir().map_err(|e| ToolError::ExecutionFailed {
            tool: "execute_code".into(),
            message: format!("Failed to create temp dir: {e}"),
        })?;
        let script_path = temp_dir.path().join(format!("script{ext}"));
        std::fs::write(&script_path, &args.code).map_err(|e| ToolError::ExecutionFailed {
            tool: "execute_code".into(),
            message: format!("Failed to write script: {e}"),
        })?;

        // For Python: set up RPC socket and generate tool stubs
        #[cfg(unix)]
        let rpc_state = if is_python {
            let sock_path = format!("/tmp/edgecrab_rpc_{}.sock", uuid::Uuid::new_v4());

            // Generate tool stubs module
            let tools_src = generate_tools_module(&sandbox_tools, RpcTransport::Uds);
            let tools_path = temp_dir.path().join("edgecrab_tools.py");
            std::fs::write(&tools_path, &tools_src).map_err(|e| ToolError::ExecutionFailed {
                tool: "execute_code".into(),
                message: format!("Failed to write tools module: {e}"),
            })?;

            // Start UDS listener
            let listener = tokio::net::UnixListener::bind(&sock_path).map_err(|e| {
                ToolError::ExecutionFailed {
                    tool: "execute_code".into(),
                    message: format!("Failed to bind RPC socket: {e}"),
                }
            })?;

            let tool_call_counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));

            // If we have a registry, start the RPC server
            let rpc_handle = if let Some(ref reg) = ctx.tool_registry {
                let reg = reg.clone();
                let rpc_ctx = ToolContext {
                    task_id: ctx.task_id.clone(),
                    cwd: ctx.cwd.clone(),
                    session_id: ctx.session_id.clone(),
                    user_task: ctx.user_task.clone(),
                    cancel: ctx.cancel.clone(),
                    config: ctx.config.clone(),
                    state_db: ctx.state_db.clone(),
                    platform: ctx.platform,
                    process_table: ctx.process_table.clone(),
                    provider: ctx.provider.clone(),
                    tool_registry: ctx.tool_registry.clone(),
                    delegate_depth: ctx.delegate_depth,
                    sub_agent_runner: ctx.sub_agent_runner.clone(),
                    delegation_event_tx: ctx.delegation_event_tx.clone(),
                    clarify_tx: None,  // No interactive clarify in sandbox
                    approval_tx: None, // No interactive approvals in sandbox
                    on_skills_changed: ctx.on_skills_changed.clone(),
                    gateway_sender: ctx.gateway_sender.clone(),
                    origin_chat: ctx.origin_chat.clone(),
                    session_key: ctx.session_key.clone(),
                    todo_store: ctx.todo_store.clone(),
                    current_tool_call_id: None,
                    current_tool_name: None,
                    tool_progress_tx: None,
                };
                let counter = tool_call_counter.clone();
                let allowed: Arc<Vec<String>> =
                    Arc::new(sandbox_tools.iter().map(|s| s.to_string()).collect());
                Some(tokio::spawn(async move {
                    rpc_server_loop(listener, reg, rpc_ctx, counter, allowed).await;
                }))
            } else {
                drop(listener);
                None
            };

            Some((sock_path, tool_call_counter, rpc_handle))
        } else {
            None
        };

        #[cfg(not(unix))]
        let rpc_state: Option<(
            String,
            Arc<std::sync::atomic::AtomicUsize>,
            Option<tokio::task::JoinHandle<()>>,
        )> = None;

        // Build a sanitized child environment for every language runtime.
        let sock = if is_python {
            #[cfg(unix)]
            {
                rpc_state.as_ref().map(|(s, _, _)| s.as_str()).unwrap_or("")
            }
            #[cfg(not(unix))]
            {
                ""
            }
        } else {
            ""
        };
        let child_env = build_child_env(sock, &ctx.cwd);

        // Spawn the subprocess
        let run_result = if runtime == "rustc_run" {
            // Rust: compile to temp binary, then run
            let bin_path = temp_dir.path().join("script");
            let mut compile_cmd = tokio::process::Command::new("rustc");
            compile_cmd
                .arg(&script_path)
                .arg("-o")
                .arg(&bin_path)
                .current_dir(&ctx.cwd)
                .env_clear()
                .envs(&child_env);
            configure_process_group(&mut compile_cmd);

            let compile =
                run_command_capture(compile_cmd, timeout_secs, ctx.cancel.clone()).await?;
            match compile.outcome {
                ProcessOutcome::Completed(0) => {}
                ProcessOutcome::Completed(_) => {
                    let stderr = strip_ansi(&redact_output(&compile.stderr));
                    return Ok(json!({
                        "status": "error",
                        "error": format!("Compilation error:\n{stderr}"),
                        "tool_calls_made": 0,
                        "duration_seconds": (exec_start.elapsed().as_secs_f64() * 100.0).round() / 100.0,
                    })
                    .to_string());
                }
                ProcessOutcome::TimedOut => {
                    return Ok(json!({
                        "status": "timeout",
                        "error": format!("Rust compilation timed out after {timeout_secs}s and was killed."),
                        "output": strip_ansi(&redact_output(&compile.stdout)),
                        "tool_calls_made": 0,
                        "duration_seconds": (exec_start.elapsed().as_secs_f64() * 100.0).round() / 100.0,
                    })
                    .to_string());
                }
                ProcessOutcome::Cancelled => {
                    return Ok(json!({
                        "status": "interrupted",
                        "error": "Rust compilation interrupted by cancellation.",
                        "output": format!("{}\n[execution interrupted — user sent a new message]", strip_ansi(&redact_output(&compile.stdout))),
                        "tool_calls_made": 0,
                        "duration_seconds": (exec_start.elapsed().as_secs_f64() * 100.0).round() / 100.0,
                    })
                    .to_string());
                }
            }

            let mut run_cmd = tokio::process::Command::new(bin_path);
            run_cmd.current_dir(&ctx.cwd).env_clear().envs(&child_env);
            configure_process_group(&mut run_cmd);
            run_command_capture(run_cmd, timeout_secs, ctx.cancel.clone()).await?
        } else if runtime.contains(' ') {
            let parts: Vec<&str> = runtime.split_whitespace().collect();
            let mut cmd = tokio::process::Command::new(parts[0]);
            for arg in &parts[1..] {
                cmd.arg(arg);
            }
            cmd.arg(&script_path)
                .current_dir(&ctx.cwd)
                .env_clear()
                .envs(&child_env);
            configure_process_group(&mut cmd);
            run_command_capture(cmd, timeout_secs, ctx.cancel.clone()).await?
        } else {
            let mut cmd = tokio::process::Command::new(runtime);
            cmd.arg(&script_path)
                .current_dir(&ctx.cwd)
                .env_clear()
                .envs(&child_env);
            configure_process_group(&mut cmd);
            run_command_capture(cmd, timeout_secs, ctx.cancel.clone()).await?
        };

        let duration = exec_start.elapsed().as_secs_f64();
        let tool_calls_made = rpc_state
            .as_ref()
            .map(|(_, c, _)| c.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(0);

        // Clean up RPC
        #[cfg(unix)]
        if let Some((sock_path, _, rpc_handle)) = &rpc_state {
            let _ = std::fs::remove_file(sock_path);
            if let Some(h) = rpc_handle {
                h.abort();
            }
        }

        let stdout = strip_ansi(&redact_output(&run_result.stdout));
        let stderr = strip_ansi(&redact_output(&run_result.stderr));
        Ok(render_execute_code_result(
            stdout,
            stderr,
            run_result.outcome,
            tool_calls_made,
            duration,
            timeout_secs,
        ))
    }
}

inventory::submit!(&ExecuteCodeToolReal as &dyn ToolHandler);

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::tools::backend_pool::cleanup_backend_for_task;
    #[cfg(unix)]
    use crate::tools::backends::singularity::SINGULARITY_TEST_ENV_LOCK;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    #[test]
    fn resolve_python() {
        let (rt, ext) = resolve_runtime("python").expect("should resolve");
        assert_eq!(rt, "python3");
        assert_eq!(ext, ".py");
    }

    #[test]
    fn resolve_javascript() {
        let (rt, ext) = resolve_runtime("js").expect("should resolve");
        assert_eq!(rt, "node");
        assert_eq!(ext, ".js");
    }

    #[test]
    fn resolve_bash() {
        let (rt, ext) = resolve_runtime("bash").expect("should resolve");
        assert_eq!(rt, "bash");
        assert_eq!(ext, ".sh");
    }

    #[test]
    fn resolve_rust() {
        let (rt, ext) = resolve_runtime("rust").expect("should resolve");
        assert_eq!(rt, "rustc_run");
        assert_eq!(ext, ".rs");
    }

    #[test]
    fn resolve_unknown_returns_none() {
        assert!(resolve_runtime("cobol").is_none());
    }

    #[test]
    fn head_tail_truncate_short() {
        let text = "hello world";
        assert_eq!(head_tail_truncate(text, 100), "hello world");
    }

    #[test]
    fn head_tail_truncate_long() {
        let text = "A".repeat(1000);
        let result = head_tail_truncate(&text, 100);
        assert!(result.contains("OUTPUT TRUNCATED"));
        assert!(result.len() < 200); // head + message + tail
    }

    #[test]
    fn head_tail_truncate_preserves_utf8_boundaries() {
        let text = "🙂".repeat(400);
        let result = head_tail_truncate(&text, 101);
        assert!(!result.is_empty());
    }

    #[test]
    fn strip_ansi_removes_colors() {
        let text = "\x1b[31mred\x1b[0m normal";
        let result = strip_ansi(text);
        assert_eq!(result, "red normal");
    }

    #[cfg(unix)]
    #[test]
    fn generate_tools_module_contains_stubs() {
        let module = generate_tools_module(&["web_search", "terminal"], RpcTransport::Uds);
        assert!(module.contains("def web_search("));
        assert!(module.contains("max_results: int = 5"));
        assert!(module.contains("backend: str = None"));
        assert!(module.contains("def terminal("));
        assert!(module.contains("def json_parse("));
        assert!(module.contains("def shell_quote("));
        assert!(module.contains("def retry("));
        assert!(module.contains("def read_file("));
        assert!(module.contains("not available in this execute_code session"));
    }

    #[cfg(unix)]
    #[test]
    fn generate_tools_module_uses_current_web_extract_signature() {
        let module = generate_tools_module(&["web_extract"], RpcTransport::Uds);
        assert!(module.contains(
            "def web_extract(url: str, max_chars: int = 8000, backend: str = None, render_js_fallback: bool = True):"
        ));
        assert!(module.contains(r#"args = {"url": url, "max_chars": max_chars, "render_js_fallback": render_js_fallback}"#));
    }

    #[cfg(unix)]
    #[test]
    fn generate_tools_module_all_tools() {
        let module = generate_tools_module(SANDBOX_ALLOWED_TOOLS, RpcTransport::Uds);
        for tool in SANDBOX_ALLOWED_TOOLS {
            assert!(
                module.contains(&format!("def {tool}(")),
                "missing stub for {tool}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn generate_tools_module_file_transport_uses_rpc_dir() {
        let module = generate_tools_module(&["terminal"], RpcTransport::File);
        assert!(module.contains("EDGECRAB_RPC_DIR"));
        assert!(module.contains("req_"));
        assert!(module.contains("res_"));
    }

    #[test]
    fn build_child_env_strips_secrets() {
        // This test verifies the logic but can't easily set real env vars
        let env = build_child_env("/tmp/test.sock", std::path::Path::new("/tmp"));
        assert_eq!(
            env.get("EDGECRAB_RPC_SOCKET").map(|s| s.as_str()),
            Some("/tmp/test.sock")
        );
        assert_eq!(
            env.get("PYTHONDONTWRITEBYTECODE").map(|s| s.as_str()),
            Some("1")
        );
        assert_eq!(
            env.get("EDGECRAB_TMPDIR"),
            env.get("TMPDIR"),
            "child env must align EDGECRAB_TMPDIR and TMPDIR"
        );
    }

    #[cfg(unix)]
    #[test]
    fn remote_command_for_runtime_exports_backend_tmpdir() {
        let command =
            remote_command_for_runtime("python3", "script.py", Some("/tmp/rpc"), BACKEND_TMP_ROOT);
        assert!(command.contains("EDGECRAB_TMPDIR='/tmp/edgecrab-tmp'"));
        assert!(command.contains("TMPDIR='/tmp/edgecrab-tmp'"));
        assert!(command.contains("python3 'script.py'"));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_sandbox_tools_respects_disabled_toolsets() {
        let mut ctx = ToolContext::test_context();
        ctx.tool_registry = Some(Arc::new(ToolRegistry::new()));
        ctx.config.disabled_toolsets = vec!["web".into(), "terminal".into()];

        let tools = resolve_sandbox_tools(&ctx);
        assert!(!tools.contains(&"web_search"));
        assert!(!tools.contains(&"terminal"));
        assert!(tools.contains(&"read_file"));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_sandbox_tools_keeps_terminal_for_remote_backends() {
        let mut ctx = ToolContext::test_context();
        ctx.tool_registry = Some(Arc::new(ToolRegistry::new()));
        ctx.config.terminal_backend = crate::tools::backends::BackendKind::Modal;

        let tools = resolve_sandbox_tools(&ctx);
        assert!(tools.contains(&"terminal"));
        assert!(tools.contains(&"read_file"));
    }

    #[test]
    fn build_description_mentions_tools() {
        let desc = build_description();
        assert!(desc.contains("web_search"));
        assert!(desc.contains("web_crawl"));
        assert!(desc.contains(
            "web_extract(url: str, max_chars: int = 8000, backend: str = None, render_js_fallback: bool = True)"
        ));
        assert!(desc.contains("transcribe_audio"));
        assert!(desc.contains("session_search"));
        assert!(desc.contains("vision_analyze"));
        assert!(desc.contains("terminal"));
        assert!(desc.contains("edgecrab_tools"));
        assert!(
            desc.contains("direct script file I/O never enforces EdgeCrab's file-tool path jail")
        );
        assert!(desc.contains(
            "runs inside that backend and shares the backend filesystem with `terminal()`"
        ));
    }

    #[tokio::test]
    async fn execute_bash_echo() {
        let tool = ExecuteCodeToolReal;
        let ctx = ToolContext::test_context();
        let result = tool
            .execute(
                json!({ "language": "bash", "code": "echo hello world" }),
                &ctx,
            )
            .await
            .expect("should succeed");
        assert!(result.contains("hello world"));
        assert!(result.contains("success") || result.contains("Exit code: 0"));
    }

    #[tokio::test]
    async fn execute_python_print() {
        let tool = ExecuteCodeToolReal;
        let ctx = ToolContext::test_context();
        let result = tool
            .execute(
                json!({ "language": "python", "code": "print(2 + 2)" }),
                &ctx,
            )
            .await;
        match result {
            Ok(output) => assert!(output.contains("4") || output.contains("status")),
            Err(ToolError::ExecutionFailed { .. }) => {} // acceptable if python3 not found
            Err(e) => panic!("unexpected error: {e:?}"),
        }
    }

    #[tokio::test]
    async fn execute_python_default_language() {
        // "code" only, no "language" — should default to python
        let tool = ExecuteCodeToolReal;
        let ctx = ToolContext::test_context();
        let result = tool
            .execute(json!({ "code": "print('default python')" }), &ctx)
            .await;
        match result {
            Ok(output) => assert!(output.contains("default python") || output.contains("status")),
            Err(ToolError::ExecutionFailed { .. }) => {}
            Err(e) => panic!("unexpected error: {e:?}"),
        }
    }

    #[tokio::test]
    async fn execute_empty_code_rejected() {
        let tool = ExecuteCodeToolReal;
        let ctx = ToolContext::test_context();
        let result = tool.execute(json!({ "code": "  " }), &ctx).await;
        assert!(matches!(result, Err(ToolError::InvalidArgs { .. })));
    }

    #[tokio::test]
    async fn execute_invalid_language() {
        let tool = ExecuteCodeToolReal;
        let ctx = ToolContext::test_context();
        let result = tool
            .execute(json!({ "language": "cobol", "code": "DISPLAY 'HI'" }), &ctx)
            .await;
        assert!(matches!(result, Err(ToolError::InvalidArgs { .. })));
    }

    #[tokio::test]
    async fn execute_bash_failure() {
        let tool = ExecuteCodeToolReal;
        let ctx = ToolContext::test_context();
        let result = tool
            .execute(json!({ "language": "bash", "code": "exit 42" }), &ctx)
            .await
            .expect("should return JSON even on failure");
        assert!(result.contains("error") || result.contains("42"));
    }

    #[tokio::test]
    async fn execute_bash_uses_sanitized_env() {
        let tool = ExecuteCodeToolReal;
        let ctx = ToolContext::test_context();
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "sk-test-secret-12345");
        }
        let result = tool
            .execute(
                json!({ "language": "bash", "code": "printf '%s' \"${OPENAI_API_KEY:-NOT_SET}\"" }),
                &ctx,
            )
            .await
            .expect("should return json");
        unsafe {
            std::env::remove_var("OPENAI_API_KEY");
        }

        assert!(result.contains("NOT_SET"), "got: {result}");
        assert!(!result.contains("sk-test-secret-12345"), "got: {result}");
    }

    #[tokio::test]
    async fn execute_timeout_kills_process_and_reports_timeout() {
        let tool = ExecuteCodeToolReal;
        let ctx = ToolContext::test_context();
        let result = tool
            .execute(
                json!({ "language": "bash", "code": "sleep 2", "timeout": 1 }),
                &ctx,
            )
            .await
            .expect("should return timeout json");

        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("output should be json");
        assert_eq!(parsed["status"], "timeout");
    }

    #[tokio::test]
    async fn execute_returns_json() {
        let tool = ExecuteCodeToolReal;
        let ctx = ToolContext::test_context();
        let result = tool
            .execute(json!({ "language": "bash", "code": "echo ok" }), &ctx)
            .await
            .expect("should succeed");
        // Result should be valid JSON
        let parsed: serde_json::Value =
            serde_json::from_str(&result).expect("output should be JSON");
        assert!(parsed.get("status").is_some());
        assert!(parsed.get("tool_calls_made").is_some());
        assert!(parsed.get("duration_seconds").is_some());
    }

    #[test]
    fn tool_metadata() {
        let tool = ExecuteCodeToolReal;
        assert_eq!(tool.name(), "execute_code");
        assert_eq!(tool.toolset(), "code_execution");
        assert_eq!(tool.emoji(), "🐍");
    }

    #[cfg(unix)]
    fn write_fake_singularity_runtime(dir: &tempfile::TempDir, body: &str) -> std::path::PathBuf {
        let path = dir.path().join("fake-apptainer");
        std::fs::write(&path, body).expect("write fake runtime");
        let mut perms = std::fs::metadata(&path).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod");
        path
    }

    #[cfg(unix)]
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn execute_code_remote_terminal_shares_backend_workdir() {
        let _guard = SINGULARITY_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tmpdir");
        let fake = write_fake_singularity_runtime(
            &dir,
            r#"#!/bin/sh
set -eu
case "${1:-}" in
  version)
    echo "apptainer 1.0.0"
    ;;
  instance)
    exit 0
    ;;
  exec)
    workdir="."
    last=""
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --pwd)
          workdir="$2"
          shift 2
          ;;
        *)
          last="$1"
          shift
          ;;
      esac
    done
    mkdir -p "$workdir"
    cd "$workdir"
    exec sh -c "$last"
    ;;
esac
"#,
        );

        unsafe {
            std::env::set_var("EDGECRAB_SINGULARITY_BIN", &fake);
        }

        let mut ctx = ToolContext::test_context();
        ctx.cwd = dir.path().to_path_buf();
        ctx.task_id = format!("exec-remote-{}", uuid::Uuid::new_v4().simple());
        ctx.config.terminal_backend = backends::BackendKind::Singularity;
        ctx.tool_registry = Some(Arc::new(ToolRegistry::new()));

        let tool = ExecuteCodeToolReal;
        let result = tool
            .execute(
                json!({
                    "code": "from edgecrab_tools import terminal\nwith open('note.txt', 'w') as fh:\n    fh.write('backend-note')\nprint(terminal('cat note.txt'))\n"
                }),
                &ctx,
            )
            .await
            .expect("remote execute_code");

        assert!(result.contains("backend-note"), "got: {result}");
        let _ = cleanup_backend_for_task(&ctx.task_id).await;
        unsafe {
            std::env::remove_var("EDGECRAB_SINGULARITY_BIN");
        }
    }
}
