use crate::fuzzy_selector::FuzzyItem;
use crate::plugins::Plugin;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginScope {
    Global,
    Platform(String),
}

impl PluginScope {
    pub fn title(&self) -> String {
        match self {
            Self::Global => "global".into(),
            Self::Platform(platform) => platform.clone(),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PluginCheckState {
    On,
    Off,
}

impl PluginCheckState {
    pub fn glyph(self) -> &'static str {
        match self {
            Self::On => "[x]",
            Self::Off => "[ ]",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::On => Self::Off,
            Self::Off => Self::On,
        }
    }
}

#[derive(Clone)]
pub struct PluginToggleEntry {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub version: String,
    pub source: String,
    pub kind: String,
    pub tool_count: usize,
    pub estimated_tokens: usize,
    pub check_state: PluginCheckState,
    pub original_check_state: PluginCheckState,
    pub runtime_status: String,
    pub trust_level: String,
    pub tools: Vec<String>,
    pub cli_commands: Vec<String>,
    pub missing_env: Vec<String>,
    pub related_skills: Vec<String>,
    pub compatibility: Option<String>,
    pub install_source: Option<String>,
    pub credentials_satisfied: bool,
    pub search_text: String,
}

impl FuzzyItem for PluginToggleEntry {
    fn primary(&self) -> &str {
        &self.display_name
    }

    fn secondary(&self) -> &str {
        &self.search_text
    }

    fn tag(&self) -> &str {
        &self.runtime_status
    }
}

impl PluginToggleEntry {
    pub fn from_plugin(plugin: &Plugin, enabled: bool) -> Self {
        let check_state = if enabled {
            PluginCheckState::On
        } else {
            PluginCheckState::Off
        };
        let tools = plugin
            .tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>();
        let cli_commands = plugin
            .cli_commands
            .iter()
            .map(|command| command.name.clone())
            .collect::<Vec<_>>();
        let trust_level = format!("{:?}", plugin.trust_level).to_ascii_lowercase();
        let runtime_status = plugin.status_label().to_string();
        let missing_env = plugin.missing_env.clone();
        let related_skills = plugin.related_skills.clone();
        let compatibility = plugin.compatibility.clone();
        let install_source = plugin.install_source.clone();
        let search_text = [
            plugin.description.clone(),
            plugin.kind.as_tag().to_string(),
            runtime_status.clone(),
            trust_level.clone(),
            tools.join(" "),
            cli_commands.join(" "),
            missing_env.join(" "),
            related_skills.join(" "),
            compatibility.clone().unwrap_or_default(),
            install_source.clone().unwrap_or_default(),
        ]
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
        Self {
            name: plugin.name.clone(),
            display_name: plugin.name.clone(),
            description: plugin.description.clone(),
            version: plugin.version.clone(),
            source: plugin.source.to_string(),
            kind: plugin.kind.as_tag().to_string(),
            tool_count: tools.len(),
            estimated_tokens: estimate_plugin_tokens(plugin),
            check_state,
            original_check_state: check_state,
            runtime_status,
            trust_level,
            tools,
            cli_commands,
            missing_env: missing_env.clone(),
            related_skills,
            compatibility,
            install_source,
            credentials_satisfied: plugin.missing_env.is_empty(),
            search_text,
        }
    }

    pub fn state_label(&self) -> &'static str {
        match self.check_state {
            PluginCheckState::On => "enabled",
            PluginCheckState::Off => "disabled",
        }
    }

    pub fn has_pending_change(&self) -> bool {
        self.check_state != self.original_check_state
    }
}

pub fn estimate_plugin_tokens(plugin: &Plugin) -> usize {
    match plugin.kind.as_tag() {
        "skill" => (plugin.description.len().max(32) * 8) / 4,
        "tool-server" => {
            plugin
                .tools
                .iter()
                .map(|tool| (tool.name.len() + plugin.description.len()).max(48))
                .sum::<usize>()
                / 4
        }
        "script" => 0,
        _ => 0,
    }
}

pub fn plugin_toggle_status_line(entries: &[PluginToggleEntry]) -> String {
    let enabled = entries
        .iter()
        .filter(|entry| matches!(entry.check_state, PluginCheckState::On))
        .count();
    let staged = entries
        .iter()
        .filter(|entry| entry.has_pending_change())
        .count();
    let setup_needed = entries
        .iter()
        .filter(|entry| !entry.credentials_satisfied)
        .count();
    let token_total = entries
        .iter()
        .filter(|entry| matches!(entry.check_state, PluginCheckState::On))
        .map(|entry| entry.estimated_tokens)
        .sum::<usize>();
    format!(
        "{} enabled · {} staged · {} need setup · Est. plugin context: ~{} tokens",
        enabled, staged, setup_needed, token_total
    )
}

#[allow(dead_code)]
pub fn plugin_toggle_text_fallback(entries: &[PluginToggleEntry]) -> String {
    entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            format!(
                "{}. {} {} {} ({})",
                idx + 1,
                entry.check_state.glyph(),
                entry.display_name,
                entry.version,
                entry.kind
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::{Plugin, PluginSource, PluginTool};
    use edgecrab_plugins::{PluginKind, PluginStatus, TrustLevel};

    #[test]
    fn status_line_sums_enabled_tokens() {
        let plugin = Plugin {
            name: "demo".into(),
            version: "1.0.0".into(),
            description: "Demo plugin".into(),
            compatibility: None,
            install_source: None,
            source: PluginSource::User,
            kind: PluginKind::ToolServer,
            status: PluginStatus::Available,
            enabled: true,
            tools: vec![PluginTool {
                name: "demo_tool".into(),
            }],
            trust_level: TrustLevel::Unverified,
            missing_env: Vec::new(),
            related_skills: Vec::new(),
            cli_commands: Vec::new(),
        };
        let entries = vec![PluginToggleEntry::from_plugin(&plugin, true)];
        assert!(plugin_toggle_status_line(&entries).contains("1 enabled"));
        assert!(plugin_toggle_status_line(&entries).contains("0 staged"));
    }
}
