use std::path::Path;

use edgecrab_types::ToolError;

/// Default per-call ceiling for text-mutation payloads.
///
/// WHY: provider tool-calling transports must carry the entire argument object in
/// one JSON payload. Once a single edit payload grows too large, the dominant
/// failure mode is malformed or truncated arguments rather than successful file
/// mutation. A shared byte cap keeps the contract deterministic across write and
/// patch style tools.
///
/// FP16: "Defaults protect, overrides empower" — the default is safe for most
/// providers (Anthropic, OpenAI, Google). Power users can override via
/// `tools.max_write_payload_kib` in config.yaml or `EDGECRAB_MAX_WRITE_PAYLOAD_KIB`.
pub const DEFAULT_MAX_MUTATION_PAYLOAD_BYTES: usize = 32 * 1024;
pub const DEFAULT_MAX_MUTATION_PAYLOAD_KIB: usize = DEFAULT_MAX_MUTATION_PAYLOAD_BYTES / 1024;

/// Safety floor — prevent accidental 0 or tiny values.
pub const MIN_MUTATION_PAYLOAD_BYTES: usize = 8 * 1024;
/// Safety ceiling — prevent obviously broken values (256 KiB).
pub const MAX_MUTATION_PAYLOAD_BYTES_CAP: usize = 256 * 1024;

/// Backward-compatible re-exports for callers that haven't migrated to
/// the configurable API yet.
pub const MAX_MUTATION_PAYLOAD_BYTES: usize = DEFAULT_MAX_MUTATION_PAYLOAD_BYTES;
pub const MAX_MUTATION_PAYLOAD_KIB: usize = DEFAULT_MAX_MUTATION_PAYLOAD_KIB;

/// Clamp a user-supplied KiB value to the [MIN, CAP] range.
pub fn clamp_write_limit_bytes(kib: usize) -> usize {
    let bytes = kib.saturating_mul(1024);
    bytes.clamp(MIN_MUTATION_PAYLOAD_BYTES, MAX_MUTATION_PAYLOAD_BYTES_CAP)
}

pub fn enforce_write_payload_limit(
    tool_name: &str,
    path: &str,
    resolved: &Path,
    content: &str,
) -> Result<(), ToolError> {
    enforce_write_payload_limit_with_max(
        tool_name,
        path,
        resolved,
        content,
        DEFAULT_MAX_MUTATION_PAYLOAD_BYTES,
    )
}

pub fn enforce_write_payload_limit_with_max(
    tool_name: &str,
    path: &str,
    resolved: &Path,
    content: &str,
    max_bytes: usize,
) -> Result<(), ToolError> {
    let bytes = content.len();
    if bytes <= max_bytes {
        return Ok(());
    }

    let target_kind = if resolved.exists() {
        "overwrite"
    } else {
        "creation"
    };

    let max_kib = max_bytes / 1024;
    Err(ToolError::Other(format!(
        "Refusing {target_kind} via {tool_name} for '{path}' ({bytes} bytes > {max_bytes} bytes / {max_kib} KiB). \
         Large single-call mutation payloads are unreliable because the model must emit \
         the entire payload in one tool call. Create a small scaffold first, then grow \
         it with focused patch/apply_patch steps."
    )))
}

pub fn enforce_patch_payload_limit(
    tool_name: &str,
    path: &str,
    payload_bytes: usize,
) -> Result<(), ToolError> {
    enforce_patch_payload_limit_with_max(
        tool_name,
        path,
        payload_bytes,
        DEFAULT_MAX_MUTATION_PAYLOAD_BYTES,
    )
}

pub fn enforce_patch_payload_limit_with_max(
    tool_name: &str,
    path: &str,
    payload_bytes: usize,
    max_bytes: usize,
) -> Result<(), ToolError> {
    if payload_bytes <= max_bytes {
        return Ok(());
    }

    let max_kib = max_bytes / 1024;
    Err(ToolError::Other(format!(
        "Refusing {tool_name} for '{path}' ({payload_bytes} bytes > {max_bytes} bytes / {max_kib} KiB). \
         Large single-call edit payloads are unreliable. Split the change into \
         smaller focused patches."
    )))
}

pub fn enforce_apply_patch_payload_limit(patch_text: &str) -> Result<(), ToolError> {
    enforce_apply_patch_payload_limit_with_max(patch_text, DEFAULT_MAX_MUTATION_PAYLOAD_BYTES)
}

pub fn enforce_apply_patch_payload_limit_with_max(
    patch_text: &str,
    max_bytes: usize,
) -> Result<(), ToolError> {
    let bytes = patch_text.len();
    if bytes <= max_bytes {
        return Ok(());
    }

    let max_kib = max_bytes / 1024;
    Err(ToolError::Other(format!(
        "Refusing apply_patch payload ({bytes} bytes > {max_bytes} bytes / {max_kib} KiB). \
         Split the refactor into multiple focused apply_patch calls so each tool \
         argument stays within the mutation contract."
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_write_limit_bytes() {
        // Below minimum — clamped up
        assert_eq!(clamp_write_limit_bytes(4), MIN_MUTATION_PAYLOAD_BYTES);
        // Normal — unchanged
        assert_eq!(clamp_write_limit_bytes(32), 32 * 1024);
        assert_eq!(clamp_write_limit_bytes(64), 64 * 1024);
        // Above cap — clamped down
        assert_eq!(clamp_write_limit_bytes(512), MAX_MUTATION_PAYLOAD_BYTES_CAP);
        // Zero — clamped to minimum
        assert_eq!(clamp_write_limit_bytes(0), MIN_MUTATION_PAYLOAD_BYTES);
    }

    #[test]
    fn test_enforce_write_limit_configurable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "existing").unwrap();

        // 16 KiB limit, 10 KiB content → OK
        let content = "x".repeat(10 * 1024);
        assert!(
            enforce_write_payload_limit_with_max(
                "write_file",
                "test.txt",
                &path,
                &content,
                16 * 1024,
            )
            .is_ok()
        );

        // 16 KiB limit, 20 KiB content → rejected
        let big = "x".repeat(20 * 1024);
        let err =
            enforce_write_payload_limit_with_max("write_file", "test.txt", &path, &big, 16 * 1024);
        assert!(err.is_err());
        let msg = format!("{}", err.unwrap_err());
        assert!(msg.contains("16 KiB"));
    }

    #[test]
    fn test_enforce_patch_limit_configurable() {
        assert!(enforce_patch_payload_limit_with_max("patch", "f.rs", 10_000, 16 * 1024).is_ok());
        assert!(enforce_patch_payload_limit_with_max("patch", "f.rs", 20_000, 16 * 1024).is_err());
    }

    #[test]
    fn test_enforce_apply_patch_limit_configurable() {
        let small = "x".repeat(10_000);
        assert!(enforce_apply_patch_payload_limit_with_max(&small, 16 * 1024).is_ok());
        let big = "x".repeat(20_000);
        assert!(enforce_apply_patch_payload_limit_with_max(&big, 16 * 1024).is_err());
    }
}
