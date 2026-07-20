//! MCP server registration control plane (spec 022/014 WS-A).
//!
//! DRY: CLI `edgecrab mcp add` and TUI `/mcp add` both call [`register_mcp_server`]
//! (and [`prepare_and_register_mcp_url`] for URL OAuth discovery).
//! SOLID: this module builds + validates + persists config only — interactive OAuth
//! login stays in [`crate::mcp_oauth`]; transport/probe stay in `mcp_client`;
//! RFC 9728 discovery stays in `edgecrab_tools::mcp_auth`.

use std::collections::HashMap;
use std::path::Path;

use edgecrab_core::config::{AppConfig, McpOauthConfig, McpServerConfig};
use edgecrab_security::url_safety::{is_preview_loopback_url, is_safe_url};
use edgecrab_tools::mcp_auth::{DiscoverOpts, DiscoveredMcpOauth, discover_mcp_oauth};
use edgecrab_tools::tools::mcp_client::{reload_mcp_connections, write_mcp_token};

/// How an HTTP MCP server authenticates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum McpAuthKind {
    /// Infer: oauth if oauth fields set, bearer if token provided, else try discovery.
    #[default]
    Auto,
    OAuth,
    Bearer,
    None,
}

impl McpAuthKind {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Ok(Self::Auto),
            "oauth" => Ok(Self::OAuth),
            "bearer" | "token" => Ok(Self::Bearer),
            "none" | "no" | "off" | "public" => Ok(Self::None),
            "header" => Ok(Self::Bearer), // treat custom header auth as bearer path for now
            other => Err(format!(
                "unknown --auth '{other}' (expected oauth|bearer|none|auto)"
            )),
        }
    }

    #[allow(dead_code)] // used by TUI/diagnostics callers
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::OAuth => "oauth",
            Self::Bearer => "bearer",
            Self::None => "none",
        }
    }
}

/// Registration request (transport + auth). CLI and TUI map args into this.
#[derive(Debug, Clone)]
pub struct RegisterMcpRequest {
    pub name: String,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub auth: McpAuthKind,
    pub token: Option<String>,
    pub token_url: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub device_authorization_url: Option<String>,
    pub authorization_url: Option<String>,
    pub redirect_url: Option<String>,
    pub scopes: Vec<String>,
    /// When true (default false), allow loopback/private URLs via existing SSRF policy.
    /// Also honored when `EDGECRAB_E2E_SSRF_ALLOW_LOCALHOST=1` (security crate).
    pub allow_loopback: bool,
    /// Force/disable OAuth discovery. `None` = auto (discover when OAuth incomplete).
    pub discover: Option<bool>,
    /// RFC 8707 resource indicator (filled by discovery).
    pub resource: Option<String>,
    /// AS issuer (RFC 9207).
    pub issuer: Option<String>,
    pub iss_parameter_supported: Option<bool>,
    pub auth_method: Option<String>,
    pub grant_type: Option<String>,
    pub use_pkce: Option<bool>,
}

impl RegisterMcpRequest {
    /// Build from CLI-style fields including legacy `NAME CMD [ARGS...]`.
    #[allow(clippy::too_many_arguments)] // CLI surface maps 1:1 onto register fields
    pub fn from_cli_parts(
        name: String,
        url: Option<String>,
        command: Option<String>,
        args: Vec<String>,
        rest: Vec<String>,
        auth: McpAuthKind,
        token: Option<String>,
        token_url: Option<String>,
        client_id: Option<String>,
        client_secret: Option<String>,
        device_authorization_url: Option<String>,
        authorization_url: Option<String>,
        redirect_url: Option<String>,
        scopes: Vec<String>,
        allow_loopback: bool,
        discover: Option<bool>,
    ) -> Result<Self, String> {
        let mut command = command;
        let mut args = args;
        if command.is_none() && url.is_none() && !rest.is_empty() {
            // Legacy: mcp add NAME CMD [ARGS...]
            let mut iter = rest.into_iter();
            command = iter.next();
            args = iter.collect();
        } else if command.is_some() && !rest.is_empty() {
            // Prefer --args; append legacy rest if provided
            args.extend(rest);
        } else if url.is_some() && !rest.is_empty() {
            return Err(
                "unexpected positional arguments after --url (use --auth / --token flags)".into(),
            );
        }

        Ok(Self {
            name,
            url,
            command,
            args,
            auth,
            token,
            token_url,
            client_id,
            client_secret,
            device_authorization_url,
            authorization_url,
            redirect_url,
            scopes,
            allow_loopback,
            discover,
            resource: None,
            issuer: None,
            iss_parameter_supported: None,
            auth_method: None,
            grant_type: None,
            use_pkce: None,
        })
    }

