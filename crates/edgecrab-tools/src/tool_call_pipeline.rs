//! Full tool-call trust boundary — name resolve + argument repair/prepare.
//!
//! Hermes splits this across `repair_tool_call`, `_repair_tool_call_arguments`, and
//! pre-API message sanitization. EdgeCrab centralizes here (DRY / single responsibility).

/// Model emitted an unregistered tool name this many turns in a row → partial abort.
pub const MAX_INVALID_TOOL_RETRIES: u32 = 3;

use std::collections::HashSet;

use edgecrab_types::{Message, Role, ToolCall, ToolError};

use crate::registry::ToolRegistry;
use crate::tool_argument_pipeline::{
    canonical_tool_args_json, prepare_parsed_tool_arguments, repair_tool_arguments,
};
use crate::tool_name_repair::{self, ResolvedToolName};

static TOOL_PIPELINE_LOGGED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// One info log per process when the tool-call pipeline runs on a local tool turn.
pub fn log_pipeline_activated() {
    if TOOL_PIPELINE_LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    tracing::info!(
        target: "edgecrab::local_llm",
        name_repair = true,
        arg_repair = true,
        pre_api_sanitize = true,
        unknown_tool_guard = MAX_INVALID_TOOL_RETRIES,
        "tool-call pipeline activated"
    );
}

/// Whether a wire tool name resolves to a registry or context-engine tool.
pub fn is_tool_registered(
    registry: &ToolRegistry,
    engine_tool_names: &HashSet<String>,
    wire_name: &str,
) -> bool {
    if engine_tool_names.contains(wire_name) {
        return true;
    }
    let resolved = registry.resolve_tool_call_name(wire_name);
    if resolved.canonical.is_empty() {
        return false;
    }
    registry.lookup_tool_name(&resolved.canonical).is_some()
        || engine_tool_names.contains(&resolved.canonical)
}

/// Normalize a provider tool call (name repair + arg canonicalization) for session storage.
pub fn normalize_incoming_tool_call(
    registry: &ToolRegistry,
    tc: &edgequake_llm::ToolCall,
) -> ToolCall {
    let mut our = ToolCall::from_llm(tc);
    let resolved = registry.resolve_tool_call_name(&our.function.name);
    if !resolved.canonical.is_empty() {
        our.function.name = resolved.canonical;
    }
    our.function.arguments =
        repair_tool_call_arguments_for_api(&our.function.arguments, &our.function.name);
    our
}

/// Names that remain unknown after repair (Hermes invalid_tool_calls parity).
pub fn unknown_tool_names(
    registry: &ToolRegistry,
    engine_tool_names: &HashSet<String>,
    tool_calls: &[ToolCall],
) -> Vec<String> {
    tool_calls
        .iter()
        .filter(|tc| !is_tool_registered(registry, engine_tool_names, &tc.function.name))
        .map(|tc| tc.function.name.clone())
        .collect()
}

/// Structured tool-not-found payload with optional fuzzy suggestion.
pub fn unknown_tool_error_response(
    registry: &ToolRegistry,
    invalid_name: &str,
) -> String {
    let suggestion = tool_name_repair::repair_tool_name(registry, invalid_name);
    let sample: Vec<String> = registry
        .tool_names()
        .into_iter()
        .take(16)
        .map(str::to_string)
        .collect();
    crate::recovery_catalog::unknown_tool(invalid_name, suggestion.as_deref(), &sample)
        .to_llm_response()
}

/// Result of validating a tool-call batch before dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownToolBatchOutcome {
    pub unknown_names: Vec<String>,
    pub retry_count: u32,
    pub should_abort: bool,
}

/// Classify unknown tools and apply Hermes 3-strike abort policy.
pub fn classify_unknown_tool_batch(
    registry: &ToolRegistry,
    engine_tool_names: &HashSet<String>,
    tool_calls: &[ToolCall],
    prior_retries: u32,
) -> UnknownToolBatchOutcome {
    let unknown_names = unknown_tool_names(registry, engine_tool_names, tool_calls);
    if unknown_names.is_empty() {
        return UnknownToolBatchOutcome {
            unknown_names,
            retry_count: 0,
            should_abort: false,
        };
    }
    let retry_count = prior_retries.saturating_add(1);
    UnknownToolBatchOutcome {
        unknown_names,
        retry_count,
        should_abort: retry_count >= MAX_INVALID_TOOL_RETRIES,
    }
}

/// A tool call normalized for dispatch (name resolved, args parsed + schema-prepared).
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedToolCall {
    pub original_name: String,
    pub name: String,
    pub name_repaired: bool,
    pub args: serde_json::Value,
    pub args_json: String,
}

/// Dispatch entry: resolve wire name → repair/prepare arguments.
pub fn prepare_tool_call(
    registry: &ToolRegistry,
    raw_name: &str,
    raw_args: &str,
) -> Result<PreparedToolCall, ToolError> {
    let resolved = registry.resolve_tool_call_name(raw_name);
    let args = prepare_parsed_tool_arguments(registry, &resolved.canonical, raw_args)?;
    let args_json = canonical_tool_args_json(&args);
    Ok(PreparedToolCall {
        original_name: resolved.original,
        name: resolved.canonical,
        name_repaired: resolved.repaired,
        args,
        args_json,
    })
}

