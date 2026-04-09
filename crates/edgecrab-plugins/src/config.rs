use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::types::TrustLevel;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PluginsConfig {
    pub enabled: bool,
    pub auto_enable: bool,
    pub call_timeout_secs: u64,
    pub startup_timeout_secs: u64,
    pub disabled: Vec<String>,
    pub platform_disabled: HashMap<String, Vec<String>>,
    pub install_dir: PathBuf,
    pub quarantine_dir: PathBuf,
    pub external_dirs: Vec<String>,
    pub security: PluginSecurityConfig,
    pub hub: PluginsHubConfig,
    pub host_api: HostApiLimitsConfig,
    pub overrides: HashMap<String, PluginOverrideConfig>,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        let home = std::env::var("EDGECRAB_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".edgecrab")
            });
        Self {
            enabled: true,
            auto_enable: true,
            call_timeout_secs: 60,
            startup_timeout_secs: 10,
            disabled: Vec::new(),
            platform_disabled: HashMap::new(),
            install_dir: home.join("plugins"),
            quarantine_dir: home.join("plugins").join(".quarantine"),
            external_dirs: Vec::new(),
            security: PluginSecurityConfig::default(),
            hub: PluginsHubConfig::default(),
            host_api: HostApiLimitsConfig::default(),
            overrides: HashMap::new(),
        }
    }
}

impl PluginsConfig {
    pub fn is_plugin_enabled(&self, name: &str, platform: Option<&str>) -> bool {
        if !self.enabled {
            return false;
        }

        if self.disabled.iter().any(|candidate| candidate == name) {
            return self
                .overrides
                .get(name)
                .and_then(|override_cfg| override_cfg.disabled)
                == Some(false);
        }

        if let Some(platform) = platform {
            let platform_key = platform.to_ascii_lowercase();
            if self
                .platform_disabled
                .get(&platform_key)
                .is_some_and(|names| names.iter().any(|candidate| candidate == name))
            {
                return self
                    .overrides
                    .get(name)
                    .and_then(|override_cfg| override_cfg.disabled)
                    == Some(false);
            }
        }

        !matches!(
            self.overrides
                .get(name)
                .and_then(|override_cfg| override_cfg.disabled),
            Some(true)
        )
    }

    pub fn expanded_external_dirs(&self) -> Vec<PathBuf> {
        self.external_dirs
            .iter()
            .filter_map(|raw| expand_plugin_dir(raw))
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PluginSecurityConfig {
    pub min_trust_level: TrustLevel,
    pub allow_caution: bool,
    pub max_tool_count: usize,
    pub max_skill_size_kb: usize,
    pub scan_on_load: bool,
}

impl Default for PluginSecurityConfig {
    fn default() -> Self {
        Self {
            min_trust_level: TrustLevel::Unverified,
            allow_caution: false,
            max_tool_count: 100,
            max_skill_size_kb: 512,
            scan_on_load: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct PluginsHubConfig {
    pub enabled: bool,
    pub cache_ttl_secs: u64,
    pub sources: Vec<HubSource>,
}

impl Default for PluginsHubConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cache_ttl_secs: 900,
            sources: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HubSource {
    pub url: String,
    pub name: String,
    pub trust_override: Option<TrustLevel>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct HostApiLimitsConfig {
    pub max_memory_write_per_min: u32,
    pub max_secret_get_per_min: u32,
    pub max_inject_per_min: u32,
    pub max_tool_delegate_per_min: u32,
    pub max_log_per_min: u32,
}

impl Default for HostApiLimitsConfig {
    fn default() -> Self {
        Self {
            max_memory_write_per_min: 60,
            max_secret_get_per_min: 20,
            max_inject_per_min: 5,
            max_tool_delegate_per_min: 30,
            max_log_per_min: 200,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct PluginOverrideConfig {
    pub call_timeout_secs: Option<u64>,
    pub disabled: Option<bool>,
}

fn expand_plugin_dir(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut expanded = trimmed.to_string();
    if let Some(rest) = expanded.strip_prefix("~/") {
        expanded = dirs::home_dir()?.join(rest).display().to_string();
    }

    for (name, value) in std::env::vars() {
        let pattern = format!("${{{name}}}");
        if expanded.contains(&pattern) {
            expanded = expanded.replace(&pattern, &value);
        }
    }

    Some(PathBuf::from(expanded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_enable_checks_global_and_platform_lists() {
        let mut config = PluginsConfig::default();
        assert!(config.is_plugin_enabled("demo", Some("cli")));

        config.disabled.push("demo".into());
        assert!(!config.is_plugin_enabled("demo", Some("cli")));

        config.disabled.clear();
        config
            .platform_disabled
            .insert("cli".into(), vec!["demo".into()]);
        assert!(!config.is_plugin_enabled("demo", Some("cli")));
        assert!(config.is_plugin_enabled("demo", Some("telegram")));
    }

    #[test]
    fn override_can_reenable_plugin() {
        let mut config = PluginsConfig::default();
        config.disabled.push("demo".into());
        config.overrides.insert(
            "demo".into(),
            PluginOverrideConfig {
                disabled: Some(false),
                call_timeout_secs: None,
            },
        );

        assert!(config.is_plugin_enabled("demo", Some("cli")));
    }

    #[test]
    fn expands_external_dirs_with_home_and_env() {
        let home = dirs::home_dir().expect("home dir");
        let env_home = std::env::var("HOME").expect("HOME env");
        let config = PluginsConfig {
            external_dirs: vec!["~/custom".into(), "${HOME}/plugins-extra".into()],
            ..PluginsConfig::default()
        };
        let dirs = config.expanded_external_dirs();

        assert_eq!(dirs[0], home.join("custom"));
        assert_eq!(dirs[1], PathBuf::from(env_home).join("plugins-extra"));
    }
}
