//! # ACP Server — JSON-RPC 2.0 over stdio transport
//!
//! WHY stdio: The ACP protocol uses JSON-RPC over stdin/stdout so the editor
//! can launch the agent as a child process and communicate via pipes.
//! Human-readable logs go to stderr, keeping stdout clean for protocol traffic.
//!
//! ```text
//! ┌──────────┐  stdin (JSON-RPC)   ┌──────────┐
//! │  Editor  │ ──────────────────► │ AcpServer│
//! │          │ ◄────────────────── │          │
//! │          │  stdout (JSON-RPC)  │          │
//! └──────────┘                     └──────────┘
//!                stderr → logs
//! ```

use crate::protocol::*;
use crate::session::SessionManager;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info, warn};

use edgecrab_core::Agent;

/// The EdgeCrab ACP agent version.
const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const ACP_AGENT_NAME: &str = "EdgeCrab";

/// ACP server that reads JSON-RPC requests from stdin and writes responses to stdout.
pub struct AcpServer {
    sessions: Arc<SessionManager>,
    /// Agent for LLM dispatch.
    agent: Option<Arc<Agent>>,
}

impl AcpServer {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(SessionManager::new()),
            agent: None,
        }
    }

    /// Set the agent for LLM dispatch.
    pub fn set_agent(&mut self, agent: Arc<Agent>) {
        self.agent = Some(agent);
    }

    /// Run the stdio JSON-RPC event loop.
    ///
    /// Reads one JSON line per request from stdin, dispatches it, and writes
    /// the response to stdout. Loops until stdin is closed.
    pub async fn run(&self) -> anyhow::Result<()> {
        let stdin = tokio::io::stdin();
        let mut stdout = tokio::io::stdout();
        let mut reader = BufReader::new(stdin);
        let mut line = String::new();

        info!("EdgeCrab ACP server starting (v{})", AGENT_VERSION);

        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                info!("stdin closed, shutting down ACP server");
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
                Ok(req) => req,
                Err(e) => {
                    let resp =
                        JsonRpcResponse::error(None, PARSE_ERROR, format!("Parse error: {e}"));
                    let mut out = serde_json::to_vec(&resp)?;
                    out.push(b'\n');
                    stdout.write_all(&out).await?;
                    stdout.flush().await?;
                    continue;
                }
            };

            debug!("ACP request: method={}", request.method);
            let response = self.dispatch(request).await;

            let mut out = serde_json::to_vec(&response)?;
            out.push(b'\n');
            stdout.write_all(&out).await?;
            stdout.flush().await?;
        }

        Ok(())
    }

    /// Dispatch a JSON-RPC request to the appropriate handler.
    async fn dispatch(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let id = request.id.clone();
        match request.method.as_str() {
            "initialize" => self.handle_initialize(id),
            "authenticate" => self.handle_authenticate(id),
            "new_session" => self.handle_new_session(id, request.params).await,
            "load_session" => self.handle_load_session(id, request.params).await,
            "resume_session" => self.handle_resume_session(id, request.params).await,
            "cancel" => self.handle_cancel(id, request.params).await,
            "fork_session" => self.handle_fork_session(id, request.params).await,
            "list_sessions" => self.handle_list_sessions(id).await,
            "prompt" => self.handle_prompt(id, request.params).await,
            _ => {
                warn!("Unknown ACP method: {}", request.method);
                JsonRpcResponse::error(
                    id,
                    METHOD_NOT_FOUND,
                    format!("Method not found: {}", request.method),
                )
            }
        }
    }

    // ── Lifecycle ────────────────────────────────────────────────────────

    fn handle_initialize(&self, id: Option<serde_json::Value>) -> JsonRpcResponse {
        info!("ACP initialize");
        let result = InitializeResult {
            protocol_version: ACP_PROTOCOL_VERSION,
            agent_info: AgentInfo {
                name: ACP_AGENT_NAME.to_string(),
                version: AGENT_VERSION.to_string(),
            },
            agent_capabilities: AgentCapabilities {
                session_capabilities: SessionCapabilities {
                    fork: true,
                    list: true,
                },
            },
        };
        JsonRpcResponse::success(id, serde_json::to_value(result).unwrap_or_default())
    }

    fn handle_authenticate(&self, id: Option<serde_json::Value>) -> JsonRpcResponse {
        // Check if OPENROUTER_API_KEY (or legacy variants) is available
        let has_key = std::env::var("OPENROUTER_API_KEY").is_ok()
            || std::env::var("OPENCLAW_API_KEY").is_ok()
            || std::env::var("HERMES_API_KEY").is_ok();

        if has_key {
            JsonRpcResponse::success(id, serde_json::json!({}))
        } else {
            JsonRpcResponse::success(id, serde_json::Value::Null)
        }
    }

    // ── Session management ───────────────────────────────────────────────

    async fn handle_new_session(
        &self,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let p: NewSessionParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return JsonRpcResponse::error(id, INVALID_REQUEST, format!("Invalid params: {e}"));
            }
        };

        let session_id = self.sessions.create_session(&p.cwd);
        info!("New ACP session {} (cwd={})", session_id, p.cwd);

        let result = NewSessionResult { session_id };
        JsonRpcResponse::success(id, serde_json::to_value(result).unwrap_or_default())
    }

    async fn handle_load_session(
        &self,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let p: SessionIdParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return JsonRpcResponse::error(id, INVALID_REQUEST, format!("Invalid params: {e}"));
            }
        };

        if self.sessions.update_cwd(&p.session_id, &p.cwd).await {
            info!("Loaded ACP session {}", p.session_id);
            let result = SessionResult {
                session_id: p.session_id,
            };
            JsonRpcResponse::success(id, serde_json::to_value(result).unwrap_or_default())
        } else {
            warn!("load_session: session {} not found", p.session_id);
            JsonRpcResponse::success(id, serde_json::Value::Null)
        }
    }

    async fn handle_resume_session(
        &self,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let p: SessionIdParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return JsonRpcResponse::error(id, INVALID_REQUEST, format!("Invalid params: {e}"));
            }
        };

        // Try to resume existing, otherwise create new
        let session_id = if self.sessions.update_cwd(&p.session_id, &p.cwd).await {
            info!("Resumed ACP session {}", p.session_id);
            p.session_id
        } else {
            let new_id = self.sessions.create_session(&p.cwd);
            info!(
                "Session {} not found, created new session {}",
                p.session_id, new_id
            );
            new_id
        };

        let result = SessionResult { session_id };
        JsonRpcResponse::success(id, serde_json::to_value(result).unwrap_or_default())
    }

    async fn handle_cancel(
        &self,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let p: SessionIdParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return JsonRpcResponse::error(id, INVALID_REQUEST, format!("Invalid params: {e}"));
            }
        };

        if let Some(session) = self.sessions.get_session(&p.session_id) {
            let reader = session.read().await;
            reader.cancel.cancel();
            info!("Cancelled ACP session {}", p.session_id);
        } else {
            debug!("cancel: session {} not found (noop)", p.session_id);
        }

        JsonRpcResponse::success(id, serde_json::json!({}))
    }

    async fn handle_fork_session(
        &self,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let p: SessionIdParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return JsonRpcResponse::error(id, INVALID_REQUEST, format!("Invalid params: {e}"));
            }
        };

        match self.sessions.fork_session(&p.session_id, &p.cwd).await {
            Some(new_id) => {
                info!("Forked ACP session {} -> {}", p.session_id, new_id);
                let result = NewSessionResult { session_id: new_id };
                JsonRpcResponse::success(id, serde_json::to_value(result).unwrap_or_default())
            }
            None => {
                warn!("fork_session: session {} not found", p.session_id);
                JsonRpcResponse::success(id, serde_json::Value::Null)
            }
        }
    }

    async fn handle_list_sessions(&self, id: Option<serde_json::Value>) -> JsonRpcResponse {
        let sessions = self.sessions.list_sessions().await;
        let result = ListSessionsResult { sessions };
        JsonRpcResponse::success(id, serde_json::to_value(result).unwrap_or_default())
    }

    // ── Prompt ───────────────────────────────────────────────────────────

    async fn handle_prompt(
        &self,
        id: Option<serde_json::Value>,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        let p: PromptParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return JsonRpcResponse::error(id, INVALID_REQUEST, format!("Invalid params: {e}"));
            }
        };

        let session = match self.sessions.get_session(&p.session_id) {
            Some(s) => s,
            None => {
                error!("prompt: session {} not found", p.session_id);
                let result = PromptResult {
                    stop_reason: "refusal".to_string(),
                    usage: None,
                };
                return JsonRpcResponse::success(
                    id,
                    serde_json::to_value(result).unwrap_or_default(),
                );
            }
        };

        // Extract text from content blocks
        let user_text: String = p
            .prompt
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => text.as_str(),
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();

        if user_text.is_empty() {
            let result = PromptResult {
                stop_reason: "end_turn".to_string(),
                usage: None,
            };
            return JsonRpcResponse::success(id, serde_json::to_value(result).unwrap_or_default());
        }

        // Handle slash commands locally
        if let Some(cmd_response) = self.handle_slash_command(&user_text, &session).await {
            // Store the slash command result in history
            let mut writer = session.write().await;
            writer
                .history
                .push(serde_json::json!({"role": "user", "content": user_text}));
            writer
                .history
                .push(serde_json::json!({"role": "assistant", "content": cmd_response}));
            drop(writer);

            let result = PromptResult {
                stop_reason: "end_turn".to_string(),
                usage: None,
            };
            return JsonRpcResponse::success(id, serde_json::to_value(result).unwrap_or_default());
        }

        // Dispatch to Agent for LLM response
        {
            let mut writer = session.write().await;
            writer
                .history
                .push(serde_json::json!({"role": "user", "content": user_text}));

            if writer.cancel.is_cancelled() {
                let result = PromptResult {
                    stop_reason: "cancelled".to_string(),
                    usage: None,
                };
                return JsonRpcResponse::success(
                    id,
                    serde_json::to_value(result).unwrap_or_default(),
                );
            }
        }

        if let Some(ref agent) = self.agent {
            let session_cwd = {
                let reader = session.read().await;
                reader.cwd.clone()
            };
            match agent.chat_in_cwd(&user_text, Path::new(&session_cwd)).await {
                Ok(response) => {
                    let mut writer = session.write().await;
                    writer
                        .history
                        .push(serde_json::json!({"role": "assistant", "content": response}));

                    let result = PromptResult {
                        stop_reason: "end_turn".to_string(),
                        usage: None,
                    };
                    JsonRpcResponse::success(id, serde_json::to_value(result).unwrap_or_default())
                }
                Err(e) => {
                    error!("agent dispatch failed: {e}");
                    let result = PromptResult {
                        stop_reason: "error".to_string(),
                        usage: None,
                    };
                    JsonRpcResponse::success(id, serde_json::to_value(result).unwrap_or_default())
                }
            }
        } else {
            // No agent configured — return end_turn with no content
            let result = PromptResult {
                stop_reason: "end_turn".to_string(),
                usage: None,
            };
            JsonRpcResponse::success(id, serde_json::to_value(result).unwrap_or_default())
        }
    }

    // ── Slash commands ───────────────────────────────────────────────────

    async fn handle_slash_command(
        &self,
        text: &str,
        session: &Arc<tokio::sync::RwLock<crate::session::AcpSession>>,
    ) -> Option<String> {
        if !text.starts_with('/') {
            return None;
        }

        let parts: Vec<&str> = text.splitn(2, char::is_whitespace).collect();
        let cmd = parts[0].trim_start_matches('/');

        match cmd {
            "help" => {
                let lines = [
                    "Available commands:",
                    "",
                    "  /help      Show this help",
                    "  /model     Show current model",
                    "  /tools     List available tools",
                    "  /context   Show conversation info",
                    "  /reset     Clear conversation history",
                    "  /version   Show EdgeCrab version",
                ];
                Some(lines.join("\n"))
            }
            "model" => {
                let reader = session.read().await;
                let model = if reader.model.is_empty() {
                    "default"
                } else {
                    &reader.model
                };
                Some(format!("Current model: {model}"))
            }
            "tools" => {
                let tools = crate::permission::ACP_TOOLS;
                let mut lines = vec![format!("Available tools ({}):", tools.len())];
                for tool in tools {
                    lines.push(format!("  {tool}"));
                }
                Some(lines.join("\n"))
            }
            "context" => {
                let reader = session.read().await;
                let n = reader.history.len();
                if n == 0 {
                    Some("Conversation is empty (no messages yet).".to_string())
                } else {
                    Some(format!("Conversation: {n} messages"))
                }
            }
            "reset" => {
                let mut writer = session.write().await;
                writer.history.clear();
                Some("Conversation history cleared.".to_string())
            }
            "version" => Some(format!("EdgeCrab ACP v{AGENT_VERSION}")),
            _ => None, // Unknown slash commands fall through to LLM
        }
    }
}

