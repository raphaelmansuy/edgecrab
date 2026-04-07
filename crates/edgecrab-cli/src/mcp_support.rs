//! Shared MCP operator helpers for CLI and TUI.
//!
//! WHY this module exists: MCP command parsing and operator diagnostics are
//! needed in both `main.rs` and `app.rs`. Keeping the logic here avoids
//! duplicated parsing quirks and keeps the TUI and CLI behavior aligned.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use edgecrab_tools::tools::mcp_client::{
    ConfiguredMcpServer, configured_servers, probe_configured_server, read_mcp_token_status,
};
use edgecrab_types::ToolError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum McpDoctorStatus {
    Pass,
    Warn,
    Fail,
}

impl McpDoctorStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug)]
struct StaticMcpReport {
    status: McpDoctorStatus,
    lines: Vec<String>,
}

pub fn parse_inline_command_tokens(input: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if let Some(active_quote) = quote {
            if ch == '\\' {
                if let Some(&next) = chars.peek() {
                    if next == active_quote || next == '\\' {
                        current.push(next);
                        let _ = chars.next();
                        continue;
                    }
                }
                current.push(ch);
                continue;
            }
            if ch == active_quote {
                quote = None;
                continue;
            }
            current.push(ch);
            continue;
        }

        match ch {
            '\'' | '"' => {
                quote = Some(ch);
            }
            c if c.is_whitespace() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if quote.is_some() {
        return Err("Unterminated quote in MCP command.".into());
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    Ok(tokens)
}

pub fn parse_named_option(
    tokens: &[String],
    name: &str,
) -> Result<(Option<String>, Vec<String>), String> {
    let inline_prefix = format!("{name}=");
    let long_flag = format!("--{name}");
    let mut value = None;
    let mut remaining = Vec::new();
    let mut idx = 0usize;

    while idx < tokens.len() {
        let token = &tokens[idx];
        if let Some(inline) = token.strip_prefix(&inline_prefix) {
            value = Some(inline.to_string());
            idx += 1;
            continue;
        }
        if token == &long_flag {
            let Some(next) = tokens.get(idx + 1) else {
                return Err(format!("Missing value for {long_flag}."));
            };
            value = Some(next.clone());
            idx += 2;
            continue;
        }
        remaining.push(token.clone());
        idx += 1;
    }

    Ok((value, remaining))
}

pub fn transport_summary(server: &ConfiguredMcpServer) -> String {
    if let Some(url) = &server.url {
        return format!("http {url}");
    }

    let mut rendered = server.command.clone();
    if !server.args.is_empty() {
        rendered.push(' ');
        rendered.push_str(&server.args.join(" "));
    }
    rendered
}

pub fn auth_summary(server: &ConfiguredMcpServer) -> String {
    if has_authorization_header(server) {
        return "authorization-header".into();
    }
    if let Some(oauth) = &server.oauth {
        return format!("oauth2/{}", oauth.grant_type_label());
    }
    if server.token_from_store {
        return "token-store".into();
    }
    if server
        .bearer_token
        .as_deref()
        .is_some_and(|token| !token.trim().is_empty())
    {
        return "config-bearer-token".into();
    }
    if !server.headers.is_empty() {
        return format!("custom-headers({})", server.headers.len());
    }
    "none".into()
}

fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
}

fn format_oauth_cache_state(server_name: &str) -> Option<String> {
    let token = read_mcp_token_status(server_name)?;
    let expiry = match token.expires_at_epoch_secs {
        Some(expiry) if expiry <= current_epoch_secs() => "expired".to_string(),
        Some(expiry) => format!("expires-at={expiry}"),
        None => "no-expiry".into(),
    };
    let refresh = if token.has_refresh_token {
        "refresh=yes"
    } else {
        "refresh=no"
    };
    Some(format!(
        "oauth-cache: access-token=yes | {refresh} | {expiry}"
    ))
}

fn contains_unresolved_env_template(value: &str) -> bool {
    if value.contains("${") {
        return true;
    }

    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '$' {
            continue;
        }
        if chars
            .peek()
            .copied()
            .is_some_and(|next| next == '_' || next.is_ascii_alphabetic())
        {
            return true;
        }
    }
    false
}

