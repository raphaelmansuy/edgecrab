//! Shared skills subsystem — slash commands, bundles, write approval.
//!
//! Mirrors hermes-agent `agent/skill_commands.py`, `agent/skill_bundles.py`,
//! and `tools/write_approval.py` as a single Rust module used by CLI, gateway,
//! and `skill_manage`.

mod archive;
mod backup;
mod bundles;
mod config_settings;
mod context;
mod credential_files;
mod curator;
mod discovery;
mod filters;
mod invocation;
mod invocation_extras;
mod preprocess;
mod protected;
mod scheduler;
mod slug;
mod usage;
mod write_approval;
mod memory_write_approval;
mod pending_store;

pub use archive::{
    ArchiveEligibility, archive_skill, check_archive_eligibility, format_archived_list,
    list_archived, restore_skill,
};
pub use backup::{
    SnapshotManifest, format_backup_list, list_snapshots, maybe_snapshot_before_mutate,
    rollback_snapshot, snapshot_skills,
};
pub use bundles::{
    SkillBundleDef, build_bundle_invocation_message, delete_bundle, format_bundles_list,
    get_skill_bundles, handle_bundles_subcommand, invalidate_bundle_cache, list_bundles,
    reload_bundles, resolve_bundle_command_key, save_bundle,
};
pub use config_settings::{
    SkillConfigEntry, format_skill_config_show, handle_skills_config_subcommand,
    scan_skill_config_entries, set_skill_config_value,
};
pub use context::{SkillsScanContext, merge_disabled_skills};
pub use credential_files::{
    CredentialFileSpec, SkillCredentialRequirement, credential_file_present,
    missing_credential_files,
};
pub use curator::{
    CuratorSettings, StaleReason, StaleSkill, find_prune_candidates, find_stale_skills,
    format_curator_status, format_stale_report, handle_curator_subcommand,
};
pub use discovery::{
    SkillCommandInfo, SkillsReloadDiff, format_reload_diff, list_installed_skill_slugs,
    reload_skills, resolve_skill_command_key, scan_skill_commands,
};
pub use filters::{
    SkillOfferMeta, parse_offer_meta, skill_matches_environment, skill_matches_platform,
};
pub use invocation::{
    SlashInvocation, SlashInvocationKind, build_skill_invocation_message,
    enrich_message_for_skill_slash, resolve_slash_invocation, resolve_slash_line,
};
pub use invocation_extras::{
    format_slash_setup_note, format_slash_supporting_files, list_supporting_files,
};
pub use preprocess::{
    PreprocessOptions, expand_inline_shell, format_skill_config_block,
    preprocess_options_from_config, preprocess_skill_content, substitute_template_vars,
};
pub use scheduler::{
    CuratorState, load_curator_state, maybe_run_scheduled_curator, seed_curator_deferred_if_needed,
    set_curator_paused,
};
pub use slug::slugify;
pub use usage::{
    activity_count, bump_patch, bump_use, bump_view, format_usage_summary, is_pinned, set_pinned,
};
pub use pending_store::PendingWriteRecord;
pub use memory_write_approval::{
    MemorySubcommandContext, MemoryWriteGate, MemoryWritePayload, apply_pending_memory_write,
    format_memory_pending_state, handle_memory_pending_subcommand, maybe_gate_memory_write,
    memory_write_approval_enabled,
};
pub use write_approval::{
    SkillManageGate, SkillsSubcommandContext, apply_pending_skill_write,
    apply_skill_manage_payload, format_skills_pending_state, handle_skills_pending_subcommand,
    maybe_gate_skill_manage, skills_write_approval_enabled,
};

/// Invalidate in-process skill discovery caches (bundles). Call alongside
/// `edgecrab_core::prompt_builder::invalidate_skills_cache()` after install/remove.
pub fn invalidate_discovery_caches() {
    invalidate_bundle_cache();
}
