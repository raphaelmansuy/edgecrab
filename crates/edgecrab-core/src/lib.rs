//! # edgecrab-core
//!
//! Agent core: conversation loop, prompt builder, context compression,
//! model routing, @context reference expansion.

#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

use edgecrab_lsp as _;

pub mod agent;
pub mod auxiliary_model;
pub mod completion_assessor;
pub mod compression;
pub mod config;
pub mod context_budget;
pub mod context_engine;
pub mod context_references;
pub mod contract_verify;
pub mod conversation;
pub mod copilot_agent_probe;
pub mod copilot_model_policy;
pub mod credential_pool;
pub mod failover;
pub mod file_manifest;
pub mod gateway_home;
pub mod goal_draft;
pub mod goal_judge;
pub mod goals;
pub mod evidence_latch;
pub mod harness_advisory;
pub mod harness_analyzer;
pub mod harness_loop_policy;
pub mod kanban_api;
pub mod kanban_auth;
pub mod kanban_decompose;
pub mod kanban_dispatcher;
pub mod kanban_notifier;
pub mod kanban_orchestration;
pub mod kanban_profile_describer;
pub mod kanban_profiles;
pub mod kanban_reaper;
pub mod kanban_respawn_guard;
pub mod kanban_slash;
pub mod kanban_task_patch;
pub mod kanban_workers;
pub mod lifecycle_hooks;
pub mod local_provider_policy;
pub mod logging;
pub mod model_catalog;
pub mod model_cost_guard;
pub mod model_discovery;
pub mod model_router;
pub mod model_transfer;
pub mod multimodal_tool_content;
pub mod oauth;
pub mod observability;
pub mod otel_export;
pub mod otel_metrics;
pub mod pricing;
pub mod progress_sink;
pub mod prompt_builder;
pub mod prompt_cache_policy;
pub mod provider_call;
pub mod provider_error_class;
pub mod session_handoff;
pub mod shadow_judge;
pub mod state_snapshot;
pub mod steering;
pub mod stream_observability;
pub mod sub_agent_runner;
pub mod subagent_registry;
pub mod subdirectory_hints;
pub mod task_class;
pub mod tool_batch;
pub mod tool_result_spill;
pub mod tool_result_summary;
pub mod turn_completion;
pub mod turn_dispatch;
pub mod turn_dispatch_policy;
pub mod turn_epilogue;
pub mod turn_prologue;

