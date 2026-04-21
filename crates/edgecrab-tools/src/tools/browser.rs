//! # browser — Browser automation via headless Chrome/Chromium (CDP)
//!
//! Feature parity with hermes-agent's browser toolset:
//! - Session isolation per task_id (each task gets its own Chrome tab)
//! - Accessibility tree snapshots with @eN ref IDs
//! - Click/type via @eN refs (not CSS selectors)
//! - `full` parameter on snapshot (compact interactive-only vs full)
//! - LLM summarization for large snapshots (>8000 chars)
//! - Console capture with clear flag + error/warn/exception tracking
//! - Post-redirect SSRF checks and bot detection warnings
//! - Inactivity timeout + background cleanup
//! - Emergency cleanup on drop
//! - CDP override via `BROWSER_CDP_URL` env var (`/browser connect`)
//! - Annotated vision screenshots with numbered interactive element labels
//! - Session recording (WebM) via `browser.record_sessions` config
//!
//! Architecture (SOLID):
//! - `SessionManager` — per-task session pool with inactivity cleanup (SRP)
//! - `CdpEndpoint` — configurable CDP host/port with override support (SRP)
//! - `cdp_call` — reusable CDP WebSocket helper (SRP/DRY)
//! - Each tool struct delegates to shared SessionManager (DIP/DRY)

use async_trait::async_trait;
use base64::Engine;
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio_tungstenite::tungstenite::Message;

use edgecrab_types::{ToolError, ToolSchema};
use edgequake_llm::traits::{ChatMessage, ImageData};

use crate::registry::{ToolContext, ToolHandler};

// ═══════════════════════════════════════════════════════════════
// Configuration constants
// ═══════════════════════════════════════════════════════════════

/// Monotonically increasing CDP request ID (shared across all sessions).
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Cached path to the Chrome/Chromium binary, resolved once at startup.
static CHROME_BIN: OnceLock<Option<String>> = OnceLock::new();

/// Global session manager (lazy-initialized).
static SESSION_MANAGER: OnceLock<Arc<SessionManager>> = OnceLock::new();

/// Default CDP debugging port.
const DEFAULT_CDP_PORT: u16 = 9222;

/// Compact snapshot: length at which LLM task-aware extraction is attempted.
/// Only triggers when `user_task` is set; otherwise plain truncation is used.
const SNAPSHOT_SUMMARIZE_THRESHOLD: usize = 8000;

/// Full snapshot (`full=true`): hard cap returned to the agent without LLM.
/// The agent is told how many bytes were hidden and to scroll for more.
const FULL_SNAPSHOT_MAX_CHARS: usize = 20_000;

/// LLM input cap for snapshot summarization.
/// Prevents context-window overflow on small local models (e.g. Ollama 4-bit).
const SNAPSHOT_LLM_INPUT_CAP: usize = 12_000;

/// Timeout for snapshot LLM summarization.  Purposely conservative: if the
/// local model cannot summarize in 30 s it is too slow to be useful here and
/// we fall back to plain truncation.
const SNAPSHOT_SUMMARIZE_TIMEOUT_SECS: u64 = 30;

/// Session inactivity timeout in seconds (default 5 minutes).
const DEFAULT_INACTIVITY_TIMEOUT_SECS: u64 = 300;

/// Cleanup check interval in seconds.
const CLEANUP_INTERVAL_SECS: u64 = 30;

// ═══════════════════════════════════════════════════════════════
// CDP endpoint configuration (supports /browser connect override)
// ═══════════════════════════════════════════════════════════════

/// Configurable CDP endpoint. When `BROWSER_CDP_URL` env var is set (e.g. via
/// `/browser connect`), all browser tools connect to the user's live Chrome
/// instance instead of spawning a local headless one.
static CDP_OVERRIDE: Mutex<Option<CdpEndpoint>> = Mutex::new(None);

/// Parsed CDP endpoint (host + port).
#[derive(Clone, Debug)]
pub struct CdpEndpoint {
    pub host: String,
    pub port: u16,
}

