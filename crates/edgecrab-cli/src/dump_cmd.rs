use crate::cli_args::CliArgs;
use crate::cron_cmd;
use crate::gateway_cmd;
use crate::plugins::PluginManager;
use crate::runtime::build_tool_registry;

/// Environment variables checked for API key status.
/// Source: hermes-agent `dump.py` + edgecrab-specific additions.
const API_KEY_VARS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "OPENROUTER_API_KEY",
    "GOOGLE_API_KEY",
    "MISTRAL_API_KEY",
    "DEEPSEEK_API_KEY",
    "GROQ_API_KEY",
    "XAI_API_KEY",
    "TOGETHER_API_KEY",
    "FIREWORKS_API_KEY",
    "CEREBRAS_API_KEY",
    "SAMBANOVA_API_KEY",
    "COHERE_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "AWS_ACCESS_KEY_ID",
    "TELEGRAM_BOT_TOKEN",
    "DISCORD_BOT_TOKEN",
    "SLACK_BOT_TOKEN",
    "WHATSAPP_ACCESS_TOKEN",
    "MATRIX_ACCESS_TOKEN",
    "MATTERMOST_TOKEN",
    "GITHUB_TOKEN",
];

/// Redact API key: show first 4 + last 4 chars.
/// Keys ≤12 chars are fully masked.
/// Matches hermes-agent `dump.py:_redact()`.
pub fn redact_key(key: &str) -> String {
    if key.len() <= 12 {
        return "****".to_string();
    }
    format!("{}...{}", &key[..4], &key[key.len() - 4..])
}

/// Generate compact plain-text dump for support / debugging.
///
/// `show_keys`: if true, show redacted key prefixes; otherwise just set/not-set.
///
/// Output is intentionally ANSI-free for copy-paste into Discord/GitHub.
pub fn run_dump(show_keys: bool) -> String {
    let mut out = String::new();
    out.push_str("--- edgecrab dump ---\n");

    // Section 1: Environment
    out.push_str(&format!("version:    {}\n", env!("CARGO_PKG_VERSION")));

    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    out.push_str(&format!("commit:     {commit}\n"));

    out.push_str(&format!(
        "os:         {} {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));

    // Section 2: Configuration
    let home = edgecrab_core::edgecrab_home();
    out.push_str(&format!("home:       {}\n", home.display()));

    let config_path = home.join("config.yaml");
    let config_text = std::fs::read_to_string(&config_path).ok();
    let config: Option<serde_json::Value> = config_text.as_ref().and_then(|text| {
        serde_yml::from_str::<serde_json::Value>(text).ok()
    });

    let model = config
        .as_ref()
        .and_then(|c| c.get("model"))
        .and_then(|m| m.get("default_model"))
        .and_then(|v| v.as_str())
        .unwrap_or("anthropic/claude-sonnet-4-20250514");
    out.push_str(&format!("model:      {model}\n"));

    let provider = model.split('/').next().unwrap_or("unknown");
    out.push_str(&format!("provider:   {provider}\n"));

    let backend = config
        .as_ref()
        .and_then(|c| c.get("terminal_backend"))
        .and_then(|v| v.as_str())
        .unwrap_or("local");
    out.push_str(&format!("terminal:   {backend}\n"));

    // Section 3: API Keys
    out.push('\n');
    out.push_str("api_keys:\n");
    let max_name_len = API_KEY_VARS.iter().map(|v| v.len()).max().unwrap_or(20);
    for var in API_KEY_VARS {
        let status = match std::env::var(var) {
            Ok(val) if !val.is_empty() => {
                if show_keys {
                    format!("set  ({})", redact_key(&val))
                } else {
                    "set".to_string()
                }
            }
            _ => "not set".to_string(),
        };
        out.push_str(&format!("  {:<width$} {status}\n", format!("{var}:"), width = max_name_len + 1));
    }

    // Section 4: Features
    out.push('\n');
    out.push_str("features:\n");

    // Toolsets
    let tools = build_tool_registry();
    let toolset_names: Vec<&str> = tools.toolset_names();
    out.push_str(&format!(
        "  toolsets:     {}\n",
        if toolset_names.is_empty() {
            "(none)".to_string()
        } else {
            toolset_names.join(", ")
        }
    ));

    // MCP servers
    let mcp_count = config
        .as_ref()
        .and_then(|c| c.get("mcp_servers"))
        .and_then(|v| v.as_object())
        .map(|m| m.len())
        .unwrap_or(0);
    out.push_str(&format!("  mcp_servers:  {mcp_count} configured\n"));

    // Gateway
    let gateway_running = gateway_cmd::snapshot().map(|g| g.running).unwrap_or(false);
    out.push_str(&format!(
        "  gateway:      {}\n",
        if gateway_running { "active" } else { "inactive" }
    ));

    // Platforms
    let platforms = config
        .as_ref()
        .and_then(|c| c.get("gateway"))
        .and_then(|g| g.get("enabled_platforms"))
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_else(|| "(none)".to_string());
    out.push_str(&format!("  platforms:    {platforms}\n"));

    // Cron
    let (cron_active, cron_total) = cron_cmd::status_snapshot()
        .map(|c| (c.active_jobs, c.total_jobs))
        .unwrap_or((0, 0));
    out.push_str(&format!(
        "  cron_jobs:    {cron_active} active / {cron_total} total\n"
    ));

    // Skills
    let skills_dir = home.join("skills");
    let skill_count = std::fs::read_dir(&skills_dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| {
                    e.path()
                        .extension()
                        .is_some_and(|ext| ext == "md")
                })
                .count()
        })
        .unwrap_or(0);
    out.push_str(&format!("  skills:       {skill_count} installed\n"));

    // Plugins
    let mut plugins = PluginManager::new();
    plugins.discover_all();
    out.push_str(&format!("  plugins:      {} loaded\n", plugins.plugins().len()));

    // Memory provider
    out.push_str("  memory:       built-in\n");

    // Section 5: Config Overrides
    if let Some(ref cfg) = config {
        let overrides = detect_config_overrides(cfg);
        if !overrides.is_empty() {
            out.push('\n');
            out.push_str("config_overrides:\n");
            for (path, value, default) in &overrides {
                out.push_str(&format!("  {path:<30} {value}  (default: {default})\n"));
            }
        }
    }

    out.push_str("--- end dump ---\n");
    out
}

