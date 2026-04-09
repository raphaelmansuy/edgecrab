pub mod config;
pub mod discovery;
pub mod error;
pub mod guard;
pub mod host_api;
pub mod hub;
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
pub use guard::{
    ScanFinding, ScanResult, ScanVerdict, Severity, ThreatCategory, VerdictResult,
    scan_plugin_bundle, should_allow_install,
};
pub use host_api::{handle_host_request, is_host_method};
pub use hub::{
    HubIndex, HubIndexPlugin, InstallSourceKind, PluginAuditEntry, PluginSearchResult,
    append_audit_entry, clear_hub_cache, materialize_source_to_dir, read_audit_entries,
    resolve_install_source, search_hub, sha256_dir,
};
pub use manifest::{
    PluginExecConfig, PluginManifest, PluginRestartPolicy, parse_plugin_manifest,
    write_install_metadata,
};
pub use skill::sync::{BundledSyncReport, BundledSyncStatus, bundled_skills_sync};
pub use types::{PluginKind, PluginStatus, SkillReadinessStatus, SkillSource, TrustLevel};