    /// Whether this HTTP request still needs RFC 9728 discovery to complete OAuth.
    pub fn needs_discovery(&self) -> bool {
        let has_url = self.url.as_ref().is_some_and(|u| !u.trim().is_empty());
        if !has_url {
            return false;
        }
        if matches!(self.auth, McpAuthKind::Bearer | McpAuthKind::None) {
            return false;
        }
        if self.discover == Some(false) {
            return false;
        }
        if self.discover == Some(true) {
            return true;
        }
        // Manual OAuth is complete when token_url + client_id + an interactive
        // endpoint (authorization_code or device_code) are present.
        let has_token_url = self
            .token_url
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty());
        let has_client_id = self
            .client_id
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty());
        let has_interactive = self
            .authorization_url
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty())
            || self
                .device_authorization_url
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty());
        let oauth_complete = has_token_url && has_client_id && has_interactive;
        match self.auth {
            McpAuthKind::OAuth => !oauth_complete,
            McpAuthKind::Auto => {
                // Discover unless bearer token provided or fully manual oauth.
                if self.token.as_ref().is_some_and(|t| !t.is_empty()) {
                    false
                } else {
                    !oauth_complete
                }
            }
            McpAuthKind::Bearer | McpAuthKind::None => false,
        }
    }

    /// Merge discovered OAuth settings into this request (manual flags win).
    pub fn apply_discovery(&mut self, discovered: &DiscoveredMcpOauth) {
        self.auth = McpAuthKind::OAuth;
        if self.token_url.as_ref().is_none_or(|s| s.trim().is_empty()) {
            self.token_url = Some(discovered.token_url.clone());
        }
        if self
            .authorization_url
            .as_ref()
            .is_none_or(|s| s.trim().is_empty())
        {
            self.authorization_url = Some(discovered.authorization_url.clone());
        }
        if self
            .device_authorization_url
            .as_ref()
            .is_none_or(|s| s.trim().is_empty())
        {
            self.device_authorization_url = discovered.device_authorization_url.clone();
        }
        if self
            .redirect_url
            .as_ref()
            .is_none_or(|s| s.trim().is_empty())
        {
            self.redirect_url = Some(discovered.redirect_url.clone());
        }
        if self.client_id.as_ref().is_none_or(|s| s.trim().is_empty()) {
            self.client_id = discovered.client_id.clone();
        }
        if self
            .client_secret
            .as_ref()
            .is_none_or(|s| s.trim().is_empty())
        {
            self.client_secret = discovered.client_secret.clone();
        }
        if self.scopes.is_empty() {
            self.scopes = discovered.scopes.clone();
        }
        if self.resource.as_ref().is_none_or(|s| s.trim().is_empty()) {
            self.resource = Some(discovered.resource.clone());
        }
        if self.issuer.as_ref().is_none_or(|s| s.trim().is_empty()) {
            self.issuer = Some(discovered.issuer.clone());
        }
        if self.iss_parameter_supported.is_none() {
            self.iss_parameter_supported = Some(discovered.iss_parameter_supported);
        }
        if self
            .auth_method
            .as_ref()
            .is_none_or(|s| s.trim().is_empty())
        {
            self.auth_method = Some(discovered.auth_method.clone());
        }
        if self.grant_type.as_ref().is_none_or(|s| s.trim().is_empty()) {
            self.grant_type = Some(discovered.grant_type.clone());
        }
        if self.use_pkce.is_none() {
            self.use_pkce = Some(discovered.use_pkce);
        }
    }
}

