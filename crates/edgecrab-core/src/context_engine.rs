//! # Pluggable Context Engine
//!
//! Abstraction for custom context management strategies. The default
//! `BuiltinCompressorEngine` wraps the existing `compression.rs` logic.
//! Users can supply custom engines via the plugin system to control what
//! the agent sees each turn (filtering, summarization, domain-specific
//! injection).
//!
//! ## Architecture (DIP / SRP)
//!
//! ```text
//!  edgecrab-core::context_engine
//!  ├── ContextEngine trait          — abstraction only (DIP)
//!  ├── BuiltinCompressorEngine      — default impl (zero subprocess cost)
//!  └── PluginContextEngine          — subprocess JSON-RPC adapter
//!
//!  edgecrab-plugins::context
//!  └── find_context_engine_manifest — pure manifest discovery (no trait dep)
//! ```
//!
//! ## Prompt Caching Constraint
//!
//! Context engines **MUST NOT** modify the system prompt after session
//! start. The only allowed operations are:
//! 1. Inject additional tools at session start (one-time)
//! 2. Handle tool calls during the session
//! 3. Perform cleanup at session end

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use edgecrab_types::{Platform, ToolSchema};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;

/// Session context passed to `on_session_start`.
#[derive(Debug, Clone)]
pub struct ContextEngineSessionCtx {
    pub session_id: String,
    pub edgecrab_home: PathBuf,
    pub platform: Platform,
    pub model: String,
    pub context_length: usize,
}

/// Maximum number of tools a context engine can inject.
pub const MAX_ENGINE_TOOLS: usize = 20;

/// Pluggable context engine trait.
///
/// Implement this to provide custom context management strategies.
/// The agent loop calls lifecycle methods at the appropriate times
/// and dispatches tool calls for engine-provided tools.
#[async_trait]
pub trait ContextEngine: Send + Sync + 'static {
    /// Human-readable engine name (e.g. "compressor", "lcm", "custom").
    fn name(&self) -> &str;

    /// Max context window this engine supports (tokens).
    fn context_length(&self) -> usize;

    /// Token threshold at which compression/shaping triggers.
    fn threshold_tokens(&self) -> usize;

    /// Additional tool schemas this engine injects into the agent's toolset.
    /// These are added once at session start and must not exceed
    /// [`MAX_ENGINE_TOOLS`]. Empty by default.
    fn get_tool_schemas(&self) -> Vec<ToolSchema> {
        vec![]
    }

    /// Handle a tool call that was routed to this engine.
    ///
    /// Called by the conversation loop when the tool name matches one of the
    /// schemas returned by [`get_tool_schemas`]. Default implementation returns
    /// `None` (fall through to ToolRegistry), which is correct for engines
    /// that inject no tools.
    ///
    /// Returns `Some(json_string)` if the engine handled the call, or `None`
    /// to fall through to the standard `ToolRegistry` dispatch.
    async fn handle_tool_call(
        &self,
        _name: &str,
        _args: serde_json::Value,
    ) -> Option<anyhow::Result<String>> {
        None
    }

    /// Called once at session start. Engine may initialize state.
    async fn on_session_start(&self, ctx: ContextEngineSessionCtx) -> anyhow::Result<()>;

    /// Called when session ends (CLI exit, `/reset`, gateway timeout).
    /// Best-effort — engine may have already exited.
    async fn on_session_end(&self, session_id: &str) -> anyhow::Result<()>;

    /// Called when session is reset without ending.
    async fn on_session_reset(&self) -> anyhow::Result<()>;

    /// Whether this engine is available (e.g. required API keys present).
    fn is_available(&self) -> bool;
}

// ─────────────────────────────────────────────────────────────────────────────
// Built-in compressor engine
// ─────────────────────────────────────────────────────────────────────────────

/// Built-in compressor engine wrapping `compression.rs`.
///
/// This is the default context engine. It uses the existing LLM-powered
/// compression pipeline and adds no extra tools.
pub struct BuiltinCompressorEngine {
    ctx_length: usize,
    threshold: f64,
}

impl BuiltinCompressorEngine {
    /// Create a new built-in compressor engine.
    ///
    /// - `context_length`: max tokens for the model's context window
    /// - `threshold`: fraction (0.0–1.0) at which compression triggers
    pub fn new(context_length: usize, threshold: f64) -> Self {
        Self {
            ctx_length: context_length,
            threshold: threshold.clamp(0.1, 0.95),
        }
    }
}

impl Default for BuiltinCompressorEngine {
    fn default() -> Self {
        Self::new(128_000, 0.50)
    }
}

#[async_trait]
impl ContextEngine for BuiltinCompressorEngine {
    fn name(&self) -> &str {
        "compressor"
    }

    fn context_length(&self) -> usize {
        self.ctx_length
    }

    fn threshold_tokens(&self) -> usize {
        (self.ctx_length as f64 * self.threshold) as usize
    }

