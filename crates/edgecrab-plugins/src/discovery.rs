use std::collections::HashSet;
use std::path::{Path, PathBuf};

use edgecrab_types::Platform;

use crate::config::PluginsConfig;
use crate::error::PluginError;
use crate::hermes::{
    looks_like_hermes_plugin, missing_required_env as missing_hermes_env,
    parse_hermes_manifest, synthesize_manifest as synthesize_hermes_manifest,
};
use crate::manifest::{PluginManifest, parse_plugin_manifest};
use crate::skill::inject::build_prompt_fragment;
use crate::skill::loader::{LoadedSkill, scan_skills_dir};
use crate::types::{PluginKind, PluginStatus, SkillReadinessStatus, SkillSource, TrustLevel};

#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub name: String,
    pub version: String,
    pub description: String,
    pub kind: PluginKind,
    pub status: PluginStatus,
    pub path: PathBuf,
    pub manifest: Option<PluginManifest>,
    pub skill: Option<LoadedSkill>,
    pub tools: Vec<String>,
    pub hooks: Vec<String>,
    pub trust_level: TrustLevel,
    pub enabled: bool,
    pub source: SkillSource,
    pub missing_env: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct PluginDiscovery {
    pub plugins: Vec<DiscoveredPlugin>,
}

pub fn discover_plugins(
    config: &PluginsConfig,
    platform: Platform,
) -> Result<PluginDiscovery, PluginError> {
    let mut seen = HashSet::new();
    let mut plugins = Vec::new();
    for (root, source) in plugin_dirs(config) {
        if !root.exists() {
            continue;
        }
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("plugin.toml");
            let plugin = if manifest_path.is_file() {
                load_plugin_from_manifest(config, &path, source, platform, &manifest_path)?
            } else if looks_like_hermes_plugin(&path) {
                load_hermes_plugin(config, &path, source, platform)?
            } else if path.join("SKILL.md").is_file() {
                load_skill_only_plugin(config, &path, source, platform)?
            } else {
                continue;
            };
            if seen.insert(plugin.name.clone()) {
                plugins.push(plugin);
            }
        }
    }
    plugins.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(PluginDiscovery { plugins })
}

pub fn build_plugin_skill_prompt(discovery: &PluginDiscovery) -> Option<String> {
    let sections: Vec<String> = discovery
        .plugins
        .iter()
        .filter_map(|plugin| {
            if !plugin.enabled {
                return None;
            }
            let skill = plugin.skill.as_ref()?;
            if !skill.platform_visible {
                return None;
            }
            match &skill.readiness {
                SkillReadinessStatus::Available => Some(build_prompt_fragment(
                    &skill.manifest.name,
                    &skill.manifest.body,
                )),
                _ => None,
            }
        })
        .collect();
    if sections.is_empty() {
        None
    } else {
        Some(format!("# Plugin Skills\n\n{}", sections.join("\n\n")))
    }
}

fn load_plugin_from_manifest(
    config: &PluginsConfig,
    path: &Path,
    source: SkillSource,
    platform: Platform,
    manifest_path: &Path,
) -> Result<DiscoveredPlugin, PluginError> {
    let manifest = parse_plugin_manifest(manifest_path)?;
    let enabled = config.is_plugin_enabled(&manifest.plugin.name, Some(&platform.to_string()));
    let skill = if manifest.plugin.kind == PluginKind::Skill && path.join("SKILL.md").is_file() {
        scan_skills_dir(path, source, &[], platform)?
            .into_iter()
            .find(|skill| skill.path == path.join("SKILL.md"))
    } else {
        None
    };
    let status = skill.as_ref().map_or_else(
        || {
            if enabled {
                PluginStatus::Available
            } else {
                PluginStatus::Disabled
            }
        },
        |skill| status_for_skill(enabled, skill),
    );
    Ok(DiscoveredPlugin {
        name: manifest.plugin.name.clone(),
        version: manifest.plugin.version.clone(),
        description: manifest.plugin.description.clone(),
        kind: manifest.plugin.kind,
        status,
        path: path.to_path_buf(),
        tools: manifest
            .tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect(),
        hooks: Vec::new(),
        trust_level: manifest
            .trust
            .as_ref()
            .map(|trust| trust.level)
            .unwrap_or_default(),
        manifest: Some(manifest),
        skill,
        enabled,
        source,
        missing_env: Vec::new(),
    })
}

fn load_skill_only_plugin(
    config: &PluginsConfig,
    path: &Path,
    source: SkillSource,
    platform: Platform,
) -> Result<DiscoveredPlugin, PluginError> {
    let loaded = scan_skills_dir(path, source, &[], platform)?
        .into_iter()
        .find(|skill| skill.path == path.join("SKILL.md"))
        .ok_or_else(|| PluginError::InvalidSkill {
            path: path.join("SKILL.md"),
            message: "failed to load skill plugin".into(),
        })?;
    let missing_env = skill_missing_env(&loaded);
    let enabled = config.is_plugin_enabled(&loaded.manifest.name, Some(&platform.to_string()));
    Ok(DiscoveredPlugin {
        name: loaded.manifest.name.clone(),
        version: loaded
            .manifest
            .version
            .clone()
            .unwrap_or_else(|| "0.1.0".into()),
        description: loaded.manifest.description.clone(),
        kind: PluginKind::Skill,
        status: status_for_skill(enabled, &loaded),
        path: path.to_path_buf(),
        manifest: None,
        skill: Some(loaded),
        tools: Vec::new(),
        hooks: Vec::new(),
        trust_level: TrustLevel::Unverified,
        enabled,
        source,
        missing_env,
    })
}