/// Outcome of a successful registration (before optional interactive login).
#[derive(Debug, Clone)]
pub struct RegisterMcpResult {
    pub name: String,
    pub transport_summary: String,
    pub auth_summary: String,
    pub next_steps: Vec<String>,
    /// True when operator should run `mcp login` next.
    pub needs_oauth_login: bool,
}

impl RegisterMcpResult {
    /// Whether OAuth login is still required before tools can be called.
    pub fn requires_login(&self) -> bool {
        self.needs_oauth_login
    }
}

/// Validate MCP HTTP URL under AE7 SSRF policy.
///
/// Control-plane registration is **stricter** than web tools: preview port
/// allowlists and session grants do **not** auto-approve loopback. Operators
/// must pass `--allow-loopback` (or TUI equivalent) for local MCP servers.
pub fn validate_mcp_http_url(url: &str, allow_loopback: bool) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("URL must not be empty".into());
    }
    let is_loopback = is_preview_loopback_url(trimmed);
    if is_loopback {
        if allow_loopback {
            return Ok(());
        }
        return Err(format!(
            "MCP URL blocked by SSRF policy: {trimmed} \
             (loopback/private hosts require --allow-loopback; \
             preview grants do not apply to MCP registration)"
        ));
    }
    match is_safe_url(trimmed) {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!(
            "MCP URL blocked by SSRF policy: {trimmed} \
             (private/cloud-metadata hosts are not allowed)"
        )),
        Err(e) => Err(format!("invalid MCP URL: {e}")),
    }
}

fn validate_server_name(name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("server name must not be empty".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(format!(
            "invalid server name '{name}' (use letters, digits, '-', '_', '.')"
        ));
    }
    Ok(())
}