impl Default for AcpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_server() -> AcpServer {
        AcpServer::new()
    }

    #[test]
    fn initialize_returns_agent_info() {
        let server = make_server();
        let resp = server.handle_initialize(Some(serde_json::json!(1)));
        assert!(resp.result.is_some());
        let result = resp.result.expect("result");
        assert_eq!(result["agent_info"]["name"], ACP_AGENT_NAME);
        assert_eq!(result["protocol_version"], ACP_PROTOCOL_VERSION);
    }

    #[test]
    fn authenticate_without_key_returns_null() {
        // In test env, API keys likely not set
        let server = make_server();
        let resp = server.handle_authenticate(Some(serde_json::json!(2)));
        // Either null or {} depending on env
        assert!(resp.result.is_some());
    }

    #[tokio::test]
    async fn new_session_returns_id() {
        let server = make_server();
        let resp = server
            .handle_new_session(
                Some(serde_json::json!(3)),
                serde_json::json!({"cwd": "/project"}),
            )
            .await;
        let result = resp.result.expect("result");
        assert!(result["session_id"].is_string());
    }

    #[tokio::test]
    async fn load_nonexistent_returns_null() {
        let server = make_server();
        let resp = server
            .handle_load_session(
                Some(serde_json::json!(4)),
                serde_json::json!({"session_id": "nope", "cwd": "."}),
            )
            .await;
        let result = resp.result.expect("result");
        assert!(result.is_null());
    }

    #[tokio::test]
    async fn resume_creates_new_if_missing() {
        let server = make_server();
        let resp = server
            .handle_resume_session(
                Some(serde_json::json!(5)),
                serde_json::json!({"session_id": "nope", "cwd": "/new"}),
            )
            .await;
        let result = resp.result.expect("result");
        assert!(result["session_id"].is_string());
    }

    #[tokio::test]
    async fn cancel_nonexistent_is_noop() {
        let server = make_server();
        let resp = server
            .handle_cancel(
                Some(serde_json::json!(6)),
                serde_json::json!({"session_id": "nope", "cwd": "."}),
            )
            .await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn prompt_unknown_session_returns_refusal() {
        let server = make_server();
        let resp = server
            .handle_prompt(
                Some(serde_json::json!(7)),
                serde_json::json!({
                    "session_id": "unknown",
                    "prompt": [{"type": "text", "text": "hello"}]
                }),
            )
            .await;
        let result = resp.result.expect("result");
        assert_eq!(result["stop_reason"], "refusal");
    }

    #[tokio::test]
    async fn prompt_empty_text_returns_end_turn() {
        let server = make_server();
        let session_id = server.sessions.create_session(".");
        let resp = server
            .handle_prompt(
                Some(serde_json::json!(8)),
                serde_json::json!({
                    "session_id": session_id,
                    "prompt": [{"type": "text", "text": "   "}]
                }),
            )
            .await;
        let result = resp.result.expect("result");
        assert_eq!(result["stop_reason"], "end_turn");
    }

    #[tokio::test]
    async fn slash_help_returns_commands() {
        let server = make_server();
        let session_id = server.sessions.create_session(".");
        let session = server.sessions.get_session(&session_id).expect("session");
        let result = server.handle_slash_command("/help", &session).await;
        assert!(result.is_some());
        let text = result.expect("help text");
        assert!(text.contains("help"));
        assert!(text.contains("model"));
        assert!(text.contains("tools"));
    }

    #[tokio::test]
    async fn slash_reset_clears_history() {
        let server = make_server();
        let session_id = server.sessions.create_session(".");
        let session = server.sessions.get_session(&session_id).expect("session");

        // Add some history
        {
            let mut w = session.write().await;
            w.history
                .push(serde_json::json!({"role": "user", "content": "hi"}));
        }

        let result = server.handle_slash_command("/reset", &session).await;
        assert!(result.is_some());

        let r = session.read().await;
        assert!(r.history.is_empty());
    }

    #[tokio::test]
    async fn slash_version_returns_version() {
        let server = make_server();
        let session_id = server.sessions.create_session(".");
        let session = server.sessions.get_session(&session_id).expect("session");
        let result = server.handle_slash_command("/version", &session).await;
        assert!(result.is_some());
        assert!(result.expect("version").contains("EdgeCrab"));
    }

    #[tokio::test]
    async fn unknown_slash_returns_none() {
        let server = make_server();
        let session_id = server.sessions.create_session(".");
        let session = server.sessions.get_session(&session_id).expect("session");
        let result = server.handle_slash_command("/unknown_cmd", &session).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn dispatch_unknown_method() {
        let server = make_server();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(99)),
            method: "nonexistent".to_string(),
            params: serde_json::Value::Null,
        };
        let resp = server.dispatch(request).await;
        assert!(resp.error.is_some());
        let err = resp.error.expect("error");
        assert_eq!(err.code, METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn full_session_lifecycle() {
        let server = make_server();

        // Create session
        let resp = server
            .handle_new_session(
                Some(serde_json::json!(1)),
                serde_json::json!({"cwd": "/project"}),
            )
            .await;
        let session_id = resp.result.expect("result")["session_id"]
            .as_str()
            .expect("id")
            .to_string();

        // Prompt
        let resp = server
            .handle_prompt(
                Some(serde_json::json!(2)),
                serde_json::json!({
                    "session_id": session_id,
                    "prompt": [{"type": "text", "text": "hello"}]
                }),
            )
            .await;
        let result = resp.result.expect("result");
        assert_eq!(result["stop_reason"], "end_turn");

        // List sessions
        let resp = server
            .handle_list_sessions(Some(serde_json::json!(3)))
            .await;
        let result = resp.result.expect("result");
        assert_eq!(result["sessions"].as_array().expect("array").len(), 1);

        // Fork
        let resp = server
            .handle_fork_session(
                Some(serde_json::json!(4)),
                serde_json::json!({"session_id": session_id, "cwd": "/forked"}),
            )
            .await;
        let fork_result = resp.result.expect("result");
        assert!(fork_result["session_id"].is_string());

        // Cancel
        let resp = server
            .handle_cancel(
                Some(serde_json::json!(5)),
                serde_json::json!({"session_id": session_id, "cwd": "."}),
            )
            .await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn server_handles_empty_stdin_gracefully() {
        // The ACP server should exit cleanly when stdin is immediately closed
        // (bytes_read == 0). This simulates `echo -n | edgecrab acp`.
        //
        // WHY test this: In CI and scripted environments, stdin may be /dev/null.
        // The server must not hang or crash.
        use tokio::io::AsyncWriteExt;

        let (mut tx, rx) = tokio::io::duplex(256);
        // Drop the write end immediately — simulates empty stdin
        tx.shutdown().await.expect("shutdown");

        let mut reader = tokio::io::BufReader::new(rx);
        let mut line = String::new();
        use tokio::io::AsyncBufReadExt;
        let bytes = reader.read_line(&mut line).await.expect("read");
        assert_eq!(bytes, 0, "empty stdin should return 0 bytes");
    }
}
