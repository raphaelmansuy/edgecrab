//! # mcp_client — Model Context Protocol client tools
//!
//! WHY MCP: The Model Context Protocol (MCP) allows agents to dynamically
//! discover and invoke tools served by external processes. This is the
//! Rust-side client that mirrors hermes-agent's MCP integration.
//!
//! ```text
//!   mcp_list_tools
//!       │
//!       └──→ McpClient::list_tools() → JSON-RPC 2.0 over stdio
//!                 │
//!                 └──→ { "method": "tools/list" } → parse response
//!
//!   mcp_call_tool("tool_name", { ...args })
//!       └──→ McpClient::call_tool() → JSON-RPC 2.0 over stdio
//!                 │
//!                 └──→ { "method": "tools/call", "params": { "name": ..., "arguments": ... } }
//! ```
//!
//! MCP connections are stored in a static `DashMap` keyed by server name
//! so that multiple tool calls reuse the same subprocess.

use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;

use crate::config_ref::resolve_edgecrab_home;
use crate::registry::{ToolContext, ToolHandler};

// ─── HTTP MCP token storage ──────────────────────────────────────────────────

/// Directory under ~/.edgecrab where MCP OAuth tokens are persisted.
const MCP_TOKENS_DIR: &str = "mcp-tokens";
const OAUTH_EXPIRY_SKEW_SECS: u64 = 60;

/// Sanitize a server name to a safe filename component.
fn sanitize_server_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let clean = clean.trim_matches('-').to_lowercase();
    if clean.is_empty() {
        "unnamed".to_string()
    } else {
        clean.chars().take(60).collect()
    }
}

fn mcp_tokens_dir() -> Option<std::path::PathBuf> {
    Some(resolve_edgecrab_home().join(MCP_TOKENS_DIR))
}

#[derive(Debug, Clone, Default, Deserialize, serde::Serialize)]
struct StoredMcpToken {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_at_epoch_secs: Option<u64>,
}

fn token_file_path(server_name: &str) -> Option<std::path::PathBuf> {
    mcp_tokens_dir().map(|dir| dir.join(format!("{}.json", sanitize_server_name(server_name))))
}

fn read_mcp_token_record(server_name: &str) -> Option<StoredMcpToken> {
    let file = token_file_path(server_name)?;
    if !file.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(&file).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_mcp_token_record(server_name: &str, token: &StoredMcpToken) -> std::io::Result<()> {
    let dir = mcp_tokens_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Cannot determine home directory",
        )
    })?;
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!("{}.json", sanitize_server_name(server_name)));
    let payload = serde_json::to_vec(token)?;
    std::fs::write(&file, payload)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn token_is_fresh(token: &StoredMcpToken) -> bool {
    token
        .expires_at_epoch_secs
        .is_none_or(|expiry| expiry > current_epoch_secs() + OAUTH_EXPIRY_SKEW_SECS)
}

/// Read a Bearer token from the token store for a given server.
///
/// Token file format: `{ "access_token": "...", "token_type": "Bearer" }`
pub fn read_mcp_token(server_name: &str) -> Option<String> {
    read_mcp_token_record(server_name).map(|token| token.access_token)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McpStoredTokenStatus {
    pub has_access_token: bool,
    pub has_refresh_token: bool,
    pub expires_at_epoch_secs: Option<u64>,
}

pub fn read_mcp_token_status(server_name: &str) -> Option<McpStoredTokenStatus> {
    let token = read_mcp_token_record(server_name)?;
    Some(McpStoredTokenStatus {
        has_access_token: !token.access_token.trim().is_empty(),
        has_refresh_token: token
            .refresh_token
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()),
        expires_at_epoch_secs: token.expires_at_epoch_secs,
    })
}

/// Persist a Bearer token to the token store for a given server.
pub fn write_mcp_token(server_name: &str, token: &str) -> std::io::Result<()> {
    let existing = read_mcp_token_record(server_name).unwrap_or_default();
    write_mcp_token_record(
        server_name,
        &StoredMcpToken {
            access_token: token.to_string(),
            token_type: existing.token_type.or(Some("Bearer".into())),
            refresh_token: existing.refresh_token,
            expires_at_epoch_secs: None,
        },
    )
}

/// Persist a refresh token to the token store while preserving any access token.
pub fn write_mcp_refresh_token(server_name: &str, refresh_token: &str) -> std::io::Result<()> {
    let mut existing = read_mcp_token_record(server_name).unwrap_or_default();
    existing.refresh_token = Some(refresh_token.to_string());
    if existing.token_type.is_none() {
        existing.token_type = Some("Bearer".into());
    }
    write_mcp_token_record(server_name, &existing)
}

/// Remove stored OAuth tokens for a given server.
pub fn remove_mcp_token(server_name: &str) {
    if let Some(dir) = mcp_tokens_dir() {
        let _ =
            std::fs::remove_file(dir.join(format!("{}.json", sanitize_server_name(server_name))));
    }
}

// ─── HTTP MCP connection ─────────────────────────────────────────────────────

/// An MCP connection backed by HTTP POST (JSON-RPC over HTTP).
///
/// Supports HTTP MCP servers such as those running Streamable HTTP transport
/// (formerly SSE transport). Sends requests as JSON-RPC 2.0 POST bodies and
/// reads the response body directly.
///
/// Authentication: Bearer token injected from config or ~/.edgecrab/mcp-tokens/.
/// Custom headers: any additional headers from the `headers` config map are
/// forwarded verbatim, allowing custom auth schemes.
struct HttpMcpConnection {
    server_name: String,
    url: String,
    auth: HttpAuthState,
    /// Extra headers sent with every request (e.g. `X-Custom-Auth`).
    headers: std::collections::HashMap<String, String>,
    client: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct OAuthConfig {
    token_url: String,
    grant_type: OAuthGrantType,
    client_id: Option<String>,
    client_secret: Option<String>,
    auth_method: OAuthClientAuthMethod,
    scopes: Vec<String>,
    audience: Option<String>,
    resource: Option<String>,
    refresh_token: Option<String>,
    extra_params: HashMap<String, String>,
}

impl OAuthConfig {
    pub fn token_url(&self) -> &str {
        &self.token_url
    }

    pub fn grant_type_label(&self) -> &'static str {
        match self.grant_type {
            OAuthGrantType::Auto => "auto",
            OAuthGrantType::ClientCredentials => "client_credentials",
            OAuthGrantType::RefreshToken => "refresh_token",
        }
    }