pub async fn render_mcp_doctor_report(server_name: Option<&str>) -> Result<String, ToolError> {
    let servers = configured_servers()?;
    if servers.is_empty() {
        return Ok("No MCP servers configured.".into());
    }

    let selected: Vec<ConfiguredMcpServer> = if let Some(server_name) = server_name {
        let server = servers
            .into_iter()
            .find(|server| server.name == server_name)
            .ok_or_else(|| ToolError::InvalidArgs {
                tool: "mcp_doctor".into(),
                message: format!("Unknown MCP server '{server_name}'"),
            })?;
        vec![server]
    } else {
        servers
    };

    let mut pass = 0usize;
    let mut warn = 0usize;
    let mut fail = 0usize;
    let mut out = vec![format!(
        "MCP Doctor — {} configured server(s)",
        selected.len()
    )];

    for server in selected {
        let mut report = analyze_server(&server);
        match probe_configured_server(&server.name).await {
            Ok(result) => {
                if result.tool_count == 0 {
                    report.status = report.status.max(McpDoctorStatus::Warn);
                    report.lines.push("probe: ok | visible-tools=0".to_string());
                } else {
                    report.lines.push(format!(
                        "probe: ok | transport={} | visible-tools={}",
                        result.transport, result.tool_count
                    ));
                }
                let sample_tools = result
                    .tools
                    .iter()
                    .take(3)
                    .map(|(name, _)| name.as_str())
                    .collect::<Vec<_>>();
                if !sample_tools.is_empty() {
                    report
                        .lines
                        .push(format!("sample-tools: {}", sample_tools.join(", ")));
                }
            }
            Err(err) => {
                report.status = McpDoctorStatus::Fail;
                report.lines.push(format!("probe: fail | {err}"));
            }
        }

        match report.status {
            McpDoctorStatus::Pass => pass += 1,
            McpDoctorStatus::Warn => warn += 1,
            McpDoctorStatus::Fail => fail += 1,
        }

        out.push(String::new());
        out.push(format!("{}  {}", server.name, report.status.label()));
        for line in report.lines {
            out.push(format!("  {line}"));
        }
    }

    out.push(String::new());
    out.push(format!("Summary: pass={pass} warn={warn} fail={fail}"));
    Ok(out.join("\n"))
}

