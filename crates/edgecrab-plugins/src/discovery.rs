use std::collections::HashSet;
use std::path::{Path, PathBuf};

use edgecrab_types::Platform;

use crate::config::PluginsConfig;
use crate::error::PluginError;
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
    pub trust_level: TrustLevel,
    pub enabled: bool,
    pub source: SkillSource,
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
        trust_level: manifest
            .trust
            .as_ref()
            .map(|trust| trust.level)
            .unwrap_or_default(),
        manifest: Some(manifest),
        skill,
        enabled,
        source,
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
        trust_level: TrustLevel::Unverified,
        enabled,
        source,
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
    for path in config.expanded_external_dirs() {
        push_dir(path, SkillSource::User);
    }
    push_dir(PathBuf::from(".edgecrab/plugins"), SkillSource::Project);
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
                trust_level: TrustLevel::Unverified,
                enabled: true,
                source: SkillSource::User,
            }],
        };

        let prompt = build_plugin_skill_prompt(&discovery).expect("prompt exists");
        assert!(prompt.contains("# Plugin Skills"));
        assert!(prompt.contains("Follow demo steps."));
    }
}