    pub fn auth_method_label(&self) -> &'static str {
        match self.auth_method {
            OAuthClientAuthMethod::ClientSecretPost => "client_secret_post",
            OAuthClientAuthMethod::ClientSecretBasic => "client_secret_basic",
            OAuthClientAuthMethod::None => "none",
        }
    }

    pub fn has_client_id(&self) -> bool {
        self.client_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    }

    pub fn has_client_secret(&self) -> bool {
        self.client_secret
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    }

    pub fn has_refresh_token(&self) -> bool {
        self.refresh_token
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OAuthGrantType {
    Auto,
    ClientCredentials,
    RefreshToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OAuthClientAuthMethod {
    ClientSecretPost,
    ClientSecretBasic,
    None,
}

#[derive(Debug, Clone)]
enum HttpAuthState {
    None,
    StaticBearer(Option<String>),
    OAuth {
        config: Box<OAuthConfig>,
        token: Option<StoredMcpToken>,
    },
}

impl HttpAuthState {
    fn can_refresh(&self) -> bool {
        matches!(self, Self::OAuth { .. })
    }

    fn invalidate_access_token(&mut self) {
        if let Self::OAuth {
            token: Some(token), ..
        } = self
        {
            token.expires_at_epoch_secs = Some(0);
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<serde_json::Value>,
}

impl HttpMcpConnection {
    /// Create an HTTP connection and verify connectivity with an initialize call.
    async fn connect(
        server_name: &str,
        url: &str,
        auth: HttpAuthState,
        headers: std::collections::HashMap<String, String>,
        timeout_secs: u64,
        connect_timeout_secs: u64,
    ) -> Result<Self, ToolError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .connect_timeout(std::time::Duration::from_secs(connect_timeout_secs))
            .build()
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "mcp_client".into(),
                message: format!("Failed to create HTTP client: {e}"),
            })?;

        let conn = Self {
            server_name: server_name.to_string(),
            url: url.to_string(),
            auth,
            headers,
            client,
        };

        // Perform JSON-RPC initialize handshake
        let init_req = json!({
            "jsonrpc": "2.0",
            "id": next_request_id(),
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "edgecrab",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        });
        let mut conn = conn;
        conn.post_rpc(init_req).await?;

        Ok(conn)
    }

    fn request_builder(
        &self,
        body: serde_json::Value,
        bearer_token: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let mut req = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .json(&body);
        if let Some(token) = bearer_token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        // Apply extra headers (may override Authorization if user sets it explicitly)
        for (k, v) in &self.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        req
    }

    async fn post_rpc(&mut self, body: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        match self.post_rpc_once(body.clone()).await {
            Ok(value) => Ok(value),
            Err(err)
                if matches!(&err, ToolError::ExecutionFailed { message, .. } if message.contains("status 401"))
                    && self.auth.can_refresh() =>
            {
                self.auth.invalidate_access_token();
                self.post_rpc_once(body).await
            }
            Err(err) => Err(err),
        }
    }

    async fn post_rpc_once(
        &mut self,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, ToolError> {
        let bearer_token = self.ensure_bearer_token().await?;
        let resp = self
            .request_builder(body, bearer_token.as_deref())
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "mcp_client".into(),
                message: format!("HTTP MCP request failed: {e}"),
            })?;

        let status = resp.status();
        if !status.is_success() {
            return Err(ToolError::ExecutionFailed {
                tool: "mcp_client".into(),
                message: format!("HTTP MCP server returned status {status}"),
            });
        }

        let val: serde_json::Value = resp.json().await.map_err(|e| ToolError::ExecutionFailed {
            tool: "mcp_client".into(),
            message: format!("Invalid JSON from HTTP MCP server: {e}"),
        })?;

        if let Some(err) = val.get("error") {
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown MCP error");
            return Err(ToolError::ExecutionFailed {
                tool: "mcp_client".into(),
                message: format!("MCP RPC error: {msg}"),
            });
        }

        Ok(val.get("result").cloned().unwrap_or(json!(null)))
    }

    async fn rpc_call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ToolError> {
        let body = json!({
            "jsonrpc": "2.0",
            "id": next_request_id(),
            "method": method,
            "params": params
        });
        self.post_rpc(body).await
    }

    async fn ensure_bearer_token(&mut self) -> Result<Option<String>, ToolError> {
        match &mut self.auth {
            HttpAuthState::None => Ok(None),
            HttpAuthState::StaticBearer(token) => Ok(token.clone()),
            HttpAuthState::OAuth { config, token } => {
                if token.as_ref().is_some_and(token_is_fresh) {
                    return Ok(token.as_ref().map(|token| token.access_token.clone()));
                }

                let refreshed =
                    fetch_oauth_token(&self.client, &self.server_name, config, token.as_ref())
                        .await?;
                let access_token = refreshed.access_token.clone();
                *token = Some(refreshed);
                Ok(Some(access_token))
            }
        }
    }
}

fn parse_expires_in_secs(value: &serde_json::Value) -> Option<u64> {
    if let Some(secs) = value.as_u64() {
        return Some(secs);
    }
    value.as_str()?.trim().parse().ok()
}

async fn fetch_oauth_token(
    client: &reqwest::Client,
    server_name: &str,
    config: &OAuthConfig,
    cached_token: Option<&StoredMcpToken>,
) -> Result<StoredMcpToken, ToolError> {
    let refresh_token = cached_token
        .and_then(|token| token.refresh_token.clone())
        .or_else(|| config.refresh_token.clone());

    let grant = match config.grant_type {
        OAuthGrantType::Auto => {
            if refresh_token.is_some() {
                OAuthGrantType::RefreshToken
            } else {
                OAuthGrantType::ClientCredentials
            }
        }
        other => other,
    };

    let mut params: Vec<(String, String)> = config
        .extra_params
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();

    match grant {
        OAuthGrantType::ClientCredentials => {
            params.push(("grant_type".into(), "client_credentials".into()));
        }
        OAuthGrantType::RefreshToken => {
            let refresh_token = refresh_token.ok_or_else(|| ToolError::ExecutionFailed {
                tool: "mcp_client".into(),
                message: format!(
                    "OAuth refresh_token grant requested for server '{server_name}' but no refresh token is available"
                ),
            })?;
            params.push(("grant_type".into(), "refresh_token".into()));
            params.push(("refresh_token".into(), refresh_token));
        }
        OAuthGrantType::Auto => unreachable!("auto is resolved earlier"),
    }

    if !config.scopes.is_empty() {
        params.push(("scope".into(), config.scopes.join(" ")));
    }
    if let Some(audience) = &config.audience {
        params.push(("audience".into(), audience.clone()));
    }
    if let Some(resource) = &config.resource {
        params.push(("resource".into(), resource.clone()));
    }

    let mut request = client.post(&config.token_url);
    match config.auth_method {
        OAuthClientAuthMethod::ClientSecretPost => {
            if let Some(client_id) = &config.client_id {
                params.push(("client_id".into(), client_id.clone()));
            }
            if let Some(client_secret) = &config.client_secret {
                params.push(("client_secret".into(), client_secret.clone()));
            }
        }
        OAuthClientAuthMethod::ClientSecretBasic => {
            request = request.basic_auth(
                config.client_id.clone().unwrap_or_default(),
                config.client_secret.clone(),
            );
        }
        OAuthClientAuthMethod::None => {
            if let Some(client_id) = &config.client_id {
                params.push(("client_id".into(), client_id.clone()));
            }
        }
    }

    let response = request
        .form(&params)
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "mcp_client".into(),
            message: format!("OAuth token request failed for server '{server_name}': {e}"),
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(ToolError::ExecutionFailed {
            tool: "mcp_client".into(),
            message: format!(
                "OAuth token endpoint returned status {status} for server '{server_name}'"
            ),
        });
    }

    let token: OAuthTokenResponse =
        response
            .json()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "mcp_client".into(),
                message: format!(
                    "OAuth token endpoint returned invalid JSON for server '{server_name}': {e}"
                ),
            })?;
    if token.access_token.trim().is_empty() {
        return Err(ToolError::ExecutionFailed {
            tool: "mcp_client".into(),
            message: format!(
                "OAuth token endpoint returned an empty access token for server '{server_name}'"
            ),
        });
    }

    let stored = StoredMcpToken {
        access_token: token.access_token,
        token_type: token.token_type.or(Some("Bearer".into())),
        refresh_token: token
            .refresh_token
            .or_else(|| cached_token.and_then(|token| token.refresh_token.clone()))
            .or_else(|| config.refresh_token.clone()),
        expires_at_epoch_secs: token
            .expires_in
            .as_ref()
            .and_then(parse_expires_in_secs)
            .map(|secs| current_epoch_secs() + secs),
    };

    write_mcp_token_record(server_name, &stored).map_err(|e| ToolError::ExecutionFailed {
        tool: "mcp_client".into(),
        message: format!("Failed to persist OAuth token for server '{server_name}': {e}"),
    })?;

    Ok(stored)
}

// ─── Unified connection enum ─────────────────────────────────────────────────

/// Either a stdio subprocess connection or an HTTP connection to an MCP server.
enum McpConnectionKind {
    Stdio(Box<McpConnection>),
    Http(Box<HttpMcpConnection>),
}

impl McpConnectionKind {
    async fn rpc_call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ToolError> {
        match self {
            McpConnectionKind::Stdio(c) => c.rpc_call(method, params).await,
            McpConnectionKind::Http(c) => c.rpc_call(method, params).await,
        }
    }
}

use edgecrab_types::{ToolError, ToolSchema};

// ─── MCP connection pool ────────────────────────────────────────

/// Global connection pool for MCP server connections (stdio or HTTP).
///
/// WHY DashMap: Multiple tool calls may arrive concurrently from parallel
/// tool execution. DashMap provides lock-free concurrent reads and
/// fine-grained write locks per shard.
static MCP_CONNECTIONS: OnceLock<DashMap<String, Mutex<McpConnectionKind>>> = OnceLock::new();

fn connections() -> &'static DashMap<String, Mutex<McpConnectionKind>> {
    MCP_CONNECTIONS.get_or_init(DashMap::new)
}

/// Monotonically increasing JSON-RPC request ID.
static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

fn next_request_id() -> u64 {
    REQUEST_ID.fetch_add(1, Ordering::Relaxed)
}