/// Config paths to diff against defaults.
const INTERESTING_OVERRIDES: &[(&str, &[&str], &str)] = &[
    ("max_iterations", &["max_iterations"], "90"),
    ("streaming", &["streaming"], "true"),
    ("save_trajectories", &["save_trajectories"], "false"),
    ("skip_context_files", &["skip_context_files"], "false"),
    ("skip_memory", &["skip_memory"], "false"),
    ("compression.threshold", &["compression", "threshold"], "0.5"),
    (
        "compression.protect_last_n",
        &["compression", "protect_last_n"],
        "20",
    ),
    ("display.skin", &["display", "skin"], "default"),
    ("agent.gateway_timeout", &["agent", "gateway_timeout"], "120"),
    ("terminal.backend", &["terminal", "backend"], "native"),
    ("terminal.docker_image", &["terminal", "docker_image"], ""),
    ("terminal.persistent_shell", &["terminal", "persistent_shell"], "true"),
    ("browser.allow_private_urls", &["browser", "allow_private_urls"], "false"),
    ("smart_model_routing.enabled", &["smart_model_routing", "enabled"], "false"),
    ("privacy.redact_pii", &["privacy", "redact_pii"], "false"),
];

fn detect_config_overrides(config: &serde_json::Value) -> Vec<(String, String, String)> {
    let mut overrides = Vec::new();
    for (label, path, default) in INTERESTING_OVERRIDES {
        let mut node = config;
        let mut found = true;
        for key in *path {
            match node.get(key) {
                Some(v) => node = v,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if !found {
            continue;
        }
        let value = match node {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            other => other.to_string(),
        };
        if value != *default {
            overrides.push((label.to_string(), value, default.to_string()));
        }
    }
    overrides
}

/// CLI subcommand entry point.
pub fn run(_args: &CliArgs, all: bool) -> anyhow::Result<()> {
    let show_keys = all; // --all flag maps to showing redacted keys
    let output = run_dump(show_keys);
    print!("{output}");
    Ok(())
}

#[allow(dead_code)]
fn format_timestamp(ts: Option<i64>) -> String {
    ts.and_then(|value| chrono::DateTime::<chrono::Utc>::from_timestamp(value, 0))
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "-".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_normal_key() {
        let r = redact_key("sk-abcdef1234567890WXYZ");
        assert!(r.starts_with("sk-a"));
        assert!(r.ends_with("WXYZ"));
        assert!(r.contains("..."));
        // Full key must NOT appear
        assert!(!r.contains("sk-abcdef1234567890WXYZ"));
    }

    #[test]
    fn redact_short_key() {
        assert_eq!(redact_key("short"), "****");
        assert_eq!(redact_key("exactly12ch"), "****");
    }

    #[test]
    fn redact_empty_key() {
        assert_eq!(redact_key(""), "****");
    }

    #[test]
    fn dump_output_has_markers() {
        let output = run_dump(false);
        assert!(output.starts_with("--- edgecrab dump ---\n"));
        assert!(output.ends_with("--- end dump ---\n"));
    }

    #[test]
    fn dump_no_full_keys_in_output() {
        // Set a test key and ensure it never appears in full
        let test_key = "sk-test-this-is-a-fake-key-12345";
        unsafe { std::env::set_var("ANTHROPIC_API_KEY", test_key) };
        let output = run_dump(false);
        assert!(
            !output.contains(test_key),
            "Full API key should never appear in dump output"
        );
        unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };
    }

    #[test]
    fn dump_show_keys_includes_redacted() {
        let test_key = "sk-test-redacted-key-abcdef";
        unsafe { std::env::set_var("ANTHROPIC_API_KEY", test_key) };
        let output = run_dump(true);
        assert!(output.contains("sk-t"), "Should show first 4 chars");
        assert!(output.contains("cdef"), "Should show last 4 chars");
        assert!(
            !output.contains(test_key),
            "Full key must never appear"
        );
        unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };
    }

    #[test]
    fn dump_contains_version() {
        let output = run_dump(false);
        assert!(output.contains("version:"));
    }

    #[test]
    fn dump_contains_api_keys_section() {
        let output = run_dump(false);
        assert!(output.contains("api_keys:"));
        assert!(output.contains("ANTHROPIC_API_KEY:"));
    }

    #[test]
    fn detect_overrides_empty_when_defaults() {
        let config = serde_json::json!({
            "max_iterations": 90,
            "streaming": true,
            "save_trajectories": false,
        });
        let overrides = detect_config_overrides(&config);
        assert!(overrides.is_empty());
    }

    #[test]
    fn detect_overrides_finds_non_default() {
        let config = serde_json::json!({
            "max_iterations": 120,
        });
        let overrides = detect_config_overrides(&config);
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].0, "max_iterations");
        assert_eq!(overrides[0].1, "120");
        assert_eq!(overrides[0].2, "90");
    }
}
