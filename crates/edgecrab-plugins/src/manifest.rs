use std::collections::HashMap;
use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};
use toml::Value;

use crate::error::PluginError;
use crate::hermes::{
    looks_like_hermes_plugin, parse_hermes_manifest,
    synthesize_manifest as synthesize_hermes_manifest,
};
use crate::skill::manifest::{SkillManifest, parse_skill_manifest};
use crate::types::{PluginKind, TrustLevel};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMetadata,
    #[serde(default)]
    pub exec: Option<PluginExecConfig>,
    #[serde(default)]
    pub script: Option<PluginScriptConfig>,
    #[serde(default)]
    pub tools: Vec<PluginToolDefinition>,
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    #[serde(default)]
    pub trust: Option<PluginTrustConfig>,
    #[serde(default)]
    pub integrity: Option<PluginIntegrityConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub kind: PluginKind,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub min_edgecrab_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginExecConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_startup_timeout_secs")]
    pub startup_timeout_secs: u64,
    #[serde(default = "default_call_timeout_secs")]
    pub call_timeout_secs: u64,
    #[serde(default)]
    pub restart_policy: PluginRestartPolicy,
    #[serde(default = "default_restart_max_attempts")]
    pub restart_max_attempts: u32,
    #[serde(default = "default_idle_timeout_secs")]
    pub idle_timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginScriptConfig {
    pub file: String,
    #[serde(default = "default_max_operations")]
    pub max_operations: u64,
    #[serde(default = "default_max_call_depth")]
    pub max_call_depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginToolDefinition {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginCapabilities {
    #[serde(default)]
    pub host: Vec<String>,
    #[serde(default)]
    pub secrets: Vec<String>,
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    #[serde(default)]
    pub required_host_toolsets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginTrustConfig {
    #[serde(default)]
    pub level: TrustLevel,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginIntegrityConfig {
    pub checksum: String,
}

pub const INSTALL_METADATA_FILE: &str = ".edgecrab-install.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallMetadata {
    pub trust_level: TrustLevel,
    pub source: String,
    pub checksum: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PluginRestartPolicy {
    Never,
    #[default]
    Once,
    Always,
}

fn default_startup_timeout_secs() -> u64 {
    10
}

fn default_call_timeout_secs() -> u64 {
    60
}

fn default_idle_timeout_secs() -> u64 {
    300
}

fn default_restart_max_attempts() -> u32 {
    3
}

fn default_max_operations() -> u64 {
    100_000
}

fn default_max_call_depth() -> usize {
    50
}

pub fn parse_plugin_manifest(path: &Path) -> Result<PluginManifest, PluginError> {
    let content = std::fs::read_to_string(path)?;
    parse_plugin_manifest_str(path, &content)
}

pub fn parse_plugin_manifest_str(
    path: &Path,
    content: &str,
) -> Result<PluginManifest, PluginError> {
    let manifest: PluginManifest = toml::from_str(content)?;
    validate_plugin_manifest(path, &manifest)?;
    Ok(manifest)
}

pub fn ensure_installable_manifest(dir: &Path) -> Result<std::path::PathBuf, PluginError> {
    let manifest_path = dir.join("plugin.toml");
    if manifest_path.is_file() {
        return Ok(manifest_path);
    }

    let manifest = if looks_like_hermes_plugin(dir) {
        let hermes_manifest = parse_hermes_manifest(dir)?;
        synthesize_hermes_manifest(dir, &hermes_manifest)
    } else {
        let skill_path = dir.join("SKILL.md");
        if !skill_path.is_file() {
            return Err(PluginError::MissingManifest {
                path: manifest_path,
            });
        }
        let skill_manifest = parse_skill_manifest(&skill_path)?;
        synthesize_skill_manifest(&skill_manifest)
    };

    write_plugin_manifest(&manifest_path, &manifest)?;
    Ok(manifest_path)
}

fn validate_plugin_manifest(path: &Path, manifest: &PluginManifest) -> Result<(), PluginError> {
    let name_re = Regex::new(r"^[a-z0-9][a-z0-9._-]*$").expect("valid regex");
    if manifest.plugin.name.trim().is_empty() {
        return invalid_manifest(path, "plugin.name is required");
    }
    if manifest.plugin.name.len() > 64 || !name_re.is_match(&manifest.plugin.name) {
        return invalid_manifest(
            path,
            "plugin.name must match [a-z0-9][a-z0-9._-]* and be <= 64 chars",
        );
    }

    if manifest.plugin.version.trim().is_empty() {
        return invalid_manifest(path, "plugin.version is required");
    }
    if manifest.plugin.description.trim().is_empty() || manifest.plugin.description.len() > 256 {
        return invalid_manifest(path, "plugin.description must be 1..=256 characters");
    }
    if let Some(homepage) = manifest.plugin.homepage.as_deref()
        && !homepage.starts_with("https://")
    {
        return invalid_manifest(path, "plugin.homepage must use https");
    }

    if let Some(trust) = manifest.trust.as_ref()
        && matches!(trust.level, TrustLevel::Official | TrustLevel::Trusted)
    {
        let installer_stamped = trust
            .source
            .as_deref()
            .is_some_and(|source| !source.trim().is_empty())
            && manifest
                .integrity
                .as_ref()
                .is_some_and(|integrity| !integrity.checksum.trim().is_empty());
        if !installer_stamped {
            return invalid_manifest(
                path,
                "plugin.toml cannot self-assign official or trusted trust level",
            );
        }
    }

    for tool in &manifest.tools {
        if tool.name.trim().is_empty() {
            return invalid_manifest(path, "tool names must be non-empty");
        }
        if !name_re.is_match(&tool.name) {
            return invalid_manifest(path, "tool names must match [a-z0-9][a-z0-9._-]*");
        }
    }

    match manifest.plugin.kind {
        PluginKind::Skill => {}
        PluginKind::ToolServer => {
            let Some(exec) = manifest.exec.as_ref() else {
                return invalid_manifest(path, "tool-server plugins require [exec]");
            };
            if exec.command.trim().is_empty() {
                return invalid_manifest(path, "exec.command is required");
            }
            if !(1..=60).contains(&exec.startup_timeout_secs) {
                return invalid_manifest(path, "exec.startup_timeout_secs must be 1..=60");
            }
            if !(1..=300).contains(&exec.call_timeout_secs) {
                return invalid_manifest(path, "exec.call_timeout_secs must be 1..=300");
            }
        }
        PluginKind::Script => {
            let Some(script) = manifest.script.as_ref() else {
                return invalid_manifest(path, "script plugins require [script]");
            };
            if script.file.trim().is_empty() {
                return invalid_manifest(path, "script.file is required");
            }
        }
        PluginKind::Hermes => {
            let Some(exec) = manifest.exec.as_ref() else {
                return invalid_manifest(path, "Hermes compatibility plugins require [exec]");
            };
            if exec.command.trim().is_empty() {
                return invalid_manifest(path, "exec.command is required");
            }
        }
    }

    if !matches!(manifest.plugin.kind, PluginKind::Skill | PluginKind::Hermes)
        && manifest.tools.is_empty()
    {
        return invalid_manifest(
            path,
            "runtime plugins must declare at least one [[tools]] entry",
        );
    }

    Ok(())
}

pub fn write_install_metadata(
    path: &Path,
    trust_level: TrustLevel,
    source: &str,
    checksum: &str,
) -> Result<(), PluginError> {
    let content = std::fs::read_to_string(path)?;
    let mut value: Value = toml::from_str(&content)?;
    let root = value
        .as_table_mut()
        .ok_or_else(|| PluginError::InvalidManifest {
            path: path.to_path_buf(),
            message: "plugin manifest must be a TOML table".into(),
        })?;

    let trust = root
        .entry("trust")
        .or_insert_with(|| Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| PluginError::InvalidManifest {
            path: path.to_path_buf(),
            message: "[trust] must be a TOML table".into(),
        })?;
    trust.insert(
        "level".into(),
        Value::String(
            match trust_level {
                TrustLevel::Official => "official",
                TrustLevel::Trusted => "trusted",
                TrustLevel::Community => "community",
                TrustLevel::AgentCreated => "agent-created",
                TrustLevel::Unverified => "unverified",
            }
            .into(),
        ),
    );
    trust.insert("source".into(), Value::String(source.to_string()));

    let integrity = root
        .entry("integrity")
        .or_insert_with(|| Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| PluginError::InvalidManifest {
            path: path.to_path_buf(),
            message: "[integrity] must be a TOML table".into(),
        })?;
    integrity.insert("checksum".into(), Value::String(checksum.to_string()));

    let rendered =
        toml::to_string_pretty(&value).map_err(|error| PluginError::InvalidManifest {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    std::fs::write(path, rendered)?;
    Ok(())
}

pub fn write_bundle_install_metadata(
    dir: &Path,
    trust_level: TrustLevel,
    source: &str,
    checksum: &str,
) -> Result<(), PluginError> {
    let metadata = InstallMetadata {
        trust_level,
        source: source.to_string(),
        checksum: checksum.to_string(),
    };
    let path = dir.join(INSTALL_METADATA_FILE);
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&metadata).map_err(PluginError::Json)?,
    )?;
    Ok(())
}

pub fn read_bundle_install_metadata(dir: &Path) -> Option<InstallMetadata> {
    let path = dir.join(INSTALL_METADATA_FILE);
    let content = std::fs::read(path).ok()?;
    serde_json::from_slice(&content).ok()
}

fn synthesize_skill_manifest(skill: &SkillManifest) -> PluginManifest {
    PluginManifest {
        plugin: PluginMetadata {
            name: skill.name.clone(),
            version: skill.version.clone().unwrap_or_else(|| "0.1.0".into()),
            description: synthesize_skill_description(skill),
            kind: PluginKind::Skill,
            author: skill.author.clone().unwrap_or_default(),
            license: skill.license.clone().unwrap_or_default(),
            homepage: None,
            min_edgecrab_version: None,
        },
        exec: None,
        script: None,
        tools: Vec::new(),
        capabilities: PluginCapabilities::default(),
        trust: None,
        integrity: None,
    }
}

fn synthesize_skill_description(skill: &SkillManifest) -> String {
    let fallback = format!("Skill plugin '{}'", skill.name);
    let description = if skill.description.trim().is_empty() {
        fallback.as_str()
    } else {
        skill.description.trim()
    };
    description.chars().take(256).collect()
}

fn write_plugin_manifest(path: &Path, manifest: &PluginManifest) -> Result<(), PluginError> {
    let rendered =
        toml::to_string_pretty(manifest).map_err(|error| PluginError::InvalidManifest {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    std::fs::write(path, rendered)?;
    Ok(())
}

fn invalid_manifest<T>(path: &Path, message: &str) -> Result<T, PluginError> {
    Err(PluginError::InvalidManifest {
        path: path.to_path_buf(),
        message: message.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn rejects_invalid_name() {
        let err = parse_plugin_manifest_str(
            Path::new("/tmp/plugin.toml"),
            r#"
[plugin]
name = "Bad Name"
version = "1.0.0"
description = "Demo"
kind = "skill"
"#,
        )
        .expect_err("invalid name rejected");

        assert!(err.to_string().contains("plugin.name"));
    }

    #[test]
    fn rejects_self_assigned_trusted_level() {
        let err = parse_plugin_manifest_str(
            Path::new("/tmp/plugin.toml"),
            r#"
[plugin]
name = "demo"
version = "1.0.0"
description = "Demo"
kind = "skill"

[trust]
level = "trusted"
"#,
        )
        .expect_err("trusted self-assignment rejected");

        assert!(err.to_string().contains("trust level"));
    }

    #[test]
    fn accepts_installer_stamped_trusted_level_with_source_and_checksum() {
        let manifest = parse_plugin_manifest_str(
            Path::new("/tmp/plugin.toml"),
            r#"
[plugin]
name = "demo"
version = "1.0.0"
description = "Demo"
kind = "skill"

[trust]
level = "trusted"
source = "hub:official/demo"

[integrity]
checksum = "sha256:abc123"
"#,
        )
        .expect("installer-stamped trust should parse");

        assert_eq!(
            manifest.trust.as_ref().map(|trust| trust.level),
            Some(TrustLevel::Trusted)
        );
    }

    #[test]
    fn synthesizes_install_manifest_for_local_hermes_bundle() {
        let temp = TempDir::new().expect("tempdir");
        std::fs::write(
            temp.path().join("plugin.yaml"),
            r#"
name: calculator
version: "1.0.0"
description: Calculator plugin
provides_tools:
  - calculate
"#,
        )
        .expect("write manifest");
        std::fs::write(
            temp.path().join("__init__.py"),
            "def register(ctx):\n    pass\n",
        )
        .expect("write init");

        let manifest_path = ensure_installable_manifest(temp.path()).expect("manifest path");
        let manifest = parse_plugin_manifest(&manifest_path).expect("parsed manifest");

        assert_eq!(manifest.plugin.kind, PluginKind::Hermes);
        assert_eq!(manifest.plugin.name, "calculator");
    }

    #[test]
    fn synthesizes_install_manifest_for_local_skill_bundle() {
        let temp = TempDir::new().expect("tempdir");
        std::fs::write(
            temp.path().join("SKILL.md"),
            r#"---
name: github-issues
description: Manage GitHub issues with a long but valid description.
version: 1.1.0
---

# GitHub Issues

Body.
"#,
        )
        .expect("write skill");

        let manifest_path = ensure_installable_manifest(temp.path()).expect("manifest path");
        let manifest = parse_plugin_manifest(&manifest_path).expect("parsed manifest");

        assert_eq!(manifest.plugin.kind, PluginKind::Skill);
        assert_eq!(manifest.plugin.name, "github-issues");
        assert_eq!(manifest.plugin.version, "1.1.0");
    }
}
