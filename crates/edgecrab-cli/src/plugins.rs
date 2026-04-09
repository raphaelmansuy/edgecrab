use edgecrab_core::AppConfig;
use edgecrab_plugins::{
    HermesCliCommand, PluginKind, PluginStatus, SkillSource, TrustLevel, discover_plugins,
};
use edgecrab_types::Platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginSource {
    User,
    Project,
    System,
}

impl std::fmt::Display for PluginSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Project => write!(f, "project"),
            Self::System => write!(f, "system"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PluginTool {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Plugin {
    pub name: String,
    pub version: String,
    pub description: String,
    pub compatibility: Option<String>,
    pub source: PluginSource,
    pub kind: PluginKind,
    pub status: PluginStatus,
    pub enabled: bool,
    pub tools: Vec<PluginTool>,
    pub trust_level: TrustLevel,
    pub install_source: Option<String>,
    pub missing_env: Vec<String>,
    pub related_skills: Vec<String>,
    pub cli_commands: Vec<HermesCliCommand>,
}

impl Plugin {
    pub fn status_label(&self) -> &'static str {
        match self.status {
            PluginStatus::Available => "running",
            PluginStatus::Disabled => "disabled",
            PluginStatus::PlatformExcluded => "platform-excluded",
            PluginStatus::SetupNeeded => "setup-needed",
            PluginStatus::Unsupported => "unsupported",
            PluginStatus::Error => "error",
        }
    }
}

pub struct PluginManager {
    plugins: Vec<Plugin>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    pub fn discover() -> Vec<Plugin> {
        let config = AppConfig::load().unwrap_or_default();
        let discovery = discover_plugins(&config.plugins, Platform::Cli).unwrap_or_default();
        discovery
            .plugins
            .into_iter()
            .map(|plugin| Plugin {
                name: plugin.name,
                version: plugin.version,
                description: plugin.description,
                compatibility: plugin.compatibility,
                source: match plugin.source {
                    SkillSource::User => PluginSource::User,
                    SkillSource::Project => PluginSource::Project,
                    SkillSource::System => PluginSource::System,
                },
                kind: plugin.kind,
                status: plugin.status,
                enabled: plugin.enabled,
                tools: plugin
                    .tools
                    .into_iter()
                    .map(|name| PluginTool { name })
                    .collect(),
                trust_level: plugin.trust_level,
                install_source: plugin.install_source,
                missing_env: plugin.missing_env,
                related_skills: plugin.related_skills,
                cli_commands: plugin.cli_commands,
            })
            .collect()
    }

    pub fn discover_all(&mut self) {
        self.plugins = Self::discover();
    }

    pub fn plugins(&self) -> &[Plugin] {
        &self.plugins
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_source_display() {
        assert_eq!(PluginSource::User.to_string(), "user");
        assert_eq!(PluginSource::Project.to_string(), "project");
        assert_eq!(PluginSource::System.to_string(), "system");
    }
}