/// Build a validated [`McpServerConfig`] without persisting.
pub fn build_mcp_server_config(req: &RegisterMcpRequest) -> Result<McpServerConfig, String> {
    validate_server_name(&req.name)?;

    // Legacy `mcp add NAME https://…` stored the URL in `command`. Coerce early.
    let mut url = req.url.clone();
    let mut command = req.command.clone();
    if url.as_ref().is_none_or(|u| u.trim().is_empty())
        && command
            .as_ref()
            .is_some_and(|c| edgecrab_tools::tools::mcp_client::looks_like_http_mcp_url(c))
    {
        url = command.take();
    }

    let has_url = url.as_ref().map(|u| !u.trim().is_empty()).unwrap_or(false);
    let has_command = command
        .as_ref()
        .map(|c| !c.trim().is_empty())
        .unwrap_or(false);

    if has_url && has_command {
        return Err("specify either --url or --command, not both".into());
    }
    if !has_url && !has_command {
        return Err(
            "must specify --url <endpoint> or --command <cmd> (or legacy: mcp add NAME CMD [ARGS...])"
                .into(),
        );
    }

    if has_url {
        let url = url.as_ref().map(|u| u.trim().to_string()).unwrap();
        validate_mcp_http_url(&url, req.allow_loopback)?;

        let mut cfg = McpServerConfig {
            url: Some(url.clone()),
            enabled: true,
            command: String::new(),
            args: Vec::new(),
            ..Default::default()
        };

        let auth = match req.auth {
            McpAuthKind::Auto => {
                if req.token_url.is_some()
                    || req.client_id.is_some()
                    || req.device_authorization_url.is_some()
                    || req.authorization_url.is_some()
                    || req.resource.is_some()
                {
                    McpAuthKind::OAuth
                } else if req.token.as_ref().is_some_and(|t| !t.is_empty()) {
                    McpAuthKind::Bearer
                } else {
                    McpAuthKind::None
                }
            }
            other => other,
        };

        match auth {
            McpAuthKind::OAuth => {
                let token_url = req
                    .token_url
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_default();
                if token_url.is_empty() {
                    return Err(
                        "oauth requires token_url (run with discovery, or pass --token-url)".into(),
                    );
                }
                cfg.oauth = Some(McpOauthConfig {
                    token_url,
                    grant_type: req
                        .grant_type
                        .clone()
                        .or_else(|| Some("authorization_code".into())),
                    client_id: req.client_id.clone(),
                    client_secret: req.client_secret.clone(),
                    auth_method: req.auth_method.clone().or_else(|| Some("none".into())),
                    device_authorization_url: req.device_authorization_url.clone(),
                    authorization_url: req.authorization_url.clone(),
                    redirect_url: req.redirect_url.clone().or_else(|| {
                        Some(edgecrab_tools::mcp_auth::DEFAULT_MCP_REDIRECT_URL.into())
                    }),
                    use_pkce: Some(req.use_pkce.unwrap_or(true)),
                    scopes: req.scopes.clone(),
                    audience: None,
                    resource: req.resource.clone().or_else(|| Some(url.clone())),
                    issuer: req.issuer.clone(),
                    iss_parameter_supported: req.iss_parameter_supported,
                    refresh_token: None,
                    authorization_params: HashMap::new(),
                    extra_params: HashMap::new(),
                });
            }
            McpAuthKind::Bearer => {
                if let Some(token) = req.token.as_ref().filter(|t| !t.is_empty()) {
                    cfg.bearer_token = Some(token.clone());
                }
            }
            McpAuthKind::None | McpAuthKind::Auto => {}
        }

        return Ok(cfg);
    }

    // stdio
    let command = command
        .as_ref()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .ok_or_else(|| "stdio MCP requires a non-empty --command".to_string())?;
    Ok(McpServerConfig {
        command,
        args: req.args.clone(),
        enabled: true,
        url: None,
        oauth: None,
        bearer_token: None,
        ..Default::default()
    })
}

fn transport_summary(cfg: &McpServerConfig) -> String {
    if let Some(url) = &cfg.url {
        format!("http {url}")
    } else {
        let mut s = cfg.command.clone();
        if !cfg.args.is_empty() {
            s.push(' ');
            s.push_str(&cfg.args.join(" "));
        }
        s
    }
}

fn auth_summary(cfg: &McpServerConfig) -> String {
    if cfg.oauth.is_some() {
        "oauth".into()
    } else if cfg.bearer_token.is_some() {
        "bearer".into()
    } else if cfg.url.is_some() {
        "none".into()
    } else {
        "stdio".into()
    }
}

fn next_steps(cfg: &McpServerConfig, name: &str) -> (Vec<String>, bool) {
    let mut steps = Vec::new();
    let mut needs_login = false;
    if cfg.oauth.is_some() {
        needs_login = true;
        steps.push(format!(
            "Run `edgecrab mcp login {name}` (or `/mcp login {name}`) to complete OAuth."
        ));
        steps.push(format!("Then `edgecrab mcp test {name}` to list tools."));
    } else if cfg.url.is_some() && cfg.bearer_token.is_none() {
        steps.push(format!(
            "If the server needs a token: `edgecrab mcp-token set {name} <token>` or re-add with --auth bearer --token …"
        ));
        steps.push(format!("Probe with `edgecrab mcp test {name}`."));
    } else {
        steps.push(format!(
            "Run `edgecrab mcp doctor {name}` to verify connectivity."
        ));
    }
    (steps, needs_login)
}