/// Hermes pre-API pass: canonicalize valid JSON or syntax-repair malformed args.
///
/// Does **not** run schema validation (e.g. write_file required fields) — that stays
/// in [`prepare_tool_call`] at dispatch time only.
pub fn repair_tool_call_arguments_for_api(raw: &str, _tool_name: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return repair_tool_arguments(trimmed);
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return canonical_tool_args_json(&value);
    }
    repair_tool_arguments(trimmed)
}

/// Repair assistant tool-call names + argument JSON before the next LLM request.
///
/// Mirrors Hermes `conversation_loop.py` pre-API tool_calls canonicalization/repair.
pub fn sanitize_assistant_tool_calls_for_api(messages: &mut [Message], registry: &ToolRegistry) {
    for msg in messages.iter_mut() {
        if msg.role != Role::Assistant {
            continue;
        }
        let Some(tool_calls) = msg.tool_calls.as_mut() else {
            continue;
        };
        for tc in tool_calls.iter_mut() {
            let ResolvedToolName {
                canonical,
                repaired,
                ..
            } = registry.resolve_tool_call_name(&tc.function.name);
            if !canonical.is_empty() {
                if repaired || canonical != tc.function.name {
                    tracing::debug!(
                        original = %tc.function.name,
                        canonical = %canonical,
                        "sanitize_assistant_tool_calls: repaired tool name for API history"
                    );
                }
                tc.function.name = canonical;
            }
            let repaired_args =
                repair_tool_call_arguments_for_api(&tc.function.arguments, &tc.function.name);
            if repaired_args != tc.function.arguments {
                tracing::debug!(
                    tool = %tc.function.name,
                    "sanitize_assistant_tool_calls: repaired tool arguments for API history"
                );
            }
            tc.function.arguments = repaired_args;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::ToolRegistry;

    #[test]
    fn tcp01_prepare_tool_call_resolves_name_and_aliases() {
        let registry = ToolRegistry::new();
        let prepared = prepare_tool_call(
            &registry,
            "WriteFileTool",
            r#"{"file_path":"a.py","text":"print(1)"}"#,
        )
        .expect("prepare");
        assert_eq!(prepared.name, "write_file");
        assert!(prepared.name_repaired);
        assert_eq!(prepared.args["path"], "a.py");
        assert_eq!(prepared.args["content"], "print(1)");
    }

    #[test]
    fn tcp02_repair_for_api_canonicalizes_valid_json() {
        let out = repair_tool_call_arguments_for_api(r#"{"a": 1, "b": 2}"#, "write_file");
        assert_eq!(out, r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn tcp03_repair_for_api_fixes_trailing_comma() {
        let out = repair_tool_call_arguments_for_api(r#"{"path":"x",}"#, "write_file");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid json");
        assert_eq!(v["path"], "x");
    }

    #[test]
    fn tcp04_sanitize_history_repairs_class_like_name() {
        use edgecrab_types::ToolCall;

        let registry = ToolRegistry::new();
        let mut messages = vec![Message::assistant_with_tool_calls(
            "",
            vec![ToolCall {
                id: "call_1".into(),
                r#type: "function".into(),
                function: edgecrab_types::FunctionCall {
                    name: "Patch_tool".into(),
                    arguments: r#"{"path":"f.rs","content":"x"}"#.into(),
                },
                thought_signature: None,
            }],
        )];
        sanitize_assistant_tool_calls_for_api(&mut messages, &registry);
        let tc = messages[0].tool_calls.as_ref().expect("tcs")[0].clone();
        assert_eq!(tc.function.name, "patch");
    }

    #[test]
    fn tcp05_unknown_tool_detected_after_repair() {
        let registry = ToolRegistry::new();
        use edgecrab_types::ToolCall;

        let calls = vec![ToolCall {
            id: "c1".into(),
            r#type: "function".into(),
            function: edgecrab_types::FunctionCall {
                name: "TotallyFakeTool_xyz".into(),
                arguments: "{}".into(),
            },
            thought_signature: None,
        }];
        assert!(!is_tool_registered(&registry, &HashSet::new(), "TotallyFakeTool_xyz"));
        assert_eq!(
            unknown_tool_names(&registry, &HashSet::new(), &calls),
            vec!["TotallyFakeTool_xyz".to_string()]
        );
    }

    #[test]
    fn tcp06_classify_unknown_batch_abort_on_third_strike() {
        let registry = ToolRegistry::new();
        use edgecrab_types::ToolCall;

        let calls = vec![ToolCall {
            id: "c1".into(),
            r#type: "function".into(),
            function: edgecrab_types::FunctionCall {
                name: "NotARealTool".into(),
                arguments: "{}".into(),
            },
            thought_signature: None,
        }];
        let batch = classify_unknown_tool_batch(&registry, &HashSet::new(), &calls, 2);
        assert!(batch.should_abort);
        assert_eq!(batch.retry_count, 3);
    }

    #[test]
    fn tcp07_todo_tool_tool_maps_to_manage_todo_list() {
        let registry = ToolRegistry::new();
        assert_eq!(
            tool_name_repair::repair_tool_name(&registry, "TodoTool_tool"),
            Some("manage_todo_list".into())
        );
    }
}
