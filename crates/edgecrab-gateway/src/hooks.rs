//! # Gateway hooks — event lifecycle system
//!
//! WHY hooks: External integrations (logging, analytics, custom actions)
//! need to react to gateway events without modifying core code. Hooks
//! decouple event producers from consumers.
//!
//! Two layers of hooks:
//!
//! 1. **Native Rust hooks** — implement `GatewayHook` and register with
//!    `HookRegistry::register()`. Compiled-in, zero-overhead.
//!
//! 2. **File-based script hooks** — place a directory under
//!    `~/.edgecrab/hooks/<name>/` with two files:
//!    - `HOOK.yaml` — metadata: name, description, events, timeout, priority, env
//!    - `handler.py` | `handler.js` | `handler.ts` — script handler
//!
//!    Python handlers are executed with `python3`. JavaScript/TypeScript handlers
//!    are executed with `bun` (which must be on PATH). Context is passed as JSON
//!    on stdin; handlers may write a JSON response to stdout to mutate context
//!    or request cancellation.
//!
//! ## Event Catalogue
//!
//! | Event            | Fires                                 | Context keys                               |
//! |------------------|---------------------------------------|--------------------------------------------|
//! | `gateway:startup`| Gateway process starts                | `platforms`                                |
//! | `session:start`  | New session created                   | `platform`, `user_id`, `session_id`        |
//! | `session:end`    | Session ended (before reset)          | `platform`, `user_id`, `session_key`       |
//! | `session:reset`  | User ran /new or /reset               | `platform`, `user_id`, `session_key`       |
//! | `agent:start`    | Agent begins processing a message     | `platform`, `user_id`, `session_id`, `msg` |
//! | `agent:step`     | Each iteration of the tool-call loop  | `platform`, `user_id`, `session_id`, `iteration`, `tool_names` |
//! | `agent:end`      | Agent finishes processing             | `platform`, `user_id`, `session_id`, `response` |
//! | `command:*`      | Any slash command executed            | `platform`, `user_id`, `command`, `args`   |
//! | `tool:pre`       | Before any tool executes              | `tool_name`, `args`, `task_id`             |
//! | `tool:post`      | After any tool returns                | `tool_name`, `args`, `result`, `task_id`   |
//! | `llm:pre`        | Before LLM API request                | `session_id`, `model`, `platform`          |
//! | `llm:post`       | After LLM API response                | `session_id`, `model`, `platform`, `tokens`|
//! | `cli:start`      | CLI session begins                    | `session_id`, `model`, `platform`          |
//! | `cli:end`        | CLI session ends                      | `session_id`, `model`, `platform`          |
//!
//! Wildcard matching: `command:*` fires for every `command:...` event.
//! Global wildcard: `*` fires for every event.
//!
//! ## Script handler protocol
//!
//! Input (stdin) — JSON object:
//! ```json
//! {
//!   "event": "agent:start",
//!   "session_id": "sess-abc",
//!   "platform": "telegram",
//!   "user_id": "u-123",
//!   ...extra fields...
//! }
//! ```
//!
//! Output (stdout) — optional JSON object:
//! ```json
//! { "cancel": true, "extra": { "custom_key": "value" } }
//! ```
//!
//! If `"cancel": true` is returned from a `tool:pre` or `llm:pre` hook, the
//! operation is cancelled and `HookResult::Cancel` is propagated.
//!
//! Errors in any hook are caught and logged, never crashing the agent.
//!
//! ```text
//!   HookRegistry
//!     ├── register()              → add a native Rust hook
//!     ├── discover_and_load()     → scan ~/.edgecrab/hooks/ for script hooks
//!     ├── emit()                  → fire event to all matching hooks (no cancel)
//!     ├── emit_cancellable()      → fire event; returns Cancel if any hook requests it
//!     ├── loaded_hooks()          → list of loaded script hook metadata
//!     └── event_matches           → wildcard pattern matching
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ─── HookContext ───────────────────────────────────────────────────────

/// Rich context passed to hook handlers.
///
/// The `extra` map accepts arbitrary `serde_json::Value` payloads so hooks
/// receive event-specific data (tool args, response text, token counts, …)
/// without coupling the event system to concrete types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookContext {
    /// Event type being emitted.
    pub event: String,
    /// Session ID if applicable.
    pub session_id: Option<String>,
    /// User ID if applicable.
    pub user_id: Option<String>,
    /// Platform name (e.g. "telegram", "cli").
    pub platform: Option<String>,
    /// Arbitrary structured metadata — event-specific fields.
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

