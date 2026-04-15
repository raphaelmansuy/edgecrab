pub mod config;
pub mod context;
pub mod discovery;
pub mod error;
pub mod guard;
pub mod hermes;
pub mod host_api;
pub mod hub;
pub mod manifest;
pub mod script;
pub mod skill;
pub mod tool_server;
pub mod types;

pub use context::{ContextEngineManifest, discover_context_engines, find_context_engine_manifest};

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
pub use hermes::{
    HermesCliCommand, HermesEntrypointPlugin, HermesPluginManifest, discover_entrypoint_plugins,
    extract_pre_llm_context, invoke_cli_command as invoke_hermes_cli_command,
    invoke_hook as invoke_hermes_hook, looks_like_hermes_plugin, parse_hermes_manifest,
    supports_hook as hermes_supports_hook, synthesize_entrypoint_manifest,
    synthesize_manifest as synthesize_hermes_manifest,
};
pub use host_api::{handle_host_request, is_host_method};
pub use hub::{
    HubIndex, HubIndexPlugin, InstallSourceKind, PluginAuditEntry, PluginHubSourceInfo, PluginMeta,
    PluginSearchGroup, PluginSearchReport, PluginSearchResult, SharedInstallFile,
    SharedInstallFileSource, append_audit_entry, clear_hub_cache, hub_source_names,
    hub_source_summaries, install_shared_files, materialize_source_to_dir, read_audit_entries,
    resolve_install_source, search_hub, search_hub_report, sha256_dir,
};
pub use manifest::{
    INSTALL_METADATA_FILE, InstallMetadata, PluginExecConfig, PluginManifest, PluginRestartPolicy,
    ensure_installable_manifest, parse_plugin_manifest, read_bundle_install_metadata,
    write_bundle_install_metadata, write_install_metadata,
};
pub use skill::sync::{BundledSyncReport, BundledSyncStatus, bundled_skills_sync};
pub use types::{PluginKind, PluginStatus, SkillReadinessStatus, SkillSource, TrustLevel};
