//! Re-export spill-to-artifact from edgecrab-tools (single implementation).

pub use edgecrab_tools::artifact_spill::{
    SpillConfig, SpillContext, SpillOutcome, SpillSequence, SpillWritten, WEB_EXTRACT_INLINE_BYTES,
    WEB_SEARCH_INLINE_BYTES, apply_web_extract_content_spill, enforce_turn_budget, maybe_spill,
    spill_context_from_args, web_extract_inline_threshold, web_search_inline_threshold,
    web_search_spilled_json, write_artifact_proactive,
};
