use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::plugins::PluginManager;

pub enum PluginAction {
    List,
    Install { repo: String, name: Option<String> },
    Update { name: String },
    Remove { name: String },
}

pub fn run(action: PluginAction) -> anyhow::Result<()> {
    match action {
        PluginAction::List => list_plugins(),
        PluginAction::Install { repo, name } => install_plugin(&repo, name.as_deref()),
        PluginAction::Update { name } => update_plugin(&name),
        PluginAction::Remove { name } => remove_plugin(&name),
    }
}

fn list_plugins() -> anyhow::Result<()> {
    let mut manager = PluginManager::new();
    manager.discover_all();
    if manager.plugins().is_empty() {
        println!("No plugins discovered.");
        return Ok(());
    }
    for plugin in manager.plugins() {
        println!(
            "{}  v{}  source={}  enabled={}  tools={}  hooks={}",
            plugin.name,
            plugin.version,
            plugin.source,
            plugin.enabled,
            plugin.tools.len(),
            plugin.hooks.len(),
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
    let path = plugins_dir.join(name);
    Ok(path)
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