/// A live connection to an MCP server subprocess.
struct McpConnection {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl McpConnection {
    /// Spawn an MCP server subprocess and perform the initialization handshake.
    ///
    /// WHY envs param: MCP servers often need API keys injected via environment
    /// variables (e.g. `GITHUB_TOKEN`, `ANTHROPIC_API_KEY`). The config yaml
    /// `mcp_servers.<name>.env` map is now forwarded to the subprocess so
    /// tools don't silently fail due to missing credentials.
    async fn spawn(
        command: &str,
        args: &[String],
        cwd: Option<&std::path::Path>,
        envs: &std::collections::HashMap<String, String>,
    ) -> Result<Self, ToolError> {
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }
        if !envs.is_empty() {
            cmd.envs(envs);
        }
        let mut child = cmd.spawn().map_err(|e| ToolError::ExecutionFailed {
            tool: "mcp_client".into(),
            message: format!("Failed to spawn MCP server '{command}': {e}"),
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: "mcp_client".into(),
                message: "Failed to capture MCP server stdin".into(),
            })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: "mcp_client".into(),
                message: "Failed to capture MCP server stdout".into(),
            })?;

        let mut conn = Self {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
        };

        // Perform JSON-RPC initialize handshake
        let init_req = json!({
            "jsonrpc": "2.0",
            "id": next_request_id(),
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "edgecrab",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        });

        conn.send_request(&init_req).await?;
        conn.read_response().await?;

        // Send initialized notification
        let notif = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        conn.send_request(&notif).await?;

        Ok(conn)
    }

    /// Send a JSON-RPC request over stdin.
    async fn send_request(&mut self, request: &serde_json::Value) -> Result<(), ToolError> {
        let msg = serde_json::to_string(request).map_err(|e| ToolError::ExecutionFailed {
            tool: "mcp_client".into(),
            message: format!("JSON serialization error: {e}"),
        })?;

        self.stdin
            .write_all(msg.as_bytes())
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "mcp_client".into(),
                message: format!("Failed to write to MCP server stdin: {e}"),
            })?;

        self.stdin
            .write_all(b"\n")
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "mcp_client".into(),
                message: format!("Failed to write newline to MCP server stdin: {e}"),
            })?;

        self.stdin
            .flush()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "mcp_client".into(),
                message: format!("Failed to flush MCP server stdin: {e}"),
            })?;

        Ok(())
    }

    /// Read a single JSON-RPC response line from stdout.
    async fn read_response(&mut self) -> Result<serde_json::Value, ToolError> {
        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool: "mcp_client".into(),
                message: format!("Failed to read from MCP server stdout: {e}"),
            })?;

        if line.is_empty() {
            return Err(ToolError::ExecutionFailed {
                tool: "mcp_client".into(),
                message: "MCP server closed connection (empty response)".into(),
            });
        }

        serde_json::from_str(&line).map_err(|e| ToolError::ExecutionFailed {
            tool: "mcp_client".into(),
            message: format!("Invalid JSON from MCP server: {e} — raw: {line}"),
        })
    }

    /// Send a JSON-RPC request and read the response.
    async fn rpc_call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, ToolError> {
        let id = next_request_id();
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        self.send_request(&request).await?;
        let response = self.read_response().await?;

        // Check for JSON-RPC error
        if let Some(err) = response.get("error") {
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown MCP error");
            return Err(ToolError::ExecutionFailed {
                tool: "mcp_client".into(),
                message: format!("MCP RPC error: {msg}"),
            });
        }

        Ok(response.get("result").cloned().unwrap_or(json!(null)))
    }
}

/// Configuration for a single MCP server (unified stdio + HTTP).
struct McpServerConfig {
    /// HTTP URL for HTTP-based servers (takes precedence over command).
    url: Option<String>,
    /// Bearer token for HTTP servers (from config or token store).
    bearer_token: Option<String>,
    /// OAuth 2.0 config for HTTP servers.
    pub oauth: Option<OAuthConfig>,
    /// Extra HTTP headers for HTTP-based servers.
    headers: std::collections::HashMap<String, String>,
    /// Command for stdio-based servers.
    command: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    envs: std::collections::HashMap<String, String>,
    /// Per-call tool invocation timeout in seconds (default: 30).
    timeout: Option<u64>,
    /// Connection / handshake timeout in seconds (default: 10).
    connect_timeout: Option<u64>,
}

/// Get or create a connection to the named MCP server.
async fn get_or_connect(server_name: &str, cfg: McpServerConfig) -> Result<(), ToolError> {
    let pool = connections();
    if pool.contains_key(server_name) {
        return Ok(());
    }

    let timeout_secs = cfg.timeout.unwrap_or(30);
    let connect_timeout_secs = cfg.connect_timeout.unwrap_or(10);

    let kind = if let Some(ref url) = cfg.url {
        let auth = if let Some(oauth) = cfg.oauth.clone() {
            HttpAuthState::OAuth {
                config: Box::new(oauth),
                token: read_mcp_token_record(server_name),
            }
        } else if cfg.bearer_token.is_some() || read_mcp_token(server_name).is_some() {
            HttpAuthState::StaticBearer(
                cfg.bearer_token
                    .clone()
                    .or_else(|| read_mcp_token(server_name)),
            )
        } else {
            HttpAuthState::None
        };
        let http = HttpMcpConnection::connect(
            server_name,
            url,
            auth,
            cfg.headers,
            timeout_secs,
            connect_timeout_secs,
        )
        .await?;
        McpConnectionKind::Http(Box::new(http))
    } else {
        // Stdio subprocess MCP server
        let conn =
            McpConnection::spawn(&cfg.command, &cfg.args, cfg.cwd.as_deref(), &cfg.envs).await?;
        McpConnectionKind::Stdio(Box::new(conn))
    };

    pool.insert(server_name.to_string(), Mutex::new(kind));
    Ok(())
}

/// Legacy MCP config path used for compatibility imports.
fn mcp_config_path() -> Option<std::path::PathBuf> {
    Some(resolve_edgecrab_home().join("mcp.json"))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct YamlConfigFile {
    mcp_servers: std::collections::HashMap<String, YamlMcpServer>,
}

/// Tool include/exclude filter deserialized from config.yaml (mirrors
/// `McpToolsFilterConfig` in edgecrab-core).
#[derive(Debug, Deserialize)]
#[serde(default)]
struct YamlMcpToolsFilter {
    /// Whitelist — when non-empty, only these tool names are exposed.
    include: Vec<String>,
    /// Blacklist — these tool names are hidden (ignored when `include` is set).
    exclude: Vec<String>,
    /// Whether to register list_resources / read_resource wrappers (default: true).
    resources: bool,
    /// Whether to register list_prompts / get_prompt wrappers (default: true).
    prompts: bool,
}

impl Default for YamlMcpToolsFilter {
    fn default() -> Self {
        Self {
            include: Vec::new(),
            exclude: Vec::new(),
            resources: true,
            prompts: true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
struct YamlMcpServer {
    /// HTTP URL — when set, uses HTTP transport instead of stdio subprocess.
    url: Option<String>,
    /// Static Bearer token for HTTP servers (alternative to token store file).
    bearer_token: Option<String>,
    /// OAuth 2.0 token acquisition settings for HTTP servers.
    oauth: Option<YamlMcpOauth>,
    /// Extra HTTP headers for HTTP-based servers (e.g. custom auth schemes).
    headers: std::collections::HashMap<String, String>,
    command: String,
    args: Vec<String>,
    env: std::collections::HashMap<String, String>,
    cwd: Option<std::path::PathBuf>,
    enabled: bool,
    /// Per-call tool invocation timeout in seconds (default: 30).
    timeout: Option<u64>,
    /// Connection / handshake timeout in seconds (default: 10).
    connect_timeout: Option<u64>,
    /// Include/exclude filtering and capability wrapper toggles.
    tools: YamlMcpToolsFilter,
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(default)]
struct YamlMcpOauth {
    token_url: String,
    grant_type: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    auth_method: Option<String>,
    scopes: Vec<String>,
    audience: Option<String>,
    resource: Option<String>,
    refresh_token: Option<String>,
    extra_params: std::collections::HashMap<String, String>,
}

impl Default for YamlMcpServer {
    fn default() -> Self {
        Self {
            url: None,
            bearer_token: None,
            oauth: None,
            headers: std::collections::HashMap::new(),
            command: String::new(),
            args: Vec::new(),
            env: std::collections::HashMap::new(),
            cwd: None,
            enabled: true,
            timeout: None,
            connect_timeout: None,
            tools: YamlMcpToolsFilter::default(),
        }
    }
}

fn yaml_config_path() -> Option<std::path::PathBuf> {
    Some(resolve_edgecrab_home().join("config.yaml"))
}

fn expand_config_string(value: &str) -> String {
    shellexpand::env(value)
        .map(|expanded| expanded.into_owned())
        .unwrap_or_else(|_| value.to_string())
}

fn parse_expanded_string(value: Option<&serde_json::Value>) -> Option<String> {
    value.and_then(|v| v.as_str()).map(expand_config_string)
}

fn parse_expanded_path(value: Option<&serde_json::Value>) -> Option<PathBuf> {
    parse_expanded_string(value).map(PathBuf::from)
}

fn parse_string_array(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(expand_config_string))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_string_map(value: Option<&serde_json::Value>) -> HashMap<String, String> {
    value
        .and_then(|obj| obj.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), expand_config_string(s))))
                .collect()
        })
        .unwrap_or_default()
}

fn parse_oauth_grant_type(value: Option<&str>) -> OAuthGrantType {
    match value.unwrap_or("auto").trim().to_ascii_lowercase().as_str() {
        "client_credentials" | "client-credentials" => OAuthGrantType::ClientCredentials,
        "refresh_token" | "refresh-token" => OAuthGrantType::RefreshToken,
        _ => OAuthGrantType::Auto,
    }
}