/// Persist server into config, optional bearer token store, reload connections.
pub fn register_mcp_server(
    config: &mut AppConfig,
    config_path: &Path,
    req: RegisterMcpRequest,
) -> Result<RegisterMcpResult, String> {
    let name = req.name.trim().to_string();
    let mut req = req;
    req.name = name.clone();

    let server_cfg = build_mcp_server_config(&req)?;

    // Prefer token store for bearer (chmod 0600 path) when token provided.
    if let Some(token) = req.token.as_ref().filter(|t| !t.is_empty())
        && server_cfg.url.is_some()
        && server_cfg.oauth.is_none()
    {
        write_mcp_token(&name, token).map_err(|e| e.to_string())?;
    }

    let mut persist_cfg = server_cfg.clone();
    // Avoid writing secrets into yaml when token store succeeded for bearer.
    if persist_cfg.oauth.is_none() && req.token.as_ref().is_some_and(|t| !t.is_empty()) {
        persist_cfg.bearer_token = None;
    }
    // Never persist client_secret in yaml when empty; keep when DCR returned one
    // only if operator explicitly configured it (still prefer env in production).
    if let Some(oauth) = persist_cfg.oauth.as_mut()
        && oauth
            .client_secret
            .as_ref()
            .is_some_and(|s| s.trim().is_empty())
    {
        oauth.client_secret = None;
    }

    config.mcp_servers.insert(name.clone(), persist_cfg.clone());
    config
        .save_to(config_path)
        .map_err(|e| format!("failed to save config: {e}"))?;
    reload_mcp_connections();

    let (next_steps, needs_oauth_login) = next_steps(&server_cfg, &name);
    Ok(RegisterMcpResult {
        name: name.clone(),
        transport_summary: transport_summary(&server_cfg),
        auth_summary: auth_summary(&server_cfg),
        next_steps,
        needs_oauth_login,
    })
}

/// Discover OAuth (when needed) then persist — shared by CLI and TUI wizard.
pub async fn prepare_and_register_mcp_url(
    config: &mut AppConfig,
    config_path: &Path,
    mut req: RegisterMcpRequest,
) -> Result<RegisterMcpResult, String> {
    if req.needs_discovery() {
        let url = req
            .url
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "discovery requires --url".to_string())?;
        validate_mcp_http_url(&url, req.allow_loopback)?;

        let force_oauth = matches!(req.auth, McpAuthKind::OAuth) || req.discover == Some(true);
        match discover_mcp_oauth(
            &url,
            DiscoverOpts {
                allow_loopback: req.allow_loopback,
                client_id: req.client_id.clone(),
                client_secret: req.client_secret.clone(),
                scopes: req.scopes.clone(),
                skip_dcr: req.client_id.as_ref().is_some_and(|s| !s.trim().is_empty()),
                prefer_offline_access: true,
                ..DiscoverOpts::default()
            },
        )
        .await
        {
            Ok(discovered) => req.apply_discovery(&discovered),
            Err(err) if !force_oauth => {
                // Auto: public MCP or non-OAuth 401 → register without oauth.
                tracing::info!(
                    %url,
                    error = %err,
                    "MCP OAuth discovery skipped; registering as public HTTP"
                );
                req.auth = McpAuthKind::None;
            }
            Err(err) => return Err(format!("OAuth discovery failed: {err}")),
        }
    } else if matches!(req.auth, McpAuthKind::Auto)
        && req.url.is_some()
        && req.token.as_ref().is_none_or(|t| t.is_empty())
        && req.token_url.as_ref().is_none_or(|s| s.trim().is_empty())
    {
        req.auth = McpAuthKind::None;
    }

    register_mcp_server(config, config_path, req)
}