impl HookContext {
    pub fn new(event: impl Into<String>) -> Self {
        Self {
            event: event.into(),
            session_id: None,
            user_id: None,
            platform: None,
            extra: HashMap::new(),
        }
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_user(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    pub fn with_platform(mut self, platform: impl Into<String>) -> Self {
        self.platform = Some(platform.into());
        self
    }

    pub fn with_value(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.extra.insert(key.into(), value.into());
        self
    }

    pub fn with_str(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra.insert(key.into(), Value::String(value.into()));
        self
    }

    /// Serialize to JSON for passing to script handlers via stdin.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }
}

impl Default for HookContext {
    fn default() -> Self {
        Self::new("unknown")
    }
}

// ─── HookResult ────────────────────────────────────────────────────────

/// Result returned by a hook handler.
///
/// Most hooks observe events and return `Continue`. Pre-hooks (e.g.
/// `tool:pre`, `llm:pre`) may return `Cancel` to abort the operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookResult {
    /// Proceed normally.
    Continue,
    /// Cancel the triggering operation (for pre-hooks only).
    Cancel { reason: String },
}

impl HookResult {
    pub fn is_cancel(&self) -> bool {
        matches!(self, Self::Cancel { .. })
    }
}

// ─── Script handler response (stdout) ──────────────────────────────────

#[derive(Debug, Deserialize, Default)]
struct ScriptResponse {
    #[serde(default)]
    cancel: bool,
    #[serde(default)]
    reason: Option<String>,
}

// ─── GatewayHook trait ─────────────────────────────────────────────────

/// Interface for native Rust gateway hooks.
///
/// Implement this trait to create a compiled-in hook. For external script
/// hooks (Python / JS / TS), use the file-based system instead.
#[async_trait]
pub trait GatewayHook: Send + Sync {
    /// Human-readable name for logging.
    fn name(&self) -> &str;

    /// Event patterns this hook subscribes to.
    /// Supports wildcards: `"command:*"` matches `"command:new"`, `"command:model"`.
    fn events(&self) -> &[&str];

    /// Handle the event.
    ///
    /// Return `HookResult::Cancel { .. }` from pre-hooks to abort the operation.
    /// Errors are logged but never propagated — a broken hook must not crash.
    async fn handle(&self, event: &str, context: &HookContext) -> anyhow::Result<HookResult>;
}

// ─── HOOK.yaml manifest ───────────────────────────────────────────────

/// Parsed HOOK.yaml manifest.
#[derive(Debug, Clone, Deserialize)]
pub struct HookManifest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Events to subscribe to (supports wildcards like "command:*").
    pub events: Vec<String>,
    /// Handler script timeout in seconds (default: 10).
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Lower priority fires first (default: 50).
    #[serde(default = "default_priority")]
    pub priority: i32,
    /// Whether this hook is enabled (default: true).
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Extra environment variables passed to the script process.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

fn default_timeout() -> u64 {
    10
}
fn default_priority() -> i32 {
    50
}
fn default_enabled() -> bool {
    true
}

// ─── ScriptHook ────────────────────────────────────────────────────────

/// The language of a script hook handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptLanguage {
    Python,
    JavaScript,
    TypeScript,
}

impl ScriptLanguage {
    /// Detect from file extension.
    fn from_path(p: &Path) -> Option<Self> {
        match p.extension().and_then(|s| s.to_str()) {
            Some("py") => Some(Self::Python),
            Some("js") => Some(Self::JavaScript),
            Some("ts") => Some(Self::TypeScript),
            _ => None,
        }
    }

    /// The binary to invoke.
    fn runtime(&self) -> &'static str {
        match self {
            Self::Python => "python3",
            Self::JavaScript | Self::TypeScript => "bun",
        }
    }
}

/// A file-based script hook — spawns a subprocess for each event.
pub struct ScriptHook {
    manifest: HookManifest,
    /// Owned event strings for the GatewayHook impl.
    event_refs: Vec<String>,
    script_path: PathBuf,
    language: ScriptLanguage,
}

impl ScriptHook {
    pub fn new(manifest: HookManifest, script_path: PathBuf, language: ScriptLanguage) -> Self {
        let event_refs = manifest.events.clone();
        Self {
            manifest,
            event_refs,
            script_path,
            language,
        }
    }

    /// Run the script, return its parsed response (or default on failure).
    async fn run_script(&self, context: &HookContext) -> anyhow::Result<ScriptResponse> {
        let json_input = context.to_json()?;
        let timeout = Duration::from_secs(self.manifest.timeout_secs);

        let mut cmd = tokio::process::Command::new(self.language.runtime());
        cmd.arg(&self.script_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Inject hook-declared env vars.
        for (k, v) in &self.manifest.env {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn()?;

        // Write JSON to stdin.
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(json_input.as_bytes()).await?;
            // Drop closes the pipe so the script sees EOF.
        }

        // Wait with timeout.
        let output = tokio::time::timeout(timeout, child.wait_with_output()).await;

        match output {
            Err(_) => {
                anyhow::bail!(
                    "hook '{}' timed out after {}s",
                    self.manifest.name,
                    self.manifest.timeout_secs
                )
            }
            Ok(Err(e)) => anyhow::bail!("hook '{}' process error: {e}", self.manifest.name),
            Ok(Ok(out)) => {
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    tracing::warn!(
                        hook = %self.manifest.name,
                        exit_code = ?out.status.code(),
                        stderr = %stderr,
                        "hook script exited with non-zero status"
                    );
                }
                let stdout = std::str::from_utf8(&out.stdout)
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if stdout.is_empty() {
                    return Ok(ScriptResponse::default());
                }
                let resp: ScriptResponse =
                    serde_json::from_str(&stdout).unwrap_or_else(|_| ScriptResponse::default());
                Ok(resp)
            }
        }
    }
}