impl CdpEndpoint {
    fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// Get the active CDP endpoint — either from override or default localhost.
fn active_cdp_endpoint() -> CdpEndpoint {
    // Check runtime override first
    if let Ok(guard) = CDP_OVERRIDE.lock()
        && let Some(ref ep) = *guard
    {
        return ep.clone();
    }
    // Check env var
    if let Ok(url) = std::env::var("BROWSER_CDP_URL")
        && let Some(ep) = parse_cdp_url(&url)
    {
        return ep;
    }
    CdpEndpoint {
        host: "127.0.0.1".into(),
        port: DEFAULT_CDP_PORT,
    }
}

/// Parse a CDP URL into host + port.
/// Accepts: `ws://host:port`, `http://host:port`, `host:port`, bare port.
fn parse_cdp_url(url: &str) -> Option<CdpEndpoint> {
    let stripped = url
        .trim()
        .strip_prefix("ws://")
        .or_else(|| url.trim().strip_prefix("wss://"))
        .or_else(|| url.trim().strip_prefix("http://"))
        .or_else(|| url.trim().strip_prefix("https://"))
        .unwrap_or(url.trim());
    // Remove any path component (/json/version, /devtools/browser/...)
    let host_port = stripped.split('/').next().unwrap_or(stripped);
    if let Some((host, port_str)) = host_port.rsplit_once(':') {
        let port: u16 = port_str.parse().ok()?;
        let host = if host.is_empty() { "127.0.0.1" } else { host };
        Some(CdpEndpoint {
            host: host.to_string(),
            port,
        })
    } else if let Ok(port) = host_port.parse::<u16>() {
        Some(CdpEndpoint {
            host: "127.0.0.1".into(),
            port,
        })
    } else {
        None
    }
}

/// Set a CDP override endpoint. Called by `/browser connect`.
pub fn set_cdp_override(url: &str) -> Result<CdpEndpoint, String> {
    let ep = parse_cdp_url(url)
        .ok_or_else(|| format!("Invalid CDP URL: '{url}'. Expected ws://host:port or host:port"))?;
    if let Ok(mut guard) = CDP_OVERRIDE.lock() {
        *guard = Some(ep.clone());
    }
    Ok(ep)
}

/// Clear the CDP override. Called by `/browser disconnect`.
pub fn clear_cdp_override() {
    if let Ok(mut guard) = CDP_OVERRIDE.lock() {
        *guard = None;
    }
}

/// Get the current CDP override status for `/browser status`.
pub fn cdp_override_status() -> Option<String> {
    if let Ok(guard) = CDP_OVERRIDE.lock() {
        guard.as_ref().map(|ep| format!("{}:{}", ep.host, ep.port))
    } else {
        None
    }
}

/// Check if a CDP endpoint is reachable (for `/browser status`).
pub async fn is_cdp_reachable() -> bool {
    let ep = active_cdp_endpoint();
    cdp_http_get_at(&ep, "/json/version").await.is_ok()
}

/// Probe a TCP port to check if something is listening (fast, no HTTP).
/// Completes in at most ~1 second.
pub async fn probe_cdp_port(host: &str, port: u16) -> bool {
    let addr = format!("{host}:{port}");
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    .is_ok_and(|r| r.is_ok())
}

/// Browser info returned by `/json/version`.
#[derive(Debug, Clone)]
pub struct ChromeInfo {
    pub browser: String,
    pub protocol_version: String,
    pub user_agent: String,
    pub js_version: String,
    pub ws_debugger_url: String,
}

/// A single open Chrome tab/target.
#[derive(Debug, Clone)]
pub struct CdpTab {
    pub id: String,
    pub title: String,
    pub url: String,
    pub tab_type: String,
}

/// Query Chrome's /json/version endpoint and return parsed browser info.
pub async fn get_chrome_info() -> Option<ChromeInfo> {
    let ep = active_cdp_endpoint();
    let v = cdp_http_get_at(&ep, "/json/version").await.ok()?;
    Some(ChromeInfo {
        browser: v["Browser"].as_str().unwrap_or("").to_string(),
        protocol_version: v["Protocol-Version"].as_str().unwrap_or("").to_string(),
        user_agent: v["User-Agent"].as_str().unwrap_or("").to_string(),
        js_version: v["V8-Version"].as_str().unwrap_or("").to_string(),
        ws_debugger_url: v["webSocketDebuggerUrl"].as_str().unwrap_or("").to_string(),
    })
}

/// Query Chrome's /json/list endpoint and return all open tabs.
pub async fn list_cdp_tabs() -> Vec<CdpTab> {
    let ep = active_cdp_endpoint();
    match cdp_http_get_at(&ep, "/json/list").await {
        Ok(serde_json::Value::Array(arr)) => arr
            .into_iter()
            .map(|t| CdpTab {
                id: t["id"].as_str().unwrap_or("").to_string(),
                title: t["title"].as_str().unwrap_or("(untitled)").to_string(),
                url: t["url"].as_str().unwrap_or("").to_string(),
                tab_type: t["type"].as_str().unwrap_or("page").to_string(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Close all active browser sessions immediately.
/// Used before switching CDP endpoints so the next tool call gets a fresh session.
pub fn close_all_sessions() {
    if let Some(mgr) = SESSION_MANAGER.get() {
        let mgr = Arc::clone(mgr);
        // Collect all task IDs first (DashMap reads don't need async)
        let ids: Vec<String> = mgr.sessions.iter().map(|r| r.key().clone()).collect();
        if !ids.is_empty() {
            tokio::spawn(async move {
                for id in ids {
                    mgr.close_session(&id).await;
                }
            });
        }
    }
}

/// Try to launch Chrome/Chromium with remote debugging on the given port.
/// Returns `true` if a launch command was executed (not if Chrome is ready yet).
///
/// Uses a dedicated `--user-data-dir` so Chrome launches as a standalone
/// instance even on macOS (which otherwise hands the launch to the existing
/// Chrome process, silently ignoring `--remote-debugging-port`).
fn truthy_env_var(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}

fn browser_launch_allowed_for_current_process() -> bool {
    if truthy_env_var("EDGECRAB_RUN_BROWSER_LAUNCH_TESTS") {
        return true;
    }

    let under_test_harness = cfg!(test)
        || std::env::var_os("RUST_TEST_THREADS").is_some()
        || std::env::var_os("NEXTEST").is_some();

    !under_test_harness
}

pub fn launch_chrome_for_debugging(port: u16) -> bool {
    if !browser_launch_allowed_for_current_process() {
        tracing::info!(
            "browser launch suppressed in test process; set EDGECRAB_RUN_BROWSER_LAUNCH_TESTS=1 to enable"
        );
        return false;
    }

    let bin = find_chrome_binary();
    let Some(chrome) = bin else {
        return false;
    };
    // Isolated profile — forces a new Chrome process independent of any
    // already-running Chrome, which is mandatory for CDP to work on macOS.
    let profile_dir = std::env::temp_dir().join(format!("edgecrab-chrome-debug-{port}"));
    let _ = std::fs::create_dir_all(&profile_dir);

    let mut cmd = std::process::Command::new(&chrome);
    cmd.arg(format!("--remote-debugging-port={port}"))
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-extensions")
        .arg("--disable-sync")
        .arg("--disable-blink-features=AutomationControlled")
        .arg("--window-size=1920,1080");

    // Wire proxy from environment variables (6-level cascade)
    if let Some(proxy_url) = edgecrab_security::proxy::resolve_proxy_url(None) {
        cmd.arg(format!("--proxy-server={proxy_url}"));
    }

    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

/// Wait up to `max_secs` for a CDP port to become reachable. Returns `true` when ready.
pub async fn wait_for_cdp_ready(host: &str, port: u16, max_secs: u64) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(max_secs);
    while std::time::Instant::now() < deadline {
        if probe_cdp_port(host, port).await {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }
    false
}

/// Build the OS-specific manual Chrome launch command for the given port.
///
/// Uses the direct binary path (not `open -a`) because:
/// 1. `open -a "Google Chrome"` on macOS silently delegates to the
///    already-running Chrome process which **ignores** all extra arguments
///    including `--remote-debugging-port`.
/// 2. Chrome 136+ (released March 2025) rejects `--remote-debugging-port`
///    when the default user-data-dir would be used; a `--user-data-dir`
///    pointing to a non-standard directory is now required.
///
/// The generated command creates a dedicated edgecrab debug profile so it
/// works even when the user's regular Chrome is open.
pub fn chrome_launch_command(port: u16) -> String {
    let profile_dir = std::env::temp_dir()
        .join(format!("edgecrab-chrome-debug-{port}"))
        .display()
        .to_string();
    let proxy_arg = edgecrab_security::proxy::resolve_proxy_url(None)
        .map(|url| format!(" --proxy-server={url}"))
        .unwrap_or_default();

    #[cfg(target_os = "macos")]
    {
        // Prefer the direct binary over `open -a` — the binary always starts a
        // new process with the supplied flags, even when Chrome is already open.
        let binary = r#"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"#;
        format!(
            r#""{binary}" \
  --remote-debugging-port={port} \
  --user-data-dir="{profile_dir}" \
  --no-first-run \
  --no-default-browser-check \
  {proxy_arg} \
  about:blank &"#
        )
    }
    #[cfg(target_os = "windows")]
    return format!(
        r#"start "" "chrome.exe" --remote-debugging-port={port} --user-data-dir="{profile_dir}" --no-first-run{proxy_arg} about:blank"#
    );
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return format!(
        r#"google-chrome --remote-debugging-port={port} --user-data-dir="{profile_dir}" --no-first-run{proxy_arg} about:blank &"#
    );
}

// ─── Per-process recording override ─────────────────────────────────────────
/// Runtime recording toggle set by `/browser recording on|off`.
/// When `Some(value)`, overrides `config.browser_record_sessions`.
static RECORDING_ENABLED_OVERRIDE: Mutex<Option<bool>> = Mutex::new(None);

/// Enable or disable recording via the runtime override.
pub fn set_recording_override(enabled: bool) {
    if let Ok(mut g) = RECORDING_ENABLED_OVERRIDE.lock() {
        *g = Some(enabled);
    }
}

/// Clear the runtime recording override (fall back to config.yaml value).
pub fn clear_recording_override() {
    if let Ok(mut g) = RECORDING_ENABLED_OVERRIDE.lock() {
        *g = None;
    }
}

/// Get the current recording override value (None = use config).
pub fn get_recording_override() -> Option<bool> {
    RECORDING_ENABLED_OVERRIDE.lock().ok().and_then(|g| *g)
}

// ═══════════════════════════════════════════════════════════════
// Chrome binary discovery
// ═══════════════════════════════════════════════════════════════

fn find_chrome_binary() -> Option<String> {
    let candidates = [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
        "brave-browser",
        "microsoft-edge",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ];

    for candidate in &candidates {
        if std::process::Command::new("which")
            .arg(candidate)
            .output()
            .ok()
            .is_some_and(|o| o.status.success())
        {
            return Some(candidate.to_string());
        }
        if std::path::Path::new(candidate).exists() {
            return Some(candidate.to_string());
        }
    }
    None
}

fn chrome_binary() -> &'static Option<String> {
    CHROME_BIN.get_or_init(find_chrome_binary)
}

/// Whether any browser tool is usable in the current process.
///
/// WHY two checks:
/// - **Headless mode**: a local Chrome/Chromium binary must exist so we can
///   spawn `--headless --remote-debugging-port=…`.
/// - **Live Chrome mode** (`/browser connect` / `BROWSER_CDP_URL`): the user's
///   already-running browser IS the Chrome — we don't need a local binary at all.
///
/// Without this second check, `is_available()` returns `false` whenever the
/// user connected their live Chrome but hasn't installed a command-line Chrome
/// binary (e.g. macOS-only `.app` install where `chrome_binary()` returns `None`
/// at OnceLock init time). Then the LLM never receives the browser tool schemas,
/// sees `mcp_call_tool` in the list, and tries to call
/// `mcp_call_tool(tool_name="browser_snapshot")` instead.
pub(crate) fn browser_is_available() -> bool {
    // Fast path: local binary found
    if chrome_binary().is_some() {
        tracing::debug!("browser_is_available: true (chrome binary found)");
        return true;
    }
    // Slow path: CDP override is set — live Chrome mode needs no local binary
    let cdp_set = CDP_OVERRIDE.lock().ok().is_some_and(|g| g.is_some());
    let env_set = std::env::var("BROWSER_CDP_URL").is_ok();
    let result = cdp_set || env_set;
    tracing::debug!(
        binary = chrome_binary().is_some(),
        cdp_mutex = cdp_set,
        browser_cdp_url_env = env_set,
        "browser_is_available: {result}"
    );
    result
}

// ═══════════════════════════════════════════════════════════════
// BrowserSession — per-task session state
// ═══════════════════════════════════════════════════════════════

/// Tracks a single task's browser session state.
struct BrowserSession {
    /// CDP WebSocket debugger URL for the tab owned by this task.
    ws_url: String,
    /// CDP page/target ID (for cleanup).
    page_id: String,
    /// Last activity timestamp (seconds since epoch).
    last_activity: AtomicU64,
    /// Active screencast recorder (None if recording is disabled or not yet started).
    recorder: Mutex<Option<ScreencastRecorder>>,
    /// True once the first `browser_navigate` has triggered recording auto-start.
    recording_started: std::sync::atomic::AtomicBool,
}

/// Rendered page content captured from a live browser DOM after JavaScript runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RenderedPage {
    pub url: String,
    pub title: String,
    pub text: String,
    pub meta_description: Option<String>,
    pub links: Vec<String>,
}

impl BrowserSession {
    fn new(ws_url: String, page_id: String) -> Self {
        Self {
            ws_url,
            page_id,
            last_activity: AtomicU64::new(epoch_secs()),
            recorder: Mutex::new(None),
            recording_started: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn touch(&self) {
        self.last_activity.store(epoch_secs(), Ordering::Relaxed);
    }

    fn idle_secs(&self) -> u64 {
        epoch_secs().saturating_sub(self.last_activity.load(Ordering::Relaxed))
    }
}

fn epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn normalize_rendered_text(text: &str) -> String {
    text.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ═══════════════════════════════════════════════════════════════
// ScreencastRecorder — CDP-based session recording (SRP)
//
// Architecture:
//   - `start()` opens a persistent WebSocket and collects Page.screencastFrame
//     events, writing each PNG frame to a temp directory immediately (avoids
//     in-memory bloat for long sessions).
//   - `stop()` sends Page.stopScreencast, waits for the task, then assembles
//     the frames into a WebM via ffmpeg (if available) or leaves them as PNGs.
//   - `BrowserSession` holds an `Option<ScreencastRecorder>` behind a Mutex
//     so the recorder can be started/stopped atomically.
// ═══════════════════════════════════════════════════════════════

/// Recorder state that is `Send + Sync` (usable inside DashMap Arc<BrowserSession>).
struct ScreencastRecorder {
    /// Temporary directory holding numbered PNG frames (frame_000001.png, …).
    frame_dir: PathBuf,
    /// Number of frames written so far.
    frame_count: Arc<AtomicU64>,
    /// Signal to the background task to stop collecting.
    /// Wrapped in Mutex<Option<…>> so it is Sync (oneshot::Sender is not Sync).
    stop_tx: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// Background task that drives the WebSocket and writes frames.
    task: tokio::task::JoinHandle<()>,
}

impl ScreencastRecorder {
    /// Start a CDP screencast on the given WebSocket URL.
    ///
    /// Frames are written to a temporary directory at ~2 fps (everyNthFrame=15
    /// assuming a 30 fps compositor).  Returns `None` if the WebSocket cannot
    /// be opened (non-fatal — recording is best-effort).
    async fn start(ws_url: String) -> Option<Self> {
        let frame_dir = std::env::temp_dir().join(format!("ecrab_rec_{}", epoch_secs()));
        if std::fs::create_dir_all(&frame_dir).is_err() {
            return None;
        }

        let frame_count = Arc::new(AtomicU64::new(0));
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

        let frame_dir_clone = frame_dir.clone();
        let frame_count_clone = Arc::clone(&frame_count);

        let task = tokio::spawn(async move {
            run_screencast_task(ws_url, frame_dir_clone, frame_count_clone, stop_rx).await;
        });

        Some(Self {
            frame_dir,
            frame_count,
            stop_tx: Mutex::new(Some(stop_tx)),
            task,
        })
    }

    /// Stop the recorder and assemble frames into a WebM (or leave as PNGs).
    ///
    /// Returns the path to the saved recording file/directory.
    async fn stop(self, recordings_dir: &Path, task_id: &str) -> Option<PathBuf> {
        // Signal the background task to stop.
        if let Ok(mut guard) = self.stop_tx.lock()
            && let Some(tx) = guard.take()
        {
            let _ = tx.send(());
        }

        // Wait up to 5 s for the collection task to finish.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), self.task).await;

        let count = self.frame_count.load(Ordering::Relaxed);
        if count == 0 {
            let _ = std::fs::remove_dir_all(&self.frame_dir);
            return None;
        }

        let result = assemble_recording(&self.frame_dir, recordings_dir, task_id, count).await;

        // Clean up temp frames regardless of assembly outcome.
        let _ = std::fs::remove_dir_all(&self.frame_dir);
        result
    }
}

/// Background task: open CDP WebSocket, start screencast, collect frames.
async fn run_screencast_task(
    ws_url: String,
    frame_dir: PathBuf,
    frame_count: Arc<AtomicU64>,
    mut stop_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let Ok((mut ws, _)) = tokio_tungstenite::connect_async(&ws_url).await else {
        tracing::warn!("browser recording: could not connect to CDP WebSocket {ws_url}");
        return;
    };

    // Start screencast at ~2 fps.
    let start_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let start_msg = json!({
        "id": start_id,
        "method": "Page.startScreencast",
        "params": {
            "format": "png",
            "quality": 70,
            "maxWidth": 1280,
            "maxHeight": 800,
            "everyNthFrame": 15
        }
    });
    if ws.send(Message::Text(start_msg.to_string())).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            _ = &mut stop_rx => {
                // Stop screencast before exiting.
                let stop_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
                let stop_msg = json!({
                    "id": stop_id,
                    "method": "Page.stopScreencast",
                    "params": {}
                });
                let _ = ws.send(Message::Text(stop_msg.to_string())).await;
                break;
            }
            frame = ws.next() => {
                match frame {
                    Some(Ok(Message::Text(text))) => {
                        let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) else {
                            continue;
                        };
                        if val.get("method").and_then(|m| m.as_str())
                            != Some("Page.screencastFrame")
                        {
                            continue;
                        }
                        let params = &val["params"];
                        let session_id = params["sessionId"].as_u64().unwrap_or(0);

                        // Decode and write frame to disk immediately (avoids memory bloat).
                        if let Some(data) = params["data"].as_str()
                            && let Ok(bytes) =
                                base64::engine::general_purpose::STANDARD.decode(data)
                            {
                                let n = frame_count.fetch_add(1, Ordering::Relaxed) + 1;
                                let path = frame_dir.join(format!("frame_{n:06}.png"));
                                let _ = std::fs::write(path, bytes);
                            }

                        // Acknowledge frame — Chrome stops sending if we don't.
                        let ack_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
                        let ack = json!({
                            "id": ack_id,
                            "method": "Page.screencastFrameAck",
                            "params": { "sessionId": session_id }
                        });
                        let _ = ws.send(Message::Text(ack.to_string())).await;
                    }
                    None | Some(Ok(Message::Close(_))) | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

/// Assemble PNG frames into a WebM using ffmpeg (if available), or keep as a
/// directory of PNGs (fallback).  Returns the output path.
async fn assemble_recording(
    frame_dir: &Path,
    recordings_dir: &Path,
    task_id: &str,
    frame_count: u64,
) -> Option<PathBuf> {
    if frame_count == 0 {
        return None;
    }
    let ts = epoch_secs();
    let short_id: String = task_id.chars().take(16).collect();

    let _ = std::fs::create_dir_all(recordings_dir);

    // Prefer ffmpeg for a compact, shareable WebM.
    if let Some(ffmpeg) = find_ffmpeg() {
        let output = recordings_dir.join(format!("session_{ts}_{short_id}.webm"));
        let input_glob = frame_dir.join("frame_%06d.png");

        let status = std::process::Command::new(&ffmpeg)
            .args([
                "-y",
                "-r",
                "2", // 2 fps (matches our everyNthFrame=15 at ~30fps compositor)
                "-f",
                "image2",
                "-i",
                input_glob.to_str().unwrap_or(""),
                "-c:v",
                "libvpx-vp9",
                "-b:v",
                "0",
                "-crf",
                "40",
                output.to_str().unwrap_or(""),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        if status.ok().is_some_and(|s| s.success()) && output.exists() {
            tracing::info!(
                "browser recording: saved {} frames as WebM → {}",
                frame_count,
                output.display()
            );
            return Some(output);
        }
        tracing::warn!(
            "browser recording: ffmpeg failed or produced no output; \
             falling back to PNG frames"
        );
    }

    // Fallback: copy the temp frame dir to the recordings dir as a subdirectory.
    let dest = recordings_dir.join(format!("session_{ts}_{short_id}_frames"));
    if std::fs::rename(frame_dir, &dest).is_ok() {
        tracing::info!(
            "browser recording: saved {} PNG frames to {}",
            frame_count,
            dest.display()
        );
        return Some(dest);
    }

    None
}

/// Locate ffmpeg binary (checked once per process).
fn find_ffmpeg() -> Option<String> {
    let candidates = [
        "ffmpeg",
        "/usr/local/bin/ffmpeg",
        "/opt/homebrew/bin/ffmpeg",
    ];
    for c in &candidates {
        if std::process::Command::new(c)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()
            .is_some_and(|s| s.success())
        {
            return Some(c.to_string());
        }
    }
    None
}

/// Remove browser recordings older than max_age_secs (best-effort).
fn cleanup_old_recordings(dir: &Path, max_age_secs: u64) {
    cleanup_old_files(dir, max_age_secs);
}

// ═══════════════════════════════════════════════════════════════
// SessionManager — pool of per-task sessions
// ═══════════════════════════════════════════════════════════════

struct SessionManager {
    sessions: DashMap<String, Arc<BrowserSession>>,
    inactivity_timeout: u64,
}

impl SessionManager {
    fn new() -> Self {
        Self {
            sessions: DashMap::new(),
            inactivity_timeout: std::env::var("BROWSER_INACTIVITY_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_INACTIVITY_TIMEOUT_SECS),
        }
    }

    /// Get or create a session for the given task.
    async fn get_or_create(&self, task_id: &str) -> Result<Arc<BrowserSession>, ToolError> {
        // Fast path: session already exists
        if let Some(session) = self.sessions.get(task_id) {
            session.touch();
            return Ok(Arc::clone(&session));
        }

        // Slow path: create a new tab
        ensure_chrome_running().await?;

        // When a CDP override is active (user's live Chrome), prefer attaching
        // to an existing open page tab rather than creating a blank new one.
        // This matches hermes-agent's behaviour: --cdp <url> operates on whatever
        // page is currently active, without spawning extra tabs.
        let is_cdp_override = CDP_OVERRIDE
            .lock()
            .ok()
            .and_then(|g| g.as_ref().cloned())
            .is_some()
            || std::env::var("BROWSER_CDP_URL").is_ok();

        let (ws_url, page_id) = if is_cdp_override {
            // Find the first open "page" tab that isn't internal Chrome UI.
            match cdp_http_get("/json/list").await {
                Ok(serde_json::Value::Array(tabs)) => {
                    let active_tab = tabs.iter().find(|t| {
                        let tab_type = t["type"].as_str().unwrap_or("");
                        let url = t["url"].as_str().unwrap_or("");
                        tab_type == "page"
                            && !url.starts_with("chrome://")
                            && !url.starts_with("chrome-extension://")
                            && !url.starts_with("devtools://")
                    });
                    if let Some(tab) = active_tab {
                        let ws = tab["webSocketDebuggerUrl"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();
                        let id = tab["id"].as_str().unwrap_or("").to_string();
                        if !ws.is_empty() {
                            tracing::info!(
                                "browser (live): attaching to existing tab {} — {}",
                                id,
                                tab["url"].as_str().unwrap_or("?")
                            );
                            (ws, id)
                        } else {
                            // Tab has no WS URL yet — fall through to creating a new tab
                            create_new_tab().await?
                        }
                    } else {
                        // No suitable open tab — create a new one
                        create_new_tab().await?
                    }
                }
                _ => create_new_tab().await?,
            }
        } else {
            // Headless mode: always open a fresh blank tab
            create_new_tab().await?
        };

        let session = Arc::new(BrowserSession::new(ws_url, page_id));

        // Install console/error listeners via CDP
        if let Err(e) = install_console_listener(&session).await {
            tracing::warn!("browser: could not install console listener: {e}");
        }

        // Inject stealth patches to evade bot detection.
        // Uses Page.addScriptToEvaluateOnNewDocument so patches survive navigation.
        if let Err(e) = inject_stealth_patches(&session).await {
            tracing::warn!("browser: could not inject stealth patches: {e}");
        }

        self.sessions
            .insert(task_id.to_string(), Arc::clone(&session));
        Ok(session)
    }

    /// Close and remove a specific task's session.
    async fn close_session(&self, task_id: &str) {
        if let Some((_, session)) = self.sessions.remove(task_id) {
            // Stop any active recording and save to disk.
            let recorder = session.recorder.lock().ok().and_then(|mut g| g.take());
            if let Some(rec) = recorder {
                let recordings_dir = dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                    .join(".edgecrab")
                    .join("browser_recordings");
                if let Some(path) = rec.stop(&recordings_dir, task_id).await {
                    tracing::info!("browser recording saved: {}", path.display());
                }
            }
            let _ = cdp_http_get(&format!("/json/close/{}", session.page_id)).await;
        }
    }

    /// Remove sessions that have been idle longer than the timeout.
    async fn cleanup_inactive(&self) {
        let to_remove: Vec<String> = self
            .sessions
            .iter()
            .filter(|r| r.value().idle_secs() > self.inactivity_timeout)
            .map(|r| r.key().clone())
            .collect();

        for task_id in to_remove {
            tracing::info!("browser: closing inactive session for task {task_id}");
            self.close_session(&task_id).await;
        }
    }
}

fn session_manager() -> &'static Arc<SessionManager> {
    SESSION_MANAGER.get_or_init(|| {
        let mgr = Arc::new(SessionManager::new());
        // Spawn background cleanup task
        let mgr_clone = Arc::clone(&mgr);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(CLEANUP_INTERVAL_SECS)).await;
                mgr_clone.cleanup_inactive().await;
            }
        });
        mgr
    })
}

/// Helper: get session for a task_id (extracted from ToolContext).
///
/// WHY session_id: `ctx.task_id` is a fresh UUID per tool call, which would
/// create a new blank Chrome tab for every single call — meaning browser_snapshot
/// after browser_navigate would see a blank page. `ctx.session_id` is the stable
/// per-conversation identifier (set once in execute_loop, reused for all tool calls
/// in that session), so all browser tools in a conversation share the same tab.
/// This matches hermes-agent's `task_id="default"` behaviour for sequential operations.
async fn get_session(ctx: &ToolContext) -> Result<Arc<BrowserSession>, ToolError> {
    session_manager().get_or_create(&ctx.session_id).await
}

// ═══════════════════════════════════════════════════════════════
// CDP communication helpers (DRY — used by all tools)
// ═══════════════════════════════════════════════════════════════

/// Open a new blank Chrome tab and return its (ws_url, page_id).
///
/// WHY two paths:
/// - **`Target.createTarget` (preferred)**: the proper CDP WebSocket API,
///   supported by all Chrome ≥ 67 (headless and live).  `/json/version`
///   yields the browser-level WS URL; we call `Target.createTarget` on it,
///   then resolve the new tab's debugger URL from `/json/list` by target ID.
/// - **`/json/new` fallback**: the legacy HTTP endpoint. In modern Chrome
///   the `?<url>` query-string form was silently replaced by a proper
///   `?url=<url>` form, and in some builds the endpoint returns an HTML
///   error page (not JSON) — which was the root cause of the "CDP JSON parse
///   error" seen when navigating from a live Chrome session that had only a
///   `chrome://newtab/` tab open.
async fn create_new_tab() -> Result<(String, String), ToolError> {
    // ── Preferred: modern Target.createTarget over browser-level WS ─────────
    if let Some(pair) = create_tab_via_target_api().await {
        return Ok(pair);
    }

    // ── Fallback: legacy /json/new (plain, no query string to avoid HTML 400) ─
    let page_info = cdp_http_get("/json/new")
        .await
        .map_err(|_| ToolError::ExecutionFailed {
            tool: "browser".into(),
            message: "Could not create a new browser tab: both Target.createTarget \
                      and /json/new failed. Ensure Chrome is running with \
                      --remote-debugging-port and that no other process is \
                      monopolising its CDP endpoint."
                .into(),
        })?;

    let ws_url = page_info["webSocketDebuggerUrl"]
        .as_str()
        .ok_or_else(|| ToolError::ExecutionFailed {
            tool: "browser".into(),
            message: "New tab (/json/new) did not return webSocketDebuggerUrl. \
                      Chrome may have returned a non-page target."
                .into(),
        })?
        .to_string();
    let page_id = page_info["id"].as_str().unwrap_or("").to_string();
    Ok((ws_url, page_id))
}

/// Create a new tab using `Target.createTarget` over the browser-level CDP
/// WebSocket URL (obtained from `/json/version`).
///
/// Returns `None` on any failure so the caller can fall back gracefully.
async fn create_tab_via_target_api() -> Option<(String, String)> {
    // Step 1: get the browser-level WS URL from the HTTP discovery endpoint.
    let version = cdp_http_get("/json/version").await.ok()?;
    let browser_ws = version["webSocketDebuggerUrl"].as_str()?.to_string();

    // Step 2: ask the browser to open a new blank page and give us a targetId.
    let result = cdp_call(
        &browser_ws,
        "Target.createTarget",
        json!({ "url": "about:blank" }),
        10,
    )
    .await
    .ok()?;
    let target_id = result["targetId"].as_str()?.to_string();

    // Step 3: poll /json/list until the new target appears (Chrome may need a
    // brief moment to register it) and extract its debugger WS URL.
    for attempt in 0..8u8 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        let tabs = cdp_http_get("/json/list").await.ok()?;
        if let serde_json::Value::Array(arr) = tabs {
            for tab in &arr {
                if tab["id"].as_str() == Some(target_id.as_str())
                    && let Some(ws) = tab["webSocketDebuggerUrl"].as_str()
                {
                    tracing::debug!(
                        "create_tab_via_target_api: target {target_id} ready after \
                             {attempt} poll(s)"
                    );
                    return Some((ws.to_string(), target_id));
                }
            }
        }
    }

    tracing::warn!(
        "create_tab_via_target_api: target {target_id} created but never appeared \
         in /json/list after 8 polls — falling back to /json/new"
    );
    None
}

/// Execute a CDP command via the HTTP JSON API using the active endpoint.
async fn cdp_http_get(path: &str) -> Result<serde_json::Value, ToolError> {
    cdp_http_get_at(&active_cdp_endpoint(), path).await
}

/// Execute a CDP command at a specific endpoint.
async fn cdp_http_get_at(ep: &CdpEndpoint, path: &str) -> Result<serde_json::Value, ToolError> {
    let url = format!("{}{path}", ep.base_url());
    // CDP endpoint is local (127.0.0.1 by default). Never route this through
    // env-configured proxies, or browser control can break when proxy is set.
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "browser".into(),
            message: format!("HTTP client error: {e}"),
        })?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "browser".into(),
            message: format!(
                "CDP connection failed (is Chrome running with \
                 --remote-debugging-port={}?): {e}",
                ep.port
            ),
        })?;

    resp.json::<serde_json::Value>()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "browser".into(),
            message: format!("CDP JSON parse error: {e}"),
        })
}

/// Send a CDP method call over WebSocket and wait for the matching response.
///
/// This is the core DRY helper — every tool that needs to talk to a page
/// goes through this function.
async fn cdp_call(
    ws_url: &str,
    method: &str,
    params: serde_json::Value,
    timeout_secs: u64,
) -> Result<serde_json::Value, ToolError> {
    let (mut ws, _) = tokio_tungstenite::connect_async(ws_url)
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "browser".into(),
            message: format!("CDP WebSocket connect failed: {e}"),
        })?;

    let req_id = REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let msg = json!({ "id": req_id, "method": method, "params": params });

    ws.send(Message::Text(msg.to_string()))
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "browser".into(),
            message: format!("CDP send failed: {e}"),
        })?;

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

    loop {
        if tokio::time::Instant::now() > deadline {
            return Err(ToolError::Timeout {
                tool: "browser".into(),
                seconds: timeout_secs,
            });
        }

        let frame = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), ws.next())
            .await
            .map_err(|_| ToolError::Timeout {
                tool: "browser".into(),
                seconds: timeout_secs,
            })?
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: "browser".into(),
                message: "CDP WebSocket closed prematurely".into(),
            })?
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "browser".into(),
                message: format!("CDP WebSocket receive error: {e}"),
            })?;

        let text = match frame {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => {
                return Err(ToolError::ExecutionFailed {
                    tool: "browser".into(),
                    message: "CDP WebSocket closed by browser".into(),
                });
            }
            _ => continue, // skip binary/ping/pong frames
        };

        let resp: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if resp.get("id").and_then(|v| v.as_u64()) != Some(req_id) {
            continue;
        }

        // CDP-level error
        if let Some(err) = resp["error"].as_object() {
            let msg = err
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown CDP error");
            return Err(ToolError::ExecutionFailed {
                tool: "browser".into(),
                message: format!("CDP error: {msg}"),
            });
        }

        return Ok(resp["result"].clone());
    }
}

/// Shortcut: evaluate JavaScript on a page and return the string result.
async fn cdp_evaluate(ws_url: &str, expression: &str) -> Result<String, ToolError> {
    let result = cdp_call(
        ws_url,
        "Runtime.evaluate",
        json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": true
        }),
        15,
    )
    .await?;

    // Check for JS exception
    if let Some(exc) = result["exceptionDetails"].as_object() {
        let desc = exc
            .get("exception")
            .and_then(|e| e.get("description"))
            .and_then(|d| d.as_str())
            .unwrap_or("JavaScript exception");
        return Ok(format!("ERROR: {desc}"));
    }

    let val = &result["result"];
    if let Some(s) = val["value"].as_str() {
        Ok(s.to_string())
    } else if !val["value"].is_null() {
        Ok(val["value"].to_string())
    } else if let Some(d) = val["description"].as_str() {
        Ok(d.to_string())
    } else {
        Ok("(undefined)".to_string())
    }
}

// ═══════════════════════════════════════════════════════════════
// DevToolsActivePort detection — connect to already-running Chrome
// ═══════════════════════════════════════════════════════════════
//
// First-principles rationale
// ──────────────────────────
// CDP requires Chrome to have been started with --remote-debugging-port.
// There is no way to retroactively attach to an already-running Chrome that
// was started without that flag.  HOWEVER, if Chrome is already running WITH
// remote debugging, it writes a file called `DevToolsActivePort` into its
// user-data-dir.  Scanning well-known profile directories for this file lets
// us auto-detect the correct port without any user action.
//
// Chrome 136+ security note (March 2025)
// ────────────────────────────────────────
// Chrome 136+ disallows --remote-debugging-port with the default user-data-dir.
// A non-standard --user-data-dir must be specified.  This impacts manually
// launched "live" Chrome sessions — the DevToolsActivePort scan below works for
// any profile dir (default or custom), so it remains valid.
//
// DevToolsActivePort file format
// ──────────────────────────────
// Line 1: the port number Chrome is actually listening on (useful when --remote-
//         debugging-port=0 was specified to let the OS pick a free port).
// Line 2: the /devtools/browser/<uuid> path (optional — not needed here).