/// Parse TUI `/mcp add …` tokens (already quote-split by `parse_inline_command_tokens`).
///
/// Tokens after the server name are flags (`--url`, `--auth`, …) or legacy CMD ARGS.
pub fn parse_mcp_add_tokens(tokens: &[String]) -> Result<RegisterMcpRequest, String> {
    if tokens.is_empty() {
        return Err(
            "Usage: /mcp add <name> --url <endpoint> | --command <cmd> | <cmd> [args…]".into(),
        );
    }
    let name = tokens[0].clone();
    let mut url = None;
    let mut command = None;
    let mut args = Vec::new();
    let mut rest = Vec::new();
    let mut auth = McpAuthKind::Auto;
    let mut token = None;
    let mut token_url = None;
    let mut client_id = None;
    let mut client_secret = None;
    let mut device_authorization_url = None;
    let mut authorization_url = None;
    let mut redirect_url = None;
    let mut scopes = Vec::new();
    let mut allow_loopback = false;
    let mut discover: Option<bool> = None;

    let mut i = 1;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        match t {
            "--url" => {
                i += 1;
                url = Some(
                    tokens
                        .get(i)
                        .cloned()
                        .ok_or_else(|| "--url requires a value".to_string())?,
                );
            }
            "--command" => {
                i += 1;
                command = Some(
                    tokens
                        .get(i)
                        .cloned()
                        .ok_or_else(|| "--command requires a value".to_string())?,
                );
            }
            "--args" => {
                i += 1;
                while i < tokens.len() && !tokens[i].starts_with("--") {
                    args.push(tokens[i].clone());
                    i += 1;
                }
                continue;
            }
            "--auth" => {
                i += 1;
                let v = tokens
                    .get(i)
                    .ok_or_else(|| "--auth requires a value".to_string())?;
                auth = McpAuthKind::parse(v)?;
            }
            "--token" => {
                i += 1;
                token = Some(
                    tokens
                        .get(i)
                        .cloned()
                        .ok_or_else(|| "--token requires a value".to_string())?,
                );
            }
            "--token-url" => {
                i += 1;
                token_url = Some(
                    tokens
                        .get(i)
                        .cloned()
                        .ok_or_else(|| "--token-url requires a value".to_string())?,
                );
            }
            "--client-id" => {
                i += 1;
                client_id = Some(
                    tokens
                        .get(i)
                        .cloned()
                        .ok_or_else(|| "--client-id requires a value".to_string())?,
                );
            }
            "--client-secret" => {
                i += 1;
                client_secret = Some(
                    tokens
                        .get(i)
                        .cloned()
                        .ok_or_else(|| "--client-secret requires a value".to_string())?,
                );
            }
            "--device-authorization-url" => {
                i += 1;
                device_authorization_url =
                    Some(tokens.get(i).cloned().ok_or_else(|| {
                        "--device-authorization-url requires a value".to_string()
                    })?);
            }
            "--authorization-url" => {
                i += 1;
                authorization_url = Some(
                    tokens
                        .get(i)
                        .cloned()
                        .ok_or_else(|| "--authorization-url requires a value".to_string())?,
                );
            }
            "--redirect-url" => {
                i += 1;
                redirect_url = Some(
                    tokens
                        .get(i)
                        .cloned()
                        .ok_or_else(|| "--redirect-url requires a value".to_string())?,
                );
            }
            "--scope" => {
                i += 1;
                scopes.push(
                    tokens
                        .get(i)
                        .cloned()
                        .ok_or_else(|| "--scope requires a value".to_string())?,
                );
            }
            "--allow-loopback" => {
                allow_loopback = true;
            }
            "--discover" => {
                discover = Some(true);
            }
            "--no-discover" => {
                discover = Some(false);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown flag '{other}'"));
            }
            _ => {
                rest.extend(tokens[i..].iter().cloned());
                break;
            }
        }
        i += 1;
    }

    RegisterMcpRequest::from_cli_parts(
        name,
        url,
        command,
        args,
        rest,
        auth,
        token,
        token_url,
        client_id,
        client_secret,
        device_authorization_url,
        authorization_url,
        redirect_url,
        scopes,
        allow_loopback,
        discover,
    )
}