fn parse_oauth_auth_method(value: Option<&str>) -> OAuthClientAuthMethod {
    match value
        .unwrap_or("client_secret_post")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "client_secret_basic" | "client-secret-basic" => OAuthClientAuthMethod::ClientSecretBasic,
        "none" => OAuthClientAuthMethod::None,
        _ => OAuthClientAuthMethod::ClientSecretPost,
    }
}

fn parse_oauth_config(value: Option<&serde_json::Value>) -> Option<OAuthConfig> {
    let oauth = value?.as_object()?;
    let token_url = oauth
        .get("token_url")
        .and_then(|value| value.as_str())
        .map(expand_config_string)
        .filter(|value| !value.trim().is_empty())?;

    Some(OAuthConfig {
        token_url,
        grant_type: parse_oauth_grant_type(
            oauth.get("grant_type").and_then(|value| value.as_str()),
        ),
        client_id: oauth
            .get("client_id")
            .and_then(|value| value.as_str())
            .map(expand_config_string),
        client_secret: oauth
            .get("client_secret")
            .and_then(|value| value.as_str())
            .map(expand_config_string),
        auth_method: parse_oauth_auth_method(
            oauth.get("auth_method").and_then(|value| value.as_str()),
        ),
        scopes: oauth
            .get("scopes")
            .and_then(|value| value.as_array())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(expand_config_string))
                    .collect()
            })
            .unwrap_or_default(),
        audience: oauth
            .get("audience")
            .and_then(|value| value.as_str())
            .map(expand_config_string),
        resource: oauth
            .get("resource")
            .and_then(|value| value.as_str())
            .map(expand_config_string),
        refresh_token: oauth
            .get("refresh_token")
            .and_then(|value| value.as_str())
            .map(expand_config_string),
        extra_params: oauth
            .get("extra_params")
            .and_then(|value| value.as_object())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|(key, value)| {
                        value
                            .as_str()
                            .map(|value| (key.clone(), expand_config_string(value)))
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn parse_configured_server(name: &str, server_config: &serde_json::Value) -> ConfiguredMcpServer {
    let token_from_store = read_mcp_token(name).is_some();
    ConfiguredMcpServer {
        name: name.to_string(),
        url: parse_expanded_string(server_config.get("url")),
        bearer_token: parse_expanded_string(server_config.get("bearer_token")),
        oauth: parse_oauth_config(server_config.get("oauth")),
        command: parse_expanded_string(server_config.get("command")).unwrap_or_default(),
        args: parse_string_array(server_config.get("args")),
        cwd: parse_expanded_path(server_config.get("cwd")),
        env: parse_string_map(server_config.get("env")),
        headers: parse_string_map(server_config.get("headers")),
        timeout: server_config.get("timeout").and_then(|t| t.as_u64()),
        connect_timeout: server_config
            .get("connect_timeout")
            .and_then(|t| t.as_u64()),
        include: parse_string_array(server_config.get("tools").and_then(|t| t.get("include"))),
        exclude: parse_string_array(server_config.get("tools").and_then(|t| t.get("exclude"))),
        token_from_config: server_config
            .get("bearer_token")
            .and_then(|t| t.as_str())
            .is_some(),
        token_from_store,
    }
}

fn to_runtime_server_config(server: &ConfiguredMcpServer) -> McpServerConfig {
    McpServerConfig {
        url: server.url.clone(),
        bearer_token: server.bearer_token.clone(),
        oauth: server.oauth.clone(),
        headers: server.headers.clone(),
        command: server.command.clone(),
        args: server.args.clone(),
        cwd: server.cwd.clone(),
        envs: server.env.clone(),
        timeout: server.timeout,
        connect_timeout: server.connect_timeout,
    }
}

// ─── Tool filtering ──────────────────────────────────────────────────────────

/// Apply include/exclude filtering to a list of MCP tool JSON objects.
///
/// Precedence rule: when both `include` and `exclude` are given, `include` wins
/// (only tools in the whitelist pass through regardless of the blacklist).
///
/// Returns filtered list preserving the original order.
fn apply_tool_filter(
    tools: &[serde_json::Value],
    include: &[String],
    exclude: &[String],
) -> Vec<serde_json::Value> {
    tools
        .iter()
        .filter(|t| {
            let name = t.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if !include.is_empty() {
                include.iter().any(|i| i == name)
            } else if !exclude.is_empty() {
                !exclude.iter().any(|e| e == name)
            } else {
                true
            }
        })
        .cloned()
        .collect()
}

/// Helper: extract the tools-filter include/exclude lists from a server config JSON value.
fn extract_tool_filter(server_config: &serde_json::Value) -> (Vec<String>, Vec<String>) {
    let tools_cfg = server_config.get("tools");
    let include: Vec<String> = tools_cfg
        .and_then(|t| t.get("include"))
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let exclude: Vec<String> = tools_cfg
        .and_then(|t| t.get("exclude"))
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    (include, exclude)
}

fn load_mcp_config() -> Result<serde_json::Value, ToolError> {
    if let Some(path) = yaml_config_path() {
        if path.is_file() {
            let content =
                std::fs::read_to_string(&path).map_err(|e| ToolError::ExecutionFailed {
                    tool: "mcp_client".into(),
                    message: format!("Failed to read config.yaml: {e}"),
                })?;
            let config: YamlConfigFile =
                serde_yml::from_str(&content).map_err(|e| ToolError::ExecutionFailed {
                    tool: "mcp_client".into(),
                    message: format!("Invalid config.yaml: {e}"),
                })?;

            if !config.mcp_servers.is_empty() {
                let mut servers = serde_json::Map::new();
                for (name, server) in config.mcp_servers {
                    if !server.enabled {
                        continue;
                    }
                    // HTTP server: url must be present
                    // Stdio server: command must be non-empty
                    if server.url.is_none() && server.command.trim().is_empty() {
                        continue;
                    }
                    servers.insert(
                        name,
                        json!({
                            "command": server.command,
                            "args": server.args,
                            "env": server.env,
                            "cwd": server.cwd,
                            "url": server.url,
                            "bearer_token": server.bearer_token,
                            "oauth": server.oauth,
                            "headers": server.headers,
                            "timeout": server.timeout,
                            "connect_timeout": server.connect_timeout,
                            "tools": {
                                "include": server.tools.include,
                                "exclude": server.tools.exclude,
                                "resources": server.tools.resources,
                                "prompts": server.tools.prompts,
                            },
                        }),
                    );
                }
                return Ok(json!({ "mcpServers": servers }));
            }
        }
    }

    if let Some(path) = mcp_config_path().filter(|path| path.is_file()) {
        let content = std::fs::read_to_string(&path).map_err(|e| ToolError::ExecutionFailed {
            tool: "mcp_client".into(),
            message: format!("Failed to read MCP config: {e}"),
        })?;

        return serde_json::from_str(&content).map_err(|e| ToolError::ExecutionFailed {
            tool: "mcp_client".into(),
            message: format!("Invalid MCP config JSON: {e}"),
        });
    }

    Ok(json!({ "mcpServers": {} }))
}

pub fn configured_servers() -> Result<Vec<ConfiguredMcpServer>, ToolError> {
    let config = load_mcp_config()?;
    let servers = config
        .get("mcpServers")
        .and_then(|s| s.as_object())
        .ok_or_else(|| ToolError::ExecutionFailed {
            tool: "mcp_client".into(),
            message: "MCP config missing 'mcpServers' object".into(),
        })?;

    let mut parsed: Vec<ConfiguredMcpServer> = servers
        .iter()
        .map(|(name, value)| parse_configured_server(name, value))
        .collect();
    parsed.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(parsed)
}

pub async fn probe_configured_server(server_name: &str) -> Result<McpProbeResult, ToolError> {
    let server = configured_servers()?
        .into_iter()
        .find(|server| server.name == server_name)
        .ok_or_else(|| ToolError::InvalidArgs {
            tool: "mcp_client".into(),
            message: format!("Unknown MCP server '{server_name}'"),
        })?;

    get_or_connect(server_name, to_runtime_server_config(&server)).await?;

    let pool = connections();
    let conn_mutex = pool
        .get(server_name)
        .ok_or_else(|| ToolError::ExecutionFailed {
            tool: "mcp_client".into(),
            message: format!("Connection to '{server_name}' not found after connect"),
        })?;

    let mut conn = conn_mutex.value().lock().await;
    let result = conn.rpc_call("tools/list", json!({})).await?;
    let tools: Vec<(String, String)> = result
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|tools| {
            let filtered = apply_tool_filter(tools, &server.include, &server.exclude);
            filtered
                .iter()
                .map(|tool| {
                    (
                        tool.get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        tool.get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(McpProbeResult {
        server_name: server.name,
        transport: if server.url.is_some() {
            "http".into()
        } else {
            "stdio".into()
        },
        tool_count: tools.len(),
        tools,
    })
}

// ─── mcp_list_tools ─────────────────────────────────────────────

/// List available MCP tools from all connected servers.
pub struct McpListToolsTool;

#[derive(Deserialize)]
struct ListArgs {
    /// Optional server name to query. If omitted, queries all configured servers.
    #[serde(default)]
    server: Option<String>,
}

#[async_trait]
impl ToolHandler for McpListToolsTool {
    fn name(&self) -> &'static str {
        "mcp_list_tools"
    }

    fn toolset(&self) -> &'static str {
        "mcp"
    }

    fn emoji(&self) -> &'static str {
        "🔌"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "mcp_list_tools".into(),
            description:
                "List available tools from connected MCP (Model Context Protocol) servers.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server": {
                        "type": "string",
                        "description": "MCP server name to query. Omit to list tools from all servers."
                    }
                }
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        yaml_config_path().is_some_and(|p| p.is_file())
            || mcp_config_path().is_some_and(|p| p.is_file())
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        let args: ListArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "mcp_list_tools".into(),
            message: e.to_string(),
        })?;

        let mut all_tools = Vec::new();
        for server in configured_servers()? {
            if let Some(ref filter) = args.server {
                if &server.name != filter {
                    continue;
                }
            }

            if ctx.cancel.is_cancelled() {
                return Err(ToolError::Other("Cancelled".into()));
            }

            get_or_connect(&server.name, to_runtime_server_config(&server)).await?;

            let pool = connections();
            if let Some(conn_mutex) = pool.get(&server.name) {
                let mut conn = conn_mutex.value().lock().await;
                let result = conn.rpc_call("tools/list", json!({})).await?;

                if let Some(raw_tools) = result.get("tools").and_then(|t| t.as_array()) {
                    let filtered = apply_tool_filter(raw_tools, &server.include, &server.exclude);
                    for tool in &filtered {
                        let tool_name = tool
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown");
                        let tool_desc = tool
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("");
                        all_tools.push(format!("[{}] {tool_name}: {tool_desc}", server.name));
                    }
                }
            }
        }

        if all_tools.is_empty() {
            return Ok("No MCP tools discovered from configured servers.".into());
        }

        Ok(format!(
            "Available MCP tools ({} total):\n\n{}",
            all_tools.len(),
            all_tools.join("\n")
        ))
    }
}

