use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginKind {
    Skill,
    ToolServer,
    Script,
}

impl PluginKind {
    pub fn as_tag(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::ToolServer => "tool-server",
            Self::Script => "script",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginStatus {
    Available,
    Disabled,
    PlatformExcluded,
    SetupNeeded,
    Unsupported,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TrustLevel {
    Official,
    Trusted,
    Community,
    AgentCreated,
    #[default]
    Unverified,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillReadinessStatus {
    Available,
    SetupNeeded { missing: Vec<String> },
    Unsupported { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    User,
    Project,
    System,
}