/// Format a human summary for CLI/TUI.
pub fn format_register_summary(result: &RegisterMcpResult) -> String {
    let mut lines = vec![format!(
        "Configured MCP server '{}' ({}, auth={})",
        result.name, result.transport_summary, result.auth_summary
    )];
    if result.requires_login() {
        lines.push("  status: oauth login required".into());
    }
    for step in &result.next_steps {
        lines.push(format!("  → {step}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn build_http_oauth_config() {
        let req = RegisterMcpRequest {
            name: "linear".into(),
            url: Some("https://mcp.example.com/mcp".into()),
            command: None,
            args: vec![],
            auth: McpAuthKind::OAuth,
            token: None,
            token_url: Some("https://auth.example.com/token".into()),
            client_id: Some("cid".into()),
            client_secret: None,
            device_authorization_url: Some("https://auth.example.com/device".into()),
            authorization_url: None,
            redirect_url: None,
            scopes: vec!["mcp".into()],
            allow_loopback: false,
            discover: Some(false),
            resource: Some("https://mcp.example.com/mcp".into()),
            issuer: Some("https://auth.example.com".into()),
            iss_parameter_supported: Some(true),
            auth_method: Some("none".into()),
            grant_type: Some("authorization_code".into()),
            use_pkce: Some(true),
        };
        let cfg = build_mcp_server_config(&req).expect("build");
        assert_eq!(cfg.url.as_deref(), Some("https://mcp.example.com/mcp"));
        let oauth = cfg.oauth.expect("oauth");
        assert_eq!(oauth.token_url, "https://auth.example.com/token");
        assert_eq!(oauth.client_id.as_deref(), Some("cid"));
        assert_eq!(oauth.scopes, vec!["mcp".to_string()]);
        assert_eq!(
            oauth.resource.as_deref(),
            Some("https://mcp.example.com/mcp")
        );
        assert_eq!(oauth.issuer.as_deref(), Some("https://auth.example.com"));
    }

    #[test]
    fn build_stdio_legacy() {
        let req = RegisterMcpRequest::from_cli_parts(
            "github".into(),
            None,
            None,
            vec![],
            vec![
                "npx".into(),
                "-y".into(),
                "@modelcontextprotocol/server-github".into(),
            ],
            McpAuthKind::Auto,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            vec![],
            false,
            None,
        )
        .expect("legacy");
        let cfg = build_mcp_server_config(&req).expect("build");
        assert_eq!(cfg.command, "npx");
        assert_eq!(cfg.args, vec!["-y", "@modelcontextprotocol/server-github"]);
        assert!(cfg.url.is_none());
    }

    #[test]
    fn reject_ssrf_loopback_by_default() {
        let err = validate_mcp_http_url("http://127.0.0.1:9/mcp", false).unwrap_err();
        assert!(err.contains("SSRF"), "{err}");
    }

    #[test]
    fn allow_public_https() {
        validate_mcp_http_url("https://mcp.example.com/v1", false).expect("public ok");
    }

    #[test]
    fn register_persists_yaml() {
        let dir = tempdir().expect("tmp");
        let path = dir.path().join("config.yaml");
        let mut config = AppConfig::default();
        let req = RegisterMcpRequest {
            name: "acme".into(),
            url: Some("https://mcp.example.com/mcp".into()),
            command: None,
            args: vec![],
            auth: McpAuthKind::None,
            token: None,
            token_url: None,
            client_id: None,
            client_secret: None,
            device_authorization_url: None,
            authorization_url: None,
            redirect_url: None,
            scopes: vec![],
            allow_loopback: false,
            discover: Some(false),
            resource: None,
            issuer: None,
            iss_parameter_supported: None,
            auth_method: None,
            grant_type: None,
            use_pkce: None,
        };
        let result = register_mcp_server(&mut config, &path, req).expect("register");
        assert_eq!(result.name, "acme");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("mcp.example.com"), "{text}");
        assert!(text.contains("acme"), "{text}");
    }

    #[test]
    fn auth_kind_parse() {
        assert_eq!(McpAuthKind::parse("oauth").unwrap(), McpAuthKind::OAuth);
        assert_eq!(McpAuthKind::parse("BEARER").unwrap(), McpAuthKind::Bearer);
        assert!(McpAuthKind::parse("nope").is_err());
    }
}