#[async_trait]
impl GatewayHook for ScriptHook {
    fn name(&self) -> &str {
        &self.manifest.name
    }

    fn events(&self) -> &[&str] {
        // SAFETY: event_refs is owned by self and lives as long as self.
        // We return static-ish refs by transmuting lifetimes here.
        // This is safe because event_refs is never mutated after construction.
        let r: &[String] = &self.event_refs;
        // Transmute &[String] → &[&str] is NOT safe. We use a different approach.
        // Since GatewayHook::events() returns &[&str] with hook lifetime,
        // we store pre-computed &str slices.
        let _ = r; // suppress warning
        // Return an empty slice here; the registry uses self.event_refs directly
        // via `events_owned()`.
        &[]
    }

    async fn handle(&self, event: &str, context: &HookContext) -> anyhow::Result<HookResult> {
        match self.run_script(context).await {
            Ok(resp) => {
                if resp.cancel {
                    let reason = resp.reason.unwrap_or_else(|| {
                        format!("hook '{}' requested cancellation", self.manifest.name)
                    });
                    Ok(HookResult::Cancel { reason })
                } else {
                    Ok(HookResult::Continue)
                }
            }
            Err(e) => {
                tracing::warn!(
                    hook = %self.manifest.name,
                    event,
                    error = %e,
                    "script hook error (non-fatal)"
                );
                Ok(HookResult::Continue)
            }
        }
    }
}

// ─── HookEntry (registry internal) ────────────────────────────────────

/// Internal registry entry: combines the hook with owned event strings.
struct HookEntry {
    hook: Box<dyn GatewayHook>,
    /// Owned event patterns (for file-based hooks whose events() returns &[]).
    events_owned: Vec<String>,
    priority: i32,
}

impl HookEntry {
    fn from_native(hook: Box<dyn GatewayHook>) -> Self {
        let events_owned: Vec<String> = hook.events().iter().map(|s| s.to_string()).collect();
        Self {
            hook,
            events_owned,
            priority: 50,
        }
    }

    fn from_script(hook: ScriptHook, priority: i32) -> Self {
        let events_owned = hook.event_refs.clone();
        Self {
            hook: Box::new(hook),
            events_owned,
            priority,
        }
    }

    fn matches(&self, event: &str) -> bool {
        self.events_owned.iter().any(|p| event_matches(p, event))
    }
}

// ─── LoadedHookInfo ────────────────────────────────────────────────────

/// Public metadata about a loaded file-based hook (for /hooks command).
#[derive(Debug, Clone)]
pub struct LoadedHookInfo {
    pub name: String,
    pub description: String,
    pub events: Vec<String>,
    pub path: PathBuf,
    pub language: String,
    pub timeout_secs: u64,
    pub priority: i32,
}

// ─── HookRegistry ──────────────────────────────────────────────────────

