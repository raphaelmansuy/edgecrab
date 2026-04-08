use std::path::Path;

use edgecrab_types::ToolError;

/// Hard per-call ceiling for text-mutation payloads.
///
/// WHY: provider tool-calling transports must carry the entire argument object in
/// one JSON payload. Once a single edit payload grows too large, the dominant
/// failure mode is malformed or truncated arguments rather than successful file
/// mutation. A shared byte cap keeps the contract deterministic across write and
/// patch style tools.
pub const MAX_MUTATION_PAYLOAD_BYTES: usize = 32 * 1024;
pub const MAX_MUTATION_PAYLOAD_KIB: usize = MAX_MUTATION_PAYLOAD_BYTES / 1024;

pub fn enforce_write_payload_limit(
    tool_name: &str,
    path: &str,
    resolved: &Path,
    content: &str,
) -> Result<(), ToolError> {
    let bytes = content.len();
    if bytes <= MAX_MUTATION_PAYLOAD_BYTES {
        return Ok(());
    }

    let target_kind = if resolved.exists() {
        "overwrite"
    } else {
        "creation"
    };

    Err(ToolError::Other(format!(
        "Refusing {target_kind} via {tool_name} for '{path}' ({bytes} bytes > {} bytes). \
         Large single-call mutation payloads are unreliable because the model must emit \
         the entire payload in one tool call. Create a small scaffold first, then grow \
         it with focused patch/apply_patch steps.",
        MAX_MUTATION_PAYLOAD_BYTES
    )))
}

pub fn enforce_patch_payload_limit(
    tool_name: &str,
    path: &str,
    payload_bytes: usize,
) -> Result<(), ToolError> {
    if payload_bytes <= MAX_MUTATION_PAYLOAD_BYTES {
        return Ok(());
    }

    Err(ToolError::Other(format!(
        "Refusing {tool_name} for '{path}' ({payload_bytes} bytes > {} bytes). \
         Large single-call edit payloads are unreliable. Split the change into \
         smaller focused patches.",
        MAX_MUTATION_PAYLOAD_BYTES
    )))
}

pub fn enforce_apply_patch_payload_limit(patch_text: &str) -> Result<(), ToolError> {
    let bytes = patch_text.len();
    if bytes <= MAX_MUTATION_PAYLOAD_BYTES {
        return Ok(());
    }

    Err(ToolError::Other(format!(
        "Refusing apply_patch payload ({bytes} bytes > {} bytes). \
         Split the refactor into multiple focused apply_patch calls so each tool \
         argument stays within the mutation contract.",
        MAX_MUTATION_PAYLOAD_BYTES
    )))
}
