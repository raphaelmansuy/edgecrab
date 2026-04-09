use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::plugins::PluginManager;

pub enum PluginAction {
    List,
    Info { name: String },
    Install { repo: String, name: Option<String> },
    Enable { name: String },
    Disable { name: String },
    Toggle { name: String },
    Status,
    Update { name: String },
    Remove { name: String },
}

pub fn run(action: PluginAction) -> anyhow::Result<()> {
    match action {
        PluginAction::List => list_plugins(),
        PluginAction::Info { name } => show_plugin_info(&name),
        PluginAction::Install { repo, name } => install_plugin(&repo, name.as_deref()),
        PluginAction::Enable { name } => set_plugin_enabled(&name, true),
        PluginAction::Disable { name } => set_plugin_enabled(&name, false),
        PluginAction::Toggle { name } => toggle_plugin_enabled(&name),
        PluginAction::Status => show_plugin_status(),
        PluginAction::Update { name } => update_plugin(&name),
        PluginAction::Remove { name } => remove_plugin(&name),
    }
}

fn list_plugins() -> anyhow::Result<()> {
    let mut manager = PluginManager::new();
    manager.discover_all();
    let plugins = manager.plugins();
    if plugins.is_empty() {
        println!("No plugins discovered.");
        return Ok(());
    }

    println!("INSTALLED PLUGINS");
    println!("─────────────────────────────────────────────────────────");
    for plugin in plugins {
        println!(
            "{}  v{}  [{}]  {}  {:?}",
            plugin.name,
            plugin.version,
            plugin.status_label().to_ascii_uppercase(),
            plugin.kind.as_tag(),
            plugin.trust_level,
        );
        println!("  {}", plugin.description);
        if !plugin.tools.is_empty() {
            println!(
                "  Tools: {}",
                plugin
                    .tools
                    .iter()
                    .map(|tool| tool.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !plugin.missing_env.is_empty() {
            println!("  Missing env: {}", plugin.missing_env.join(", "));
        }
        println!();
    }
    Ok(())
}

fn show_plugin_info(name: &str) -> anyhow::Result<()> {
    let mut manager = PluginManager::new();
    manager.discover_all();
    let plugin = manager
        .plugins()
        .iter()
        .find(|plugin| plugin.name == name)
        .with_context(|| format!("plugin '{name}' not found"))?;

    println!("PLUGIN: {}", plugin.name);
    println!("─────────────────────────────────────────────────────────");
    println!("Version:      {}", plugin.version);
    println!("Kind:         {}", plugin.kind.as_tag());
    println!("State:        {}", plugin.status_label());
    println!("Trust Level:  {:?}", plugin.trust_level);
    println!("Source:       {}", plugin.source);
    println!("Enabled:      {}", plugin.enabled);
    println!("Description:  {}", plugin.description);
    if plugin.tools.is_empty() {
        println!("Tools:        none");
    } else {
        println!(
            "Tools:        {}",
            plugin
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !plugin.missing_env.is_empty() {
        println!("Missing Env:  {}", plugin.missing_env.join(", "));
    }
    Ok(())
}

fn show_plugin_status() -> anyhow::Result<()> {
    let mut manager = PluginManager::new();
    manager.discover_all();
    if manager.plugins().is_empty() {
        println!("No plugins discovered.");
        return Ok(());
    }

    for plugin in manager.plugins() {
        println!(
            "{}: {} (kind={}, enabled={}, tools={})",
            plugin.name,
            plugin.status_label(),
            plugin.kind.as_tag(),
            plugin.enabled,
            plugin.tools.len()
        );
    }
    Ok(())
}

fn install_plugin(repo: &str, explicit_name: Option<&str>) -> anyhow::Result<()> {
    let plugins_dir = user_plugins_dir()?;
    std::fs::create_dir_all(&plugins_dir)?;

    let plugin_name = explicit_name
        .map(ToString::to_string)
        .unwrap_or_else(|| infer_plugin_name(repo));
    let target = safe_plugin_path(&plugins_dir, &plugin_name)?;
    if target.exists() {
        anyhow::bail!(
            "plugin '{}' already exists at {}",
            plugin_name,
            target.display()
        );
    }

    let repo_url = normalize_repo(repo);
    let status = std::process::Command::new("git")
        .args(["clone", "--depth", "1", &repo_url])
        .arg(&target)
        .status()
        .with_context(|| format!("failed to run git clone for {}", repo_url))?;
    if !status.success() {
        anyhow::bail!("git clone failed for {}", repo_url);
    }

    println!("Installed plugin '{}' to {}", plugin_name, target.display());
    Ok(())
}

fn toggle_plugin_enabled(name: &str) -> anyhow::Result<()> {
    let home =
        edgecrab_core::ensure_edgecrab_home().context("failed to initialize edgecrab home")?;
    let config_path = home.join("config.yaml");
    let config = if config_path.is_file() {
        edgecrab_core::AppConfig::load_from(&config_path)?
    } else {
        edgecrab_core::AppConfig::default()
    };
    let enabled = !config
        .plugins
        .disabled
        .iter()
        .any(|candidate| candidate == name);
    set_plugin_enabled(name, !enabled)
}

fn set_plugin_enabled(name: &str, enabled: bool) -> anyhow::Result<()> {
    let home =
        edgecrab_core::ensure_edgecrab_home().context("failed to initialize edgecrab home")?;
    let config_path = home.join("config.yaml");
    let mut config = if config_path.is_file() {
        edgecrab_core::AppConfig::load_from(&config_path)?
    } else {
        edgecrab_core::AppConfig::default()
    };

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
    println!(
        "{} plugin '{}'",
        if enabled { "Enabled" } else { "Disabled" },
        name
    );
    Ok(())
}

fn update_plugin(name: &str) -> anyhow::Result<()> {
    let plugins_dir = user_plugins_dir()?;
    let target = safe_plugin_path(&plugins_dir, name)?;
    if !target.is_dir() {
        anyhow::bail!("plugin '{}' is not installed", name);
    }
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(&target)
        .args(["pull", "--ff-only"])
        .status()
        .with_context(|| format!("failed to update plugin {}", name))?;
    if !status.success() {
        anyhow::bail!("git pull failed for plugin '{}'", name);
    }
    println!("Updated plugin '{}'", name);
    Ok(())
}

fn remove_plugin(name: &str) -> anyhow::Result<()> {
    let plugins_dir = user_plugins_dir()?;
    let target = safe_plugin_path(&plugins_dir, name)?;
    if !target.exists() {
        anyhow::bail!("plugin '{}' is not installed", name);
    }
    std::fs::remove_dir_all(&target)
        .with_context(|| format!("failed to remove plugin {}", target.display()))?;
    println!("Removed plugin '{}'", name);
    Ok(())
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

fn infer_plugin_name(repo: &str) -> String {
    repo.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(repo)
        .trim_end_matches(".git")
        .to_string()
}

fn normalize_repo(repo: &str) -> String {
    if repo.contains("://") || repo.starts_with("git@") {
        repo.to_string()
    } else {
        format!("https://github.com/{}.git", repo.trim_end_matches(".git"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_name_from_repo() {
        assert_eq!(infer_plugin_name("owner/repo"), "repo");
        assert_eq!(
            infer_plugin_name("https://github.com/owner/repo.git"),
            "repo"
        );
    }

    #[test]
    fn normalize_repo_shortcuts() {
        assert_eq!(
            normalize_repo("owner/repo"),
            "https://github.com/owner/repo.git"
        );
        assert_eq!(
            normalize_repo("https://github.com/owner/repo.git"),
            "https://github.com/owner/repo.git"
        );
    }
}
