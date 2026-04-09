use std::collections::HashMap;
use std::path::Path;

use regex::Regex;
use serde::Deserialize;

use crate::error::PluginError;
use crate::types::{PluginKind, TrustLevel};

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
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

#[derive(Debug, Clone, Deserialize)]
pub struct PluginScriptConfig {
    pub file: String,
    #[serde(default = "default_max_operations")]
    pub max_operations: u64,
    #[serde(default = "default_max_call_depth")]
    pub max_call_depth: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginToolDefinition {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PluginCapabilities {
    #[serde(default)]
    pub host: Vec<String>,
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    #[serde(default)]
    pub required_host_toolsets: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginTrustConfig {
    #[serde(default)]
    pub level: TrustLevel,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PluginIntegrityConfig {
    pub checksum: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
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
    if let Some(homepage) = manifest.plugin.homepage.as_deref() {
        if !homepage.starts_with("https://") {
            return invalid_manifest(path, "plugin.homepage must use https");
        }
    }

    if let Some(trust) = manifest.trust.as_ref() {
        if matches!(trust.level, TrustLevel::Official | TrustLevel::Trusted) {
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
    }

    if !matches!(manifest.plugin.kind, PluginKind::Skill) && manifest.tools.is_empty() {
        return invalid_manifest(
            path,
            "runtime plugins must declare at least one [[tools]] entry",
        );
    }

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
}