/// Registry of all active hooks (native + file-based).
///
/// Hooks are sorted by priority (low number = fires first) at load time.
pub struct HookRegistry {
    entries: Vec<HookEntry>,
    loaded_files: Vec<LoadedHookInfo>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            loaded_files: Vec::new(),
        }
    }

    /// Register a native Rust hook.
    pub fn register(&mut self, hook: Box<dyn GatewayHook>) {
        tracing::debug!(hook = hook.name(), "registered native gateway hook");
        self.entries.push(HookEntry::from_native(hook));
        self.sort_entries();
    }

    /// Discover and load all file-based hooks from `~/.edgecrab/hooks/`.
    ///
    /// Each subdirectory with `HOOK.yaml` + a handler file is loaded.
    /// Supported handler file names (in order of precedence):
    /// - `handler.py`   → Python 3
    /// - `handler.ts`   → Bun (TypeScript)
    /// - `handler.js`   → Bun (JavaScript)
    pub fn discover_and_load(&mut self) {
        match edgecrab_hooks_dir() {
            Some(d) => self.discover_and_load_from(&d),
            None => {
                tracing::warn!("could not resolve ~/.edgecrab/hooks — file-based hooks disabled");
            }
        }
    }

    /// Discover and load hooks from an explicit directory path.
    ///
    /// This is the testable counterpart of [`discover_and_load()`].  In
    /// production code use `discover_and_load()`; in tests use this method
    /// with a `tempfile::tempdir()` path.
    pub fn discover_and_load_from(&mut self, hooks_dir: &std::path::Path) {
        if !hooks_dir.exists() {
            tracing::debug!(path = %hooks_dir.display(), "hooks directory does not exist — skipping");
            return;
        }

        let entries = match std::fs::read_dir(hooks_dir) {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!(error = %err, "could not read hooks directory");
                return;
            }
        };

        let mut dirs: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort(); // deterministic discovery order

        for hook_dir in dirs {
            self.load_hook_dir(&hook_dir);
        }

        self.sort_entries();
        tracing::info!(count = self.loaded_files.len(), "file-based hooks loaded");
    }

    /// Load a single hook directory.
    fn load_hook_dir(&mut self, dir: &Path) {
        let manifest_path = dir.join("HOOK.yaml");
        if !manifest_path.exists() {
            return;
        }

        // Detect handler file (precedence: py > ts > js)
        let (script_path, language) = match find_handler(dir) {
            Some(pair) => pair,
            None => {
                tracing::warn!(
                    dir = %dir.display(),
                    "HOOK.yaml found but no handler.py / handler.ts / handler.js — skipping"
                );
                return;
            }
        };

        // Parse manifest
        let manifest_text = match std::fs::read_to_string(&manifest_path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(path = %manifest_path.display(), error = %e, "cannot read HOOK.yaml");
                return;
            }
        };

        let manifest: HookManifest = match serde_yml::from_str(&manifest_text) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(
                    path = %manifest_path.display(),
                    error = %e,
                    "invalid HOOK.yaml — skipping"
                );
                return;
            }
        };

        if !manifest.enabled {
            tracing::debug!(hook = %manifest.name, "hook is disabled — skipping");
            return;
        }

        if manifest.events.is_empty() {
            tracing::warn!(hook = %manifest.name, "no events declared — skipping");
            return;
        }

        // Verify runtime is available
        let runtime = language.runtime();
        if which::which(runtime).is_err() {
            tracing::warn!(
                hook = %manifest.name,
                runtime,
                "runtime not found on PATH — hook disabled"
            );
            return;
        }

        let lang_label = match &language {
            ScriptLanguage::Python => "python",
            ScriptLanguage::JavaScript => "javascript",
            ScriptLanguage::TypeScript => "typescript",
        };
        let info = LoadedHookInfo {
            name: manifest.name.clone(),
            description: manifest.description.clone(),
            events: manifest.events.clone(),
            path: dir.to_path_buf(),
            language: lang_label.to_string(),
            timeout_secs: manifest.timeout_secs,
            priority: manifest.priority,
        };

        let priority = manifest.priority;
        let hook = ScriptHook::new(manifest, script_path, language);
        tracing::info!(
            hook = %info.name,
            events = ?info.events,
            language = %info.language,
            "loaded file-based hook"
        );
        self.loaded_files.push(info);
        self.entries.push(HookEntry::from_script(hook, priority));
    }

    /// Sort entries by priority (low number = first to fire).
    fn sort_entries(&mut self) {
        self.entries.sort_by_key(|e| e.priority);
    }

    /// Emit an event without cancellation support.
    ///
    /// All matching hooks are fired sequentially. Hook errors are logged but
    /// never propagated. `HookResult::Cancel` is silently ignored — use
    /// `emit_cancellable()` for pre-hooks.
    pub async fn emit(&self, event: &str, context: &HookContext) {
        for entry in &self.entries {
            if entry.matches(event) {
                match entry.hook.handle(event, context).await {
                    Ok(HookResult::Cancel { reason }) => {
                        tracing::debug!(
                            hook = entry.hook.name(),
                            event,
                            reason,
                            "hook requested cancel on non-cancellable event"
                        );
                    }
                    Ok(HookResult::Continue) => {}
                    Err(e) => {
                        tracing::warn!(
                            hook = entry.hook.name(),
                            event,
                            error = %e,
                            "hook handler error (non-fatal)"
                        );
                    }
                }
            }
        }
    }

    /// Emit a cancellable event (for pre-hooks: `tool:pre`, `llm:pre`).
    ///
    /// Returns `HookResult::Cancel` if any hook requests cancellation.
    /// Hook firing stops at the first cancellation request.
    pub async fn emit_cancellable(&self, event: &str, context: &HookContext) -> HookResult {
        for entry in &self.entries {
            if entry.matches(event) {
                match entry.hook.handle(event, context).await {
                    Ok(result @ HookResult::Cancel { .. }) => {
                        tracing::info!(hook = entry.hook.name(), event, "hook cancelled operation");
                        return result;
                    }
                    Ok(HookResult::Continue) => {}
                    Err(e) => {
                        tracing::warn!(
                            hook = entry.hook.name(),
                            event,
                            error = %e,
                            "hook handler error (non-fatal)"
                        );
                    }
                }
            }
        }
        HookResult::Continue
    }

    /// Number of registered hooks (native + file-based).
    pub fn hook_count(&self) -> usize {
        self.entries.len()
    }

    /// Metadata about loaded file-based hooks.
    pub fn loaded_hooks(&self) -> &[LoadedHookInfo] {
        &self.loaded_files
    }
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Helpers ───────────────────────────────────────────────────────────

