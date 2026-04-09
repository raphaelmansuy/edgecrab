use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::Utc;
use edgecrab_plugins::{
    PluginAuditEntry, append_audit_entry, clear_hub_cache, materialize_source_to_dir,
    hub_source_names, parse_plugin_manifest, read_audit_entries, resolve_install_source,
    scan_plugin_bundle, search_hub, sha256_dir, should_allow_install, write_install_metadata,
};

use crate::plugins::PluginManager;

pub enum PluginAction {
    List,
    Info {
        name: String,
    },
    Install {
        source: String,
        name: Option<String>,
        force: bool,
        no_enable: bool,
    },
    Enable {
        name: String,
    },
    Disable {
        name: String,
    },
    Toggle {
        name: String,
    },
    Status,
    Update {
        name: Option<String>,
    },
    Remove {
        name: String,
    },
    Audit {
        lines: usize,
    },
    Search {
        query: String,
        source: Option<String>,
    },
    Browse,
    Refresh,
}

pub fn run(action: PluginAction) -> anyhow::Result<()> {
    let output = run_capture(action)?;
    if !output.is_empty() {
        println!("{output}");
    }
    Ok(())
}

pub fn action_from_slash_args(args: &str) -> Option<PluginAction> {
    let trimmed = args.trim();
    if trimmed.is_empty() || matches!(trimmed, "list" | "ls") {
        return Some(PluginAction::List);
    }
    if let Some(name) = trimmed.strip_prefix("info ").map(str::trim) {
        return Some(PluginAction::Info {
            name: name.to_string(),
        });
    }
    if matches!(trimmed, "status") {
        return Some(PluginAction::Status);
    }
    if let Some(name) = trimmed.strip_prefix("enable ").map(str::trim) {
        return Some(PluginAction::Enable {
            name: name.to_string(),
        });
    }
    if let Some(name) = trimmed.strip_prefix("disable ").map(str::trim) {
        return Some(PluginAction::Disable {
            name: name.to_string(),
        });
    }
    if let Some(name) = trimmed.strip_prefix("remove ").map(str::trim) {
        return Some(PluginAction::Remove {
            name: name.to_string(),
        });
    }
    if let Some(name) = trimmed.strip_prefix("update ").map(str::trim) {
        return Some(PluginAction::Update {
            name: if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            },
        });
    }
    if trimmed == "update" {
        return Some(PluginAction::Update { name: None });
    }
    if let Some(rest) = trimmed.strip_prefix("search ").map(str::trim) {
        return Some(parse_search_action(rest));
    }
    if let Some(query) = trimmed.strip_prefix("hub search ").map(str::trim) {
        return Some(parse_search_action(query));
    }
    if matches!(trimmed, "hub" | "hub search" | "search") {
        return Some(PluginAction::Browse);
    }
    if matches!(trimmed, "hub browse" | "browse") {
        return Some(PluginAction::Browse);
    }
    if matches!(trimmed, "hub refresh" | "refresh") {
        return Some(PluginAction::Refresh);
    }
    if trimmed.starts_with("audit") {
        let lines = trimmed
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse().ok())
            .unwrap_or(20);
        return Some(PluginAction::Audit { lines });
    }
    if let Some(source) = trimmed.strip_prefix("install ").map(str::trim) {
        return Some(PluginAction::Install {
            source: source.to_string(),
            name: None,
            force: false,
            no_enable: false,
        });
    }
    None
}

pub fn run_capture(action: PluginAction) -> anyhow::Result<String> {
    match action {
        PluginAction::List => list_plugins(),
        PluginAction::Info { name } => show_plugin_info(&name),
        PluginAction::Install {
            source,
            name,
            force,
            no_enable,
        } => install_plugin(&source, name.as_deref(), force, no_enable),
        PluginAction::Enable { name } => set_plugin_enabled(&name, true),
        PluginAction::Disable { name } => set_plugin_enabled(&name, false),
        PluginAction::Toggle { name } => toggle_plugin_enabled(&name),
        PluginAction::Status => show_plugin_status(),
        PluginAction::Update { name } => update_plugins(name.as_deref()),
        PluginAction::Remove { name } => remove_plugin(&name),
        PluginAction::Audit { lines } => show_plugin_audit(lines),
        PluginAction::Search { query, source } => search_plugin_hub(&query, source.as_deref()),
        PluginAction::Browse => browse_plugin_hub(),
        PluginAction::Refresh => refresh_plugin_hub(),
    }
}

