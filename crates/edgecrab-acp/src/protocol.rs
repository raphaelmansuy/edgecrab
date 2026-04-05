//! # ACP Protocol types — JSON-RPC 2.0 message structures
//!
//! WHY protocol types: The ACP (Agent Communication Protocol) uses JSON-RPC 2.0
//! over stdio. These types define the wire format for requests, responses, and
//! session update notifications.
//!
//! ```text
//! ┌──────────┐   JSON-RPC/stdio   ┌──────────┐
//! │  Editor  │ ◄─────────────────► │ ACP Srv  │
//! │ (VS Code │   initialize        │ (Edge-   │
//! │  / Zed / │   new_session       │  Crab)   │
//! │  JBrains)│   prompt            │          │
//! └──────────┘   session_update    └──────────┘
//! ```

use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 protocol version constant.
pub const JSONRPC_VERSION: &str = "2.0";

/// ACP protocol version we implement.
pub const ACP_PROTOCOL_VERSION: u32 = 1;

// ── JSON-RPC envelope ────────────────────────────────────────────────────

/// A JSON-RPC 2.0 request (method call from editor → server).
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// A JSON-RPC 2.0 response (server → editor).
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 notification (server → editor, no response expected).
#[derive(Debug, Serialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
}

/// JSON-RPC error object.
#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    pub fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<serde_json::Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params,
        }
    }
}

// ── Standard JSON-RPC error codes ────────────────────────────────────────

pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INTERNAL_ERROR: i32 = -32603;

// ── ACP-specific types ──────────────────────────────────────────────────

/// Response to `initialize` — advertises agent capabilities.
#[derive(Debug, Serialize)]
pub struct InitializeResult {
    pub protocol_version: u32,
    pub agent_info: AgentInfo,
    pub agent_capabilities: AgentCapabilities,
}

/// Agent identification.
#[derive(Debug, Serialize)]
pub struct AgentInfo {
    pub name: String,
    pub version: String,
}

/// What the agent supports.
#[derive(Debug, Serialize)]
pub struct AgentCapabilities {
    pub session_capabilities: SessionCapabilities,
}

/// Session management capabilities.
#[derive(Debug, Serialize)]
pub struct SessionCapabilities {
    pub fork: bool,
    pub list: bool,
}

/// Response to `new_session`.
#[derive(Debug, Serialize)]
pub struct NewSessionResult {
    pub session_id: String,
}

/// Response to `load_session` / `resume_session`.
#[derive(Debug, Serialize)]
pub struct SessionResult {
    pub session_id: String,
}

/// A session entry in `list_sessions` response.
#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub cwd: String,
    pub model: String,
    pub history_len: usize,
}

/// Response to `list_sessions`.
#[derive(Debug, Serialize)]
pub struct ListSessionsResult {
    pub sessions: Vec<SessionInfo>,
}

/// Response to `prompt`.
#[derive(Debug, Serialize)]
pub struct PromptResult {
    pub stop_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageInfo>,
}

/// Token usage information.
#[derive(Debug, Serialize)]
pub struct UsageInfo {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Content block in a prompt request (text-only for now).
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
}

/// Params for the `prompt` method.
#[derive(Debug, Deserialize)]
pub struct PromptParams {
    pub session_id: String,
    pub prompt: Vec<ContentBlock>,
}

/// Params for `new_session`.
#[derive(Debug, Deserialize)]
pub struct NewSessionParams {
    #[serde(default = "default_cwd")]
    pub cwd: String,
}

fn default_cwd() -> String {
    ".".to_string()
}

/// Params for `load_session` / `resume_session` / `cancel` / `fork_session`.
#[derive(Debug, Deserialize)]
pub struct SessionIdParams {
    pub session_id: String,
    #[serde(default = "default_cwd")]
    pub cwd: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonrpc_response_success_serializes() {
        let resp =
            JsonRpcResponse::success(Some(serde_json::json!(1)), serde_json::json!({"ok": true}));
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"ok\":true"));
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn jsonrpc_response_error_serializes() {
        let resp =
            JsonRpcResponse::error(Some(serde_json::json!(2)), METHOD_NOT_FOUND, "not found");
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(json.contains("\"-32601\"") || json.contains("-32601"));
        assert!(json.contains("not found"));
        assert!(!json.contains("\"result\""));
    }

    #[test]
    fn content_block_deserializes() {
        let json = r#"{"type":"text","text":"hello"}"#;
        let block: ContentBlock = serde_json::from_str(json).expect("deserialize");
        match block {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
        }
    }

    #[test]
    fn prompt_params_deserializes() {
        let json = r#"{"session_id":"abc","prompt":[{"type":"text","text":"hi"}]}"#;
        let params: PromptParams = serde_json::from_str(json).expect("deserialize");
        assert_eq!(params.session_id, "abc");
        assert_eq!(params.prompt.len(), 1);
    }

    #[test]
    fn new_session_params_defaults_cwd() {
        let json = r#"{}"#;
        let params: NewSessionParams = serde_json::from_str(json).expect("deserialize");
        assert_eq!(params.cwd, ".");
    }
}