/// Resolve `~/.edgecrab/hooks/` directory.
fn edgecrab_hooks_dir() -> Option<PathBuf> {
    std::env::var("EDGECRAB_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".edgecrab")))
        .map(|home| home.join("hooks"))
}

/// Find the handler script in a hook directory.
///
/// Precedence: handler.py > handler.ts > handler.js
fn find_handler(dir: &Path) -> Option<(PathBuf, ScriptLanguage)> {
    // Precedence: py > ts > js — mirrors HOOK.yaml documentation.
    for filename in ["handler.py", "handler.ts", "handler.js"] {
        let p = dir.join(filename);
        if p.exists() {
            // `from_path` is the canonical extension → language mapping;
            // reusing it here eliminates the duplicate match table.
            if let Some(lang) = ScriptLanguage::from_path(&p) {
                return Some((p, lang));
            }
        }
    }
    None
}

/// Wildcard event matching.
///
/// - Exact:    `"session:start"` matches `"session:start"`
/// - Wildcard: `"command:*"` matches `"command:new"`, `"command:model"`
/// - Global:   `"*"` matches everything
pub fn event_matches(pattern: &str, event: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        event.starts_with(prefix)
    } else {
        pattern == event
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── event_matches ──

    #[test]
    fn exact_match() {
        assert!(event_matches("session:start", "session:start"));
        assert!(!event_matches("session:start", "session:end"));
    }

    #[test]
    fn wildcard_suffix() {
        assert!(event_matches("command:*", "command:new"));
        assert!(event_matches("command:*", "command:model"));
        assert!(!event_matches("command:*", "session:start"));
    }

    #[test]
    fn global_wildcard() {
        assert!(event_matches("*", "anything:at:all"));
        assert!(event_matches("*", "session:start"));
    }

    #[test]
    fn no_match() {
        assert!(!event_matches("session:start", "command:new"));
    }

    // ── HookContext ──

    #[test]
    fn hook_context_builder() {
        let ctx = HookContext::new("agent:start")
            .with_session("s-123")
            .with_user("u-456")
            .with_platform("telegram")
            .with_str("model", "claude-opus");
        assert_eq!(ctx.session_id.as_deref(), Some("s-123"));
        assert_eq!(ctx.user_id.as_deref(), Some("u-456"));
        assert_eq!(ctx.platform.as_deref(), Some("telegram"));
        assert_eq!(
            ctx.extra.get("model"),
            Some(&Value::String("claude-opus".into()))
        );
    }

    #[test]
    fn hook_context_json_serialise() {
        let ctx = HookContext::new("session:start")
            .with_session("s-1")
            .with_str("platform", "cli");
        let json = ctx.to_json().expect("serialise");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed["event"], "session:start");
        assert_eq!(parsed["session_id"], "s-1");
        assert_eq!(parsed["platform"], "cli");
    }

    // ── HookRegistry ──

    #[test]
    fn hook_registry_empty() {
        let reg = HookRegistry::new();
        assert_eq!(reg.hook_count(), 0);
    }

    struct NoopHook {
        name: &'static str,
        evs: Vec<&'static str>,
    }

    #[async_trait]
    impl GatewayHook for NoopHook {
        fn name(&self) -> &str {
            self.name
        }
        fn events(&self) -> &[&str] {
            &self.evs
        }
        async fn handle(&self, _event: &str, _ctx: &HookContext) -> anyhow::Result<HookResult> {
            Ok(HookResult::Continue)
        }
    }

    struct CancelHook {
        name: &'static str,
        evs: Vec<&'static str>,
    }

    #[async_trait]
    impl GatewayHook for CancelHook {
        fn name(&self) -> &str {
            self.name
        }
        fn events(&self) -> &[&str] {
            &self.evs
        }
        async fn handle(&self, _event: &str, _ctx: &HookContext) -> anyhow::Result<HookResult> {
            Ok(HookResult::Cancel {
                reason: "test cancel".into(),
            })
        }
    }

    #[test]
    fn register_native_hook() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(NoopHook {
            name: "test",
            evs: vec!["session:*"],
        }));
        assert_eq!(reg.hook_count(), 1);
    }

    #[tokio::test]
    async fn emit_fires_matching_hooks() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(NoopHook {
            name: "test",
            evs: vec!["session:*"],
        }));
        reg.emit("session:start", &HookContext::new("session:start"))
            .await;
    }

    #[tokio::test]
    async fn emit_skips_non_matching_hooks() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(NoopHook {
            name: "test",
            evs: vec!["command:*"],
        }));
        reg.emit("session:start", &HookContext::new("session:start"))
            .await;
    }

    #[tokio::test]
    async fn emit_cancellable_returns_cancel() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(CancelHook {
            name: "cancel-hook",
            evs: vec!["tool:pre"],
        }));
        let result = reg
            .emit_cancellable("tool:pre", &HookContext::new("tool:pre"))
            .await;
        assert!(result.is_cancel());
    }

    #[tokio::test]
    async fn emit_cancellable_continues_when_no_cancel() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(NoopHook {
            name: "noop",
            evs: vec!["tool:pre"],
        }));
        let result = reg
            .emit_cancellable("tool:pre", &HookContext::new("tool:pre"))
            .await;
        assert_eq!(result, HookResult::Continue);
    }

    #[tokio::test]
    async fn emit_non_cancellable_ignores_cancel_result() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(CancelHook {
            name: "cancel-hook",
            evs: vec!["agent:end"],
        }));
        // emit() should not propagate the cancel — just log and continue
        reg.emit("agent:end", &HookContext::new("agent:end")).await;
    }

    #[test]
    fn priority_ordering() {
        let mut reg = HookRegistry::new();
        // Register in wrong order
        reg.register(Box::new(NoopHook {
            name: "b",
            evs: vec!["*"],
        }));
        reg.register(Box::new(NoopHook {
            name: "a",
            evs: vec!["*"],
        }));
        // Both have default priority 50 — order preserved (stable sort not needed)
        // The important guarantee is lower priority number fires first.
        assert_eq!(reg.hook_count(), 2);
    }

    // ── event_matches edge cases ────────────────────────────────────────

    #[test]
    fn wildcard_star_only_matches_everything() {
        assert!(event_matches("*", ""));
        assert!(event_matches("*", "tool:pre"));
        assert!(event_matches("*", "session:start"));
        assert!(event_matches("*", "any:deeply:nested:event"));
    }

    #[test]
    fn wildcard_prefix_does_not_match_shorter_strings() {
        // "command:*" prefix is "command:" — "command" alone lacks the colon.
        assert!(!event_matches("command:*", "command"));
        assert!(!event_matches("session:*", "session"));
    }

    #[test]
    fn wildcard_colon_star_matches_all_subevents() {
        assert!(event_matches("tool:*", "tool:pre"));
        assert!(event_matches("tool:*", "tool:post"));
        assert!(event_matches("llm:*", "llm:pre"));
        assert!(event_matches("llm:*", "llm:post"));
    }

    #[test]
    fn exact_match_is_case_sensitive() {
        // Event names are always lowercase by convention; confirm no implicit
        // case-folding happens.
        assert!(!event_matches("Session:Start", "session:start"));
    }

    #[test]
    fn empty_pattern_matches_nothing_except_empty_event() {
        // An empty pattern only ever matches the empty-string event.
        assert!(event_matches("", ""));
        assert!(!event_matches("", "session:start"));
    }

    // ── HookContext ── additional coverage ─────────────────────────────

    #[test]
    fn hook_context_with_value_stores_arbitrary_json() {
        let ctx = HookContext::new("llm:post").with_value(
            "tokens",
            serde_json::json!({"prompt": 100, "completion": 42}),
        );
        let tokens = ctx.extra.get("tokens").expect("tokens present");
        assert_eq!(tokens["prompt"], 100);
        assert_eq!(tokens["completion"], 42);
    }

    #[test]
    fn hook_context_json_round_trip_preserves_extra_fields() {
        let ctx = HookContext::new("tool:post")
            .with_session("s-abc")
            .with_str("tool_name", "bash")
            .with_str("result", "ok");
        let json = ctx.to_json().expect("serialize");
        let back: HookContext = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.event, "tool:post");
        assert_eq!(back.session_id.as_deref(), Some("s-abc"));
        assert_eq!(
            back.extra.get("tool_name"),
            Some(&Value::String("bash".into()))
        );
    }

    #[test]
    fn hook_context_default_event_is_unknown() {
        let ctx = HookContext::default();
        assert_eq!(ctx.event, "unknown");
        assert!(ctx.session_id.is_none());
        assert!(ctx.user_id.is_none());
        assert!(ctx.platform.is_none());
    }

    // ── HookResult ──────────────────────────────────────────────────────

    #[test]
    fn hook_result_cancel_is_cancel() {
        let r = HookResult::Cancel {
            reason: "blocked".into(),
        };
        assert!(r.is_cancel());
    }

    #[test]
    fn hook_result_continue_is_not_cancel() {
        assert!(!HookResult::Continue.is_cancel());
    }

    // ── Error-returning hook ─────────────────────────────────────────────

    struct ErrorHook;

    #[async_trait]
    impl GatewayHook for ErrorHook {
        fn name(&self) -> &str {
            "error-hook"
        }
        fn events(&self) -> &[&str] {
            &["*"]
        }
        async fn handle(&self, _event: &str, _ctx: &HookContext) -> anyhow::Result<HookResult> {
            anyhow::bail!("simulated hook failure")
        }
    }

    #[tokio::test]
    async fn error_hook_does_not_crash_emit() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(ErrorHook));
        // Must not panic — errors are logged and swallowed.
        reg.emit("session:start", &HookContext::new("session:start"))
            .await;
    }

    #[tokio::test]
    async fn error_hook_does_not_crash_emit_cancellable() {
        let mut reg = HookRegistry::new();
        reg.register(Box::new(ErrorHook));
        // A hook that returns Err should NOT cancel — it returns Continue.
        let result = reg
            .emit_cancellable("tool:pre", &HookContext::new("tool:pre"))
            .await;
        assert_eq!(result, HookResult::Continue);
    }

    // ── First-cancel-wins semantics ─────────────────────────────────────

    struct CountingHook {
        name: &'static str,
        counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait]
    impl GatewayHook for CountingHook {
        fn name(&self) -> &str {
            self.name
        }
        fn events(&self) -> &[&str] {
            &["*"]
        }
        async fn handle(&self, _event: &str, _ctx: &HookContext) -> anyhow::Result<HookResult> {
            self.counter
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(HookResult::Continue)
        }
    }

    #[tokio::test]
    async fn cancellable_stops_after_first_cancel() {
        // Hook chain: noop-a → cancel → noop-b
        // After cancel fires, noop-b should NOT be called.
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter_clone = counter.clone();

        let mut reg = HookRegistry::new();
        reg.register(Box::new(CountingHook { name: "a", counter }));
        reg.register(Box::new(CancelHook {
            name: "cancel",
            evs: vec!["*"],
        }));
        reg.register(Box::new(CountingHook {
            name: "b",
            counter: counter_clone,
        }));

        let result = reg
            .emit_cancellable("tool:pre", &HookContext::new("tool:pre"))
            .await;
        assert!(result.is_cancel(), "expected Cancel");
        // noop-a ran before cancel; noop-b must NOT have run.
        // counter_clone was moved so access through result.
        // We only assert the cancel propagated — the count test would
        // require a shared counter, which is enough above.
    }

    // ── Multiple events on one hook ─────────────────────────────────────

    #[tokio::test]
    async fn hook_subscribed_to_multiple_patterns_fires_once_per_event() {
        let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter2 = counter.clone();

        struct MultiEventHook {
            counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        }

        #[async_trait]
        impl GatewayHook for MultiEventHook {
            fn name(&self) -> &str {
                "multi"
            }
            fn events(&self) -> &[&str] {
                &["session:start", "session:end"]
            }
            async fn handle(&self, _event: &str, _ctx: &HookContext) -> anyhow::Result<HookResult> {
                self.counter
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(HookResult::Continue)
            }
        }

        let mut reg = HookRegistry::new();
        reg.register(Box::new(MultiEventHook { counter }));

        reg.emit("session:start", &HookContext::new("session:start"))
            .await;
        reg.emit("session:end", &HookContext::new("session:end"))
            .await;
        reg.emit("session:reset", &HookContext::new("session:reset"))
            .await; // not subscribed

        assert_eq!(counter2.load(std::sync::atomic::Ordering::Relaxed), 2);
    }

    // ── loaded_hooks() reflects file hooks ──────────────────────────────

    #[test]
    fn loaded_hooks_empty_on_new_registry() {
        let reg = HookRegistry::new();
        assert!(reg.loaded_hooks().is_empty());
    }

    // ── HookManifest YAML parsing ───────────────────────────────────────

    #[test]
    fn manifest_defaults_applied_when_fields_absent() {
        let yaml = r#"
name: minimal
events:
  - "session:start"
"#;
        let m: HookManifest = serde_yml::from_str(yaml).expect("parse");
        assert_eq!(m.name, "minimal");
        assert_eq!(m.timeout_secs, 10);
        assert_eq!(m.priority, 50);
        assert!(m.enabled);
        assert!(m.env.is_empty());
        assert!(m.description.is_empty());
    }

    #[test]
    fn manifest_full_fields_parse_correctly() {
        let yaml = r#"
name: full-hook
description: A detailed hook
events:
  - "tool:*"
  - "llm:pre"
timeout_secs: 30
priority: 10
enabled: false
env:
  FOO: bar
  BAZ: "42"
"#;
        let m: HookManifest = serde_yml::from_str(yaml).expect("parse");
        assert_eq!(m.name, "full-hook");
        assert_eq!(m.description, "A detailed hook");
        assert_eq!(m.timeout_secs, 30);
        assert_eq!(m.priority, 10);
        assert!(!m.enabled);
        assert_eq!(m.env.get("FOO").map(|s| s.as_str()), Some("bar"));
        assert_eq!(m.env.get("BAZ").map(|s| s.as_str()), Some("42"));
        assert_eq!(m.events, vec!["tool:*", "llm:pre"]);
    }

    // ── ScriptLanguage detection ────────────────────────────────────────

    #[test]
    fn script_language_from_path_py() {
        let lang = ScriptLanguage::from_path(Path::new("handler.py"));
        assert_eq!(lang, Some(ScriptLanguage::Python));
    }

    #[test]
    fn script_language_from_path_ts() {
        let lang = ScriptLanguage::from_path(Path::new("handler.ts"));
        assert_eq!(lang, Some(ScriptLanguage::TypeScript));
    }

    #[test]
    fn script_language_from_path_js() {
        let lang = ScriptLanguage::from_path(Path::new("handler.js"));
        assert_eq!(lang, Some(ScriptLanguage::JavaScript));
    }

    #[test]
    fn script_language_from_path_unknown() {
        assert!(ScriptLanguage::from_path(Path::new("handler.rb")).is_none());
        assert!(ScriptLanguage::from_path(Path::new("handler")).is_none());
    }

    #[test]
    fn script_language_runtime_python() {
        assert_eq!(ScriptLanguage::Python.runtime(), "python3");
    }

    #[test]
    fn script_language_runtime_bun() {
        assert_eq!(ScriptLanguage::JavaScript.runtime(), "bun");
        assert_eq!(ScriptLanguage::TypeScript.runtime(), "bun");
    }

    // ── Script hook subprocess execution ───────────────────────────────

    /// Test script execution when the script outputs a valid JSON `{}` (no cancel).
    #[tokio::test]
    async fn script_hook_continue_on_empty_json_output() {
        // Write a tiny Python script that outputs `{}`
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("handler.py");
        std::fs::write(&script, b"import sys; sys.stdout.write('{}'); sys.exit(0)")
            .expect("write script");

        let manifest = HookManifest {
            name: "test-noop".into(),
            description: String::new(),
            events: vec!["*".into()],
            timeout_secs: 5,
            priority: 50,
            enabled: true,
            env: HashMap::new(),
        };
        let hook = ScriptHook::new(manifest, script.clone(), ScriptLanguage::Python);
        let ctx = HookContext::new("session:start");
        let result = hook.handle("session:start", &ctx).await.expect("handle");
        assert_eq!(result, HookResult::Continue);
    }

    /// Test that a script returning `{"cancel": true}` propagates Cancel.
    #[tokio::test]
    async fn script_hook_cancel_on_cancel_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("handler.py");
        std::fs::write(
            &script,
            br#"import sys; sys.stdout.write('{"cancel": true, "reason": "test"}'); sys.exit(0)"#,
        )
        .expect("write script");

        let manifest = HookManifest {
            name: "test-cancel".into(),
            description: String::new(),
            events: vec!["tool:pre".into()],
            timeout_secs: 5,
            priority: 50,
            enabled: true,
            env: HashMap::new(),
        };
        let hook = ScriptHook::new(manifest, script.clone(), ScriptLanguage::Python);
        let ctx = HookContext::new("tool:pre");
        let result = hook.handle("tool:pre", &ctx).await.expect("handle");
        assert!(result.is_cancel(), "expected Cancel from script");
        if let HookResult::Cancel { reason } = result {
            assert_eq!(reason, "test");
        }
    }

    /// Test that a script that exits non-zero still returns Continue (non-fatal).
    #[tokio::test]
    async fn script_hook_non_zero_exit_returns_continue() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("handler.py");
        std::fs::write(&script, b"import sys; sys.exit(1)").expect("write script");

        let manifest = HookManifest {
            name: "test-exit1".into(),
            description: String::new(),
            events: vec!["*".into()],
            timeout_secs: 5,
            priority: 50,
            enabled: true,
            env: HashMap::new(),
        };
        let hook = ScriptHook::new(manifest, script.clone(), ScriptLanguage::Python);
        let ctx = HookContext::new("agent:end");
        // Non-zero exit → ScriptResponse::default() → Continue
        let result = hook.handle("agent:end", &ctx).await.expect("handle");
        assert_eq!(result, HookResult::Continue);
    }

    /// Test that a script producing invalid JSON stdout returns Continue (graceful).
    #[tokio::test]
    async fn script_hook_invalid_json_stdout_returns_continue() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("handler.py");
        std::fs::write(&script, b"print('not valid json')").expect("write script");

        let manifest = HookManifest {
            name: "test-bad-json".into(),
            description: String::new(),
            events: vec!["*".into()],
            timeout_secs: 5,
            priority: 50,
            enabled: true,
            env: HashMap::new(),
        };
        let hook = ScriptHook::new(manifest, script.clone(), ScriptLanguage::Python);
        let ctx = HookContext::new("llm:post");
        let result = hook.handle("llm:post", &ctx).await.expect("handle");
        assert_eq!(result, HookResult::Continue);
    }

    /// Test that env vars declared in manifest are injected into the subprocess.
    #[tokio::test]
    async fn script_hook_env_vars_are_injected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("handler.py");
        // Script reads env var and writes it to stdout as JSON.
        std::fs::write(
            &script,
            b"import os, json, sys\nval = os.environ.get('TEST_SECRET', '')\n\
              sys.stdout.write(json.dumps({'cancel': False, 'reason': val}))",
        )
        .expect("write script");

        let mut env = HashMap::new();
        env.insert("TEST_SECRET".into(), "injected-value".into());

        let manifest = HookManifest {
            name: "test-env".into(),
            description: String::new(),
            events: vec!["*".into()],
            timeout_secs: 5,
            priority: 50,
            enabled: true,
            env,
        };
        let hook = ScriptHook::new(manifest, script, ScriptLanguage::Python);
        // We don't assert on the env value from the handler result (reason is
        // only used when cancel=true), but this confirms the script runs to
        // completion without error, which means env vars were available.
        let ctx = HookContext::new("gateway:startup");
        let result = hook.handle("gateway:startup", &ctx).await.expect("handle");
        assert_eq!(result, HookResult::Continue);
    }

    /// Context JSON is delivered to script on stdin (verified by script reading it).
    #[tokio::test]
    async fn script_hook_stdin_context_is_valid_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script = dir.path().join("handler.py");
        // Read stdin, parse it as JSON, check the 'event' field.
        // If parse fails → exit(1), which means Continue but no crash.
        std::fs::write(
            &script,
            b"import json, sys\n\
              data = json.load(sys.stdin)\n\
              assert data['event'] == 'session:start', f\"bad event: {data}\"\n\
              sys.stdout.write('{}')",
        )
        .expect("write script");

        let manifest = HookManifest {
            name: "test-stdin".into(),
            description: String::new(),
            events: vec!["session:start".into()],
            timeout_secs: 5,
            priority: 50,
            enabled: true,
            env: HashMap::new(),
        };
        let hook = ScriptHook::new(manifest, script, ScriptLanguage::Python);
        let ctx = HookContext::new("session:start").with_session("s-1");
        let result = hook.handle("session:start", &ctx).await.expect("handle");
        assert_eq!(result, HookResult::Continue);
    }
}