pub fn render_mcp_auth_guide(server_name: &str) -> anyhow::Result<String> {
    let server = configured_servers()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?
        .into_iter()
        .find(|server| server.name == server_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown MCP server '{server_name}'"))?;

    let mut lines = vec![
        format!("MCP Auth — {}", server.name),
        format!("transport: {}", transport_summary(&server)),
        format!("auth: {}", auth_summary(&server)),
    ];

    if server.url.is_none() {
        lines
            .push("This server uses stdio. HTTP bearer-token and OAuth flows do not apply.".into());
        return Ok(lines.join("\n"));
    }

    if let Some(url) = &server.url {
        lines.push(format!("url: {url}"));
    }

    if let Some(oauth) = &server.oauth {
        let auth_method = oauth.auth_method_label();
        lines.push(format!("oauth: token-url={}", oauth.token_url()));
        lines.push(format!(
            "oauth: grant={} auth-method={}",
            oauth.grant_type_label(),
            auth_method
        ));

        let cache_status = read_mcp_token_status(&server.name);
        if let Some(cache) = format_oauth_cache_state(&server.name) {
            lines.push(cache);
        }

        let has_cached_access = cache_status.is_some_and(|status| status.has_access_token);
        let has_refresh = oauth.has_refresh_token()
            || cache_status.is_some_and(|status| status.has_refresh_token);
        let mut next_steps = Vec::new();

        if oauth.token_url().trim().is_empty() {
            next_steps.push(
                "Add `oauth.token_url` in `~/.edgecrab/config.yaml`; OAuth cannot start without a token endpoint."
                    .to_string(),
            );
        }

        match oauth.grant_type_label() {
            "client_credentials" => {
                if auth_method != "none" && !oauth.has_client_id() {
                    next_steps
                        .push("Add `oauth.client_id` in `~/.edgecrab/config.yaml`.".to_string());
                }
                if auth_method != "none" && !oauth.has_client_secret() {
                    next_steps.push(
                        "Add `oauth.client_secret` in `~/.edgecrab/config.yaml` for the client-credentials flow.".to_string(),
                    );
                }
                if next_steps.is_empty() && !has_cached_access {
                    next_steps.push(format!(
                        "Run `/mcp test {}` to fetch and cache an access token before the next real tool call.",
                        server.name
                    ));
                } else if next_steps.is_empty() {
                    next_steps.push(
                        "EdgeCrab will refresh the access token automatically on expiry or after a 401."
                            .to_string(),
                    );
                }
            }
            "refresh_token" => {
                if !has_refresh {
                    next_steps.push(format!(
                        "Store a refresh token with `/mcp-token set-refresh {} <refresh-token>` or add `oauth.refresh_token` in `~/.edgecrab/config.yaml`.",
                        server.name
                    ));
                } else if !has_cached_access {
                    next_steps.push(format!(
                        "Run `/mcp test {}` to exchange the refresh token for an access token and warm the cache.",
                        server.name
                    ));
                } else {
                    next_steps.push(
                        "Refresh-token OAuth is ready. EdgeCrab can renew the access token automatically."
                            .to_string(),
                    );
                }
            }
            "auto" => {
                if has_refresh {
                    if !has_cached_access {
                        next_steps.push(format!(
                            "Auto mode detected a refresh token. Run `/mcp test {}` once to exchange it for an access token.",
                            server.name
                        ));
                    } else {
                        next_steps.push(
                            "Auto mode will prefer the refresh-token path and renew tokens automatically."
                                .to_string(),
                        );
                    }
                } else if auth_method != "none"
                    && oauth.has_client_id()
                    && oauth.has_client_secret()
                {
                    if !has_cached_access {
                        next_steps.push(format!(
                            "Auto mode will fall back to client credentials. Run `/mcp test {}` to fetch the first access token.",
                            server.name
                        ));
                    } else {
                        next_steps.push(
                            "Auto mode can refresh with client credentials when the cached access token expires."
                                .to_string(),
                        );
                    }
                } else {
                    next_steps.push(
                        "Auto mode needs either a refresh token or usable client credentials; the current config does not provide either."
                            .to_string(),
                    );
                }
            }
            _ => {}
        }

        lines.push("next:".into());
        for step in next_steps {
            lines.push(format!("- {step}"));
        }
        return Ok(lines.join("\n"));
    }

    if server.token_from_store {
        lines.push("A stored bearer token is present in `~/.edgecrab/mcp-tokens/`.".into());
        lines.push("next:".into());
        lines.push(format!(
            "- Run `/mcp test {}` if you want to validate the token immediately.",
            server.name
        ));
        lines.push(format!(
            "- Use `/mcp-token remove {}` to revoke the local cached token.",
            server.name
        ));
        return Ok(lines.join("\n"));
    }

    if server.token_from_config {
        lines.push("A static bearer token is configured in `~/.edgecrab/config.yaml`.".into());
        lines.push("next:".into());
        lines.push(format!(
            "- Run `/mcp test {}` to validate the configured token.",
            server.name
        ));
        return Ok(lines.join("\n"));
    }

    if has_authorization_header(&server) {
        lines.push("Authorization is injected through custom HTTP headers.".into());
        lines.push("next:".into());
        lines.push(
            "- Verify the custom header value is current; EdgeCrab will forward it exactly as configured."
                .into(),
        );
        return Ok(lines.join("\n"));
    }

    lines.push("No HTTP auth is configured yet.".into());
    lines.push("next:".into());
    lines.push(format!(
        "- Use `/mcp-token set {} <bearer-token>` for a static bearer token.",
        server.name
    ));
    lines.push(
        "- Or add `bearer_token`, `headers.Authorization`, or an `oauth` block in `~/.edgecrab/config.yaml`."
            .into(),
    );
    Ok(lines.join("\n"))
}

fn analyze_server(server: &ConfiguredMcpServer) -> StaticMcpReport {
    let mut status = McpDoctorStatus::Pass;
    let mut lines = Vec::new();

    lines.push(format!("transport: {}", transport_summary(server)));

    if let Some(url) = &server.url {
        match reqwest::Url::parse(url) {
            Ok(parsed) => lines.push(format!("url: ok | scheme={}", parsed.scheme())),
            Err(err) => {
                status = McpDoctorStatus::Fail;
                lines.push(format!("url: invalid | {err}"));
            }
        }

        let auth = auth_summary(server);
        if auth == "none" {
            status = status.max(McpDoctorStatus::Warn);
            lines.push("auth: none configured".into());
        } else {
            lines.push(format!("auth: {auth}"));
        }

        if let Some(oauth) = &server.oauth {
            if oauth.token_url().trim().is_empty() {
                status = McpDoctorStatus::Fail;
                lines.push("oauth: token_url is missing".into());
            } else if contains_unresolved_env_template(oauth.token_url()) {
                status = status.max(McpDoctorStatus::Warn);
                lines.push("oauth: token_url contains an unresolved env placeholder".into());
            } else {
                lines.push(format!("oauth: token-url={}", oauth.token_url()));
            }

            lines.push(format!(
                "oauth: grant={} auth-method={}",
                oauth.grant_type_label(),
                oauth.auth_method_label()
            ));

            if oauth.auth_method_label() != "none" && !oauth.has_client_id() {
                status = status.max(McpDoctorStatus::Warn);
                lines.push("oauth: client_id is missing".into());
            }
            if oauth.auth_method_label() != "none"
                && oauth.grant_type_label() == "client_credentials"
                && !oauth.has_client_secret()
            {
                status = status.max(McpDoctorStatus::Warn);
                lines.push("oauth: client_secret is missing for client-credentials flow".into());
            }
            if oauth.grant_type_label() == "refresh_token"
                && !oauth.has_refresh_token()
                && read_mcp_token_status(&server.name)
                    .is_none_or(|status| !status.has_refresh_token)
            {
                status = status.max(McpDoctorStatus::Warn);
                lines.push("oauth: refresh_token grant selected but no refresh token is configured or cached".into());
            }
            if let Some(cache) = format_oauth_cache_state(&server.name) {
                lines.push(cache);
            } else if oauth.has_refresh_token() {
                lines.push("oauth-cache: no stored access token yet | refresh=yes".into());
            }
        }

        if server
            .bearer_token
            .as_deref()
            .is_some_and(|token| token.trim().is_empty())
        {
            status = status.max(McpDoctorStatus::Warn);
            lines.push("auth: config bearer_token is blank".into());
        }
        if server
            .bearer_token
            .as_deref()
            .is_some_and(contains_unresolved_env_template)
        {
            status = status.max(McpDoctorStatus::Warn);
            lines.push("auth: config bearer_token contains an unresolved env placeholder".into());
        }
        if server
            .headers
            .values()
            .any(|value| contains_unresolved_env_template(value))
        {
            status = status.max(McpDoctorStatus::Warn);
            lines.push("auth: one or more HTTP headers contain unresolved env placeholders".into());
        }
        if has_authorization_header(server)
            && (server.token_from_store
                || server
                    .bearer_token
                    .as_deref()
                    .is_some_and(|token| !token.trim().is_empty()))
        {
            status = status.max(McpDoctorStatus::Warn);
            lines.push("auth: Authorization header overrides bearer token settings".into());
        }
        if !server.command.trim().is_empty() {
            status = status.max(McpDoctorStatus::Warn);
            lines.push("transport: url is set, so stdio command is ignored".into());
        }
    } else if server.command.trim().is_empty() {
        status = McpDoctorStatus::Fail;
        lines.push("command: missing".into());
    } else {
        match resolve_stdio_command(server) {
            Ok(path) => lines.push(format!("command: ok | {}", path.display())),
            Err(detail) => {
                status = McpDoctorStatus::Fail;
                lines.push(format!("command: fail | {detail}"));
            }
        }
    }

    if let Some(cwd) = &server.cwd {
        if !cwd.exists() {
            status = McpDoctorStatus::Fail;
            lines.push(format!("cwd: missing | {}", cwd.display()));
        } else if !cwd.is_dir() {
            status = McpDoctorStatus::Fail;
            lines.push(format!("cwd: not-a-directory | {}", cwd.display()));
        } else {
            lines.push(format!("cwd: ok | {}", cwd.display()));
        }
    }

    if !server.include.is_empty() || !server.exclude.is_empty() {
        let include = if server.include.is_empty() {
            "*".to_string()
        } else {
            server.include.join(",")
        };
        let exclude = if server.exclude.is_empty() {
            "-".to_string()
        } else {
            server.exclude.join(",")
        };
        lines.push(format!("filters: include={include} exclude={exclude}"));
    }
    if !server.include.is_empty() && !server.exclude.is_empty() {
        status = status.max(McpDoctorStatus::Warn);
        lines.push("filters: include and exclude both set; include wins".into());
    }

    if let Some(timeout) = server.timeout {
        lines.push(format!("timeout: {timeout}s"));
    }
    if let Some(timeout) = server.connect_timeout {
        lines.push(format!("connect-timeout: {timeout}s"));
    }

    StaticMcpReport { status, lines }
}

fn has_authorization_header(server: &ConfiguredMcpServer) -> bool {
    server
        .headers
        .keys()
        .any(|key| key.eq_ignore_ascii_case("authorization"))
}

fn resolve_stdio_command(server: &ConfiguredMcpServer) -> Result<PathBuf, String> {
    let command = server.command.trim();
    if command.is_empty() {
        return Err("missing command".into());
    }

    if is_path_like_command(command) {
        let path = resolve_path_like_command(Path::new(command), server.cwd.as_deref());
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!("path '{}' does not exist", path.display()));
    }

    which::which(command).map_err(|_| format!("command '{command}' not found on PATH"))
}