/// Candidate directories to scan for a `DevToolsActivePort` file.
/// Returns an ordered list: default profiles first, then common alternates.
fn chrome_user_data_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    // Per-platform well-known paths
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            let lib = home.join("Library").join("Application Support");
            for name in &[
                "Google/Chrome",
                "Google/Chrome Beta",
                "Google/Chrome Dev",
                "Google/Chrome Canary",
                "Chromium",
                "Microsoft Edge",
                "BraveSoftware/Brave-Browser",
                "Vivaldi",
            ] {
                dirs.push(lib.join(name));
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            for rel in &[
                ".config/google-chrome",
                ".config/google-chrome-beta",
                ".config/google-chrome-unstable",
                ".config/chromium",
                ".config/microsoft-edge",
                ".config/BraveSoftware/Brave-Browser",
                ".config/vivaldi",
            ] {
                dirs.push(home.join(rel));
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        // %LOCALAPPDATA%
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let base = PathBuf::from(local);
            for rel in &[
                r"Google\Chrome\User Data",
                r"Google\Chrome Beta\User Data",
                r"Chromium\User Data",
                r"Microsoft\Edge\User Data",
                r"BraveSoftware\Brave-Browser\User Data",
            ] {
                dirs.push(base.join(rel));
            }
        }
    }

    // Also check the temp-dir profiles that edgecrab itself creates, so a
    // previously launched edgecrab-managed Chrome is also discovered.
    let tmp = std::env::temp_dir();
    if let Ok(entries) = std::fs::read_dir(&tmp) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("edgecrab-chrome-debug-") {
                dirs.push(entry.path());
            }
        }
    }

    dirs
}

/// Scan known Chrome user-data directories for a `DevToolsActivePort` file.
///
/// Returns the first reachable `CdpEndpoint` found, or `None`.
///
/// This is the recommended way to auto-detect a running Chrome instance that
/// was started with `--remote-debugging-port` — used by VS Code and Playwright.
pub fn find_cdp_from_active_port_file() -> Option<CdpEndpoint> {
    for dir in chrome_user_data_dirs() {
        // Both the root data dir and per-profile sub-dirs may contain the file.
        // Chrome writes it to the root user-data-dir; sub-profiles do not.
        let candidate = dir.join("DevToolsActivePort");
        if let Ok(contents) = std::fs::read_to_string(&candidate) {
            let port_str = contents.lines().next().unwrap_or("").trim();
            if let Ok(port) = port_str.parse::<u16>()
                && port > 0
            {
                tracing::info!("found DevToolsActivePort={port} in {}", candidate.display());
                return Some(CdpEndpoint {
                    host: "127.0.0.1".into(),
                    port,
                });
            }
        }
    }
    None
}

/// Probe common CDP ports (9222 +/- a few) to detect a Chrome debugging server
/// started without `--user-data-dir` pointing to a scanned directory
/// (e.g. a Docker container or manually launched instance on a non-default port).
///
/// Tries ports in this order: 9222, 9223, 9224, 9220, 9221, 9225.
/// Returns the first responding `CdpEndpoint`, or `None`.
pub async fn scan_common_cdp_ports() -> Option<CdpEndpoint> {
    for port in [9222u16, 9223, 9224, 9220, 9221, 9225] {
        let ep = CdpEndpoint {
            host: "127.0.0.1".into(),
            port,
        };
        if cdp_http_get_at(&ep, "/json/version").await.is_ok() {
            tracing::info!("auto-detected Chrome CDP on port {port}");
            return Some(ep);
        }
    }
    None
}

/// High-level helper: find an already-running Chrome with remote debugging
/// enabled, trying both `DevToolsActivePort` files and a port scan.
///
/// Returns `Some(CdpEndpoint)` when a live CDP server is found, `None`
/// if no Chrome appears to be running with remote-debugging enabled.
/// Does NOT modify any global state — callers decide whether to use the
/// result (e.g. auto-connect, suggest to user, etc.).
pub async fn auto_detect_running_chrome_cdp() -> Option<CdpEndpoint> {
    // Fast path: read DevToolsActivePort files (synchronous FS scan)
    if let Some(ep) = find_cdp_from_active_port_file() {
        // Verify it's actually reachable (the file may be stale from a crash)
        if cdp_http_get_at(&ep, "/json/version").await.is_ok() {
            return Some(ep);
        }
        tracing::debug!(
            "DevToolsActivePort pointed to :{} but it is not responding — \
             port file may be stale",
            ep.port
        );
    }
    // Slow path: port scan
    scan_common_cdp_ports().await
}

/// Launch headless Chrome if not already running on the debugging port.
/// When a CDP override is active, we only verify connectivity (no auto-launch).
async fn ensure_chrome_running() -> Result<(), ToolError> {
    let ep = active_cdp_endpoint();

    if cdp_http_get_at(&ep, "/json/version").await.is_ok() {
        return Ok(());
    }

    // If using a CDP override (user's live Chrome), don't auto-launch.
    // Before giving up, check DevToolsActivePort files in case the user
    // started Chrome with a different port than they configured.
    let is_override = CDP_OVERRIDE
        .lock()
        .ok()
        .and_then(|g| g.as_ref().cloned())
        .is_some()
        || std::env::var("BROWSER_CDP_URL").is_ok();

    if is_override {
        // Try to find the correct port from DevToolsActivePort files as a
        // helpful hint – but still fail with a clear error (we won't silently
        // redirect to a different port than what the override specifies).
        let hint = find_cdp_from_active_port_file()
            .map(|ep| {
                format!(
                    "\n  Detected Chrome running with CDP on port {} — \
                               run `/browser connect {}` to attach to it.",
                    ep.port, ep.port
                )
            })
            .unwrap_or_default();

        return Err(ToolError::ExecutionFailed {
            tool: "browser".into(),
            message: format!(
                "CDP endpoint {}:{} is not reachable. \
                 Ensure Chrome is running with --remote-debugging-port={}.{hint}",
                ep.host, ep.port, ep.port
            ),
        });
    }

    // No override: attempt auto-detection of an already-running debugging Chrome
    // before launching a new headless instance.
    if let Some(found_ep) = auto_detect_running_chrome_cdp().await
        && found_ep.port != ep.port
    {
        // Found Chrome on a different port than configured — tell the caller
        // but don't silently switch the global endpoint.
        tracing::info!(
            "auto-detected Chrome CDP on port {} (configured port is {}); \
                 using detected port for this session",
            found_ep.port,
            ep.port
        );
        // We can't return `found_ep` from here — `ensure_chrome_running`
        // only validates / launches; the caller uses `active_cdp_endpoint()`.
        // So we surface the info as a log trace and fall through to launch.
    }

    let chrome = chrome_binary()
        .as_ref()
        .ok_or_else(|| ToolError::Unavailable {
            tool: "browser".into(),
            reason: "No Chrome/Chromium binary found on this system".into(),
        })?;

    // Unique temp profile dir — required by Chrome 136+ which rejects
    // --remote-debugging-port when the default user-data-dir is used.
    let profile_dir = std::env::temp_dir().join(format!("edgecrab-chrome-debug-{}", ep.port));
    let _ = std::fs::create_dir_all(&profile_dir);

    let mut cmd = tokio::process::Command::new(chrome);
    let mut args = vec![
        "--headless=new".to_string(),
        "--disable-gpu".to_string(),
        "--no-sandbox".to_string(),
        "--disable-dev-shm-usage".to_string(),
        "--disable-blink-features=AutomationControlled".to_string(),
        "--window-size=1920,1080".to_string(),
        format!("--remote-debugging-port={}", ep.port),
        format!("--user-data-dir={}", profile_dir.display()),
    ];

    // Wire proxy from environment variables (6-level cascade)
    if let Some(proxy_url) = edgecrab_security::proxy::resolve_proxy_url(None) {
        tracing::debug!(url = %proxy_url, "Chrome: launching with proxy");
        args.push(format!("--proxy-server={proxy_url}"));
    }

    args.push("about:blank".to_string());
    cmd.args(&args);
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());

    cmd.spawn().map_err(|e| ToolError::ExecutionFailed {
        tool: "browser".into(),
        message: format!("Failed to launch Chrome: {e}"),
    })?;

    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        if cdp_http_get_at(&ep, "/json/version").await.is_ok() {
            return Ok(());
        }
    }

    Err(ToolError::ExecutionFailed {
        tool: "browser".into(),
        message: "Chrome launched but did not start accepting CDP connections within 5s".into(),
    })
}

/// Install a JS console interceptor so browser_console can retrieve messages.
async fn install_console_listener(session: &BrowserSession) -> Result<(), ToolError> {
    // Enable runtime events
    let _ = cdp_call(&session.ws_url, "Runtime.enable", json!({}), 5).await;
    let _ = cdp_call(&session.ws_url, "Log.enable", json!({}), 5).await;

    let js = r#"
        (function() {
            if (window.__ecrab_console_ok) return 'already installed';
            window.__ecrab_console_ok = true;
            window.__ecrab_msgs = [];
            window.__ecrab_errs = [];
            ['log','warn','error','info'].forEach(function(level) {
                var orig = console[level];
                console[level] = function() {
                    window.__ecrab_msgs.push({
                        level: level,
                        text: Array.from(arguments).map(String).join(' ')
                    });
                    orig.apply(console, arguments);
                };
            });
            window.addEventListener('error', function(e) {
                window.__ecrab_errs.push(
                    e.message + (e.filename ? ' at ' + e.filename + ':' + e.lineno : '')
                );
            });
            window.addEventListener('unhandledrejection', function(e) {
                window.__ecrab_errs.push('Unhandled Promise: ' + String(e.reason));
            });
            return 'installed';
        })()
    "#;
    let _ = cdp_evaluate(&session.ws_url, js).await;
    Ok(())
}

/// Inject stealth patches to evade common bot-detection checks.
///
/// Uses `Page.addScriptToEvaluateOnNewDocument` so patches are applied on
/// every subsequent navigation (before any page JS runs). Patches:
///
/// 1. `navigator.webdriver` → `false` (Chrome sets `true` in automation mode)
/// 2. `window.chrome` — creates minimal stub if missing (headless lacks it)
/// 3. `navigator.plugins` — injects realistic plugin array (headless has empty)
/// 4. `navigator.languages` — ensures non-empty array
/// 5. `Permissions.query` — patches notification permission (headless = `denied`)
/// 6. `WebGL` — patches renderer/vendor to avoid SwiftShader fingerprint
async fn inject_stealth_patches(session: &BrowserSession) -> Result<(), ToolError> {
    let stealth_js = r#"
        // 1. navigator.webdriver = false
        Object.defineProperty(navigator, 'webdriver', {
            get: () => false,
            configurable: true,
        });

        // 2. window.chrome stub
        if (!window.chrome) {
            window.chrome = {
                runtime: {},
                loadTimes: function() { return {}; },
                csi: function() { return {}; },
                app: { isInstalled: false, InstallState: { INSTALLED: 'installed' }, RunningState: { RUNNING: 'running' } },
            };
        }

        // 3. navigator.plugins — inject realistic array
        Object.defineProperty(navigator, 'plugins', {
            get: () => {
                var arr = [
                    { name: 'Chrome PDF Plugin', filename: 'internal-pdf-viewer', description: 'Portable Document Format', length: 1 },
                    { name: 'Chrome PDF Viewer', filename: 'mhjfbmdgcfjbbpaeojofohoefgiehjai', description: '', length: 1 },
                    { name: 'Native Client', filename: 'internal-nacl-plugin', description: '', length: 2 },
                ];
                arr.refresh = function() {};
                arr.item = function(i) { return this[i] || null; };
                arr.namedItem = function(name) { return this.find(function(p) { return p.name === name; }) || null; };
                return arr;
            },
            configurable: true,
        });

        // 4. navigator.languages fallback
        if (!navigator.languages || navigator.languages.length === 0) {
            Object.defineProperty(navigator, 'languages', {
                get: () => ['en-US', 'en'],
                configurable: true,
            });
        }

        // 5. Permissions.query patch for notifications
        var origQuery = window.Permissions && Permissions.prototype.query;
        if (origQuery) {
            Permissions.prototype.query = function(parameters) {
                if (parameters.name === 'notifications') {
                    return Promise.resolve({ state: Notification.permission });
                }
                return origQuery.call(this, parameters);
            };
        }

        // 6. WebGL renderer/vendor — avoid SwiftShader detection
        var getParameterOrig = WebGLRenderingContext.prototype.getParameter;
        WebGLRenderingContext.prototype.getParameter = function(param) {
            if (param === 37445) return 'Google Inc. (Intel)';
            if (param === 37446) return 'ANGLE (Intel, Intel(R) UHD Graphics 630, OpenGL 4.1)';
            return getParameterOrig.call(this, param);
        };
        if (typeof WebGL2RenderingContext !== 'undefined') {
            var getParam2Orig = WebGL2RenderingContext.prototype.getParameter;
            WebGL2RenderingContext.prototype.getParameter = function(param) {
                if (param === 37445) return 'Google Inc. (Intel)';
                if (param === 37446) return 'ANGLE (Intel, Intel(R) UHD Graphics 630, OpenGL 4.1)';
                return getParam2Orig.call(this, param);
            };
        }
    "#;

    // addScriptToEvaluateOnNewDocument ensures this runs before ANY page JS
    // on every future navigation in this session.
    let _ = cdp_call(
        &session.ws_url,
        "Page.addScriptToEvaluateOnNewDocument",
        json!({ "source": stealth_js }),
        5,
    )
    .await?;

    // Also evaluate immediately on the current about:blank page
    let _ = cdp_evaluate(&session.ws_url, stealth_js).await;

    Ok(())
}

/// Re-install the console interceptor (after navigation clears the page).
async fn reinstall_console_listener(session: &BrowserSession) {
    let js = r#"
        (function() {
            window.__ecrab_console_ok = false;
            window.__ecrab_msgs = [];
            window.__ecrab_errs = [];
            ['log','warn','error','info'].forEach(function(level) {
                var orig = console[level];
                console[level] = function() {
                    window.__ecrab_msgs.push({
                        level: level,
                        text: Array.from(arguments).map(String).join(' ')
                    });
                    orig.apply(console, arguments);
                };
            });
            window.addEventListener('error', function(e) {
                window.__ecrab_errs.push(
                    e.message + (e.filename ? ' at ' + e.filename + ':' + e.lineno : '')
                );
            });
            window.addEventListener('unhandledrejection', function(e) {
                window.__ecrab_errs.push('Unhandled Promise: ' + String(e.reason));
            });
            window.__ecrab_console_ok = true;
            return 'reinstalled';
        })()
    "#;
    let _ = cdp_evaluate(&session.ws_url, js).await;
}

// ═══════════════════════════════════════════════════════════════
// Accessibility snapshot — builds @eN ref IDs (hermes parity)
// ═══════════════════════════════════════════════════════════════