    async fn on_session_start(&self, _ctx: ContextEngineSessionCtx) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_session_end(&self, _session_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_session_reset(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_available(&self) -> bool {
        true
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Plugin context engine — subprocess JSON-RPC 2.0 over stdio
// ─────────────────────────────────────────────────────────────────────────────

/// State held by the live subprocess connection.
struct PluginProcess {
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
}

impl PluginProcess {
    /// Send a JSON-RPC 2.0 request and return the `result` field.
    async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let id = self.next_id;
        self.next_id += 1;
        let req =
            serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let line = serde_json::to_string(&req)? + "\n";
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;

        // Read until we get the matching response (skip interleaved log lines)
        let mut buf = String::new();
        loop {
            buf.clear();
            let n = self.stdout.read_line(&mut buf).await?;
            if n == 0 {
                anyhow::bail!("plugin process closed stdout unexpectedly");
            }
            let trimmed = buf.trim();
            if trimmed.is_empty() {
                continue;
            }
            let resp: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue, // not JSON — skip (e.g. log line)
            };
            if resp.get("id").and_then(|v| v.as_u64()) == Some(id) {
                if let Some(err) = resp.get("error") {
                    anyhow::bail!("plugin JSON-RPC error: {err}");
                }
                return Ok(resp
                    .get("result")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null));
            }
        }
    }
}

/// Context engine that communicates with a subprocess via JSON-RPC 2.0 over stdio.
///
/// The subprocess must implement the protocol defined in ADR-0605 §5.2.
pub struct PluginContextEngine {
    engine_name: String,
    command: String,
    /// Args stored for potential future restart capability.
    #[allow(dead_code)]
    args: Vec<String>,
    ctx_length: usize,
    threshold: f64,
    cached_schemas: Vec<ToolSchema>,
    proc: Arc<Mutex<Option<PluginProcess>>>,
}

impl PluginContextEngine {
    /// Spawn the subprocess and fetch tool schemas. Returns `Err` if the
    /// process could not be spawned or the initial `get_tool_schemas` call
    /// fails.
    pub async fn start(
        engine_name: String,
        command: String,
        args: Vec<String>,
        ctx_length: usize,
        threshold: f64,
    ) -> anyhow::Result<Self> {
        let mut child = Command::new(&command)
            .args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit()) // plugin logs go to host stderr
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn context engine '{engine_name}': {e}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("plugin stdin unavailable"))?;
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| anyhow::anyhow!("plugin stdout unavailable"))?,
        );

        let mut proc = PluginProcess {
            stdin,
            stdout,
            next_id: 0,
        };

        // Fetch tool schemas once at startup
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            proc.call("get_tool_schemas", serde_json::json!({})),
        )
        .await
        .map_err(|_| anyhow::anyhow!("timeout fetching tool schemas from '{engine_name}'"))??;

        let cached_schemas: Vec<ToolSchema> = if result.is_null() {
            vec![]
        } else {
            serde_json::from_value(result).unwrap_or_default()
        };

        Ok(Self {
            engine_name,
            command,
            args,
            ctx_length,
            threshold: threshold.clamp(0.1, 0.95),
            cached_schemas,
            proc: Arc::new(Mutex::new(Some(proc))),
        })
    }

    async fn rpc_call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let mut guard = self.proc.lock().await;
        let proc = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("plugin process has exited"))?;
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            proc.call(method, params),
        )
        .await
        .map_err(|_| anyhow::anyhow!("timeout on '{method}'"))?
    }
}

#[async_trait]
impl ContextEngine for PluginContextEngine {
    fn name(&self) -> &str {
        &self.engine_name
    }

    fn context_length(&self) -> usize {
        self.ctx_length
    }

    fn threshold_tokens(&self) -> usize {
        (self.ctx_length as f64 * self.threshold) as usize
    }

    fn get_tool_schemas(&self) -> Vec<ToolSchema> {
        self.cached_schemas.clone()
    }

    async fn handle_tool_call(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Option<anyhow::Result<String>> {
        let result = self
            .rpc_call(
                "handle_tool_call",
                serde_json::json!({"name": name, "args": args}),
            )
            .await;
        match result {
            Ok(v) => Some(Ok(v
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| v.to_string()))),
            Err(e) => Some(Err(e)),
        }
    }

    async fn on_session_start(&self, ctx: ContextEngineSessionCtx) -> anyhow::Result<()> {
        self.rpc_call(
            "on_session_start",
            serde_json::json!({
                "session_id": ctx.session_id,
                "edgecrab_home": ctx.edgecrab_home.to_string_lossy(),
                "platform": ctx.platform.to_string(),
                "model": ctx.model,
                "context_length": ctx.context_length,
            }),
        )
        .await?;
        Ok(())
    }