fn is_path_like_command(command: &str) -> bool {
    let path = Path::new(command);
    path.is_absolute()
        || command.contains('/')
        || command.contains('\\')
        || command.starts_with('.')
}

fn resolve_path_like_command(command: &Path, cwd: Option<&Path>) -> PathBuf {
    if command.is_absolute() {
        return command.to_path_buf();
    }
    if let Some(cwd) = cwd {
        return cwd.join(command);
    }
    std::env::current_dir()
        .map(|dir| dir.join(command))
        .unwrap_or_else(|_| command.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn configured_server(command: &str) -> ConfiguredMcpServer {
        ConfiguredMcpServer {
            name: "test".into(),
            url: None,
            bearer_token: None,
            oauth: None,
            command: command.into(),
            args: Vec::new(),
            cwd: None,
            env: HashMap::new(),
            headers: HashMap::new(),
            timeout: None,
            connect_timeout: None,
            include: Vec::new(),
            exclude: Vec::new(),
            token_from_config: false,
            token_from_store: false,
        }
    }

    #[test]
    fn parse_inline_command_tokens_preserves_quoted_spaces() {
        let tokens = parse_inline_command_tokens(
            r#"install filesystem --path "/Users/raphael/My Project" name="local fs""#,
        )
        .expect("tokens");

        assert_eq!(
            tokens,
            vec![
                "install",
                "filesystem",
                "--path",
                "/Users/raphael/My Project",
                "name=local fs",
            ]
        );
    }

    #[test]
    fn parse_inline_command_tokens_preserves_windows_backslashes() {
        let tokens =
            parse_inline_command_tokens(r#"install filesystem path="C:\Users\Raphael\My Project""#)
                .expect("tokens");

        assert_eq!(
            tokens,
            vec![
                "install",
                "filesystem",
                r#"path=C:\Users\Raphael\My Project"#,
            ]
        );
    }

    #[test]
    fn parse_inline_command_tokens_rejects_unterminated_quotes() {
        let err = parse_inline_command_tokens(r#"install filesystem --path "unterminated"#)
            .expect_err("expected parse error");
        assert!(err.contains("Unterminated quote"));
    }

    #[test]
    fn parse_named_option_supports_inline_and_long_forms() {
        let tokens = vec![
            "install".to_string(),
            "filesystem".to_string(),
            "--path".to_string(),
            "/tmp/workspace".to_string(),
            "name=local fs".to_string(),
        ];

        let (path, remaining) = parse_named_option(&tokens, "path").expect("path");
        let (name, remaining) = parse_named_option(&remaining, "name").expect("name");

        assert_eq!(path.as_deref(), Some("/tmp/workspace"));
        assert_eq!(name.as_deref(), Some("local fs"));
        assert_eq!(remaining, vec!["install", "filesystem"]);
    }

    #[test]
    fn analyze_server_warns_when_url_and_command_are_both_set() {
        let mut server = configured_server("npx");
        server.url = Some("https://example.com/mcp".into());

        let report = analyze_server(&server);

        assert_eq!(report.status, McpDoctorStatus::Warn);
        assert!(
            report
                .lines
                .iter()
                .any(|line| line.contains("stdio command is ignored"))
        );
    }

    #[test]
    fn auth_summary_prefers_authorization_header() {
        let mut server = configured_server("npx");
        server
            .headers
            .insert("Authorization".into(), "Bearer token".into());
        server.token_from_store = true;

        assert_eq!(auth_summary(&server), "authorization-header");
    }

    #[test]
    fn contains_unresolved_env_template_detects_common_forms() {
        assert!(contains_unresolved_env_template("${MCP_TOKEN}"));
        assert!(contains_unresolved_env_template("$MCP_TOKEN"));
        assert!(!contains_unresolved_env_template("Bearer token"));
    }
}
