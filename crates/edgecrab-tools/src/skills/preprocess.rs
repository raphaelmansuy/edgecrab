//! SKILL.md preprocessing — Hermes/Claude template tokens + config injection.
//!
//! Safe by default: template substitution only; inline shell is opt-in via
//! `skills.inline_shell` and passes the command scanner before execution.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use edgecrab_security::command_scan::CommandScanner;
use regex::Regex;
use serde_yml::Value;
use shellexpand::tilde;

const INLINE_SHELL_MAX_OUTPUT: usize = 4000;

fn inline_shell_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"!`([^`\n]+)`").expect("inline shell regex"))
}

#[derive(Debug, Clone, Copy)]
pub struct PreprocessOptions {
    pub template_vars: bool,
    pub inline_shell: bool,
    pub inline_shell_timeout_secs: u32,
}

impl Default for PreprocessOptions {
    fn default() -> Self {
        Self {
            template_vars: true,
            inline_shell: false,
            inline_shell_timeout_secs: 10,
        }
    }
}

/// Read `skills.{template_vars,inline_shell,inline_shell_timeout}` from config.yaml.
pub fn preprocess_options_from_config(config_path: &Path) -> PreprocessOptions {
    let cfg = load_config_value(config_path);
    let Some(skills) = cfg.get("skills").and_then(|v| v.as_mapping()) else {
        return PreprocessOptions::default();
    };
    let template_vars = skills
        .get(Value::from("template_vars"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let inline_shell = skills
        .get(Value::from("inline_shell"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let inline_shell_timeout_secs = skills
        .get(Value::from("inline_shell_timeout"))
        .and_then(|v| v.as_u64())
        .unwrap_or(10)
        .clamp(1, 120) as u32;
    PreprocessOptions {
        template_vars,
        inline_shell,
        inline_shell_timeout_secs,
    }
}

static COMMAND_SCANNER: OnceLock<CommandScanner> = OnceLock::new();

fn scanner() -> &'static CommandScanner {
    COMMAND_SCANNER.get_or_init(CommandScanner::new)
}

/// Execute one `!`cmd`` snippet (Hermes parity). Failures become inline markers.
pub fn run_inline_shell(command: &str, skill_dir: &Path, timeout_secs: u32) -> String {
    let cmd = command.trim();
    if cmd.is_empty() {
        return String::new();
    }
    let scan = scanner().scan(cmd);
    if scan.is_dangerous {
        return format!("[inline-shell blocked: dangerous pattern in `{cmd}`]");
    }
    let timeout = Duration::from_secs(u64::from(timeout_secs.max(1)));
    let mut child = match Command::new("bash")
        .args(["-c", cmd])
        .current_dir(skill_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return format!("[inline-shell error: {e}]"),
    };

    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = std::io::Read::read_to_string(&mut out, &mut stdout);
                }
                if let Some(mut err) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut err, &mut stderr);
                }
                let mut output = stdout.trim_end_matches('\n').to_string();
                if output.is_empty() && !stderr.is_empty() {
                    output = stderr.trim_end_matches('\n').to_string();
                }
                if !status.success() && output.is_empty() {
                    output = format!("[inline-shell exit {}: {cmd}]", status);
                }
                if output.len() > INLINE_SHELL_MAX_OUTPUT {
                    output.truncate(INLINE_SHELL_MAX_OUTPUT);
                    output.push_str("...[truncated]");
                }
                return output;
            }
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return format!("[inline-shell timeout after {timeout_secs}s: {cmd}]");
            }
            Err(e) => return format!("[inline-shell error: {e}]"),
        }
    }
}

/// Replace every `!`cmd`` snippet with command stdout (skill dir as CWD).
pub fn expand_inline_shell(content: &str, skill_dir: &Path, timeout_secs: u32) -> String {
    if !content.contains("!`") {
        return content.to_string();
    }
    inline_shell_re()
        .replace_all(content, |caps: &regex::Captures| {
            run_inline_shell(
                caps.get(1).map(|m| m.as_str()).unwrap_or(""),
                skill_dir,
                timeout_secs,
            )
        })
        .into_owned()
}

/// Full preprocessing pipeline for SKILL.md body content.
pub fn preprocess_skill_content(
    content: &str,
    skill_dir: &Path,
    session_id: Option<&str>,
    options: PreprocessOptions,
) -> String {
    if content.is_empty() {
        return content.to_string();
    }
    let mut out = content.to_string();
    if options.template_vars {
        out = substitute_template_vars(&out, skill_dir, session_id);
    }
    if options.inline_shell {
        out = expand_inline_shell(&out, skill_dir, options.inline_shell_timeout_secs);
    }
    out
}

#[derive(Debug, Clone)]
pub(crate) struct SkillConfigVar {
    pub(crate) key: String,
    pub(crate) description: String,
    pub(crate) default: Option<String>,
}

fn parse_frontmatter_yaml(content: &str) -> Option<Value> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after = &trimmed[3..];
    let end = after.find("\n---")?;
    serde_yml::from_str(after[..end].trim()).ok()
}