fn plugin_dirs(config: &PluginsConfig) -> Vec<(PathBuf, SkillSource)> {
    let mut seen = HashSet::new();
    let mut dirs = Vec::new();

    let mut push_dir = |path: PathBuf, source: SkillSource| {
        if seen.insert(path.clone()) {
            dirs.push((path, source));
        }
    };

    push_dir(config.install_dir.clone(), SkillSource::User);
    let legacy_user_plugins = dirs::home_dir()
        .map(|home| home.join(".hermes").join("plugins"))
        .unwrap_or_else(|| PathBuf::from("~/.hermes/plugins"));
    push_dir(legacy_user_plugins, SkillSource::User);
    for path in config.expanded_external_dirs() {
        push_dir(path, SkillSource::User);
    }
    push_dir(PathBuf::from(".edgecrab/plugins"), SkillSource::Project);
    if env_var_enabled("HERMES_ENABLE_PROJECT_PLUGINS") {
        push_dir(PathBuf::from(".hermes/plugins"), SkillSource::Project);
    }
    #[cfg(unix)]
    push_dir(
        PathBuf::from("/usr/share/edgecrab/plugins"),
        SkillSource::System,
    );
    dirs
}

fn status_for_skill(enabled: bool, loaded: &LoadedSkill) -> PluginStatus {
    if !enabled {
        return PluginStatus::Disabled;
    }
    if !loaded.platform_visible {
        return PluginStatus::PlatformExcluded;
    }
    match loaded.readiness {
        SkillReadinessStatus::Available => PluginStatus::Available,
        SkillReadinessStatus::SetupNeeded { .. } => PluginStatus::SetupNeeded,
        SkillReadinessStatus::Unsupported { .. } => PluginStatus::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn builds_prompt_only_for_enabled_available_skills() {
        let discovery = PluginDiscovery {
            plugins: vec![DiscoveredPlugin {
                name: "demo".into(),
                version: "0.1.0".into(),
                description: "Demo".into(),
                kind: PluginKind::Skill,
                status: PluginStatus::Available,
                path: PathBuf::from("/tmp/demo"),
                manifest: None,
                skill: Some(LoadedSkill {
                    path: PathBuf::from("/tmp/demo/SKILL.md"),
                    source: SkillSource::User,
                    manifest: crate::skill::manifest::SkillManifest {
                        name: "demo".into(),
                        description: "Demo".into(),
                        body: "Follow demo steps.".into(),
                        ..Default::default()
                    },
                    readiness: SkillReadinessStatus::Available,
                    platform_visible: true,
                }),
                tools: Vec::new(),
                hooks: Vec::new(),
                trust_level: TrustLevel::Unverified,
                enabled: true,
                source: SkillSource::User,
                missing_env: Vec::new(),
            }],
        };

        let prompt = build_plugin_skill_prompt(&discovery).expect("prompt exists");
        assert!(prompt.contains("# Plugin Skills"));
        assert!(prompt.contains("Follow demo steps."));
    }

    #[test]
    fn hermes_plugin_with_missing_env_is_setup_needed() {
        let temp = TempDir::new().expect("tempdir");
        let plugin_dir = temp.path().join("demo");
        std::fs::create_dir_all(&plugin_dir).expect("plugin dir");
        std::fs::write(
            plugin_dir.join("plugin.yaml"),
            r#"
name: hermes-demo
version: "1.0.0"
description: Demo Hermes plugin
provides_tools:
  - hello_world
requires_env:
  - EDGECRAB_TEST_HERMES_TOKEN
"#,
        )
        .expect("manifest");
        std::fs::write(
            plugin_dir.join("__init__.py"),
            "def register(ctx):\n    pass\n",
        )
        .expect("plugin");

        let config = PluginsConfig {
            install_dir: temp.path().join("empty-install-root"),
            external_dirs: vec![plugin_dir
                .parent()
                .expect("parent")
                .to_string_lossy()
                .to_string()],
            ..PluginsConfig::default()
        };

        let discovery = discover_plugins(&config, Platform::Cli).expect("discovery");
        let plugin = discovery
            .plugins
            .iter()
            .find(|plugin| plugin.name == "hermes-demo")
            .expect("plugin discovered");

        assert_eq!(plugin.status, PluginStatus::SetupNeeded);
        assert_eq!(plugin.missing_env, vec!["EDGECRAB_TEST_HERMES_TOKEN"]);
    }
}

fn env_var_enabled(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn load_hermes_plugin(
    config: &PluginsConfig,
    path: &Path,
    source: SkillSource,
    platform: Platform,
) -> Result<DiscoveredPlugin, PluginError> {
    let hermes_manifest = parse_hermes_manifest(path)?;
    let manifest = synthesize_hermes_manifest(path, &hermes_manifest);
    let enabled = config.is_plugin_enabled(&manifest.plugin.name, Some(&platform.to_string()));
    let missing_env = missing_hermes_env(&hermes_manifest);
    Ok(DiscoveredPlugin {
        name: manifest.plugin.name.clone(),
        version: manifest.plugin.version.clone(),
        description: manifest.plugin.description.clone(),
        kind: manifest.plugin.kind,
        status: if !enabled {
            PluginStatus::Disabled
        } else if missing_env.is_empty() {
            PluginStatus::Available
        } else {
            PluginStatus::SetupNeeded
        },
        path: path.to_path_buf(),
        manifest: Some(manifest),
        skill: None,
        tools: hermes_manifest.provides_tools,
        hooks: hermes_manifest.provides_hooks,
        trust_level: TrustLevel::Unverified,
        enabled,
        source,
        missing_env,
    })
}

fn skill_missing_env(loaded: &LoadedSkill) -> Vec<String> {
    match &loaded.readiness {
        SkillReadinessStatus::SetupNeeded { missing } => missing.clone(),
        _ => Vec::new(),
    }
}
