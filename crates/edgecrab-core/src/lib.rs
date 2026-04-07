//! # edgecrab-core
//!
//! Agent core: conversation loop, prompt builder, context compression,
//! model routing, @context reference expansion.

#![deny(clippy::unwrap_used)]

use edgecrab_lsp as _;

pub mod agent;
pub mod compression;
pub mod config;
pub mod context_references;
pub mod conversation;
pub mod model_catalog;
pub mod model_discovery;
pub mod model_router;
pub mod pricing;
pub mod prompt_builder;
pub mod sub_agent_runner;

pub use agent::{
    Agent, AgentBuilder, AgentConfig, ApprovalChoice, ConversationResult, IsolatedAgentOptions,
    IterationBudget, SessionSnapshot, SessionState, StreamEvent,
};
pub use compression::{PRUNED_TOOL_PLACEHOLDER, SUMMARY_PREFIX};
pub use config::{
    AppConfig, CliOverrides, SmartRoutingYaml, edgecrab_home, ensure_edgecrab_home,
    gateway_image_cache_dir, gateway_media_dir,
};
pub use context_references::{ContextRef, ExpansionResult, expand_context_refs};
pub use model_catalog::{
    CatalogData, ModelCatalog, ModelEntry, ModelTier, PricingPair, ProviderEntry,
};
pub use model_discovery::{
    DiscoveryAvailability, DiscoverySource, ProviderModels, discover_multiple,
    discover_provider_models, discovery_provider_statuses, live_discovery_availability,
    live_discovery_providers, merge_grouped_catalog_with_dynamic, normalize_discovery_provider,
};
pub use model_router::{
    SmartRoutingConfig, TurnRoute, classify_message, fallback_route, resolve_turn_route,
};
pub use pricing::{
    CanonicalUsage, CostResult, CostSource, CostStatus, PricingEntry, estimate_cost, get_pricing,
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
    PromptBuilder, extract_frontmatter_name, extract_skill_description, load_memory_sections,
    load_preloaded_skills, load_skill_summary,
};
pub use sub_agent_runner::CoreSubAgentRunner;