inventory::submit!(&McpListToolsTool as &dyn ToolHandler);

// ─── mcp_call_tool ──────────────────────────────────────────────

/// Call an MCP tool by name on a specified server.
pub struct McpCallToolTool;

#[derive(Deserialize)]
struct CallArgs {
    /// Name of the MCP server to call the tool on.
    server: String,
    /// Name of the MCP tool to invoke.
    tool_name: String,
    /// Arguments to pass to the tool (JSON object).
    #[serde(default)]
    arguments: serde_json::Value,
}

#[async_trait]
impl ToolHandler for McpCallToolTool {
    fn name(&self) -> &'static str {
        "mcp_call_tool"
    }

    fn toolset(&self) -> &'static str {
        "mcp"
    }

    fn emoji(&self) -> &'static str {
        "🔌"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "mcp_call_tool".into(),
            description:
                "Call an MCP tool by name on a specific server. Use mcp_list_tools to discover available tools first."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server": {
                        "type": "string",
                        "description": "MCP server name (from mcp_list_tools)"
                    },
                    "tool_name": {
                        "type": "string",
                        "description": "Name of the MCP tool to call"
                    },
                    "arguments": {
                        "type": "object",
                        "description": "Arguments to pass to the tool"
                    }
                },
                "required": ["server", "tool_name"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        yaml_config_path().is_some_and(|p| p.is_file())
            || mcp_config_path().is_some_and(|p| p.is_file())
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        let args: CallArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "mcp_call_tool".into(),
            message: e.to_string(),
        })?;

        // Ensure server is connected
        let config = load_mcp_config()?;
        let servers = config
            .get("mcpServers")
            .and_then(|s| s.as_object())
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: "mcp_call_tool".into(),
                message: "MCP config missing 'mcpServers' object".into(),
            })?;

        let server_config = servers
            .get(&args.server)
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "mcp_call_tool".into(),
                message: format!("Unknown MCP server '{}'", args.server),
            })?;

        let command = parse_expanded_string(server_config.get("command")).unwrap_or_default();

        let url = parse_expanded_string(server_config.get("url"));

        let bearer_token = parse_expanded_string(server_config.get("bearer_token"));

        let cmd_args = parse_string_array(server_config.get("args"));

        // Extract env vars from config so they reach the subprocess
        let cmd_envs = parse_string_map(server_config.get("env"));

        get_or_connect(
            &args.server,
            McpServerConfig {
                url,
                bearer_token,
                oauth: parse_oauth_config(server_config.get("oauth")),
                headers: parse_string_map(server_config.get("headers")),
                command,
                args: cmd_args,
                cwd: parse_expanded_path(server_config.get("cwd")),
                envs: cmd_envs,
                timeout: server_config.get("timeout").and_then(|t| t.as_u64()),
                connect_timeout: server_config
                    .get("connect_timeout")
                    .and_then(|t| t.as_u64()),
            },
        )
        .await?;

        // Validate that the requested tool is not excluded by the filter
        {
            let (include, exclude) = extract_tool_filter(server_config);
            let name_ref = args.tool_name.as_str();
            let allowed = if !include.is_empty() {
                include.iter().any(|i| i == name_ref)
            } else if !exclude.is_empty() {
                !exclude.iter().any(|e| e == name_ref)
            } else {
                true
            };
            if !allowed {
                return Err(ToolError::InvalidArgs {
                    tool: "mcp_call_tool".into(),
                    message: format!(
                        "Tool '{}' on server '{}' is excluded by the server's tools filter",
                        args.tool_name, args.server
                    ),
                });
            }
        }

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        let pool = connections();
        let conn_mutex = pool
            .get(&args.server)
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: "mcp_call_tool".into(),
                message: format!("Connection to '{}' not found", args.server),
            })?;

        let mut conn = conn_mutex.value().lock().await;
        let result = conn
            .rpc_call(
                "tools/call",
                json!({
                    "name": args.tool_name,
                    "arguments": args.arguments
                }),
            )
            .await?;

        // Extract text content from MCP tool response
        if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            let texts: Vec<&str> = content
                .iter()
                .filter_map(|item| {
                    if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                        item.get("text").and_then(|t| t.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            if !texts.is_empty() {
                return Ok(texts.join("\n"));
            }
        }

        // Fallback: return raw JSON
        Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()))
    }
}

inventory::submit!(&McpCallToolTool as &dyn ToolHandler);

// ─── Public utilities ─────────────────────────────────────────────────

/// Drop all active MCP server connections so they are re-established on the
/// next tool call.  Called by `/reload-mcp` in the CLI.
///
/// WHY: MCP servers may be restarted or reconfigured without restarting
/// EdgeCrab. Clearing the pool forces a fresh subprocess spawn + handshake
/// on the next `mcp_list_tools` / `mcp_call_tool` invocation.
pub fn reload_mcp_connections() {
    connections().clear();
}

#[derive(Debug, Clone)]
pub struct ConfiguredMcpServer {
    pub name: String,
    pub url: Option<String>,
    pub bearer_token: Option<String>,
    pub oauth: Option<OAuthConfig>,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub timeout: Option<u64>,
    pub connect_timeout: Option<u64>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub token_from_config: bool,
    pub token_from_store: bool,
}

#[derive(Debug, Clone)]
pub struct McpProbeResult {
    pub server_name: String,
    pub transport: String,
    pub tool_count: usize,
    pub tools: Vec<(String, String)>,
}

// ─── Dynamic prefixed MCP tools (mcp_<server>_<tool>) ────────────────────────

/// Leak a `String` to produce a `&'static str`.
///
/// WHY Box::leak: The `ToolHandler` trait requires `&'static str` for `name()`
/// and `toolset()`. For MCP dynamic tools, these strings are known at runtime
/// (discovered from the server). Box::leak permanently allocates the string
/// in the process heap and returns a static reference — acceptable here
/// because discovery happens at startup/reload only, so the total number of
/// leaked strings is bounded by `(servers * tools)`.
fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Sanitize a name to a safe Rust identifier fragment (alphanumeric + `_`).
fn sanitize_to_identifier(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // Collapse consecutive underscores and trim trailing/leading ones
    let mut prev_underscore = false;
    let mut clean = String::with_capacity(out.len());
    for c in out.chars() {
        if c == '_' {
            if !prev_underscore {
                clean.push(c);
            }
            prev_underscore = true;
        } else {
            clean.push(c);
            prev_underscore = false;
        }
    }
    clean.trim_matches('_').to_string()
}