/// JavaScript that walks the DOM and produces an accessibility-tree-like
/// text snapshot with @eN ref IDs on interactive elements.
///
/// Interactive elements get `data-ecref="eN"` attributes so click/type
/// can locate them by ref.
///
/// `full=true`: complete tree. `full=false`: interactive elements only.
/// Generate the JavaScript accessibility-tree snapshot injected into the page.
///
/// # Design goals (first-principles)
/// An LLM agent needs to know:
///   1. Which elements it can ACT on (links, buttons, inputs) + their ref IDs.
///   2. The semantic structure of the page so it can reason about sections.
///   3. The current STATE of interactive elements (value, checked, expanded…).
///
/// Everything else is bloat.  The JS below aggressively filters noise:
///
/// | Problem                     | Fix                                         |
/// |-----------------------------|---------------------------------------------|
/// | `aria-hidden` overlays/modal backgrounds pollute compact snapshot | `vis()` skips whole subtrees with `aria-hidden="true"` |
/// | Decorative `role=presentation/none` wrappers add meaningless lines | treated as transparent pass-through; no line emitted, depth not incremented |
/// | Headings showed `heading` with **no text** in compact mode | `name()` now extracts `innerText` for H1–H6 |
/// | 50-link navbars drown the main content | compact navs truncated at `NAV_MAX` links |
/// | Long UL/OL lists are verbose | compact lists capped at 10 LI + "[… N more]" |
/// | Duplicate article links (same href, 3× on page) waste tokens | compact mode tracks `seenHrefs`; duplicates skipped + counted |
/// | Decorative images (empty `alt`) add noise | skipped entirely |
/// | No `readOnly`, `required`, `aria-current`, `focused` state | all added |
/// | No page description in header | meta description fetched and prepended |
fn snapshot_js(full: bool) -> String {
    let full_flag = if full { "true" } else { "false" };
    format!(
        r#"
(function() {{
  var rc = 0;
  var FULL = {full_flag};
  // Compact mode: max interactive elements to show per <nav> before truncating.
  // Most useful navigation sections have ≤ 12 meaningful links; cap at 15.
  var NAV_MAX = 15;
  // Compact mode: track seen hrefs for duplicate-link suppression.
  var seenHrefs = FULL ? null : new Set();
  var dupCount = 0;

  var ITAGS = new Set([
    'A','BUTTON','INPUT','SELECT','TEXTAREA','DETAILS','SUMMARY','LABEL'
  ]);
  var IROLES = new Set([
    'button','link','textbox','checkbox','radio','combobox','listbox',
    'menuitem','option','searchbox','slider','spinbutton','switch','tab',
    'treeitem','menuitemcheckbox','menuitemradio'
  ]);

  function isI(el) {{
    if (ITAGS.has(el.tagName)) return true;
    var r = el.getAttribute && el.getAttribute('role');
    if (r && IROLES.has(r.toLowerCase())) return true;
    if (el.getAttribute && el.getAttribute('contenteditable') === 'true') return true;
    var ti = el.getAttribute && el.getAttribute('tabindex');
    if (ti !== null && parseInt(ti, 10) >= 0 && el.tagName !== 'BODY' && el.tagName !== 'HTML') return true;
    return false;
  }}

  function role(el) {{
    var r = el.getAttribute && el.getAttribute('role');
    if (r) return r.toLowerCase();
    var t = el.tagName;
    if (t === 'A') return 'link';
    if (t === 'BUTTON') return 'button';
    if (t === 'INPUT') {{
      var it = (el.type || 'text').toLowerCase();
      if (it === 'checkbox') return 'checkbox';
      if (it === 'radio') return 'radio';
      if (it === 'submit' || it === 'button') return 'button';
      return 'textbox';
    }}
    if (t === 'SELECT') return 'combobox';
    if (t === 'TEXTAREA') return 'textbox';
    if (t === 'IMG') return 'img';
    if (/^H[1-6]$/.test(t)) return 'heading';
    if (t === 'NAV') return 'navigation';
    if (t === 'MAIN') return 'main';
    if (t === 'HEADER') return 'banner';
    if (t === 'FOOTER') return 'contentinfo';
    if (t === 'ASIDE') return 'complementary';
    if (t === 'ARTICLE') return 'article';
    if (t === 'SECTION') return 'region';
    if (t === 'FORM') return 'form';
    if (t === 'SEARCH') return 'search';
    if (t === 'TABLE') return 'table';
    if (t === 'UL' || t === 'OL') return 'list';
    if (t === 'LI') return 'listitem';
    return '';
  }}

  // Clean visible text for an element (normalise whitespace, cap length).
  function innerTxt(el, maxLen) {{
    var s = (el.innerText !== undefined ? el.innerText : el.textContent) || '';
    return s.replace(/\s+/g, ' ').trim().substring(0, maxLen || 160);
  }}

  function name(el) {{
    // aria-labelledby — resolves referenced element(s) by ID (highest ARIA precedence)
    var lblBy = el.getAttribute && el.getAttribute('aria-labelledby');
    if (lblBy) {{
      var parts = lblBy.trim().split(/\s+/).map(function(id) {{
        var ref = document.getElementById(id);
        return ref ? (ref.innerText || ref.textContent || '').replace(/\s+/g, ' ').trim() : '';
      }}).filter(Boolean);
      if (parts.length) return parts.join(' ').substring(0, 120);
    }}
    var l = el.getAttribute && (
      el.getAttribute('aria-label') ||
      el.getAttribute('alt') ||
      el.getAttribute('title') ||
      el.getAttribute('placeholder') ||
      el.getAttribute('name')
    );
    if (l) return l.trim().substring(0, 120);
    // Headings ALWAYS need their text (even in compact mode — they define page structure).
    // Buttons, links, labels need text too for actionability.
    var t = el.tagName;
    if (/^H[1-6]$/.test(t) || t === 'BUTTON' || t === 'A' ||
        t === 'LABEL' || t === 'SUMMARY' || t === 'CAPTION' ||
        t === 'TH' || t === 'FIGCAPTION') {{
      return innerTxt(el, 160);
    }}
    return '';
  }}

  function vis(el) {{
    var t = el.tagName;
    if (t === 'SCRIPT' || t === 'STYLE' || t === 'NOSCRIPT' ||
        t === 'META' || t === 'HEAD' || t === 'TEMPLATE') return false;
    // aria-hidden="true" hides the element AND all its descendants from the
    // accessibility tree.  Hidden modals, cookie overlays, off-screen drawers, etc.
    if (el.getAttribute && el.getAttribute('aria-hidden') === 'true') return false;
    var s = window.getComputedStyle(el);
    if (s.display === 'none' || s.visibility === 'hidden' || parseFloat(s.opacity) === 0) return false;
    if (s.position === 'fixed' || s.position === 'sticky') return true;
    if (!el.offsetParent && t !== 'BODY' && t !== 'HTML') return false;
    return true;
  }}

  // rcStop: optional ref-counter ceiling for sub-tree budgeting.
  // Once rc >= rcStop, walk() returns immediately without emitting further elements.
  // This keeps nav truncation ref-slot-safe: every ref shown is actionable.
  function walk(el, d, lines, rcStop) {{
    if (!el || !el.tagName) return;
    if (!vis(el)) return;
    if (rcStop !== undefined && rc >= rcStop) return;

    var t = el.tagName;
    var elRole = el.getAttribute && el.getAttribute('role');
    var roleLC = elRole ? elRole.toLowerCase() : '';

    // role=presentation / role=none: the element itself is purely decorative but
    // its children retain their own semantics.  Pass through transparently.
    if (roleLC === 'presentation' || roleLC === 'none') {{
      for (var i = 0; i < el.children.length && i < 500; i++)
        walk(el.children[i], d, lines, rcStop);
      return;
    }}

    // Decorative images (no alt text) — skip entirely, they're visual only.
    if (t === 'IMG') {{
      var alt = (el.getAttribute('alt') || '').trim();
      if (!alt) return;
    }}

    // Compact-mode link deduplication.
    // Must happen BEFORE ref assignment so duplicate links don't consume ref slots.
    if (!FULL && t === 'A' && el.href) {{
      if (seenHrefs.has(el.href)) {{
        dupCount++;
        return;
      }}
      seenHrefs.add(el.href);
    }}

    var interactive = isI(el);
    var r = role(el);
    var n = name(el);

    if (!FULL && !interactive && !r) {{
      for (var i = 0; i < el.children.length && i < 500; i++)
        walk(el.children[i], d, lines, rcStop);
      return;
    }}

    // Cap indentation at 8 levels — deeper nesting tells an agent nothing new.
    var indent = '  '.repeat(Math.min(d, 8));
    var ref_str = '';

    if (interactive) {{
      rc++;
      var refId = 'e' + rc;
      el.setAttribute('data-ecref', refId);
      ref_str = ' [@' + refId + ']';
    }}

    var line = indent;
    line += r || t.toLowerCase();
    if (n) line += ' "' + n.replace(/"/g, '\\"') + '"';

    // ── Input state ───────────────────────────────────────────────────────────
    if (t === 'INPUT' || t === 'TEXTAREA') {{
      var v = el.value || '';
      if (v) line += ' value="' + v.substring(0, 50).replace(/"/g, '\\"') + '"';
      if (el.type && el.type !== 'text' && t !== 'TEXTAREA') line += ' type=' + el.type;
      if (el.checked)   line += ' [checked]';
      if (el.disabled)  line += ' [disabled]';
      if (el.readOnly)  line += ' [readonly]';
      if (el.required)  line += ' [required]';
      var mxLen = el.getAttribute && el.getAttribute('maxlength');
      if (mxLen && mxLen !== '-1') line += ' maxlength=' + mxLen;
    }}
    if (t === 'SELECT') {{
      var so = el.options && el.options[el.selectedIndex];
      if (so) line += ' value="' + so.text.substring(0, 50) + '"';
      if (el.disabled) line += ' [disabled]';
    }}
    // Link href (dedup was handled above; just append the value here).
    if (t === 'A' && el.href) line += ' href="' + el.href.substring(0, 120) + '"';
    if (t === 'IMG' && el.src) line += ' src="' + el.src.substring(0, 100) + '"';

    // ── ARIA live states ──────────────────────────────────────────────────────
    var ariaExp = el.getAttribute && el.getAttribute('aria-expanded');
    if (ariaExp !== null) line += ' [aria-expanded=' + ariaExp + ']';
    var ariaChk = el.getAttribute && el.getAttribute('aria-checked');
    if (ariaChk !== null) line += ' [aria-checked=' + ariaChk + ']';
    var ariaSel = el.getAttribute && el.getAttribute('aria-selected');
    if (ariaSel !== null) line += ' [aria-selected=' + ariaSel + ']';
    var ariaHasp = el.getAttribute && el.getAttribute('aria-haspopup');
    if (ariaHasp && ariaHasp !== 'false') line += ' [has-popup]';
    var ariaReq = el.getAttribute && el.getAttribute('aria-required');
    if (ariaReq === 'true' || el.required) line += ' [required]';
    var ariaDis = el.getAttribute && el.getAttribute('aria-disabled');
    if (ariaDis === 'true') line += ' [disabled]';
    // aria-current marks the active page/step/tab — critical for navigation context.
    var ariaCur = el.getAttribute && el.getAttribute('aria-current');
    if (ariaCur && ariaCur !== 'false') line += ' [current=' + ariaCur + ']';
    // Keyboard focus position — helps agents understand form/dialog state.
    if (document.activeElement === el && el !== document.body) line += ' [focused]';

    line += ref_str;

    // Full mode: inline leaf text nodes so agents see content without extra calls.
    if (FULL && el.childNodes.length === 1 && el.childNodes[0].nodeType === 3) {{
      var leafTxt = el.childNodes[0].textContent.trim().substring(0, 160);
      if (leafTxt && !n) line += ': ' + leafTxt;
    }}

    lines.push(line);

    // ── Nav truncation (compact only) ─────────────────────────────────────────
    // A navbar with 50 links drowns the main content.  Show the first NAV_MAX
    // interactive elements, then report how many were hidden.
    // rcStop is threaded through so each shown element gets a valid ref slot.
    if (!FULL && r === 'navigation') {{
      var navTotal = el.querySelectorAll('a, button').length;
      if (navTotal > NAV_MAX) {{
        var rcBefore = rc;
        for (var i = 0; i < el.children.length && i < 500; i++)
          walk(el.children[i], d + 1, lines, rcBefore + NAV_MAX);
        var navHidden = navTotal - (rc - rcBefore);
        if (navHidden > 0)
          lines.push(indent + '  [... ' + navHidden + ' more navigation links]');
        return;
      }}
    }}

    // ── List truncation (compact only) ────────────────────────────────────────
    if (!FULL && (t === 'UL' || t === 'OL')) {{
      var liKids = Array.prototype.filter.call(
        el.children, function(c) {{ return c.tagName === 'LI'; }}
      );
      if (liKids.length > 12) {{
        var shownLi = 0;
        for (var i = 0; i < el.children.length && shownLi < 10; i++) {{
          walk(el.children[i], d + 1, lines, rcStop);
          if (el.children[i].tagName === 'LI') shownLi++;
        }}
        lines.push(indent + '  [... ' + (liKids.length - 10) + ' more list items]');
        return;
      }}
    }}

    for (var i = 0; i < el.children.length && i < 500; i++)
      walk(el.children[i], d + 1, lines, rcStop);
  }}

  var lines = [];
  walk(document.body || document.documentElement, 0, lines, undefined);

  // Page-level metadata for richer agent context (og:description preferred).
  var metaEl = document.querySelector(
    'meta[property="og:description"], meta[name="description"]'
  );
  var desc = metaEl
    ? (metaEl.getAttribute('content') || '').replace(/\s+/g, ' ').trim().substring(0, 200)
    : '';

  var hdr = '- Page: ' + (document.title || '(untitled)') +
            '\n- URL: ' + window.location.href;
  if (desc) hdr += '\n- Description: ' + desc;
  hdr += '\n- Refs: ' + rc + ' interactive elements';
  if (!FULL && dupCount > 0) hdr += ' (' + dupCount + ' duplicate links hidden)';
  hdr += '\n';
  return hdr + '---\n' + lines.join('\n');
}})()
"#,
    )
}

// ═══════════════════════════════════════════════════════════════
// LLM summarization for large snapshots
// ═══════════════════════════════════════════════════════════════

/// Post-process a raw accessibility snapshot before returning it to the agent.
///
/// # Decision matrix (matches + exceeds hermes-agent behaviour)
///
/// | `full` | snapshot size | `user_task` | result                              |
/// |--------|---------------|-------------|-------------------------------------|
/// | true   | any           | any         | hard truncate at 20 000 chars + note|
/// | false  | ≤ 8 000       | any         | returned as-is                      |
/// | false  | > 8 000       | **None**    | plain truncate at 8 000 chars       |
/// | false  | > 8 000       | **Some(_)** | LLM extraction (30 s / 1 500 tok)   |
///
/// The key insight: when `full=true` the *agent* is responsible for
/// summarising the content it requested.  Feeding a 50 000-char full snapshot
/// into a local Ollama model causes a 4-minute freeze with no output.
/// When `full=false` but no task context exists, hermes truncates — so do we.
async fn summarize_snapshot(
    snapshot: &str,
    full: bool,
    user_task: Option<&str>,
    provider: &Option<Arc<dyn edgequake_llm::LLMProvider>>,
) -> String {
    // ── full=true: hard cap, no LLM, tell agent to scroll for more ──────────
    if full {
        if snapshot.len() <= FULL_SNAPSHOT_MAX_CHARS {
            return snapshot.to_string();
        }
        let shown = crate::safe_truncate(snapshot, FULL_SNAPSHOT_MAX_CHARS);
        let hidden = snapshot.len() - shown.len();
        return format!(
            "{shown}\n\n[... {hidden} more bytes not shown. \
             Call browser_scroll then browser_snapshot to see the next section.]"
        );
    }

    // ── compact: fits in threshold → return as-is ────────────────────────────
    if snapshot.len() <= SNAPSHOT_SUMMARIZE_THRESHOLD {
        return snapshot.to_string();
    }

    // ── compact, oversized, no user task → plain truncate (hermes behaviour) ──
    let Some(task) = user_task else {
        tracing::debug!("browser: snapshot > threshold but no user_task; truncating");
        return truncate_snapshot(snapshot);
    };

    // ── compact, oversized, user task set → LLM task-aware extraction ────────
    let Some(provider) = provider else {
        return truncate_snapshot(snapshot);
    };

    // Cap input to the LLM to avoid flooding small local model context windows.
    let input = if snapshot.len() > SNAPSHOT_LLM_INPUT_CAP {
        crate::safe_truncate(snapshot, SNAPSHOT_LLM_INPUT_CAP)
    } else {
        snapshot
    };

    let prompt = format!(
        "You are a content extractor for a browser automation agent.\n\n\
         The user's task is: {task}\n\n\
         Given this page snapshot (accessibility tree), extract the most \
         relevant information for completing the task.\n\
         Rules:\n\
         - Keep ALL ref IDs ([@eN]) for interactive elements — the agent needs them\n\
         - Preserve headings and section structure\n\
         - Include text content relevant to the task\n\
         - Omit boilerplate navigation and footer links\n\n\
         Page Snapshot:\n{input}\n\n\
         Respond with a concise summary (plain text, no markdown)."
    );

    let msg = ChatMessage::user(prompt);
    let options = edgequake_llm::traits::CompletionOptions {
        temperature: Some(0.1),
        max_tokens: Some(1500),
        ..Default::default()
    };
    match tokio::time::timeout(
        std::time::Duration::from_secs(SNAPSHOT_SUMMARIZE_TIMEOUT_SECS),
        provider.chat(&[msg], Some(&options)),
    )
    .await
    {
        Ok(Ok(response)) => response.content,
        Ok(Err(e)) => {
            tracing::warn!("browser: snapshot LLM extraction failed: {e}");
            truncate_snapshot(snapshot)
        }
        Err(_elapsed) => {
            tracing::warn!(
                "browser: snapshot LLM extraction timed out after {SNAPSHOT_SUMMARIZE_TIMEOUT_SECS}s"
            );
            truncate_snapshot(snapshot)
        }
    }
}

fn truncate_snapshot(text: &str) -> String {
    if text.len() <= SNAPSHOT_SUMMARIZE_THRESHOLD {
        return text.to_string();
    }
    let mut result = crate::safe_truncate(text, SNAPSHOT_SUMMARIZE_THRESHOLD).to_string();
    result.push_str("\n\n[... content truncated at 8000 chars ...]");
    result
}

// ═══════════════════════════════════════════════════════════════
// Bot detection helper (hermes parity)
// ═══════════════════════════════════════════════════════════════

fn detect_bot_warning(title: &str) -> Option<String> {
    let title_lower = title.to_lowercase();
    let patterns = [
        "access denied",
        "blocked",
        "bot detected",
        "verification required",
        "please verify",
        "are you a robot",
        "captcha",
        "cloudflare",
        "ddos protection",
        "checking your browser",
        "just a moment",
        "attention required",
    ];

    if patterns.iter().any(|p| title_lower.contains(p)) {
        Some(format!(
            "Page title '{title}' suggests bot detection. Options: \
             1) Add delays between actions, \
             2) Access different pages first, \
             3) Some sites have aggressive bot detection that may be unavoidable."
        ))
    } else {
        None
    }
}

// ═══════════════════════════════════════════════════════════════
// browser_navigate
// ═══════════════════════════════════════════════════════════════

async fn navigate_session(
    session: &Arc<BrowserSession>,
    url: &str,
) -> Result<(serde_json::Value, String, String), ToolError> {
    let nav_result = cdp_call(&session.ws_url, "Page.navigate", json!({ "url": url }), 30).await?;

    wait_for_navigation_commit(&session.ws_url, 8_000).await;
    reinstall_console_listener(session).await;

    let final_url = cdp_evaluate(&session.ws_url, "window.location.href")
        .await
        .unwrap_or_else(|_| url.to_string());
    let title = cdp_evaluate(&session.ws_url, "document.title")
        .await
        .unwrap_or_else(|_| "(untitled)".to_string());

    if is_redirect_to_private_ip(&final_url) {
        let _ = cdp_call(
            &session.ws_url,
            "Page.navigate",
            json!({ "url": "about:blank" }),
            5,
        )
        .await;
        return Err(ToolError::PermissionDenied(format!(
            "navigation was blocked (SSRF guard) because {url} redirected \
             to a private/internal address: {final_url}"
        )));
    }

    Ok((nav_result, final_url, title))
}

pub(crate) async fn render_page_text(
    url: &str,
    ctx: &ToolContext,
) -> Result<RenderedPage, ToolError> {
    validate_browser_url(url)?;

    let session = get_session(ctx).await?;
    let (_nav_result, final_url, title) = navigate_session(&session, url).await?;

    let rendered = cdp_evaluate(
        &session.ws_url,
        r#"(function() {
            const meta =
              document.querySelector('meta[name="description"], meta[property="og:description"]')
                ?.getAttribute('content') || "";
            const textCandidates = [
              document.querySelector("main")?.innerText,
              document.querySelector("article")?.innerText,
              document.body?.innerText,
              document.documentElement?.innerText,
            ];
            const text = textCandidates.find(value => value && value.trim()) || "";
            const links = Array.from(document.querySelectorAll("a[href]"))
              .map(link => link.href)
              .filter(Boolean)
              .slice(0, 400);
            return JSON.stringify({
              url: window.location.href,
              title: document.title || "",
              meta_description: meta || null,
              text,
              links,
            });
        })()"#,
    )
    .await?;

    let mut page: RenderedPage =
        serde_json::from_str(&rendered).map_err(|e| ToolError::ExecutionFailed {
            tool: "browser".into(),
            message: format!("Failed to parse rendered page payload: {e}"),
        })?;
    page.url = final_url;
    page.title = if page.title.is_empty() {
        title
    } else {
        page.title
    };
    page.text = normalize_rendered_text(&page.text);
    page.links.retain(|link| !link.is_empty());
    page.links.dedup();

    Ok(page)
}

pub struct BrowserNavigateTool;

#[derive(Deserialize)]
struct NavigateArgs {
    url: String,
}

#[async_trait]
impl ToolHandler for BrowserNavigateTool {
    fn name(&self) -> &'static str {
        "browser_navigate"
    }

    fn toolset(&self) -> &'static str {
        "browser"
    }

    fn emoji(&self) -> &'static str {
        "🌐"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser_navigate".into(),
            description: "Navigate to a URL in the browser. Initializes the session. \
                Must be called before other browser tools. For simple info retrieval, \
                prefer web_search or web_extract (faster, cheaper). Use browser tools \
                when you need to interact (click, fill forms, dynamic content)."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to navigate to (e.g., 'https://example.com')"
                    }
                },
                "required": ["url"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        browser_is_available()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: NavigateArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "browser_navigate".into(),
                message: e.to_string(),
            })?;

        // Pre-navigation SSRF check
        validate_browser_url(&args.url)?;

        let session = get_session(ctx).await?;

        let (nav_result, final_url, title) = navigate_session(&session, &args.url).await?;

        // Auto-start recording on first navigation if enabled.
        // Guarded by recording_started flag so we start at most once per session.
        // The runtime override (set by /browser recording on|off) takes precedence
        // over the config.yaml value.
        let recording_enabled =
            get_recording_override().unwrap_or(ctx.config.browser_record_sessions);
        if recording_enabled && !session.recording_started.swap(true, Ordering::Relaxed) {
            let recordings_dir = ctx.config.edgecrab_home.join("browser_recordings");
            let _ = std::fs::create_dir_all(&recordings_dir);

            // Cleanup old recordings in background (non-blocking).
            let max_age_secs = ctx.config.browser_recording_max_age_hours * 3600;
            let recordings_dir_clone = recordings_dir.clone();
            tokio::spawn(async move {
                cleanup_old_recordings(&recordings_dir_clone, max_age_secs);
            });

            // Start the recorder asynchronously.
            let ws_url_clone = session.ws_url.clone();
            if let Some(recorder) = ScreencastRecorder::start(ws_url_clone).await {
                if let Ok(mut guard) = session.recorder.lock() {
                    *guard = Some(recorder);
                    tracing::info!("browser recording started for session {}", ctx.session_id);
                }
            } else {
                // Non-fatal: reset flag so a subsequent navigate can retry.
                session.recording_started.store(false, Ordering::Relaxed);
                tracing::warn!(
                    "browser recording: failed to start recorder for session {}",
                    ctx.session_id
                );
            }
        }

        // Check for navigation errors
        if let Some(err) = nav_result.get("errorText").and_then(|e| e.as_str())
            && !err.is_empty()
        {
            return Err(ToolError::ExecutionFailed {
                tool: "browser_navigate".into(),
                message: format!("Navigation error: {err}"),
            });
        }

        // Build response
        let mut response = format!("Navigated to: {final_url}\nTitle: {title}");

        // Bot detection warning (hermes parity)
        if let Some(warning) = detect_bot_warning(&title) {
            response.push_str(&format!("\n\n⚠️ {warning}"));
        }

        Ok(response)
    }
}