fn config_var_list(node: &Value) -> Vec<SkillConfigVar> {
    let items = match node {
        Value::Sequence(seq) => seq.clone(),
        Value::Mapping(map) => vec![Value::Mapping(map.clone())],
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for item in items {
        let Some(map) = item.as_mapping() else {
            continue;
        };
        let key = map
            .get(Value::from("key"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let Some(key) = key else { continue };
        let description = map
            .get(Value::from("description"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(key)
            .to_string();
        let default = map.get(Value::from("default")).and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        });
        out.push(SkillConfigVar {
            key: key.to_string(),
            description,
            default,
        });
    }
    out
}

/// Extract `metadata.{hermes,edgecrab}.config` declarations from SKILL.md frontmatter.
pub(crate) fn extract_skill_config_vars(content: &str) -> Vec<SkillConfigVar> {
    let Some(fm) = parse_frontmatter_yaml(content) else {
        return Vec::new();
    };
    let Some(metadata) = fm.get("metadata").and_then(|v| v.as_mapping()) else {
        return Vec::new();
    };
    let mut vars = Vec::new();
    for agent_key in ["hermes", "edgecrab"] {
        if let Some(agent) = metadata
            .get(Value::from(agent_key))
            .and_then(|v| v.as_mapping())
            && let Some(config) = agent.get(Value::from("config"))
        {
            vars.extend(config_var_list(config));
        }
    }
    vars
}

fn resolve_dotpath(config: &Value, dotted_key: &str) -> Option<String> {
    let mut current = config;
    for part in dotted_key.split('.') {
        current = current.get(part)?;
    }
    match current {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn resolve_storage_key(logical_key: &str) -> String {
    format!("skills.config.{logical_key}")
}

/// Resolve declared skill config keys from `~/.edgecrab/config.yaml`.
pub(crate) fn resolve_skill_config_values(
    vars: &[SkillConfigVar],
    config_yaml: &Value,
) -> Vec<(String, String)> {
    let mut resolved = Vec::new();
    for var in vars {
        let storage = resolve_storage_key(&var.key);
        let value = resolve_dotpath(config_yaml, &storage)
            .or_else(|| var.default.clone())
            .unwrap_or_default();
        let expanded = tilde(&value).into_owned();
        resolved.push((var.key.clone(), expanded));
    }
    resolved
}

fn load_config_value(config_path: &Path) -> Value {
    if !config_path.is_file() {
        return Value::Null;
    }
    std::fs::read_to_string(config_path)
        .ok()
        .and_then(|text| serde_yml::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

/// Build `[Skill config: ...]` block for slash/skill_view injection (Hermes parity).
pub fn format_skill_config_block(skill_md: &str, config_path: &Path) -> Option<String> {
    let vars = extract_skill_config_vars(skill_md);
    if vars.is_empty() {
        return None;
    }
    let config = load_config_value(config_path);
    let resolved = resolve_skill_config_values(&vars, &config);
    if resolved.is_empty() {
        return None;
    }
    let home_display = config_path
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| config_path.display().to_string());
    let mut lines = vec![format!("[Skill config (from {home_display}/config.yaml):")];
    for (key, value) in resolved {
        let display = if value.is_empty() {
            "(not set)"
        } else {
            value.as_str()
        };
        lines.push(format!("  {key} = {display}"));
    }
    lines.push("]".into());
    Some(lines.join("\n"))
}

/// Replace `${CLAUDE,HERMES,EDGECRAB}_SKILL_DIR` and `*_SESSION_ID` tokens.
pub fn substitute_template_vars(
    content: &str,
    skill_dir: &Path,
    session_id: Option<&str>,
) -> String {
    if content.is_empty() {
        return content.to_string();
    }
    let dir_str = skill_dir.to_string_lossy();
    let mut out = content.to_string();
    for token in [
        "${CLAUDE_SKILL_DIR}",
        "${HERMES_SKILL_DIR}",
        "${EDGECRAB_SKILL_DIR}",
    ] {
        out = out.replace(token, dir_str.as_ref());
    }
    if let Some(sid) = session_id {
        for token in [
            "${CLAUDE_SESSION_ID}",
            "${HERMES_SESSION_ID}",
            "${EDGECRAB_SESSION_ID}",
        ] {
            out = out.replace(token, sid);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn substitutes_skill_dir_and_session() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("skills").join("demo");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        let body = "Run from ${HERMES_SKILL_DIR} session ${EDGECRAB_SESSION_ID}";
        let out = substitute_template_vars(body, &skill_dir, Some("sess-1"));
        assert!(out.contains(&skill_dir.to_string_lossy().to_string()));
        assert!(out.contains("sess-1"));
    }

    #[test]
    fn blocked_inline_shell_not_executed() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("demo");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        let body = "Today is !`rm -rf /`";
        let out = expand_inline_shell(body, &skill_dir, 5);
        assert!(out.contains("inline-shell blocked"));
        assert!(!out.contains("inline-shell error"));
    }

    #[cfg(unix)]
    #[test]
    fn expand_inline_shell_echo() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("demo");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        let body = "version !`echo edgecrab-test`";
        let out = expand_inline_shell(body, &skill_dir, 5);
        assert!(out.contains("edgecrab-test"), "out={out}");
    }
}