/// A dynamically registered MCP tool that proxies calls to a specific server+tool.
///
/// These are registered at startup via `discover_and_register_mcp_tools()` and
/// appear in the LLM tool schema as `mcp_<server_name>_<tool_name>` — making
/// MCP server tools first-class tools the model can call directly without
/// needing to go through the `mcp_call_tool` meta-tool.
pub struct McpDynamicTool {
    /// Prefixed tool name, e.g. `"mcp_github_list_issues"` — leaked static str.
    name_static: &'static str,
    /// Per-server toolset, e.g. `"mcp-github"` — leaked static str.
    toolset_static: &'static str,
    /// Original tool name as reported by the server (used for the RPC call).
    original_name: String,
    /// MCP server name (key in mcp_servers config).
    server_name: String,
    /// Tool description forwarded from the server.
    description: String,
    /// JSON Schema of the tool's input parameters (from the server).
    input_schema: serde_json::Value,
}

impl McpDynamicTool {
    /// Construct a dynamic tool wrapper for one server+tool combination.
    ///
    /// `server_name` — config key, e.g. `"github"`
    /// `original_name` — tool name returned by `tools/list`, e.g. `"list_issues"`
    pub fn new(
        server_name: &str,
        original_name: &str,
        description: &str,
        input_schema: serde_json::Value,
    ) -> Self {
        let safe_server = sanitize_to_identifier(server_name);
        let safe_tool = sanitize_to_identifier(original_name);
        let name_str = format!("mcp_{safe_server}_{safe_tool}");
        let toolset_str = format!("mcp-{safe_server}");

        Self {
            name_static: leak_str(name_str),
            toolset_static: leak_str(toolset_str),
            original_name: original_name.to_string(),
            server_name: server_name.to_string(),
            description: description.to_string(),
            input_schema,
        }
    }
}

#[async_trait]
impl ToolHandler for McpDynamicTool {
    fn name(&self) -> &'static str {
        self.name_static
    }

    fn toolset(&self) -> &'static str {
        self.toolset_static
    }

    fn emoji(&self) -> &'static str {
        "🔌"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name_static.into(),
            description: format!(
                "[MCP:{server}] {desc}",
                server = self.server_name,
                desc = self.description
            ),
            parameters: self.input_schema.clone(),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        let pool = connections();
        let conn_mutex = pool
            .get(&self.server_name)
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: self.name_static.to_string(),
                message: format!(
                    "No connection to MCP server '{}'. Try running `/reload-mcp`.",
                    self.server_name
                ),
            })?;

        let mut conn = conn_mutex.value().lock().await;
        let result = conn
            .rpc_call(
                "tools/call",
                json!({
                    "name": self.original_name,
                    "arguments": args
                }),
            )
            .await?;

        // Extract text content from MCP tool response
        if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            let texts: Vec<&str> = content
                .iter()
                .filter_map(|item| {
                    if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                        item.get("text").and_then(|t| t.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            if !texts.is_empty() {
                return Ok(texts.join("\n"));
            }
        }

        Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()))
    }
}

/// Discover all tools from configured MCP servers and register them as
/// prefixed dynamic tools (`mcp_<server>_<tool_name>`) in the registry.
///
/// Called once at startup. Connects to each enabled MCP server, fetches its
/// tool list, applies include/exclude filters, and registers each tool so the
/// LLM can call them directly by name without the `mcp_call_tool` meta-tool.
///
/// Errors from individual servers are logged as warnings but do not prevent
/// other servers from being registered.
pub async fn discover_and_register_mcp_tools(registry: &mut crate::registry::ToolRegistry) {
    let config = match load_mcp_config() {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(
                "MCP config not found or unreadable, skipping dynamic registration: {e}"
            );
            return;
        }
    };

    let servers = match config.get("mcpServers").and_then(|s| s.as_object()) {
        Some(s) => s.clone(),
        None => return,
    };

    for (server_name, server_config) in &servers {
        let command = parse_expanded_string(server_config.get("command")).unwrap_or_default();

        let url = parse_expanded_string(server_config.get("url"));

        let bearer_token = parse_expanded_string(server_config.get("bearer_token"));

        let cmd_args = parse_string_array(server_config.get("args"));

        let cmd_envs = parse_string_map(server_config.get("env"));

        let headers = parse_string_map(server_config.get("headers"));

        let timeout = server_config.get("timeout").and_then(|t| t.as_u64());
        let connect_timeout = server_config
            .get("connect_timeout")
            .and_then(|t| t.as_u64());

        // Skip servers with no valid transport
        if url.is_none() && command.trim().is_empty() {
            tracing::debug!("MCP server '{server_name}' has no url or command, skipping");
            continue;
        }

        // Connect (or reuse existing connection)
        if let Err(e) = get_or_connect(
            server_name,
            McpServerConfig {
                url,
                bearer_token,
                oauth: parse_oauth_config(server_config.get("oauth")),
                headers,
                command,
                args: cmd_args,
                cwd: parse_expanded_path(server_config.get("cwd")),
                envs: cmd_envs,
                timeout,
                connect_timeout,
            },
        )
        .await
        {
            tracing::warn!("Failed to connect to MCP server '{server_name}': {e}");
            continue;
        }

        // Fetch tool list from server
        let tools_result = {
            let pool = connections();
            let conn_mutex = match pool.get(server_name.as_str()) {
                Some(c) => c,
                None => continue,
            };
            let mut conn = conn_mutex.value().lock().await;
            conn.rpc_call("tools/list", json!({})).await
        };

        let tools_value = match tools_result {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("tools/list failed for MCP server '{server_name}': {e}");
                continue;
            }
        };

        let raw_tools: &[serde_json::Value] = tools_value
            .get("tools")
            .and_then(|t| t.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[]);

        let (include, exclude) = extract_tool_filter(server_config);
        let filtered = apply_tool_filter(raw_tools, &include, &exclude);

        let mut registered = 0usize;
        for tool in &filtered {
            let tool_name = match tool.get("name").and_then(|n| n.as_str()) {
                Some(n) => n,
                None => continue,
            };
            let description = tool
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");
            let schema = tool
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object", "properties": {}}));

            let dynamic_tool = McpDynamicTool::new(server_name, tool_name, description, schema);
            tracing::debug!(
                "Registering dynamic MCP tool '{}' (server='{server_name}')",
                dynamic_tool.name_static
            );
            registry.register_dynamic(Box::new(dynamic_tool));
            registered += 1;
        }

        if registered > 0 {
            tracing::info!(
                "Registered {registered} dynamic tool(s) for MCP server '{server_name}' \
                 (toolset 'mcp-{}')",
                sanitize_to_identifier(server_name)
            );
        }

        // Check server capabilities for utility wrapper registration
        // (resources / prompts toggles from config)
        let resources_enabled = server_config
            .get("tools")
            .and_then(|t| t.get("resources"))
            .and_then(|b| b.as_bool())
            .unwrap_or(true);
        let prompts_enabled = server_config
            .get("tools")
            .and_then(|t| t.get("prompts"))
            .and_then(|b| b.as_bool())
            .unwrap_or(true);

        // Probe resources capability with a benign resources/list call
        if resources_enabled {
            let probe = {
                let pool = connections();
                let conn_mutex = match pool.get(server_name.as_str()) {
                    Some(c) => c,
                    None => continue,
                };
                let mut conn = conn_mutex.value().lock().await;
                conn.rpc_call("resources/list", json!({})).await
            };
            if probe.is_ok() {
                let lr = McpDynamicTool::new(
                    server_name,
                    "list_resources",
                    "List available resources on this MCP server",
                    json!({"type": "object", "properties": {}}),
                );
                let rr = McpDynamicTool::new(
                    server_name,
                    "read_resource",
                    "Read a resource by URI from this MCP server",
                    json!({
                        "type": "object",
                        "properties": {
                            "uri": {"type": "string", "description": "Resource URI to read"}
                        },
                        "required": ["uri"]
                    }),
                );
                registry.register_dynamic(Box::new(lr));
                registry.register_dynamic(Box::new(rr));
                tracing::debug!("Registered resource utility wrappers for '{server_name}'");
            }
        }

        if prompts_enabled {
            let probe = {
                let pool = connections();
                let conn_mutex = match pool.get(server_name.as_str()) {
                    Some(c) => c,
                    None => continue,
                };
                let mut conn = conn_mutex.value().lock().await;
                conn.rpc_call("prompts/list", json!({})).await
            };
            if probe.is_ok() {
                let lp = McpDynamicTool::new(
                    server_name,
                    "list_prompts",
                    "List available prompts on this MCP server",
                    json!({"type": "object", "properties": {}}),
                );
                let gp = McpDynamicTool::new(
                    server_name,
                    "get_prompt",
                    "Get a prompt by name from this MCP server",
                    json!({
                        "type": "object",
                        "properties": {
                            "name": {"type": "string", "description": "Prompt name"},
                            "arguments": {
                                "type": "object",
                                "description": "Optional prompt arguments"
                            }
                        },
                        "required": ["name"]
                    }),
                );
                registry.register_dynamic(Box::new(lp));
                registry.register_dynamic(Box::new(gp));
                tracing::debug!("Registered prompt utility wrappers for '{server_name}'");
            }
        }
    }
}