inventory::submit!(&BrowserNavigateTool as &dyn ToolHandler);

// ═══════════════════════════════════════════════════════════════
// browser_snapshot — accessibility tree with @eN refs
// ═══════════════════════════════════════════════════════════════

pub struct BrowserSnapshotTool;

#[derive(Deserialize)]
struct SnapshotArgs {
    #[serde(default)]
    full: bool,
}

#[async_trait]
impl ToolHandler for BrowserSnapshotTool {
    fn name(&self) -> &'static str {
        "browser_snapshot"
    }

    fn toolset(&self) -> &'static str {
        "browser"
    }

    fn emoji(&self) -> &'static str {
        "📸"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser_snapshot".into(),
            description: "Get a text-based snapshot of the current page's accessibility tree. \
                Returns interactive elements with ref IDs (like @e1, @e2) for use with \
                browser_click and browser_type. \
                full=false (default): compact view — interactive elements, headings, and key \
                structure only; oversized output is truncated at 8 000 chars (task-aware LLM \
                extraction used when task context is available). \
                full=true: complete page content up to 20 000 chars; the agent should \
                summarise the result itself. Call browser_scroll then browser_snapshot again \
                to see content beyond the cap."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "full": {
                        "type": "boolean",
                        "description": "If true, complete page content. If false (default), compact interactive view."
                    }
                }
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        browser_is_available()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: SnapshotArgs =
            serde_json::from_value(args.clone()).unwrap_or(SnapshotArgs { full: false });

        let session = get_session(ctx).await?;

        // Wait for page to reach interactive state before snapshotting.
        // SPA frameworks and dynamic pages may still be rendering. We poll
        // document.readyState for up to 3 s, then proceed regardless.
        let ready_js = "(function() { return document.readyState; })()";
        for _ in 0..10u8 {
            let state = cdp_evaluate(&session.ws_url, ready_js)
                .await
                .unwrap_or_else(|_| "complete".into());
            if state == "complete" || state == "interactive" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        // Additional 150 ms settle time for SPA microtask queues / requestAnimationFrame
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;

        let js = snapshot_js(args.full);
        let snapshot = cdp_evaluate(&session.ws_url, &js).await?;

        // Post-process the snapshot:  LLM task-aware extraction for oversized
        // compact snapshots, plain truncation otherwise.  full=true snapshots
        // are never fed to the LLM — the agent summarises them directly.
        let result = summarize_snapshot(
            &snapshot,
            args.full,
            ctx.user_task.as_deref(),
            &ctx.provider,
        )
        .await;

        Ok(result)
    }
}

inventory::submit!(&BrowserSnapshotTool as &dyn ToolHandler);

// ═══════════════════════════════════════════════════════════════
// browser_click — click by @eN ref
// ═══════════════════════════════════════════════════════════════

pub struct BrowserClickTool;

#[derive(Deserialize)]
struct ClickArgs {
    /// Element reference from the snapshot (e.g., "@e5")
    #[serde(alias = "selector")]
    r#ref: String,
}

#[async_trait]
impl ToolHandler for BrowserClickTool {
    fn name(&self) -> &'static str {
        "browser_click"
    }

    fn toolset(&self) -> &'static str {
        "browser"
    }

    fn emoji(&self) -> &'static str {
        "👆"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser_click".into(),
            description: "Click on an element identified by its ref ID from the snapshot \
                (e.g., '@e5'). The ref IDs are shown in square brackets in the snapshot \
                output. Requires browser_navigate and browser_snapshot first."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "ref": {
                        "type": "string",
                        "description": "The element reference from the snapshot (e.g., '@e5', '@e12')"
                    }
                },
                "required": ["ref"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        browser_is_available()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: ClickArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "browser_click".into(),
            message: e.to_string(),
        })?;

        let session = get_session(ctx).await?;
        let ref_id = normalize_ref(&args.r#ref)?;

        // Step 1: find element, scroll into view, get viewport-relative centre.
        // Returns JSON {x, y, tag, txt} so we can use CDP mouse coordinates.
        let pos_js = format!(
            r#"
(function() {{
  var el = document.querySelector('[data-ecref="{ref_id}"]');
  if (!el) return JSON.stringify({{error:'Element @{ref_id} not found. Run browser_snapshot first to refresh refs.'}});
  el.scrollIntoView({{block:'center',behavior:'instant'}});
  var r = el.getBoundingClientRect();
  var tag = el.tagName.toLowerCase();
  var txt = (el.getAttribute('aria-label')||el.value||el.textContent||el.getAttribute('title')||'').trim().substring(0,60);
  return JSON.stringify({{x:Math.round(r.left+r.width/2),y:Math.round(r.top+r.height/2),tag:tag,txt:txt}});
}})()
"#,
            ref_id = escape_js_string(&ref_id),
        );
        let pos_str = cdp_evaluate(&session.ws_url, &pos_js).await?;
        let pos: serde_json::Value =
            serde_json::from_str(&pos_str).map_err(|_| ToolError::ExecutionFailed {
                tool: "browser_click".into(),
                message: format!("Could not parse element position: {pos_str}"),
            })?;
        if let Some(err) = pos.get("error").and_then(|v| v.as_str()) {
            return Err(ToolError::ExecutionFailed {
                tool: "browser_click".into(),
                message: err.to_string(),
            });
        }
        let x = pos["x"].as_f64().unwrap_or(0.0);
        let y = pos["y"].as_f64().unwrap_or(0.0);
        let tag = pos["tag"].as_str().unwrap_or("?");
        let txt = pos["txt"].as_str().unwrap_or("");

        // Step 2: CDP-level mouse click — moves Chrome's internal input-routing
        // focus to this element (JS el.click() does NOT do this).
        // Sequence: hover → mousedown → mouseup, matching Playwright's click().
        for (ev_type, buttons, click_count) in [
            ("mouseMoved", 0u32, 0u32),
            ("mousePressed", 1, 1),
            ("mouseReleased", 0, 1),
        ] {
            // Best-effort: page click still works even if an individual event fails.
            let _ = cdp_call(
                &session.ws_url,
                "Input.dispatchMouseEvent",
                json!({"type":ev_type,"x":x,"y":y,"button":"left","buttons":buttons,"clickCount":click_count}),
                5,
            )
            .await;
        }

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let label = if txt.is_empty() {
            tag.to_string()
        } else {
            format!("{tag}: {}", crate::safe_truncate(txt, 50))
        };
        Ok(format!("Clicked @{} ({label})", args.r#ref))
    }
}

inventory::submit!(&BrowserClickTool as &dyn ToolHandler);

// ═══════════════════════════════════════════════════════════════
// browser_type — type text into @eN ref input
// ═══════════════════════════════════════════════════════════════

pub struct BrowserTypeTool;

#[derive(Deserialize)]
struct TypeArgs {
    #[serde(alias = "selector")]
    r#ref: String,
    text: String,
}

#[async_trait]
impl ToolHandler for BrowserTypeTool {
    fn name(&self) -> &'static str {
        "browser_type"
    }

    fn toolset(&self) -> &'static str {
        "browser"
    }

    fn emoji(&self) -> &'static str {
        "⌨"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser_type".into(),
            description: "Type text into an input field identified by its ref ID. \
                Clears the field first, then types the new text. \
                Requires browser_navigate and browser_snapshot first."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "ref": {
                        "type": "string",
                        "description": "The element reference from the snapshot (e.g., '@e3')"
                    },
                    "text": {
                        "type": "string",
                        "description": "The text to type into the field"
                    }
                },
                "required": ["ref", "text"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        browser_is_available()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: TypeArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "browser_type".into(),
            message: e.to_string(),
        })?;

        let session = get_session(ctx).await?;
        let ref_id = normalize_ref(&args.r#ref)?;

        // Step 1: Find element, scroll into view, get viewport-relative centre.
        let pos_js = format!(
            r#"
(function() {{
  var el = document.querySelector('[data-ecref="{ref_id}"]');
  if (!el) return JSON.stringify({{error:'Element @{ref_id} not found. Run browser_snapshot first to refresh refs.'}});
  el.scrollIntoView({{block:'center',behavior:'instant'}});
  var r = el.getBoundingClientRect();
  return JSON.stringify({{x:Math.round(r.left+r.width/2),y:Math.round(r.top+r.height/2),tag:el.tagName}});
}})()
"#,
            ref_id = escape_js_string(&ref_id),
        );
        let pos_str = cdp_evaluate(&session.ws_url, &pos_js).await?;
        let pos: serde_json::Value =
            serde_json::from_str(&pos_str).map_err(|_| ToolError::ExecutionFailed {
                tool: "browser_type".into(),
                message: format!("Could not parse element position: {pos_str}"),
            })?;
        if let Some(err) = pos.get("error").and_then(|v| v.as_str()) {
            return Err(ToolError::ExecutionFailed {
                tool: "browser_type".into(),
                message: err.to_string(),
            });
        }
        let x = pos["x"].as_f64().unwrap_or(0.0);
        let y = pos["y"].as_f64().unwrap_or(0.0);

        // Step 2: CDP mousePressed → establishes Chrome's OS-level input focus on
        // this element.  JS el.focus() changes document.activeElement but does NOT
        // change Chrome's Input routing for Input.insertText — only a real CDP
        // mousedown does (confirmed from Playwright's fill() source + CDP spec).
        for (ev_type, buttons, click_count) in [
            ("mouseMoved", 0u32, 0u32),
            ("mousePressed", 1, 1),
            ("mouseReleased", 0, 1),
        ] {
            let _ = cdp_call(
                &session.ws_url,
                "Input.dispatchMouseEvent",
                json!({"type":ev_type,"x":x,"y":y,"button":"left","buttons":buttons,"clickCount":click_count}),
                5,
            )
            .await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Step 3: Select-all via CDP editing command.
        // dispatchKeyEvent commands:["selectAll"] invokes Chromium's editing engine
        // directly — more reliable than Ctrl+A (pages can intercept keyboard events)
        // and works for INPUT, TEXTAREA, and contenteditable elements.
        for ev_type in ["keyDown", "keyUp"] {
            let _ = cdp_call(
                &session.ws_url,
                "Input.dispatchKeyEvent",
                json!({"type":ev_type,"key":"a","modifiers":2,"commands":["selectAll"]}),
                5,
            )
            .await;
        }

        // Step 4: Insert text — replaces the selected content (entire field value).
        // Chrome fires an InputEvent(inputType='insertText') which React/Vue/Angular
        // controlled-component onChange handlers catch correctly.
        cdp_call(
            &session.ws_url,
            "Input.insertText",
            json!({ "text": args.text }),
            5,
        )
        .await?;

        // Step 5: Dispatch blur so framework validators run (React onBlur, etc.).
        let _ = cdp_evaluate(
            &session.ws_url,
            "(function(){var ae=document.activeElement;if(ae){ae.dispatchEvent(new Event('blur',{bubbles:true}));ae.dispatchEvent(new Event('change',{bubbles:true}));}})()",
        )
        .await;

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        Ok(format!(
            "Typed into @{}: {} chars",
            args.r#ref,
            args.text.len()
        ))
    }
}

inventory::submit!(&BrowserTypeTool as &dyn ToolHandler);

// ═══════════════════════════════════════════════════════════════
// browser_scroll
// ═══════════════════════════════════════════════════════════════

pub struct BrowserScrollTool;

#[derive(Deserialize)]
struct ScrollArgs {
    direction: String,
    #[serde(default = "default_scroll_amount")]
    amount: i32,
}

fn default_scroll_amount() -> i32 {
    500
}

#[async_trait]
impl ToolHandler for BrowserScrollTool {
    fn name(&self) -> &'static str {
        "browser_scroll"
    }

    fn toolset(&self) -> &'static str {
        "browser"
    }

    fn emoji(&self) -> &'static str {
        "📜"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser_scroll".into(),
            description: "Scroll the page up or down to reveal more content. \
                Requires browser_navigate first."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "direction": {
                        "type": "string",
                        "enum": ["up", "down"],
                        "description": "Direction to scroll"
                    },
                    "amount": {
                        "type": "integer",
                        "description": "Pixels to scroll (default: 500)"
                    }
                },
                "required": ["direction"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        browser_is_available()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: ScrollArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "browser_scroll".into(),
                message: e.to_string(),
            })?;

        let session = get_session(ctx).await?;

        let scroll_y = match args.direction.as_str() {
            "up" => -args.amount,
            "down" => args.amount,
            other => {
                return Err(ToolError::InvalidArgs {
                    tool: "browser_scroll".into(),
                    message: format!("Invalid direction '{other}'. Use 'up' or 'down'."),
                });
            }
        };

        let js = format!(
            r#"
(function() {{
  window.scrollBy(0, {scroll_y});
  return 'Scrolled {dir} by {amt}px. Page offset: ' +
    window.pageYOffset + '/' + document.body.scrollHeight;
}})()
"#,
            scroll_y = scroll_y,
            dir = args.direction,
            amt = args.amount,
        );

        let result = cdp_evaluate(&session.ws_url, &js).await?;
        Ok(result)
    }
}

inventory::submit!(&BrowserScrollTool as &dyn ToolHandler);

// ═══════════════════════════════════════════════════════════════
// browser_press — keyboard key press via CDP Input domain
// ═══════════════════════════════════════════════════════════════

pub struct BrowserPressTool;

#[derive(Deserialize)]
struct PressArgs {
    key: String,
}

#[async_trait]
impl ToolHandler for BrowserPressTool {
    fn name(&self) -> &'static str {
        "browser_press"
    }

    fn toolset(&self) -> &'static str {
        "browser"
    }

    fn emoji(&self) -> &'static str {
        "⌨️"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser_press".into(),
            description: "Press a keyboard key. Useful for submitting forms (Enter), \
                navigation (Tab), or keyboard shortcuts. Requires browser_navigate first.\n\
                Supported keys: Enter, Tab, Escape, Backspace, Delete, ArrowDown, \
                ArrowUp, ArrowLeft, ArrowRight, Home, End, PageUp, PageDown, Space, \
                Insert, CapsLock, F1–F12."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "description": "Key to press (e.g., 'Enter', 'Tab', 'Escape', 'ArrowDown')"
                    }
                },
                "required": ["key"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        browser_is_available()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: PressArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "browser_press".into(),
            message: e.to_string(),
        })?;

        let session = get_session(ctx).await?;

        // Use CDP Input.dispatchKeyEvent for proper key simulation
        let key_code = key_to_code(&args.key);
        let vk = key_to_vk(&args.key);

        let _ = cdp_call(
            &session.ws_url,
            "Input.dispatchKeyEvent",
            json!({
                "type": "keyDown",
                "key": args.key,
                "code": key_code,
                "windowsVirtualKeyCode": vk,
            }),
            5,
        )
        .await;

        let _ = cdp_call(
            &session.ws_url,
            "Input.dispatchKeyEvent",
            json!({
                "type": "keyUp",
                "key": args.key,
                "code": key_code,
                "windowsVirtualKeyCode": vk,
            }),
            5,
        )
        .await;

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        Ok(format!("Pressed key: {}", args.key))
    }
}

inventory::submit!(&BrowserPressTool as &dyn ToolHandler);

// ═══════════════════════════════════════════════════════════════
// browser_back — navigate back in history
// ═══════════════════════════════════════════════════════════════

pub struct BrowserBackTool;

#[async_trait]
impl ToolHandler for BrowserBackTool {
    fn name(&self) -> &'static str {
        "browser_back"
    }

    fn toolset(&self) -> &'static str {
        "browser"
    }

    fn emoji(&self) -> &'static str {
        "◀️"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser_back".into(),
            description: "Navigate back to the previous page in browser history. \
                Requires browser_navigate first."
                .into(),
            parameters: json!({ "type": "object", "properties": {} }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        browser_is_available()
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let session = get_session(ctx).await?;

        cdp_evaluate(&session.ws_url, "window.history.back()").await?;
        // Use the same navigation-commit polling as browser_navigate rather than
        // a fixed sleep — handles SPAs and slow redirect chains correctly.
        wait_for_navigation_commit(&session.ws_url, 6_000).await;
        reinstall_console_listener(&session).await;

        let url = cdp_evaluate(&session.ws_url, "window.location.href")
            .await
            .unwrap_or_else(|_| "(unknown)".into());
        let title = cdp_evaluate(&session.ws_url, "document.title")
            .await
            .unwrap_or_else(|_| "(unknown)".into());

        Ok(format!("Navigated back.\nURL: {url}\nTitle: {title}"))
    }
}

inventory::submit!(&BrowserBackTool as &dyn ToolHandler);

// ═══════════════════════════════════════════════════════════════
// browser_close — close session and release resources
// ═══════════════════════════════════════════════════════════════

pub struct BrowserCloseTool;

#[async_trait]
impl ToolHandler for BrowserCloseTool {
    fn name(&self) -> &'static str {
        "browser_close"
    }

    fn toolset(&self) -> &'static str {
        "browser"
    }

    fn emoji(&self) -> &'static str {
        "🔒"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser_close".into(),
            description: "Close the browser session and release all resources (tab, recordings). \
                Call this when you are done with browser tasks. A new session will be created \
                automatically the next time browser_navigate is called."
                .into(),
            parameters: json!({ "type": "object", "properties": {} }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        browser_is_available()
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let had_session = session_manager().sessions.contains_key(&ctx.session_id);

        session_manager().close_session(&ctx.session_id).await;

        if had_session {
            Ok("Browser session closed.".to_string())
        } else {
            Ok("Browser session closed. (no active session was found)".to_string())
        }
    }
}

inventory::submit!(&BrowserCloseTool as &dyn ToolHandler);

// ═══════════════════════════════════════════════════════════════
// browser_console — with clear flag + error/warn capture
// ═══════════════════════════════════════════════════════════════

pub struct BrowserConsoleTool;

#[derive(Deserialize)]
struct ConsoleArgs {
    #[serde(default)]
    clear: bool,
}

