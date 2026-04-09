pub mod config;
pub mod discovery;
pub mod error;
pub mod manifest;
pub mod script;
pub mod skill;
pub mod tool_server;
pub mod types;

pub use config::{
    HostApiLimitsConfig, HubSource, PluginOverrideConfig, PluginSecurityConfig, PluginsConfig,
    PluginsHubConfig,
};
pub use discovery::{
    DiscoveredPlugin, PluginDiscovery, build_plugin_skill_prompt, discover_plugins,
};
pub use error::PluginError;
pub use manifest::{PluginExecConfig, PluginManifest, PluginRestartPolicy, parse_plugin_manifest};
pub use skill::sync::{BundledSyncReport, BundledSyncStatus, bundled_skills_sync};
pub use types::{PluginKind, PluginStatus, SkillReadinessStatus, SkillSource, TrustLevel};