pub use agent::{
    Agent, AgentBuilder, AgentConfig, ApprovalChoice, ConversationResult, IsolatedAgentOptions,
    IterationBudget, SessionSnapshot, SessionState, StreamEvent,
};
pub use compression::{PRUNED_TOOL_PLACEHOLDER, SUMMARY_PREFIX};
pub use config::{
    AppConfig, CliOverrides, ForwardAdapterKind, ForwardUpstreamConfig, GoalJudgeConfig,
    GoalsConfig, ObservabilityConfig, ProxyConfig, ShelfDetailsConfig, SmartRoutingYaml,
    ToolProgressMode, edgecrab_home, ensure_edgecrab_home, gateway_image_cache_dir,
    gateway_media_dir, install_edgecrab_home, merge_global_inherited,
};
pub use context_budget::{ContextBudgetBreakdown, estimate_context_budget};
pub use context_engine::{
    BuiltinCompressorEngine, ContextEngine, ContextEngineSessionCtx, MAX_ENGINE_TOOLS,
    load_context_engine,
};
pub use context_references::{ContextRef, ExpansionResult, expand_context_refs};
pub use edgecrab_tools::{
    HarnessBuildInput, HarnessSnapshot, build_harness_snapshot, terminal_mutation_tool_error,
};
pub use credential_pool::{
    CredentialPool, PooledCredential, global_mark_exhausted_and_rotate, global_pool_ensure,
};
pub use tool_batch::{PlannedToolCall, ToolBatchPlan, parallel_max_workers, plan_tool_batch};
pub use turn_prologue::{
    TurnPrologueState, compression_made_progress, should_run_preflight_estimate,
};
pub use gateway_home::{
    HANDOFF_PLATFORM_HINT, handoff_platform_from_name, resolve_gateway_home_channel,
};
pub use goals::{
    GoalContinuationDecision, GoalState, GoalStatus, GoalStatusChip, GoalStore, InMemoryGoalStore,
    SqliteGoalStore, SubGoal, compact_status_chip, drain_goal_continuations_from_queue,
    evaluate_goal_after_turn, goal_flash_from_decision, goal_store_for_db,
    is_goal_continuation_text, looks_like_slash_command, next_continuation_prompt,
    prompt_queue_has_real_user_message, render_goal_block, render_subgoals_list, status_line,
};
pub use harness_analyzer::{
    HarnessLogReport, analyze_harness_file, analyze_harness_log, format_harness_report,
};
pub use kanban_auth::{
    check_kanban_token, default_kanban_token_path, ensure_kanban_api_token, load_kanban_api_token,
    resolved_kanban_token_path,
};
pub use kanban_decompose::{
    DecomposeOutcome, decompose_outcome_json, decompose_task_by_id, format_decompose_outcome,
    run_auto_decompose_tick,
};
pub use kanban_dispatcher::{
    KanbanDispatchConfig, KanbanDispatchResult, KanbanSpawnRequest, dispatch_once,
};
pub use kanban_notifier::{
    KanbanActivePlatformsFn, KanbanNotifyDeliverFn, KanbanNotifyOutbound, format_notifier_message,
    spawn_kanban_notifier,
};
pub use kanban_orchestration::{
    OrchestrationSettingsPatch, get_orchestration_settings, patch_orchestration_settings,
};
pub use kanban_profile_describer::{
    DescribeProfileOutcome, describe_outcome_json, describe_profile,
};
pub use kanban_profiles::{
    KanbanProfileEntry, active_profile_name, format_roster_for_prompt, install_root,
    list_profile_roster, load_config_for_profile, normalize_assignee_choice,
    normalize_profile_name, profile_effective_home, profile_exists, profiles_api_json,
    resolve_default_assignee, resolve_orchestrator_profile, valid_assignee_names,
    write_profile_description, write_profile_meta,
};
pub use kanban_reaper::{KanbanSpawnFn, spawn_kanban_reaper, spawn_kanban_watcher};
pub use kanban_slash::{KanbanNotifyOrigin, handle_kanban_slash, handle_kanban_slash_gateway};
pub use kanban_task_patch::{CONFLICT_PREFIX, TaskPatch, parse_conflict, patch_kanban_task};
pub use logging::{
    HARNESS_JSON_LOG_NAME, LogFileInfo, LogLevelSetting, LoggingGuards, LoggingMode, StderrMode,
    default_tail_lines, effective_log_level, format_log_size, format_relative_time, init_logging,
    list_log_files, persist_log_level, read_last_lines, reload_runtime_log_level, tail_preview,
};
pub use model_catalog::{
    CatalogData, ModelCatalog, ModelEntry, ModelTier, PricingPair, ProviderEntry, ResolvedModelSpec,
};
pub use model_cost_guard::{
    ExpensiveModelWarning, INPUT_COST_WARNING_THRESHOLD, OUTPUT_COST_WARNING_THRESHOLD,
    expensive_model_warning, is_expensive_pricing,
};
pub use model_discovery::{
    DiscoveryAvailability, DiscoverySource, ProviderModels, discover_multiple,
    discover_provider_models, discovery_provider_statuses, live_discovery_availability,
    live_discovery_providers, merge_grouped_catalog_with_dynamic, normalize_discovery_provider,
};
pub use model_router::{
    SmartRoutingConfig, SmartRoutingStats, TurnRoute, classify_message, fallback_route,
    list_plugin_provider_aliases, resolve_turn_route,
};
pub use model_transfer::{
    MODEL_TRANSFER_BUSY_MESSAGE, MODEL_TRANSFER_USAGE, ModelChangeOutcome, ModelSwitchOutcome,
    ModelTransferBrief, ModelTransferContext, ModelTransferError, ModelTransferOrchestrator,
    ModelTransferOutcome, ModelTransferTarget, context_window_for_model,
    create_model_transfer_provider, format_model_change_confirmation, format_model_change_error,
    format_model_change_result, format_model_switch_confirmation,
    format_model_transfer_confirmation, format_model_transfer_insights_section,
    format_model_transfer_result, format_model_transfer_user_message,
    generate_model_transfer_brief, maybe_compress_for_model_transfer,
    resolve_model_transfer_target, session_requires_model_transfer,
};
pub use observability::{
    OBSERVABILITY_FILTER_DIRECTIVES, TARGET_GENAI_SPANS, TARGET_HARNESS, TARGET_LOCAL_LLM,
    TARGET_PROVIDER_LLM, apply_runtime_from_config, load_app_config_and_apply_observability,
};
pub use otel_export::{
    OtelGuard, collector_reachable, maybe_otel_layer, otel_export_enabled, otel_metrics_enabled,
    otel_traces_enabled,
};
pub use pricing::{
    CanonicalUsage, CostResult, CostSource, CostStatus, PricingEntry, estimate_cost, get_pricing,
};
pub use session_handoff::{
    SESSION_HANDOFF_BUSY_MESSAGE, SESSION_HANDOFF_USAGE, SessionHandoffState, SessionHandoffStatus,
    format_session_handoff_cli_success, format_session_handoff_synthetic_message,
};
pub use state_snapshot::{
    PRE_UPDATE_SNAPSHOT_LABEL, SnapshotManifest, create_pre_update_snapshot, create_quick_snapshot,
    handle_snapshot_slash, list_quick_snapshots, prune_labeled_snapshots, prune_quick_snapshots,
    restore_quick_snapshot,
};
pub use steering::{
    SteeringEvent, SteeringKind, SteeringReceiver, SteeringSender, drain_pending_steers,
    steering_channel,
};

/// Truncate `s` to at most `max_bytes` bytes, always stopping at a valid UTF-8
/// char boundary so that multi-byte / emoji characters are never split.
#[inline]
pub fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // Walk backwards from max_bytes to find the last valid char boundary.
    let boundary = (0..=max_bytes)
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0);
    &s[..boundary]
}

/// Find the first valid UTF-8 char boundary at or after `start_bytes`.
/// Used for safe tail slicing: `&s[safe_char_start(s, n)..]`.
#[inline]
pub fn safe_char_start(s: &str, start_bytes: usize) -> usize {
    if start_bytes >= s.len() {
        return s.len();
    }
    (start_bytes..=s.len())
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(s.len())
}
pub use prompt_builder::{
    PromptBuilder, SkillPromptParts, extract_frontmatter_name, extract_skill_description,
    load_memory_sections, load_preloaded_skills, load_skill_prompt_parts, load_skill_summary,
};
pub use sub_agent_runner::CoreSubAgentRunner;
pub use subagent_registry::{interrupt_subagent, register_subagent, unregister_subagent};