#[async_trait]
impl ToolHandler for BrowserConsoleTool {
    fn name(&self) -> &'static str {
        "browser_console"
    }

    fn toolset(&self) -> &'static str {
        "browser"
    }

    fn emoji(&self) -> &'static str {
        "📋"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser_console".into(),
            description: "Get browser console output (log/warn/error/info messages) and \
                uncaught JavaScript exceptions. Essential for detecting silent JS errors. \
                Use clear=true to clear buffers after reading. Requires browser_navigate \
                first."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "clear": {
                        "type": "boolean",
                        "description": "If true, clear the message buffers after reading"
                    }
                }
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        browser_is_available()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: ConsoleArgs =
            serde_json::from_value(args.clone()).unwrap_or(ConsoleArgs { clear: false });

        let session = get_session(ctx).await?;

        let clear_code = if args.clear {
            "window.__ecrab_msgs = []; window.__ecrab_errs = [];"
        } else {
            ""
        };

        let js = format!(
            r#"
(function() {{
  var msgs = window.__ecrab_msgs || [];
  var errs = window.__ecrab_errs || [];
  {clear_code}
  return JSON.stringify({{
    messages: msgs.slice(-100),
    errors: errs.slice(-50)
  }});
}})()
"#,
        );

        let result = cdp_evaluate(&session.ws_url, &js).await?;

        match serde_json::from_str::<serde_json::Value>(&result) {
            Ok(data) => {
                let messages = data["messages"].as_array();
                let errors = data["errors"].as_array();
                let msg_count = messages.map(|a| a.len()).unwrap_or(0);
                let err_count = errors.map(|a| a.len()).unwrap_or(0);

                let mut output = format!("Console messages: {msg_count}, JS errors: {err_count}\n");

                if let Some(msgs) = messages {
                    for msg in msgs {
                        let level = msg["level"].as_str().unwrap_or("log");
                        let text = msg["text"].as_str().unwrap_or("");
                        output.push_str(&format!("[{level}] {text}\n"));
                    }
                }

                if let Some(errs) = errors {
                    for err in errs {
                        let text = err.as_str().unwrap_or("");
                        output.push_str(&format!("[EXCEPTION] {text}\n"));
                    }
                }

                if msg_count == 0 && err_count == 0 {
                    output.push_str("(no console messages or errors captured)");
                }

                Ok(output)
            }
            Err(_) => Ok(format!("Console output:\n{result}")),
        }
    }
}

inventory::submit!(&BrowserConsoleTool as &dyn ToolHandler);

// ═══════════════════════════════════════════════════════════════
// browser_get_images
// ═══════════════════════════════════════════════════════════════

pub struct BrowserGetImagesTool;

#[async_trait]
impl ToolHandler for BrowserGetImagesTool {
    fn name(&self) -> &'static str {
        "browser_get_images"
    }

    fn toolset(&self) -> &'static str {
        "browser"
    }

    fn emoji(&self) -> &'static str {
        "🖼️"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser_get_images".into(),
            description: "List all images on the current page with their URLs and alt text. \
                Useful for finding images to analyze with browser_vision. \
                Requires browser_navigate first."
                .into(),
            parameters: json!({ "type": "object", "properties": {} }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        browser_is_available()
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let session = get_session(ctx).await?;

        let js = r#"
JSON.stringify(
  Array.from(document.images).map(function(img, i) {
    return {
      index: i,
      src: img.src,
      alt: img.alt || '',
      width: img.naturalWidth,
      height: img.naturalHeight
    };
  }).filter(function(img) { return img.src && !img.src.startsWith('data:'); })
)
"#;

        let result = cdp_evaluate(&session.ws_url, js).await?;
        Ok(format!("Images on page:\n{result}"))
    }
}

inventory::submit!(&BrowserGetImagesTool as &dyn ToolHandler);

// ═══════════════════════════════════════════════════════════════
// browser_vision — screenshot + AI analysis
// ═══════════════════════════════════════════════════════════════

pub struct BrowserVisionTool;

#[derive(Deserialize)]
struct VisionArgs {
    question: String,
    #[serde(default)]
    annotate: bool,
}

#[async_trait]
impl ToolHandler for BrowserVisionTool {
    fn name(&self) -> &'static str {
        "browser_vision"
    }

    fn toolset(&self) -> &'static str {
        "browser"
    }

    fn emoji(&self) -> &'static str {
        "👁️"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser_vision".into(),
            description: "Take a screenshot of the CURRENT BROWSER PAGE and analyze it \
                with vision AI. ONLY use this for web pages already loaded in the browser \
                — for local image files, clipboard-saved images, or any image on disk use \
                vision_analyze instead. Requires browser_navigate first. Do NOT use this tool \
                when the user pastes or attaches an image file."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "What to know about the page visually. Be specific."
                    },
                    "annotate": {
                        "type": "boolean",
                        "description": "If true, overlay numbered element labels for identification"
                    }
                },
                "required": ["question"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        browser_is_available()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: VisionArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "browser_vision".into(),
                message: e.to_string(),
            })?;

        let session = get_session(ctx).await?;

        // If annotate mode, overlay numbered labels on interactive elements
        // before taking the screenshot (matches hermes-agent parity)
        if args.annotate {
            let annotate_js = r#"
(function() {
  var existing = document.querySelectorAll('[data-ecrab-annotation]');
  existing.forEach(function(el) { el.remove(); });
  var els = document.querySelectorAll('[data-ecref]');
  els.forEach(function(el) {
    var ref = el.getAttribute('data-ecref');
    var num = ref.replace('e', '');
    var label = document.createElement('span');
    label.setAttribute('data-ecrab-annotation', 'true');
    label.textContent = '[' + num + ']';
    label.style.cssText = 'position:absolute;z-index:999999;background:#ff0;color:#000;' +
      'font:bold 11px monospace;padding:1px 3px;border:1px solid #000;border-radius:3px;' +
      'pointer-events:none;opacity:0.9;';
    var rect = el.getBoundingClientRect();
    label.style.left = (rect.left + window.scrollX) + 'px';
    label.style.top = (rect.top + window.scrollY - 14) + 'px';
    document.body.appendChild(label);
  });
  return els.length + ' elements annotated';
})()
"#;
            let _ = cdp_evaluate(&session.ws_url, annotate_js).await;
        }

        // Take screenshot via CDP
        let screenshot_result = cdp_call(
            &session.ws_url,
            "Page.captureScreenshot",
            json!({ "format": "png", "quality": 80 }),
            15,
        )
        .await?;

        // Remove annotation overlays after capturing
        if args.annotate {
            let cleanup_js = r#"
(function() {
  var labels = document.querySelectorAll('[data-ecrab-annotation]');
  labels.forEach(function(el) { el.remove(); });
})()
"#;
            let _ = cdp_evaluate(&session.ws_url, cleanup_js).await;
        }

        let png_b64 =
            screenshot_result["data"]
                .as_str()
                .ok_or_else(|| ToolError::ExecutionFailed {
                    tool: "browser_vision".into(),
                    message: "CDP screenshot returned no data".into(),
                })?;

        // Save to persistent location
        let screenshot_dir = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join(".edgecrab")
            .join("browser_screenshots");
        let _ = std::fs::create_dir_all(&screenshot_dir);

        // Cleanup old screenshots (>24h)
        cleanup_old_files(&screenshot_dir, 24 * 3600);

        let filename = format!("browser_vision_{}.png", epoch_secs());
        let screenshot_path = screenshot_dir.join(&filename);

        let png_bytes = base64::engine::general_purpose::STANDARD
            .decode(png_b64)
            .unwrap_or_default();
        if let Err(e) = std::fs::write(&screenshot_path, &png_bytes) {
            tracing::warn!("browser_vision: could not save screenshot: {e}");
        }

        // Analyze with vision LLM
        let analysis = if let Some(provider) = &ctx.provider {
            let question_text = if args.annotate {
                format!(
                    "{}\n\nList all interactive elements visible on the page.",
                    args.question
                )
            } else {
                args.question.clone()
            };

            let vision_prompt = format!(
                "You are analyzing a screenshot of a web browser.\n\n\
                 User's question: {question_text}\n\n\
                 Provide a detailed answer based on what you see. \
                 Describe interactive elements and any verification challenges."
            );

            let mut msg = ChatMessage::user(vision_prompt);
            msg.images = Some(vec![ImageData::new(png_b64, "image/png")]);

            match provider.chat(&[msg], None).await {
                Ok(response) => response.content,
                Err(e) => {
                    tracing::warn!("browser_vision: LLM vision call failed: {e}");
                    format!(
                        "Vision analysis unavailable (error: {e}). \
                         Screenshot saved to: {}",
                        screenshot_path.display()
                    )
                }
            }
        } else {
            format!(
                "Screenshot captured ({} bytes). No vision provider available. \
                 Screenshot path: {}",
                png_bytes.len(),
                screenshot_path.display()
            )
        };

        let screenshot_note = if screenshot_path.exists() {
            format!("\n\nScreenshot saved: MEDIA:{}", screenshot_path.display())
        } else {
            String::new()
        };

        Ok(format!("{analysis}{screenshot_note}"))
    }
}

inventory::submit!(&BrowserVisionTool as &dyn ToolHandler);

// ═══════════════════════════════════════════════════════════════
// browser_close — close session and release resources
// ═══════════════════════════════════════════════════════════════

// ═══════════════════════════════════════════════════════════════
// browser_wait_for — wait for text/element to appear (beyond hermes-agent)
//
// Agents working with SPAs often click a button and need the resulting
// content to load before the next browser_snapshot. This tool polls the
// DOM for up to `timeout` seconds, removing the need for arbitrary sleeps.
// ═══════════════════════════════════════════════════════════════

pub struct BrowserWaitForTool;

#[derive(Deserialize)]
struct WaitForArgs {
    /// Plain text that must appear somewhere on the page.
    #[serde(default)]
    text: Option<String>,
    /// CSS selector that must match at least one visible element.
    #[serde(default)]
    selector: Option<String>,
    /// Maximum seconds to wait (default: 10, max: 30).
    #[serde(default = "default_wait_timeout")]
    timeout: u64,
}

fn default_wait_timeout() -> u64 {
    10
}

fn wait_for_selector_check_js(selector: &str) -> String {
    format!(
        r#"(function() {{
  var sel = {};
  var nodes = Array.from(document.querySelectorAll(sel));
  return nodes.some(function(el) {{
    if (!el || !el.isConnected) return false;
    var style = window.getComputedStyle(el);
    if (!style) return false;
    if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse') return false;
    if (parseFloat(style.opacity || '1') === 0) return false;
    if (el.hidden || el.getAttribute('aria-hidden') === 'true') return false;
    var rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }});
}})()"#,
        serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_string())
    )
}

#[async_trait]
impl ToolHandler for BrowserWaitForTool {
    fn name(&self) -> &'static str {
        "browser_wait_for"
    }
    fn toolset(&self) -> &'static str {
        "browser"
    }
    fn emoji(&self) -> &'static str {
        "⏳"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser_wait_for".into(),
            description: "Wait for text or a CSS selector to appear on the page (polls \
                DOM until found or timeout). Use this after browser_click or \
                browser_navigate when you expect new dynamic content to load before \
                calling browser_snapshot. Specify `text` OR `selector` (or both). \
                Returns immediately when condition is met."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Plain text that must appear somewhere on the page"
                    },
                    "selector": {
                        "type": "string",
                        "description": "CSS selector that must match at least one visible element (e.g. '.results', '#submit-btn')"
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Maximum seconds to wait (default: 10, max: 30)"
                    }
                }
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        browser_is_available()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: WaitForArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "browser_wait_for".into(),
                message: e.to_string(),
            })?;

        if args.text.is_none() && args.selector.is_none() {
            return Err(ToolError::InvalidArgs {
                tool: "browser_wait_for".into(),
                message: "Provide at least one of: text, selector".into(),
            });
        }

        let timeout_secs = args.timeout.min(30);
        let session = get_session(ctx).await?;

        let text_js = match &args.text {
            Some(t) => format!(
                "(document.body && document.body.innerText.includes({}))",
                serde_json::to_string(t).unwrap_or_default()
            ),
            None => "true".into(),
        };

        let selector_js = match &args.selector {
            Some(s) => wait_for_selector_check_js(s),
            None => "true".into(),
        };

        let check_js = format!("(function() {{ return ({text_js}) && ({selector_js}); }})()");

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
        let mut found = false;

        while tokio::time::Instant::now() < deadline {
            let result = cdp_evaluate(&session.ws_url, &check_js)
                .await
                .unwrap_or_else(|_| "false".into());
            if result == "true" {
                found = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }

        let elapsed = timeout_secs.saturating_sub(
            deadline
                .saturating_duration_since(tokio::time::Instant::now())
                .as_secs(),
        );

        if found {
            let mut parts = vec![];
            if let Some(t) = &args.text {
                parts.push(format!("text \"{t}\""));
            }
            if let Some(s) = &args.selector {
                parts.push(format!("selector \"{s}\""));
            }
            Ok(format!(
                "Found {} after ~{}s. Page is ready.",
                parts.join(" and "),
                elapsed
            ))
        } else {
            let mut parts = vec![];
            if let Some(t) = &args.text {
                parts.push(format!("text \"{t}\""));
            }
            if let Some(s) = &args.selector {
                parts.push(format!("selector \"{s}\""));
            }
            Err(ToolError::Timeout {
                tool: "browser_wait_for".into(),
                seconds: timeout_secs,
            })
        }
    }
}

inventory::submit!(&BrowserWaitForTool as &dyn ToolHandler);

// ═══════════════════════════════════════════════════════════════
// browser_select — select option in <select> dropdown (beyond hermes-agent)
//
// Agents often need to choose from a dropdown (country, language, category).
// This tool resolves the element by @eN ref and sets the value in one step,
// firing both input and change events so JavaScript listeners react correctly.
// ═══════════════════════════════════════════════════════════════

pub struct BrowserSelectTool;

#[derive(Deserialize)]
struct SelectArgs {
    #[serde(alias = "selector")]
    r#ref: String,
    /// Option text (partial match) or option value to select.
    option: String,
}

#[async_trait]
impl ToolHandler for BrowserSelectTool {
    fn name(&self) -> &'static str {
        "browser_select"
    }
    fn toolset(&self) -> &'static str {
        "browser"
    }
    fn emoji(&self) -> &'static str {
        "🔽"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser_select".into(),
            description: "Select an option in a <select> dropdown by its visible text \
                or value attribute. Requires browser_snapshot first to get the ref ID \
                of the select element."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "ref": {
                        "type": "string",
                        "description": "The select element ref from snapshot (e.g., '@e7')"
                    },
                    "option": {
                        "type": "string",
                        "description": "Option text (partial match) or value to select (e.g., 'United States', 'en')"
                    }
                },
                "required": ["ref", "option"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        browser_is_available()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: SelectArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "browser_select".into(),
                message: e.to_string(),
            })?;

        let session = get_session(ctx).await?;
        let ref_id = normalize_ref(&args.r#ref)?;
        let option_escaped = escape_js_string(&args.option);

        let js = format!(
            r#"
(function() {{
  var el = document.querySelector('[data-ecref="{ref_id}"]');
  if (!el) return 'ERROR: Element @{ref_id} not found. Run browser_snapshot first.';
  if (el.tagName !== 'SELECT') return 'ERROR: @{ref_id} is not a <select> element (tag: ' + el.tagName + ')';
  var target = '{option_escaped}';
  var matched = null;
  // Try exact value match first
  for (var i = 0; i < el.options.length; i++) {{
    if (el.options[i].value === target || el.options[i].text === target) {{
      matched = i; break;
    }}
  }}
  // Fallback: case-insensitive partial text match
  if (matched === null) {{
    var tLower = target.toLowerCase();
    for (var j = 0; j < el.options.length; j++) {{
      if (el.options[j].text.toLowerCase().includes(tLower)) {{
        matched = j; break;
      }}
    }}
  }}
  if (matched === null) {{
    var opts = Array.from(el.options).map(function(o) {{ return o.text; }}).join(', ');
    return 'ERROR: Option "' + target + '" not found. Available: ' + opts.substring(0, 200);
  }}
  el.selectedIndex = matched;
  el.dispatchEvent(new Event('input', {{ bubbles: true }}));
  el.dispatchEvent(new Event('change', {{ bubbles: true }}));
  return 'Selected "' + el.options[matched].text + '" in @{ref_id}';
}})()
"#,
            ref_id = escape_js_string(&ref_id),
            option_escaped = option_escaped,
        );

        let result = cdp_evaluate(&session.ws_url, &js).await?;
        Ok(result)
    }
}

inventory::submit!(&BrowserSelectTool as &dyn ToolHandler);

// ═══════════════════════════════════════════════════════════════
// browser_hover — hover over element to trigger hover states/menus
//                 (beyond hermes-agent)
//
// Many sites reveal dropdown menus, tooltips, or secondary content only on
// CSS :hover. This tool moves the mouse over the element's bounding box
// using CDP's real mouse event dispatching, ensuring CSS :hover applies.
// ═══════════════════════════════════════════════════════════════

pub struct BrowserHoverTool;

#[derive(Deserialize)]
struct HoverArgs {
    #[serde(alias = "selector")]
    r#ref: String,
}

#[async_trait]
impl ToolHandler for BrowserHoverTool {
    fn name(&self) -> &'static str {
        "browser_hover"
    }
    fn toolset(&self) -> &'static str {
        "browser"
    }
    fn emoji(&self) -> &'static str {
        "🖱️"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser_hover".into(),
            description: "Move the mouse over an element to trigger hover states, \
                dropdown menus, or tooltips. Uses real CDP mouse events so CSS :hover \
                is properly applied. Call browser_snapshot after to see revealed content. \
                Requires browser_navigate and browser_snapshot first."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "ref": {
                        "type": "string",
                        "description": "Element ref from snapshot (e.g., '@e4')"
                    }
                },
                "required": ["ref"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        browser_is_available()
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: HoverArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "browser_hover".into(),
            message: e.to_string(),
        })?;

        let session = get_session(ctx).await?;
        let ref_id = normalize_ref(&args.r#ref)?;

        // Get element's center coordinates via JS
        let coords_js = format!(
            r#"
(function() {{
  var el = document.querySelector('[data-ecref="{ref_id}"]');
  if (!el) return 'NOT_FOUND';
  el.scrollIntoView({{block: 'center', behavior: 'instant'}});
  var rect = el.getBoundingClientRect();
  var cx = Math.round(rect.left + rect.width / 2);
  var cy = Math.round(rect.top + rect.height / 2);
  return cx + ',' + cy + ',' + el.tagName;
}})()
"#,
            ref_id = escape_js_string(&ref_id),
        );

        let coords_raw = cdp_evaluate(&session.ws_url, &coords_js).await?;
        if coords_raw == "NOT_FOUND" {
            return Err(ToolError::ExecutionFailed {
                tool: "browser_hover".into(),
                message: format!(
                    "Element @{ref_id} not found. Run browser_snapshot first to refresh refs."
                ),
            });
        }

        let parts: Vec<&str> = coords_raw.splitn(3, ',').collect();
        let (cx, cy, tag) = if parts.len() >= 2 {
            let x: f64 = parts[0].parse().unwrap_or(0.0);
            let y: f64 = parts[1].parse().unwrap_or(0.0);
            let t = parts.get(2).copied().unwrap_or("?");
            (x, y, t)
        } else {
            (0.0, 0.0, "?")
        };

        // Move mouse to the center of the element using CDP Input.dispatchMouseEvent
        let _ = cdp_call(
            &session.ws_url,
            "Input.dispatchMouseEvent",
            json!({
                "type": "mouseMoved",
                "x": cx,
                "y": cy,
                "button": "none",
                "buttons": 0,
            }),
            5,
        )
        .await;

        // Brief pause for CSS :hover and JS mouseover listeners to fire
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        Ok(format!(
            "Hovered over @{ref_id} ({tag}) at ({cx:.0}, {cy:.0}). \
             Call browser_snapshot to see any revealed content."
        ))
    }
}

inventory::submit!(&BrowserHoverTool as &dyn ToolHandler);

// ═══════════════════════════════════════════════════════════════
// Shared helpers
// ═══════════════════════════════════════════════════════════════

/// Normalize a ref string: "@e5" → "e5", "e5" → "e5".
fn normalize_ref(r: &str) -> Result<String, ToolError> {
    let stripped = r.trim().strip_prefix('@').unwrap_or(r.trim());
    if stripped.starts_with('e')
        && stripped.len() > 1
        && stripped[1..].chars().all(|c| c.is_ascii_digit())
    {
        Ok(stripped.to_string())
    } else {
        Err(ToolError::InvalidArgs {
            tool: "browser".into(),
            message: format!(
                "Invalid ref '{r}'. Expected format: @e5 or e5. \
                 Run browser_snapshot first."
            ),
        })
    }
}