// ─── Static utility tools (mcp_list_resources / mcp_read_resource / etc.) ────
//
// These provide a server-agnostic way to access MCP resources and prompts
// without relying on per-server dynamic registration (useful when servers
// are added after startup or when discovery is skipped).

/// List MCP resources on a named server.
pub struct McpListResourcesTool;

#[async_trait]
impl ToolHandler for McpListResourcesTool {
    fn name(&self) -> &'static str {
        "mcp_list_resources"
    }
    fn toolset(&self) -> &'static str {
        "mcp"
    }
    fn emoji(&self) -> &'static str {
        "🔌"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "mcp_list_resources".into(),
            description: "List available resources from an MCP server.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server": {
                        "type": "string",
                        "description": "MCP server name"
                    }
                },
                "required": ["server"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        yaml_config_path().is_some_and(|p| p.is_file())
            || mcp_config_path().is_some_and(|p| p.is_file())
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        #[derive(Deserialize)]
        struct Args {
            server: String,
        }
        let a: Args = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "mcp_list_resources".into(),
            message: e.to_string(),
        })?;
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        ensure_server_connected(&a.server).await?;

        let pool = connections();
        let conn_mutex = pool
            .get(&a.server)
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: "mcp_list_resources".into(),
                message: format!("Not connected to server '{}'", a.server),
            })?;
        let mut conn = conn_mutex.value().lock().await;
        let result = conn.rpc_call("resources/list", json!({})).await?;
        Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()))
    }
}

inventory::submit!(&McpListResourcesTool as &dyn ToolHandler);

/// Read an MCP resource by URI on a named server.
pub struct McpReadResourceTool;

#[async_trait]
impl ToolHandler for McpReadResourceTool {
    fn name(&self) -> &'static str {
        "mcp_read_resource"
    }
    fn toolset(&self) -> &'static str {
        "mcp"
    }
    fn emoji(&self) -> &'static str {
        "🔌"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "mcp_read_resource".into(),
            description: "Read a resource by URI from an MCP server.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server": {"type": "string", "description": "MCP server name"},
                    "uri":    {"type": "string", "description": "Resource URI to read"}
                },
                "required": ["server", "uri"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        yaml_config_path().is_some_and(|p| p.is_file())
            || mcp_config_path().is_some_and(|p| p.is_file())
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        #[derive(Deserialize)]
        struct Args {
            server: String,
            uri: String,
        }
        let a: Args = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "mcp_read_resource".into(),
            message: e.to_string(),
        })?;
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        ensure_server_connected(&a.server).await?;

        let pool = connections();
        let conn_mutex = pool
            .get(&a.server)
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: "mcp_read_resource".into(),
                message: format!("Not connected to server '{}'", a.server),
            })?;
        let mut conn = conn_mutex.value().lock().await;
        let result = conn
            .rpc_call("resources/read", json!({"uri": a.uri}))
            .await?;
        Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()))
    }
}

inventory::submit!(&McpReadResourceTool as &dyn ToolHandler);

/// List MCP prompts on a named server.
pub struct McpListPromptsTool;

#[async_trait]
impl ToolHandler for McpListPromptsTool {
    fn name(&self) -> &'static str {
        "mcp_list_prompts"
    }
    fn toolset(&self) -> &'static str {
        "mcp"
    }
    fn emoji(&self) -> &'static str {
        "🔌"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "mcp_list_prompts".into(),
            description: "List available prompts from an MCP server.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server": {"type": "string", "description": "MCP server name"}
                },
                "required": ["server"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        yaml_config_path().is_some_and(|p| p.is_file())
            || mcp_config_path().is_some_and(|p| p.is_file())
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        #[derive(Deserialize)]
        struct Args {
            server: String,
        }
        let a: Args = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "mcp_list_prompts".into(),
            message: e.to_string(),
        })?;
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        ensure_server_connected(&a.server).await?;

        let pool = connections();
        let conn_mutex = pool
            .get(&a.server)
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: "mcp_list_prompts".into(),
                message: format!("Not connected to server '{}'", a.server),
            })?;
        let mut conn = conn_mutex.value().lock().await;
        let result = conn.rpc_call("prompts/list", json!({})).await?;
        Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()))
    }
}

inventory::submit!(&McpListPromptsTool as &dyn ToolHandler);

/// Get a named prompt from an MCP server.
pub struct McpGetPromptTool;

#[async_trait]
impl ToolHandler for McpGetPromptTool {
    fn name(&self) -> &'static str {
        "mcp_get_prompt"
    }
    fn toolset(&self) -> &'static str {
        "mcp"
    }
    fn emoji(&self) -> &'static str {
        "🔌"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "mcp_get_prompt".into(),
            description: "Get a prompt by name from an MCP server.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "server": {
                        "type": "string",
                        "description": "MCP server name"
                    },
                    "name": {
                        "type": "string",
                        "description": "Prompt name to retrieve"
                    },
                    "arguments": {
                        "type": "object",
                        "description": "Optional prompt arguments"
                    }
                },
                "required": ["server", "name"]
            }),
            strict: None,
        }
    }

    fn is_available(&self) -> bool {
        yaml_config_path().is_some_and(|p| p.is_file())
            || mcp_config_path().is_some_and(|p| p.is_file())
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        #[derive(Deserialize)]
        struct Args {
            server: String,
            name: String,
            #[serde(default)]
            arguments: serde_json::Value,
        }
        let a: Args = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: "mcp_get_prompt".into(),
            message: e.to_string(),
        })?;
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        ensure_server_connected(&a.server).await?;

        let pool = connections();
        let conn_mutex = pool
            .get(&a.server)
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: "mcp_get_prompt".into(),
                message: format!("Not connected to server '{}'", a.server),
            })?;
        let mut conn = conn_mutex.value().lock().await;
        let result = conn
            .rpc_call(
                "prompts/get",
                json!({"name": a.name, "arguments": a.arguments}),
            )
            .await?;
        Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()))
    }
}

inventory::submit!(&McpGetPromptTool as &dyn ToolHandler);