fn list_plugins() -> anyhow::Result<String> {
    let mut manager = PluginManager::new();
    manager.discover_all();
    let plugins = manager.plugins();
    if plugins.is_empty() {
        return Ok(
            "No plugins installed.\nUse `/plugins search <query>` to find remote plugins, including Hermes-compatible registries.".into(),
        );
    }

    let mut text = String::from("INSTALLED PLUGINS\n");
    text.push_str("─────────────────────────────────────────────────────────\n");
    let mut running = 0usize;
    let mut disabled = 0usize;
    for plugin in plugins {
        if plugin.enabled {
            running += 1;
        } else {
            disabled += 1;
        }
        text.push_str(&format!(
            "{:<20} v{:<8} [{}]  {:<11}  {:?}\n",
            plugin.name,
            plugin.version,
            plugin.status_label().to_ascii_uppercase(),
            plugin.kind.as_tag(),
            plugin.trust_level,
        ));
        text.push_str(&format!("  {}\n", plugin.description));
        if !plugin.tools.is_empty() {
            text.push_str(&format!(
                "  Tools: {}\n",
                plugin
                    .tools
                    .iter()
                    .map(|tool| tool.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !plugin.missing_env.is_empty() {
            text.push_str(&format!(
                "  Missing env: {}\n",
                plugin.missing_env.join(", ")
            ));
        }
        text.push('\n');
    }
    text.push_str("─────────────────────────────────────────────────────────\n");
    text.push_str(&format!(
        "{} plugins ({} running, {} disabled)",
        running + disabled,
        running,
        disabled
    ));
    Ok(text.trim_end().to_string())
}

fn show_plugin_info(name: &str) -> anyhow::Result<String> {
    let mut manager = PluginManager::new();
    manager.discover_all();
    let plugin = manager
        .plugins()
        .iter()
        .find(|plugin| plugin.name == name)
        .with_context(|| format!("plugin '{name}' not found"))?;

    let mut text = format!("PLUGIN: {}\n", plugin.name);
    text.push_str("─────────────────────────────────────────────────────────\n");
    text.push_str(&format!("Version:      {}\n", plugin.version));
    text.push_str(&format!("Kind:         {}\n", plugin.kind.as_tag()));
    text.push_str(&format!("State:        {}\n", plugin.status_label()));
    text.push_str(&format!("Trust Level:  {:?}\n", plugin.trust_level));
    text.push_str(&format!("Source:       {}\n", plugin.source));
    text.push_str(&format!("Enabled:      {}\n", plugin.enabled));
    text.push_str(&format!("Description:  {}\n", plugin.description));
    if plugin.tools.is_empty() {
        text.push_str("Tools:        none\n");
    } else {
        text.push_str(&format!(
            "Tools:        {}\n",
            plugin
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !plugin.missing_env.is_empty() {
        text.push_str(&format!(
            "Missing Env:  {}\n",
            plugin.missing_env.join(", ")
        ));
    }
    Ok(text.trim_end().to_string())
}

fn show_plugin_status() -> anyhow::Result<String> {
    let mut manager = PluginManager::new();
    manager.discover_all();
    if manager.plugins().is_empty() {
        return Ok("No plugins installed.".into());
    }

    let mut text = String::from("PLUGIN RUNTIME STATUS\n");
    text.push_str("─────────────────────────────────────────────────────────\n");
    let mut runtime_tools = 0usize;
    let mut skill_injections = 0usize;
    for plugin in manager.plugins() {
        runtime_tools += plugin.tools.len();
        if matches!(plugin.kind.as_tag(), "skill") && plugin.enabled {
            skill_injections += 1;
        }
        text.push_str(&format!(
            "{:<20} {:<16} kind={} enabled={} tools={}\n",
            plugin.name,
            plugin.status_label().to_ascii_uppercase(),
            plugin.kind.as_tag(),
            plugin.enabled,
            plugin.tools.len()
        ));
    }
    text.push_str("─────────────────────────────────────────────────────────\n");
    text.push_str(&format!(
        "Runtime tools: {} | Skill injections: {} | Total: {} plugins",
        runtime_tools,
        skill_injections,
        manager.plugins().len()
    ));
    Ok(text.trim_end().to_string())
}

fn install_plugin(
    source: &str,
    explicit_name: Option<&str>,
    force: bool,
    no_enable: bool,
) -> anyhow::Result<String> {
    let (config_path, mut config) = load_config()?;
    std::fs::create_dir_all(&config.plugins.install_dir)?;
    std::fs::create_dir_all(&config.plugins.quarantine_dir)?;

    let quarantine = config.plugins.quarantine_dir.join(format!(
        "{}-{}",
        resolve_install_source(source).plugin_name_hint,
        Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    std::fs::create_dir_all(&quarantine)?;

    let rt = tokio::runtime::Runtime::new().context("failed to create plugin install runtime")?;
    let resolved = match rt.block_on(materialize_source_to_dir(&config.plugins, source, &quarantine))
    {
        Ok(resolved) => resolved,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&quarantine);
            return Err(anyhow::anyhow!(error));
        }
    };
    let manifest_path = quarantine.join("plugin.toml");
    let manifest = parse_plugin_manifest(&manifest_path)
        .with_context(|| format!("invalid plugin manifest in {}", resolved.display))?;
    let plugin_name = explicit_name
        .map(ToString::to_string)
        .unwrap_or_else(|| manifest.plugin.name.clone());
    let target = safe_plugin_path(&config.plugins.install_dir, &plugin_name)?;
    if target.exists() {
        let _ = std::fs::remove_dir_all(&quarantine);
        anyhow::bail!(
            "plugin '{}' already exists at {}",
            plugin_name,
            target.display()
        );
    }

    let scan = scan_plugin_bundle(
        &quarantine,
        &plugin_name,
        &resolved.display,
        resolved.trust_level,
    )
    .context("plugin scan failed")?;
    let verdict = should_allow_install(
        resolved.trust_level,
        &scan,
        config.plugins.security.allow_caution,
        force,
    );
    if !verdict.allowed {
        let _ = std::fs::remove_dir_all(&quarantine);
        anyhow::bail!(render_scan_block(&scan));
    }

    let checksum = sha256_dir(&quarantine).context("failed to hash plugin bundle")?;
    if let Some(expected_checksum) = &resolved.expected_checksum
        && &checksum != expected_checksum
    {
        let _ = std::fs::remove_dir_all(&quarantine);
        anyhow::bail!(
            "checksum mismatch for '{}': expected {}, got {}",
            plugin_name,
            expected_checksum,
            checksum
        );
    }
    write_install_metadata(
        &manifest_path,
        resolved.trust_level,
        &resolved.display,
        &checksum,
    )
    .context("failed to stamp plugin manifest trust metadata")?;

    std::fs::rename(&quarantine, &target)
        .with_context(|| format!("failed to install plugin to {}", target.display()))?;

    config
        .plugins
        .disabled
        .retain(|candidate| candidate != &plugin_name);
    if no_enable {
        config.plugins.disabled.push(plugin_name.clone());
        config.plugins.disabled.sort();
        config.plugins.disabled.dedup();
    }
    config.save_to(&config_path)?;

    append_audit_entry(
        &config.plugins,
        &PluginAuditEntry {
            timestamp: Utc::now().to_rfc3339(),
            action: "install".into(),
            plugin: plugin_name.clone(),
            source: resolved.display.clone(),
            trust_level: resolved.trust_level,
            checksum: checksum.clone(),
            forced: verdict.forced,
        },
    )
    .context("failed to append plugin audit entry")?;

    let mode = if no_enable {
        "installed but disabled"
    } else {
        "installed and enabled"
    };
    Ok(format!(
        "Installing {}...\n- Source: {}\n- Security scan: {:?} ({} findings)\n- Checksum: {}\n\nPlugin '{}' {}.",
        plugin_name,
        resolved.display,
        scan.verdict,
        scan.findings.len(),
        checksum,
        plugin_name,
        mode
    ))
}

fn toggle_plugin_enabled(name: &str) -> anyhow::Result<String> {
    let (_config_path, config) = load_config()?;
    let enabled = !config
        .plugins
        .disabled
        .iter()
        .any(|candidate| candidate == name);
    set_plugin_enabled(name, !enabled)
}

fn set_plugin_enabled(name: &str, enabled: bool) -> anyhow::Result<String> {
    let (config_path, mut config) = load_config()?;
    config
        .plugins
        .disabled
        .retain(|candidate| candidate != name);
    if !enabled {
        config.plugins.disabled.push(name.to_string());
        config.plugins.disabled.sort();
        config.plugins.disabled.dedup();
    }
    config.save_to(&config_path)?;
    Ok(format!(
        "{} plugin '{}'",
        if enabled { "Enabled" } else { "Disabled" },
        name
    ))
}

fn update_plugins(name: Option<&str>) -> anyhow::Result<String> {
    let plugins_dir = user_plugins_dir()?;
    let mut messages = Vec::new();
    if let Some(name) = name {
        messages.push(update_plugin_dir(
            &safe_plugin_path(&plugins_dir, name)?,
            name,
        )?);
    } else {
        for entry in std::fs::read_dir(&plugins_dir)
            .with_context(|| format!("failed to read {}", plugins_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() || path.file_name().and_then(|name| name.to_str()) == Some(".hub") {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            messages.push(update_plugin_dir(&path, &name)?);
        }
    }
    Ok(messages.join("\n"))
}

fn update_plugin_dir(path: &Path, name: &str) -> anyhow::Result<String> {
    if !path.is_dir() {
        anyhow::bail!("plugin '{}' is not installed", name);
    }
    if !path.join(".git").exists() {
        return Ok(format!("Skipped plugin '{}' (not a git checkout)", name));
    }
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["pull", "--ff-only"])
        .status()
        .with_context(|| format!("failed to update plugin {}", name))?;
    if !status.success() {
        anyhow::bail!("git pull failed for plugin '{}'", name);
    }
    Ok(format!("Updated plugin '{}'", name))
}

fn remove_plugin(name: &str) -> anyhow::Result<String> {
    let (config_path, mut config) = load_config()?;
    let target = safe_plugin_path(&config.plugins.install_dir, name)?;
    if !target.exists() {
        anyhow::bail!("plugin '{}' is not installed", name);
    }
    let checksum = sha256_dir(&target).unwrap_or_else(|_| "sha256:unavailable".into());
    let trust_level = resolved_trust_level(name);
    std::fs::remove_dir_all(&target)
        .with_context(|| format!("failed to remove plugin {}", target.display()))?;
    config
        .plugins
        .disabled
        .retain(|candidate| candidate != name);
    config.save_to(&config_path)?;
    append_audit_entry(
        &config.plugins,
        &PluginAuditEntry {
            timestamp: Utc::now().to_rfc3339(),
            action: "remove".into(),
            plugin: name.to_string(),
            source: target.display().to_string(),
            trust_level,
            checksum,
            forced: false,
        },
    )
    .ok();
    Ok(format!("Removed plugin '{}'", name))
}

fn show_plugin_audit(lines: usize) -> anyhow::Result<String> {
    let (_config_path, config) = load_config()?;
    let entries =
        read_audit_entries(&config.plugins, lines).context("failed to read plugin audit log")?;
    if entries.is_empty() {
        return Ok("No plugin audit entries found.".into());
    }
    let mut text = String::from("PLUGIN AUDIT\n");
    text.push_str("─────────────────────────────────────────────────────────\n");
    for entry in entries {
        text.push_str(&format!(
            "{}  {}  {}  {:?}  forced={}\n",
            entry.timestamp, entry.action, entry.plugin, entry.trust_level, entry.forced
        ));
        text.push_str(&format!(
            "  source: {}\n  checksum: {}\n",
            entry.source, entry.checksum
        ));
    }
    Ok(text.trim_end().to_string())
}

fn search_plugin_hub(query: &str, source: Option<&str>) -> anyhow::Result<String> {
    let (_config_path, config) = load_config()?;
    let rt = tokio::runtime::Runtime::new().context("failed to create hub runtime")?;
    let results = rt
        .block_on(search_hub(&config.plugins, query, source, 20))
        .context("plugin hub search failed")?;
    if results.is_empty() {
        let scope = source
            .map(|value| format!(" in source '{value}'"))
            .unwrap_or_default();
        return Ok(format!("No plugin hub results for '{}'{}.", query, scope));
    }
    let mut text = String::from("PLUGIN SEARCH RESULTS\n");
    text.push_str("─────────────────────────────────────────────────────────\n");
    for result in results {
        let install_ref = format!("hub:{}/{}", result.source_name, result.plugin.name);
        text.push_str(&format!(
            "{}  v{}  {:?}  {}  source={}  score={:.1}\n",
            result.plugin.name,
            result.plugin.version,
            result.trust_level,
            result.plugin.kind.as_tag(),
            result.source_name,
            result.score
        ));
        text.push_str(&format!("  {}\n", result.plugin.description));
        text.push_str(&format!("  install: edgecrab plugins install {}\n", install_ref));
        text.push_str(&format!("  source url: {}\n", result.plugin.install_url));
        if !result.plugin.requires_env.is_empty() {
            text.push_str(&format!(
                "  requires env: {}\n",
                result.plugin.requires_env.join(", ")
            ));
        }
    }
    Ok(text.trim_end().to_string())
}

fn browse_plugin_hub() -> anyhow::Result<String> {
    let (_config_path, config) = load_config()?;
    let sources = hub_source_names(&config.plugins);
    let mut text = String::from("PLUGIN SEARCH SOURCES\n");
    text.push_str("─────────────────────────────────────────────────────────\n");
    for source in sources {
        text.push_str(&format!("- {source}\n"));
    }
    text.push_str("─────────────────────────────────────────────────────────\n");
    text.push_str("Examples:\n");
    text.push_str("  /plugins search github\n");
    text.push_str("  /plugins search --source hermes weather\n");
    text.push_str("  edgecrab plugins search github\n");
    text.push_str("  edgecrab plugins search --source hermes weather\n");
    Ok(text.trim_end().to_string())
}

fn refresh_plugin_hub() -> anyhow::Result<String> {
    let (_config_path, config) = load_config()?;
    let removed = clear_hub_cache(&config.plugins).context("failed to clear plugin hub cache")?;
    Ok(format!(
        "Cleared {} cached plugin hub index file(s).",
        removed
    ))
}

fn load_config() -> anyhow::Result<(PathBuf, edgecrab_core::AppConfig)> {
    let home =
        edgecrab_core::ensure_edgecrab_home().context("failed to initialize edgecrab home")?;
    let config_path = home.join("config.yaml");
    let config = if config_path.is_file() {
        edgecrab_core::AppConfig::load_from(&config_path)?
    } else {
        edgecrab_core::AppConfig::default()
    };
    Ok((config_path, config))
}

fn user_plugins_dir() -> anyhow::Result<PathBuf> {
    let home = edgecrab_core::edgecrab_home();
    if home.as_os_str().is_empty() {
        anyhow::bail!("cannot resolve edgecrab home directory");
    }
    Ok(home.join("plugins"))
}

fn safe_plugin_path(plugins_dir: &Path, name: &str) -> anyhow::Result<PathBuf> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        anyhow::bail!("invalid plugin name '{}'", name);
    }
    Ok(plugins_dir.join(name))
}

fn resolved_trust_level(name: &str) -> edgecrab_plugins::TrustLevel {
    let mut manager = PluginManager::new();
    manager.discover_all();
    manager
        .plugins()
        .iter()
        .find(|plugin| plugin.name == name)
        .map(|plugin| plugin.trust_level)
        .unwrap_or(edgecrab_plugins::TrustLevel::Unverified)
}

fn parse_search_action(input: &str) -> PluginAction {
    let trimmed = input.trim();
    let (source, query) = if let Some(rest) = trimmed.strip_prefix("--source ").map(str::trim) {
        match rest.split_once(' ') {
            Some((source, query)) => (Some(source.to_string()), query.trim().to_string()),
            None => (Some(rest.to_string()), String::new()),
        }
    } else {
        (None, trimmed.to_string())
    };
    PluginAction::Search { query, source }
}

fn render_scan_block(scan: &edgecrab_plugins::ScanResult) -> String {
    let mut text = format!(
        "Installation blocked. Security scan verdict: {:?}\n",
        scan.verdict
    );
    for finding in &scan.findings {
        text.push_str(&format!(
            "- {:?} {:?} {}:{} {}\n",
            finding.severity,
            finding.category,
            finding.file.display(),
            finding.line,
            finding.description
        ));
    }
    text.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_scan_block_lists_findings() {
        let scan = edgecrab_plugins::ScanResult {
            plugin_name: "demo".into(),
            source: "local".into(),
            trust_level: edgecrab_plugins::TrustLevel::Unverified,
            verdict: edgecrab_plugins::ScanVerdict::Dangerous,
            findings: vec![edgecrab_plugins::ScanFinding {
                pattern_id: "x".into(),
                severity: edgecrab_plugins::Severity::High,
                category: edgecrab_plugins::ThreatCategory::Execution,
                file: PathBuf::from("plugin.py"),
                line: 4,
                excerpt: "subprocess.run".into(),
                description: "spawns a subprocess".into(),
            }],
        };
        assert!(render_scan_block(&scan).contains("plugin.py:4"));
    }

    #[test]
    fn slash_search_supports_source_flag() {
        let action = action_from_slash_args("search --source hermes weather").expect("action");
        match action {
            PluginAction::Search { query, source } => {
                assert_eq!(query, "weather");
                assert_eq!(source.as_deref(), Some("hermes"));
            }
            other => panic!("unexpected action: {:?}", std::mem::discriminant(&other)),
        }
    }
}