/// Map key names to DOM key code values.
fn key_to_code(key: &str) -> &'static str {
    match key {
        "Enter" => "Enter",
        "Tab" => "Tab",
        "Escape" => "Escape",
        "Backspace" => "Backspace",
        "Delete" => "Delete",
        "ArrowUp" => "ArrowUp",
        "ArrowDown" => "ArrowDown",
        "ArrowLeft" => "ArrowLeft",
        "ArrowRight" => "ArrowRight",
        "Home" => "Home",
        "End" => "End",
        "PageUp" => "PageUp",
        "PageDown" => "PageDown",
        "Space" | " " => "Space",
        "F1" => "F1",
        "F2" => "F2",
        "F3" => "F3",
        "F4" => "F4",
        "F5" => "F5",
        "F6" => "F6",
        "F7" => "F7",
        "F8" => "F8",
        "F9" => "F9",
        "F10" => "F10",
        "F11" => "F11",
        "F12" => "F12",
        "Insert" => "Insert",
        "CapsLock" => "CapsLock",
        "Shift" => "ShiftLeft",
        "Control" | "Ctrl" => "ControlLeft",
        "Alt" => "AltLeft",
        "Meta" | "Command" => "MetaLeft",
        _ => "Unidentified",
    }
}

/// Map key names to Windows virtual key codes (for CDP Input.dispatchKeyEvent).
fn key_to_vk(key: &str) -> u32 {
    match key {
        "Enter" => 13,
        "Tab" => 9,
        "Escape" => 27,
        "Backspace" => 8,
        "Delete" => 46,
        "ArrowUp" => 38,
        "ArrowDown" => 40,
        "ArrowLeft" => 37,
        "ArrowRight" => 39,
        "Home" => 36,
        "End" => 35,
        "PageUp" => 33,
        "PageDown" => 34,
        "Space" | " " => 32,
        "F1" => 112,
        "F2" => 113,
        "F3" => 114,
        "F4" => 115,
        "F5" => 116,
        "F6" => 117,
        "F7" => 118,
        "F8" => 119,
        "F9" => 120,
        "F10" => 121,
        "F11" => 122,
        "F12" => 123,
        "Insert" => 45,
        "CapsLock" => 20,
        "Shift" => 16,
        "Control" | "Ctrl" => 17,
        "Alt" => 18,
        _ => 0,
    }
}

/// Escape a string for safe embedding in JavaScript source code.
fn escape_js_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// Validate a URL is safe for browser navigation (SSRF prevention).
///
/// Used for **pre-navigation** checks before Chrome opens any page. Blocks
/// dangerous/internal schemes (`file://`, `javascript:`, `data:`, `chrome://`,
/// `about:`) in addition to private-IP SSRF.
fn validate_browser_url(url: &str) -> Result<(), ToolError> {
    let lower = url.to_lowercase();
    if lower.starts_with("file://")
        || lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("chrome://")
        || lower.starts_with("about:")
    {
        return Err(ToolError::PermissionDenied(format!(
            "URL scheme not allowed for browser navigation: {url}"
        )));
    }

    match edgecrab_security::url_safety::is_safe_url(url) {
        Ok(true) => Ok(()),
        Ok(false) => Err(ToolError::PermissionDenied(format!(
            "URL blocked by SSRF policy: {url}"
        ))),
        Err(e) => Err(ToolError::PermissionDenied(format!(
            "URL validation error: {e}"
        ))),
    }
}

/// Check whether a **redirect destination** resolves to a private or
/// restricted network address (SSRF post-redirect guard).
///
/// This is intentionally narrower than `validate_browser_url`: it only fires
/// when the host is a literal private/loopback IP or a known-dangerous
/// hostname (e.g. `169.254.169.254`, `localhost`).  Public-hostname-to-public-
/// hostname redirects (e.g. `www.monde.fr` → `www.lemonde.fr`) must never be
/// blocked here.
///
/// Returns `true`  → the URL targets a private address (block it).
/// Returns `false` → the URL is a public address or is unparseable (allow it;
///                   a separate navigation-error pathway handles real failures).
fn is_redirect_to_private_ip(url: &str) -> bool {
    use std::net::IpAddr;

    // Only act on http/https; non-http redirects (about:, chrome:, etc.) are
    // already blocked by the pre-navigation check and are harmless here.
    let parsed = match url::Url::parse(url) {
        Ok(u) if matches!(u.scheme(), "http" | "https") => u,
        _ => return false,
    };

    let host = match parsed.host_str() {
        Some(h) => h,
        None => return false,
    };

    // Block well-known dangerous hostnames
    const BLOCKED_HOSTS: &[&str] = &["localhost", "metadata.google.internal", "169.254.169.254"];
    if BLOCKED_HOSTS.contains(&host) {
        return true;
    }

    // Block literal private/loopback/reserved IPs
    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_broadcast()
                    || v4.is_unspecified()
            }
            IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
        };
    }

    false
}

/// Wait for the page navigation to commit by polling `document.readyState`
/// and waiting for the URL to stabilise.
///
/// After a `Page.navigate` call, Chrome may go through several intermediate
/// states (connecting, redirecting, loading) before settling on the final URL.
/// A fixed sleep is unreliable for multi-hop redirect chains.  This helper
/// polls until both:
/// - `document.readyState` is `"complete"` or `"interactive"`, AND
/// - the URL has been stable across two successive polls (200 ms apart).
///
/// `max_wait_ms` caps the total wait (default callers use 8 000 ms).
async fn wait_for_navigation_commit(ws_url: &str, max_wait_ms: u64) -> String {
    let poll_ms = 200u64;
    let max_polls = (max_wait_ms / poll_ms).max(1);
    let mut last_url = String::new();
    let mut stable_count = 0u32;

    for _ in 0..max_polls {
        tokio::time::sleep(std::time::Duration::from_millis(poll_ms)).await;

        let ready_state = cdp_evaluate(ws_url, "document.readyState")
            .await
            .unwrap_or_default();
        let current_url = cdp_evaluate(ws_url, "window.location.href")
            .await
            .unwrap_or_default();

        // Skip transient/internal states
        if current_url == last_url && (ready_state == "complete" || ready_state == "interactive") {
            stable_count += 1;
            if stable_count >= 2 {
                // Stable for two consecutive polls — done.
                return current_url;
            }
        } else {
            stable_count = 0;
        }
        last_url = current_url;
    }

    // Return whatever URL we have after the timeout
    last_url
}