/// Helper: ensure a named server is connected, loading its config from disk.
///
/// Uses `load_mcp_config()` to look up the server by name and calls
/// `get_or_connect()`. Returns an error if the server is not found in config.
async fn ensure_server_connected(server_name: &str) -> Result<(), ToolError> {
    if connections().contains_key(server_name) {
        return Ok(());
    }
    let server = configured_servers()?
        .into_iter()
        .find(|server| server.name == server_name)
        .ok_or_else(|| ToolError::InvalidArgs {
            tool: "mcp_client".into(),
            message: format!("Unknown MCP server '{server_name}'"),
        })?;

    get_or_connect(server_name, to_runtime_server_config(&server)).await
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEdgecrabHome;
    use std::sync::Mutex;

    static EDGECRAB_HOME_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn mcp_list_tools_schema_valid() {
        let schema = McpListToolsTool.schema();
        assert_eq!(schema.name, "mcp_list_tools");
        assert!(schema.parameters["properties"].get("server").is_some());
    }

    #[test]
    fn mcp_call_tool_schema_valid() {
        let schema = McpCallToolTool.schema();
        assert_eq!(schema.name, "mcp_call_tool");
        let required = schema.parameters["required"].as_array().expect("array");
        assert!(required.iter().any(|v| v == "server"));
        assert!(required.iter().any(|v| v == "tool_name"));
    }

    #[test]
    fn request_id_increments() {
        let id1 = next_request_id();
        let id2 = next_request_id();
        assert!(id2 > id1);
    }

    #[test]
    fn connections_pool_is_singleton() {
        let pool1 = connections();
        let pool2 = connections();
        assert!(std::ptr::eq(pool1, pool2));
    }

    #[test]
    fn mcp_config_path_has_expected_suffix() {
        if let Some(path) = mcp_config_path() {
            assert!(path.ends_with("mcp.json"));
        }
    }

    #[test]
    fn mcp_list_tools_toolset() {
        assert_eq!(McpListToolsTool.toolset(), "mcp");
        assert_eq!(McpCallToolTool.toolset(), "mcp");
    }

    #[tokio::test]
    async fn mcp_call_tool_rejects_missing_server() {
        let ctx = ToolContext::test_context();
        let result = McpCallToolTool
            .execute(json!({"tool_name": "some_tool"}), &ctx)
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn mcp_list_tools_invalid_args() {
        let _guard = EDGECRAB_HOME_LOCK.lock().expect("lock");
        let _home = TestEdgecrabHome::new();
        let ctx = ToolContext::test_context();
        // Empty args are fine; no config should now behave as an empty catalog
        // rather than a hard legacy-path failure.
        let result = tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(async { McpListToolsTool.execute(json!({}), &ctx).await });
        let output = result.expect("empty MCP config should be tolerated");
        assert!(output.contains("No MCP tools discovered"));
    }

    #[tokio::test]
    async fn mcp_call_tool_cancelled() {
        let ctx = ToolContext::test_context();
        ctx.cancel.cancel();
        let result = McpCallToolTool
            .execute(json!({"server": "test", "tool_name": "test"}), &ctx)
            .await;
        assert!(result.is_err());
        assert!(
            result
                .expect_err("cancelled")
                .to_string()
                .contains("Cancelled")
        );
    }

    // ─── Tool filter tests ────────────────────────────────────────────

    fn make_tool(name: &str) -> serde_json::Value {
        json!({"name": name, "description": ""})
    }

    #[test]
    fn filter_empty_lists_returns_all() {
        let tools = vec![make_tool("a"), make_tool("b"), make_tool("c")];
        let result = apply_tool_filter(&tools, &[], &[]);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn filter_include_whitelist() {
        let tools = vec![make_tool("a"), make_tool("b"), make_tool("c")];
        let include = vec!["a".to_string(), "c".to_string()];
        let result = apply_tool_filter(&tools, &include, &[]);
        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|t| t["name"] == "a"));
        assert!(result.iter().any(|t| t["name"] == "c"));
    }

    #[test]
    fn filter_exclude_blacklist() {
        let tools = vec![make_tool("a"), make_tool("b"), make_tool("c")];
        let exclude = vec!["b".to_string()];
        let result = apply_tool_filter(&tools, &[], &exclude);
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|t| t["name"] != "b"));
    }

    #[test]
    fn filter_include_wins_over_exclude() {
        // When both include and exclude are set, include (whitelist) wins
        let tools = vec![make_tool("a"), make_tool("b")];
        let include = vec!["a".to_string()];
        let exclude = vec!["a".to_string()]; // would exclude "a" but include wins
        let result = apply_tool_filter(&tools, &include, &exclude);
        // Should have only "a" (include wins)
        assert_eq!(result.len(), 1);
        assert!(result.iter().any(|t| t["name"] == "a"));
    }

    #[test]
    fn dynamic_tool_name_prefixed() {
        let tool = McpDynamicTool::new("github", "list_issues", "desc", json!({}));
        assert_eq!(tool.name_static, "mcp_github_list_issues");
        assert_eq!(tool.toolset_static, "mcp-github");
    }

    #[test]
    fn dynamic_tool_sanitizes_special_chars() {
        let tool = McpDynamicTool::new("my-server", "get/resource", "desc", json!({}));
        // Name should be sanitized: hyphens and slashes → underscores
        assert!(tool.name_static.starts_with("mcp_"));
        assert!(!tool.name_static.contains('-'));
        assert!(!tool.name_static.contains('/'));
    }

    #[test]
    fn utility_tools_have_correct_toolset() {
        assert_eq!(McpListResourcesTool.toolset(), "mcp");
        assert_eq!(McpReadResourceTool.toolset(), "mcp");
        assert_eq!(McpListPromptsTool.toolset(), "mcp");
        assert_eq!(McpGetPromptTool.toolset(), "mcp");
    }

    #[test]
    fn utility_tools_schema_valid() {
        let schemas = [
            McpListResourcesTool.schema(),
            McpReadResourceTool.schema(),
            McpListPromptsTool.schema(),
            McpGetPromptTool.schema(),
        ];
        for schema in &schemas {
            // All utility tools require a "server" parameter
            let required = schema.parameters["required"]
                .as_array()
                .expect("required array");
            assert!(
                required.iter().any(|v| v == "server"),
                "schema {} missing required 'server'",
                schema.name
            );
        }
    }

    #[test]
    fn mcp_config_path_respects_edgecrab_home() {
        let _guard = EDGECRAB_HOME_LOCK.lock().expect("lock");
        let home = TestEdgecrabHome::new();
        let path = mcp_config_path().expect("mcp path");
        assert_eq!(path, home.path().join("mcp.json"));
    }

    #[test]
    fn configured_servers_reads_yaml_and_preserves_cwd() {
        let _guard = EDGECRAB_HOME_LOCK.lock().expect("lock");
        let home = TestEdgecrabHome::new();
        std::fs::write(
            home.path().join("config.yaml"),
            "mcp_servers:\n  filesystem:\n    command: npx\n    args: ['-y', '@modelcontextprotocol/server-filesystem', '/tmp']\n    cwd: /tmp\n    enabled: true\n",
        )
        .expect("config");
        let servers = configured_servers().expect("servers");

        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "filesystem");
        assert_eq!(
            servers[0].cwd.as_deref(),
            Some(std::path::Path::new("/tmp"))
        );
    }

    #[test]
    fn configured_servers_expand_env_backed_http_auth_fields() {
        let _guard = EDGECRAB_HOME_LOCK.lock().expect("lock");
        let home = TestEdgecrabHome::new();
        // SAFETY: serialized by EDGECRAB_HOME_LOCK for the guard lifetime.
        unsafe {
            std::env::set_var("MCP_HTTP_URL", "https://auth.example.com/mcp");
            std::env::set_var("MCP_ACCESS_TOKEN", "oauth-token");
        }
        std::fs::write(
            home.path().join("config.yaml"),
            "mcp_servers:\n  oauth:\n    url: ${MCP_HTTP_URL}\n    bearer_token: ${MCP_ACCESS_TOKEN}\n    headers:\n      X-Tenant: ${MCP_ACCESS_TOKEN}\n    enabled: true\n",
        )
        .expect("config");

        let servers = configured_servers().expect("servers");

        assert_eq!(servers.len(), 1);
        assert_eq!(
            servers[0].url.as_deref(),
            Some("https://auth.example.com/mcp")
        );
        assert_eq!(servers[0].bearer_token.as_deref(), Some("oauth-token"));
        assert_eq!(
            servers[0].headers.get("X-Tenant").map(String::as_str),
            Some("oauth-token")
        );

        // SAFETY: serialized by EDGECRAB_HOME_LOCK for the guard lifetime.
        unsafe {
            std::env::remove_var("MCP_HTTP_URL");
            std::env::remove_var("MCP_ACCESS_TOKEN");
        }
    }

    #[test]
    fn configured_servers_read_oauth_from_yaml_config() {
        let _guard = EDGECRAB_HOME_LOCK.lock().expect("lock");
        let home = TestEdgecrabHome::new();
        std::fs::write(
            home.path().join("config.yaml"),
            "mcp_servers:\n  oauth:\n    url: https://example.com/mcp\n    enabled: true\n    oauth:\n      token_url: https://example.com/oauth/token\n      grant_type: refresh_token\n      auth_method: none\n",
        )
        .expect("config");

        let servers = configured_servers().expect("servers");

        assert_eq!(servers.len(), 1);
        let oauth = servers[0].oauth.as_ref().expect("oauth config");
        assert_eq!(oauth.token_url(), "https://example.com/oauth/token");
        assert_eq!(oauth.grant_type_label(), "refresh_token");
        assert_eq!(oauth.auth_method_label(), "none");
    }

    #[test]
    fn expand_config_string_leaves_unresolved_placeholders_visible() {
        // Missing vars should not panic; the unresolved placeholder remains visible
        // so doctor/reporting code can explain the problem.
        assert_eq!(
            expand_config_string("${EDGECRAB_UNKNOWN_TOKEN}"),
            "${EDGECRAB_UNKNOWN_TOKEN}"
        );
    }

    #[test]
    fn write_mcp_token_preserves_existing_refresh_token() {
        let _guard = EDGECRAB_HOME_LOCK.lock().expect("lock");
        let home = TestEdgecrabHome::new();
        let _ = home;

        write_mcp_refresh_token("oauth", "refresh-1").expect("write refresh");
        write_mcp_token("oauth", "access-1").expect("write access");

        let status = read_mcp_token_status("oauth").expect("status");
        assert!(status.has_access_token);
        assert!(status.has_refresh_token);
    }

    #[test]
    fn write_mcp_refresh_token_preserves_existing_access_token() {
        let _guard = EDGECRAB_HOME_LOCK.lock().expect("lock");
        let home = TestEdgecrabHome::new();
        let _ = home;

        write_mcp_token("oauth", "access-1").expect("write access");
        write_mcp_refresh_token("oauth", "refresh-1").expect("write refresh");

        assert_eq!(read_mcp_token("oauth").as_deref(), Some("access-1"));
        let status = read_mcp_token_status("oauth").expect("status");
        assert!(status.has_access_token);
        assert!(status.has_refresh_token);
    }
}
