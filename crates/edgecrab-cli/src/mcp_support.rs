//! Shared MCP operator helpers for CLI and TUI.
//!
//! WHY this module exists: MCP command parsing and operator diagnostics are
//! needed in both `main.rs` and `app.rs`. Keeping the logic here avoids
//! duplicated parsing quirks and keeps the TUI and CLI behavior aligned.

use std::path::{Path, PathBuf};

use edgecrab_tools::tools::mcp_client::{
    ConfiguredMcpServer, configured_servers, probe_configured_server,
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