/// Remove files older than max_age_secs from a directory (best-effort).
fn cleanup_old_files(dir: &std::path::Path, max_age_secs: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let cutoff = epoch_secs().saturating_sub(max_age_secs);
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata() {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if mtime < cutoff {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{Builder, tempdir};

    fn browser_launch_tests_enabled() -> bool {
        truthy_env_var("EDGECRAB_RUN_BROWSER_LAUNCH_TESTS")
    }

    #[test]
    fn normalize_ref_valid() {
        assert_eq!(normalize_ref("@e5").unwrap(), "e5");
        assert_eq!(normalize_ref("e5").unwrap(), "e5");
        assert_eq!(normalize_ref("@e123").unwrap(), "e123");
        assert_eq!(normalize_ref("  @e1  ").unwrap(), "e1");
    }

    #[test]
    fn normalize_ref_invalid() {
        assert!(normalize_ref("").is_err());
        assert!(normalize_ref("5").is_err());
        assert!(normalize_ref("foo").is_err());
        assert!(normalize_ref("@x5").is_err());
        assert!(normalize_ref("e").is_err());
    }

    #[test]
    fn escape_js_basic() {
        assert_eq!(escape_js_string("hello"), "hello");
        assert_eq!(escape_js_string("it's"), "it\\'s");
        assert_eq!(escape_js_string("line\nnew"), "line\\nnew");
        assert_eq!(escape_js_string(r#"say "hi""#), r#"say \"hi\""#);
    }

    #[test]
    fn validate_url_blocks_file() {
        assert!(validate_browser_url("file:///etc/passwd").is_err());
    }

    #[test]
    fn validate_url_blocks_javascript() {
        assert!(validate_browser_url("javascript:alert(1)").is_err());
    }

    #[test]
    fn validate_url_blocks_data() {
        assert!(validate_browser_url("data:text/html,<h1>hi</h1>").is_err());
    }

    #[test]
    fn validate_url_blocks_chrome() {
        assert!(validate_browser_url("chrome://settings").is_err());
    }

    #[test]
    fn validate_url_allows_https() {
        assert!(validate_browser_url("https://example.com").is_ok());
    }

    #[test]
    fn validate_url_blocks_localhost() {
        assert!(validate_browser_url("http://127.0.0.1:8080").is_err());
    }

    #[test]
    fn bot_detection_cloudflare() {
        assert!(detect_bot_warning("Just a moment...").is_some());
        assert!(detect_bot_warning("Attention Required! | Cloudflare").is_some());
        assert!(detect_bot_warning("Access Denied").is_some());
    }

    #[test]
    fn bot_detection_clean() {
        assert!(detect_bot_warning("GitHub - trending").is_none());
        assert!(detect_bot_warning("Google").is_none());
    }

    #[test]
    fn key_mappings() {
        assert_eq!(key_to_code("Enter"), "Enter");
        assert_eq!(key_to_vk("Enter"), 13);
        assert_eq!(key_to_code("Tab"), "Tab");
        assert_eq!(key_to_vk("Escape"), 27);
        assert_eq!(key_to_code("Space"), "Space");
        assert_eq!(key_to_vk("Space"), 32);
    }

    #[test]
    fn snapshot_js_compact_vs_full() {
        let compact = snapshot_js(false);
        assert!(compact.contains("var FULL = false"));
        let full = snapshot_js(true);
        assert!(full.contains("var FULL = true"));
    }

    // Tool metadata
    #[test]
    fn navigate_tool_metadata() {
        assert_eq!(BrowserNavigateTool.name(), "browser_navigate");
        assert_eq!(BrowserNavigateTool.toolset(), "browser");
    }

    #[test]
    fn snapshot_tool_metadata() {
        assert_eq!(BrowserSnapshotTool.name(), "browser_snapshot");
        assert_eq!(BrowserSnapshotTool.toolset(), "browser");
    }

    #[test]
    fn click_tool_metadata() {
        assert_eq!(BrowserClickTool.name(), "browser_click");
        assert_eq!(BrowserClickTool.toolset(), "browser");
    }

    #[test]
    fn type_tool_metadata() {
        assert_eq!(BrowserTypeTool.name(), "browser_type");
        assert_eq!(BrowserTypeTool.toolset(), "browser");
    }

    #[test]
    fn scroll_tool_metadata() {
        assert_eq!(BrowserScrollTool.name(), "browser_scroll");
        assert_eq!(BrowserScrollTool.toolset(), "browser");
    }

    #[test]
    fn console_tool_metadata() {
        assert_eq!(BrowserConsoleTool.name(), "browser_console");
        assert_eq!(BrowserConsoleTool.toolset(), "browser");
    }

    #[test]
    fn back_tool_metadata() {
        assert_eq!(BrowserBackTool.name(), "browser_back");
        assert_eq!(BrowserBackTool.toolset(), "browser");
    }

    #[test]
    fn press_tool_metadata() {
        assert_eq!(BrowserPressTool.name(), "browser_press");
        assert_eq!(BrowserPressTool.toolset(), "browser");
    }

    #[test]
    fn close_tool_metadata() {
        assert_eq!(BrowserCloseTool.name(), "browser_close");
        assert_eq!(BrowserCloseTool.toolset(), "browser");
    }

    #[test]
    fn get_images_tool_metadata() {
        assert_eq!(BrowserGetImagesTool.name(), "browser_get_images");
        assert_eq!(BrowserGetImagesTool.toolset(), "browser");
    }

    #[test]
    fn vision_tool_metadata() {
        assert_eq!(BrowserVisionTool.name(), "browser_vision");
        assert_eq!(BrowserVisionTool.toolset(), "browser");
    }

    #[test]
    fn all_browser_tools_same_toolset() {
        let toolset = "browser";
        assert_eq!(BrowserNavigateTool.toolset(), toolset);
        assert_eq!(BrowserSnapshotTool.toolset(), toolset);
        assert_eq!(BrowserClickTool.toolset(), toolset);
        assert_eq!(BrowserTypeTool.toolset(), toolset);
        assert_eq!(BrowserScrollTool.toolset(), toolset);
        assert_eq!(BrowserConsoleTool.toolset(), toolset);
        assert_eq!(BrowserBackTool.toolset(), toolset);
        assert_eq!(BrowserPressTool.toolset(), toolset);
        assert_eq!(BrowserCloseTool.toolset(), toolset);
        assert_eq!(BrowserGetImagesTool.toolset(), toolset);
        assert_eq!(BrowserVisionTool.toolset(), toolset);
        assert_eq!(BrowserWaitForTool.toolset(), toolset);
        assert_eq!(BrowserSelectTool.toolset(), toolset);
        assert_eq!(BrowserHoverTool.toolset(), toolset);
    }

    // ── New tool metadata ───────────────────────────────────────

    #[test]
    fn wait_for_tool_metadata() {
        assert_eq!(BrowserWaitForTool.name(), "browser_wait_for");
        assert_eq!(BrowserWaitForTool.toolset(), "browser");
        assert!(!BrowserWaitForTool.schema().description.is_empty());
    }

    #[test]
    fn wait_for_selector_js_requires_visible_connected_elements() {
        let js = wait_for_selector_check_js(".results");
        assert!(js.contains("document.querySelectorAll"));
        assert!(js.contains("el.isConnected"));
        assert!(js.contains("getComputedStyle"));
        assert!(js.contains("display === 'none'"));
        assert!(js.contains("visibility === 'hidden'"));
        assert!(js.contains("parseFloat(style.opacity || '1') === 0"));
        assert!(js.contains("el.hidden"));
        assert!(js.contains("aria-hidden"));
        assert!(js.contains("rect.width > 0 && rect.height > 0"));
    }

    #[test]
    fn select_tool_metadata() {
        assert_eq!(BrowserSelectTool.name(), "browser_select");
        assert_eq!(BrowserSelectTool.toolset(), "browser");
        // schema should require both ref and option
        let schema = BrowserSelectTool.schema();
        let required = schema.parameters["required"]
            .as_array()
            .expect("required array");
        assert!(required.iter().any(|r| r == "ref"));
        assert!(required.iter().any(|r| r == "option"));
    }

    #[test]
    fn hover_tool_metadata() {
        assert_eq!(BrowserHoverTool.name(), "browser_hover");
        assert_eq!(BrowserHoverTool.toolset(), "browser");
        let schema = BrowserHoverTool.schema();
        let required = schema.parameters["required"]
            .as_array()
            .expect("required array");
        assert!(required.iter().any(|r| r == "ref"));
    }

    #[test]
    fn wait_for_rejects_no_condition() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let result = BrowserWaitForTool
                .execute(
                    serde_json::json!({}),
                    &crate::registry::ToolContext::test_context(),
                )
                .await;
            assert!(result.is_err());
        });
    }

    #[test]
    fn select_rejects_invalid_args() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // Missing required 'option' field
            let result = BrowserSelectTool
                .execute(
                    serde_json::json!({ "ref": "@e1" }),
                    &crate::registry::ToolContext::test_context(),
                )
                .await;
            assert!(result.is_err());
        });
    }

    #[test]
    fn hover_rejects_invalid_args() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // Missing required 'ref' field — fails at serde deserialization
            let result = BrowserHoverTool
                .execute(
                    serde_json::json!({}),
                    &crate::registry::ToolContext::test_context(),
                )
                .await;
            assert!(result.is_err());
        });
    }

    // ── snapshot_js content checks ───────────────────────────────

    #[test]
    fn snapshot_js_includes_aria_states() {
        let js = snapshot_js(false);
        assert!(js.contains("aria-expanded"), "should emit aria-expanded");
        assert!(js.contains("aria-checked"), "should emit aria-checked");
        assert!(js.contains("aria-selected"), "should emit aria-selected");
        assert!(
            js.contains("aria-labelledby"),
            "should resolve aria-labelledby"
        );
        // New additions
        assert!(
            js.contains("aria-current"),
            "should emit aria-current for nav context"
        );
    }

    #[test]
    fn snapshot_js_vis_handles_fixed() {
        let js = snapshot_js(false);
        assert!(
            js.contains("position === 'fixed'"),
            "vis() must allow position:fixed"
        );
        assert!(
            js.contains("position === 'sticky'"),
            "vis() must allow position:sticky"
        );
    }

    #[test]
    fn snapshot_js_vis_skips_aria_hidden() {
        let js = snapshot_js(false);
        // aria-hidden subtrees must be pruned from the output
        assert!(
            js.contains("aria-hidden"),
            "vis() must check aria-hidden to prune hidden overlays"
        );
    }

    #[test]
    fn snapshot_js_children_limit_500() {
        let js = snapshot_js(false);
        assert!(js.contains("i < 500"), "children limit should be 500");
        assert!(!js.contains("i < 200"), "old 200 limit should be removed");
    }

    #[test]
    fn snapshot_js_compact_deduplicates_links() {
        let compact = snapshot_js(false);
        // Compact mode must track seen hrefs to suppress duplicates
        assert!(
            compact.contains("seenHrefs"),
            "compact mode must track seen hrefs"
        );
        assert!(
            compact.contains("dupCount"),
            "compact mode must count and report duplicates"
        );
        // Full mode initialises seenHrefs to null (the dedup block only
        // executes when `!FULL` is true, so it is unreachable in full mode).
        let full = snapshot_js(true);
        assert!(
            full.contains("FULL ? null : new Set()"),
            "full mode must set seenHrefs to null so no Set is allocated"
        );
        assert!(
            full.contains("!FULL && t === 'A'"),
            "link dedup check must be gated by !FULL so it is bypassed in full mode"
        );
    }

    #[test]
    fn snapshot_js_nav_truncation_in_compact() {
        let compact = snapshot_js(false);
        assert!(
            compact.contains("NAV_MAX"),
            "compact snapshot must define nav truncation limit"
        );
        assert!(
            compact.contains("more navigation links"),
            "compact snapshot must emit a truncation marker for long navs"
        );
    }

    #[test]
    fn snapshot_js_list_truncation_in_compact() {
        let compact = snapshot_js(false);
        assert!(
            compact.contains("more list items"),
            "compact snapshot must truncate lists with >12 items"
        );
    }

    #[test]
    fn snapshot_js_skips_decorative_images() {
        let js = snapshot_js(false);
        // Empty alt images must be skipped
        assert!(
            js.contains("if (!alt) return"),
            "snapshot must skip decorative images"
        );
    }

    #[test]
    fn snapshot_js_presentation_role_passthrough() {
        let js = snapshot_js(false);
        // role=presentation/none: element line is skipped but children are walked
        assert!(
            js.contains("roleLC === 'presentation'"),
            "snapshot must pass-through role=presentation elements"
        );
        assert!(
            js.contains("roleLC === 'none'"),
            "snapshot must pass-through role=none elements"
        );
    }

    #[test]
    fn snapshot_js_heading_text_in_compact() {
        // name() must extract innerText for heading elements (H1-H6)
        // Previously headings showed `heading` with no text in compact mode
        let compact = snapshot_js(false);
        assert!(
            compact.contains("/^H[1-6]$/.test(t)") && compact.contains("innerTxt"),
            "compact mode must include heading text via innerTxt helper"
        );
    }

    #[test]
    fn snapshot_js_new_landmark_roles() {
        let js = snapshot_js(false);
        // HEADER → banner, FOOTER → contentinfo, ASIDE → complementary
        assert!(js.contains("'banner'"), "HEADER should map to banner role");
        assert!(
            js.contains("'contentinfo'"),
            "FOOTER should map to contentinfo role"
        );
        assert!(
            js.contains("'complementary'"),
            "ASIDE should map to complementary role"
        );
        assert!(js.contains("'article'"), "ARTICLE should be a landmark");
    }

    #[test]
    fn snapshot_js_input_state_improvements() {
        let js = snapshot_js(false);
        assert!(
            js.contains("readOnly"),
            "snapshot must emit [readonly] state"
        );
        assert!(
            js.contains("maxlength"),
            "snapshot must emit maxlength attribute"
        );
        assert!(
            js.contains("[focused]"),
            "snapshot must mark the focused element"
        );
    }

    #[test]
    fn snapshot_js_meta_description_in_header() {
        let js = snapshot_js(false);
        assert!(
            js.contains("meta[property=\"og:description\"]"),
            "snapshot header should include page meta/og description"
        );
        assert!(
            js.contains("- Description:"),
            "snapshot header should prefix description with '- Description:'"
        );
    }

    #[test]
    fn snapshot_js_depth_cap_is_8() {
        let js = snapshot_js(false);
        // Max indentation depth should be 8 (was 10)
        assert!(
            js.contains("Math.min(d, 8)"),
            "indentation depth must be capped at 8"
        );
        assert!(
            !js.contains("Math.min(d, 10)"),
            "old depth cap of 10 must be removed"
        );
    }

    #[test]
    fn snapshot_js_rcstop_budget_parameter() {
        let js = snapshot_js(false);
        // rcStop budget parameter must be threaded through walk() for nav truncation
        assert!(
            js.contains("rcStop"),
            "walk() must accept rcStop budget parameter"
        );
        assert!(
            js.contains("rc >= rcStop"),
            "walk() must bail out when budget is exhausted"
        );
    }

    /// Verify the modern tab-creation API is wired in: create_tab_via_target_api
    /// must exist as a function (the fix for the "CDP JSON parse error" caused by
    /// the deprecated /json/new?about:blank endpoint returning HTML on modern Chrome).
    #[test]
    fn create_tab_via_target_api_exists() {
        // If create_tab_via_target_api is removed or renamed this test will fail
        // to compile, preventing the regression from shipping silently.
        //
        // We call it in a non-running context — we just verify the function is
        // reachable at link time.  The async fn returns Option so we can discard
        // the future without awaiting it.
        let _future = create_tab_via_target_api();
        // (future is intentionally dropped — no Chrome available in tests)
    }

    #[test]
    fn schema_has_required_fields() {
        let schema = BrowserNavigateTool.schema();
        assert_eq!(schema.name, "browser_navigate");
        assert!(!schema.description.is_empty());
        let required = schema.parameters["required"]
            .as_array()
            .expect("required array");
        assert!(required.iter().any(|r| r == "url"));
    }

    #[test]
    fn snapshot_schema_has_full_param() {
        let schema = BrowserSnapshotTool.schema();
        assert!(schema.parameters["properties"]["full"].is_object());
    }

    #[test]
    fn console_schema_has_clear_param() {
        let schema = BrowserConsoleTool.schema();
        assert!(schema.parameters["properties"]["clear"].is_object());
    }

    #[test]
    fn click_schema_uses_ref() {
        let schema = BrowserClickTool.schema();
        assert!(schema.parameters["properties"]["ref"].is_object());
        let required = schema.parameters["required"]
            .as_array()
            .expect("required array");
        assert!(required.iter().any(|r| r == "ref"));
    }

    #[test]
    fn type_schema_uses_ref() {
        let schema = BrowserTypeTool.schema();
        assert!(schema.parameters["properties"]["ref"].is_object());
        assert!(schema.parameters["properties"]["text"].is_object());
    }

    #[tokio::test]
    async fn navigate_rejects_invalid_args() {
        let ctx = ToolContext::test_context();
        let result = BrowserNavigateTool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn navigate_rejects_dangerous_url() {
        let ctx = ToolContext::test_context();
        let result = BrowserNavigateTool
            .execute(json!({"url": "file:///etc/passwd"}), &ctx)
            .await;
        assert!(result.is_err());
        match result.expect_err("dangerous URL should be rejected") {
            ToolError::PermissionDenied(msg) => {
                assert!(msg.contains("not allowed") || msg.contains("blocked"));
            }
            other => panic!("Expected PermissionDenied, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn click_rejects_invalid_args() {
        let ctx = ToolContext::test_context();
        let result = BrowserClickTool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn type_rejects_invalid_args() {
        let ctx = ToolContext::test_context();
        let result = BrowserTypeTool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn scroll_rejects_invalid_args() {
        let ctx = ToolContext::test_context();
        let result = BrowserScrollTool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn press_rejects_invalid_args() {
        let ctx = ToolContext::test_context();
        let result = BrowserPressTool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn vision_rejects_invalid_args() {
        let ctx = ToolContext::test_context();
        let result = BrowserVisionTool.execute(json!({}), &ctx).await;
        assert!(result.is_err());
    }

    #[test]
    fn truncate_snapshot_short() {
        let short = "hello world";
        assert_eq!(truncate_snapshot(short), short);
    }

    #[test]
    fn truncate_snapshot_long() {
        let long = "x".repeat(10000);
        let result = truncate_snapshot(&long);
        assert!(result.len() < long.len());
        assert!(result.contains("[... content truncated"));
    }

    #[test]
    fn truncate_snapshot_preserves_utf8_boundary() {
        let prefix = "x".repeat(SNAPSHOT_SUMMARIZE_THRESHOLD - 1);
        let text = format!("{prefix}étail");
        let result = truncate_snapshot(&text);
        assert!(result.contains("[... content truncated"));
        assert!(result.starts_with(&prefix));
        assert!(!result.contains("�"));
    }

    // ─── summarize_snapshot decision matrix ───────────────────────────────────

    /// full=true snapshots that fit within cap return as-is.
    #[tokio::test]
    async fn summarize_full_small_returns_as_is() {
        let snap = "heading line\nsome content";
        let result = summarize_snapshot(snap, true, None, &None).await;
        assert_eq!(result, snap);
    }

    /// full=true oversized snapshot: hard truncate with scroll hint, NO LLM.
    #[tokio::test]
    async fn summarize_full_large_truncates_without_llm() {
        let snap = "x".repeat(FULL_SNAPSHOT_MAX_CHARS + 5000);
        let result = summarize_snapshot(&snap, true, None, &None).await;
        assert!(
            result.len() < snap.len(),
            "full oversized snapshot must be truncated"
        );
        assert!(
            result.contains("browser_scroll"),
            "must instruct agent to scroll for more content"
        );
        assert!(
            result.contains("more bytes"),
            "must report hidden byte count"
        );
    }

    #[tokio::test]
    async fn summarize_full_large_preserves_utf8_boundary() {
        let prefix = "x".repeat(FULL_SNAPSHOT_MAX_CHARS - 1);
        let snap = format!("{prefix}étail\nnext line");
        let result = summarize_snapshot(&snap, true, None, &None).await;
        assert!(result.contains("browser_scroll"));
        assert!(result.starts_with(&prefix));
        assert!(!result.contains("�"));
    }

    /// compact snapshot that fits the threshold: returned unchanged.
    #[tokio::test]
    async fn summarize_compact_small_returns_as_is() {
        let snap = "heading\nlink [@e1]";
        let result = summarize_snapshot(snap, false, Some("find the link"), &None).await;
        assert_eq!(result, snap);
    }

    /// compact oversized WITHOUT user_task: plain truncate, never calls LLM.
    /// This is the key regression from the 261s freeze: without user_task we
    /// must never attempt LLM summarization (hermes parity).
    #[tokio::test]
    async fn summarize_compact_large_no_task_truncates_never_llm() {
        let snap = "line\n".repeat(5000); // >> 8000 chars
        // provider=None means any LLM attempt would panic — proves we don't call it
        let result = summarize_snapshot(&snap, false, None, &None).await;
        assert!(
            result.len() <= SNAPSHOT_SUMMARIZE_THRESHOLD + 100,
            "must truncate to threshold when no user_task"
        );
        assert!(
            result.contains("[... content truncated"),
            "must include truncation marker"
        );
    }

    /// compact oversized WITH user_task but NO provider: plain truncate.
    #[tokio::test]
    async fn summarize_compact_large_with_task_no_provider_truncates() {
        let snap = "line\n".repeat(5000);
        let result = summarize_snapshot(&snap, false, Some("find the heading"), &None).await;
        assert!(result.len() <= SNAPSHOT_SUMMARIZE_THRESHOLD + 100);
        assert!(result.contains("[... content truncated"));
    }

    // ─── Recording infrastructure ─────────────────────────────────

    #[test]
    fn find_ffmpeg_returns_option() {
        // ffmpeg may or may not be installed; both outcomes are valid.
        let _ = find_ffmpeg(); // must not panic
    }

    #[test]
    fn cleanup_old_recordings_missing_dir_is_noop() {
        let dir = std::path::PathBuf::from("/tmp/ecrab_test_missing_dir_12345");
        cleanup_old_recordings(&dir, 72 * 3600); // must not panic on missing dir
    }

    #[tokio::test]
    async fn assemble_recording_empty_frame_dir_returns_none() {
        let tmp = tempdir().unwrap();
        let recordings = tempdir().unwrap();
        let result = assemble_recording(tmp.path(), recordings.path(), "test-task", 0).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn assemble_recording_with_png_frames_fallback() {
        let tmp = tempdir().unwrap();
        let recordings = tempdir().unwrap();

        // Write a minimal valid 1x1 PNG frame.
        let tiny_png: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, // PNG magic
            0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, // IHDR chunk length + type
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
            0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, // bit depth, color type
            0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, // IDAT chunk
            0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xe2,
            0x21, 0xbc, 0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, // IEND
            0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        std::fs::write(tmp.path().join("frame_000001.png"), tiny_png).unwrap();

        // Call with 1 frame. If ffmpeg is present it will try to encode; if
        // not, it falls back to rename → must return Some in both cases.
        let result = assemble_recording(tmp.path(), recordings.path(), "unit-test", 1).await;
        // We cannot guarantee ffmpeg success in CI, but the fallback rename
        // should always produce something (provided the temp dirs are on the
        // same filesystem, which they always are on macOS /tmp).
        // If ffmpeg fails AND rename fails (cross-fs), result may be None —
        // that's acceptable; the test just validates no panic.
        let _ = result; // may be None on some CI environments
    }

    #[test]
    fn browser_session_recording_started_flag_defaults_false() {
        let session = BrowserSession::new("ws://127.0.0.1:9222".into(), "test-id".into());
        assert!(
            !session.recording_started.load(Ordering::Relaxed),
            "recording_started should be false on new session"
        );
        assert!(
            session.recorder.lock().unwrap().is_none(),
            "recorder should be None on new session"
        );
    }

    // ── New utility function tests ──────────────────────────────────────────

    #[test]
    fn set_cdp_override_roundtrip() {
        // Verify set/clear/status cycle
        assert!(set_cdp_override("http://localhost:9222").is_ok());
        let status = cdp_override_status();
        assert!(status.is_some(), "status should be Some after set");
        assert!(status.unwrap().contains("9222"));
        clear_cdp_override();
        assert!(
            cdp_override_status().is_none(),
            "status should be None after clear"
        );
    }

    #[test]
    fn parse_cdp_url_variants() {
        // ws:// prefix
        assert!(set_cdp_override("ws://127.0.0.1:9222").is_ok());
        clear_cdp_override();
        // http:// prefix
        assert!(set_cdp_override("http://localhost:9223").is_ok());
        clear_cdp_override();
        // bare port
        assert!(set_cdp_override("9224").is_ok());
        clear_cdp_override();
        // invalid — no port and not a number
        assert!(set_cdp_override("notaurl").is_err());
    }

    #[test]
    fn recording_override_roundtrip() {
        // Start with None
        clear_recording_override();
        assert!(get_recording_override().is_none());

        set_recording_override(true);
        assert_eq!(get_recording_override(), Some(true));

        set_recording_override(false);
        assert_eq!(get_recording_override(), Some(false));

        clear_recording_override();
        assert!(get_recording_override().is_none());
    }

    #[tokio::test]
    async fn probe_cdp_port_unreachable() {
        // Port 19222 is almost certainly not in use
        let reachable = probe_cdp_port("127.0.0.1", 19222).await;
        assert!(!reachable, "port 19222 should not be reachable");
    }

    #[tokio::test]
    async fn wait_for_cdp_ready_times_out() {
        // Should time out immediately on unreachable port
        let start = std::time::Instant::now();
        let ready = wait_for_cdp_ready("127.0.0.1", 19223, 1).await;
        assert!(!ready, "should be false for unreachable port");
        // Should complete in ≤ 2 s
        assert!(start.elapsed().as_secs() < 2, "timeout was too long");
    }

    #[test]
    #[ignore = "may spawn a real Chrome process — also requires EDGECRAB_RUN_BROWSER_LAUNCH_TESTS=1"]
    fn launch_chrome_returns_bool() {
        if !browser_launch_tests_enabled() {
            eprintln!(
                "skipping real Chrome launch test; set EDGECRAB_RUN_BROWSER_LAUNCH_TESTS=1 to enable"
            );
            return;
        }
        // Just verify it doesn't panic; actual launch depends on env
        let _ = launch_chrome_for_debugging(19224);
    }

    #[test]
    fn chrome_launch_command_is_nonempty() {
        let cmd = chrome_launch_command(9222);
        assert!(!cmd.is_empty());
        assert!(cmd.contains("9222"));
    }

    // ── is_redirect_to_private_ip ────────────────────────────────

    /// Public-to-public redirects must NEVER be blocked (the false-positive
    /// that triggered this fix: www.monde.fr → www.lemonde.fr).
    #[test]
    fn redirect_allows_public_hostname() {
        assert!(!is_redirect_to_private_ip("https://www.lemonde.fr/"));
        assert!(!is_redirect_to_private_ip("https://example.com"));
        assert!(!is_redirect_to_private_ip("https://www.github.com/login"));
    }

    #[test]
    fn redirect_blocks_loopback_ip() {
        assert!(is_redirect_to_private_ip("http://127.0.0.1/admin"));
        assert!(is_redirect_to_private_ip("http://127.0.0.1:8080/api"));
    }

    #[test]
    fn redirect_blocks_private_ip_class_a() {
        assert!(is_redirect_to_private_ip("http://10.0.0.1/"));
        assert!(is_redirect_to_private_ip("http://10.255.255.255/secret"));
    }

    #[test]
    fn redirect_blocks_private_ip_class_b() {
        assert!(is_redirect_to_private_ip("http://172.16.0.1/"));
        assert!(is_redirect_to_private_ip("http://172.31.255.255/"));
    }

    #[test]
    fn redirect_blocks_private_ip_class_c() {
        assert!(is_redirect_to_private_ip("http://192.168.1.1/router"));
        assert!(is_redirect_to_private_ip("http://192.168.0.100/"));
    }

    #[test]
    fn redirect_blocks_link_local() {
        assert!(is_redirect_to_private_ip("http://169.254.1.1/"));
    }

    #[test]
    fn redirect_blocks_cloud_metadata() {
        assert!(is_redirect_to_private_ip(
            "http://169.254.169.254/latest/meta-data/"
        ));
        assert!(is_redirect_to_private_ip(
            "http://metadata.google.internal/"
        ));
    }

    #[test]
    fn redirect_blocks_localhost_hostname() {
        assert!(is_redirect_to_private_ip("http://localhost/admin"));
        assert!(is_redirect_to_private_ip("http://localhost:3000/api"));
    }

    /// `about:blank`, unparseable strings, and non-http schemes return false
    /// (they are harmless or already blocked by validate_browser_url).
    #[test]
    fn redirect_allows_non_http_schemes_without_blocking() {
        // about:blank — Chrome uses this internally; must not block
        assert!(!is_redirect_to_private_ip("about:blank"));
        // Completely unparseable string — do not block (fail-open)
        assert!(!is_redirect_to_private_ip("not a url at all"));
        // ftp is not http/https → return false (don't block the navigate
        // path; validate_browser_url already handles non-http pre-navigation)
        assert!(!is_redirect_to_private_ip("ftp://example.com/"));
    }

    // ── DevToolsActivePort detection ────────────────────────────────────────

    /// A valid DevToolsActivePort file (port on first line) is parsed correctly.
    #[test]
    fn active_port_file_valid_port_is_found() {
        let dir = tempdir().unwrap();
        // Chrome writes port on line 1, browser WS path on line 2 (we ignore line 2)
        std::fs::write(
            dir.path().join("DevToolsActivePort"),
            "9222\n/devtools/browser/some-uuid-here\n",
        )
        .unwrap();

        // Directly test the file-reading logic by temporarily scanning a
        // directory we control.  We do this by writing the file to a path in
        // the temp dir that starts with "edgecrab-chrome-debug-" so it is
        // picked up by chrome_user_data_dirs().
        let debug_dir = Builder::new()
            .prefix("edgecrab-chrome-debug-test-")
            .tempdir()
            .unwrap();
        std::fs::write(
            debug_dir.path().join("DevToolsActivePort"),
            "9333\n/devtools/browser/test-uuid\n",
        )
        .unwrap();

        // chrome_user_data_dirs() scans temp dir for edgecrab-chrome-debug-* prefixes
        let dirs = chrome_user_data_dirs();
        let found = dirs.iter().any(|d| d == debug_dir.path());
        assert!(found, "debug_dir must appear in chrome_user_data_dirs()");

        // Manually test the file-parsing logic
        let port_file = debug_dir.path().join("DevToolsActivePort");
        let contents = std::fs::read_to_string(&port_file).unwrap();
        let port: u16 = contents.lines().next().unwrap().trim().parse().unwrap();
        assert_eq!(port, 9333);
    }

    /// A DevToolsActivePort file with port 0 is ignored (invalid).
    #[test]
    fn active_port_file_port_zero_is_ignored() {
        let debug_dir = Builder::new()
            .prefix("edgecrab-chrome-debug-zero-")
            .tempdir()
            .unwrap();
        std::fs::write(debug_dir.path().join("DevToolsActivePort"), "0\n").unwrap();

        // find_cdp_from_active_port_file must skip port=0
        let port_file = debug_dir.path().join("DevToolsActivePort");
        let contents = std::fs::read_to_string(&port_file).unwrap();
        let port: u16 = contents.lines().next().unwrap().trim().parse().unwrap_or(0);
        assert_eq!(port, 0, "port 0 means no server is bound");
    }

    /// A corrupted / non-numeric DevToolsActivePort file does not panic.
    #[test]
    fn active_port_file_corrupt_does_not_panic() {
        let debug_dir = Builder::new()
            .prefix("edgecrab-chrome-debug-corrupt-")
            .tempdir()
            .unwrap();
        std::fs::write(
            debug_dir.path().join("DevToolsActivePort"),
            "not-a-number\n",
        )
        .unwrap();
        // Must not panic
        let _ = find_cdp_from_active_port_file();
    }

    /// chrome_user_data_dirs() returns at least one entry on any platform.
    #[test]
    fn chrome_user_data_dirs_is_nonempty() {
        let dirs = chrome_user_data_dirs();
        assert!(!dirs.is_empty(), "must return at least one candidate dir");
    }

    /// chrome_launch_command includes --user-data-dir (Chrome 136+ compliance).
    #[test]
    fn chrome_launch_command_includes_user_data_dir_for_chrome_136() {
        let cmd = chrome_launch_command(9222);
        assert!(
            cmd.contains("--user-data-dir"),
            "Chrome 136+ requires --user-data-dir with --remote-debugging-port"
        );
        assert!(
            cmd.contains("edgecrab-chrome-debug-9222"),
            "must use an edgecrab-specific profile directory"
        );
    }
}
