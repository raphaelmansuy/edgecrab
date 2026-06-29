//! Domain-specific self-reflective recovery catalogs for EdgeCrab tools.
//!
//! Validators emit concise diagnoses; this module supplies structured
//! `recovery_feedback.suggestions[]` payloads the agent can merge on retry.
//! Single responsibility: map EdgeCrab policy rejections → recovery actions.

use edgecrab_types::{RecoveryAction, RecoveryFeedbackBuilder, ToolError};
use serde_json::json;

fn recovery_guidance() -> RecoveryFeedbackBuilder {
    RecoveryFeedbackBuilder::new("recovery_guidance")
}

/// Path already exists — agent intended to create a new file.
pub fn write_file_path_exists_abort(path: String, size_bytes: u64) -> ToolError {
    ToolError::InvalidArgs {
        tool: "write_file".into(),
        message: format!(
            "'{path}' already exists ({size_bytes} bytes). \
             Target path is occupied — choose another path or overwrite explicitly."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Target path already exists")
            .suggestion(
                RecoveryAction::UseDifferentPath,
                json!({ "tool": "write_file", "path": path.clone() }),
            )
            .suggestion(
                RecoveryAction::SetParameter,
                json!({
                    "tool": "write_file",
                    "path": path,
                    "if_exists": "overwrite"
                }),
            )
            .build(),
    )
}

/// Overwrite guard — snapshot recorded; retry or switch to patch.
pub fn write_file_overwrite_guard(path: String, preview: String, truncated: bool) -> ToolError {
    let trunc_note = if truncated {
        "\n[Preview truncated — read_file returns full content when needed.]"
    } else {
        ""
    };
    ToolError::InvalidArgs {
        tool: "write_file".into(),
        message: format!(
            "'{path}' already exists and requires an explicit overwrite decision.\n\
             Snapshot recorded for freshness.\n\
             --- preview ---\n{preview}{trunc_note}\n---"
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Existing file requires overwrite confirmation")
            .suggestion(
                RecoveryAction::RetrySameCall,
                json!({
                    "tool": "write_file",
                    "path": path.clone(),
                    "if_exists": "overwrite",
                    "note": "read snapshot already recorded — retry same call without read_file"
                }),
            )
            .suggestion(
                RecoveryAction::SwitchTool,
                json!({
                    "from_tool": "write_file",
                    "to_tool": "patch",
                    "path": path,
                    "reason": "targeted edits are more token-efficient than full overwrite"
                }),
            )
            .build(),
    )
}

/// File changed since last read — stale cached context guard.
pub fn stale_file_context(tool: &str, display_path: &str) -> ToolError {
    ToolError::InvalidArgs {
        tool: tool.into(),
        message: format!(
            "'{display_path}' changed since it was last read in this session. \
             Cached context may be stale."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("File modified since last read")
            .suggestion(
                RecoveryAction::CallToolFirst,
                json!({
                    "tool": "read_file",
                    "path": display_path,
                    "then_retry": tool
                }),
            )
            .build(),
    )
}

/// Single-call mutation payload exceeds configured limit.
pub fn mutation_payload_too_large(
    tool_name: &str,
    path: &str,
    bytes: usize,
    max_bytes: usize,
    creating: bool,
) -> ToolError {
    let max_kib = max_bytes / 1024;
    ToolError::InvalidArgs {
        tool: tool_name.into(),
        message: format!(
            "Refusing {tool_name} for '{path}' ({bytes} bytes > {max_bytes} bytes / {max_kib} KiB). \
             Payload exceeds the per-call mutation limit."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Mutation payload too large for one tool call")
            .suggestion(
                RecoveryAction::SplitPayload,
                json!({
                    "tool": tool_name,
                    "path": path,
                    "max_bytes": max_bytes,
                    "strategy": if creating {
                        "write minimal scaffold with write_file, then grow with patch/apply_patch"
                    } else {
                        "split into smaller focused patch/apply_patch steps"
                    }
                }),
            )
            .suggestion(
                RecoveryAction::SwitchTool,
                json!({
                    "from_tool": tool_name,
                    "to_tool": "patch",
                    "path": path
                }),
            )
            .build(),
    )
}

/// TOCTOU content mismatch during write.
pub fn write_file_content_mismatch(path: String) -> ToolError {
    ToolError::ContentMismatch {
        tool: "write_file".into(),
        path: path.clone(),
        message: format!(
            "'{path}' changed on disk while the write was being prepared. \
             Re-read the current file before mutating."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("File changed during write (TOCTOU)")
            .suggestion(
                RecoveryAction::CallToolFirst,
                json!({
                    "tool": "read_file",
                    "path": path,
                    "then_retry": "write_file"
                }),
            )
            .build(),
    )
}

/// Tool-call JSON exceeded the derived one-completion budget (pre-dispatch guard).
pub fn tool_argument_budget_exceeded(
    tool_name: &str,
    argument_bytes: usize,
    max_bytes: usize,
    estimated_tokens: usize,
) -> ToolError {
    let max_kib = max_bytes / 1024;
    ToolError::InvalidArgs {
        tool: tool_name.into(),
        message: format!(
            "Refusing {tool_name}: argument payload is {argument_bytes} bytes (~{estimated_tokens} tokens) \
             but the one-completion budget is {max_bytes} bytes ({max_kib} KiB). \
             Split into scaffold + patch steps."
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("Tool argument too large for one completion")
            .suggestion(
                RecoveryAction::SplitPayload,
                json!({
                    "tool": tool_name,
                    "max_bytes": max_bytes,
                    "strategy": "write minimal scaffold, then patch/apply_patch in ≤{max_kib} KiB chunks"
                }),
            )
            .suggestion(
                RecoveryAction::SwitchTool,
                json!({
                    "from_tool": tool_name,
                    "to_tool": "patch",
                    "reason": "incremental edits fit local provider completion budgets"
                }),
            )
            .build(),
    )
}

/// write_file called without a resolvable `path` (Hermes #19096 parity).
pub fn write_file_missing_path() -> ToolError {
    ToolError::InvalidArgs {
        tool: "write_file".into(),
        message: "write_file: missing required field 'path'. Re-emit the tool call with \
                  both 'path' and 'content' set."
            .into(),
    }
    .with_recovery(
        recovery_guidance()
            .message("write_file missing path")
            .suggestion(
                RecoveryAction::SetParameter,
                json!({
                    "tool": "write_file",
                    "required": ["path", "content"],
                    "note": "use path not file_path; aliases are normalized at dispatch"
                }),
            )
            .build(),
    )
}

/// write_file called with path but no `content` key (dropped-arg under context pressure).
pub fn write_file_missing_content(max_argument_bytes: Option<usize>) -> ToolError {
    let budget_hint = max_argument_bytes.map_or(String::new(), |max| {
        format!(
            " One-completion argument budget is ~{max} bytes — split very large files \
             into scaffold + patch steps."
        )
    });
    ToolError::InvalidArgs {
        tool: "write_file".into(),
        message: format!(
            "write_file: missing required field 'content'. The tool call included a path \
             but no content argument — this is almost always a dropped-arg bug under \
             context pressure. Re-emit the tool call with the full content payload, or use \
             execute_code for very large files.{budget_hint}"
        ),
    }
    .with_recovery(
        recovery_guidance()
            .message("write_file missing content")
            .suggestion(
                RecoveryAction::RetrySameCall,
                json!({
                    "tool": "write_file",
                    "required": ["path", "content"],
                    "strategy": "re-emit full payload or write scaffold then patch"
                }),
            )
            .build(),
    )
}

/// Unknown / hallucinated tool name after repair (Hermes conversation_loop parity + structured recovery).
pub fn unknown_tool(
    invalid_name: &str,
    suggestion: Option<&str>,
    sample_tools: &[String],
) -> ToolError {
    let suggest = suggestion
        .map(|s| format!(" Did you mean '{s}'?"))
        .unwrap_or_default();
    let sample = if sample_tools.is_empty() {
        "(no tools registered)".to_string()
    } else {
        sample_tools.join(", ")
    };
    ToolError::NotFound(format!(
        "Tool '{invalid_name}' does not exist.{suggest} \
         Available tools include: {sample}. Use exact snake_case names from the tool schema."
    ))
    .with_recovery(
        recovery_guidance()
            .message("Unknown tool name")
            .suggestion(
                RecoveryAction::RetrySameCall,
                json!({
                    "invalid_name": invalid_name,
                    "suggested_name": suggestion,
                    "sample_tools": sample_tools,
                }),
            )
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_types::RecoveryAction;

    #[test]
    fn write_file_abort_includes_structured_recovery() {
        let err = write_file_path_exists_abort("src/main.rs".into(), 128);
        let payload = err.to_llm_payload();
        assert!(payload.error.contains("already exists"));
        let recovery = payload.recovery_feedback.expect("recovery attached");
        assert_eq!(recovery.feedback_type, "recovery_guidance");
        assert!(recovery.suggestions.len() >= 2);
        assert_eq!(
            recovery.suggestions[0].action,
            RecoveryAction::UseDifferentPath
        );
    }

    #[test]
    fn stale_context_suggests_read_file_first() {
        let err = stale_file_context("write_file", "lib.rs");
        let recovery = err
            .to_llm_payload()
            .recovery_feedback
            .expect("recovery");
        assert!(
            recovery
                .suggestions
                .iter()
                .any(|s| s.action == RecoveryAction::CallToolFirst)
        );
    }
}