    async fn on_session_end(&self, session_id: &str) -> anyhow::Result<()> {
        // Best-effort: ignore errors (process may have already exited)
        let _ = self
            .rpc_call(
                "on_session_end",
                serde_json::json!({"session_id": session_id}),
            )
            .await;
        // Signal the process to exit cleanly
        let mut guard = self.proc.lock().await;
        *guard = None; // Drop stdin → subprocess receives EOF
        Ok(())
    }

    async fn on_session_reset(&self) -> anyhow::Result<()> {
        let _ = self
            .rpc_call("on_session_reset", serde_json::json!({}))
            .await;
        Ok(())
    }

    fn is_available(&self) -> bool {
        // Check command is on PATH
        std::process::Command::new(&self.command)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|_| true)
            .unwrap_or(false)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Engine loader — cascade: builtin > plugin > warn+fallback
// ─────────────────────────────────────────────────────────────────────────────

/// Load the context engine specified by `engine_name`.
///
/// Cascade:
/// 1. `"compressor"` (or `None`) → `BuiltinCompressorEngine`
/// 2. Plugin found in `~/.edgecrab/plugins/context_engine/<name>/` → `PluginContextEngine`
/// 3. Plugin not found or spawn fails → warn + fallback to `BuiltinCompressorEngine`
pub async fn load_context_engine(
    engine_name: Option<&str>,
    ctx_length: usize,
    threshold: f64,
) -> Arc<dyn ContextEngine> {
    let name = match engine_name {
        None | Some("compressor") | Some("builtin") => {
            return Arc::new(BuiltinCompressorEngine::new(ctx_length, threshold));
        }
        Some(n) => n,
    };

    match edgecrab_plugins::find_context_engine_manifest(name) {
        None => {
            tracing::warn!(
                engine = %name,
                "context engine plugin not found — falling back to builtin compressor"
            );
            Arc::new(BuiltinCompressorEngine::new(ctx_length, threshold))
        }
        Some(manifest) => {
            match PluginContextEngine::start(
                manifest.name,
                manifest.command,
                manifest.args,
                ctx_length,
                threshold,
            )
            .await
            {
                Ok(engine) => {
                    tracing::info!(engine = %name, "context engine plugin loaded");
                    Arc::new(engine)
                }
                Err(e) => {
                    tracing::warn!(
                        engine = %name,
                        error = %e,
                        "failed to start context engine plugin — falling back to builtin compressor"
                    );
                    Arc::new(BuiltinCompressorEngine::new(ctx_length, threshold))
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn builtin_engine_defaults() {
        let engine = BuiltinCompressorEngine::default();
        assert_eq!(engine.name(), "compressor");
        assert_eq!(engine.context_length(), 128_000);
        assert_eq!(engine.threshold_tokens(), 64_000);
        assert!(engine.is_available());
        assert!(engine.get_tool_schemas().is_empty());
    }

    #[test]
    fn builtin_engine_custom() {
        let engine = BuiltinCompressorEngine::new(200_000, 0.70);
        assert_eq!(engine.context_length(), 200_000);
        assert_eq!(engine.threshold_tokens(), 140_000);
    }

    #[test]
    fn threshold_clamped() {
        let engine = BuiltinCompressorEngine::new(100_000, 1.5);
        assert_eq!(engine.threshold_tokens(), 95_000);

        let engine = BuiltinCompressorEngine::new(100_000, 0.01);
        assert_eq!(engine.threshold_tokens(), 10_000);
    }

    #[tokio::test]
    async fn lifecycle_methods_succeed() {
        let engine = BuiltinCompressorEngine::default();
        let ctx = ContextEngineSessionCtx {
            session_id: "test-123".into(),
            edgecrab_home: std::path::PathBuf::from("/tmp/test"),
            platform: Platform::Cli,
            model: "test/model".into(),
            context_length: 128_000,
        };
        engine.on_session_start(ctx).await.unwrap();
        engine.on_session_reset().await.unwrap();
        engine.on_session_end("test-123").await.unwrap();
    }

    #[tokio::test]
    async fn handle_tool_call_default_returns_none() {
        let engine = BuiltinCompressorEngine::default();
        let result = engine
            .handle_tool_call("any_tool", serde_json::json!({}))
            .await;
        assert!(result.is_none(), "builtin engine must not handle any tools");
    }

    #[test]
    fn max_engine_tools_limit() {
        assert_eq!(MAX_ENGINE_TOOLS, 20);
    }

    #[tokio::test]
    async fn load_context_engine_builtin_names() {
        // All builtin aliases resolve to BuiltinCompressorEngine (no subprocess)
        for name in &[None, Some("compressor"), Some("builtin")] {
            let engine = load_context_engine(*name, 128_000, 0.5).await;
            assert_eq!(engine.name(), "compressor");
        }
    }

    #[tokio::test]
    async fn load_context_engine_unknown_falls_back() {
        let engine = load_context_engine(Some("__no_such_engine__"), 64_000, 0.6).await;
        // Must fall back gracefully — name is still "compressor"
        assert_eq!(engine.name(), "compressor");
        assert_eq!(engine.context_length(), 64_000);
    }
}
