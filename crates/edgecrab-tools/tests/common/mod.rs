//! Shared helpers for web search integration tests.
#![allow(dead_code)]
#![allow(clippy::await_holding_lock)]

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use edgecrab_tools::tools::web::search::registry::{reset_registry_for_tests, test_registry_lock};
use edgecrab_tools::{AppConfigRef, ToolContext};
use edgecrab_types::Platform;
use tokio_util::sync::CancellationToken;

/// Default URL when SearXNG is started via `e2e/docker-compose.searxng.yml`.
pub const DEFAULT_SEARXNG_DOCKER_URL: &str = "http://127.0.0.1:8888";

/// Check whether SearXNG JSON search API responds with at least one hit.
pub fn searxng_json_api_ready(base_url: &str) -> bool {
    searxng_result_count(base_url, "rust").unwrap_or(0) > 0
}

/// Parse SearXNG JSON and return result count (None when API unreachable).
pub fn searxng_result_count(base_url: &str, query: &str) -> Option<usize> {
    let url = format!(
        "{}/search?q={}&format=json",
        base_url.trim_end_matches('/'),
        query.replace(' ', "+")
    );
    let output = Command::new("curl")
        .args(["-sf", "--max-time", "10", &url])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let body = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&body).ok()?;
    Some(parsed.get("results")?.as_array()?.len())
}

/// Reset backend registry and return an exclusive lock for the duration of a test.
pub fn registry_guard() -> std::sync::MutexGuard<'static, ()> {
    let lock = test_registry_lock();
    reset_registry_for_tests();
    lock
}

pub fn test_ctx() -> ToolContext {
    ToolContext {
        task_id: "web-search-e2e".into(),
        cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        session_id: "web-search-e2e-session".into(),
        user_task: Some("web search e2e".into()),
        cancel: CancellationToken::new(),
        config: AppConfigRef::default(),
        state_db: None,
        platform: Platform::Cli,
        process_table: None,
        provider: None,
        tool_registry: None,
        delegate_depth: 0,
        delegate_agent_id: None,
        delegate_parent_id: None,
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
        mutation_turn: None,
        lsp_gate: None,
        kanban_task_id: None,
    }
}

pub fn ctx_with_config(cfg: AppConfigRef) -> ToolContext {
    let mut ctx = test_ctx();
    ctx.config = cfg;
    ctx
}

/// RAII guard — sets `EDGECRAB_HOME` for the duration of a test.
pub struct EdgecrabHomeGuard {
    previous: Option<String>,
}

impl EdgecrabHomeGuard {
    pub fn set(path: &std::path::Path) -> Self {
        let previous = std::env::var("EDGECRAB_HOME").ok();
        unsafe { std::env::set_var("EDGECRAB_HOME", path) };
        Self { previous }
    }
}

impl Drop for EdgecrabHomeGuard {
    fn drop(&mut self) {
        unsafe { std::env::remove_var("EDGECRAB_HOME") };
        if let Some(value) = &self.previous {
            unsafe { std::env::set_var("EDGECRAB_HOME", value) };
        }
    }
}

/// Alias for [`EdgecrabHomeGuard::set`].
pub fn edgecrab_home_guard(path: &std::path::Path) -> EdgecrabHomeGuard {
    EdgecrabHomeGuard::set(path)
}

pub fn register_mock(
    name: &'static str,
    mode: edgecrab_tools::tools::web::search::backends::mock::MockMode,
) {
    use edgecrab_tools::register_web_search_backend;
    use edgecrab_tools::tools::web::search::backends::mock::MockBackend;
    register_web_search_backend(Arc::new(MockBackend::new(name, mode)));
}

/// Replace live DDGS with a deterministic mock (avoids bot-challenge flakes).
pub fn register_ddgs_mock_success() {
    use edgecrab_tools::tools::web::search::backend::SearchResult;
    use edgecrab_tools::tools::web::search::backends::mock::MockMode;
    register_mock(
        "ddgs",
        MockMode::Success(vec![SearchResult::new(
            1,
            "Mock DDGS Hit",
            "https://example.com/ddgs",
            "mock snippet",
            "ddgs",
        )]),
    );
}

/// Mock DDGS as unavailable so chain tests do not hit the live network.
pub fn register_ddgs_mock_fail() {
    use edgecrab_tools::tools::web::search::backends::mock::MockMode;
    register_mock("ddgs", MockMode::Network);
}

/// Build `n` mock hits for limit/clamp tests.
pub fn many_mock_results(
    n: usize,
    source: &str,
) -> Vec<edgecrab_tools::tools::web::search::backend::SearchResult> {
    use edgecrab_tools::tools::web::search::backend::SearchResult;
    (1..=n)
        .map(|i| {
            SearchResult::new(
                i,
                format!("Hit {i}"),
                format!("https://example.com/{i}"),
                format!("snippet {i}"),
                source,
            )
        })
        .collect()
}

/// Apply env vars required for Docker SearXNG on loopback.
pub fn apply_searxng_docker_env(url: &str) {
    unsafe {
        std::env::set_var("SEARXNG_URL", url);
        std::env::set_var("EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST", "1");
    }
}

/// Resolve SearXNG docker URL when the JSON API is up, else `None`.
pub fn searxng_docker_url_if_ready() -> Option<String> {
    let url = std::env::var("SEARXNG_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_SEARXNG_DOCKER_URL.to_string());
    if searxng_json_api_ready(&url) {
        Some(url)
    } else {
        None
    }
}
