//! Local KV / MTP **prefix stability** for oMLX, MTPLX, Ollama, LM Studio.
//!
//! Cloud Anthropic-style `cache_control` is handled by [`crate::prompt_cache_policy`].
//! Local servers reuse KV on **byte-identical** request prefixes. Mutating tool
//! schemas every turn (e.g. local budget suffixes that change with max_tokens)
//! causes `prefix_divergence_at_token` and multi-minute TTFT.
//!
//! # Law (July 2026)
//!
//! Freeze the **wire** tool definitions for a local session after the first
//! annotate, until the tool *set* (ordered names) changes.
//!
//! Spec: `specs/023-omlx/013-local-prefix-cache-july-2026.md`

use edgequake_llm::ToolDefinition;

use crate::local_provider_policy::is_local_inference_provider;

/// Fingerprint of the active tool set (order-sensitive names).
pub fn tool_set_fingerprint(defs: &[ToolDefinition]) -> String {
    let mut names: Vec<&str> = defs.iter().map(|d| d.function.name.as_str()).collect();
    // Names are already in registry order; do not sort — order is part of the wire prefix.
    let _ = &mut names;
    names.join("\n")
}

/// Stable hash of tool wire payload for tests / observability.
pub fn tool_defs_content_fingerprint(defs: &[ToolDefinition]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    for d in defs {
        d.function.name.hash(&mut hasher);
        d.function.description.hash(&mut hasher);
        // parameters as compact JSON
        if let Ok(s) = serde_json::to_string(&d.function.parameters) {
            s.hash(&mut hasher);
        }
    }
    format!("{:016x}", hasher.finish())
}

/// Session-side freeze state (also mirrored on [`crate::agent::SessionState`]).
#[derive(Clone, Debug, Default)]
pub struct LocalToolFreeze {
    pub fingerprint: String,
    pub tools: Vec<ToolDefinition>,
}

impl LocalToolFreeze {
    pub fn matches(&self, fingerprint: &str) -> bool {
        !self.fingerprint.is_empty() && self.fingerprint == fingerprint && !self.tools.is_empty()
    }
}

/// Resolve API tool definitions with **session freeze** for local providers.
///
/// - Non-local / no tools → annotate path (or passthrough).
/// - Local + matching freeze → return frozen tools (byte-stable).
/// - Local + miss → annotate once, store freeze, return.
pub fn resolve_frozen_local_api_tools(
    freeze: &mut Option<LocalToolFreeze>,
    provider_name: &str,
    active_tool_defs: &[ToolDefinition],
    local_turn_budget: Option<(usize, usize)>,
) -> Vec<ToolDefinition> {
    if active_tool_defs.is_empty() {
        *freeze = None;
        return Vec::new();
    }

    if !is_local_inference_provider(provider_name) {
        return edgecrab_tools::registry::annotate_llm_definitions_for_local_turn(
            active_tool_defs.to_vec(),
            provider_name,
            local_turn_budget,
        );
    }

    let fp = tool_set_fingerprint(active_tool_defs);
    if let Some(existing) = freeze.as_ref()
        && existing.matches(&fp)
    {
        tracing::debug!(
            target: "edgecrab::local_prefix",
            provider = provider_name,
            tools = existing.tools.len(),
            "local prefix: reusing frozen tool wire schemas"
        );
        return existing.tools.clone();
    }

    let annotated = edgecrab_tools::registry::annotate_llm_definitions_for_local_turn(
        active_tool_defs.to_vec(),
        provider_name,
        local_turn_budget,
    );

    tracing::info!(
        target: "edgecrab::local_prefix",
        provider = provider_name,
        tools = annotated.len(),
        budget = ?local_turn_budget,
        content_fp = %tool_defs_content_fingerprint(&annotated),
        "local prefix: freezing tool wire schemas for session"
    );

    *freeze = Some(LocalToolFreeze {
        fingerprint: fp,
        tools: annotated.clone(),
    });
    annotated
}

/// Drop freeze (tool set changed, model transfer, new session).
pub fn clear_local_tool_freeze(freeze: &mut Option<LocalToolFreeze>) {
    if freeze.is_some() {
        tracing::debug!(target: "edgecrab::local_prefix", "local prefix: cleared frozen tools");
    }
    *freeze = None;
}

/// Whether this provider benefits from local prefix freeze (observability).
pub fn local_prefix_freeze_applies(provider_name: &str) -> bool {
    is_local_inference_provider(provider_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgequake_llm::ToolDefinition;
    use serde_json::json;

    fn def(name: &str, desc: &str) -> ToolDefinition {
        ToolDefinition::function(name, desc, json!({"type": "object", "properties": {}}))
    }

    #[test]
    fn fingerprint_order_sensitive() {
        let a = vec![def("a", "x"), def("b", "y")];
        let b = vec![def("b", "y"), def("a", "x")];
        assert_ne!(tool_set_fingerprint(&a), tool_set_fingerprint(&b));
        assert_eq!(
            tool_set_fingerprint(&a),
            tool_set_fingerprint(&[def("a", "x"), def("b", "z")])
        );
    }

    #[test]
    fn freeze_reuses_despite_budget_change() {
        let active = vec![
            def("write_file", "Write a file"),
            def("read_file", "Read a file"),
        ];
        let mut freeze = None;
        let first =
            resolve_frozen_local_api_tools(&mut freeze, "mtplx", &active, Some((10_000, 2048)));
        let second = resolve_frozen_local_api_tools(
            &mut freeze,
            "mtplx",
            &active,
            Some((99_999, 8192)), // different budget — must not re-annotate
        );
        assert_eq!(first.len(), second.len());
        assert_eq!(
            tool_defs_content_fingerprint(&first),
            tool_defs_content_fingerprint(&second)
        );
        // Mutation tools should carry the *first* budget suffix, not the second.
        let write = first
            .iter()
            .find(|d| d.function.name == "write_file")
            .expect("write_file");
        assert!(
            write.function.description.contains("10000")
                || write.function.description.contains("10_000")
                || write.function.description.contains("Local turn limit"),
            "desc={}",
            write.function.description
        );
        assert!(
            !write.function.description.contains("99999"),
            "must not re-annotate with new budget"
        );
    }

    #[test]
    fn tool_set_change_refreezes() {
        let mut freeze = None;
        let a = vec![def("read_file", "Read")];
        let _ = resolve_frozen_local_api_tools(&mut freeze, "omlx", &a, Some((1000, 512)));
        let b = vec![def("read_file", "Read"), def("write_file", "Write")];
        let second = resolve_frozen_local_api_tools(&mut freeze, "omlx", &b, Some((1000, 512)));
        assert_eq!(second.len(), 2);
        assert_eq!(
            freeze.as_ref().map(|f| f.fingerprint.as_str()),
            Some(tool_set_fingerprint(&b).as_str())
        );
    }

    #[test]
    fn non_local_does_not_freeze() {
        let mut freeze = None;
        let active = vec![def("read_file", "Read")];
        let _ =
            resolve_frozen_local_api_tools(&mut freeze, "anthropic", &active, Some((1000, 512)));
        assert!(freeze.is_none());
    }

    #[test]
    fn empty_tools_clears_freeze() {
        let mut freeze = Some(LocalToolFreeze {
            fingerprint: "x".into(),
            tools: vec![def("a", "b")],
        });
        let out = resolve_frozen_local_api_tools(&mut freeze, "mtplx", &[], Some((1, 1)));
        assert!(out.is_empty());
        assert!(freeze.is_none());
    }
}
