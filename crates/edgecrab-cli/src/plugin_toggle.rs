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
    pub needs_credentials: bool,
    pub credentials_satisfied: bool,
}

impl FuzzyItem for PluginToggleEntry {
    fn primary(&self) -> &str {
        &self.display_name
    }

    fn secondary(&self) -> &str {
        &self.description
    }

    fn tag(&self) -> &str {
        &self.kind
    }
}

impl PluginToggleEntry {
    pub fn from_plugin(plugin: &Plugin, enabled: bool) -> Self {
        Self {
            name: plugin.name.clone(),
            display_name: plugin.name.clone(),
            description: plugin.description.clone(),
            version: plugin.version.clone(),
            source: plugin.source.to_string(),
            kind: plugin.kind.as_tag().to_string(),
            tool_count: plugin.tools.len(),
            estimated_tokens: estimate_plugin_tokens(plugin),
            check_state: if enabled {
                PluginCheckState::On
            } else {
                PluginCheckState::Off
            },
            needs_credentials: !plugin.missing_env.is_empty(),
            credentials_satisfied: plugin.missing_env.is_empty(),
        }
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
    let token_total = entries
        .iter()
        .filter(|entry| matches!(entry.check_state, PluginCheckState::On))
        .map(|entry| entry.estimated_tokens)
        .sum::<usize>();
    format!(
        "{} enabled · Est. plugin context: ~{} tokens",
        enabled, token_total
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
            source: PluginSource::User,
            kind: PluginKind::ToolServer,
            status: PluginStatus::Available,
            enabled: true,
            tools: vec![PluginTool {
                name: "demo_tool".into(),
            }],
            trust_level: TrustLevel::Unverified,
            missing_env: Vec::new(),
        };
        let entries = vec![PluginToggleEntry::from_plugin(&plugin, true)];
        assert!(plugin_toggle_status_line(&entries).contains("1 enabled"));
    }
}
